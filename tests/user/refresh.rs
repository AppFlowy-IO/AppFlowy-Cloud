use app_error::AppError;
use client_api_test::generate_unique_registered_user_client;
use futures::future::join_all;
use std::time::SystemTime;

#[tokio::test]
async fn refresh_success() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let old_token = c.access_token().unwrap();
  tokio::time::sleep(std::time::Duration::from_secs(2)).await;
  c.refresh_token("").await.unwrap();
  let new_token = c.access_token().unwrap();
  assert_ne!(old_token, new_token);
}

#[tokio::test]
async fn concurrent_refresh() {
  let (c, _user) = generate_unique_registered_user_client().await;
  let old_token = c.access_token().unwrap();
  tokio::time::sleep(std::time::Duration::from_secs(2)).await;

  let mut join_handles = vec![];
  for _ in 0..20 {
    let cloned_client = c.clone();
    let handle = tokio::spawn(async move {
      cloned_client.refresh_token("").await.unwrap();
      Ok::<(), AppError>(())
    });
    join_handles.push(handle);
  }
  let results = join_all(join_handles).await;
  assert_eq!(results.len(), 20);
  for result in results {
    result.unwrap().unwrap();
  }

  let new_token = c.access_token().unwrap();
  assert_ne!(old_token, new_token);
}

#[tokio::test]
async fn refresh_trigger() {
  let (c, _user) = generate_unique_registered_user_client().await;
  tokio::time::sleep(std::time::Duration::from_secs(2)).await;
  let old_access_token = c.access_token().unwrap();

  // Set the token to be expired
  c.token().write().as_mut().unwrap().expires_at = SystemTime::now()
    .duration_since(SystemTime::UNIX_EPOCH)
    .unwrap()
    .as_secs() as i64;

  // querying that requires auth should trigger a refresh
  let _workspaces = c.get_workspaces().await.unwrap();
  let new_token = c.access_token().unwrap();

  assert_ne!(old_access_token, new_token);
}
