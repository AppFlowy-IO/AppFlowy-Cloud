use async_trait::async_trait;
use collab_entity::CollabType;
use openai_dive::v1::api::Client;
use openai_dive::v1::models::EmbeddingsEngine;
use openai_dive::v1::resources::embedding::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingOutput, EmbeddingParameters,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use database::index::{has_collab_embeddings, remove_collab_embeddings, upsert_collab_embeddings};
use database_entity::dto::{AFCollabEmbeddingParams, EmbeddingContentType};

use crate::error::Result;

#[async_trait]
pub trait Indexer: Send + Sync {
  /// Check if document with given id has been already a corresponding index entry.
  async fn was_indexed(&self, object_id: &str) -> Result<bool>;
  async fn update_index(&self, documents: Vec<Fragment>) -> Result<()>;
  async fn remove(&self, ids: &[FragmentID]) -> Result<()>;
}

pub type FragmentID = String;

/// Fragment represents a single piece of indexable data.
/// This can be a piece of document (like block), that belongs to a document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fragment {
  /// Unique fragment identifier.
  pub fragment_id: FragmentID,
  /// Object, which this fragment belongs to.
  pub object_id: String,
  /// Type of the document object.
  pub collab_type: CollabType,
  /// Type of the content, current fragment represents.
  pub content_type: EmbeddingContentType,
  /// Content of the fragment.
  pub content: String,
}

/// Fragment represents a single piece of indexable data.
/// This can be a piece of document (like block), that belongs to a document.
#[derive(Debug, Clone, PartialEq)]
struct EmbedFragment {
  /// Unique fragment identifier.
  pub fragment_id: FragmentID,
  /// Object, which this fragment belongs to.
  pub object_id: String,
  /// Type of the document object.
  pub collab_type: CollabType,
  /// Content of the fragment.
  pub content: String,
  pub content_type: EmbeddingContentType,
  pub embedding: Option<Vec<f32>>,
}

impl From<Fragment> for EmbedFragment {
  fn from(fragment: Fragment) -> Self {
    EmbedFragment {
      fragment_id: fragment.fragment_id,
      object_id: fragment.object_id,
      collab_type: fragment.collab_type,
      content: fragment.content,
      content_type: fragment.content_type,
      embedding: None,
    }
  }
}

impl From<EmbedFragment> for AFCollabEmbeddingParams {
  fn from(f: EmbedFragment) -> Self {
    AFCollabEmbeddingParams {
      fragment_id: f.fragment_id.into(),
      object_id: f.object_id,
      collab_type: f.collab_type,
      content_type: f.content_type,
      content: f.content,
      embedding: f.embedding,
    }
  }
}

pub struct PostgresIndexer {
  openai: Client,
  db: PgPool,
}

impl PostgresIndexer {
  #[allow(dead_code)]
  pub async fn open(openai_api_key: &str, pg_conn: &str) -> Result<Self> {
    let openai = Client::new(openai_api_key.to_string());
    let db = PgPool::connect(&pg_conn).await?;
    Ok(Self { openai, db })
  }

  #[allow(dead_code)]
  pub fn new(openai: Client, db: PgPool) -> Self {
    Self { openai, db }
  }

  async fn get_embeddings(&self, fragments: Vec<Fragment>) -> Result<Vec<EmbedFragment>> {
    let inputs: Vec<_> = fragments
      .iter()
      .map(|fragment| fragment.content.clone())
      .collect();
    let resp = self
      .openai
      .embeddings()
      .create(EmbeddingParameters {
        input: EmbeddingInput::StringArray(inputs),
        model: EmbeddingsEngine::TextEmbedding3Small.to_string(),
        encoding_format: Some(EmbeddingEncodingFormat::Float),
        dimensions: Some(1536), // text-embedding-3-small default number of dimensions
        user: None,
      })
      .await
      .map_err(|e| crate::error::Error::OpenAI(e.to_string()))?;

    tracing::trace!("fetched {} embeddings", resp.data.len());
    if let Some(usage) = resp.usage {
      tracing::info!("OpenAI API usage: {}", usage.total_tokens);
      //TODO: report usage statistics
    }

    let mut fragments: Vec<_> = fragments.into_iter().map(EmbedFragment::from).collect();
    for e in resp.data.into_iter() {
      let embedding = match e.embedding {
        EmbeddingOutput::Float(embedding) => embedding
          .into_iter()
          .map(|f| f as f32)
          .collect::<Vec<f32>>(),

        EmbeddingOutput::Base64(_) => unreachable!("Unexpected base64 encoding"),
      };
      fragments[e.index as usize].embedding = Some(embedding);
    }
    Ok(fragments)
  }

