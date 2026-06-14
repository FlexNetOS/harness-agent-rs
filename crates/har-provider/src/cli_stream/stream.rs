//! Line-framed NDJSON reader over subprocess stdout bytes.
//!
//! Produces a `Stream<Item = Result<serde_json::Value, StreamError>>` from a raw
//! byte stream (real tokio process stdout or a fake `FakeByteStream`).
//!
//! Architecture §6.6: `stream.rs` — line-framed NDJSON read of stdout.
//!
//! Behaviour:
//! - Split on `\n` (LF). `\r\n` lines: the `\r` is stripped.
//! - Empty lines are skipped.
//! - Non-UTF8 bytes are logged and skipped (match TS's `for await` which skips bad chunks).
//! - JSON parse errors are emitted as `Err(StreamError::ParseError)` so the caller can
//!   decide whether to abort or continue.

use bytes::Bytes;
use futures_core::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StreamError {
    #[error("I/O error reading subprocess stdout: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error on line {line_no}: {source}\n  line: {line:?}")]
    ParseError {
        line_no: usize,
        line: String,
        #[source]
        source: serde_json::Error,
    },
}

/// A line-framed NDJSON stream over a byte source.
///
/// Constructed from either:
/// - A `tokio::io::AsyncRead` (real subprocess stdout) via `NdjsonStream::from_async_read`
/// - A `Bytes` blob (fake subprocess) via `NdjsonStream::from_bytes`
pub struct NdjsonStream {
    inner: Pin<Box<dyn Stream<Item = Result<serde_json::Value, StreamError>> + Send>>,
}

impl NdjsonStream {
    /// Create from a raw byte stream (e.g. `tokio_util::codec::FramedRead`).
    pub fn from_byte_stream<S>(byte_stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    {
        use async_stream::stream;
        use futures::StreamExt;

        let inner_stream = stream! {
            let mut buf: Vec<u8> = Vec::new();
            let mut line_no: usize = 0;
            let mut byte_stream = Box::pin(byte_stream);
            loop {
                match byte_stream.next().await {
                    None => break,
                    Some(Err(e)) => {
                        yield Err(StreamError::Io(e));
                        return;
                    }
                    Some(Ok(chunk)) => {
                        buf.extend_from_slice(&chunk);
                        // Drain complete lines from buf.
                        while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                            let raw_line: Vec<u8> = buf.drain(..=pos).collect();
                            // Trim trailing \r\n
                            let line_bytes = if raw_line.ends_with(b"\r\n") {
                                &raw_line[..raw_line.len() - 2]
                            } else if raw_line.ends_with(b"\n") {
                                &raw_line[..raw_line.len() - 1]
                            } else {
                                &raw_line[..]
                            };
                            if line_bytes.is_empty() {
                                continue;
                            }
                            line_no += 1;
                            let s = match std::str::from_utf8(line_bytes) {
                                Ok(s) => s,
                                Err(_) => {
                                    tracing::warn!(line_no, "cli_stream.non_utf8_line_skipped");
                                    continue;
                                }
                            };
                            match serde_json::from_str::<serde_json::Value>(s) {
                                Ok(v) => yield Ok(v),
                                Err(e) => yield Err(StreamError::ParseError {
                                    line_no,
                                    line: s.to_owned(),
                                    source: e,
                                }),
                            }
                        }
                    }
                }
            }
            // Handle any remaining bytes (no trailing newline — treat as a partial line).
            if !buf.is_empty() {
                line_no += 1;
                if let Ok(s) = std::str::from_utf8(&buf) {
                    let s = s.trim();
                    if !s.is_empty() {
                        match serde_json::from_str::<serde_json::Value>(s) {
                            Ok(v) => yield Ok(v),
                            Err(e) => yield Err(StreamError::ParseError {
                                line_no,
                                line: s.to_owned(),
                                source: e,
                            }),
                        }
                    }
                }
            }
        };

        Self { inner: Box::pin(inner_stream) }
    }

    /// Create from a `Bytes` blob (used by `FakeSpawner`).
    pub fn from_bytes(data: Bytes) -> Self {
        let stream = futures::stream::once(async move { Ok::<Bytes, std::io::Error>(data) });
        Self::from_byte_stream(stream)
    }
}

impl Stream for NdjsonStream {
    type Item = Result<serde_json::Value, StreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    async fn collect_all(stream: NdjsonStream) -> Vec<Result<serde_json::Value, String>> {
        stream
            .map(|r| r.map_err(|e| e.to_string()))
            .collect()
            .await
    }

    #[tokio::test]
    async fn parse_two_ndjson_lines() {
        let data = b"{\"type\":\"assistant\"}\n{\"type\":\"result\"}\n";
        let stream = NdjsonStream::from_bytes(Bytes::from(data.as_ref()));
        let items = collect_all(stream).await;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_ref().unwrap()["type"], "assistant");
        assert_eq!(items[1].as_ref().unwrap()["type"], "result");
    }

    #[tokio::test]
    async fn empty_lines_are_skipped() {
        let data = b"{\"a\":1}\n\n\n{\"b\":2}\n";
        let stream = NdjsonStream::from_bytes(Bytes::from(data.as_ref()));
        let items = collect_all(stream).await;
        assert_eq!(items.len(), 2);
    }

    #[tokio::test]
    async fn crlf_line_endings_stripped() {
        let data = b"{\"x\":1}\r\n{\"y\":2}\r\n";
        let stream = NdjsonStream::from_bytes(Bytes::from(data.as_ref()));
        let items = collect_all(stream).await;
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_ref().unwrap()["x"], 1);
    }

    #[tokio::test]
    async fn invalid_json_yields_parse_error() {
        let data = b"not-json\n{\"ok\":true}\n";
        let stream = NdjsonStream::from_bytes(Bytes::from(data.as_ref()));
        let items = collect_all(stream).await;
        assert_eq!(items.len(), 2);
        assert!(items[0].is_err());
        assert!(items[1].is_ok());
    }

    #[tokio::test]
    async fn no_trailing_newline_still_parsed() {
        let data = b"{\"last\":true}";
        let stream = NdjsonStream::from_bytes(Bytes::from(data.as_ref()));
        let items = collect_all(stream).await;
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].as_ref().unwrap()["last"], true);
    }

    #[tokio::test]
    async fn empty_input_produces_no_items() {
        let stream = NdjsonStream::from_bytes(Bytes::new());
        let items = collect_all(stream).await;
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn non_utf8_line_is_skipped_not_errored() {
        // A line of invalid UTF-8 bytes between two valid JSON lines must be SKIPPED
        // (matches the TS `for await` which silently drops bad chunks), not surfaced
        // as an item or an error. The two valid lines still parse.
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(b"{\"a\":1}\n");
        data.extend_from_slice(&[0xff, 0xfe, 0xfd]); // invalid UTF-8
        data.push(b'\n');
        data.extend_from_slice(b"{\"b\":2}\n");
        let stream = NdjsonStream::from_bytes(Bytes::from(data));
        let items = collect_all(stream).await;
        // Exactly the two valid lines; the non-UTF8 line produced neither an item nor an error.
        assert_eq!(items.len(), 2, "non-utf8 line must be skipped: {items:?}");
        assert!(items.iter().all(|r| r.is_ok()));
        assert_eq!(items[0].as_ref().unwrap()["a"], 1);
        assert_eq!(items[1].as_ref().unwrap()["b"], 2);
    }
}
