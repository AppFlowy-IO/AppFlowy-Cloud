use crate::util::{spawn_server, TestUser};
use actix_web::http::StatusCode;
use appflowy_server::component::auth::LoginResponse;

#[tokio::test]
async fn login_success() {
    let server = spawn_server().await;
    let test_user = TestUser::generate();
    test_user.register(&server).await;

    let http_resp = server.login(&test_user.email, &test_user.password).await;
    assert_eq!(http_resp.status(), StatusCode::OK);

    let bytes = http_resp.bytes().await.unwrap();
    let response: LoginResponse = serde_json::from_slice(&bytes).unwrap();
    assert!(!response.token.is_empty())
}

#[tokio::test]
async fn login_with_empty_email() {
    let server = spawn_server().await;
    let test_user = TestUser::generate();
    test_user.register(&server).await;

    let http_resp = server.login("", &test_user.password).await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_with_empty_password() {
    let server = spawn_server().await;
    let test_user = TestUser::generate();
    test_user.register(&server).await;

    let http_resp = server.login(&test_user.email, "").await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_with_unknown_user() {
    let server = spawn_server().await;
    let http_resp = server.login("unknown@appflowy.io", "Abc@123!").await;
    assert_eq!(http_resp.status(), StatusCode::UNAUTHORIZED);
}
