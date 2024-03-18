use shared_entity::response::{AppResponseError, ErrorCode};

use crate::ext::entities::JsonResponse;

pub mod api;
pub mod entities;
pub mod error;

async fn from_json_response<T>(resp: reqwest::Response) -> Result<T, error::Error>
where
  T: serde::de::DeserializeOwned,
{
  if !resp.status().is_success() {
    let status = resp.status();
    let payload = resp.text().await?;
    return Err(error::Error::NotOk(status.as_u16(), payload));
  }

  let payload = resp.text().await?;
  match serde_json::from_str::<JsonResponse<T>>(&payload) {
    Ok(data) => Ok(data.data),
    Err(_) => match serde_json::from_str::<AppResponseError>(&payload) {
      Ok(af_cloud_err) => Err(error::Error::AppFlowyCloud(af_cloud_err)),
      Err(err) => Err(error::Error::Unhandled(format!(
        "Failed to parse JSON response: {:?}, Payload: {}",
        err, payload
      ))),
    },
  }
}

async fn check_response(resp: reqwest::Response) -> Result<(), error::Error> {
  let status = resp.status();
  let payload = resp.text().await?;

  if !status.is_success() {
    return Err(error::Error::NotOk(status.as_u16(), payload));
  }

  if let Ok(cloud_err) = serde_json::from_str::<AppResponseError>(&payload) {
    if cloud_err.code == ErrorCode::Ok {
      return Ok(());
    } else {
      return Err(error::Error::AppFlowyCloud(cloud_err));
    }
  };

  Ok(())
}
