//! Differential parity for the cycle-13 argv builder (`build_claude_argv`, PR-03).
//!
//! The TS SOURCE builds an SDK `Options` object; the Rust port builds the CLI argv.
//! target-architecture.md §6.2 is the authoritative SDK-option → CLI-flag contract.
//! The `argv_oracle.ts` (transient, deleted from Archon after capture) built the REAL
//! SDK Options via the verbatim `buildBaseClaudeOptions` + `applyNodeConfig`, then applied
//! the §6.2 mapping (encoded once, independent of the Rust impl) to derive the expected
//! argv. Each `*.expected.json` here is that frozen oracle output.
//!
//! Comparison is by **flag/value pairing + presence** (set semantics), which catches a
//! wrong flag name, wrong value encoding, a dropped flag, or an extra flag — the four
//! divergence modes that make a wrong CLI invocation. `--no-env-file` ordering (must
//! precede `--print`) is asserted separately.
//!
//! See `.handoff/loop/findings/parity-cycle13.md` for the full trail.

use har_contract::{ClaudeProviderDefaults, NodeConfig, SendQueryOptions};
use har_provider::claude::argv::build_claude_argv;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/argv")
}

/// Split an argv vector into:
/// - flag/value pairs `(flag, value)` for `--flag value` shapes
/// - bare flags (no following value) — boolean flags like `--fork-session`
///
/// A token starting with `--` whose NEXT token also starts with `--` (or is the last
/// token) is a bare flag; otherwise it pairs with the next token.
fn argv_to_pair_set(argv: &[String]) -> (BTreeSet<(String, String)>, BTreeSet<String>) {
    let mut pairs = BTreeSet::new();
    let mut bare = BTreeSet::new();
    let mut i = 0;
    // transport flags `--output-format stream-json` etc. pair normally; `--print`,
    // `--verbose`, `--fork-session`, `--dangerously-skip-permissions` are bare.
    let known_bare = [
        "--print",
        "--verbose",
        "--fork-session",
        "--dangerously-skip-permissions",
        "--no-env-file",
    ];
    while i < argv.len() {
        let tok = &argv[i];
        if tok.starts_with("--") {
            if known_bare.contains(&tok.as_str()) {
                bare.insert(tok.clone());
                i += 1;
            } else if i + 1 < argv.len() && !argv[i + 1].starts_with("--") {
                pairs.insert((tok.clone(), argv[i + 1].clone()));
                i += 2;
            } else {
                bare.insert(tok.clone());
                i += 1;
            }
        } else {
            // stray value — record as a degenerate bare for visibility
            bare.insert(format!("<stray:{}>", tok));
            i += 1;
        }
    }
    (pairs, bare)
}

/// Recursively sort all object keys in a JSON value, producing a canonical form
/// that is independent of key insertion order.
///
/// NOTE: `serde_json` is built with the `preserve_order` feature workspace-wide, so
/// a plain reparse+reserialize PRESERVES insertion order and does NOT canonicalize.
/// JSON-valued flags whose source is an unordered `HashMap` (e.g. `--output-format-schema`
/// from `NodeConfig.output_format: HashMap<String, Value>`) therefore emit a
/// NON-DETERMINISTIC key order. We must deep-sort to get a stable comparison key —
/// otherwise the differential is flaky (passes/fails by `HashMap` seed). Key order is
/// semantically irrelevant for the JSON Schema / config objects these flags carry, so
/// sorting is a sound canonicalization, not a relaxation of the contract.
fn deep_sort_json(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut sorted = serde_json::Map::new();
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), deep_sort_json(&map[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(deep_sort_json).collect()),
        other => other.clone(),
    }
}

/// Canonicalize a `(flag, value)` pair where the value is JSON — reparse, deep-sort
/// object keys, and reserialize both sides so key-order / whitespace differences in
/// JSON-valued flags (`--agents`, `--thinking`, `--sandbox`, `--output-format-schema`)
/// don't false-fail.
fn canon_pair(flag: &str, value: &str) -> (String, String) {
    let json_flags = [
        "--agents",
        "--thinking",
        "--sandbox",
        "--output-format-schema",
    ];
    if json_flags.contains(&flag) {
        if let Ok(v) = serde_json::from_str::<Value>(value) {
            return (flag.to_owned(), deep_sort_json(&v).to_string());
        }
    }
    (flag.to_owned(), value.to_owned())
}

fn canon_set(pairs: BTreeSet<(String, String)>) -> BTreeSet<(String, String)> {
    pairs.into_iter().map(|(f, v)| canon_pair(&f, &v)).collect()
}

