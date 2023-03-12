use crate::test_server::{error_msg_from_resp, spawn_server};
use appflowy_server::component::auth::{InputParamsError, RegisterResponse};
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
    assert_eq!(error_msg_from_resp(http_resp).await, InputParamsError::InvalidPassword.to_string());
}

#[tokio::test]
async fn register_with_invalid_name() {
    let server = spawn_server().await;
    let name = "".to_string();
    let http_resp = server
        .register(&name, "fake@appflowy.io", "FakePassword!123")
        .await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_msg_from_resp(http_resp).await, InputParamsError::InvalidName(name).to_string());
}

#[tokio::test]
async fn register_with_invalid_email() {
    let server = spawn_server().await;
    let email = "appflowy.io".to_string();
    let http_resp = server
        .register("me", &email, "FakePassword!123")
        .await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(error_msg_from_resp(http_resp).await, InputParamsError::InvalidEmail(email).to_string());
}
