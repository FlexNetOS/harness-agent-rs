//! Cycle-20 differential parity harness — Pi community provider (PR-09, ~2038 LOC).
//!
//! Oracle = the LIVE TypeScript source under
//! `meta/Archon/packages/providers/src/community/pi/` run through `bun 1.3.14`
//! (the REAL modules + the REAL `@earendil-works/pi-coding-agent` / `pi-ai` SDKs
//! installed in `packages/providers/node_modules`). Golden values below were
//! captured from that live run during cycle-20 verification (see
//! `.handoff/loop/findings/parity-cycle20.md`). The transient oracle scripts were
//! deleted; Archon is kept pristine.
//!
//! Areas: config, model_ref, event_bridge (map_pi_event/serialize/usage/result),
//! options_translator (thinking + tools), native_tools, session_resolver,
//! ui_context_stub, resource_loader cache, provider pre-seam + seam isolation.
//!
//! NOTE on intentional FAILs: the `divergences` module asserts the **TS-correct**
//! behavior captured from the live oracle. Where the Rust port diverges, those
//! tests FAIL — that is the gate doing its job (see the FAIL verdict + required
//! porter fixes in parity-cycle20.md). They are NOT `#[ignore]`d.

use har_contract::{AgentProvider, MessageChunk, NodeConfig, SendQueryOptions};
use har_provider::pi::config::parse_pi_config;
use har_provider::pi::event_bridge::{
    build_result_chunk, map_pi_event, serialize_tool_result, usage_to_tokens, PiAssistantMessage,
    PiEvent, PiUsage,
};
use har_provider::pi::model_ref::parse_pi_model_ref;
use har_provider::pi::native_tools::build_pi_native_tool_definitions;
use har_provider::pi::options_translator::{resolve_pi_thinking_level, resolve_pi_tools};
use har_provider::pi::session_resolver::{
    resolve_pi_session_logic, PiSessionEntry, SessionResolutionDecision,
};
use har_provider::pi::ui_context_stub::{create_archon_ui_bridge, ArchonUiContextSpec, NotifyType};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ── helpers ────────────────────────────────────────────────────────────────

fn to_map(v: Value) -> HashMap<String, Value> {
    match v {
        Value::Object(m) => m.into_iter().collect(),
        _ => HashMap::new(),
    }
}

