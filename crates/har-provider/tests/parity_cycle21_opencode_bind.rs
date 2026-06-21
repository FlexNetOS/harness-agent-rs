//! Cycle-21 parity harness — OpenCode SDK bind (PR-21).

#[test]
fn sse_event_struct_construction() {
    use har_provider::opencode::http_client::SseEvent;
    let e = SseEvent {
        event_type: "message.updated".to_owned(),
        properties: serde_json::Map::new(),
    };
    assert_eq!(e.event_type, "message.updated");
    assert!(e.properties.is_empty());
}

#[test]
fn runtime_error_aborted_display() {
    use har_provider::opencode::runtime::RuntimeError;
    let e = RuntimeError::Aborted;
    assert!(e.to_string().contains("aborted"));
}

#[test]
fn runtime_error_spawn_failed_display() {
    use har_provider::opencode::runtime::RuntimeError;
    let e = RuntimeError::SpawnFailed("binary not found".to_owned());
    let s = e.to_string().to_lowercase();
    assert!(s.contains("spawn") || s.contains("binary") || s.contains("opencode"));
}

#[test]
fn runtime_error_startup_timeout_display() {
    use har_provider::opencode::runtime::RuntimeError;
    let e = RuntimeError::StartupTimeout(5000);
    let s = e.to_string();
    assert!(s.contains("5000") || s.contains("timed out") || s.contains("timeout"));
}

#[test]
fn port_bind_conflict_eaddrinuse() {
    use har_provider::opencode::runtime::is_port_bind_conflict;
    assert!(is_port_bind_conflict("EADDRINUSE: address already in use"));
}

#[test]
fn port_bind_conflict_address_in_use() {
    use har_provider::opencode::runtime::is_port_bind_conflict;
    assert!(is_port_bind_conflict("address already in use"));
}

#[test]
fn port_bind_conflict_false_for_normal() {
    use har_provider::opencode::runtime::is_port_bind_conflict;
    assert!(!is_port_bind_conflict("connection refused"));
}

#[test]
fn extract_port_from_url_works() {
    use har_provider::opencode::runtime::extract_port_from_url;
    assert_eq!(extract_port_from_url("http://127.0.0.1:12345"), Some(12345));
    assert_eq!(extract_port_from_url("not-a-url"), None);
}

#[test]
fn opencode_client_construction() {
    use har_provider::opencode::http_client::OpenCodeClient;
    let _client = OpenCodeClient::new("http://127.0.0.1:9000".to_owned(), "/tmp".to_owned());
}

#[tokio::test]
#[ignore = "opencode binary not available (set OPENCODE_LIVE_TEST=1 to enable)"]
async fn live_acquire_and_create_session() {
    use har_provider::opencode::runtime::{
        acquire_embedded_runtime, release_embedded_runtime_for_url,
    };
    let runtime = acquire_embedded_runtime(false)
        .await
        .expect("should acquire runtime");
    let client = har_provider::opencode::http_client::OpenCodeClient::new(
        runtime.server_url.clone(),
        "/tmp".to_owned(),
    );
    let session = client
        .create_session(None, None)
        .await
        .expect("should create session");
    assert!(session.get("id").and_then(|v| v.as_str()).is_some());
    release_embedded_runtime_for_url(&runtime.server_url).await;
}
