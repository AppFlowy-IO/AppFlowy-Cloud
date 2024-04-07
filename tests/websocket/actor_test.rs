use actix::{Actor, Context, Handler};
use appflowy_cloud::biz::actix_ws::client::rt_client::{
  HandlerResult, RealtimeClient, RealtimeServer,
};
use appflowy_cloud::biz::actix_ws::entities::{ClientMessage, Connect, Disconnect};
use collab_rt_entity::user::RealtimeUser;
use semver::Version;
use std::time::Duration;

#[actix_rt::test]
async fn test_handle_message() {
  let device_id = "device_id".to_string();
  let session_id = "session_id".to_string();
  let user = RealtimeUser::new(1, device_id, session_id, 2);
  let server = MockRealtimeServer.start();
  let client_version = Version::new(0, 5, 0);

  let (_tx, external_source) = tokio::sync::mpsc::channel(100);
  let _client = RealtimeClient::new(
    user,
    server,
    Duration::from_secs(6),
    Duration::from_secs(10),
    client_version,
    external_source,
  );
}

struct MockRealtimeServer;

impl Actor for MockRealtimeServer {
  type Context = Context<Self>;
}

impl Handler<ClientMessage> for MockRealtimeServer {
  type Result = HandlerResult;

  fn handle(&mut self, _msg: ClientMessage, _ctx: &mut Self::Context) -> Self::Result {
    Box::pin(async { Ok(()) })
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
