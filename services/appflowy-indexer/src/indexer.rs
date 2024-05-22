use std::ops::DerefMut;

use async_trait::async_trait;
use collab_entity::CollabType;
use openai_dive::v1::api::Client;
use openai_dive::v1::models::EmbeddingsEngine;
use openai_dive::v1::resources::embedding::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingOutput, EmbeddingParameters,
};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::{Executor, PgPool, QueryBuilder};

use crate::error::Result;

#[async_trait]
pub trait Indexer: Send + Sync {
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
  /// See: migrations/20240412083446_history_init.sql
  pub partition_key: u32,
  /// Content of the fragment.
  pub content: String,
  pub embedding: Option<Vector>,
}

impl From<Fragment> for EmbedFragment {
  fn from(fragment: Fragment) -> Self {
    EmbedFragment {
      fragment_id: fragment.fragment_id,
      object_id: fragment.object_id,
      partition_key: match fragment.collab_type {
        CollabType::Document => 0,
        CollabType::Database => 1,
        CollabType::WorkspaceDatabase => 2,
        CollabType::Folder => 3,
        CollabType::DatabaseRow => 4,
        CollabType::UserAwareness => 5,
        CollabType::Unknown => 6, // not really supported
      },
      content: fragment.content,
      embedding: None,
    }
  }
}

pub type Embedding = Vec<f64>;

pub struct PostgresIndexer {
  openai: Client,
  db: PgPool,
}

impl PostgresIndexer {
  pub async fn open(openai_api_key: &str, pg_conn: &str) -> Result<Self> {
    let openai = Client::new(openai_api_key.to_string());
    let db = PgPool::connect(&pg_conn).await?;
    Ok(Self { openai, db })
  }

  pub fn new(openai: Client, db: PgPool) -> Self {
    Self { openai, db }
  }

  async fn get_embeddings(&self, fragments: Vec<Fragment>) -> Result<Vec<EmbedFragment>> {
    let inputs: Vec<_> = fragments
      .iter()
      .map(|fragment| fragment.content.clone())
      .collect();
    println!("inputs: {:#?}", inputs);
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
      tracing::trace!("OpenAI API usage: {}", usage.prompt_tokens);
      //TODO: report usage statistics
    }

    let mut fragments: Vec<_> = fragments.into_iter().map(EmbedFragment::from).collect();
    for e in resp.data.into_iter() {
      let embedding = match e.embedding {
        EmbeddingOutput::Float(embedding) => Vector::from(
          embedding
            .into_iter()
            .map(|f| f as f32)
            .collect::<Vec<f32>>(),
        ),
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
    for f in fragments {
      sqlx::query(
        r#"INSERT INTO af_collab_embeddings (fragment_id, oid, partition_key, content, embedding)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (fragment_id) DO UPDATE SET content = $4, embedding = $5"#,
      )
      .bind(f.fragment_id)
      .bind(f.object_id)
      .bind(f.partition_key as i32)
      .bind(f.content)
      .bind(f.embedding)
      .execute(tx.deref_mut())
      .await?;
    }
    tx.commit().await?;
    Ok(())
  }
}

#[async_trait]
impl Indexer for PostgresIndexer {
  async fn update_index(&self, documents: Vec<Fragment>) -> Result<()> {
    let embeddings = self.get_embeddings(documents).await?;
    self.store_embeddings(embeddings).await?;
    Ok(())
  }

  async fn remove(&self, ids: &[FragmentID]) -> Result<()> {
    let mut tx = self.db.begin().await?;
    sqlx::query!(
      "DELETE FROM af_collab_embeddings WHERE fragment_id IN (SELECT unnest($1::text[]))",
      ids
    )
    .execute(tx.deref_mut())
    .await?;
    tx.commit().await?;
    Ok(())
  }
}

#[cfg(test)]
mod test {
  use pgvector::Vector;
  use sqlx::Row;

  use crate::indexer::{Indexer, PostgresIndexer};
  use crate::test_utils::{db_pool, openai_client};

  #[tokio::test]
  async fn test_indexing_embeddings() {
    let _ = env_logger::builder().is_test(true).try_init();

    let db = db_pool().await;
    let openai = openai_client();

    let indexer = PostgresIndexer::new(openai, db);

    let fragment_id = uuid::Uuid::new_v4().to_string();
    let object_id = uuid::Uuid::new_v4().to_string();
    let user_id = uuid::Uuid::new_v4();
    let workspace_id = uuid::Uuid::new_v4().to_string();

    let fragments = vec![super::Fragment {
      fragment_id: fragment_id.clone(),
      object_id: object_id.clone(),
      collab_type: collab_entity::CollabType::Document,
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

    tx.commit().await.unwrap();
  }
}
