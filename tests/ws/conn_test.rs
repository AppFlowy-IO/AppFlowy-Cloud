use crate::util::spawn_server;

#[tokio::test]
async fn login_success() {
    let server = spawn_server().await;
    // let test_user = TestUser::generate();
    // test_user.register(&server).await;
    //
    // let http_resp = server.login(&test_user.email, &test_user.password).await;
    // assert_eq!(http_resp.status(), StatusCode::OK);
    //
    // let bytes = http_resp.bytes().await.unwrap();
    // let response: LoginResponse = serde_json::from_slice(&bytes).unwrap();
    // assert!(!response.token.is_empty())
}
