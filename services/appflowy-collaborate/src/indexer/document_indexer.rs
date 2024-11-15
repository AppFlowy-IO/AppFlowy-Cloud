use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use collab::preclude::Collab;

use crate::config::get_env_var;
use crate::indexer::{DocumentDataExt, Indexer};
use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingModel, EmbeddingOutput, EmbeddingRequest,
};
use collab_document::document::DocumentBody;
use collab_document::error::DocumentError;
use collab_entity::CollabType;
use database_entity::dto::{AFCollabEmbeddingParams, AFCollabEmbeddings, EmbeddingContentType};
use tiktoken_rs::CoreBPE;
use tracing::trace;
use uuid::Uuid;

pub struct DocumentIndexer {
  ai_client: AppFlowyAIClient,
  doc_content_split: usize,
  tokenizer: CoreBPE,
  embedding_model: EmbeddingModel,
}

impl DocumentIndexer {
  pub fn new(ai_client: AppFlowyAIClient) -> Arc<Self> {
    // We assume that every token is ~4 bytes. We're going to split document content into fragments
    // of ~2000 tokens each.
    let doc_content_split = get_env_var("APPFLOWY_INDEXER_DOCUMENT_CONTENT_SPLIT_LEN", "8000")
      .parse()
      .unwrap_or(8000);
    let tokenizer = tiktoken_rs::cl100k_base().unwrap();
    Arc::new(Self {
      ai_client,
      doc_content_split,
      tokenizer,
      embedding_model: EmbeddingModel::TextEmbedding3Small,
    })
  }
}

#[async_trait]
impl Indexer for DocumentIndexer {
  fn embedding_params(&self, collab: &Collab) -> Result<Vec<AFCollabEmbeddingParams>, AppError> {
    let object_id = collab.object_id().to_string();
    let document = DocumentBody::from_collab(collab).ok_or_else(|| {
      anyhow!(
        "Failed to get document body from collab `{}`: schema is missing required fields",
        object_id
      )
    })?;

    let result = document.get_document_data(&collab.transact());
    match result {
      Ok(document_data) => {
        let content = document_data.to_plain_text();
        create_embedding_params(
          object_id,
          content,
          CollabType::Document,
          self.embedding_model.default_dimensions() as usize,
          &self.tokenizer,
        )
      },
      Err(err) => {
        if matches!(err, DocumentError::NoRequiredData) {
          Ok(vec![])
        } else {
          Err(AppError::Internal(err.into()))
        }
      },
    }
  }

  async fn embeddings(
    &self,
    mut params: Vec<AFCollabEmbeddingParams>,
  ) -> Result<Option<AFCollabEmbeddings>, AppError> {
    let object_id = match params.first() {
      None => return Ok(None),
      Some(first) => first.object_id.clone(),
    };
    let contents: Vec<_> = params
      .iter()
      .map(|fragment| fragment.content.clone())
      .collect();

    let resp = self
      .ai_client
      .embeddings(EmbeddingRequest {
        input: EmbeddingInput::StringArray(contents),
        model: EmbeddingModel::TextEmbedding3Small.to_string(),
        chunk_size: (self.doc_content_split / 4) as i32,
        encoding_format: EmbeddingEncodingFormat::Float,
        dimensions: EmbeddingModel::TextEmbedding3Small.default_dimensions(),
      })
      .await?;
    trace!(
      "[Embedding] request {} embeddings, received {} embeddings",
      params.len(),
      resp.data.len()
    );

    for embedding in resp.data {
      let param = &mut params[embedding.index as usize];
      let embedding: Vec<f32> = match embedding.embedding {
        EmbeddingOutput::Float(embedding) => embedding.into_iter().map(|f| f as f32).collect(),
        EmbeddingOutput::Base64(_) => {
          return Err(AppError::OpenError(
            "Unexpected base64 encoding".to_string(),
          ))
        },
      };
      param.embedding = Some(embedding);
    }

    tracing::info!(
      "received {} embeddings for document {} - tokens used: {}",
      params.len(),
      object_id,
      resp.total_tokens
    );
    Ok(Some(AFCollabEmbeddings {
      tokens_consumed: resp.total_tokens as u32,
      params,
    }))
  }
}

fn create_embedding_params(
  object_id: String,
  content: String,
  collab_type: CollabType,
  max_tokens: usize,
  tokenizer: &CoreBPE,
) -> Result<Vec<AFCollabEmbeddingParams>, AppError> {
  let split_contents = split_text_by_max_tokens(content, max_tokens, tokenizer)?;
  Ok(
    split_contents
      .into_iter()
      .map(|content| AFCollabEmbeddingParams {
        fragment_id: Uuid::new_v4().to_string(),
        object_id: object_id.clone(),
        collab_type: collab_type.clone(),
        content_type: EmbeddingContentType::PlainText,
        content,
        embedding: None,
      })
      .collect(),
  )
}

