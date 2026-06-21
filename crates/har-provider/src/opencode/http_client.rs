//! Pure-Rust HTTP/SSE client for the embedded OpenCode server.
//!
//! PORT of packages/providers/src/community/opencode/client.js / client.gen.js
//!
//! Replaces the `@opencode-ai/sdk` Node.js client with a native reqwest-based
//! HTTP client + an SSE event stream. All requests carry the `directory` query
//! parameter so the embedded server scopes the request to the right working dir.

use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HttpClientError {
    #[error("HTTP request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("SSE parse error: {0}")]
    SseParse(String),
    #[error("Server error {status}: {body}")]
    ServerError { status: u16, body: String },
}

/// A parsed SSE event frame.
///
/// PORT of the `{ type, properties }` event shape emitted by `client.event.subscribe`.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event_type: String,
    pub properties: Map<String, Value>,
}

/// Native HTTP client for the embedded OpenCode server.
///
/// PORT of the `OpencodeClient` surface used by session.ts / provider.ts.
pub struct OpenCodeClient {
    base_url: String,
    directory: String,
    client: reqwest::Client,
}

impl OpenCodeClient {
    /// Create a client bound to a server URL and working directory.
    pub fn new(base_url: String, directory: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            directory,
            client: reqwest::Client::new(),
        }
    }

    /// Build a full URL for a server path.
    fn url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/{}", self.base_url, path)
        }
    }

    /// URL-encode the directory for use as a query parameter.
    fn dir_param(&self) -> String {
        url::form_urlencoded::byte_serialize(self.directory.as_bytes()).collect::<String>()
    }

    /// Create a new session. `parent_id` / `title` are optional.
    ///
    /// PORT of `client.session.create({ body, query: { directory } })`.
    pub async fn create_session(
        &self,
        parent_id: Option<&str>,
        title: Option<&str>,
    ) -> Result<Value, HttpClientError> {
        let mut body = Map::new();
        if let Some(pid) = parent_id {
            body.insert("parentID".to_owned(), Value::String(pid.to_owned()));
        }
        if let Some(t) = title {
            body.insert("title".to_owned(), Value::String(t.to_owned()));
        }
        let url = format!("{}?directory={}", self.url("/session"), self.dir_param());
        let resp = self
            .client
            .post(&url)
            .json(&Value::Object(body))
            .send()
            .await?;
        Self::json_or_error(resp).await
    }

    /// Fetch a session by id.
    ///
    /// PORT of `client.session.get({ path: { id }, query: { directory } })`.
    pub async fn get_session(&self, session_id: &str) -> Result<Value, HttpClientError> {
        let url = format!(
            "{}?directory={}",
            self.url(&format!("/session/{}", session_id)),
            self.dir_param()
        );
        let resp = self.client.get(&url).send().await?;
        Self::json_or_error(resp).await
    }

    /// Submit a prompt asynchronously (fire-and-forget; results arrive over SSE).
    ///
    /// PORT of `client.session.promptAsync({ path: { id }, body, query: { directory } })`.
    pub async fn prompt_async(
        &self,
        session_id: &str,
        body: &Value,
    ) -> Result<(), HttpClientError> {
        let url = format!(
            "{}?directory={}",
            self.url(&format!("/session/{}/prompt_async", session_id)),
            self.dir_param()
        );
        let resp = self.client.post(&url).json(body).send().await?;
        Self::unit_or_error(resp).await
    }

    /// Subscribe to the server-sent event stream.
    ///
    /// PORT of `client.event.subscribe({ query: { directory } })`.
    pub async fn subscribe_events(
        &self,
    ) -> Result<impl futures_core::Stream<Item = Result<SseEvent, HttpClientError>>, HttpClientError>
    {
        use futures_util::StreamExt;

        let url = format!("{}?directory={}", self.url("/event"), self.dir_param());
        let resp = self.client.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(HttpClientError::ServerError { status, body });
        }

        let byte_stream = resp.bytes_stream();
        let stream = async_stream::try_stream! {
            let mut buf = String::new();
            tokio::pin!(byte_stream);
            while let Some(chunk_result) = byte_stream.next().await {
                let chunk: bytes::Bytes = chunk_result.map_err(HttpClientError::Reqwest)?;
                let text = String::from_utf8_lossy(&chunk);
                buf.push_str(&text);

                while let Some(frame_end) = buf.find("\n\n") {
                    let frame = buf[..frame_end].to_owned();
                    buf.drain(..frame_end + 2);

                    for line in frame.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data.is_empty() || data == "[DONE]" {
                                continue;
                            }
                            let v: Value = serde_json::from_str(data)
                                .map_err(|e| HttpClientError::SseParse(format!("{}: {}", e, data)))?;
                            if let Value::Object(obj) = v {
                                let event_type = obj
                                    .get("type")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("unknown")
                                    .to_owned();
                                let properties = if let Some(props) =
                                    obj.get("properties").and_then(|p| p.as_object())
                                {
                                    props.clone()
                                } else {
                                    let mut props = obj.clone();
                                    props.remove("type");
                                    props
                                };
                                yield SseEvent { event_type, properties };
                            }
                        }
                    }
                }
            }
        };

        Ok(stream)
    }

    /// Fetch a single message (used to read structured output at idle).
    ///
    /// PORT of `client.session.message({ path: { id, messageID }, query: { directory } })`.
    pub async fn get_message(
        &self,
        session_id: &str,
        message_id: &str,
    ) -> Result<Value, HttpClientError> {
        let url = format!(
            "{}?directory={}",
            self.url(&format!("/session/{}/message/{}", session_id, message_id)),
            self.dir_param()
        );
        let resp = self.client.get(&url).send().await?;
        Self::json_or_error(resp).await
    }

    /// Abort a running session.
    ///
    /// PORT of `client.session.abort({ path: { id }, query: { directory } })`.
    pub async fn abort_session(&self, session_id: &str) -> Result<(), HttpClientError> {
        let url = format!(
            "{}?directory={}",
            self.url(&format!("/session/{}/abort", session_id)),
            self.dir_param()
        );
        let resp = self.client.post(&url).send().await?;
        Self::unit_or_error(resp).await
    }

    /// Dispose OpenCode's cached instance state for this directory.
    ///
    /// PORT of `client.instance.dispose({ query: { directory } })`.
    pub async fn dispose_instance(&self) -> Result<(), HttpClientError> {
        let url = format!(
            "{}?directory={}",
            self.url("/instance/dispose"),
            self.dir_param()
        );
        let resp = self.client.post(&url).send().await?;
        Self::unit_or_error(resp).await
    }

    // ─── Response helpers ──────────────────────────────────────────────────────

    async fn json_or_error(resp: reqwest::Response) -> Result<Value, HttpClientError> {
        if resp.status().is_success() {
            let text = resp.text().await?;
            let v: Value = serde_json::from_str(&text)?;
            Ok(v)
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(HttpClientError::ServerError { status, body })
        }
    }

    async fn unit_or_error(resp: reqwest::Response) -> Result<(), HttpClientError> {
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            Err(HttpClientError::ServerError { status, body })
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_url_construction() {
        let client = OpenCodeClient::new("http://127.0.0.1:9000".to_owned(), "/tmp".to_owned());
        let url = client.url("/session");
        assert!(url.contains("http://127.0.0.1:9000"));
        assert!(url.ends_with("/session"));
    }

    #[test]
    fn dir_param_encodes_slashes() {
        let client = OpenCodeClient::new(
            "http://127.0.0.1:9000".to_owned(),
            "/home/user/proj".to_owned(),
        );
        let encoded = client.dir_param();
        assert!(encoded.contains("%2F"));
        assert!(!encoded.contains('/'));
    }

    #[test]
    fn sse_event_struct_fields() {
        let e = SseEvent {
            event_type: "message.updated".to_owned(),
            properties: Map::new(),
        };
        assert_eq!(e.event_type, "message.updated");
        assert!(e.properties.is_empty());
    }
}
