//! TRANSIENT parity oracle for WF-14 (model-validation). Emits canonical JSON
//! mirroring the TS oracle so the two can be diffed. Run with:
//!   cargo run -q --example parity_wf14_oracle -p har-dag-executor
//! Kept as a durable differential example (it doubles as a golden harness).

use std::collections::HashMap;

use har_dag_executor::model_validation::{
    build_ai_profile, is_literal_spec, resolve_model_spec, route_preset_effort, BuildAiProfileOptions,
    EffortField, ModelValidationError, RawAliasEntry, RawAliasesConfig, RawTiersConfig,
    ResolvedAiProfile, ResolvedModelSpec, CLAUDE_EFFORTS, CODEX_REASONING_EFFORTS, TIER_NAMES,
};
use har_workflow_schema::dag_node::ThinkingConfig;
use serde_json::{json, Value};

fn entry(provider: &str, model: &str) -> RawAliasEntry {
    RawAliasEntry {
        provider: provider.to_owned(),
        model: model.to_owned(),
        effort: None,
        thinking: None,
    }
}
fn entry_eff(provider: &str, model: &str, effort: &str) -> RawAliasEntry {
    RawAliasEntry {
        provider: provider.to_owned(),
        model: model.to_owned(),
        effort: Some(effort.to_owned()),
        thinking: None,
    }
}

fn map1(k: &str, e: RawAliasEntry) -> HashMap<String, RawAliasEntry> {
    let mut m = HashMap::new();
    m.insert(k.to_owned(), e);
    m
}

/// Normalize a ResolvedModelSpec to the same JSON shape as the TS oracle.
fn spec_json(s: &ResolvedModelSpec) -> Value {
    match s {
        ResolvedModelSpec::Literal { literal } => json!({"kind":"literal","literal":literal}),
        ResolvedModelSpec::Preset(p) => {
            let thinking = match &p.thinking {
                None => Value::Null,
                Some(t) => serde_json::to_value(t).unwrap(),
            };
            json!({
                "kind":"preset",
                "provider": p.provider,
                "model": p.model,
                "effort": p.effort.clone().map(Value::String).unwrap_or(Value::Null),
                "thinking": thinking,
            })
        }
    }
}

/// Map a preset (from aliases map) to the `{provider,model,effort?}` TS shape.
fn preset_json(p: &har_dag_executor::model_validation::ModelAliasPreset) -> Value {
    let mut o = serde_json::Map::new();
    o.insert("provider".into(), json!(p.provider));
    o.insert("model".into(), json!(p.model));
    if let Some(e) = &p.effort {
        o.insert("effort".into(), json!(e));
    }
    if let Some(t) = &p.thinking {
        o.insert("thinking".into(), serde_json::to_value(t).unwrap());
    }
    Value::Object(o)
}

fn err_msg(e: &ModelValidationError) -> String {
    e.to_string()
}

