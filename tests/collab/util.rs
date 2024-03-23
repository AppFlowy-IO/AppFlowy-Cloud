use anyhow::Context;
use appflowy_cloud::config::config::get_env_var;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

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
  let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  collab.insert(key, value);
  collab.encode_collab_v1().doc_state.to_vec()
}

pub fn test_encode_collab_v1(object_id: &str, key: &str, value: &str) -> EncodedCollab {
  let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  collab.insert(key, value);
  collab.encode_collab_v1()
}

#[allow(dead_code)]
pub async fn redis_client() -> redis::Client {
  let redis_uri = get_env_var("APPFLOWY_REDIS_URI", "redis://redis:6379");
  redis::Client::open(redis_uri)
    .context("failed to connect to redis")
    .unwrap()
}
