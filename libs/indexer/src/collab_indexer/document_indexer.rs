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
use database_entity::dto::{AFCollabEmbeddedChunk, AFCollabEmbeddings, EmbeddingContentType};
use infra::env_util::get_env_var;
use serde_json::json;
use tracing::{debug, error, trace, warn};
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
    // Group paragraphs into chunks of roughly 8000 characters.
    split_text_into_chunks(
      object_id,
      paragraphs,
      model,
      get_env_var("APPFLOWY_EMBEDDING_CHUNK_SIZE", "2000")
        .parse::<usize>()
        .unwrap_or(1000),
      get_env_var("APPFLOWY_EMBEDDING_CHUNK_OVERLAP", "200")
        .parse::<usize>()
        .unwrap_or(200),
    )
  }

  async fn embed(
    &self,
    embedder: &AFEmbedder,
    mut chunks: Vec<AFCollabEmbeddedChunk>,
  ) -> Result<Option<AFCollabEmbeddings>, AppError> {
    let mut valid_indices = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
      if let Some(ref content) = chunk.content {
        if !content.is_empty() {
          valid_indices.push(i);
        }
      }
    }

    if valid_indices.is_empty() {
      return Ok(None);
    }

    let mut contents = Vec::with_capacity(valid_indices.len());
    for &i in &valid_indices {
      contents.push(chunks[i].content.as_ref().unwrap().to_owned());
    }

    let request = CreateEmbeddingRequestArgs::default()
      .model(embedder.model().name())
      .input(EmbeddingInput::StringArray(contents))
      .encoding_format(EncodingFormat::Float)
      .dimensions(EmbeddingModel::default_model().default_dimensions())
      .build()
      .map_err(|err| AppError::Unhandled(err.to_string()))?;

    let resp = embedder.async_embed(request).await?;
    if resp.data.len() != valid_indices.len() {
      error!(
        "[Embedding] requested {} embeddings, received {} embeddings",
        valid_indices.len(),
        resp.data.len()
      );
      return Err(AppError::Unhandled(format!(
        "Mismatch in number of embeddings requested and received: {} vs {}",
        valid_indices.len(),
        resp.data.len()
      )));
    }

    for embedding in resp.data {
      let chunk_idx = valid_indices[embedding.index as usize];
      chunks[chunk_idx].embedding = Some(embedding.embedding);
    }

    Ok(Some(AFCollabEmbeddings {
      tokens_consumed: resp.usage.total_tokens,
      chunks,
    }))
  }
}

/// chunk_size:
/// Small Chunks (50–256 tokens): Best for precision-focused tasks (e.g., Q&A, technical docs) where specific details matter.
/// Medium Chunks (256–1,024 tokens): Ideal for balanced tasks like RAG or contextual search, providing enough context without noise.
/// Large Chunks (1,024–2,048 tokens): Suited for analysis or thematic tasks where broad understanding is key.
///
/// overlap:
/// Add 10–20% overlap for larger chunks (e.g., 50–100 tokens for 512-token chunks) to preserve context across boundaries.
pub fn split_text_into_chunks(
  object_id: Uuid,
  paragraphs: Vec<String>,
  embedding_model: EmbeddingModel,
  chunk_size: usize,
  overlap: usize,
) -> Result<Vec<AFCollabEmbeddedChunk>, AppError> {
  // we only support text embedding 3 small for now
  debug_assert!(matches!(
    embedding_model,
    EmbeddingModel::TextEmbedding3Small
  ));

  if paragraphs.is_empty() {
    return Ok(vec![]);
  }

  trace!(
    "[Embedding] Splitting document `{}` into chunks with chunk_size: {}, overlap: {}, paragraphs: {:?}",
    object_id,
    chunk_size,
    overlap, paragraphs
  );
  let split_contents = group_paragraphs_by_max_content_len(paragraphs, chunk_size, overlap);
  let metadata = json!({
      "id": object_id,
      "source": "appflowy",
      "name": "document",
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

  trace!(
    "[Embedding] Created {} chunks for object_id `{}`, chunk_size: {}, overlap: {}",
    chunks.len(),
    object_id,
    chunk_size,
    overlap
  );
  Ok(chunks)
}