fn main() {
    let mut results: Vec<Value> = Vec::new();
    let push_ok = |buf: &mut Vec<Value>, case: &str, v: Value| {
        buf.push(json!({"case":case,"ok":true,"value":v}))
    };
    let push_err = |buf: &mut Vec<Value>, case: &str, msg: String| {
        buf.push(json!({"case":case,"ok":false,"error":msg}))
    };
    let buf = &mut results;

    // ---- constants ----
    push_ok(buf, "TIER_NAMES", json!(TIER_NAMES));
    let mut ce: Vec<&str> = CLAUDE_EFFORTS.to_vec();
    ce.sort();
    push_ok(buf, "CLAUDE_EFFORTS", json!(ce));
    let mut cre: Vec<&str> = CODEX_REASONING_EFFORTS.to_vec();
    cre.sort();
    push_ok(buf, "CODEX_REASONING_EFFORTS", json!(cre));

    // ---- profile seed per provider ----
    for prov in ["claude", "codex", "pi", "copilot", "opencode", "unknown-provider"] {
        let p = build_ai_profile(prov, BuildAiProfileOptions::default()).unwrap();
        let mut keys: Vec<&String> = p.aliases.keys().collect();
        keys.sort();
        let aliases: Vec<Value> = keys
            .iter()
            .map(|k| {
                let pr = &p.aliases[*k];
                let mut o = serde_json::Map::new();
                o.insert("key".into(), json!(k));
                if let Value::Object(m) = preset_json(pr) {
                    for (kk, vv) in m {
                        o.insert(kk, vv);
                    }
                }
                Value::Object(o)
            })
            .collect();
        push_ok(
            buf,
            &format!("profile.seed.{prov}"),
            json!({"defaultProvider":p.default_provider,"aliases":aliases}),
        );
    }

    // ---- resolve branches ----
    let claude = build_ai_profile("claude", BuildAiProfileOptions::default()).unwrap();
    let codex = build_ai_profile("codex", BuildAiProfileOptions::default()).unwrap();
    push_ok(buf, "resolve.claude.small", spec_json(&resolve_model_spec(&claude, "small").unwrap()));
    push_ok(buf, "resolve.claude.medium", spec_json(&resolve_model_spec(&claude, "medium").unwrap()));
    push_ok(buf, "resolve.claude.large", spec_json(&resolve_model_spec(&claude, "large").unwrap()));
    push_ok(buf, "resolve.codex.small", spec_json(&resolve_model_spec(&codex, "small").unwrap()));
    push_ok(buf, "resolve.codex.large", spec_json(&resolve_model_spec(&codex, "large").unwrap()));
    push_ok(
        buf,
        "resolve.literal.modelstring",
        spec_json(&resolve_model_spec(&claude, "claude-opus-4-7-20251101").unwrap()),
    );
    push_ok(buf, "resolve.literal.empty", spec_json(&resolve_model_spec(&claude, "").unwrap()));
    push_ok(buf, "resolve.literal.atless-word", spec_json(&resolve_model_spec(&claude, "mymodel").unwrap()));

    // fallback chains
    {
        let t: RawTiersConfig = map1("medium", entry("myprovider", "medium-model"));
        let p = build_ai_profile("myprovider", BuildAiProfileOptions { repo_tiers: Some(&t), ..Default::default() }).unwrap();
        push_ok(buf, "resolve.fallback.large_to_medium", spec_json(&resolve_model_spec(&p, "large").unwrap()));
    }
    {
        let t: RawTiersConfig = map1("large", entry("myprovider", "large-model"));
        let p = build_ai_profile("myprovider", BuildAiProfileOptions { repo_tiers: Some(&t), ..Default::default() }).unwrap();
        push_ok(buf, "resolve.fallback.medium_to_large", spec_json(&resolve_model_spec(&p, "medium").unwrap()));
    }
    {
        let t: RawTiersConfig = map1("small", entry("myprovider", "small-only"));
        let p = build_ai_profile("myprovider", BuildAiProfileOptions { repo_tiers: Some(&t), ..Default::default() }).unwrap();
        push_ok(buf, "resolve.fallback.medium_to_small", spec_json(&resolve_model_spec(&p, "medium").unwrap()));
    }
    {
        let p = build_ai_profile("ghost", BuildAiProfileOptions::default()).unwrap();
        match resolve_model_spec(&p, "small") {
            Ok(_) => push_ok(buf, "resolve.tier_not_configured", Value::Null),
            Err(e) => push_err(buf, "resolve.tier_not_configured", err_msg(&e)),
        }
    }
    {
        let a: RawAliasesConfig = map1("@fast", entry("claude", "haiku"));
        let p = build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }).unwrap();
        push_ok(buf, "resolve.alias.found", spec_json(&resolve_model_spec(&p, "@fast").unwrap()));
    }
    {
        let mut a: RawAliasesConfig = HashMap::new();
        a.insert("@zebra".into(), entry("claude", "haiku"));
        a.insert("@alpha".into(), entry("claude", "sonnet"));
        let p = build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }).unwrap();
        match resolve_model_spec(&p, "@nope") {
            Ok(_) => push_ok(buf, "resolve.alias.unknown_lists_keys", Value::Null),
            Err(e) => push_err(buf, "resolve.alias.unknown_lists_keys", err_msg(&e)),
        }
    }
    {
        let empty = ResolvedAiProfile { default_provider: "claude".into(), aliases: HashMap::new() };
        match resolve_model_spec(&empty, "@ghost") {
            Ok(_) => push_ok(buf, "resolve.alias.unknown_none", Value::Null),
            Err(e) => push_err(buf, "resolve.alias.unknown_none", err_msg(&e)),
        }
    }

    // ---- layering ----
    {
        let g: RawTiersConfig = map1("small", entry("g", "gm"));
        let r: RawTiersConfig = map1("small", entry("r", "rm"));
        let p = build_ai_profile("unknown", BuildAiProfileOptions { global_tiers: Some(&g), repo_tiers: Some(&r), ..Default::default() }).unwrap();
        push_ok(buf, "layer.repo_tier_beats_global", preset_json(p.aliases.get("small").unwrap()));
    }
    {
        let g: RawAliasesConfig = map1("@x", entry("claude", "haiku"));
        let r: RawAliasesConfig = map1("@x", entry("claude", "sonnet"));
        let p = build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&g), repo_aliases: Some(&r), ..Default::default() }).unwrap();
        push_ok(buf, "layer.repo_alias_beats_global", preset_json(p.aliases.get("@x").unwrap()));
    }
    {
        let g: RawTiersConfig = map1("small", entry_eff("codex", "gpt-5.5", "minimal"));
        let p = build_ai_profile("claude", BuildAiProfileOptions { global_tiers: Some(&g), ..Default::default() }).unwrap();
        push_ok(buf, "layer.global_tier_overrides_default", preset_json(p.aliases.get("small").unwrap()));
    }

    // ---- rejections ----
    let reject = |buf: &mut Vec<Value>, case: &str, res: Result<ResolvedAiProfile, ModelValidationError>| {
        match res {
            Ok(_) => buf.push(json!({"case":case,"ok":true,"value":"UNEXPECTED_OK"})),
            Err(e) => buf.push(json!({"case":case,"ok":false,"error":e.to_string()})),
        }
    };
    {
        let a: RawAliasesConfig = map1("small", entry("c", "m"));
        reject(buf, "reject.alias_reserved_small", build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }));
    }
    {
        let a: RawAliasesConfig = map1("medium", entry("c", "m"));
        reject(buf, "reject.alias_reserved_medium_repo", build_ai_profile("claude", BuildAiProfileOptions { repo_aliases: Some(&a), ..Default::default() }));
    }
    {
        let a: RawAliasesConfig = map1("large", entry("c", "m"));
        reject(buf, "reject.alias_reserved_large", build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }));
    }
    {
        let a: RawAliasesConfig = map1("myalias", entry("c", "m"));
        reject(buf, "reject.alias_missing_at", build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }));
    }
    {
        let a: RawAliasesConfig = map1("@test", entry("", "m"));
        reject(buf, "reject.alias_empty_provider", build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }));
    }
    {
        let a: RawAliasesConfig = map1("@test", entry("c", ""));
        reject(buf, "reject.alias_empty_model", build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }));
    }
    {
        let t: RawTiersConfig = map1("xlarge", entry("c", "m"));
        reject(buf, "reject.tier_invalid_name", build_ai_profile("claude", BuildAiProfileOptions { global_tiers: Some(&t), ..Default::default() }));
    }
    {
        let t: RawTiersConfig = map1("small", entry("", "m"));
        reject(buf, "reject.tier_empty_provider", build_ai_profile("claude", BuildAiProfileOptions { global_tiers: Some(&t), ..Default::default() }));
    }
    {
        let t: RawTiersConfig = map1("small", entry("c", ""));
        reject(buf, "reject.tier_empty_model", build_ai_profile("claude", BuildAiProfileOptions { global_tiers: Some(&t), ..Default::default() }));
    }

    // ---- routePresetEffort matrix ----
    let field_name = |f: &EffortField| match f {
        EffortField::Effort => "effort",
        EffortField::ModelReasoningEffort => "modelReasoningEffort",
    };
    for pr in ["claude", "codex", "pi", "unknown"] {
        for ef in ["low", "medium", "high", "max", "minimal", "xhigh", ""] {
            let label = if ef.is_empty() { "EMPTY" } else { ef };
            let v = match route_preset_effort(pr, ef) {
                None => Value::Null,
                Some(r) => json!({"field": field_name(&r.field), "value": r.value}),
            };
            push_ok(buf, &format!("route.{pr}.{label}"), v);
        }
    }

    // ---- isLiteralSpec ----
    push_ok(buf, "isLiteral.literal", json!(is_literal_spec(&ResolvedModelSpec::Literal { literal: "x".into() })));
    push_ok(
        buf,
        "isLiteral.preset",
        json!(is_literal_spec(&ResolvedModelSpec::Preset(har_dag_executor::model_validation::ModelAliasPreset {
            provider: "c".into(),
            model: "m".into(),
            effort: None,
            thinking: None,
        }))),
    );

    // ---- effort/thinking preservation (object form) ----
    {
        let mut e = entry_eff("claude", "opus", "high");
        e.thinking = Some(ThinkingConfig::Enabled { budget_tokens: Some(1024) });
        let a: RawAliasesConfig = map1("@deep", e);
        let p = build_ai_profile("claude", BuildAiProfileOptions { global_aliases: Some(&a), ..Default::default() }).unwrap();
        push_ok(buf, "preset.preserves_effort_thinking", spec_json(&resolve_model_spec(&p, "@deep").unwrap()));
    }

    print!("{}", serde_json::to_string(&results).unwrap());
}
