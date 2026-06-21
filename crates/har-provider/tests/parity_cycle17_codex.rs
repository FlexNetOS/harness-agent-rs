//! Differential parity harness — Cycle 17, Codex provider (PR-07 + PR-08).
//!
//! ORACLE = live `@openai/codex-sdk@0.125.0` (`dist/index.js`) + the Archon
//! `packages/providers/src/codex/{provider,config,binary-resolver,capabilities}.ts`
//! source, captured via bun. These tests pin the Rust port against that oracle.
//!
//! Each test cites the oracle source. Where the Rust port DIVERGES from the
//! oracle, the test documents the divergence (some are `#[ignore]`d as KNOWN
//! FAILs that the porter must fix — see findings/parity-cycle17.md).
//!
//! Cross-checked oracle (bun, 2026-06-21):
//!   - SDK argv order: exec --experimental-json [config…] [--model] [--sandbox]
//!     [--cd] [--add-dir…] [--skip-git-repo-check] [--output-schema]
//!     [model_reasoning_effort] [network_access] [web_search] [approval_policy]
//!     [resume]  (dist/index.js:163-216)
//!   - toTomlValue string == JSON.stringify (dist/index.js:330-331)

use har_contract::{
    CodexProviderDefaults, MessageChunk, ModelReasoningEffortCodex, WebSearchModeCodex,
};
use har_provider::codex::argv::{build_codex_argv, to_toml_value};
use har_provider::codex::config::parse_codex_config;
use har_provider::codex::parser::{parse_codex_event, CodexStreamState, ParseResult};
use har_provider::codex::provider::{
    build_codex_mcp_config_overrides, build_model_access_message, classify_codex_error,
    has_open_additional_properties, normalize_json_schema_for_openai_strict, CodexErrorClass,
};
use serde_json::{json, Map, Value};

