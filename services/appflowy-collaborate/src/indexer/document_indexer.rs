use crate::indexer::open_ai::split_text_by_max_content_len;
use crate::indexer::vector::embedder::Embedder;
use crate::indexer::Indexer;
use crate::thread_pool_no_abort::ThreadPoolNoAbort;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingModel, EmbeddingOutput, EmbeddingRequest,
};
use async_trait::async_trait;
use collab::preclude::Collab;
use collab_document::document::DocumentBody;
use collab_document::error::DocumentError;
use collab_entity::CollabType;
use database_entity::dto::{AFCollabEmbeddedChunk, AFCollabEmbeddings, EmbeddingContentType};
use tracing::trace;
use uuid::Uuid;

pub struct DocumentIndexer;

#[async_trait]
impl Indexer for DocumentIndexer {
  fn create_embedded_chunks(
    &self,
    collab: &Collab,
    embedding_model: EmbeddingModel,
  ) -> Result<Vec<AFCollabEmbeddedChunk>, AppError> {
    let object_id = collab.object_id().to_string();
    let document = DocumentBody::from_collab(collab).ok_or_else(|| {
      anyhow!(
        "Failed to get document body from collab `{}`: schema is missing required fields",
        object_id
      )
    })?;

    let result = document.to_plain_text(collab.transact(), false, true);
    match result {
      Ok(content) => {
        split_text_into_chunks(object_id, content, CollabType::Document, &embedding_model)
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

  fn embed(
    &self,
    embedder: &Embedder,
    mut content: Vec<AFCollabEmbeddedChunk>,
  ) -> Result<Option<AFCollabEmbeddings>, AppError> {
    if content.is_empty() {
      return Ok(None);
    }

    let object_id = match content.first() {
      None => return Ok(None),
      Some(first) => first.object_id.clone(),
    };

    let contents: Vec<_> = content
      .iter()
      .map(|fragment| fragment.content.clone())
      .collect();
    let resp = embedder.embed(EmbeddingRequest {
      input: EmbeddingInput::StringArray(contents),
      model: embedder.model().name().to_string(),
      encoding_format: EmbeddingEncodingFormat::Float,
      dimensions: EmbeddingModel::TextEmbedding3Small.default_dimensions(),
    })?;

    trace!(
      "[Embedding] request {} embeddings, received {} embeddings",
      content.len(),
      resp.data.len()
    );

    for embedding in resp.data {
      let param = &mut content[embedding.index as usize];
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
      content.len(),
      object_id,
      resp.usage.total_tokens
    );
    Ok(Some(AFCollabEmbeddings {
      tokens_consumed: resp.usage.total_tokens as u32,
      params: content,
    }))
  }

  fn embed_in_thread_pool(
    &self,
    embedder: &Embedder,
    content: Vec<AFCollabEmbeddedChunk>,
    thread_pool: &ThreadPoolNoAbort,
  ) -> Result<Option<AFCollabEmbeddings>, AppError> {
    if content.is_empty() {
      return Ok(None);
    }

    thread_pool
      .install(|| self.embed(embedder, content))
      .map_err(|e| AppError::Unhandled(e.to_string()))?
  }
}

fn split_text_into_chunks(
  object_id: String,
  content: String,
  collab_type: CollabType,
  embedding_model: &EmbeddingModel,
) -> Result<Vec<AFCollabEmbeddedChunk>, AppError> {
  debug_assert!(matches!(
    embedding_model,
    EmbeddingModel::TextEmbedding3Small
  ));
  // We assume that every token is ~4 bytes. We're going to split document content into fragments
  // of ~2000 tokens each.
  let split_contents = split_text_by_max_content_len(content, 8000)?;
  Ok(
    split_contents
      .into_iter()
      .map(|content| AFCollabEmbeddedChunk {
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
