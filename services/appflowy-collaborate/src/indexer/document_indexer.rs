use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use collab::preclude::Collab;
use collab_document::document::DocumentBody;
use collab_entity::CollabType;

use app_error::AppError;
use appflowy_ai_client::client::AppFlowyAIClient;
use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingOutput, EmbeddingRequest, EmbeddingsModel,
};
use database_entity::dto::{AFCollabEmbeddingParams, AFCollabEmbeddings, EmbeddingContentType};

use crate::indexer::{DocumentDataExt, Indexer};

pub struct DocumentIndexer {
  ai_client: AppFlowyAIClient,
}

impl DocumentIndexer {
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
    let document_data = document
      .get_document_data(&collab.transact())
      .map_err(|err| {
        anyhow!(
          "Failed to get document data from collab `{}`: {}",
          object_id,
          err
        )
      })?;
    let content = document_data.to_plain_text();

    let plain_text_param = AFCollabEmbeddingParams {
      fragment_id: object_id.clone(),
      object_id: object_id.clone(),
      collab_type: CollabType::Document,
      content_type: EmbeddingContentType::PlainText,
      content,
      embedding: None,
    };

    Ok(vec![plain_text_param])
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
        chunk_size: 2000,
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
