use crate::test_server::{spawn_server, TestUser};
use actix_web::http::StatusCode;

#[tokio::test]
async fn change_password_with_unmatched_password() {
    let server = spawn_server().await;
    let test_user = TestUser::generate();
    let token = test_user.register(&server).await;

    let new_password = "HelloWorld@1a";
    let new_password_confirm = "HeloWorld@1a";
    let http_resp = server
        .change_password(
            token,
            &test_user.password,
            new_password,
            new_password_confirm,
        )
        .await;
    assert_eq!(http_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_fail_after_change_password() {
    let server = spawn_server().await;
    let test_user = TestUser::generate();
    let token = test_user.register(&server).await;

    let new_password = "HelloWorld@1a";
    let http_resp = server
        .change_password(token, &test_user.password, new_password, new_password)
        .await;
    assert_eq!(http_resp.status(), StatusCode::OK);

    let http_resp = server.login(&test_user.email, &test_user.password).await;
    assert_eq!(http_resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_success_with_new_password() {
    let server = spawn_server().await;
    let test_user = TestUser::generate();
    let token = test_user.register(&server).await;

    let new_password = "HelloWorld@1a";
    let http_resp = server
        .change_password(token, &test_user.password, new_password, new_password)
        .await;
    assert_eq!(http_resp.status(), StatusCode::OK);

    let http_resp = server.login(&test_user.email, new_password).await;
    assert_eq!(http_resp.status(), StatusCode::OK);
}