/// Serialize a MessageChunk to its wire JSON (sorted keys for stable compare).
fn chunk_wire(c: &MessageChunk) -> Value {
    serde_json::to_value(c).expect("chunk serializes")
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 1 — config::parse_pi_config (defensive parse matrix)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn area1_config_defensive_matrix_matches_live_oracle() {
    // (raw input, expected wire-serialized PiProviderDefaults) — from live bun.
    let cases: Vec<(Value, Value)> = vec![
        (
            json!({"model": "google/gemini-2.5-pro"}),
            json!({"model": "google/gemini-2.5-pro"}),
        ),
        (json!({"model": 123}), json!({})),
        (
            json!({"futureField": "x", "model": "google/gemini-2.5-pro"}),
            json!({"model": "google/gemini-2.5-pro"}),
        ),
        (json!({}), json!({})),
        (json!({"model": null}), json!({})),
        (json!({"model": []}), json!({})),
        (
            json!({"enableExtensions": true}),
            json!({"enableExtensions": true}),
        ),
        (
            json!({"enableExtensions": false}),
            json!({"enableExtensions": false}),
        ),
        (json!({"enableExtensions": "yes"}), json!({})),
        (json!({"enableExtensions": 1}), json!({})),
        (json!({"interactive": true}), json!({"interactive": true})),
        (json!({"interactive": 0}), json!({})),
        (
            json!({"extensionFlags": {"plan": true, "profile": "Default"}}),
            json!({"extensionFlags": {"plan": true, "profile": "Default"}}),
        ),
        (
            json!({"extensionFlags": {"plan": true, "bogus": 42, "nested": {"x": 1}, "nullish": null}}),
            json!({"extensionFlags": {"plan": true}}),
        ),
        (json!({"extensionFlags": {"bogus": 42}}), json!({})),
        (json!({"extensionFlags": "plan=true"}), json!({})),
        (json!({"extensionFlags": ["a"]}), json!({})),
        (
            json!({"env": {"FOO": "bar", "BOOL": true, "NUM": 42}}),
            json!({"env": {"FOO": "bar"}}),
        ),
        (json!({"env": {"NUM": 42}}), json!({})),
        (json!({"env": "X=1"}), json!({})),
        (json!({"maxConcurrent": 4}), json!({"maxConcurrent": 4})),
        (json!({"maxConcurrent": 1}), json!({"maxConcurrent": 1})),
        (json!({"maxConcurrent": 0}), json!({})),
        (json!({"maxConcurrent": -1}), json!({})),
        (json!({"maxConcurrent": 1.5}), json!({})),
        (json!({"maxConcurrent": 4.0}), json!({"maxConcurrent": 4})),
        (json!({"maxConcurrent": "four"}), json!({})),
        (json!({"maxConcurrent": null}), json!({})),
    ];

    for (raw, expected) in cases {
        let got = parse_pi_config(&to_map(raw.clone()));
        let got_wire = serde_json::to_value(&got).expect("serialize defaults");
        // Compare only the keys present in `expected` plus absence of others:
        // serialize both to canonical sorted JSON objects.
        assert_eq!(got_wire, expected, "config parse diverged for input {raw}");
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 2 — model_ref::parse_pi_model_ref
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn area2_model_ref_matches_live_oracle() {
    // (raw, Some((provider, model_id)) | None) — from live bun.
    let cases: Vec<(&str, Option<(&str, &str)>)> = vec![
        ("google/gemini-2.5-pro", Some(("google", "gemini-2.5-pro"))),
        (
            "openrouter/qwen/qwen3-coder",
            Some(("openrouter", "qwen/qwen3-coder")),
        ),
        (
            "openai-codex/gpt-5.1-codex-mini",
            Some(("openai-codex", "gpt-5.1-codex-mini")),
        ),
        ("google", None),
        ("/gemini", None),
        ("google/", None),
        ("Google/gemini", None),
        ("3google/gemini", None),
        ("open_ai/gpt", None),
        ("", None),
        ("grok2/fast", Some(("grok2", "fast"))),
        ("a/b", Some(("a", "b"))),
        ("//", None),
        ("a//b", Some(("a", "/b"))),
        ("a/", None),
        ("-foo/bar", None),
        ("foo-/bar", Some(("foo-", "bar"))),
        ("FOO/bar", None),
        ("ab.cd/x", None),
        ("a b/c", None),
        ("café/x", None),
    ];

    for (raw, expected) in cases {
        let got = parse_pi_model_ref(raw);
        match expected {
            None => assert!(got.is_none(), "expected None for {raw:?}, got {got:?}"),
            Some((p, m)) => {
                let r = got.unwrap_or_else(|| panic!("expected Some for {raw:?}"));
                assert_eq!(r.provider, p, "provider mismatch for {raw:?}");
                assert_eq!(r.model_id, m, "model_id mismatch for {raw:?}");
            }
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 3 — event_bridge: serialize_tool_result, usage_to_tokens, build_result_chunk
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn area3_serialize_tool_result_matches_live_oracle() {
    let cases: Vec<(Value, &str)> = vec![
        (json!("hello"), "hello"),
        (json!({"a": 1, "b": "x"}), r#"{"a":1,"b":"x"}"#),
        (json!([1, 2, 3]), "[1,2,3]"),
        (json!(42), "42"),
        (json!(true), "true"),
        (json!(null), "null"),
    ];
    for (input, expected) in cases {
        assert_eq!(
            serialize_tool_result(&input),
            expected,
            "serialize for {input}"
        );
    }
}

#[test]
fn area3_usage_to_tokens_matches_live_oracle() {
    // {input,output,total,cost}
    let u = PiUsage {
        input: 100,
        output: 50,
        total_tokens: 150,
        cost_total: 0.003,
    };
    let t = usage_to_tokens(&u);
    assert_eq!(t.input, 100);
    assert_eq!(t.output, 50);
    assert_eq!(t.total, Some(150));
    assert!((t.cost.unwrap() - 0.003).abs() < 1e-12);
}

#[test]
fn area3_build_result_chunk_matches_live_oracle() {
    // success: end_turn → tokens + cost + stopReason, no isError.
    let ok = PiAssistantMessage {
        usage: PiUsage {
            input: 10,
            output: 5,
            total_tokens: 15,
            cost_total: 0.001,
        },
        stop_reason: Some("end_turn".to_owned()),
        error_message: None,
        text_blocks: vec![],
    };
    let w = chunk_wire(&build_result_chunk(Some(&ok)));
    assert_eq!(
        w,
        json!({"type":"result","tokens":{"input":10,"output":5,"total":15,"cost":0.001},
               "cost":0.001,"stopReason":"end_turn"})
    );

    // error stopReason → isError, errorSubtype, errors[].
    let err = PiAssistantMessage {
        usage: PiUsage {
            input: 1,
            output: 0,
            total_tokens: 1,
            cost_total: 0.0,
        },
        stop_reason: Some("error".to_owned()),
        error_message: Some("Pi API failed".to_owned()),
        text_blocks: vec![],
    };
    let w = chunk_wire(&build_result_chunk(Some(&err)));
    assert_eq!(
        w,
        json!({"type":"result","tokens":{"input":1,"output":0,"total":1,"cost":0.0},
               "cost":0.0,"stopReason":"error","isError":true,"errorSubtype":"error",
               "errors":["Pi API failed"]})
    );

    // missing assistant → isError + missing_assistant_message.
    let w = chunk_wire(&build_result_chunk(None));
    assert_eq!(
        w,
        json!({"type":"result","isError":true,"errorSubtype":"missing_assistant_message"})
    );
}

// ── map_pi_event PASS cases (object args, deltas, tool_end, retry) ──────────

#[test]
fn area3_map_pi_event_object_args_matches_live_oracle() {
    let chunks = map_pi_event(&PiEvent::ToolExecutionStart {
        tool_name: "bash".to_owned(),
        args: json!({"command": "ls"}),
        tool_call_id: "c1".to_owned(),
    });
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        chunk_wire(&chunks[0]),
        json!({"type":"tool","toolName":"bash","toolInput":{"command":"ls"},"toolCallId":"c1"})
    );
}

#[test]
fn area3_map_pi_event_text_thinking_deltas() {
    assert_eq!(
        chunk_wire(
            &map_pi_event(&PiEvent::TextDelta {
                delta: "hello".to_owned()
            })[0]
        ),
        json!({"type":"assistant","content":"hello"})
    );
    assert_eq!(
        chunk_wire(
            &map_pi_event(&PiEvent::ThinkingDelta {
                delta: "r".to_owned()
            })[0]
        ),
        json!({"type":"thinking","content":"r"})
    );
}

#[test]
fn area3_map_pi_event_tool_end_and_retry() {
    // success tool_end → single tool_result
    let chunks = map_pi_event(&PiEvent::ToolExecutionEnd {
        tool_name: "bash".to_owned(),
        result: json!("output"),
        tool_call_id: "c1".to_owned(),
        is_error: false,
    });
    assert_eq!(chunks.len(), 1);
    assert_eq!(
        chunk_wire(&chunks[0]),
        json!({"type":"tool_result","toolName":"bash","toolOutput":"output","toolCallId":"c1"})
    );

    // error tool_end → system (⚠️) then tool_result
    let chunks = map_pi_event(&PiEvent::ToolExecutionEnd {
        tool_name: "bash".to_owned(),
        result: json!("erroutput"),
        tool_call_id: "c1".to_owned(),
        is_error: true,
    });
    assert_eq!(chunks.len(), 2);
    assert_eq!(
        chunk_wire(&chunks[0]),
        json!({"type":"system","content":"\u{26A0}\u{FE0F} Tool bash failed"})
    );

    // auto_retry_start → system "⚠️ retry a/b: msg"
    let chunks = map_pi_event(&PiEvent::AutoRetryStart {
        attempt: 2,
        max_attempts: 3,
        error_message: "rate limit".to_owned(),
    });
    assert_eq!(
        chunk_wire(&chunks[0]),
        json!({"type":"system","content":"\u{26A0}\u{FE0F} retry 2/3: rate limit"})
    );

    // skipped events
    assert!(map_pi_event(&PiEvent::TurnStart).is_empty());
    assert!(map_pi_event(&PiEvent::MessageUpdateOther).is_empty());
    assert!(map_pi_event(&PiEvent::Other).is_empty());
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 4 — options_translator: thinking + tools
// ══════════════════════════════════════════════════════════════════════════

fn nc_thinking(v: Value) -> NodeConfig {
    NodeConfig {
        thinking: Some(v),
        ..Default::default()
    }
}
fn nc_effort(s: &str) -> NodeConfig {
    NodeConfig {
        effort: Some(s.to_owned()),
        ..Default::default()
    }
}

#[test]
fn area4_thinking_level_matches_live_oracle() {
    // (NodeConfig, expected level Option, expected warning-substring Option)
    assert_eq!(resolve_pi_thinking_level(None).level, None);
    assert_eq!(
        resolve_pi_thinking_level(Some(&NodeConfig::default())).level,
        None
    );

    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_thinking(json!("high")))).level,
        Some("high".into())
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_thinking(json!("xhigh")))).level,
        Some("xhigh".into())
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_thinking(json!("minimal")))).level,
        Some("minimal".into())
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_thinking(json!("max")))).level,
        Some("xhigh".into())
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_effort("medium"))).level,
        Some("medium".into())
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_effort("max"))).level,
        Some("xhigh".into())
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_effort("xhigh"))).level,
        Some("xhigh".into())
    );

    let nc = NodeConfig {
        thinking: Some(json!("high")),
        effort: Some("low".into()),
        ..Default::default()
    };
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc)).level,
        Some("high".into())
    );

    // off short-circuits
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_thinking(json!("off")))).level,
        None
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_effort("off"))).level,
        None
    );
    let nc = NodeConfig {
        thinking: Some(json!("off")),
        effort: Some("high".into()),
        ..Default::default()
    };
    assert_eq!(resolve_pi_thinking_level(Some(&nc)).level, None);
    let nc = NodeConfig {
        thinking: Some(json!("minimal")),
        effort: Some("off".into()),
        ..Default::default()
    };
    assert_eq!(resolve_pi_thinking_level(Some(&nc)).level, None);

    // valid thinking wins over invalid effort
    let nc = NodeConfig {
        thinking: Some(json!("low")),
        effort: Some("crushing".into()),
        ..Default::default()
    };
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc)).level,
        Some("low".into())
    );

    // null / number thinking → none, no warning
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_thinking(json!(null)))).level,
        None
    );
    assert_eq!(
        resolve_pi_thinking_level(Some(&nc_thinking(json!(123)))).warning,
        None
    );
}

