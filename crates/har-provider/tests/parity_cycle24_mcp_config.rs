//! Differential parity gate for PR-12 `loadMcpConfig`
//! (`packages/providers/src/mcp/config.ts` → `crates/har-provider/src/mcp/config.rs`).
//!
//! Every golden expectation here was captured from the LIVE TypeScript source run
//! under `bun` 1.3.14 (`loadMcpConfig` over the identical input matrix); see the
//! parity findings for the oracle. The Rust `load_mcp_config` is run over the same
//! inputs and diffed: servers JSON (incl. KEY ORDER), serverNames, missingVars, and
//! the exact thrown error MESSAGE.
//!
//! `[≈]` lenient asserts (cross-runtime free-text detail tail only — never the
//! prefix or the error condition):
//!
//! - `invalid_json`: V8 `SyntaxError` text vs `serde_json` text differ → assert
//!   the message PREFIX + that it errors. The condition (invalid JSON → error)
//!   and the prefix match the source exactly.
//!
//! Everything else (every success shape, every error PREFIX-or-full message, key
//! order, missing-var dup accounting, expansion scope, regex casing) is asserted
//! BYTE-EXACT against the live source.

use std::collections::HashMap;
use std::io::Write;

use har_provider::mcp::load_mcp_config;
use serde_json::{json, Value};
use tempfile::NamedTempFile;

fn env(pairs: &[(&str, Option<&str>)]) -> HashMap<String, Option<String>> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.map(|s| s.to_string())))
        .collect()
}

async fn load(
    contents: &str,
    env_source: &HashMap<String, Option<String>>,
) -> Result<har_provider::mcp::LoadedMcpConfig, String> {
    let mut tf = NamedTempFile::new().unwrap();
    tf.write_all(contents.as_bytes()).unwrap();
    let path = tf.path().to_string_lossy().into_owned();
    let cwd = tf.path().parent().unwrap().to_string_lossy().into_owned();
    load_mcp_config(&path, &cwd, env_source).await
}

// ─── Success shapes (servers JSON + serverNames + missingVars) ──────────────────

#[tokio::test]
async fn bare_passthrough() {
    let r = load(
        r#"{ "fs": { "command": "node", "args": ["server.js"] } }"#,
        &env(&[]),
    )
    .await
    .unwrap();
    assert_eq!(r.server_names, vec!["fs"]);
    assert!(r.missing_vars.is_empty());
    assert_eq!(
        Value::Object(r.servers),
        json!({ "fs": { "command": "node", "args": ["server.js"] } })
    );
}

#[tokio::test]
async fn wrapper_unwrap() {
    let r = load(
        r#"{ "mcpServers": { "a": { "command": "x" }, "b": { "command": "y" } } }"#,
        &env(&[]),
    )
    .await
    .unwrap();
    assert_eq!(r.server_names, vec!["a", "b"]);
    assert_eq!(
        Value::Object(r.servers),
        json!({ "a": { "command": "x" }, "b": { "command": "y" } })
    );
}

