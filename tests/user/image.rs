use client_api_test::generate_unique_registered_user_client;

#[tokio::test]
async fn upload_and_retrieve_image_asset() {
  let (client, _) = generate_unique_registered_user_client().await;
  let asset_source = client
    .upload_user_image_asset("tests/user/asset/avatar.png")
    .await
    .unwrap();
  let file_id = asset_source.file_id;
  let person_id = client.get_profile().await.unwrap().uuid;
  client
    .get_user_image_asset(&person_id, &file_id)
    .await
    .unwrap();
}
