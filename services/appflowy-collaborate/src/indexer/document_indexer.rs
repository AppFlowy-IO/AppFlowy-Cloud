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

use crate::config::get_env_var;
use crate::indexer::open_ai::{split_text_by_max_content_len, split_text_by_max_tokens};
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
        create_embedding(
          object_id,
          content,
          CollabType::Document,
          &self.embedding_model,
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
  embedding_model: &EmbeddingModel,
  tokenizer: Arc<CoreBPE>,
) -> Result<Vec<AFCollabEmbeddingParams>, AppError> {
  let use_tiktoken = get_env_var("APPFLOWY_AI_CONTENT_SPLITTER_TIKTOKEN", "false")
    .parse::<bool>()
    .unwrap_or(false);

  let split_contents = if use_tiktoken {
    let max_tokens = embedding_model.default_dimensions() as usize;
    if content.len() < 500 {
      split_text_by_max_tokens(content, max_tokens, tokenizer.as_ref())?
    } else {
      tokio::task::spawn_blocking(move || {
        split_text_by_max_tokens(content, max_tokens, tokenizer.as_ref())
      })
      .await??
    }
  } else {
    debug_assert!(matches!(
      embedding_model,
      EmbeddingModel::TextEmbedding3Small
    ));
    // We assume that every token is ~4 bytes. We're going to split document content into fragments
    // of ~2000 tokens each.
    split_text_by_max_content_len(content, 8000)?
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