fn to_map(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => panic!("expected object"),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// AREA 1 — build_codex_argv vs live SDK CodexExec.run() argv
// ════════════════════════════════════════════════════════════════════════════

/// Oracle CASE1 (bun): minimal config.
#[test]
fn argv_minimal_matches_sdk_oracle() {
    let argv = build_codex_argv(
        None,
        &CodexProviderDefaults::default(),
        None,
        None,
        "/workspace",
        None,
    );
    let expected = vec![
        "exec",
        "--experimental-json",
        "--sandbox",
        "danger-full-access",
        "--cd",
        "/workspace",
        "--skip-git-repo-check",
        "--config",
        "sandbox_workspace_write.network_access=true",
        "--config",
        "approval_policy=\"never\"",
    ];
    assert_eq!(argv, expected, "minimal argv must match SDK oracle CASE1");
}

/// Oracle CASE2 (bun): model + effort high + web live + 2 add-dirs.
#[test]
fn argv_full_matches_sdk_oracle() {
    let d = CodexProviderDefaults {
        model: Some("gpt-5.2-codex".to_owned()),
        model_reasoning_effort: Some(ModelReasoningEffortCodex::High),
        web_search_mode: Some(WebSearchModeCodex::Live),
        additional_directories: Some(vec!["/a".to_owned(), "/b".to_owned()]),
        ..Default::default()
    };
    let argv = build_codex_argv(None, &d, None, None, "/workspace", None);
    let expected = vec![
        "exec",
        "--experimental-json",
        "--model",
        "gpt-5.2-codex",
        "--sandbox",
        "danger-full-access",
        "--cd",
        "/workspace",
        "--add-dir",
        "/a",
        "--add-dir",
        "/b",
        "--skip-git-repo-check",
        "--config",
        "model_reasoning_effort=\"high\"",
        "--config",
        "sandbox_workspace_write.network_access=true",
        "--config",
        "web_search=\"live\"",
        "--config",
        "approval_policy=\"never\"",
    ];
    assert_eq!(argv, expected, "full argv must match SDK oracle CASE2");
}

/// Oracle CASE3 (bun): resume appended at very end, after approval_policy.
#[test]
fn argv_resume_matches_sdk_oracle() {
    let d = CodexProviderDefaults {
        model: Some("m1".to_owned()),
        ..Default::default()
    };
    let argv = build_codex_argv(None, &d, Some("thread-abc"), None, "/workspace", None);
    assert_eq!(
        &argv[argv.len() - 2..],
        &["resume".to_owned(), "thread-abc".to_owned()]
    );
    // approval_policy must be the LAST flag before resume
    let resume_idx = argv.iter().position(|a| a == "resume").unwrap();
    assert_eq!(argv[resume_idx - 1], "approval_policy=\"never\"");
}

/// Oracle CASE4 (bun): MCP config overrides flattened, placed RIGHT AFTER
/// `--experimental-json` and BEFORE `--model`/`--sandbox`. http_headers nested.
#[test]
fn argv_mcp_overrides_position_and_flatten_matches_sdk_oracle() {
    let overrides = json!({
        "mcp_servers": {
            "figma": {
                "url": "http://127.0.0.1:3845/mcp",
                "http_headers": { "Authorization": "Bearer x" }
            }
        }
    });
    let argv = build_codex_argv(
        None,
        &CodexProviderDefaults::default(),
        None,
        None,
        "/workspace",
        Some(&overrides),
    );
    // SDK oracle: config overrides come immediately after --experimental-json
    assert_eq!(argv[2], "--config");
    assert_eq!(
        argv[3],
        "mcp_servers.figma.url=\"http://127.0.0.1:3845/mcp\""
    );
    assert_eq!(argv[4], "--config");
    assert_eq!(
        argv[5],
        "mcp_servers.figma.http_headers.Authorization=\"Bearer x\""
    );
    // then --sandbox (no --model since none set)
    assert_eq!(argv[6], "--sandbox");
}

/// Oracle CASE7 (bun): number/bool/array TOML rendering.
/// SDK: `n=42`, `b=false`, `arr=["x", 1, true]` (space after comma).
#[test]
fn argv_toml_number_bool_array_matches_sdk_oracle() {
    let overrides =
        json!({ "mcp_servers": { "s": { "n": 42, "b": false, "arr": ["x", 1, true] } } });
    let argv = build_codex_argv(
        None,
        &CodexProviderDefaults::default(),
        None,
        None,
        "/workspace",
        Some(&overrides),
    );
    let flat: Vec<&String> = argv.iter().collect();
    assert!(
        flat.iter().any(|s| *s == "mcp_servers.s.n=42"),
        "argv={:?}",
        argv
    );
    assert!(
        flat.iter().any(|s| *s == "mcp_servers.s.b=false"),
        "argv={:?}",
        argv
    );
    assert!(
        flat.iter()
            .any(|s| *s == "mcp_servers.s.arr=[\"x\", 1, true]"),
        "array TOML must match SDK ', '-joined form; argv={:?}",
        argv
    );
}

// ── FIXED #1: toTomlValue control-char escaping ───────────────────────────────
//
// SDK `toTomlValue` (dist/index.js:330-331) does `JSON.stringify(value)` which
// escapes control chars: \n→\\n, \t→\\t, \r→\\r, others→\\uXXXX.
// Fix: to_toml_value now delegates to serde_json::to_string which produces
// JSON.stringify-compatible output for all control chars.
#[test]
fn toml_value_escapes_control_chars_like_json_stringify() {
    // newline
    assert_eq!(
        to_toml_value(&Value::String("line1\nline2".to_owned()), "k").unwrap(),
        "\"line1\\nline2\"",
        "newline must be escaped as backslash-n (JSON.stringify)"
    );
    // tab
    assert_eq!(
        to_toml_value(&Value::String("a\tb".to_owned()), "k").unwrap(),
        "\"a\\tb\""
    );
    // carriage return
    assert_eq!(
        to_toml_value(&Value::String("a\rb".to_owned()), "k").unwrap(),
        "\"a\\rb\""
    );
}

/// Plain strings (no control chars) MUST match — this part of to_toml_value is correct.
#[test]
fn toml_value_plain_string_matches_json_stringify() {
    assert_eq!(
        to_toml_value(&Value::String("hello".to_owned()), "k").unwrap(),
        "\"hello\""
    );
    assert_eq!(
        to_toml_value(&Value::String("a\"b\\c".to_owned()), "k").unwrap(),
        "\"a\\\"b\\\\c\"",
        "quote + backslash escaping matches JSON.stringify"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// AREA 3 — parseCodexConfig defensive parse
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn config_defensive_parse_matrix() {
    // valid all-fields
    let r = parse_codex_config(&to_map(json!({
        "model": "gpt-5.2-codex",
        "modelReasoningEffort": "xhigh",
        "webSearchMode": "cached",
        "additionalDirectories": ["/x", 1, "/y"],
        "codexBinaryPath": "/bin/codex"
    })));
    assert_eq!(r.model.as_deref(), Some("gpt-5.2-codex"));
    assert_eq!(
        r.model_reasoning_effort,
        Some(ModelReasoningEffortCodex::Xhigh)
    );
    assert_eq!(r.web_search_mode, Some(WebSearchModeCodex::Cached));
    assert_eq!(
        r.additional_directories,
        Some(vec!["/x".to_owned(), "/y".to_owned()])
    );
    assert_eq!(r.codex_binary_path.as_deref(), Some("/bin/codex"));

    // wrong-typed → dropped; invalid enum → dropped
    let r2 = parse_codex_config(&to_map(json!({
        "model": 7, "modelReasoningEffort": "ultra", "webSearchMode": "streaming",
        "additionalDirectories": "/notarray", "codexBinaryPath": false
    })));
    assert!(r2.model.is_none());
    assert!(r2.model_reasoning_effort.is_none());
    assert!(r2.web_search_mode.is_none());
    assert!(r2.additional_directories.is_none());
    assert!(r2.codex_binary_path.is_none());

    // empty additionalDirectories array → Some(empty) (NO length guard, unlike Claude)
    let r3 = parse_codex_config(&to_map(json!({"additionalDirectories": []})));
    assert_eq!(r3.additional_directories, Some(vec![]));
}

// ════════════════════════════════════════════════════════════════════════════
// AREA 4 — normalizeJsonSchemaForOpenAiStrict (D2 golden tests vs bun oracle)
//
// Oracle: packages/providers/src/shared/structured-output.test.ts (2026-06-21).
// Provider.ts:310 calls normalizeJsonSchemaForOpenAiStrict before writing
// --output-schema; without it OpenAI strict-mode returns HTTP 400.
// ════════════════════════════════════════════════════════════════════════════

/// Oracle: top-level object schema gets additionalProperties:false injected.
/// structured-output.test.ts:194-201
#[test]
fn normalizer_top_level_object_gets_closed() {
    let schema = json!({
        "type": "object",
        "properties": {"a": {"type": "string"}},
        "required": ["a"]
    });
    let result = normalize_json_schema_for_openai_strict(schema.as_object().unwrap());
    assert_eq!(
        result.get("additionalProperties"),
        Some(&Value::Bool(false))
    );
}

/// Oracle: nested object properties are also closed.
/// structured-output.test.ts:203-212
#[test]
fn normalizer_recurses_into_nested_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "inner": {"type": "object", "properties": {"b": {"type": "number"}}}
        }
    });
    let result = normalize_json_schema_for_openai_strict(schema.as_object().unwrap());
    assert_eq!(
        result.get("additionalProperties"),
        Some(&Value::Bool(false))
    );
    let inner = result["properties"]["inner"].as_object().unwrap();
    assert_eq!(inner.get("additionalProperties"), Some(&Value::Bool(false)));
}

