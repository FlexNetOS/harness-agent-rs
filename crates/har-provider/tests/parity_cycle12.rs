//! Differential parity tests for ITERATE cycle 12 — Claude provider sub-units
//! PR-04 (binary-resolver), PR-05 (config parse), PR-06 (native-tools).
//!
//! These tests pin the Rust behavior to the golden outputs captured from the
//! live TypeScript source (`bun`, Archon v0.4.1) during cycle-12 verification.
//! The TS oracle (`__parity_oracle_c12.ts`) was transient and is deleted from
//! Archon after capture; its outputs are frozen here as the parity oracle.
//!
//! See `.handoff/loop/findings/parity-cycle12.md` for the full differential trail.

use har_contract::{NativeTool, SettingSource};
use har_provider::claude::binary_resolver::{
    resolve_claude_binary_path, CLAUDE_BINARY_NAME,
};
use har_provider::claude::config::parse_claude_config;
use har_provider::claude::native_tools::{build_archon_mcp_server, ToolFieldKind};
use serde_json::{json, Map, Value};
use serial_test::serial;
use std::env;
use std::fs;

// ─────────────────────────────────────────────────────────────────────────────
// PR-06 native-tools — canonical-shape differential vs the TS Zod introspection.
// Each case is `(name, input_schema, expected_canonical_json_from_TS)`.
// The expected strings are the EXACT golden outputs captured from bun.
// ─────────────────────────────────────────────────────────────────────────────

/// Render the Rust result of build_archon_mcp_server([tool]) into the same
/// canonical JSON shape the TS oracle emits, so the two can be string-compared.
fn render_native(input_schema: Value) -> String {
    let schema_map: std::collections::HashMap<String, Value> =
        serde_json::from_value(input_schema).unwrap();
    let tool = NativeTool {
        name: "manage_run".to_owned(),
        description: "test tool".to_owned(),
        input_schema: schema_map,
        handler: None,
    };
    match build_archon_mcp_server(&[tool]) {
        Ok(desc) => {
            let td = &desc.tools[0];
            let mut fields: Vec<Value> = td
                .fields
                .iter()
                .map(|f| {
                    let kind = match &f.kind {
                        ToolFieldKind::String => json!({"kind": "string"}),
                        ToolFieldKind::Boolean => json!({"kind": "boolean"}),
                        ToolFieldKind::StringEnum { values } => {
                            json!({"kind": "string_enum", "values": values})
                        }
                    };
                    json!({
                        "name": f.name,
                        "kind": kind,
                        "required": f.required,
                        "description": f.description,
                    })
                })
                .collect();
            // Oracle sorts fields by name to neutralize key-order nondeterminism.
            fields.sort_by(|a, b| {
                a["name"].as_str().unwrap().cmp(b["name"].as_str().unwrap())
            });
            let out = json!({
                "ok": true,
                "serverName": desc.name,
                "version": desc.version,
                "alwaysLoad": desc.always_load,
                "archonToolServer": "archon",
                "toolName": td.name,
                "toolDescription": td.description,
                "fields": fields,
            });
            serde_json::to_string(&out).unwrap()
        }
        Err(e) => serde_json::to_string(&json!({"ok": false, "error": e})).unwrap(),
    }
}

/// Compare against the TS golden by parsing both to Value (key-order agnostic).
fn assert_native(input_schema: Value, ts_golden: &str) {
    let rust = render_native(input_schema);
    let rv: Value = serde_json::from_str(&rust).unwrap();
    let tv: Value = serde_json::from_str(ts_golden).unwrap();
    assert_eq!(
        rv, tv,
        "\n native-tools DIVERGENCE\n  RUST: {rust}\n  TS:   {ts_golden}\n"
    );
}

#[test]
fn pr06_native_valid_full() {
    assert_native(
        json!({"type":"object","properties":{
            "action":{"type":"string","enum":["list","get"],"description":"the action"},
            "runId":{"type":"string"},
            "confirm":{"type":"boolean","description":"guard"}},"required":["action"]}),
        r#"{"ok":true,"serverName":"archon","version":"1.0.0","alwaysLoad":true,"archonToolServer":"archon","toolName":"manage_run","toolDescription":"test tool","fields":[{"name":"action","kind":{"kind":"string_enum","values":["list","get"]},"required":true,"description":"the action"},{"name":"confirm","kind":{"kind":"boolean"},"required":false,"description":"guard"},{"name":"runId","kind":{"kind":"string"},"required":false,"description":null}]}"#,
    );
}

