
#[derive(Debug, thiserror::Error)]
pub enum WSError {
    #[cfg(not(target_arch = "wasm32"))]
    #[error(transparent)]
    TungsteniteError(tokio_tungstenite::tungstenite::Error),

    #[cfg(target_arch = "wasm32")]
    #[error("Wasm error: {0}")]
    WasmError(web_sys::wasm_bindgen::JsValue),

    #[error("{0}")]
    LostConnection(String),

    #[error("Auth error: {0}")]
    AuthError(String),

    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