/// Oracle: array items that are object schemas are closed.
/// structured-output.test.ts:214-220
#[test]
fn normalizer_recurses_into_array_items() {
    let schema = json!({
        "type": "array",
        "items": {"type": "object", "properties": {"c": {"type": "string"}}}
    });
    let result = normalize_json_schema_for_openai_strict(schema.as_object().unwrap());
    let items = result["items"].as_object().unwrap();
    assert_eq!(items.get("additionalProperties"), Some(&Value::Bool(false)));
}

/// Oracle: $defs and anyOf composition keywords are recursed into.
/// structured-output.test.ts:222-232
#[test]
fn normalizer_recurses_into_defs_and_any_of() {
    let schema = json!({
        "$defs": {"Foo": {"type": "object", "properties": {"x": {"type": "string"}}}},
        "anyOf": [{"type": "object", "properties": {"y": {"type": "string"}}}]
    });
    let result = normalize_json_schema_for_openai_strict(schema.as_object().unwrap());
    let foo = result["$defs"]["Foo"].as_object().unwrap();
    assert_eq!(foo.get("additionalProperties"), Some(&Value::Bool(false)));
    let any0 = result["anyOf"][0].as_object().unwrap();
    assert_eq!(any0.get("additionalProperties"), Some(&Value::Bool(false)));
}

