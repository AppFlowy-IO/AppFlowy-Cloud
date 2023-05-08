use collab_client_ws::WSClient;
use collab_persistence::kv::rocks_kv::RocksCollabDB;
use collab_sync::server::{CollabMsgCodec, CollabSink, CollabStream};
use std::sync::Arc;

pub async fn spawn_client(
  uid: i64,
  object_id: &str,
  address: String,
) -> std::io::Result<(Arc<RocksCollabDB>, Arc<MutexCollab>)> {
  let ws_client = WSClient::new(address, 100);
  let origin = origin_from_tcp_stream(&ws_client);
  let (reader, writer) = stream.into_split();
  let collab = Arc::new(MutexCollab::new(origin.clone(), object_id, vec![]));

  // sync
  let stream = CollabStream::new(reader, CollabMsgCodec::default());
  let sink = CollabSink::new(writer, CollabMsgCodec::default());
  let sync_plugin = SyncPlugin::new(origin, object_id, collab.clone(), sink, stream);
  collab.lock().add_plugin(Arc::new(sync_plugin));

  // disk
  let tempdir = TempDir::new().unwrap();
  let path = tempdir.into_path();
  let db = Arc::new(RocksCollabDB::open(path).unwrap());
  let disk_plugin = RocksdbDiskPlugin::new(uid, db.clone()).unwrap();
  collab.lock().add_plugin(Arc::new(disk_plugin));
  collab.initial();

  {
    let client = collab.lock();
    client.with_transact_mut(|txn| {
      let map = client.create_map_with_txn(txn, "map");
      map.insert_with_txn(txn, "task1", "a");
      map.insert_with_txn(txn, "task2", "b");
    });
  }

  Ok((db, collab))
}

fn origin_from_tcp_stream(ws_client: &WSClient) -> CollabOrigin {
  let address = stream.local_addr().unwrap();
  let origin = CollabClient::new(address.port() as i64, &address.to_string());
  CollabOrigin::Client(origin)
}
