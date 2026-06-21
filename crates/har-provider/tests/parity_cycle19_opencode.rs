//! Cycle-19 differential parity harness — OpenCode community provider (PR-11).
//!
//! Oracle = live TypeScript source under `meta/Archon/packages/providers/src/community/opencode/`
//! run through `bun 1.3.14`. Golden values in this file were captured from that live run
//! (see `.handoff/loop/findings/parity-cycle19.md`). This harness diffs the Rust port against
//! those golden values, fail-closed.
//!
//! Areas: config, errors, tokens, agent_config, agent_fs (byte-exact), runtime, session,
//! multi_agent, seam isolation.
//!
//! NOTE on intentional FAILs: tests in the `divergences` module assert the **TS-correct**
//! behavior captured from the oracle. Where the Rust port diverges they FAIL — that is the
//! gate doing its job (see parity-cycle19.md for the FAIL verdict + required porter fixes).

use har_contract::{InlineAgentDefinition, MessageChunk, NodeConfig, SendQueryOptions};
use har_provider::opencode::agent_config::{
    adapt_named_agent_for_opencode, build_tools_permissions_map, get_ordered_agents,
    has_multiple_agents, list_named_agents, resolve_prompt_for_agent, to_kebab_case,
    NamedAgentConfig,
};
use har_provider::opencode::agent_fs::build_agent_file_content;
use har_provider::opencode::config::{parse_model_ref, parse_opencode_config, ProviderModel};
use har_provider::opencode::errors::{
    classify_opencode_error, enrich_opencode_error, RetryableErrorClass,
};
use har_provider::opencode::runtime::{
    extract_port_from_url, generate_random_password, is_port_bind_conflict,
    pick_random_startup_port,
};
use har_provider::opencode::tokens::normalize_tokens;
use std::collections::HashMap;