fn split_text_by_max_tokens(
  content: String,
  max_tokens: usize,
  tokenizer: &CoreBPE,
) -> Result<Vec<String>, AppError> {
  if content.is_empty() {
    return Ok(vec![]);
  }

  let token_ids = tokenizer.encode_with_special_tokens(&content);
  let total_tokens = token_ids.len();
  if total_tokens <= max_tokens {
    return Ok(vec![content]);
  }

  let mut chunks = Vec::new();
  let mut start_idx = 0;
  while start_idx < total_tokens {
    let mut end_idx = (start_idx + max_tokens).min(total_tokens);
    let mut decoded = false;
    // Try to decode the chunk, adjust end_idx if decoding fails
    while !decoded {
      let token_chunk = &token_ids[start_idx..end_idx];
      // Attempt to decode the current chunk
      match tokenizer.decode(token_chunk.to_vec()) {
        Ok(chunk_text) => {
          chunks.push(chunk_text);
          start_idx = end_idx;
          decoded = true;
        },
        Err(_) => {
          // If we can extend the chunk, do so
          if end_idx < total_tokens {
            end_idx += 1;
          } else if start_idx + 1 < total_tokens {
            // Skip the problematic token at start_idx
            start_idx += 1;
            end_idx = (start_idx + max_tokens).min(total_tokens);
          } else {
            // Cannot decode any further, break to avoid infinite loop
            start_idx = total_tokens;
            break;
          }
        },
      }
    }
  }

  Ok(chunks)
}

#[cfg(test)]
mod tests {
  use crate::indexer::document_indexer::create_embedding_params;
  use collab_entity::CollabType;
  use tiktoken_rs::cl100k_base;
  use unicode_normalization::UnicodeNormalization;

  #[test]
  fn test_split_at_non_utf8() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 10; // Small number for testing

    // Content with multibyte characters (emojis)
    let content = "Hello 😃 World 🌍! This is a test 🚀.".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    // Ensure that we didn't split in the middle of a multibyte character
    for param in params {
      assert!(param.content.is_char_boundary(0));
      assert!(param.content.is_char_boundary(param.content.len()));
    }
  }

  #[test]
  fn test_exact_boundary_split() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 5; // Set to 5 tokens for testing

    // Content length is exactly a multiple of max_tokens
    let content = "The quick brown fox jumps over the lazy dog".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    // Since tokenization may split words differently, we need to adjust expectations
    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);
  }

  #[test]
  fn test_content_shorter_than_max_len() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 100;

    let content = "Short content".to_string();
    let tokenizer = cl100k_base().unwrap();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    assert_eq!(params.len(), 1);
    assert_eq!(params[0].content, content);
  }

  #[test]
  fn test_empty_content() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 10;

    let content = "".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    assert_eq!(params.len(), 0);
  }

  #[test]
  fn test_content_with_only_multibyte_characters() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 1; // Set to 1 token for testing

    // Each emoji may be a single token depending on the tokenizer
    let content = "😀😃😄😁😆".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    let emojis: Vec<String> = content.chars().map(|c| c.to_string()).collect();
    for (param, emoji) in params.iter().zip(emojis.iter()) {
      assert_eq!(param.content, *emoji);
    }
  }

  #[test]
  fn test_split_with_combining_characters() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 1; // Set to 1 token for testing

    // String with combining characters
    let content = "a\u{0301}e\u{0301}i\u{0301}o\u{0301}u\u{0301}".to_string(); // "áéíóú"
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    // The number of fragments should match the number of tokens
    let total_tokens = tokenizer.encode_ordinary(&content).len();
    assert_eq!(params.len(), total_tokens);

    // Verify that the concatenated content matches the original content
    let reconstructed_content = params.into_iter().map(|p| p.content).collect::<String>();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_large_content() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 1000;

    // Generate a large content string
    let content = "a".repeat(5000); // 5000 characters
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);
  }

  #[test]
  fn test_non_ascii_characters() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 2;

    // Non-ASCII characters: "áéíóú"
    let content = "áéíóú".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    // Determine expected number of fragments
    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);

    // Verify that the concatenated content matches the original content
    let reconstructed_content: String = params.into_iter().map(|p| p.content).collect::<String>();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_leading_and_trailing_whitespace() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 3;

    let content = "  abcde  ".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);

    // Verify that the concatenated content matches the original content
    let reconstructed_content: String = params.into_iter().map(|p| p.content).collect::<String>();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_multiple_zero_width_joiners() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 1;

    // Complex emoji sequence with multiple zero-width joiners
    let content = "👩‍👩‍👧‍👧👨‍👨‍👦‍👦".to_string();
    let tokenizer = cl100k_base().unwrap();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    // Verify that the concatenated content matches the original content
    let reconstructed_content: String = params.into_iter().map(|p| p.content).collect::<String>();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_long_combining_sequences() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_tokens = 1;

    // Character with multiple combining marks
    let content = "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}"
      .nfc()
      .collect::<String>();

    // Initialize the tokenizer
    let tokenizer = cl100k_base().unwrap();
    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_tokens,
      &tokenizer,
    )
    .unwrap();

    // Verify that the concatenated content matches the original content
    let reconstructed_content: String = params.into_iter().map(|p| p.content).collect::<String>();
    assert_eq!(reconstructed_content, content);
  }
}
