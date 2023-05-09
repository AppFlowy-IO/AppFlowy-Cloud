use collab::core::collab::MutexCollab;
use collab::core::origin::{CollabClient, CollabOrigin};

use collab_client_ws::WSClient;
use collab_plugins::disk::kv::rocks_kv::RocksCollabDB;
use collab_plugins::disk::rocksdb::RocksdbDiskPlugin;
use collab_plugins::sync::SyncPlugin;
use std::net::SocketAddr;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

pub async fn spawn_client(
  uid: i64,
  object_id: &str,
  address: String,
) -> std::io::Result<TestClient> {
  let ws_client = WSClient::new(address, 100);
  let addr = ws_client.connect().await.unwrap().unwrap();
  let origin = origin_from_tcp_stream(&addr);
  let handler = ws_client.subscribe(object_id.to_string()).await.unwrap();

  //
  let (sink, stream) = (handler.sink(), handler.stream());
  let collab = Arc::new(MutexCollab::new(origin.clone(), object_id, vec![]));
  let sync_plugin = SyncPlugin::new(origin, object_id, collab.clone(), sink, stream);
  collab.lock().add_plugin(Arc::new(sync_plugin));

  // disk
  let tempdir = TempDir::new().unwrap();
  let db_path = tempdir.into_path();
  let db = Arc::new(RocksCollabDB::open(db_path.clone()).unwrap());
  let disk_plugin = RocksdbDiskPlugin::new(uid, db.clone()).unwrap();
  collab.lock().add_plugin(Arc::new(disk_plugin));
  collab.initial();

  let cleaner = Cleaner::new(db_path);
  Ok(TestClient {
    ws_client,
    db,
    collab,
    cleaner,
  })
}

fn origin_from_tcp_stream(addr: &SocketAddr) -> CollabOrigin {
  let origin = CollabClient::new(addr.port() as i64, &addr.to_string());
  CollabOrigin::Client(origin)
}

pub struct TestClient {
  #[allow(dead_code)]
  ws_client: WSClient,
  pub db: Arc<RocksCollabDB>,
  pub collab: Arc<MutexCollab>,

  #[allow(dead_code)]
  cleaner: Cleaner,
}

struct Cleaner(PathBuf);

impl Cleaner {
  fn new(dir: PathBuf) -> Self {
    Cleaner(dir)
  }

  fn cleanup(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
  }
}

impl Drop for Cleaner {
  fn drop(&mut self) {
    Self::cleanup(&self.0)
  }
}

impl Deref for TestClient {
  type Target = Arc<MutexCollab>;

  fn deref(&self) -> &Self::Target {
    &self.collab
  }
}
