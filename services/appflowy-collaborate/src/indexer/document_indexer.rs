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
        let mut result = Vec::with_capacity(1 + content.len() / Self::DOC_CONTENT_SPLIT);

        let mut slice = content.as_str();
        while slice.len() > Self::DOC_CONTENT_SPLIT {
          // we should split document into multiple fragments
          let (left, right) = slice.split_at(Self::DOC_CONTENT_SPLIT);
          let param = AFCollabEmbeddingParams {
            fragment_id: Uuid::new_v4().to_string(),
            object_id: object_id.clone(),
            collab_type: CollabType::Document,
            content_type: EmbeddingContentType::PlainText,
            content: left.to_string(),
            embedding: None,
          };
          result.push(param);
          slice = right;
        }

        let content = if slice.len() == content.len() {
          content // we didn't slice the content
        } else {
          slice.to_string()
        };
        if !content.is_empty() {
          let param = AFCollabEmbeddingParams {
            fragment_id: object_id.clone(),
            object_id: object_id.clone(),
            collab_type: CollabType::Document,
            content_type: EmbeddingContentType::PlainText,
            content,
            embedding: None,
          };
          result.push(param);
        }

        Ok(result)
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