#[test]
fn area4_thinking_warning_strings_byte_exact() {
    // object form → Claude-specific warning (byte-exact, incl. → arrow)
    let r = resolve_pi_thinking_level(Some(&nc_thinking(
        json!({"type":"enabled","budget_tokens":4000}),
    )));
    assert_eq!(r.level, None);
    assert_eq!(
        r.warning.as_deref(),
        Some(
            "Pi ignored `thinking` (object form is Claude-specific). \
              Use `effort: low|medium|high|max` in YAML (max \u{2192} xhigh on Pi)."
        ),
        "object-form warning must be byte-exact vs live bun"
    );

    // unknown string thinking
    let r = resolve_pi_thinking_level(Some(&nc_thinking(json!("ultra"))));
    assert_eq!(
        r.warning.as_deref(),
        Some(
            "Pi ignored unknown thinking level 'ultra'. \
              Valid: minimal, low, medium, high, xhigh, max, off."
        ),
        "unknown-thinking warning must be byte-exact vs live bun"
    );

    // unknown effort
    let r = resolve_pi_thinking_level(Some(&nc_effort("crushing")));
    assert_eq!(
        r.warning.as_deref(),
        Some(
            "Pi ignored unknown thinking level 'crushing'. \
              Valid: minimal, low, medium, high, xhigh, max, off."
        ),
    );
}