fn run_scenario(base: &str) {
    let dir = fixture_dir();
    let scenario_raw = fs::read_to_string(dir.join(format!("{base}.json")))
        .unwrap_or_else(|_| panic!("missing scenario {base}.json"));
    let scenario: Value = serde_json::from_str(&scenario_raw).expect("scenario JSON");
    let expected_raw = fs::read_to_string(dir.join(format!("{base}.expected.json")))
        .unwrap_or_else(|_| panic!("missing expected {base}.expected.json"));
    let expected: Value = serde_json::from_str(&expected_raw).expect("expected JSON");

    // Deserialize inputs from the scenario.
    let request_options: Option<SendQueryOptions> = scenario
        .get("requestOptions")
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()).expect("requestOptions deser"));
    let node_config: Option<NodeConfig> = scenario
        .get("nodeConfig")
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()).expect("nodeConfig deser"));
    let defaults: ClaudeProviderDefaults = scenario
        .get("assistantDefaults")
        .filter(|v| !v.is_null())
        .map(|v| serde_json::from_value(v.clone()).expect("assistantDefaults deser"))
        .unwrap_or_default();
    let cli_path = scenario.get("cliPath").and_then(|v| v.as_str());
    let resume = scenario.get("resumeSessionId").and_then(|v| v.as_str());
    let mcp_server_names: Vec<String> = scenario
        .get("mcpServerNames")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let mcp_missing_vars: Vec<String> = scenario
        .get("mcpMissingVars")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();

    let (argv, warnings) = build_claude_argv(
        request_options.as_ref(),
        node_config.as_ref(),
        &defaults,
        resume,
        cli_path,
        &mcp_server_names,
        &mcp_missing_vars,
        None,
    );

    // ── Differential 1: flag/value pair set + bare flag set ──
    let expected_argv: Vec<String> = expected
        .get("expectedArgv")
        .and_then(|v| v.as_array())
        .expect("expectedArgv array")
        .iter()
        .map(|v| v.as_str().expect("argv token str").to_owned())
        .collect();

    let (rust_pairs, rust_bare) = argv_to_pair_set(&argv);
    let (exp_pairs, exp_bare) = argv_to_pair_set(&expected_argv);
    let rust_pairs = canon_set(rust_pairs);
    let exp_pairs = canon_set(exp_pairs);

    assert_eq!(
        rust_pairs, exp_pairs,
        "\n=== ARGV PAIR DIVERGENCE in {base} ===\nRUST argv:     {:?}\nEXPECTED argv: {:?}\nRUST pairs:     {:?}\nEXPECTED pairs: {:?}\n",
        argv, expected_argv, rust_pairs, exp_pairs
    );
    assert_eq!(
        rust_bare, exp_bare,
        "\n=== ARGV BARE-FLAG DIVERGENCE in {base} ===\nRUST argv:     {:?}\nEXPECTED argv: {:?}\n",
        argv, expected_argv
    );

    // ── Differential 2: warnings (code set) ──
    let exp_warn_codes: BTreeSet<String> = expected
        .get("warnings")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|w| w.get("code").and_then(|c| c.as_str()).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let rust_warn_codes: BTreeSet<String> = warnings.iter().map(|w| w.code.clone()).collect();
    assert_eq!(
        rust_warn_codes, exp_warn_codes,
        "\n=== WARNING-CODE DIVERGENCE in {base} ===\nRUST: {:?}\nEXPECTED: {:?}\n",
        warnings, exp_warn_codes
    );

    // ── Differential 3: --no-env-file must precede --print when present ──
    if let Some(neo) = argv.iter().position(|a| a == "--no-env-file") {
        let print_pos = argv.iter().position(|a| a == "--print").expect("--print present");
        assert!(
            neo < print_pos,
            "{base}: --no-env-file must precede --print; argv={argv:?}"
        );
    }
}

macro_rules! argv_test {
    ($name:ident, $base:literal) => {
        #[test]
        fn $name() {
            run_scenario($base);
        }
    };
}

argv_test!(plain, "a01_plain");
argv_test!(model_from_request, "a02_model");
argv_test!(model_from_defaults, "a03_model_from_defaults");
argv_test!(model_and_fallback, "a04_model_fallback");
argv_test!(max_budget, "a05_maxbudget");
argv_test!(resume_and_fork, "a06_resume_fork");
argv_test!(setting_sources_user, "a07_setting_sources_user");
argv_test!(system_prompt_string, "a08_sysprompt_string");
argv_test!(system_prompt_preset_append, "a09_sysprompt_preset_append");
argv_test!(effort_thinking_sandbox_betas, "a10_effort_thinking_sandbox_betas");
argv_test!(output_format_node_config, "a11_output_format");
argv_test!(allowed_denied_tools, "a12_allowed_denied_tools");
argv_test!(mcp_present_env, "a13_mcp_present_env");
argv_test!(mcp_missing_env_warning, "a14_mcp_missing_env");
argv_test!(mcp_haiku_warning, "a15_mcp_haiku");
argv_test!(skills_to_agents, "a16_skills");
argv_test!(skills_with_tools, "a17_skills_with_tools");
argv_test!(inline_agents, "a18_inline_agents");
argv_test!(inline_agents_override_skills, "a19_inline_agents_override_skills");
argv_test!(js_cli_no_env_file, "a20_js_cli_noenvfile");
argv_test!(native_cli_no_no_env_file, "a21_native_cli_no_noenvfile");
argv_test!(request_output_format, "a22_request_output_format");
argv_test!(node_config_system_prompt_override, "a23_nodeconfig_sysprompt_override");
