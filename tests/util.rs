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

  pub async fn execute(&self, collab: CollabRef) {
    for t in self.txns.iter() {
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
    let expected = self.end_content.as_str();
    let lock = collab.read().await;
    let collab = lock.borrow();
    let txn = collab.context.transact();
    let txt: TextRef = collab.data.get_with_txn(&txn, "text-id").unwrap();
    let actual = txt.get_string(&txn);
    assert_eq!(actual, expected);
  }
}