/// Oracle: open-record detection — additionalProperties: {type:'string'} is open.
/// structured-output.test.ts:249-260
#[test]
fn has_open_additional_properties_detects_open_and_closed() {
    // Open: typed additionalProperties subschema
    assert!(has_open_additional_properties(&json!({
        "type": "object",
        "properties": {},
        "additionalProperties": {"type": "string"}
    })));
    // Closed: additionalProperties: false → not open
    assert!(!has_open_additional_properties(&json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })));
    // Non-object node → not open
    assert!(!has_open_additional_properties(&json!({"type": "string"})));
}

/// Oracle: existing additionalProperties subschema is replaced with false (not merged).
#[test]
fn normalizer_replaces_typed_additional_properties_with_false() {
    let schema = json!({
        "type": "object",
        "properties": {"key": {"type": "string"}},
        "additionalProperties": {"type": "number"}
    });
    let result = normalize_json_schema_for_openai_strict(schema.as_object().unwrap());
    assert_eq!(
        result.get("additionalProperties"),
        Some(&Value::Bool(false))
    );
    // Input NOT mutated
    assert_eq!(schema["additionalProperties"], json!({"type": "number"}));
}

// ════════════════════════════════════════════════════════════════════════════
// AREA 5 — classify_codex_error + model-access message (BYTE-EXACT vs bun oracle)
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_codex_error_all_classes() {
    // model_access takes precedence over rate_limit etc.
    assert_eq!(
        classify_codex_error("model not available"),
        CodexErrorClass::ModelAccess
    );
    assert_eq!(
        classify_codex_error("Model not found"),
        CodexErrorClass::ModelAccess
    );
    assert_eq!(
        classify_codex_error("model access denied"),
        CodexErrorClass::ModelAccess
    );
    assert_eq!(
        classify_codex_error("rate limit hit"),
        CodexErrorClass::RateLimit
    );
    assert_eq!(classify_codex_error("429"), CodexErrorClass::RateLimit);
    assert_eq!(
        classify_codex_error("overloaded"),
        CodexErrorClass::RateLimit
    );
    assert_eq!(
        classify_codex_error("too many requests"),
        CodexErrorClass::RateLimit
    );
    assert_eq!(
        classify_codex_error("credit balance too low"),
        CodexErrorClass::Auth
    );
    assert_eq!(classify_codex_error("401"), CodexErrorClass::Auth);
    assert_eq!(classify_codex_error("403 forbidden"), CodexErrorClass::Auth);
    assert_eq!(classify_codex_error("invalid token"), CodexErrorClass::Auth);
    assert_eq!(
        classify_codex_error("codex exec exited with code 1"),
        CodexErrorClass::Crash
    );
    assert_eq!(classify_codex_error("killed"), CodexErrorClass::Crash);
    assert_eq!(
        classify_codex_error("signal SIGTERM"),
        CodexErrorClass::Crash
    );
    assert_eq!(
        classify_codex_error("something weird"),
        CodexErrorClass::Unknown
    );
}

