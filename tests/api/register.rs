use crate::test_server::spawn_server;
use appflowy_server::component::auth::RegisterResponse;
use reqwest::StatusCode;

#[tokio::test]
// curl -X POST --url http://0.0.0.0:8000/api/user/register --header 'content-type: application/json' --data '{"name":"fake name", "email":"fake@appflowy.io", "password":"Fake@123"}'
async fn register_success() {
    let server = spawn_server().await;
    let http_resp = server
        .register("user 1", "fake@appflowy.io", "FakePassword!123")
        .await;

    let bytes = http_resp.bytes().await.unwrap();
    let response: RegisterResponse = serde_json::from_slice(&bytes).unwrap();

    println!("{:?}", response);
}

#[tokio::test]
async fn register_with_invalid_password() {
    let server = spawn_server().await;
    let http_resp = server.register("user 1", "fake@appflowy.io", "123").await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_with_invalid_name() {
    let server = spawn_server().await;
    let http_resp = server
        .register("", "fake@appflowy.io", "FakePassword!123")
        .await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_with_invalid_email() {
    let server = spawn_server().await;
    let http_resp = server
        .register("me", "appflowy.io", "FakePassword!123")
        .await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
}
