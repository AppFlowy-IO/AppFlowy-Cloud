use crate::application::AppState;
use crate::biz::history::get_snapshots;
use collab_entity::CollabType;
use tonic::{Request, Response, Status};
use tonic_proto::history::history_server::History;
use tonic_proto::history::{RepeatedSnapshotMeta, SnapshotRequest};

pub struct HistoryImpl {
  pub state: AppState,
}

#[tonic::async_trait]
impl History for HistoryImpl {
  async fn get_snapshots(
    &self,
    request: Request<SnapshotRequest>,
  ) -> Result<Response<tonic_proto::history::RepeatedSnapshotMeta>, Status> {
    let request = request.into_inner();
    let collab_type = CollabType::from(request.collab_type);
    let data = get_snapshots(&request.object_id, &collab_type, &self.state.pg_pool).await?;
    Ok(Response::new(data))
  }
}
