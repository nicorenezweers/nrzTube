//! Zugriff auf die JSON-API unter `/api`. Fehler kommen als `{"error": "…"}`.

use gloo_net::http::{Request, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

fn network(e: impl std::fmt::Display) -> ApiError {
    ApiError { status: 0, message: format!("Keine Verbindung zum Server ({e}).") }
}

async fn finish<T: DeserializeOwned>(res: Response) -> ApiResult<T> {
    let status = res.status();
    let text = res.text().await.map_err(network)?;
    if (200..300).contains(&status) {
        let value: Value = if text.trim().is_empty() { Value::Null } else {
            serde_json::from_str(&text).map_err(|e| ApiError { status, message: e.to_string() })?
        };
        return serde_json::from_value(value).map_err(|e| ApiError { status, message: format!("Unerwartete Antwort: {e}") });
    }
    if status == 401 {
        crate::state::on_unauthorized();
    }
    let message = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get("error").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| format!("Fehler {status}"));
    Err(ApiError { status, message })
}

pub async fn get<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    finish(Request::get(path).send().await.map_err(network)?).await
}

pub async fn post<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    finish(Request::post(path).json(body).map_err(network)?.send().await.map_err(network)?).await
}

pub async fn put<B: Serialize, T: DeserializeOwned>(path: &str, body: &B) -> ApiResult<T> {
    finish(Request::put(path).json(body).map_err(network)?.send().await.map_err(network)?).await
}

pub async fn delete<T: DeserializeOwned>(path: &str) -> ApiResult<T> {
    finish(Request::delete(path).send().await.map_err(network)?).await
}

pub async fn post_text<T: DeserializeOwned>(path: &str, body: String) -> ApiResult<T> {
    finish(Request::post(path).header("content-type", "text/plain").body(body).map_err(network)?.send().await.map_err(network)?).await
}

pub fn enc(s: &str) -> String {
    urlencoding::encode(s).into_owned()
}
