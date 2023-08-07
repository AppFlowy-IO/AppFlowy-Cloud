use anyhow::Error;

pub struct Client {
    http_client: reqwest::Client,
    base_url: String,
}

impl Client {
    pub fn from(c: reqwest::Client, base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            http_client: c,
        }
    }

    pub async fn register(&self, name: &str, email: &str, password: &str) -> Result<(), Error> {
        let url = format!("{}/api/user/register", self.base_url);
        let payload = serde_json::json!({
            "name": name,
            "password": password,
            "email": email,
        });
        let resp = self.http_client
            .post(&url)
            .json(&payload)
            .send()
            .await?;
        let text = resp.text().await?;
        println!("{}", text);

        Ok(())
    }
}