#[test]
fn pr06_native_enum_no_type() {
    assert_native(
        json!({"type":"object","properties":{"a":{"enum":["x","y"]}},"required":["a"]}),
        r#"{"ok":true,"serverName":"archon","version":"1.0.0","alwaysLoad":true,"archonToolServer":"archon","toolName":"manage_run","toolDescription":"test tool","fields":[{"name":"a","kind":{"kind":"string_enum","values":["x","y"]},"required":true,"description":null}]}"#,
    );
}

#[test]
fn pr06_native_enum_with_nonstring_members() {
    // TS filters non-string enum members; keeps ["x","y"].
    assert_native(
        json!({"type":"object","properties":{"a":{"enum":["x",5,"y",true]}},"required":["a"]}),
        r#"{"ok":true,"serverName":"archon","version":"1.0.0","alwaysLoad":true,"archonToolServer":"archon","toolName":"manage_run","toolDescription":"test tool","fields":[{"name":"a","kind":{"kind":"string_enum","values":["x","y"]},"required":true,"description":null}]}"#,
    );
}

#[test]
fn pr06_native_enum_all_nonstring() {
    assert_native(
        json!({"type":"object","properties":{"a":{"enum":[1,2,3]}},"required":["a"]}),
        r#"{"ok":false,"error":"native tool schema: enum for 'a' must be non-empty strings"}"#,
    );
}

#[test]
fn pr06_native_empty_enum() {
    assert_native(
        json!({"type":"object","properties":{"a":{"enum":[]}},"required":["a"]}),
        r#"{"ok":false,"error":"native tool schema: enum for 'a' must be non-empty strings"}"#,
    );
}

#[test]
fn pr06_native_no_required_array() {
    assert_native(
        json!({"type":"object","properties":{"x":{"type":"string"},"y":{"type":"boolean"}}}),
        r#"{"ok":true,"serverName":"archon","version":"1.0.0","alwaysLoad":true,"archonToolServer":"archon","toolName":"manage_run","toolDescription":"test tool","fields":[{"name":"x","kind":{"kind":"string"},"required":false,"description":null},{"name":"y","kind":{"kind":"boolean"},"required":false,"description":null}]}"#,
    );
}

#[test]
fn pr06_native_required_nonstring_members() {
    // TS filters required to strings: ['x',7,null] -> ['x'].
    assert_native(
        json!({"type":"object","properties":{"x":{"type":"string"}},"required":["x",7,null]}),
        r#"{"ok":true,"serverName":"archon","version":"1.0.0","alwaysLoad":true,"archonToolServer":"archon","toolName":"manage_run","toolDescription":"test tool","fields":[{"name":"x","kind":{"kind":"string"},"required":true,"description":null}]}"#,
    );
}

#[test]
fn pr06_native_empty_properties() {
    assert_native(
        json!({"type":"object","properties":{},"required":[]}),
        r#"{"ok":true,"serverName":"archon","version":"1.0.0","alwaysLoad":true,"archonToolServer":"archon","toolName":"manage_run","toolDescription":"test tool","fields":[]}"#,
    );
}

#[test]
fn pr06_native_unsupported_number() {
    assert_native(
        json!({"type":"object","properties":{"n":{"type":"number"}},"required":[]}),
        r#"{"ok":false,"error":"native tool schema: unsupported type for 'n' (only string / string-enum / boolean)"}"#,
    );
}

#[test]
fn pr06_native_unsupported_object_prop() {
    assert_native(
        json!({"type":"object","properties":{"o":{"type":"object"}},"required":[]}),
        r#"{"ok":false,"error":"native tool schema: unsupported type for 'o' (only string / string-enum / boolean)"}"#,
    );
}

#[test]
fn pr06_native_prop_no_type_no_enum() {
    assert_native(
        json!({"type":"object","properties":{"weird":{"description":"no type"}},"required":[]}),
        r#"{"ok":false,"error":"native tool schema: unsupported type for 'weird' (only string / string-enum / boolean)"}"#,
    );
}

#[test]
fn pr06_native_non_object_schema_type() {
    assert_native(
        json!({"type":"string"}),
        r#"{"ok":false,"error":"native tool inputSchema must be an object schema with `properties`"}"#,
    );
}

#[test]
fn pr06_native_missing_properties() {
    assert_native(
        json!({"type":"object"}),
        r#"{"ok":false,"error":"native tool inputSchema must be an object schema with `properties`"}"#,
    );
}

