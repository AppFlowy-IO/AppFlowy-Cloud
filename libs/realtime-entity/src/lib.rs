pub mod collab_msg;

pub mod message;
pub mod sync_protocol;
pub mod user;

// If the realtime_proto not exist, the following code will be generated:
// ```shell
//  cd libs/realtime-entity
//  cargo clean
//  cargo build
// ```
pub mod realtime_proto;

pub use collab::core::collab_plugin::EncodedCollabV1;
