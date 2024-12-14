use std::time::Duration;

use anyhow::Context;
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
use collab_document::blocks::Block;
use collab_document::document::Document;
use collab_document::document_data::default_document_collab_data;
use collab_entity::CollabType;
use nanoid::nanoid;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use redis::aio::ConnectionManager;
use tokio::time::sleep;

#[allow(dead_code)]
pub fn generate_random_bytes(size: usize) -> Vec<u8> {
  let s: String = thread_rng()
    .sample_iter(&Alphanumeric)
    .take(size)
    .map(char::from)
    .collect();
  s.into_bytes()
}

#[allow(dead_code)]
pub fn generate_random_string(len: usize) -> String {
  let rng = thread_rng();
  rng
    .sample_iter(&Alphanumeric)
    .take(len)
    .map(char::from)
    .collect()
}

pub fn make_big_collab_doc_state(object_id: &str, key: &str, value: String) -> Vec<u8> {
  let mut collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  collab.insert(key, value);
  collab
    .encode_collab_v1(|_| Ok::<(), anyhow::Error>(()))
    .unwrap()
    .doc_state
    .to_vec()
}

pub struct TestDocumentEditor {
  document: Document,
}

impl TestDocumentEditor {
  pub(crate) fn insert_paragraphs(&mut self, paragraphs: Vec<String>) {
    let page_id = self.document.get_page_id().unwrap();
    let mut prev_id = "".to_string();
    for paragraph in paragraphs {
      let block_id = nanoid!(6);
      let text_id = nanoid!(6);
      let block = Block {
        id: block_id.clone(),
        ty: "paragraph".to_owned(),
        parent: page_id.clone(),
        children: "".to_string(),
        external_id: Some(text_id.clone()),
        external_type: Some("text".to_owned()),
        data: Default::default(),
      };

      self
        .document
        .insert_block(block, Some(prev_id.clone()))
        .unwrap();
      prev_id.clone_from(&block_id);
      self
        .document
        .apply_text_delta(&text_id, format!(r#"[{{"insert": "{}"}}]"#, paragraph));
    }
  }

  pub fn encode_collab(&self) -> EncodedCollab {
    self.document.encode_collab().unwrap()
  }

  pub fn to_plain_text(&self) -> String {
    self.document.to_plain_text(false).unwrap()
  }
}

pub fn empty_document_editor(object_id: &str) -> TestDocumentEditor {
  let doc_state = default_document_collab_data(&object_id)
    .unwrap()
    .doc_state
    .to_vec();
  let collab = Collab::new_with_source(
    CollabOrigin::Empty,
    object_id,
    DataSource::DocStateV1(doc_state),
    vec![],
    false,
  )
  .unwrap();
  TestDocumentEditor {
    document: Document::open(collab).unwrap(),
  }
}

pub fn empty_collab_doc_state(object_id: &str, collab_type: CollabType) -> Vec<u8> {
  match collab_type {
    CollabType::Document => default_document_collab_data(&object_id)
      .unwrap()
      .doc_state
      .to_vec(),
    CollabType::Unknown => {
      let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
      collab
        .encode_collab_v1(|_| Ok::<(), anyhow::Error>(()))
        .unwrap()
        .doc_state
        .to_vec()
    },
    _ => panic!("collab type not supported"),
  }
}

pub fn test_encode_collab_v1(object_id: &str, key: &str, value: &str) -> EncodedCollab {
  let mut collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  collab.insert(key, value);
  collab
    .encode_collab_v1(|_| Ok::<(), anyhow::Error>(()))
    .unwrap()
}

pub async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri)
    .context("failed to connect to redis")
    .unwrap()
}

pub async fn redis_connection_manager() -> ConnectionManager {
  let mut attempt = 0;
  let max_attempts = 5;
  let mut wait_time = 500;
  loop {
    match redis_client().await.get_connection_manager().await {
      Ok(manager) => return manager,
      Err(err) => {
        if attempt >= max_attempts {
          panic!("{:?}", err); // Exceeded maximum attempts, return the last error.
        }
        sleep(Duration::from_millis(wait_time)).await;
        wait_time *= 2;
        attempt += 1;
      },
    }
  }
}