/// BYTE-EXACT vs bun oracle (2026-06-21). `\`-continuation gotcha checked:
/// the backtick `model:` literal carries no leading indent.
#[test]
fn model_access_message_byte_exact_with_fallback() {
    let got = build_model_access_message(Some("gpt-5.3-codex"));
    let expected = "\u{274C} Model \"gpt-5.3-codex\" is not available for your account.\n\nTo fix: update your model in ~/.archon/config.yaml:\n  assistants:\n    codex:\n      model: gpt-5.2-codex\n\nOr set it per-workflow with `model: gpt-5.2-codex` in workflow YAML.";
    assert_eq!(got, expected);
}

#[test]
fn model_access_message_byte_exact_no_fallback() {
    let got = build_model_access_message(Some("unknown-model-x"));
    let expected = "\u{274C} Model \"unknown-model-x\" is not available for your account.\n\nTo fix: update your model in ~/.archon/config.yaml to one your account can access.\n\nOr set it per-workflow with a valid `model:` in workflow YAML.";
    assert_eq!(got, expected);
}

#[test]
fn model_access_message_byte_exact_none() {
    let got = build_model_access_message(None);
    let expected = "\u{274C} Model \"the configured model\" is not available for your account.\n\nTo fix: update your model in ~/.archon/config.yaml to one your account can access.\n\nOr set it per-workflow with a valid `model:` in workflow YAML.";
    assert_eq!(got, expected);
}

/// trim() applies before fallback lookup: '  gpt-5.3-codex  ' → fallback hit.
#[test]
fn model_access_message_trims_before_fallback() {
    let got = build_model_access_message(Some("  gpt-5.3-codex  "));
    assert!(got.contains("Model \"gpt-5.3-codex\""));
    assert!(got.contains("model: gpt-5.2-codex"));
}

// ════════════════════════════════════════════════════════════════════════════
// AREA 6 — MCP config: headers→http_headers remap + passthrough keys
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn mcp_headers_remapped_to_http_headers() {
    let mut servers = Map::new();
    servers.insert(
        "api".to_owned(),
        json!({"url": "https://e", "headers": {"X": "y"}}),
    );
    let ov = build_codex_mcp_config_overrides(&servers).unwrap();
    let api = ov["mcp_servers"]["api"].as_object().unwrap();
    assert!(api.contains_key("http_headers"));
    assert!(!api.contains_key("headers"));
}

#[test]
fn mcp_explicit_http_headers_not_overwritten_by_headers() {
    // Source provider.ts:181 — only remap `headers` when http_headers NOT already present.
    let mut servers = Map::new();
    servers.insert(
        "api".to_owned(),
        json!({"http_headers": {"A": "1"}, "headers": {"B": "2"}}),
    );
    let ov = build_codex_mcp_config_overrides(&servers).unwrap();
    let api = ov["mcp_servers"]["api"].as_object().unwrap();
    // http_headers kept from explicit value, headers NOT merged in
    assert_eq!(api["http_headers"]["A"], json!("1"));
    assert!(api["http_headers"].as_object().unwrap().get("B").is_none());
}

