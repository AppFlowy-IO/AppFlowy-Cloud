use crate::test_server::spawn_server;
use appflowy_server::component::auth::RegisterResponse;

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
