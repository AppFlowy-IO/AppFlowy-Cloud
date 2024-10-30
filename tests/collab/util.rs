use std::time::Duration;

use anyhow::Context;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab::preclude::Collab;
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

use std::io::{BufReader, Read};

use collab::preclude::MapExt;
use flate2::bufread::GzDecoder;
use serde::Deserialize;
use yrs::{GetString, Text, TextRef};

use client_api_test::CollabRef;

/// (position, delete length, insert content).
#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
pub struct TestPatch(pub usize, pub usize, pub String);

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
pub struct TestTxn {
  // time: String, // ISO String. Unused.
  pub patches: Vec<TestPatch>,
}

#[derive(Debug, Clone, Deserialize, Eq, PartialEq)]
pub struct TestScenario {
  #[serde(default)]
  pub using_byte_positions: bool,

  #[serde(rename = "startContent")]
  pub start_content: String,
  #[serde(rename = "endContent")]
  pub end_content: String,

  pub txns: Vec<TestTxn>,
}

impl TestScenario {
  /// Load the testing data at the specified file. If the filename ends in .gz, it will be
  /// transparently uncompressed.
  ///
  /// This method panics if the file does not exist, or is corrupt. It'd be better to have a try_
  /// variant of this method, but given this is mostly for benchmarking and testing, I haven't felt
  /// the need to write that code.
  pub fn open(fpath: &str) -> TestScenario {
    // let start = SystemTime::now();
    // let mut file = File::open("benchmark_data/automerge-paper.json.gz").unwrap();
    let file = std::fs::File::open(fpath).unwrap();

    let mut reader = BufReader::new(file);
    // We could pass the GzDecoder straight to serde, but it makes it way slower to parse for
    // some reason.
    let mut raw_json = vec![];

    if fpath.ends_with(".gz") {
      let mut reader = GzDecoder::new(reader);
      reader.read_to_end(&mut raw_json).unwrap();
    } else {
      reader.read_to_end(&mut raw_json).unwrap();
    }

    let data: TestScenario = serde_json::from_reader(raw_json.as_slice()).unwrap();
    data
  }

  pub async fn execute(&self, collab: CollabRef, step_count: usize) -> String {
    let mut i = 0;
    for t in self.txns.iter().take(step_count) {
      i += 1;
      if i % 10_000 == 0 {
        tracing::trace!("Executed {}/{} steps", i, step_count);
      }
      let mut lock = collab.write().await;
      let collab = lock.borrow_mut();
      let mut txn = collab.context.transact_mut();
      let txt = collab.data.get_or_init_text(&mut txn, "text-id");
      for patch in t.patches.iter() {
        let at = patch.0;
        let delete = patch.1;
        let content = patch.2.as_str();

        if delete != 0 {
          txt.remove_range(&mut txn, at as u32, delete as u32);
        }
        if !content.is_empty() {
          txt.insert(&mut txn, at as u32, content);
        }
      }
    }

    // validate after applying all patches
    let lock = collab.read().await;
    let collab = lock.borrow();
    let txn = collab.context.transact();
    let txt: TextRef = collab.data.get_with_txn(&txn, "text-id").unwrap();
    txt.get_string(&txn)
  }
}
