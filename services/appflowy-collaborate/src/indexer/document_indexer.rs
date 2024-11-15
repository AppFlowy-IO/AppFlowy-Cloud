use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use collab::preclude::Collab;

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
  tokenizer: Arc<CoreBPE>,
  embedding_model: EmbeddingModel,
}

impl DocumentIndexer {
  pub fn new(ai_client: AppFlowyAIClient) -> Arc<Self> {
    let tokenizer = tiktoken_rs::cl100k_base().unwrap();
    Arc::new(Self {
      ai_client,
      tokenizer: Arc::new(tokenizer),
      embedding_model: EmbeddingModel::TextEmbedding3Small,
    })
  }
}

#[async_trait]
impl Indexer for DocumentIndexer {
  async fn embedding_params(
    &self,
    collab: &Collab,
  ) -> Result<Vec<AFCollabEmbeddingParams>, AppError> {
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
        let max_tokens = self.embedding_model.default_dimensions() as usize;
        create_embedding(
          object_id,
          content,
          CollabType::Document,
          max_tokens,
          self.tokenizer.clone(),
        )
        .await
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
        chunk_size: 2000,
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

async fn create_embedding(
  object_id: String,
  content: String,
  collab_type: CollabType,
  max_tokens: usize,
  tokenizer: Arc<CoreBPE>,
) -> Result<Vec<AFCollabEmbeddingParams>, AppError> {
  // Running execution_time_comparison_tests, we got the following results:
  // Content Size: 500 | Direct Time: 1ms | spawn_blocking Time: 1ms
  // Content Size: 1000 | Direct Time: 2ms | spawn_blocking Time: 2ms
  // Content Size: 2000 | Direct Time: 5ms | spawn_blocking Time: 5ms
  // Content Size: 5000 | Direct Time: 11ms | spawn_blocking Time: 11ms
  // Content Size: 20000 | Direct Time: 49ms | spawn_blocking Time: 48ms
  let split_contents = if content.len() < 500 {
    split_text_by_max_tokens(content, max_tokens, tokenizer.as_ref())?
  } else {
    tokio::task::spawn_blocking(move || {
      split_text_by_max_tokens(content, max_tokens, tokenizer.as_ref())
    })
    .await??
  };

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

  let token_ids = tokenizer.encode_ordinary(&content);
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
  use crate::indexer::document_indexer::split_text_by_max_tokens;

  use tiktoken_rs::cl100k_base;

  #[test]
  fn test_split_at_non_utf8() {
    let max_tokens = 10; // Small number for testing

    // Content with multibyte characters (emojis)
    let content = "Hello 😃 World 🌍! This is a test 🚀.".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    // Ensure that we didn't split in the middle of a multibyte character
    for content in params {
      assert!(content.is_char_boundary(0));
      assert!(content.is_char_boundary(content.len()));
    }
  }
  #[test]
  fn test_exact_boundary_split() {
    let max_tokens = 5; // Set to 5 tokens for testing
    let content = "The quick brown fox jumps over the lazy dog".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);
  }

  #[test]
  fn test_content_shorter_than_max_len() {
    let max_tokens = 100;
    let content = "Short content".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    assert_eq!(params.len(), 1);
    assert_eq!(params[0], content);
  }

  #[test]
  fn test_empty_content() {
    let max_tokens = 10;
    let content = "".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    assert_eq!(params.len(), 0);
  }

  #[test]
  fn test_content_with_only_multibyte_characters() {
    let max_tokens = 1; // Set to 1 token for testing
    let content = "😀😃😄😁😆".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let emojis: Vec<String> = content.chars().map(|c| c.to_string()).collect();
    for (param, emoji) in params.iter().zip(emojis.iter()) {
      assert_eq!(param, emoji);
    }
  }

  #[test]
  fn test_split_with_combining_characters() {
    let max_tokens = 1; // Set to 1 token for testing
    let content = "a\u{0301}e\u{0301}i\u{0301}o\u{0301}u\u{0301}".to_string(); // "áéíóú"
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    assert_eq!(params.len(), total_tokens);

    let reconstructed_content = params.join("");
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_large_content() {
    let max_tokens = 1000;
    let content = "a".repeat(5000); // 5000 characters
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);
  }

  #[test]
  fn test_non_ascii_characters() {
    let max_tokens = 2;
    let content = "áéíóú".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);

    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_leading_and_trailing_whitespace() {
    let max_tokens = 3;
    let content = "  abcde  ".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let total_tokens = tokenizer.encode_ordinary(&content).len();
    let expected_fragments = (total_tokens + max_tokens - 1) / max_tokens;
    assert_eq!(params.len(), expected_fragments);

    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_multiple_zero_width_joiners() {
    let max_tokens = 1;
    let content = "👩‍👩‍👧‍👧👨‍👨‍👦‍👦".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }

  #[test]
  fn test_content_with_long_combining_sequences() {
    let max_tokens = 1;
    let content = "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}".to_string();
    let tokenizer = cl100k_base().unwrap();
    let params = split_text_by_max_tokens(content.clone(), max_tokens, &tokenizer).unwrap();

    let reconstructed_content: String = params.concat();
    assert_eq!(reconstructed_content, content);
  }
}

// #[cfg(test)]
// mod execution_time_comparison_tests {
//   use crate::indexer::document_indexer::split_text_by_max_tokens;
//   use rand::distributions::Alphanumeric;
//   use rand::{thread_rng, Rng};
//   use std::sync::Arc;
//   use std::time::Instant;
//   use tiktoken_rs::{cl100k_base, CoreBPE};
//
//   #[tokio::test]
//   async fn test_execution_time_comparison() {
//     let tokenizer = Arc::new(cl100k_base().unwrap());
//     let max_tokens = 100;
//
//     let sizes = vec![500, 1000, 2000, 5000, 20000]; // Content sizes to test
//     for size in sizes {
//       let content = generate_random_string(size);
//
//       // Measure direct execution time
//       let direct_time = measure_direct_execution(content.clone(), max_tokens, &tokenizer);
//
//       // Measure spawn_blocking execution time
//       let spawn_blocking_time =
//         measure_spawn_blocking_execution(content, max_tokens, Arc::clone(&tokenizer)).await;
//
//       println!(
//         "Content Size: {} | Direct Time: {}ms | spawn_blocking Time: {}ms",
//         size, direct_time, spawn_blocking_time
//       );
//     }
//   }
//
//   // Measure direct execution time
//   fn measure_direct_execution(content: String, max_tokens: usize, tokenizer: &CoreBPE) -> u128 {
//     let start = Instant::now();
//     split_text_by_max_tokens(content, max_tokens, tokenizer).unwrap();
//     start.elapsed().as_millis()
//   }
//
//   // Measure `spawn_blocking` execution time
//   async fn measure_spawn_blocking_execution(
//     content: String,
//     max_tokens: usize,
//     tokenizer: Arc<CoreBPE>,
//   ) -> u128 {
//     let start = Instant::now();
//     tokio::task::spawn_blocking(move || {
//       split_text_by_max_tokens(content, max_tokens, tokenizer.as_ref()).unwrap()
//     })
//     .await
//     .unwrap();
//     start.elapsed().as_millis()
//   }
//
//   pub fn generate_random_string(len: usize) -> String {
//     let rng = thread_rng();
//     rng
//       .sample_iter(&Alphanumeric)
//       .take(len)
//       .map(char::from)
//       .collect()
//   }
// }