fn mk_config(
    description: &str,
    prompt: &str,
    model: Option<&str>,
    tools: Option<Vec<&str>>,
    disallowed: Option<Vec<&str>>,
    skills: Option<Vec<&str>>,
    max_turns: Option<u32>,
) -> InlineAgentDefinition {
    InlineAgentDefinition {
        description: description.to_owned(),
        prompt: prompt.to_owned(),
        model: model.map(str::to_owned),
        tools: tools.map(|v| v.into_iter().map(str::to_owned).collect()),
        disallowed_tools: disallowed.map(|v| v.into_iter().map(str::to_owned).collect()),
        skills: skills.map(|v| v.into_iter().map(str::to_owned).collect()),
        max_turns,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 1 — config::parse_model_ref / parse_opencode_config
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn area1_parse_model_ref_matches_oracle() {
    // (input, Some((provider, model)) | None) — golden from bun oracle.
    let cases: &[(&str, Option<(&str, &str)>)] = &[
        ("anthropic/claude-3-5-sonnet", Some(("anthropic", "claude-3-5-sonnet"))),
        ("test/mock-model", Some(("test", "mock-model"))),
        ("noSlashModel", None),
        ("/model", None),
        ("provider/", None),
        ("", None),
        (" anthropic / claude-3-5-sonnet ", Some(("anthropic", "claude-3-5-sonnet"))),
        ("  /  ", None),
        ("a/b/c", Some(("a", "b/c"))),
        ("provider//", Some(("provider", "/"))),
        ("//model", None),
        ("p/m/", Some(("p", "m/"))),
        ("   /x", None),
        ("x/   ", None),
        (" / ", None),
    ];
    for (input, expected) in cases {
        let got = parse_model_ref(input);
        let exp = expected.map(|(p, m)| ProviderModel {
            provider_id: p.to_owned(),
            model_id: m.to_owned(),
        });
        assert_eq!(got, exp, "parse_model_ref({input:?})");
    }
}

#[test]
fn area1_parse_opencode_config_defensive_matrix() {
    use serde_json::json;
    let m = |v: serde_json::Value| -> HashMap<String, serde_json::Value> {
        v.as_object().unwrap().clone().into_iter().collect()
    };
    // empty
    let r = parse_opencode_config(&m(json!({})));
    assert!(r.model.is_none() && r.base_url.is_none() && r.agent.is_none());
    // model only
    let r = parse_opencode_config(&m(json!({ "model": "anthropic/claude-3-5-sonnet" })));
    assert_eq!(r.model.as_deref(), Some("anthropic/claude-3-5-sonnet"));
    // baseUrl only
    let r = parse_opencode_config(&m(json!({ "baseUrl": "http://localhost:4096" })));
    assert_eq!(r.base_url.as_deref(), Some("http://localhost:4096"));
    // opencode.agent
    let r = parse_opencode_config(&m(json!({ "opencode": { "agent": "my-agent" } })));
    assert_eq!(r.agent.as_deref(), Some("my-agent"));
    // wrong types dropped
    let r = parse_opencode_config(&m(json!({ "model": 42, "baseUrl": true })));
    assert!(r.model.is_none() && r.base_url.is_none());
    // all three
    let r = parse_opencode_config(&m(json!({ "model": "m", "baseUrl": "b", "opencode": { "agent": "a" } })));
    assert_eq!(r.model.as_deref(), Some("m"));
    assert_eq!(r.base_url.as_deref(), Some("b"));
    assert_eq!(r.agent.as_deref(), Some("a"));
    // opencode not object
    let r = parse_opencode_config(&m(json!({ "opencode": "not-an-object" })));
    assert!(r.agent.is_none());
    // opencode.agent non-string
    let r = parse_opencode_config(&m(json!({ "opencode": { "agent": 99 } })));
    assert!(r.agent.is_none());
    // model null
    let r = parse_opencode_config(&m(json!({ "model": null })));
    assert!(r.model.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 2 — errors
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn area2_classify_full_corpus() {
    // (combined-lowercased message, aborted, expected) — golden from oracle.
    // NOTE: Rust classify takes the pre-combined lowercase string; the TS builds it from
    // Error.name+message etc. We feed the lowercased message which is what the port consumes.
    let cases: &[(&str, bool, RetryableErrorClass)] = &[
        ("rate limit exceeded", false, RetryableErrorClass::RateLimit),
        ("too many requests", false, RetryableErrorClass::RateLimit),
        ("429 backoff", false, RetryableErrorClass::RateLimit),
        ("system overloaded", false, RetryableErrorClass::RateLimit),
        ("unauthorized", false, RetryableErrorClass::Auth),
        ("authentication failed", false, RetryableErrorClass::Auth),
        ("invalid token here", false, RetryableErrorClass::Auth),
        ("401", false, RetryableErrorClass::Auth),
        ("403 forbidden", false, RetryableErrorClass::Auth),
        ("bad api key", false, RetryableErrorClass::Auth),
        ("server disconnected", false, RetryableErrorClass::Crash),
        ("disposed", false, RetryableErrorClass::Crash),
        ("econnreset", false, RetryableErrorClass::Crash),
        ("socket hang up", false, RetryableErrorClass::Crash),
        ("connection terminated", false, RetryableErrorClass::Crash),
        ("process terminated", false, RetryableErrorClass::Crash),
        ("agent not found", false, RetryableErrorClass::AgentNotFound),
        ("unknown agent", false, RetryableErrorClass::AgentNotFound),
        ("invalid agent", false, RetryableErrorClass::AgentNotFound),
        ("no agent named foo", false, RetryableErrorClass::AgentNotFound),
        ("totally unexpected", false, RetryableErrorClass::Unknown),
        ("rate limit", true, RetryableErrorClass::Aborted), // aborted wins
        ("", false, RetryableErrorClass::Unknown),
        // precedence: rate_limit checked before auth
        ("rate limit and unauthorized", false, RetryableErrorClass::RateLimit),
        ("unauthorized and overloaded", false, RetryableErrorClass::RateLimit),
    ];
    for (msg, aborted, expected) in cases {
        assert_eq!(
            classify_opencode_error(msg, *aborted),
            *expected,
            "classify({msg:?}, aborted={aborted})"
        );
    }
}

#[test]
fn area2_enrich_byte_exact() {
    // golden from oracle — `OpenCode <class>: <msg>` except aborted.
    let classes = [
        (RetryableErrorClass::RateLimit, "OpenCode rate_limit: boom detail"),
        (RetryableErrorClass::Auth, "OpenCode auth: boom detail"),
        (RetryableErrorClass::Crash, "OpenCode crash: boom detail"),
        (RetryableErrorClass::AgentNotFound, "OpenCode agent_not_found: boom detail"),
        (RetryableErrorClass::Unknown, "OpenCode unknown: boom detail"),
        (RetryableErrorClass::Aborted, "OpenCode query aborted"),
    ];
    for (cls, expected) in classes {
        assert_eq!(enrich_opencode_error("boom detail", cls), expected);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 3 — tokens::normalize_tokens
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn area3_normalize_tokens_matches_oracle() {
    use serde_json::json;
    // (info, expected (input, output, total, cost)) — None means returns None.
    assert!(normalize_tokens(None).is_none());
    assert!(normalize_tokens(Some(&json!({}))).is_none());
    assert!(normalize_tokens(Some(&json!({ "cost": 0.1 }))).is_none());
    assert!(normalize_tokens(Some(&json!({ "tokens": "notobj" }))).is_none());

    let t = normalize_tokens(Some(&json!({ "tokens": {} }))).unwrap();
    assert_eq!((t.input, t.output, t.total, t.cost), (0, 0, None, None));

    let t = normalize_tokens(Some(&json!({ "tokens": { "input": 11, "output": 7, "reasoning": 3 } }))).unwrap();
    assert_eq!((t.input, t.output, t.total, t.cost), (11, 7, Some(21), None));

    let t = normalize_tokens(Some(&json!({ "tokens": { "input": 11, "output": 7, "reasoning": 3 }, "cost": 0.42 }))).unwrap();
    assert_eq!((t.input, t.output, t.total, t.cost), (11, 7, Some(21), Some(0.42)));

    let t = normalize_tokens(Some(&json!({ "tokens": { "input": 5 } }))).unwrap();
    assert_eq!((t.input, t.output, t.total), (5, 0, Some(5)));

    let t = normalize_tokens(Some(&json!({ "tokens": { "reasoning": 4 } }))).unwrap();
    assert_eq!((t.input, t.output, t.total), (0, 0, Some(4)));

    let t = normalize_tokens(Some(&json!({ "tokens": { "input": 0, "output": 0, "reasoning": 0 } }))).unwrap();
    assert_eq!((t.input, t.output, t.total), (0, 0, None));

    // non-number input → 0 (TS `typeof === 'number'` guard); non-number cost → omitted.
    let t = normalize_tokens(Some(&json!({ "tokens": { "input": "5", "output": 3 } }))).unwrap();
    assert_eq!((t.input, t.output, t.total), (0, 3, Some(3)));
    let t = normalize_tokens(Some(&json!({ "tokens": { "input": 11, "output": 7 }, "cost": "free" }))).unwrap();
    assert_eq!((t.input, t.output, t.total, t.cost), (11, 7, Some(18), None));
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 4 — agent_config
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn area4_kebab_full_corpus() {
    let cases: &[(&str, &str)] = &[
        ("My Agent", "my-agent"),
        ("test-agent", "test-agent"),
        ("Order Check", "order-check"),
        ("My_Agent!", "my-agent"),
        ("REVIEWER", "reviewer"),
        ("  leading", "leading"),
        ("trailing  ", "trailing"),
        ("--dashes--", "dashes"),
        ("multi   space", "multi-space"),
        ("a1b2c3", "a1b2c3"),
        ("HTTP Server", "http-server"),
        ("v2.0", "v2-0"),
        ("Agent#1", "agent-1"),
        ("___", ""),
        ("café", "caf"),
        ("日本語agent", "agent"),
        ("A-B_C D", "a-b-c-d"),
        ("", ""),
        ("UPPER lower 123", "upper-lower-123"),
        ("snake_case_name", "snake-case-name"),
    ];
    for (input, expected) in cases {
        assert_eq!(to_kebab_case(input), *expected, "to_kebab_case({input:?})");
    }
}

#[test]
fn area4_list_named_and_has_multiple() {
    let single: HashMap<String, InlineAgentDefinition> =
        [("My Agent".to_owned(), mk_config("d", "p", None, None, None, None, None))].into();
    let named = list_named_agents(Some(&single));
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].key, "My Agent");
    assert_eq!(named[0].opencode_agent_name, "archon-my-agent");

    assert!(list_named_agents(None).is_empty());
    assert!(!has_multiple_agents(None));
    assert!(!has_multiple_agents(Some(&single)));

    let mut multi = single.clone();
    multi.insert("Reviewer Two".to_owned(), mk_config("d2", "p2", None, None, None, None, None));
    assert!(has_multiple_agents(Some(&multi)));
}

#[test]
fn area4_get_ordered_agents() {
    let mut nc = NodeConfig::default();
    let agents: HashMap<String, InlineAgentDefinition> =
        [("reviewer".to_owned(), mk_config("d", "p", None, None, None, None, None))].into();
    nc.agents = Some(agents);
    let ordered = get_ordered_agents(Some(&nc));
    assert_eq!(ordered.len(), 1);
    assert_eq!(ordered[0].key, "reviewer");
    assert!(get_ordered_agents(None).is_empty());
}

#[test]
fn area4_adapt_named_agent() {
    // with model + tools (oracle: agent=archon-reviewer-two, model parsed, tools map)
    let agent = NamedAgentConfig {
        key: "Reviewer Two".to_owned(),
        opencode_agent_name: "archon-reviewer-two".to_owned(),
        config: mk_config(
            "d2",
            "p2",
            Some("anthropic/claude-3-5-sonnet"),
            Some(vec!["read", "grep"]),
            Some(vec!["bash"]),
            None,
            None,
        ),
    };
    let a = adapt_named_agent_for_opencode(&agent).unwrap();
    assert_eq!(a.agent, "archon-reviewer-two");
    assert_eq!(
        a.model,
        Some(ProviderModel { provider_id: "anthropic".into(), model_id: "claude-3-5-sonnet".into() })
    );
    let tools = a.tools.unwrap();
    assert_eq!(tools.get("read"), Some(&true));
    assert_eq!(tools.get("grep"), Some(&true));
    assert_eq!(tools.get("bash"), Some(&false));

    // no model → adapted.model None
    let agent2 = NamedAgentConfig {
        key: "My Agent".to_owned(),
        opencode_agent_name: "archon-my-agent".to_owned(),
        config: mk_config("d", "p", None, None, None, None, None),
    };
    let a2 = adapt_named_agent_for_opencode(&agent2).unwrap();
    assert!(a2.model.is_none() && a2.tools.is_none());

    // invalid model → byte-exact error message (oracle)
    let bad = NamedAgentConfig {
        key: "bad".to_owned(),
        opencode_agent_name: "archon-bad".to_owned(),
        config: mk_config("d", "p", Some("noslash"), None, None, None, None),
    };
    let err = adapt_named_agent_for_opencode(&bad).unwrap_err();
    assert_eq!(
        err,
        "Invalid OpenCode agent model ref for 'bad': 'noslash'. Expected format '<provider>/<model>' (for example 'anthropic/claude-3-5-sonnet')."
    );
}

#[test]
fn area4_build_tools_permissions_map() {
    // oracle: [null, {read,grep}, {bash}, {read,grep,bash,write}, {x:false}, null]
    assert!(build_tools_permissions_map(None, None).is_none());
    let r = build_tools_permissions_map(Some(&["read".into(), "grep".into()]), None).unwrap();
    assert_eq!(r.get("read"), Some(&true));
    assert_eq!(r.get("grep"), Some(&true));
    let r = build_tools_permissions_map(None, Some(&["bash".into()])).unwrap();
    assert_eq!(r.get("bash"), Some(&false));
    // collision: denied processed last → false
    let r = build_tools_permissions_map(Some(&["x".into()]), Some(&["x".into()])).unwrap();
    assert_eq!(r.get("x"), Some(&false));
    // empty slices → None
    assert!(build_tools_permissions_map(Some(&[]), Some(&[])).is_none());
}

#[test]
fn area4_resolve_prompt_returns_node_prompt() {
    let agent = NamedAgentConfig {
        key: "k".to_owned(),
        opencode_agent_name: "archon-k".to_owned(),
        config: mk_config("d", "p", None, None, None, None, None),
    };
    assert_eq!(resolve_prompt_for_agent(Some(&agent), "NODE PROMPT"), "NODE PROMPT");
    assert_eq!(resolve_prompt_for_agent(None, "NP2"), "NP2");
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 5 — agent_fs::build_agent_file_content (byte-exact vs bun)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn area5_build_agent_file_content_minimal() {
    // oracle 'minimal': '---\nmode: subagent\ndescription: "A minimal agent"\n---\n\nDo the thing'
    let c = mk_config("A minimal agent", "Do the thing", None, None, None, None, None);
    assert_eq!(
        build_agent_file_content(&c),
        "---\nmode: subagent\ndescription: \"A minimal agent\"\n---\n\nDo the thing"
    );
}

#[test]
fn area5_build_agent_file_content_quoted_desc() {
    // oracle 'quotedDesc' — JSON.stringify escaping of quotes + newline.
    let c = mk_config("has \"quotes\" and\nnewline", "p", None, None, None, None, None);
    assert_eq!(
        build_agent_file_content(&c),
        "---\nmode: subagent\ndescription: \"has \\\"quotes\\\" and\\nnewline\"\n---\n\np"
    );
}

#[test]
fn area5_build_agent_file_content_multiline_prompt() {
    let c = mk_config("d", "line1\nline2\nline3", None, None, None, None, None);
    assert_eq!(
        build_agent_file_content(&c),
        "---\nmode: subagent\ndescription: \"d\"\n---\n\nline1\nline2\nline3"
    );
}

#[test]
fn area5_build_agent_file_content_zero_turns() {
    // oracle 'zeroTurns': steps: 0 IS emitted (typeof 0 === 'number')
    let c = mk_config("d", "p", None, None, None, None, Some(0));
    assert_eq!(
        build_agent_file_content(&c),
        "---\nmode: subagent\ndescription: \"d\"\nsteps: 0\n---\n\np"
    );
}

#[test]
fn area5_build_agent_file_content_model_only() {
    let c = mk_config("d", "p", Some("x/y"), None, None, None, None);
    assert_eq!(
        build_agent_file_content(&c),
        "---\nmode: subagent\ndescription: \"d\"\nmodel: \"x/y\"\n---\n\np"
    );
}

#[test]
fn area5_build_agent_file_content_empty_skills_array() {
    // oracle 'emptySkills': empty skills array → NO skills: block
    let c = mk_config("d", "p", None, None, None, Some(vec![]), None);
    assert_eq!(
        build_agent_file_content(&c),
        "---\nmode: subagent\ndescription: \"d\"\n---\n\np"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 6 — runtime (portable helpers)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn area6_generate_random_password_64_hex() {
    let p = generate_random_password();
    assert_eq!(p.len(), 64);
    assert!(p.chars().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(generate_random_password(), generate_random_password());
}

#[test]
fn area6_extract_port_from_url() {
    assert_eq!(extract_port_from_url("http://127.0.0.1:4096"), Some(4096));
    assert_eq!(extract_port_from_url("http://mock-opencode.local:8080"), Some(8080));
    assert_eq!(extract_port_from_url("not-a-url"), None);
    // default port not exposed
    assert_eq!(extract_port_from_url("http://example.com"), None);
}

#[test]
fn area6_is_port_bind_conflict_all_patterns() {
    assert!(is_port_bind_conflict("EADDRINUSE: address already in use"));
    assert!(is_port_bind_conflict("eaddrinuse"));
    assert!(is_port_bind_conflict("address already in use"));
    assert!(is_port_bind_conflict("Failed to start server on port 4096"));
    assert!(is_port_bind_conflict("port 4096 is busy"));
    assert!(!is_port_bind_conflict("OpenCode binary missing"));
    assert!(!is_port_bind_conflict("network timeout"));
}

#[test]
fn area6_pick_random_startup_port_in_range() {
    for _ in 0..200 {
        let p = pick_random_startup_port();
        assert!((20000..60000).contains(&p), "port {p} out of [20000,60000)");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 8 — multi_agent::aggregate_tokens (reduce semantics vs bun)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn area8_aggregate_tokens_reduce_semantics() {
    use har_provider::opencode::multi_agent::{aggregate_tokens, AgentRunState};
    use serde_json::json;

    fn state(info: Option<serde_json::Value>) -> AgentRunState {
        AgentRunState {
            agent: NamedAgentConfig {
                key: "k".to_owned(),
                opencode_agent_name: "archon-k".to_owned(),
                config: mk_config("d", "p", None, None, None, None, None),
            },
            cwd: "/tmp".to_owned(),
            session_id: "s".to_owned(),
            chunks: vec![],
            latest_assistant_info: info.map(|v| v.as_object().unwrap().clone()),
            last_assistant_message_id: None,
            done: true,
        }
    }

    // empty → None (oracle: undefined)
    let s0 = state(None);
    let s0b = state(None);
    assert!(aggregate_tokens(&[&s0, &s0b]).is_none());

    // single, no cost → cost None (oracle 'singleNoCost': {input:5,output:3,total:8} no cost)
    let s = state(Some(json!({ "tokens": { "input": 5, "output": 3, "reasoning": 0 } })));
    let t = aggregate_tokens(&[&s]).unwrap();
    assert_eq!((t.input, t.output, t.total, t.cost), (5, 3, Some(8), None));

    // two both cost (oracle 'twoBothCost': 15/7/23/0.3)
    let a = state(Some(json!({ "tokens": { "input": 5, "output": 3, "reasoning": 0 }, "cost": 0.1 })));
    let b = state(Some(json!({ "tokens": { "input": 10, "output": 4, "reasoning": 1 }, "cost": 0.2 })));
    let t = aggregate_tokens(&[&a, &b]).unwrap();
    assert_eq!((t.input, t.output, t.total), (15, 7, Some(23)));
    assert!((t.cost.unwrap() - 0.3).abs() < 1e-9);

    // first no cost, second cost (oracle 'firstNoCostSecondCost': 15/7/22/0.2)
    let a = state(Some(json!({ "tokens": { "input": 5, "output": 3 } })));
    let b = state(Some(json!({ "tokens": { "input": 10, "output": 4 }, "cost": 0.2 })));
    let t = aggregate_tokens(&[&a, &b]).unwrap();
    assert_eq!((t.input, t.output, t.total), (15, 7, Some(22)));
    assert!((t.cost.unwrap() - 0.2).abs() < 1e-9);

    // zero-totals first + real second (oracle 'zeroTotalsMixed': 10/4/14/0)
    let a = state(Some(json!({ "tokens": { "input": 0, "output": 0, "reasoning": 0 } })));
    let b = state(Some(json!({ "tokens": { "input": 10, "output": 4 } })));
    let t = aggregate_tokens(&[&a, &b]).unwrap();
    assert_eq!((t.input, t.output, t.total), (10, 4, Some(14)));
    assert_eq!(t.cost, Some(0.0));
}

// ─────────────────────────────────────────────────────────────────────────────
// AREA 9 — seam isolation + materialize-before-seam side effect
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn area9_send_query_materializes_before_seam() {
    use futures_util::StreamExt;
    use har_contract::{AgentProvider, CancelToken};
    use har_provider::opencode::OpencodeProvider;
    use std::sync::Arc;

    struct NeverCancel;
    impl CancelToken for NeverCancel {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let cwd = tmp.path().to_str().unwrap().to_owned();
    let provider = OpencodeProvider::new();

    let agents: HashMap<String, InlineAgentDefinition> = [(
        "Reviewer".to_owned(),
        mk_config("Code review specialist", "Review the patch carefully", None, None, None, None, None),
    )]
    .into();
    let mut config = HashMap::new();
    config.insert("model".to_owned(), serde_json::Value::String("test/mock-model".to_owned()));
    let opts = SendQueryOptions {
        assistant_config: Some(config),
        node_config: Some(NodeConfig { agents: Some(agents), ..Default::default() }),
        ..Default::default()
    };

    let chunks: Vec<MessageChunk> = provider
        .send_query("hi".to_owned(), cwd.clone(), None, Some(opts), Arc::new(NeverCancel))
        .collect()
        .await;

    // seam result emitted
    let seam = chunks.iter().find_map(|c| match c {
        MessageChunk::Result { is_error: Some(true), error_subtype, .. } => error_subtype.clone(),
        _ => None,
    });
    assert_eq!(seam.as_deref(), Some("opencode_sdk_not_bound"));

    // FS side effect fired BEFORE the seam
    let agent_path = std::path::Path::new(&cwd)
        .join(".opencode")
        .join("agents")
        .join("archon-reviewer.md");
    assert!(agent_path.exists(), "materialize_agents must run before the SDK seam");
}

// ─────────────────────────────────────────────────────────────────────────────
// DIVERGENCES — these assert the TS-correct (oracle) behavior. They FAIL where the
// Rust port diverges. The FAILs are the gate's evidence; see parity-cycle19.md.
// ─────────────────────────────────────────────────────────────────────────────

mod divergences {
    use super::*;

    /// D1 — empty `description` must be OMITTED (TS `if (agentConfig.description)` is falsy
    /// for ""). Rust emits `description: ""`. Oracle 'emptyDescEmptyPrompt':
    ///   '---\nmode: subagent\n---'
    #[test]
    fn d1_empty_description_must_be_omitted() {
        let c = mk_config("", "", None, None, None, None, None);
        assert_eq!(
            build_agent_file_content(&c),
            "---\nmode: subagent\n---",
            "TS omits an empty description line; Rust emits `description: \"\"`"
        );
    }

    /// D1b — empty description, non-empty prompt. Oracle 'emptyDesc':
    ///   '---\nmode: subagent\n---\n\nhas prompt'
    #[test]
    fn d1b_empty_description_with_prompt() {
        let c = mk_config("", "has prompt", None, None, None, None, None);
        assert_eq!(
            build_agent_file_content(&c),
            "---\nmode: subagent\n---\n\nhas prompt"
        );
    }

    /// D3 — an empty-string `systemPrompt` ("") must be OMITTED from the body
    /// (TS `requestOptions?.systemPrompt ?` is falsy for ""). Rust inserts `system: ""`.
    /// Oracle 'emptySystem' keyOrder: ["parts","model"] — no `system`.
    #[test]
    fn d3_empty_system_prompt_must_be_omitted() {
        use har_contract::SystemPromptInput;
        use har_provider::opencode::session::create_session_prompt_body;
        let model = ProviderModel { provider_id: "test".into(), model_id: "mock-model".into() };
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Single(String::new())),
            ..Default::default()
        };
        let body = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert!(
            body.body.get("system").is_none(),
            "empty-string systemPrompt must be omitted (TS falsy); Rust emitted system={:?}",
            body.body.get("system")
        );
    }

    /// D3b — whitespace-only `Single(" ")` is TRUTHY in JS → INCLUDED as `" "`.
    /// Oracle: present=true | system=" ".
    #[test]
    fn d3b_whitespace_single_is_truthy_included() {
        use har_contract::SystemPromptInput;
        use har_provider::opencode::session::create_session_prompt_body;
        let model = ProviderModel { provider_id: "test".into(), model_id: "mock-model".into() };
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Single(" ".into())),
            ..Default::default()
        };
        let body = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert_eq!(
            body.body.get("system"),
            Some(&serde_json::Value::String(" ".into())),
            "whitespace-only Single is JS-truthy and must be included as \" \""
        );
    }

    /// D3c — **empty array `Multi([])` is TRUTHY in JS** (only ""/0/null/undefined/false/NaN
    /// are falsy). The TS `requestOptions?.systemPrompt ?` check INCLUDES it.
    /// Oracle 'Multi([])': present=true | system=[] | keys=["parts","model","system"].
    /// The porter's `is_falsy` fix wrongly treats `[].is_empty()` as falsy → OMITS it.
    /// This test asserts the TS-correct behavior; it FAILS against the over-applied fix.
    #[test]
    fn d3c_empty_array_multi_is_truthy_must_be_included() {
        use har_contract::SystemPromptInput;
        use har_provider::opencode::session::create_session_prompt_body;
        let model = ProviderModel { provider_id: "test".into(), model_id: "mock-model".into() };
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Multi(vec![])),
            ..Default::default()
        };
        let body = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert_eq!(
            body.body.get("system"),
            Some(&serde_json::Value::Array(vec![])),
            "empty array systemPrompt is JS-truthy ([] !== falsy) and must be INCLUDED as []; \
             Rust emitted system={:?}",
            body.body.get("system")
        );
    }

    /// D3d — non-empty `Multi(["a"])` is truthy → INCLUDED as ["a"].
    /// Oracle 'Multi(["a"])': present=true | system=["a"].
    #[test]
    fn d3d_nonempty_array_multi_included() {
        use har_contract::SystemPromptInput;
        use har_provider::opencode::session::create_session_prompt_body;
        let model = ProviderModel { provider_id: "test".into(), model_id: "mock-model".into() };
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Multi(vec!["a".into()])),
            ..Default::default()
        };
        let body = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert_eq!(
            body.body.get("system"),
            Some(&serde_json::json!(["a"])),
            "non-empty array systemPrompt must be included as [\"a\"]"
        );
    }

    /// D3e — `Preset` object is truthy → INCLUDED as the serialized object.
    /// Oracle 'Preset': present=true | system={"type":"preset","preset":"claude_code"}.
    #[test]
    fn d3e_preset_included() {
        use har_contract::{
            SystemPromptInput, SystemPromptPreset, SystemPromptPresetName, SystemPromptPresetType,
        };
        use har_provider::opencode::session::create_session_prompt_body;
        let model = ProviderModel { provider_id: "test".into(), model_id: "mock-model".into() };
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Preset(SystemPromptPreset {
                kind: SystemPromptPresetType::Preset,
                preset: SystemPromptPresetName::ClaudeCode,
                append: None,
                exclude_dynamic_sections: None,
            })),
            ..Default::default()
        };
        let body = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert_eq!(
            body.body.get("system"),
            Some(&serde_json::json!({"type": "preset", "preset": "claude_code"})),
            "preset systemPrompt must be included as the serialized object"
        );
    }

    /// D2c — collision: a tool in BOTH allowed and disallowed. JS object re-assigns the
    /// VALUE in place (key keeps its original insertion position). Oracle:
    ///   tools:\n  read: false\n  grep: true\n  bash: false  (read stays at pos 0, value=false)
    #[test]
    fn d2c_tool_collision_keeps_original_position_value_overwritten() {
        let c = mk_config(
            "",
            "",
            None,
            Some(vec!["read", "grep"]),
            Some(vec!["read", "bash"]),
            None,
            None,
        );
        assert_eq!(
            build_agent_file_content(&c),
            "---\nmode: subagent\ntools:\n  read: false\n  grep: true\n  bash: false\n---",
            "collision: last-writer-wins on VALUE, original key position preserved"
        );
    }

    /// D2f — disallowed-only.  Oracle: tools:\n  bash: false\n  net: false
    #[test]
    fn d2f_disallowed_only() {
        let c = mk_config("", "", None, None, Some(vec!["bash", "net"]), None, None);
        assert_eq!(
            build_agent_file_content(&c),
            "---\nmode: subagent\ntools:\n  bash: false\n  net: false\n---"
        );
    }

    /// D1c — whitespace-only description " " is JS-truthy → EMITTED.
    /// Oracle: ---\nmode: subagent\ndescription: " "\n---
    #[test]
    fn d1c_whitespace_description_emitted() {
        let c = mk_config(" ", "", None, None, None, None, None);
        assert_eq!(
            build_agent_file_content(&c),
            "---\nmode: subagent\ndescription: \" \"\n---",
            "whitespace-only description is JS-truthy and must be emitted"
        );
    }

    /// D2 — tools key order must follow INSERTION order (allowed first, then denied),
    /// matching JS object iteration. Rust sorts keys alphabetically. Oracle 'full':
    ///   tools:\n  read: true\n  grep: true\n  bash: false
    #[test]
    fn d2_tools_insertion_order_not_alphabetical() {
        let c = mk_config(
            "Code review specialist",
            "Review the patch carefully",
            Some("anthropic/claude-3-5-sonnet"),
            Some(vec!["read", "grep"]),
            Some(vec!["bash"]),
            Some(vec!["review-work"]),
            Some(7),
        );
        assert_eq!(
            build_agent_file_content(&c),
            "---\nmode: subagent\ndescription: \"Code review specialist\"\nmodel: \"anthropic/claude-3-5-sonnet\"\nsteps: 7\nskills:\n- \"review-work\"\ntools:\n  read: true\n  grep: true\n  bash: false\n---\n\nReview the patch carefully",
            "TS preserves insertion order (read, grep, bash); Rust sorts (bash, grep, read)"
        );
    }
}
