use crate::config::Config;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
  pub config: Arc<Config>,
}
