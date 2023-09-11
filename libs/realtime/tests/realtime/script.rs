use crate::util::{spawn_server, Cleaner, TestServer};

use collab::core::collab::MutexCollab;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::preclude::Collab;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;

use collab_plugins::sync_plugin::{SyncObject, SyncPlugin};
use collab_ws::{WSClient, WSClientConfig, WSObjectHandler};

use collab_plugins::kv::rocks_kv::RocksCollabDB;
use collab_plugins::local_storage::rocksdb::RocksdbDiskPlugin;

use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::RwLock;

pub struct CollabTest {
  server: TestServer,
  pub clients: RwLock<HashMap<i64, TestClient>>,
}

impl CollabTest {
  pub async fn new() -> Self {
    let server = spawn_server().await;
    CollabTest {
      server,
      clients: RwLock::new(HashMap::new()),
    }
  }

  pub async fn get_client_doc(&self, uid: i64, object_id: &str) -> Value {
    self
      .clients
      .read()
      .await
      .get(&uid)
      .unwrap()
      .collab_by_object_id
      .get(object_id)
      .unwrap()
      .lock()
      .to_json_value()
  }

  pub async fn create_client(&mut self, uid: i64) {
    let address = format!("{}/{}", self.server.ws_addr, uid);
    let ws = WSClient::new(address, WSClientConfig::default());
    let addr = ws.connect().await.unwrap().unwrap();
    let origin = origin_from_tcp_stream(&addr);
    let tempdir = TempDir::new().unwrap().into_path();
    let db_path = tempdir.join(uuid::Uuid::new_v4().to_string());
    std::fs::create_dir_all(&db_path).unwrap();

    let db = Arc::new(RocksCollabDB::open(db_path.clone()).unwrap());
    let cleaner = Cleaner::new(db_path);
    let client = TestClient {
      ws,
      db,
      origin,
      collab_by_object_id: Default::default(),
      handlers: vec![],
      cleaner,
    };
    self.clients.write().await.insert(uid, client);
  }
  pub async fn open_object(&self, uid: i64, object_id: &str) {
    let mut clients = self.clients.write().await;
    let client = clients.get_mut(&uid).unwrap();
    let handler = client.ws.subscribe(1, object_id.to_string()).await.unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let collab = Arc::new(MutexCollab::new(client.origin.clone(), object_id, vec![]));

    let object = SyncObject::new(object_id, "1");
    // Sync
    let sync_plugin = SyncPlugin::new(
      client.origin.clone(),
      object,
      Arc::downgrade(&collab),
      sink,
      stream,
    );
    collab.lock().add_plugin(Arc::new(sync_plugin));

    // Disk
    let disk_plugin = RocksdbDiskPlugin::new(uid, Arc::downgrade(&client.db));
    collab.lock().add_plugin(Arc::new(disk_plugin));

    collab.async_initialize().await;
    client.handlers.push(handler);
    client
      .collab_by_object_id
      .insert(object_id.to_string(), collab);
  }

  pub async fn modify_object(&self, uid: i64, object_id: &str, f: fn(&Collab)) {
    let mut clients = self.clients.write().await;
    let client = clients.get_mut(&uid).unwrap();
    let collab = client
      .collab_by_object_id
      .get_mut(object_id)
      .unwrap()
      .lock();
    f(&collab);
  }

  pub async fn wait(&self, secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
  }

  pub async fn get_server_doc(&self, object_id: &str) -> Value {
    self.server.get_doc(object_id).await
  }
}

fn origin_from_tcp_stream(addr: &SocketAddr) -> CollabOrigin {
  let origin = CollabClient::new(addr.port() as i64, addr.to_string());
  CollabOrigin::Client(origin)
}

/// Used to simulate a client with AppFlowy application
pub struct TestClient {
  pub ws: WSClient,
  pub db: Arc<RocksCollabDB>,
  pub origin: CollabOrigin,
  pub collab_by_object_id: HashMap<String, Arc<MutexCollab>>,
  pub handlers: Vec<Arc<WSObjectHandler>>,
  #[allow(dead_code)]
  cleaner: Cleaner,
}