#[tokio::test]
async fn empty_object_passthrough() {
    let r = load(r#"{}"#, &env(&[])).await.unwrap();
    assert!(r.server_names.is_empty());
    assert_eq!(Value::Object(r.servers), json!({}));
    assert!(r.missing_vars.is_empty());
}

#[tokio::test]
async fn key_order_preserved() {
    // Source: serverNames = Object.keys(expanded) → insertion order ["z","a","m"].
    let r = load(r#"{ "z": {}, "a": {}, "m": {} }"#, &env(&[]))
        .await
        .unwrap();
    assert_eq!(r.server_names, vec!["z", "a", "m"]);
    // Map order check via serialization (preserve_order).
    let s = serde_json::to_string(&Value::Object(r.servers)).unwrap();
    assert_eq!(s, r#"{"z":{},"a":{},"m":{}}"#);
}

#[tokio::test]
async fn expansion_scope_env_and_headers_only() {
    // $VAR expanded ONLY in env/headers; command/url/args left literal.
    let r = load(
        r#"{ "s": { "command": "$HOME/bin", "url": "$URL", "args": ["$ARG"], "env": { "TOKEN": "$TOK" }, "headers": { "Auth": "Bearer ${TOK}" } } }"#,
        &env(&[("TOK", Some("secret")), ("HOME", Some("/no")), ("URL", Some("/no")), ("ARG", Some("/no"))]),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(r.servers),
        json!({ "s": {
            "command": "$HOME/bin",
            "url": "$URL",
            "args": ["$ARG"],
            "env": { "TOKEN": "secret" },
            "headers": { "Auth": "Bearer secret" }
        }})
    );
    assert!(r.missing_vars.is_empty());
}

#[tokio::test]
async fn lowercase_var_left_literal() {
    let r = load(
        r#"{ "s": { "env": { "A": "$lower", "B": "${lower}", "C": "$UPPER" } } }"#,
        &env(&[("UPPER", Some("ok"))]),
    )
    .await
    .unwrap();
    assert_eq!(
        Value::Object(r.servers),
        json!({ "s": { "env": { "A": "$lower", "B": "${lower}", "C": "ok" } } })
    );
    assert!(r.missing_vars.is_empty());
}

#[tokio::test]
async fn bare_var_stops_at_lowercase() {
    // $FOO_bar → matches FOO_, leaves `bar` literal → "Xbar".
    let r = load(
        r#"{ "s": { "env": { "K": "$FOO_bar" } } }"#,
        &env(&[("FOO_", Some("X"))]),
    )
    .await
    .unwrap();
    assert_eq!(r.servers["s"]["env"]["K"], json!("Xbar"));
}

#[tokio::test]
async fn braced_var_with_tail() {
    let r = load(
        r#"{ "s": { "env": { "K": "${BRACED}-tail" } } }"#,
        &env(&[("BRACED", Some("V"))]),
    )
    .await
    .unwrap();
    assert_eq!(r.servers["s"]["env"]["K"], json!("V-tail"));
}

#[tokio::test]
async fn multi_var_one_value() {
    let r = load(
        r#"{ "s": { "env": { "K": "$A-$B-${C}" } } }"#,
        &env(&[("A", Some("1")), ("B", Some("2")), ("C", Some("3"))]),
    )
    .await
    .unwrap();
    assert_eq!(r.servers["s"]["env"]["K"], json!("1-2-3"));
}

#[tokio::test]
async fn missing_vars_recorded_with_duplicates() {
    let r = load(
        r#"{ "s": { "env": { "X": "$MISSING", "Y": "$MISSING" } } }"#,
        &env(&[]),
    )
    .await
    .unwrap();
    assert_eq!(r.missing_vars, vec!["MISSING", "MISSING"]);
    assert_eq!(r.servers["s"]["env"]["X"], json!(""));
    assert_eq!(r.servers["s"]["env"]["Y"], json!(""));
}

#[tokio::test]
async fn missing_vars_across_env_and_headers_in_order() {
    // env first (X="$A"), then headers (H="$A $B") → ["A","A","B"]; H expands to " ".
    let r = load(
        r#"{ "s": { "env": { "X": "$A" }, "headers": { "H": "$A $B" } } }"#,
        &env(&[]),
    )
    .await
    .unwrap();
    assert_eq!(r.missing_vars, vec!["A", "A", "B"]);
    assert_eq!(r.servers["s"]["headers"]["H"], json!(" "));
}

#[tokio::test]
async fn present_but_undefined_key_counts_as_missing() {
    // env source has the key but mapped to None (JS `undefined`) → recorded missing,
    // replaced by "". (Source: `envVal === undefined` → push.)
    let r = load(
        r#"{ "s": { "env": { "K": "$PRESENT" } } }"#,
        &env(&[("PRESENT", None)]),
    )
    .await
    .unwrap();
    assert_eq!(r.missing_vars, vec!["PRESENT"]);
    assert_eq!(r.servers["s"]["env"]["K"], json!(""));
}

// ─── Error messages (BYTE-EXACT vs live source, minus the resolved-path tail) ───

/// The source error embeds the absolute resolved `mcpPath`; we assert the message
/// minus that path-dependent prefix/suffix where needed. For these cases the
/// message tail after the path is empty or a fixed literal, so we assert the
/// stable substring exactly (the dynamic part is only the temp-file path).
async fn err(contents: &str) -> String {
    load(contents, &env(&[])).await.unwrap_err()
}

#[tokio::test]
async fn mixed_mcpservers_with_other_keys_throws() {
    let e = err(r#"{ "mcpServers": { "a": {} }, "other": 1 }"#).await;
    assert!(
        e.starts_with("MCP config cannot mix top-level \"mcpServers\" with other keys: "),
        "{e}"
    );
    assert!(
        e.ends_with(". Use either a direct server map or { \"mcpServers\": { ... } }."),
        "{e}"
    );
}

#[tokio::test]
async fn mcpservers_nonobject_throws() {
    for body in [
        r#"{ "mcpServers": [1,2] }"#,
        r#"{ "mcpServers": 5 }"#,
        r#"{ "mcpServers": null }"#,
    ] {
        let e = err(body).await;
        assert!(
            e.starts_with("MCP config field \"mcpServers\" must be a JSON object: "),
            "{body} -> {e}"
        );
    }
}

#[tokio::test]
async fn server_nonobject_throws_with_type() {
    // (input, expected describeJsonType label) — captured from live source.
    let cases = [
        (r#"{ "bad": 42 }"#, "number"),
        (r#"{ "bad": "x" }"#, "string"),
        (r#"{ "bad": [1] }"#, "array"),
        (r#"{ "bad": null }"#, "null"),
        (r#"{ "bad": true }"#, "boolean"),
    ];
    for (body, ty) in cases {
        let e = err(body).await;
        assert_eq!(
            e,
            format!("MCP server \"bad\" must be a JSON object (got {ty})"),
            "{body}"
        );
    }
}

#[tokio::test]
async fn env_value_nonstring_throws_with_type() {
    let cases = [
        (r#"{ "s": { "env": { "K": 5 } } }"#, "number"),
        (r#"{ "s": { "env": { "K": true } } }"#, "boolean"),
        (r#"{ "s": { "env": { "K": null } } }"#, "null"),
        (r#"{ "s": { "env": { "K": [1] } } }"#, "array"),
        (r#"{ "s": { "env": { "K": {} } } }"#, "object"),
    ];
    for (body, ty) in cases {
        let e = err(body).await;
        assert_eq!(
            e,
            format!("MCP config s.env.K must be a string (got {ty})"),
            "{body}"
        );
    }
}

#[tokio::test]
async fn headers_value_nonstring_throws_with_type() {
    let e = err(r#"{ "s": { "headers": { "K": 5 } } }"#).await;
    assert_eq!(e, "MCP config s.headers.K must be a string (got number)");
}

#[tokio::test]
async fn env_nonobject_throws_with_type() {
    let cases = [
        (r#"{ "s": { "env": "nope" } }"#, "string"),
        (r#"{ "s": { "env": [1] } }"#, "array"),
        (r#"{ "s": { "env": null } }"#, "null"),
    ];
    for (body, ty) in cases {
        let e = err(body).await;
        assert_eq!(
            e,
            format!("MCP config s.env must be a JSON object of string values (got {ty})"),
            "{body}"
        );
    }
}

#[tokio::test]
async fn headers_nonobject_throws_with_type() {
    let e = err(r#"{ "s": { "headers": 7 } }"#).await;
    assert_eq!(
        e,
        "MCP config s.headers must be a JSON object of string values (got number)"
    );
}

#[tokio::test]
async fn toplevel_nonobject_throws() {
    for body in [r#"[1,2,3]"#, r#"null"#, r#"42"#, r#""hello""#] {
        let e = err(body).await;
        assert!(
            e.starts_with("MCP config must be a JSON object (Record<string, ServerConfig>): "),
            "{body} -> {e}"
        );
    }
}

#[tokio::test]
async fn invalid_json_message_prefix() {
    // `[≈]` — only the prefix + error condition are contractual; the parser detail
    // tail legitimately differs V8 vs serde_json.
    let e = err("{ not json").await;
    assert!(e.starts_with("MCP config file is not valid JSON: "), "{e}");
}

#[tokio::test]
async fn not_found_message_exact() {
    // ENOENT message is fully contractual (no parser detail).
    let cwd = "/tmp";
    let e = load_mcp_config("/no/such/file-parity-xyz.json", cwd, &env(&[]))
        .await
        .unwrap_err();
    assert_eq!(
        e,
        "MCP config file not found: /no/such/file-parity-xyz.json \
         (resolved to /no/such/file-parity-xyz.json)"
    );
}
