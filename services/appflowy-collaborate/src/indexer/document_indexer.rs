use async_trait::async_trait;
use std::sync::Arc;

use collab::core::collab::MutexCollab;
use collab_document::document::Document;
use collab_document::error::DocumentError;
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

  fn get_document_contents(
    collab: Arc<MutexCollab>,
  ) -> Result<(String, Vec<AFCollabEmbeddingParams>), DocumentError> {
    let object_id = collab.try_lock().unwrap().object_id.clone();
    let document = Document::open(collab)?;
    let document_data = document.get_document_data()?;
    let content = document_data.to_plain_text();

    let plain_text_param = AFCollabEmbeddingParams {
      fragment_id: object_id.clone(),
      object_id: object_id.clone(),
      collab_type: CollabType::Document,
      content_type: EmbeddingContentType::PlainText,
      content,
      embedding: None,
    };

    Ok((object_id, vec![plain_text_param]))
  }
}

#[async_trait]
impl Indexer for DocumentIndexer {
  async fn index(&self, collab: MutexCollab) -> Result<AFCollabEmbeddings, AppError> {
    let (object_id, mut params) = Self::get_document_contents(Arc::new(collab))
      .map_err(|e| AppError::OpenError(e.to_string()))?;
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
    Ok(AFCollabEmbeddings {
      object_id,
      collab_type: CollabType::Document,
      tokens_consumed: resp.total_tokens as u32,
      params,
    })
  }
}