#[test]
fn area4_tools_selection_matches_live_oracle() {
    // (NodeConfig?, env?, expected tool-name list Option, expected unknown list)
    fn names(r: &har_provider::pi::options_translator::ResolvedTools) -> Option<Vec<String>> {
        r.tools
            .as_ref()
            .map(|v| v.iter().map(|s| s.name.clone()).collect())
    }

    // no restrictions, no env → None
    let r = resolve_pi_tools(None, None);
    assert_eq!(names(&r), None);
    assert!(r.unknown_tools.is_empty());

    // empty allowed → empty vec
    let nc = NodeConfig {
        allowed_tools: Some(vec![]),
        ..Default::default()
    };
    let r = resolve_pi_tools(Some(&nc), None);
    assert_eq!(names(&r), Some(vec![]));

    // allowed read,bash → preserve order
    let nc = NodeConfig {
        allowed_tools: Some(vec!["read".into(), "bash".into()]),
        ..Default::default()
    };
    assert_eq!(
        names(&resolve_pi_tools(Some(&nc), None)),
        Some(vec!["read".into(), "bash".into()])
    );

    // case-insensitive normalize
    let nc = NodeConfig {
        allowed_tools: Some(vec!["Read".into(), "BASH".into(), "Edit".into()]),
        ..Default::default()
    };
    assert_eq!(
        names(&resolve_pi_tools(Some(&nc), None)),
        Some(vec!["read".into(), "bash".into(), "edit".into()])
    );

    // unknown collected, order preserved
    let nc = NodeConfig {
        allowed_tools: Some(vec!["read".into(), "WebFetch".into(), "bash".into()]),
        ..Default::default()
    };
    let r = resolve_pi_tools(Some(&nc), None);
    assert_eq!(names(&r), Some(vec!["read".into(), "bash".into()]));
    assert_eq!(r.unknown_tools, vec!["WebFetch".to_owned()]);

    // dedupe
    let nc = NodeConfig {
        allowed_tools: Some(vec!["read".into(), "read".into(), "Read".into()]),
        ..Default::default()
    };
    assert_eq!(
        names(&resolve_pi_tools(Some(&nc), None)),
        Some(vec!["read".into()])
    );

    // denied alone → full set minus denied, PI_TOOL_NAMES order
    let nc = NodeConfig {
        denied_tools: Some(vec!["bash".into()]),
        ..Default::default()
    };
    assert_eq!(
        names(&resolve_pi_tools(Some(&nc), None)),
        Some(vec![
            "read".into(),
            "edit".into(),
            "write".into(),
            "grep".into(),
            "find".into(),
            "ls".into()
        ])
    );

    // denied multiple
    let nc = NodeConfig {
        denied_tools: Some(vec!["bash".into(), "write".into()]),
        ..Default::default()
    };
    assert_eq!(
        names(&resolve_pi_tools(Some(&nc), None)),
        Some(vec![
            "read".into(),
            "edit".into(),
            "grep".into(),
            "find".into(),
            "ls".into()
        ])
    );

    // allowed - denied
    let nc = NodeConfig {
        allowed_tools: Some(vec!["read".into(), "bash".into(), "edit".into()]),
        denied_tools: Some(vec!["bash".into()]),
        ..Default::default()
    };
    assert_eq!(
        names(&resolve_pi_tools(Some(&nc), None)),
        Some(vec!["read".into(), "edit".into()])
    );

    // unknown from both allow + deny, order allow-then-deny
    let nc = NodeConfig {
        allowed_tools: Some(vec!["read".into(), "UnknownA".into()]),
        denied_tools: Some(vec!["UnknownB".into()]),
        ..Default::default()
    };
    let r = resolve_pi_tools(Some(&nc), None);
    assert_eq!(names(&r), Some(vec!["read".into()]));
    assert_eq!(
        r.unknown_tools,
        vec!["UnknownA".to_owned(), "UnknownB".to_owned()]
    );

    // no restrictions + non-empty env → default 4 tools
    let mut env = HashMap::new();
    env.insert("DATABASE_URL".to_owned(), "postgres://x".to_owned());
    assert_eq!(
        names(&resolve_pi_tools(None, Some(&env))),
        Some(vec![
            "read".into(),
            "bash".into(),
            "edit".into(),
            "write".into()
        ])
    );

    // no restrictions + empty env → None
    let empty: HashMap<String, String> = HashMap::new();
    assert_eq!(names(&resolve_pi_tools(None, Some(&empty))), None);
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 5 — native_tools: validate + normalize (accept/reject)
// ══════════════════════════════════════════════════════════════════════════

fn native_tool(name: &str, schema: Value) -> har_contract::NativeTool {
    har_contract::NativeTool {
        name: name.to_owned(),
        description: format!("{name} tool"),
        input_schema: to_map(schema),
        handler: Some(Arc::new(|_p| Box::pin(async { "ok".to_owned() }))),
    }
}

#[test]
fn area5_native_tools_accept_and_reject() {
    // accepts: string / boolean / enum props
    let ok = native_tool(
        "manage_run",
        json!({"type":"object","properties":{
            "action":{"type":"string","description":"The action"},
            "enabled":{"type":"boolean"},
            "mode":{"type":"string","enum":["fast","slow"]}},
            "required":["action"]}),
    );
    let defs = build_pi_native_tool_definitions(&[ok]).expect("accept valid schema");
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "manage_run");
    assert_eq!(defs[0].label, "manage_run"); // label derived from name

    // rejects: non-object schema, missing properties, unsupported type, empty enum
    assert!(
        build_pi_native_tool_definitions(&[native_tool("bad", json!({"type":"string"}))]).is_err()
    );
    assert!(
        build_pi_native_tool_definitions(&[native_tool("bad", json!({"type":"object"}))]).is_err()
    );
    assert!(build_pi_native_tool_definitions(&[native_tool(
        "bad",
        json!({"type":"object","properties":{"x":{"type":"number"}}})
    )])
    .is_err());
    assert!(build_pi_native_tool_definitions(&[native_tool(
        "bad",
        json!({"type":"object","properties":{"x":{"type":"string","enum":[]}}})
    )])
    .is_err());

    // empty input → empty output
    assert!(build_pi_native_tool_definitions(&[]).unwrap().is_empty());
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 6 — session_resolver decision logic
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn area6_session_resolution_decision_logic() {
    // no resume → Fresh
    let r = resolve_pi_session_logic("/p", None, Some(&[]));
    assert!(!r.resume_failed);
    assert_eq!(
        r.decision,
        SessionResolutionDecision::Fresh { cwd: "/p".into() }
    );

    // empty resume id → Fresh (matches `!resumeSessionId` truthiness)
    let r = resolve_pi_session_logic("/p", Some(""), Some(&[]));
    assert!(matches!(
        r.decision,
        SessionResolutionDecision::Fresh { .. }
    ));

    // matching id → Open(path)
    let sessions = vec![
        PiSessionEntry {
            id: "a".into(),
            path: "/s/a.jsonl".into(),
        },
        PiSessionEntry {
            id: "b".into(),
            path: "/s/b.jsonl".into(),
        },
    ];
    let r = resolve_pi_session_logic("/p", Some("b"), Some(&sessions));
    assert!(!r.resume_failed);
    assert_eq!(
        r.decision,
        SessionResolutionDecision::Open {
            path: "/s/b.jsonl".into()
        }
    );

    // unmatched id → FreshWithFailedResume
    let r = resolve_pi_session_logic("/p", Some("zz"), Some(&sessions));
    assert!(r.resume_failed);
    assert!(matches!(
        r.decision,
        SessionResolutionDecision::FreshWithFailedResume { .. }
    ));

    // ENOENT list (None) with id → FreshWithFailedResume
    let r = resolve_pi_session_logic("/p", Some("x"), None);
    assert!(r.resume_failed);
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 8 — ui_context_stub::notify (icon dispatch + flush)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn area8_ui_context_notify_byte_exact() {
    let bridge = create_archon_ui_bridge();
    let captured: Arc<Mutex<Vec<MessageChunk>>> = Arc::new(Mutex::new(vec![]));
    let cc = captured.clone();
    bridge.set_emitter(Some(Box::new(move |c| cc.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(c))));
    let ctx = ArchonUiContextSpec::new(bridge);

    ctx.notify("PR review complete", NotifyType::Info);
    ctx.notify("rate limit", NotifyType::Warning);
    ctx.notify("fatal", NotifyType::Error);

    let chunks = captured.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(chunks.len(), 3);
    assert_eq!(
        chunk_wire(&chunks[0]),
        json!({"type":"assistant","content":"\n[pi extension \u{2139}\u{FE0F}] PR review complete\n","flush":true})
    );
    assert_eq!(
        chunk_wire(&chunks[1]),
        json!({"type":"assistant","content":"\n[pi extension \u{26A0}\u{FE0F}] rate limit\n","flush":true})
    );
    assert_eq!(
        chunk_wire(&chunks[2]),
        json!({"type":"assistant","content":"\n[pi extension \u{274C}] fatal\n","flush":true})
    );
}

// ══════════════════════════════════════════════════════════════════════════
//  AREA 9/10 — provider pre-seam side-effects + seam isolation
// ══════════════════════════════════════════════════════════════════════════

struct NoCancel;
impl har_contract::CancelToken for NoCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[tokio::test]
async fn area9_send_query_reaches_pi_rpc_client() {
    // Cycle-23: the pi_sdk_not_bound seam is now filled by run_pi_rpc_session.
    // Without PI_CODING_AGENT_CLI set, send_query reaches the real RPC client and
    // surfaces "pi_binary_not_found" (Pi CLI not configured) rather than the old
    // "pi_sdk_not_bound" stub error.
    use futures_util::StreamExt;
    har_provider::pi::provider::reset_pi_semaphore();
    har_provider::pi::provider::reset_resource_loader_cache();

    let provider = har_provider::pi::PiProvider::new();
    let opts = SendQueryOptions {
        model: Some("google/gemini-2.5-pro".to_owned()),
        ..Default::default()
    };
    let chunks: Vec<_> = provider
        .send_query(
            "hi".into(),
            "/tmp".into(),
            None,
            Some(opts),
            Arc::new(NoCancel),
        )
        .collect()
        .await;
    let result = chunks
        .iter()
        .find(|c| matches!(c, MessageChunk::Result { .. }))
        .expect("a result chunk");
    match result {
        MessageChunk::Result {
            is_error,
            error_subtype,
            ..
        } => {
            assert_eq!(*is_error, Some(true));
            // "pi_binary_not_found" = RPC client reached but PI_CODING_AGENT_CLI not set
            assert_eq!(error_subtype.as_deref(), Some("pi_binary_not_found"));
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn area9_send_query_missing_model_before_seam() {
    use futures_util::StreamExt;
    har_provider::pi::provider::reset_pi_semaphore();
    har_provider::pi::provider::reset_resource_loader_cache();
    let provider = har_provider::pi::PiProvider::new();
    let chunks: Vec<_> = provider
        .send_query("hi".into(), "/tmp".into(), None, None, Arc::new(NoCancel))
        .collect()
        .await;
    let result = chunks
        .iter()
        .find(|c| matches!(c, MessageChunk::Result { .. }))
        .unwrap();
    if let MessageChunk::Result { error_subtype, .. } = result {
        assert_eq!(error_subtype.as_deref(), Some("pi_model_missing"));
    }
}

// ══════════════════════════════════════════════════════════════════════════
//  DIVERGENCES — assert the TS-CORRECT behavior from the live oracle.
//  Where the Rust port diverges these FAIL → the gate's FAIL verdict.
//  See parity-cycle20.md. NOT #[ignore]d.
// ══════════════════════════════════════════════════════════════════════════

mod divergences {
    use super::*;

    /// D1a — `tool_execution_start` with NON-OBJECT (scalar/null) `args`.
    ///
    /// Live bun (event-bridge.ts:231-234, `typeof args==='object' && !==null ? args : {}`),
    /// and its own source test `coerces non-object args to empty record`, emit
    /// `toolInput: {}` (present, empty object on the wire).
    ///
    /// The Rust port maps non-object args to `tool_input: None`, which serializes
    /// with `skip_serializing_if = Option::is_none` → the `toolInput` key is
    /// OMITTED entirely. Wire-shape downgrade.
    #[test]
    fn d1a_non_object_args_must_emit_empty_object() {
        for args in [json!("rawstring"), json!(null), json!(42), json!(true)] {
            let chunks = map_pi_event(&PiEvent::ToolExecutionStart {
                tool_name: "bash".to_owned(),
                args: args.clone(),
                tool_call_id: "c".to_owned(),
            });
            let wire = chunk_wire(&chunks[0]);
            assert_eq!(
                wire,
                json!({"type":"tool","toolName":"bash","toolInput":{},"toolCallId":"c"}),
                "D1a: non-object args {args} must emit toolInput:{{}} (live bun), \
                 but Rust omits the key"
            );
        }
    }

    /// D1b — `tool_execution_start` with ARRAY `args`.
    ///
    /// Live bun: `typeof [] === 'object' && [] !== null` is TRUE, so the array
    /// passes through unchanged → `toolInput: [1,2]` on the wire.
    ///
    /// Rust: only `Value::Object` maps to `Some`; an array → `tool_input: None`
    /// → key omitted (and `tool_input: Option<HashMap>` cannot even hold an
    /// array). Wire-shape divergence + data loss.
    #[test]
    fn d1b_array_args_must_pass_through() {
        let chunks = map_pi_event(&PiEvent::ToolExecutionStart {
            tool_name: "bash".to_owned(),
            args: json!([1, 2]),
            tool_call_id: "c".to_owned(),
        });
        let wire = chunk_wire(&chunks[0]);
        assert_eq!(
            wire,
            json!({"type":"tool","toolName":"bash","toolInput":[1,2],"toolCallId":"c"}),
            "D1b: array args must pass through as toolInput (live bun), \
             but Rust omits the key + cannot represent an array"
        );
    }
}
