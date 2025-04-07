use crate::collab_indexer::Indexer;
use crate::vector::embedder::AFEmbedder;
use crate::vector::open_ai::group_paragraphs_by_max_content_len;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_ai_client::dto::EmbeddingModel;
use async_openai::types::{CreateEmbeddingRequestArgs, EmbeddingInput, EncodingFormat};
use async_trait::async_trait;
use collab::preclude::Collab;
use collab_document::document::DocumentBody;
use collab_entity::CollabType;
use database_entity::dto::{AFCollabEmbeddedChunk, AFCollabEmbeddings, EmbeddingContentType};
use serde_json::json;
use tracing::{debug, trace, warn};
use twox_hash::xxhash64::Hasher;
use uuid::Uuid;

pub struct DocumentIndexer;

#[async_trait]
impl Indexer for DocumentIndexer {
  fn create_embedded_chunks_from_collab(
    &self,
    collab: &Collab,
    model: EmbeddingModel,
  ) -> Result<Vec<AFCollabEmbeddedChunk>, AppError> {
    let object_id = collab.object_id().parse()?;
    let document = DocumentBody::from_collab(collab).ok_or_else(|| {
      anyhow!(
        "Failed to get document body from collab `{}`: schema is missing required fields",
        object_id
      )
    })?;

    let paragraphs = document.paragraphs(collab.transact());
    self.create_embedded_chunks_from_text(object_id, paragraphs, model)
  }

  fn create_embedded_chunks_from_text(
    &self,
    object_id: Uuid,
    paragraphs: Vec<String>,
    model: EmbeddingModel,
  ) -> Result<Vec<AFCollabEmbeddedChunk>, AppError> {
    if paragraphs.is_empty() {
      warn!(
        "[Embedding] No paragraphs found in document `{}`. Skipping embedding.",
        object_id
      );

      return Ok(vec![]);
    }
    split_text_into_chunks(object_id, paragraphs, CollabType::Document, model)
  }

  async fn embed(
    &self,
    embedder: &AFEmbedder,
    mut content: Vec<AFCollabEmbeddedChunk>,
  ) -> Result<Option<AFCollabEmbeddings>, AppError> {
    if content.is_empty() {
      return Ok(None);
    }

    let contents: Vec<_> = content
      .iter()
      .map(|fragment| fragment.content.clone().unwrap_or_default())
      .collect();

    let request = CreateEmbeddingRequestArgs::default()
      .model(embedder.model().name())
      .input(EmbeddingInput::StringArray(contents))
      .encoding_format(EncodingFormat::Float)
      .dimensions(EmbeddingModel::default_model().default_dimensions())
      .build()
      .map_err(|err| AppError::Unhandled(err.to_string()))?;

    let resp = embedder.async_embed(request).await?;

    trace!(
      "[Embedding] request {} embeddings, received {} embeddings",
      content.len(),
      resp.data.len()
    );

    for embedding in resp.data {
      let param = &mut content[embedding.index as usize];
      if param.content.is_some() {
        param.embedding = Some(embedding.embedding);
      }
    }

    Ok(Some(AFCollabEmbeddings {
      tokens_consumed: resp.usage.total_tokens,
      params: content,
    }))
  }
}
fn split_text_into_chunks(
  object_id: Uuid,
  paragraphs: Vec<String>,
  collab_type: CollabType,
  embedding_model: EmbeddingModel,
) -> Result<Vec<AFCollabEmbeddedChunk>, AppError> {
  debug_assert!(matches!(
    embedding_model,
    EmbeddingModel::TextEmbedding3Small
  ));

  if paragraphs.is_empty() {
    return Ok(vec![]);
  }
  // Group paragraphs into chunks of roughly 8000 characters.
  let split_contents = group_paragraphs_by_max_content_len(paragraphs, 8000);
  let metadata = json!({
      "id": object_id,
      "source": "appflowy",
      "name": "document",
      "collab_type": collab_type
  });

  let mut seen = std::collections::HashSet::new();
  let mut chunks = Vec::new();

  for (index, content) in split_contents.into_iter().enumerate() {
    let consistent_hash = Hasher::oneshot(0, content.as_bytes());
    let fragment_id = format!("{:x}", consistent_hash);
    if seen.insert(fragment_id.clone()) {
      chunks.push(AFCollabEmbeddedChunk {
        fragment_id,
        object_id,
        content_type: EmbeddingContentType::PlainText,
        content: Some(content),
        embedding: None,
        metadata: metadata.clone(),
        fragment_index: index as i32,
        embedded_type: 0,
      });
    } else {
      debug!(
        "[Embedding] Duplicate fragment_id detected: {}. This fragment will not be added.",
        fragment_id
      );
    }
  }
  Ok(chunks)
}
