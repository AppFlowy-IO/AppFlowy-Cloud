use admin_frontend::models::OAuthRedirect;
use admin_frontend::models::OAuthRedirectToken;
use base64::engine::Engine;
use base64::prelude::BASE64_STANDARD_NO_PAD;
use gotrue_entity::dto::GotrueTokenResponse;
use reqwest::StatusCode;
use reqwest::Url;
use sha2::Digest;

use crate::utils::AdminFrontendClient;

#[tokio::test]
async fn oauth_sign_in() {
  let mut af_client = AdminFrontendClient::new();
  af_client
    .web_api_sign_in("admin@example.com", "password")
    .await;

  let code_challenge_orginal = "hello123";
  let code_challenge_sha256 = {
    let mut hasher = sha2::Sha256::new();
    hasher.update(code_challenge_orginal.as_bytes());
    hasher.finalize().to_vec()
  };

  // OAuth Param
  let code_challenge = BASE64_STANDARD_NO_PAD.encode(code_challenge_sha256);
  let client_id = "appflowy_cloud";
  let state = "state123";

  {
    // redirect url not in allowed list
    let resp = af_client
      .web_api_oauth_redirect(&OAuthRedirect {
        client_id: client_id.to_string(),
        state: state.to_string(),
        redirect_uri: "https://mywebsite.com".to_string(),
        response_type: "code".to_string(),
        code_challenge: Some(code_challenge.clone()),
        code_challenge_method: Some("S256".to_string()),
      })
      .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
      resp.text().await.unwrap(),
      "invalid redirect_uri: https://mywebsite.com, allowable_uris: http://localhost:3000"
    );
  }

  {
    let resp = af_client
      .web_api_oauth_redirect(&OAuthRedirect {
        client_id: client_id.to_string(),
        state: state.to_string(),
        redirect_uri: "http://localhost:3000".to_string(),
        response_type: "code".to_string(),
        code_challenge: Some(code_challenge.clone()),
        code_challenge_method: Some("S256".to_string()),
      })
      .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let redirect_url = resp.headers().get("location").unwrap().to_str().unwrap();
    let (code, ret_state) = extract_code_and_state(redirect_url);
    assert_eq!(ret_state, state);

    {
      // did not provide code_verifier
      let resp = af_client
        .web_api_oauth_redirect_token(&OAuthRedirectToken {
          code: code.clone(),
          grant_type: "authorization_code".to_string(),
          ..Default::default()
        })
        .await;
      assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
      assert_eq!(resp.text().await.unwrap(), "missing code_verifier");
    }

    {
      let resp = af_client
        .web_api_oauth_redirect_token(&OAuthRedirectToken {
          code,
          grant_type: "authorization_code".to_string(),
          code_verifier: Some(code_challenge_orginal.to_string()),
          ..Default::default()
        })
        .await;
      assert_eq!(resp.status(), StatusCode::OK);
      let token_str = resp.text().await.unwrap();
      let _gotrue_token: GotrueTokenResponse = serde_json::from_str(&token_str).unwrap();
    }
  }
}

fn extract_code_and_state(url_str: &str) -> (String, String) {
  // Parse the URL
  let url = Url::parse(url_str).expect("Failed to parse URL");

  // Extract the query parameters
  let code = url
    .query_pairs()
    .find(|(key, _)| key == "code")
    .map(|(_, value)| value.to_string())
    .unwrap_or_else(|| "code not found".to_string());

  let state = url
    .query_pairs()
    .find(|(key, _)| key == "state")
    .map(|(_, value)| value.to_string())
    .unwrap_or_else(|| "state not found".to_string());

  (code, state)
}
