use crate::util::{spawn_server, TestServer, TestUser};

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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::RwLock;

pub enum TestScript {
  CreateClient {
    uid: i64,
  },
  OpenObject {
    uid: i64,
    object_id: String,
  },
  AssertClientContent {
    uid: i64,
    object_id: String,
    expected: Value,
  },
  Wait {
    secs: u64,
  },
  AssertServerContent {
    object_id: String,
    expected: Value,
  },
  ModifyClientCollab {
    uid: i64,
    object_id: String,
    f: fn(&Collab),
  },
  AssertClientEqualToServer {
    uid: i64,
    object_id: String,
  },
}

pub struct ScriptTest {
  server: TestServer,
  pub clients: RwLock<HashMap<i64, TestClient>>,
}

impl ScriptTest {
  pub async fn new() -> Self {
    let server = spawn_server().await;
    ScriptTest {
      server,
      clients: RwLock::new(HashMap::new()),
    }
  }

  async fn get_client_doc_value(&self, uid: i64, object_id: &str) -> Value {
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

  pub async fn run_script(&mut self, script: TestScript) {
    match script {
      TestScript::CreateClient { uid } => {
        let test_user = TestUser::generate();
        let token = test_user.register(&self.server).await;
        let address = format!("{}/{}", self.server.ws_addr, token);
        let ws = WSClient::new(address, WSClientConfig::default());
        let addr = ws.connect().await.unwrap().unwrap();
        let origin = origin_from_tcp_stream(&addr);
        let tempdir = TempDir::new().unwrap();
        let db_path = tempdir.into_path();
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
      },
      TestScript::OpenObject { uid, object_id } => {
        let mut clients = self.clients.write().await;
        let client = clients.get_mut(&uid).unwrap();
        let handler = client.ws.subscribe(1, object_id.clone()).await.unwrap();
        let (sink, stream) = (handler.sink(), handler.stream());
        let collab = Arc::new(MutexCollab::new(client.origin.clone(), &object_id, vec![]));

        let object = SyncObject::new(&object_id, "1");
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
        client.collab_by_object_id.insert(object_id, collab);
      },
      TestScript::Wait { secs } => {
        tokio::time::sleep(Duration::from_secs(secs)).await;
      },
      TestScript::AssertClientContent {
        uid,
        object_id,
        expected,
      } => {
        let value = self.get_client_doc_value(uid, &object_id).await;
        assert_json_diff::assert_json_eq!(value, expected);
      },
      TestScript::AssertServerContent {
        object_id,
        expected,
      } => {
        let value = self.server.get_doc(&object_id).await;
        assert_json_diff::assert_json_eq!(value, expected);
      },
      TestScript::AssertClientEqualToServer { uid, object_id } => {
        let server_value = self.server.get_doc(&object_id).await;
        let client_value = self.get_client_doc_value(uid, &object_id).await;
        assert_eq!(client_value, server_value);
        assert_json_diff::assert_json_eq!(client_value, server_value);
      },
      TestScript::ModifyClientCollab { uid, object_id, f } => {
        let mut clients = self.clients.write().await;
        let client = clients.get_mut(&uid).unwrap();
        let collab = client
          .collab_by_object_id
          .get_mut(&object_id)
          .unwrap()
          .lock();
        f(&collab);
      },
    }
  }

  pub async fn run_scripts(&mut self, scripts: Vec<TestScript>) {
    for script in scripts {
      self.run_script(script).await;
    }
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