#[test]
fn pr06_native_properties_null() {
    assert_native(
        json!({"type":"object","properties":null}),
        r#"{"ok":false,"error":"native tool inputSchema must be an object schema with `properties`"}"#,
    );
}

#[test]
fn pr06_native_enum_takes_precedence_over_type() {
    // prop has both type:boolean AND enum -> enum wins (TS checks enum first).
    assert_native(
        json!({"type":"object","properties":{"a":{"type":"boolean","enum":["x","y"],"description":"d"}},"required":[]}),
        r#"{"ok":true,"serverName":"archon","version":"1.0.0","alwaysLoad":true,"archonToolServer":"archon","toolName":"manage_run","toolDescription":"test tool","fields":[{"name":"a","kind":{"kind":"string_enum","values":["x","y"]},"required":false,"description":"d"}]}"#,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// PR-05 config — differential vs parseClaudeConfig.
// Each case is `(input, expected_canonical_json_from_TS)`.
// ─────────────────────────────────────────────────────────────────────────────

fn render_config(raw: Value) -> String {
    let map: Map<String, Value> = match raw {
        Value::Object(m) => m,
        _ => panic!(),
    };
    let r = parse_claude_config(&map);
    let mut out = Map::new();
    if let Some(m) = &r.model {
        out.insert("model".into(), json!(m));
    }
    if let Some(ss) = &r.setting_sources {
        let arr: Vec<&str> = ss
            .iter()
            .map(|s| match s {
                SettingSource::Project => "project",
                SettingSource::User => "user",
            })
            .collect();
        out.insert("settingSources".into(), json!(arr));
    }
    if let Some(cbp) = &r.claude_binary_path {
        out.insert("claudeBinaryPath".into(), json!(cbp));
    }
    serde_json::to_string(&Value::Object(out)).unwrap()
}

fn assert_config(raw: Value, ts_golden: &str) {
    let rust = render_config(raw);
    let rv: Value = serde_json::from_str(&rust).unwrap();
    let tv: Value = serde_json::from_str(ts_golden).unwrap();
    assert_eq!(
        rv, tv,
        "\n config DIVERGENCE\n  RUST: {rust}\n  TS:   {ts_golden}\n"
    );
}

#[test]
fn pr05_config_empty() {
    assert_config(json!({}), r#"{}"#);
}
#[test]
fn pr05_config_model_string() {
    assert_config(json!({"model":"claude-opus-4"}), r#"{"model":"claude-opus-4"}"#);
}
#[test]
fn pr05_config_model_nonstring() {
    assert_config(json!({"model":42}), r#"{}"#);
}
#[test]
fn pr05_config_ss_both() {
    assert_config(
        json!({"settingSources":["project","user"]}),
        r#"{"settingSources":["project","user"]}"#,
    );
}
#[test]
fn pr05_config_ss_project() {
    assert_config(json!({"settingSources":["project"]}), r#"{"settingSources":["project"]}"#);
}
#[test]
fn pr05_config_ss_user() {
    assert_config(json!({"settingSources":["user"]}), r#"{"settingSources":["user"]}"#);
}
#[test]
fn pr05_config_ss_invalid_only() {
    assert_config(json!({"settingSources":["invalid","nope"]}), r#"{}"#);
}
#[test]
fn pr05_config_ss_mixed() {
    assert_config(
        json!({"settingSources":["project","invalid","user"]}),
        r#"{"settingSources":["project","user"]}"#,
    );
}
#[test]
fn pr05_config_ss_empty() {
    assert_config(json!({"settingSources":[]}), r#"{}"#);
}
#[test]
fn pr05_config_ss_nonarray() {
    assert_config(json!({"settingSources":"project"}), r#"{}"#);
}
#[test]
fn pr05_config_ss_dup() {
    // TS does NOT dedup: ["user","user","project"] preserved verbatim.
    assert_config(
        json!({"settingSources":["user","user","project"]}),
        r#"{"settingSources":["user","user","project"]}"#,
    );
}
#[test]
fn pr05_config_ss_nonstring_members() {
    assert_config(
        json!({"settingSources":["project",5,true,"user"]}),
        r#"{"settingSources":["project","user"]}"#,
    );
}
#[test]
fn pr05_config_cbp_string() {
    assert_config(
        json!({"claudeBinaryPath":"/usr/local/bin/claude"}),
        r#"{"claudeBinaryPath":"/usr/local/bin/claude"}"#,
    );
}
#[test]
fn pr05_config_cbp_nonstring() {
    assert_config(json!({"claudeBinaryPath":true}), r#"{}"#);
}
#[test]
fn pr05_config_all_three() {
    assert_config(
        json!({"model":"claude-sonnet-4","settingSources":["project"],"claudeBinaryPath":"/home/u/.local/bin/claude"}),
        r#"{"model":"claude-sonnet-4","settingSources":["project"],"claudeBinaryPath":"/home/u/.local/bin/claude"}"#,
    );
}
#[test]
fn pr05_config_extra_keys() {
    assert_config(json!({"model":"m","unknownFutureProp":"x","anotherProp":99}), r#"{"model":"m"}"#);
}

// ─────────────────────────────────────────────────────────────────────────────
// PR-04 binary-resolver — on-disk fixtures, real path classification.
// The TS source's behavior is recreated here against the SAME on-disk inputs:
// these mirror the bun differential runs captured in parity-cycle12.md.
// `#[serial]` because CLAUDE_BIN_PATH is process-global.
// ─────────────────────────────────────────────────────────────────────────────

fn mk_file(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    fs::write(&p, b"#!/bin/sh\n").unwrap();
    p
}

#[test]
#[serial]
fn pr04_env_file_wins_in_both_modes() {
    let d = tempfile::tempdir().unwrap();
    let f = mk_file(d.path(), "claude");
    env::set_var("CLAUDE_BIN_PATH", &f);
    let bin = resolve_claude_binary_path(None, true).unwrap();
    let dev = resolve_claude_binary_path(None, false).unwrap();
    env::remove_var("CLAUDE_BIN_PATH");
    assert_eq!(bin, Some(f.clone()));
    assert_eq!(dev, Some(f)); // env honored in dev mode too
}

#[test]
#[serial]
fn pr04_env_precedence_over_config_and_autodetect() {
    let d = tempfile::tempdir().unwrap();
    let envf = mk_file(d.path(), "env-claude");
    let cfgf = mk_file(d.path(), "cfg-claude");
    env::set_var("CLAUDE_BIN_PATH", &envf);
    let r = resolve_claude_binary_path(Some(cfgf.to_str().unwrap()), true).unwrap();
    env::remove_var("CLAUDE_BIN_PATH");
    assert_eq!(r, Some(envf));
}

#[test]
#[serial]
fn pr04_env_missing_exact_error() {
    env::set_var("CLAUDE_BIN_PATH", "/nonexistent/cli.js");
    let e = resolve_claude_binary_path(None, true).unwrap_err();
    env::remove_var("CLAUDE_BIN_PATH");
    // EXACT TS text (sourceLabel = CLAUDE_BIN_PATH).
    let expected = "CLAUDE_BIN_PATH is set to \"/nonexistent/cli.js\" but the file does not exist.\n\
        Please verify the path points to the Claude Code executable (native binary\n\
        from the curl/PowerShell installer, or cli.js from an npm global install).";
    assert_eq!(e, expected);
}

#[test]
#[serial]
fn pr04_empty_env_falls_through_dev_none() {
    env::set_var("CLAUDE_BIN_PATH", "");
    let r = resolve_claude_binary_path(None, false).unwrap();
    env::remove_var("CLAUDE_BIN_PATH");
    assert_eq!(r, None); // empty string is unset; dev+no-env -> None
}

#[test]
#[serial]
fn pr04_dev_no_env_returns_none_ignores_config() {
    env::remove_var("CLAUDE_BIN_PATH");
    let d = tempfile::tempdir().unwrap();
    let cfgf = mk_file(d.path(), "claude");
    // config path provided but dev mode -> ignored -> None
    let r = resolve_claude_binary_path(Some(cfgf.to_str().unwrap()), false).unwrap();
    assert_eq!(r, None);
}

#[test]
#[serial]
fn pr04_config_file_binary_mode() {
    env::remove_var("CLAUDE_BIN_PATH");
    let d = tempfile::tempdir().unwrap();
    let cfgf = mk_file(d.path(), "claude");
    let r = resolve_claude_binary_path(Some(cfgf.to_str().unwrap()), true).unwrap();
    assert_eq!(r, Some(cfgf));
}

#[test]
#[serial]
fn pr04_config_missing_exact_error() {
    env::remove_var("CLAUDE_BIN_PATH");
    let e = resolve_claude_binary_path(Some("/nonexistent/cli.js"), true).unwrap_err();
    let expected = "assistants.claude.claudeBinaryPath is set to \"/nonexistent/cli.js\" but the file does not exist.\n\
        Please verify the path points to the Claude Code executable (native binary\n\
        from the curl/PowerShell installer, or cli.js from an npm global install).";
    assert_eq!(e, expected);
}

#[test]
#[serial]
fn pr04_env_dir_expands_to_inner_binary() {
    env::remove_var("CLAUDE_BIN_PATH");
    let d = tempfile::tempdir().unwrap();
    let inner = mk_file(d.path(), CLAUDE_BINARY_NAME);
    env::set_var("CLAUDE_BIN_PATH", d.path());
    let r = resolve_claude_binary_path(None, true).unwrap();
    env::remove_var("CLAUDE_BIN_PATH");
    assert_eq!(r, Some(inner));
}

#[test]
#[serial]
fn pr04_config_dir_expands_to_inner_binary() {
    env::remove_var("CLAUDE_BIN_PATH");
    let d = tempfile::tempdir().unwrap();
    let inner = mk_file(d.path(), CLAUDE_BINARY_NAME);
    let r = resolve_claude_binary_path(Some(d.path().to_str().unwrap()), true).unwrap();
    assert_eq!(r, Some(inner));
}

#[test]
#[serial]
fn pr04_config_dir_missing_inner_exact_error() {
    env::remove_var("CLAUDE_BIN_PATH");
    let d = tempfile::tempdir().unwrap(); // empty dir
    let e = resolve_claude_binary_path(Some(d.path().to_str().unwrap()), true).unwrap_err();
    let expected = format!(
        "assistants.claude.claudeBinaryPath is set to \"{}\", which is a directory, but it does not contain {}.\n\
         Please point this setting at the Claude Code executable itself (native binary\n\
         from the curl/PowerShell installer, or cli.js from an npm global install).",
        d.path().display(),
        CLAUDE_BINARY_NAME
    );
    assert_eq!(e, expected);
}

#[test]
#[serial]
fn pr04_env_dir_missing_inner_exact_error() {
    let d = tempfile::tempdir().unwrap();
    env::set_var("CLAUDE_BIN_PATH", d.path());
    let e = resolve_claude_binary_path(None, true).unwrap_err();
    env::remove_var("CLAUDE_BIN_PATH");
    let expected = format!(
        "CLAUDE_BIN_PATH is set to \"{}\", which is a directory, but it does not contain {}.\n\
         Please point this setting at the Claude Code executable itself (native binary\n\
         from the curl/PowerShell installer, or cli.js from an npm global install).",
        d.path().display(),
        CLAUDE_BINARY_NAME
    );
    assert_eq!(e, expected);
}

#[test]
#[serial]
fn pr04_install_instructions_exact_text_vs_ts_golden() {
    // DIFFERENTIAL: the EXACT install-instructions string was captured from the
    // live TS source via a forced autodetect-miss (`bun`, pathKind->'missing').
    // The golden lives at tests/fixtures/claude_install_instructions.golden.txt.
    //
    // We deterministically force the install-instructions branch regardless of host:
    // env unset, config None, and HOME pointed at an empty temp dir so the
    // autodetect path (~/.local/bin/claude) cannot exist. `directories::BaseDirs`
    // reads $HOME on Linux, so this reliably misses.
    let golden = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/claude_install_instructions.golden.txt"
    ))
    .expect("golden fixture present");
    // The shell capture may append exactly one trailing newline; the TS message
    // has none. Normalize a single trailing '\n' off the golden.
    let golden = golden.strip_suffix('\n').unwrap_or(&golden);

    env::remove_var("CLAUDE_BIN_PATH");
    let orig_home = env::var_os("HOME");
    let empty = tempfile::tempdir().unwrap(); // no .local/bin/claude inside
    env::set_var("HOME", empty.path());
    let result = resolve_claude_binary_path(None, true);
    // restore HOME before asserting
    match orig_home {
        Some(h) => env::set_var("HOME", h),
        None => env::remove_var("HOME"),
    }

    let err = result.expect_err(
        "binary mode with no env/config and a HOME without ~/.local/bin/claude must Err",
    );
    assert_eq!(
        err, golden,
        "\n install-instructions DRIFT vs TS golden\n  RUST:\n{err}\n  TS:\n{golden}\n"
    );
}
