use actix::{Actor, Context, Handler};
use appflowy_collaborate::actix_ws::client::rt_client::{
  HandlerResult, RealtimeClient, RealtimeServer,
};
use appflowy_collaborate::actix_ws::entities::{ClientMessage, Connect, Disconnect};
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::RealtimeMessage;
use semver::Version;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::sleep;

#[actix_rt::test]
async fn test_handle_message() {
  let device_id = "device_id".to_string();
  let session_id = "session_id".to_string();
  let user = RealtimeUser::new(1, device_id, session_id, 2);
  let server = MockRealtimeServer::new(10).start();
  let client_version = Version::new(0, 5, 0);

  let (_tx, external_source) = tokio::sync::mpsc::channel(100);
  let client = RealtimeClient::new(
    user,
    server,
    Duration::from_secs(6),
    Duration::from_secs(10),
    client_version,
    external_source,
    10,
  );

  let mut message_by_oid = HashMap::new();
  message_by_oid.insert("object_id".to_string(), vec![]);
  let message = RealtimeMessage::ClientCollabV2(message_by_oid);
  client.try_send(message).unwrap();
}

#[actix_rt::test]
#[should_panic]
async fn server_mailbox_full_test() {
  let device_id = "device_id".to_string();
  let session_id = "session_id".to_string();
  let user = RealtimeUser::new(1, device_id, session_id, 2);
  let server = MockRealtimeServer::new(5).start();
  let client_version = Version::new(0, 5, 0);

  let mut handles = vec![];
  // simulate 5 clients sending messages to the server
  // the mailbox size of the server is 5, so the server will be overwhelmed. When client want to send
  // more message to the server, the server will return mailbox full error.
  for _ in 0..5 {
    let cloned_user = user.clone();
    let cloned_server = server.clone();
    let cloned_client_version = client_version.clone();
    let handle = tokio::spawn(async move {
      let (_tx, external_source) = tokio::sync::mpsc::channel(100);
      let client = RealtimeClient::new(
        cloned_user,
        cloned_server,
        Duration::from_secs(6),
        Duration::from_secs(10),
        cloned_client_version,
        external_source,
        10,
      );
      for _ in 0..10 {
        let mut message_by_oid = HashMap::new();
        message_by_oid.insert("object_id".to_string(), vec![]);
        let message = RealtimeMessage::ClientCollabV2(message_by_oid);
        client.try_send(message).unwrap();
      }
    });
    handles.push(handle);
  }
  let results = futures::future::join_all(handles).await;
  for result in results {
    assert!(result.is_ok(), "{:?}", result.unwrap());
  }
}

#[actix_rt::test]
async fn client_rate_limit_hit_test() {
  let device_id = "device_id".to_string();
  let session_id = "session_id".to_string();
  let user = RealtimeUser::new(1, device_id, session_id, 2);
  let server = MockRealtimeServer::new(5).start();
  let client_version = Version::new(0, 5, 0);

  let mut handles = vec![];
  // We are setting up a simulation where five clients attempt to send messages to a server simultaneously.
  // The server has been configured to handle a maximum of five messages at any given time, which represents
  // its mailbox capacity. This limitation means the server can become overwhelmed if it receives too many
  // messages in a short period.
  // However, to manage this potential overflow, each client incorporates a rate-limiting mechanism
  // set to allow only one message per client. This rate-limiting effectively prevents the scenario where
  // all clients try to send messages beyond the server's capacity simultaneously.
  for _ in 0..5 {
    let cloned_user = user.clone();
    let cloned_server = server.clone();
    let cloned_client_version = client_version.clone();
    let handle = tokio::spawn(async move {
      let (_tx, external_source) = tokio::sync::mpsc::channel(100);
      let client = RealtimeClient::new(
        cloned_user,
        cloned_server,
        Duration::from_secs(6),
        Duration::from_secs(10),
        cloned_client_version,
        external_source,
        1,
      );
      for _ in 0..10 {
        let mut message_by_oid = HashMap::new();
        message_by_oid.insert("object_id".to_string(), vec![]);
        let message = RealtimeMessage::ClientCollabV2(message_by_oid);
        if let Err(err) = client.try_send(message) {
          if err.is_too_many_message() {
            continue;
          } else {
            panic!("{:?}", err);
          }
        }
      }
    });
    handles.push(handle);
  }
  let results = futures::future::join_all(handles).await;
  for result in results {
    assert!(result.is_ok(), "{:?}", result.unwrap());
  }
}

struct MockRealtimeServer {
  mailbox_size: usize,
}

impl MockRealtimeServer {
  fn new(mailbox_size: usize) -> Self {
    Self { mailbox_size }
  }
}

impl Actor for MockRealtimeServer {
  type Context = Context<Self>;

  fn started(&mut self, ctx: &mut Self::Context) {
    ctx.set_mailbox_capacity(self.mailbox_size);
  }
}

impl Handler<ClientMessage> for MockRealtimeServer {
  type Result = HandlerResult;

  fn handle(&mut self, _msg: ClientMessage, _ctx: &mut Self::Context) -> Self::Result {
    Box::pin(async {
      // simulate some work
      sleep(Duration::from_millis(500)).await;
      Ok(())
    })
  }
}

impl Handler<Connect> for MockRealtimeServer {
  type Result = HandlerResult;

  fn handle(&mut self, _msg: Connect, _ctx: &mut Self::Context) -> Self::Result {
    Box::pin(async { Ok(()) })
  }
}

impl Handler<Disconnect> for MockRealtimeServer {
  type Result = HandlerResult;

  fn handle(&mut self, _msg: Disconnect, _ctx: &mut Self::Context) -> Self::Result {
    Box::pin(async { Ok(()) })
  }
}

impl RealtimeServer for MockRealtimeServer {}
