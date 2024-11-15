use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use collab::preclude::Collab;

use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingOutput, EmbeddingRequest, EmbeddingsModel,
};
use collab_document::document::DocumentBody;
use collab_document::error::DocumentError;
use collab_entity::CollabType;
use database_entity::dto::{AFCollabEmbeddingParams, AFCollabEmbeddings, EmbeddingContentType};
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

use crate::indexer::{DocumentDataExt, Indexer};

pub struct DocumentIndexer {
  ai_client: AppFlowyAIClient,
}

impl DocumentIndexer {
  /// We assume that every token is ~4 bytes. We're going to split document content into fragments
  /// of ~2000 tokens each.
  pub const DOC_CONTENT_SPLIT: usize = 8000;
  pub fn new(ai_client: AppFlowyAIClient) -> Arc<Self> {
    Arc::new(Self { ai_client })
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
          Self::DOC_CONTENT_SPLIT,
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
        model: EmbeddingsModel::TextEmbedding3Small.to_string(),
        chunk_size: (Self::DOC_CONTENT_SPLIT / 4) as i32,
        encoding_format: EmbeddingEncodingFormat::Float,
        dimensions: 1536,
      })
      .await?;

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
  max_content_len: usize,
) -> Result<Vec<AFCollabEmbeddingParams>, AppError> {
  let mut result = Vec::with_capacity(1 + content.len() / max_content_len);

  let mut start = 0;
  let mut current_len = 0;
  let graphemes: Vec<&str> = content.graphemes(true).collect();

  for (i, grapheme) in graphemes.iter().enumerate() {
    current_len += grapheme.len();
    if current_len > max_content_len {
      let fragment = graphemes[start..i].concat();
      result.push(AFCollabEmbeddingParams {
        fragment_id: Uuid::new_v4().to_string(),
        object_id: object_id.clone(),
        collab_type: collab_type.clone(),
        content_type: EmbeddingContentType::PlainText,
        content: fragment,
        embedding: None,
      });
      start = i;
      current_len = grapheme.len();
    }
  }

  if start < graphemes.len() {
    let fragment = graphemes[start..].concat();
    if !fragment.is_empty() {
      result.push(AFCollabEmbeddingParams {
        fragment_id: object_id.clone(),
        object_id: object_id.clone(),
        collab_type,
        content_type: EmbeddingContentType::PlainText,
        content: fragment,
        embedding: None,
      });
    }
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
    let expected_contents = vec!["😀", "😃", "😄", "😁", "😆"];
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
    let expected_contents = vec!["á", "é", "í", "ó", "ú"];
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
}