  async fn store_embeddings(&self, fragments: Vec<EmbedFragment>) -> Result<()> {
    tracing::trace!(
      "storing {} embeddings inside of vector database",
      fragments.len()
    );
    let mut tx = self.db.begin().await?;
    upsert_collab_embeddings(
      &mut tx,
      fragments.into_iter().map(EmbedFragment::into).collect(),
    )
    .await?;
    tx.commit().await?;
    Ok(())
  }
}

#[async_trait]
impl Indexer for PostgresIndexer {
  async fn was_indexed(&self, object_id: &str) -> Result<bool> {
    let found = has_collab_embeddings(&mut self.db.begin().await?, object_id).await?;
    Ok(found)
  }

  async fn update_index(&self, documents: Vec<Fragment>) -> Result<()> {
    let embeddings = self.get_embeddings(documents).await?;
    self.store_embeddings(embeddings).await?;
    Ok(())
  }

  async fn remove(&self, ids: &[FragmentID]) -> Result<()> {
    let mut tx = self.db.begin().await?;
    remove_collab_embeddings(&mut tx, ids).await?;
    tx.commit().await?;
    Ok(())
  }
}

#[cfg(test)]
mod test {
  use database_entity::dto::EmbeddingContentType;
  use pgvector::Vector;
  use sqlx::Row;

  use crate::indexer::{Indexer, PostgresIndexer};
  use crate::test_utils::{db_pool, openai_client, setup_collab};

  #[tokio::test]
  async fn test_indexing_embeddings() {
    let _ = env_logger::builder().is_test(true).try_init();

    let db = db_pool().await;
    let object_id = uuid::Uuid::new_v4();
    let uid = rand::random();
    setup_collab(&db, uid, object_id, vec![]).await;

    let openai = openai_client();

    let indexer = PostgresIndexer::new(openai, db);

    let fragment_id = uuid::Uuid::new_v4().to_string();

    let fragments = vec![super::Fragment {
      fragment_id: fragment_id.clone(),
      object_id: object_id.to_string(),
      collab_type: collab_entity::CollabType::Document,
      content_type: EmbeddingContentType::PlainText,
      content: "Hello, world!".to_string(),
    }];

    // resolve embeddings from OpenAI
    let embeddings = indexer.get_embeddings(fragments).await.unwrap();
    assert_eq!(embeddings[0].embedding.is_some(), true);

    // store embeddings in DB
    indexer.store_embeddings(embeddings).await.unwrap();

    // search for embedding
    let mut tx = indexer.db.begin().await.unwrap();
    let row =
      sqlx::query("SELECT content, embedding FROM af_collab_embeddings WHERE fragment_id = $1")
        .bind(&fragment_id)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let content: String = row.get(0);
    assert_eq!(&content, "Hello, world!");
    let embedding: Option<Vector> = row.get(1);
    assert!(embedding.is_some());

    // remove embeddings
    indexer.remove(&[fragment_id.clone()]).await.unwrap();

    let mut tx = indexer.db.begin().await.unwrap();
    let row =
      sqlx::query("SELECT content, embedding FROM af_collab_embeddings WHERE fragment_id = $1")
        .bind(&fragment_id)
        .fetch_one(&mut *tx)
        .await;
    assert!(row.is_err());
  }
}
