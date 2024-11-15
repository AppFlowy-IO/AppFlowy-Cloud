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
use tracing::trace;
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

pub struct DocumentIndexer {
  ai_client: AppFlowyAIClient,
  doc_content_split: usize,
}

impl DocumentIndexer {
  pub fn new(ai_client: AppFlowyAIClient) -> Arc<Self> {
    // We assume that every token is ~4 bytes. We're going to split document content into fragments
    // of ~2000 tokens each.
    let doc_content_split = get_env_var("APPFLOWY_INDEXER_DOCUMENT_CONTENT_SPLIT_LEN", "8000")
      .parse()
      .unwrap_or(8000);
    Arc::new(Self {
      ai_client,
      doc_content_split,
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
          self.doc_content_split,
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
#[inline]
fn create_embedding_params(
  object_id: String,
  content: String,
  collab_type: CollabType,
  max_content_len: usize,
) -> Result<Vec<AFCollabEmbeddingParams>, AppError> {
  if content.is_empty() {
    return Ok(vec![]);
  }

  // Helper function to create AFCollabEmbeddingParams
  fn create_param(
    fragment_id: String,
    object_id: &str,
    collab_type: &CollabType,
    content: String,
  ) -> AFCollabEmbeddingParams {
    AFCollabEmbeddingParams {
      fragment_id,
      object_id: object_id.to_string(),
      collab_type: collab_type.clone(),
      content_type: EmbeddingContentType::PlainText,
      content,
      embedding: None,
    }
  }

  if content.len() <= max_content_len {
    // Content is short enough; return as a single fragment
    let param = create_param(object_id.clone(), &object_id, &collab_type, content);
    return Ok(vec![param]);
  }

  // Content is longer than max_content_len; need to split
  let mut result = Vec::with_capacity(1 + content.len() / max_content_len);
  let mut fragment = String::with_capacity(max_content_len);
  let mut current_len = 0;

  for grapheme in content.graphemes(true) {
    let grapheme_len = grapheme.len();
    if current_len + grapheme_len > max_content_len {
      if !fragment.is_empty() {
        // Move the fragment to avoid cloning
        result.push(create_param(
          Uuid::new_v4().to_string(),
          &object_id,
          &collab_type,
          std::mem::take(&mut fragment),
        ));
      }
      current_len = 0;

      // Check if the grapheme itself is longer than max_content_len
      if grapheme_len > max_content_len {
        // Push the grapheme as a fragment on its own
        result.push(create_param(
          Uuid::new_v4().to_string(),
          &object_id,
          &collab_type,
          grapheme.to_string(),
        ));
        continue;
      }
    }
    fragment.push_str(grapheme);
    current_len += grapheme_len;
  }

  // Add the last fragment if it's not empty
  if !fragment.is_empty() {
    result.push(create_param(
      object_id.clone(),
      &object_id,
      &collab_type,
      fragment,
    ));
  }

  Ok(result)
}
#[cfg(test)]
mod tests {
  use crate::indexer::document_indexer::create_embedding_params;
  use collab_entity::CollabType;

  #[test]
  fn test_split_at_non_utf8() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 10; // Small number for testing

    // Content with multibyte characters (emojis)
    let content = "Hello 😃 World 🌍! This is a test 🚀.".to_string();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
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
    let max_content_len = 5; // Set to 5 for testing

    // Content length is exactly a multiple of max_content_len
    let content = "abcdefghij".to_string(); // 10 characters

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    assert_eq!(params.len(), 2);
    assert_eq!(params[0].content, "abcde");
    assert_eq!(params[1].content, "fghij");
  }

  #[test]
  fn test_content_shorter_than_max_len() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 100;

    let content = "Short content".to_string();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    assert_eq!(params.len(), 1);
    assert_eq!(params[0].content, content);
  }

  #[test]
  fn test_empty_content() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 10;

    let content = "".to_string();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    assert_eq!(params.len(), 0);
  }

  #[test]
  fn test_content_with_only_multibyte_characters() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 4; // Small number for testing

    // Each emoji is 4 bytes in UTF-8
    let content = "😀😃😄😁😆".to_string();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    assert_eq!(params.len(), 5);
    let expected_contents = ["😀", "😃", "😄", "😁", "😆"];
    for (param, expected) in params.iter().zip(expected_contents.iter()) {
      assert_eq!(param.content, *expected);
    }
  }

  #[test]
  fn test_split_with_combining_characters() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 5; // Small number for testing

    // String with combining characters (e.g., letters with accents)
    let content = "a\u{0301}e\u{0301}i\u{0301}o\u{0301}u\u{0301}".to_string(); // "áéíóú"

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    assert_eq!(params.len(), 5);
    let expected_contents = ["á", "é", "í", "ó", "ú"];
    for (param, expected) in params.iter().zip(expected_contents.iter()) {
      assert_eq!(param.content, *expected);
    }
  }

  #[test]
  fn test_large_content() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 1000;

    // Generate a large content string
    let content = "a".repeat(5000); // 5000 characters

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    assert_eq!(params.len(), 5); // 5000 / 1000 = 5
    for param in params {
      assert_eq!(param.content.len(), 1000);
    }
  }
  #[test]
  fn test_non_ascii_characters() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 5;

    // Non-ASCII characters: "áéíóú"
    let content = "áéíóú".to_string();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    // Content should be split into two fragments
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].content, "áé");
    assert_eq!(params[1].content, "íó");
    assert_eq!(params[2].content, "ú");
  }

  #[test]
  fn test_content_with_leading_and_trailing_whitespace() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 5;

    let content = "  abcde  ".to_string();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    // Content should include leading and trailing whitespace
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].content, "  abc");
    assert_eq!(params[1].content, "de  ");
  }

  #[test]
  fn test_content_with_multiple_zero_width_joiners() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 10;

    // Complex emoji sequence with multiple zero-width joiners
    let content = "👩‍👩‍👧‍👧👨‍👨‍👦‍👦".to_string();

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    // Each complex emoji should be treated as a single grapheme
    assert_eq!(params.len(), 2);
    assert_eq!(params[0].content, "👩‍👩‍👧‍👧");
    assert_eq!(params[1].content, "👨‍👨‍👦‍👦");
  }

  #[test]
  fn test_content_with_long_combining_sequences() {
    let object_id = "test_object".to_string();
    let collab_type = CollabType::Document;
    let max_content_len = 5;

    // Character with multiple combining marks
    let content = "a\u{0300}\u{0301}\u{0302}\u{0303}\u{0304}".to_string(); // a with multiple accents

    let params = create_embedding_params(
      object_id.clone(),
      content.clone(),
      collab_type.clone(),
      max_content_len,
    )
    .unwrap();

    // The entire combining sequence should be in one fragment
    assert_eq!(params.len(), 1);
    assert_eq!(params[0].content, content);
  }
}