#[test]
fn mcp_all_22_passthrough_keys_preserved() {
    let keys = [
        "command",
        "args",
        "env",
        "url",
        "enabled",
        "required",
        "startup_timeout_sec",
        "startup_timeout_ms",
        "tool_timeout_sec",
        "enabled_tools",
        "disabled_tools",
        "supports_parallel_tool_calls",
        "cwd",
        "env_vars",
        "experimental_environment",
        "http_headers",
        "env_http_headers",
        "oauth_resource",
        "scopes",
        "bearer_token_env_var",
        "default_tools_approval_mode",
        "tools",
    ];
    assert_eq!(keys.len(), 22);
    let mut cfg = Map::new();
    for (i, k) in keys.iter().enumerate() {
        cfg.insert((*k).to_owned(), json!(format!("v{i}")));
    }
    let mut servers = Map::new();
    servers.insert("s".to_owned(), Value::Object(cfg));
    let ov = build_codex_mcp_config_overrides(&servers).unwrap();
    let s = ov["mcp_servers"]["s"].as_object().unwrap();
    for k in keys {
        assert!(s.contains_key(k), "passthrough key {k} dropped");
    }
}

#[test]
fn mcp_unknown_keys_dropped() {
    let mut servers = Map::new();
    servers.insert(
        "s".to_owned(),
        json!({"url": "u", "unknown_future_key": "x"}),
    );
    let ov = build_codex_mcp_config_overrides(&servers).unwrap();
    let s = ov["mcp_servers"]["s"].as_object().unwrap();
    assert!(s.contains_key("url"));
    assert!(!s.contains_key("unknown_future_key"));
}

// ════════════════════════════════════════════════════════════════════════════
// AREA 2 — parse_codex_stream event normalization (representative + edges)
// ════════════════════════════════════════════════════════════════════════════

fn parse(ev: Value, state: &mut CodexStreamState, hof: bool, smc: bool) -> Vec<MessageChunk> {
    parse_codex_event(&to_map(ev), state, hof, smc).into_chunks()
}

#[test]
fn stream_full_happy_sequence() {
    let mut s = CodexStreamState::new(None);
    // thread.started → sets id, no chunk
    assert!(parse(
        json!({"type":"thread.started","thread_id":"T1"}),
        &mut s,
        false,
        false
    )
    .is_empty());
    assert_eq!(s.resolved_thread_id.as_deref(), Some("T1"));
    // agent_message → assistant
    let c = parse(
        json!({"type":"item.completed","item":{"type":"agent_message","id":"a","text":"hi"}}),
        &mut s,
        false,
        false,
    );
    assert!(matches!(&c[0], MessageChunk::Assistant{content,..} if content=="hi"));
    // turn.completed → terminal result, session from thread.started
    let r = parse_codex_event(
        &to_map(json!({"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":4}})),
        &mut s,
        false,
        false,
    );
    assert!(r.is_terminal());
    let c = r.into_chunks();
    assert!(
        matches!(&c[0], MessageChunk::Result{session_id:Some(sid),tokens:Some(t),is_error:None,..} if sid=="T1" && t.input==3 && t.output==4)
    );
}

#[test]
fn stream_command_execution_exit_code_suffix() {
    let mut s = CodexStreamState::new(Some("T"));
    let c = parse(
        json!({"type":"item.completed","item":{"type":"command_execution","id":"c","command":"ls","aggregated_output":"out\n","exit_code":2}}),
        &mut s,
        false,
        false,
    );
    assert!(
        matches!(&c[1], MessageChunk::ToolResult{tool_output,..} if tool_output=="out\n\n[exit code: 2]")
    );
}

#[test]
fn stream_turn_failed_terminal_error() {
    let mut s = CodexStreamState::new(Some("T"));
    let r = parse_codex_event(
        &to_map(json!({"type":"turn.failed","error":{"message":"boom"}})),
        &mut s,
        false,
        false,
    );
    assert!(r.is_terminal());
    let c = r.into_chunks();
    assert!(
        matches!(&c[0], MessageChunk::Result{is_error:Some(true),error_subtype:Some(st),errors:Some(e),..} if st=="codex_turn_failed" && e[0]=="boom")
    );
}

