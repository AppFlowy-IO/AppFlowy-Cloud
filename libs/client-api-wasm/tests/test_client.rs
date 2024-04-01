use client_api::ClientConfiguration;
use client_api_wasm::ClientAPI;
use client_api_wasm::entities::ClientAPIConfig;
use uuid::Uuid;

pub struct TestClient {
	pub client_api: ClientAPI,
}

impl TestClient {
	pub fn new() -> Self {
		let device_id = Uuid::new_v4().to_string();
		let config = ClientAPIConfig {
			base_url: "http://localhost:8000".to_string(),
			ws_addr: "ws://localhost:8000/ws".to_string(),
			gotrue_url:
			device_id,
			client_id: "web".to_string(), // "web" is the default client_id
			configuration: None
		};

		let client_api = ClientAPI::new();
		Self { client_api }
	}
}