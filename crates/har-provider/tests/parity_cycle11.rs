//! Differential parity test for PR-02 Provider Registry (ITERATE cycle 11).
//!
//! This is the NO-DOWNGRADE gate: it drives the LIVE `har-provider` registry through the
//! same scenarios as the source (`packages/providers/src/registry.ts` + per-provider
//! `capabilities.ts`), builds the same canonical JSON, and diffs it against the committed
//! source golden captured live from `bun` (1.3.14) — `tests/fixtures/parity_cycle11_source_golden.json`.
//!
//! The crate's unit tests are NOT the oracle; the bun-captured golden is. If the source
//! behavior changes, regenerate the golden from the transient oracle and re-diff.
//!
//! Behaviors covered (rollup):
//!   B1  register_provider duplicate THROWS exact message
//!   B1b registerBuiltinProviders twice IDEMPOTENT (no throw, no dup)
//!   B2  insertion order preserved
//!   B3  getAgentProvider/getProviderCapabilities(unknown) exact UnknownProviderError message
//!   B4  THE capability table (5 providers × 14 flags) — re-derived from source
//!   B5  registration sets + order + builtIn + ProviderInfo projection keys
//!   B6  clearRegistry semantics
//!   B7  isRegisteredProvider

use har_contract::{ProviderRegistration, StructuredOutputCapability};
use har_provider::*;
use serde_json::{json, Value};
use serial_test::serial;

/// The 14 capability flags in source declaration order, with their source (camelCase) JSON keys.
const FLAGS: [&str; 14] = [
    "sessionResume",
    "mcp",
    "hooks",
    "skills",
    "agents",
    "toolRestrictions",
    "structuredOutput",
    "envInjection",
    "costControl",
    "effortControl",
    "thinkingControl",
    "fallbackModel",
    "sandbox",
    "nativeTools",
];

fn noop_reg(id: &str, display_name: &str, built_in: bool) -> ProviderRegistration {
    ProviderRegistration {
        id: id.to_owned(),
        display_name: display_name.to_owned(),
        factory: Box::new(|| panic!("noop factory not callable in parity test")),
        capabilities: har_provider::CLAUDE_CAPABILITIES,
        built_in,
    }
}

/// Serialize a capability tier to the SOURCE wire form: "enforced" / "best-effort" / false (bool).
fn structured_to_source(c: &StructuredOutputCapability) -> Value {
    match c {
        StructuredOutputCapability::Enforced => json!("enforced"),
        StructuredOutputCapability::BestEffort => json!("best-effort"),
        // Source type is `'enforced' | 'best-effort' | false`: the unsupported tier is the JS
        // literal `false` (a boolean), NOT the string "none". None of the 5 providers use it.
        StructuredOutputCapability::None => json!(false),
    }
}

/// Build the capability-table row for one provider from the LIVE registry, keyed by source flag names.
fn cap_row(id: &str) -> Value {
    let caps = get_provider_capabilities(id).expect("registered");
    let mut row = serde_json::Map::new();
    for f in FLAGS {
        let v: Value = match f {
            "sessionResume" => json!(caps.session_resume),
            "mcp" => json!(caps.mcp),
            "hooks" => json!(caps.hooks),
            "skills" => json!(caps.skills),
            "agents" => json!(caps.agents),
            "toolRestrictions" => json!(caps.tool_restrictions),
            "structuredOutput" => structured_to_source(&caps.structured_output),
            "envInjection" => json!(caps.env_injection),
            "costControl" => json!(caps.cost_control),
            "effortControl" => json!(caps.effort_control),
            "thinkingControl" => json!(caps.thinking_control),
            "fallbackModel" => json!(caps.fallback_model),
            "sandbox" => json!(caps.sandbox),
            "nativeTools" => json!(caps.native_tools),
            _ => unreachable!(),
        };
        row.insert(f.to_owned(), v);
    }
    Value::Object(row)
}