#[test]
fn stream_error_event_mcp_vs_non_mcp() {
    let mut s = CodexStreamState::new(None);
    // non-mcp error captured
    parse(
        json!({"type":"error","message":"model not available"}),
        &mut s,
        false,
        false,
    );
    assert_eq!(s.last_non_mcp_error.as_deref(), Some("model not available"));
    // mcp client error NOT captured, surfaced only when flag set
    let mut s2 = CodexStreamState::new(None);
    let c = parse(
        json!({"type":"error","message":"MCP client failed"}),
        &mut s2,
        false,
        true,
    );
    assert!(
        matches!(&c[0], MessageChunk::System{content} if content.contains("MCP client failed"))
    );
    assert!(s2.last_non_mcp_error.is_none());
}

#[test]
fn stream_unknown_item_type_and_event_silently_ignored() {
    let mut s = CodexStreamState::new(Some("T"));
    assert!(parse(
        json!({"type":"item.completed","item":{"type":"mystery_item","id":"x"}}),
        &mut s,
        false,
        false
    )
    .is_empty());
    assert!(parse(json!({"type":"some.unknown.event"}), &mut s, false, false).is_empty());
}

#[test]
fn stream_structured_output_valid_and_invalid() {
    // valid JSON → structured_output Some, single terminal
    let mut s = CodexStreamState::new(Some("T"));
    parse(
        json!({"type":"item.completed","item":{"type":"agent_message","id":"a","text":"{\"ok\":true}"}}),
        &mut s,
        true,
        false,
    );
    let r = parse_codex_event(
        &to_map(json!({"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}})),
        &mut s,
        true,
        false,
    );
    let c = r.into_chunks();
    assert!(matches!(
        c.last().unwrap(),
        MessageChunk::Result {
            structured_output: Some(_),
            ..
        }
    ));

    // invalid JSON → warning System chunk THEN result with structured_output None
    let mut s2 = CodexStreamState::new(Some("T"));
    parse(
        json!({"type":"item.completed","item":{"type":"agent_message","id":"a","text":"not json"}}),
        &mut s2,
        true,
        false,
    );
    let r2 = parse_codex_event(
        &to_map(json!({"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}})),
        &mut s2,
        true,
        false,
    );
    assert!(matches!(r2, ParseResult::TerminalWithPreamble(_)));
    let c2 = r2.into_chunks();
    assert_eq!(c2.len(), 2);
    assert!(
        matches!(&c2[0], MessageChunk::System{content} if content.contains("Structured output requested"))
    );
    assert!(matches!(
        &c2[1],
        MessageChunk::Result {
            structured_output: None,
            ..
        }
    ));
}

// ── FIXED #3: UTF-8 char-boundary-safe truncation in warning preview ──────────
//
// parser.rs previously sliced `accumulated_text` by raw bytes which could panic
// when a multibyte char (e.g. 😀, 4 bytes) straddles byte index 200.
// Fix: the parser now uses `.chars().take(200).collect::<String>()` which is
// char-boundary-safe and matches the TS behavior (never panics).
//
// This test confirms:
//   1. The precondition still holds (byte 200 is mid-char), so the old code
//      would still panic — proving the fix is needed.
//   2. The char-based truncation (the fix) does NOT panic on the same input.
#[test]
fn structured_output_warning_preview_char_safe_truncation() {
    // Build a string where byte 200 is mid-char: 199 ASCII bytes + 1 multibyte char.
    let mut text = "a".repeat(199);
    text.push('😀'); // 4 bytes at byte offset 199..203 → [..200] is mid-char
    assert!(
        !text.is_char_boundary(text.len().min(200)),
        "precondition: byte 200 is still mid-char — old code would panic here"
    );
    // The fix: chars().take(200) never panics regardless of char boundaries.
    let preview: String = text.chars().take(200).collect();
    assert_eq!(
        preview.chars().count(),
        200,
        "should have exactly 200 chars"
    );
    // The preview should end with the emoji (the 200th char), not panic.
    assert!(preview.ends_with('😀'));
}