/// Drive the LIVE Rust registry through every scenario and produce the same JSON shape
/// the source oracle produced.
fn build_rust_actual() -> Value {
    let mut out = serde_json::Map::new();

    // B1: duplicate THROWS exact message
    clear_registry();
    register_provider(noop_reg("dup", "Dup", false)).unwrap();
    let b1 = match register_provider(noop_reg("dup", "Dup2", false)) {
        Ok(()) => json!({ "threw": false }),
        Err(msg) => json!({ "threw": true, "message": msg }),
    };
    out.insert("b1_duplicate_throw".into(), b1);

    // B1b: registerBuiltinProviders twice IDEMPOTENT
    clear_registry();
    register_builtin_providers();
    register_builtin_providers(); // must not throw / dup (Rust fn is infallible — models no-throw)
    out.insert(
        "b1b_builtin_idempotent".into(),
        json!({ "threw": false, "count": get_registered_providers().len() }),
    );

    // B2: insertion order preserved
    clear_registry();
    register_provider(noop_reg("zebra", "Z", false)).unwrap();
    register_provider(noop_reg("alpha", "A", false)).unwrap();
    register_provider(noop_reg("mike", "M", false)).unwrap();
    out.insert(
        "b2_insertion_order".into(),
        json!(get_registered_providers()
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()),
    );

    // B3: getAgentProvider(unknown) + getProviderCapabilities(unknown) + empty-registry
    clear_registry();
    register_provider(noop_reg("a", "A", false)).unwrap();
    register_provider(noop_reg("b", "B", false)).unwrap();
    register_provider(noop_reg("c", "C", false)).unwrap();
    let agent_err = match get_agent_provider("zzz") {
        Ok(_) => panic!("expected UnknownProviderError"),
        Err(e) => e,
    };
    out.insert(
        "b3_get_agent_unknown".into(),
        json!({
            "threw": true,
            "name": "UnknownProviderError",
            "message": agent_err.to_string(),
            "isUnknownProviderError": true,
        }),
    );
    let caps_err = get_provider_capabilities("zzz").unwrap_err();
    out.insert(
        "b3_get_caps_unknown".into(),
        json!({ "threw": true, "message": caps_err.to_string() }),
    );
    clear_registry();
    let empty_err = match get_agent_provider("missing") {
        Ok(_) => panic!("expected UnknownProviderError"),
        Err(e) => e,
    };
    out.insert(
        "b3_empty_unknown".into(),
        json!({ "message": empty_err.to_string() }),
    );

    // B4: THE capability table (live, real registrations)
    clear_registry();
    register_builtin_providers();
    register_community_providers();
    let mut table = serde_json::Map::new();
    for id in ["claude", "codex", "copilot", "pi", "opencode"] {
        table.insert(id.to_owned(), cap_row(id));
    }
    out.insert("b4_capability_table".into(), Value::Object(table));

    // B5: registration sets + order + builtIn + ProviderInfo projection keys
    clear_registry();
    register_builtin_providers();
    register_community_providers();
    out.insert(
        "b5_full_order".into(),
        json!(get_provider_info_list()
            .iter()
            .map(|i| json!({ "id": i.id, "displayName": i.display_name, "builtIn": i.built_in }))
            .collect::<Vec<_>>()),
    );
    // ProviderInfo projection keys — serialize one entry and take its (renamed) keys, sorted.
    let info0 = &get_provider_info_list()[0];
    let info_val = serde_json::to_value(info0).unwrap();
    let mut keys: Vec<String> = info_val.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    out.insert("b5_info_keys".into(), json!(keys));

    clear_registry();
    register_community_providers();
    out.insert(
        "b5_community_order".into(),
        json!(get_registered_providers()
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()),
    );

    // B6: clearRegistry semantics
    clear_registry();
    register_builtin_providers();
    register_community_providers();
    let before = get_registered_providers().len();
    clear_registry();
    out.insert(
        "b6_clear".into(),
        json!({
            "before": before,
            "after": get_registered_providers().len(),
            "isRegisteredAfter": is_registered_provider("claude"),
        }),
    );

    // B7: isRegisteredProvider
    clear_registry();
    register_provider(noop_reg("here", "Here", true)).unwrap();
    out.insert(
        "b7_is_registered".into(),
        json!({
            "present": is_registered_provider("here"),
            "absent": is_registered_provider("nothere"),
        }),
    );

    Value::Object(out)
}

#[test]
#[serial]
fn differential_parity_against_bun_source_golden() {
    let golden: Value =
        serde_json::from_str(include_str!("fixtures/parity_cycle11_source_golden.json"))
            .expect("golden fixture parses");
    let actual = build_rust_actual();

    // Per-behavior diff (so a failure names the offending behavior + symbol).
    let keys = [
        "b1_duplicate_throw",
        "b1b_builtin_idempotent",
        "b2_insertion_order",
        "b3_get_agent_unknown",
        "b3_get_caps_unknown",
        "b3_empty_unknown",
        "b4_capability_table",
        "b5_full_order",
        "b5_info_keys",
        "b5_community_order",
        "b6_clear",
        "b7_is_registered",
    ];
    let mut mismatches = Vec::new();
    for k in keys {
        let g = golden.get(k).unwrap_or(&Value::Null);
        let a = actual.get(k).unwrap_or(&Value::Null);
        if g != a {
            mismatches.push(format!(
                "  [{k}]\n    source(bun): {g}\n    rust       : {a}"
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "DIFFERENTIAL PARITY MISMATCH (source bun ⇄ Rust):\n{}",
        mismatches.join("\n")
    );
}

/// Per-provider, per-flag capability diff with a precise failure on any single wrong flag.
/// This is the consumer-facing contract: a single wrong flag = a real capability downgrade = FAIL.
#[test]
#[serial]
fn differential_capability_table_every_flag() {
    let golden: Value =
        serde_json::from_str(include_str!("fixtures/parity_cycle11_source_golden.json")).unwrap();
    let g_table = golden.get("b4_capability_table").unwrap();

    clear_registry();
    register_builtin_providers();
    register_community_providers();

    let mut bad = Vec::new();
    for id in ["claude", "codex", "copilot", "pi", "opencode"] {
        let g_row = g_table.get(id).unwrap();
        let a_row = cap_row(id);
        for f in FLAGS {
            let gv = g_row.get(f).unwrap();
            let av = a_row.get(f).unwrap();
            if gv != av {
                bad.push(format!("{id}.{f}: source={gv} rust={av}"));
            }
        }
    }
    clear_registry();
    assert!(
        bad.is_empty(),
        "CAPABILITY DOWNGRADE — wrong flag(s):\n  {}",
        bad.join("\n  ")
    );
}

/// Edge: the unsupported structured-output tier serializes to the source `false` literal
/// (NOT the string "none"). Source type: `'enforced' | 'best-effort' | false`.
#[test]
fn structured_output_unsupported_tier_serializes_as_false_literal() {
    assert_eq!(
        structured_to_source(&StructuredOutputCapability::None),
        json!(false)
    );
    // And the contract's own serde wire form is the string "false" (the documented mapping).
    let wire = serde_json::to_value(StructuredOutputCapability::None).unwrap();
    assert_eq!(wire, json!("false"));
}
