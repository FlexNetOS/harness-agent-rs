//! Differential parity harness — emits Rust-side parse results for the same cycle-1
//! fixtures the TS oracle (Archon/parity_oracle.ts) runs. Output is JSON: a list of
//! { id, ok, data? }. Run: `cargo run -p har-workflow-schema --example parity_diff`.
//!
//! Semantics mapping (TS zod `.safeParse` == deserialize + validate in one shot):
//!   - loop/retry  → `Config::parse(Value)` (deserialize + validate)
//!   - hook event  → `serde_json::from_value::<WorkflowHookEvent>`
//!   - hook matcher→ deserialize, then `.validate()`
//!   - node hooks  → `WorkflowNodeHooks::parse(Value)`
//!   - dag node    → `from_value::<DagNode>` THEN `validate_dag_node().is_empty()`
//!     (WF-01; the faithful analog of `dagNodeSchema.safeParse`, which runs
//!     structural deserialize AND superRefine in one shot)
//!   - workflow    → `from_value::<WorkflowDefinition>` (+ node validation) / `WorkflowBase`
//!     (WF-02; `workflowDefinitionSchema.safeParse`)

use har_workflow_schema::{
    validate_dag_node, validate_workflow_base, validate_workflow_definition, DagNode,
    LoopNodeConfig, ModelReasoningEffort, StepRetryConfig, ThinkingConfig, WebSearchMode,
    WorkflowBase, WorkflowDefinition, WorkflowHookEvent, WorkflowHookMatcher, WorkflowNodeHooks,
    WorkflowRequirement, WORKFLOW_HOOK_EVENTS,
};
use serde_json::{json, Value};

fn rec_ok(id: &str, data: Value) -> Value {
    json!({ "id": id, "ok": true, "data": data })
}
fn rec_err(id: &str) -> Value {
    json!({ "id": id, "ok": false })
}

fn loop_case(id: &str, input: Value) -> Value {
    match LoopNodeConfig::parse(input) {
        Ok(c) => rec_ok(id, serde_json::to_value(&c).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn retry_case(id: &str, input: Value) -> Value {
    match StepRetryConfig::parse(input) {
        Ok(c) => rec_ok(id, serde_json::to_value(&c).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn event_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WorkflowHookEvent>(input) {
        Ok(e) => rec_ok(id, serde_json::to_value(e).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn matcher_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WorkflowHookMatcher>(input) {
        Ok(m) => {
            if m.validate().is_empty() {
                rec_ok(id, serde_json::to_value(&m).unwrap())
            } else {
                rec_err(id)
            }
        }
        Err(_) => rec_err(id),
    }
}
fn nodehooks_case(id: &str, input: Value) -> Value {
    // .strict() gate. But also: TS rejects if a matcher inside fails (e.g. missing response).
    // First deserialize permissively then run strict parse; then validate each matcher.
    match WorkflowNodeHooks::parse(input.clone()) {
        Ok(h) => {
            // Validate every matcher (mirror zod validating nested matcher schema).
            let mut any_err = false;
            for matchers in h.events.values() {
                for m in matchers {
                    if !m.validate().is_empty() {
                        any_err = true;
                    }
                }
            }
            if any_err {
                rec_err(id)
            } else {
                rec_ok(id, serde_json::to_value(&h).unwrap())
            }
        }
        Err(_) => rec_err(id),
    }
}

/// WF-01 dagNodeSchema.safeParse analog: deserialize THEN validate (collect-all).
/// Accept only when BOTH the custom Deserialize succeeds AND validate_dag_node is empty —
/// because `safeParse` fails if ANY superRefine issue fires.
fn dag_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<DagNode>(input) {
        Ok(node) => {
            if validate_dag_node(&node).is_empty() {
                rec_ok(id, serde_json::to_value(&node).unwrap())
            } else {
                rec_err(id)
            }
        }
        Err(_) => rec_err(id),
    }
}
fn thinking_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<ThinkingConfig>(input) {
        Ok(t) => rec_ok(id, serde_json::to_value(&t).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn mre_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<ModelReasoningEffort>(input) {
        Ok(t) => rec_ok(id, serde_json::to_value(&t).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn wsm_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WebSearchMode>(input) {
        Ok(t) => rec_ok(id, serde_json::to_value(&t).unwrap()),
        Err(_) => rec_err(id),
    }
}
fn req_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WorkflowRequirement>(input) {
        Ok(t) => rec_ok(id, serde_json::to_value(&t).unwrap()),
        Err(_) => rec_err(id),
    }
}
/// WF-02 workflowBaseSchema.safeParse analog: deserialize THEN validate all value-bounds.
fn base_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WorkflowBase>(input) {
        Ok(b) => {
            if validate_workflow_base(&b).is_empty() {
                rec_ok(id, serde_json::to_value(&b).unwrap())
            } else {
                rec_err(id)
            }
        }
        Err(_) => rec_err(id),
    }
}
/// WF-02 workflowDefinitionSchema.safeParse analog: deserialize THEN validate all bounds
/// (base fields + every node, mirrors zod composing dagNodeSchema for each element of `nodes`).
fn def_case(id: &str, input: Value) -> Value {
    match serde_json::from_value::<WorkflowDefinition>(input) {
        Ok(def) => {
            if validate_workflow_definition(&def).is_empty() {
                rec_ok(id, serde_json::to_value(&def).unwrap())
            } else {
                rec_err(id)
            }
        }
        Err(_) => rec_err(id),
    }
}

fn id_node(extra: Value) -> Value {
    let mut v = json!({ "id": "n1" });
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        o.extend(e.clone());
    }
    v
}

fn main() {
    let mut out: Vec<Value> = Vec::new();

    // ── WF-03 Loop ──
    out.push(loop_case("loop.valid_min", json!({"prompt":"p","until":"DONE","max_iterations":3})));
    out.push(loop_case("loop.valid_full", json!({"prompt":"iterate","until":"COMPLETE","max_iterations":10,"fresh_context":true,"until_bash":"test -f done.txt","interactive":true,"gate_message":"Continue?"})));
    out.push(loop_case("loop.interactive_false_no_gate", json!({"prompt":"p","until":"D","max_iterations":1,"interactive":false})));
    out.push(loop_case("loop.interactive_true_no_gate", json!({"prompt":"p","until":"D","max_iterations":1,"interactive":true})));
    out.push(loop_case("loop.interactive_true_empty_gate", json!({"prompt":"p","until":"D","max_iterations":1,"interactive":true,"gate_message":""})));
    out.push(loop_case("loop.empty_prompt", json!({"prompt":"","until":"D","max_iterations":1})));
    out.push(loop_case("loop.empty_until", json!({"prompt":"p","until":"","max_iterations":1})));
    out.push(loop_case("loop.zero_max_iter", json!({"prompt":"p","until":"D","max_iterations":0})));
    out.push(loop_case("loop.neg_max_iter", json!({"prompt":"p","until":"D","max_iterations":-1})));
    out.push(loop_case("loop.float_max_iter", json!({"prompt":"p","until":"D","max_iterations":2.5})));
    out.push(loop_case("loop.all_errors", json!({"prompt":"","until":"","max_iterations":0,"interactive":true})));
    out.push(loop_case("loop.fresh_context_default", json!({"prompt":"p","until":"D","max_iterations":1})));
    out.push(loop_case("loop.extra_field", json!({"prompt":"p","until":"D","max_iterations":1,"futureField":99})));

    // ── WF-04 Retry ──
    out.push(retry_case("retry.valid_min", json!({"max_attempts":2})));
    out.push(retry_case("retry.attempts_1", json!({"max_attempts":1})));
    out.push(retry_case("retry.attempts_5", json!({"max_attempts":5})));
    out.push(retry_case("retry.attempts_0", json!({"max_attempts":0})));
    out.push(retry_case("retry.attempts_6", json!({"max_attempts":6})));
    out.push(retry_case("retry.attempts_float", json!({"max_attempts":2.5})));
    out.push(retry_case("retry.attempts_missing", json!({})));
    out.push(retry_case("retry.delay_1000", json!({"max_attempts":1,"delay_ms":1000})));
    out.push(retry_case("retry.delay_60000", json!({"max_attempts":1,"delay_ms":60000})));
    out.push(retry_case("retry.delay_999", json!({"max_attempts":1,"delay_ms":999})));
    out.push(retry_case("retry.delay_60001", json!({"max_attempts":1,"delay_ms":60001})));
    out.push(retry_case("retry.delay_float", json!({"max_attempts":1,"delay_ms":1500.5})));
    // Adversarial fractional-boundary cases (WF-04 re-verify, cycle-1 retest).
    out.push(retry_case("retry.delay_frac_below", json!({"max_attempts":1,"delay_ms":999.9})));
    out.push(retry_case("retry.delay_frac_above", json!({"max_attempts":1,"delay_ms":60000.5})));
    out.push(retry_case("retry.delay_frac_at_min", json!({"max_attempts":1,"delay_ms":1000.5})));
    out.push(retry_case("retry.delay_frac_at_max", json!({"max_attempts":1,"delay_ms":59999.9})));
    out.push(retry_case("retry.delay_int_roundtrip", json!({"max_attempts":1,"delay_ms":2000})));
    out.push(retry_case("retry.on_error_transient", json!({"max_attempts":1,"on_error":"transient"})));
    out.push(retry_case("retry.on_error_all", json!({"max_attempts":1,"on_error":"all"})));
    out.push(retry_case("retry.on_error_bad", json!({"max_attempts":1,"on_error":"sometimes"})));
    out.push(retry_case("retry.full", json!({"max_attempts":3,"delay_ms":2000,"on_error":"transient"})));
    out.push(retry_case("retry.extra_field", json!({"max_attempts":1,"futureField":true})));

    // ── WF-05 Hooks: event enum ──
    for e in WORKFLOW_HOOK_EVENTS {
        out.push(event_case(&format!("hookevent.{}", e.as_str()), json!(e.as_str())));
    }
    out.push(event_case("hookevent.camel", json!("preToolUse")));
    out.push(event_case("hookevent.snake", json!("pre_tool_use")));
    out.push(event_case("hookevent.empty", json!("")));
    out.push(event_case("hookevent.unknown", json!("Unknown")));

    // ── WF-05 Hooks: matcher ──
    out.push(matcher_case("matcher.full", json!({"matcher":"Bash","response":{"decision":"allow"},"timeout":30})));
    out.push(matcher_case("matcher.no_optional", json!({"response":{"decision":"deny"}})));
    out.push(matcher_case("matcher.timeout_neg", json!({"response":{},"timeout":-1})));
    out.push(matcher_case("matcher.timeout_zero", json!({"response":{},"timeout":0})));
    out.push(matcher_case("matcher.missing_response", json!({"matcher":"Bash"})));

    // ── WF-05 Hooks: node hooks (.strict) ──
    out.push(nodehooks_case("nodehooks.known", json!({"PreToolUse":[{"matcher":"Bash","response":{"decision":"allow"}}],"PostToolUse":[{"response":{"type":"log"}}]})));
    out.push(nodehooks_case("nodehooks.empty", json!({})));
    out.push(nodehooks_case("nodehooks.unknown_camel", json!({"PreToolUse":[{"response":{"decision":"allow"}}],"preToolUse":[{"response":{"decision":"deny"}}]})));
    out.push(nodehooks_case("nodehooks.unknown_snake", json!({"pre_tool_use":[{"response":{}}]})));
    let mut all21 = serde_json::Map::new();
    for e in WORKFLOW_HOOK_EVENTS {
        all21.insert(e.as_str().to_owned(), json!([{"response":{"ok":true}}]));
    }
    out.push(nodehooks_case("nodehooks.all21", Value::Object(all21)));

    // ════════════════════════════════════════════════════════════════════════
    // WF-01 dag-node (cycle 2) — dagNodeSchema.safeParse differential
    // ════════════════════════════════════════════════════════════════════════

    // ── ThinkingConfig: string shorthand + object forms ──
    out.push(thinking_case("think.str_adaptive", json!("adaptive")));
    out.push(thinking_case("think.str_enabled", json!("enabled")));
    out.push(thinking_case("think.str_disabled", json!("disabled")));
    out.push(thinking_case("think.str_unknown", json!("turbo")));
    out.push(thinking_case("think.obj_adaptive", json!({"type":"adaptive"})));
    out.push(thinking_case("think.obj_enabled", json!({"type":"enabled"})));
    out.push(thinking_case("think.obj_enabled_budget", json!({"type":"enabled","budgetTokens":1024})));
    out.push(thinking_case("think.obj_disabled", json!({"type":"disabled"})));
    out.push(thinking_case("think.obj_unknown_type", json!({"type":"maximal"})));
    // budgetTokens is z.number().int().positive(): 0 reject, fractional reject, negative reject.
    out.push(thinking_case("think.budget_zero", json!({"type":"enabled","budgetTokens":0})));
    out.push(thinking_case("think.budget_frac", json!({"type":"enabled","budgetTokens":1.5})));
    out.push(thinking_case("think.budget_neg", json!({"type":"enabled","budgetTokens":-5})));

    // ── DagNode: every single-mode accept (all 7 variants) ──
    out.push(dag_case("dag.command", id_node(json!({"command":"my-command"}))));
    out.push(dag_case("dag.prompt", id_node(json!({"prompt":"do it"}))));
    out.push(dag_case("dag.bash", id_node(json!({"bash":"echo hi"}))));
    out.push(dag_case("dag.script", id_node(json!({"script":"print(1)","runtime":"uv"}))));
    out.push(dag_case("dag.loop", id_node(json!({"loop":{"prompt":"p","until":"D","max_iterations":3}}))));
    out.push(dag_case("dag.approval", id_node(json!({"approval":{"message":"review"}}))));
    out.push(dag_case("dag.cancel", id_node(json!({"cancel":"halt"}))));

    // ── DagNode: mutual exclusivity (multi-mode rejects) ──
    out.push(dag_case("dag.cmd_bash", id_node(json!({"command":"foo","bash":"echo"}))));
    out.push(dag_case("dag.prompt_loop", id_node(json!({"prompt":"p","loop":{"prompt":"x","until":"D","max_iterations":1}}))));
    out.push(dag_case("dag.approval_cancel", id_node(json!({"approval":{"message":"m"},"cancel":"c"}))));
    out.push(dag_case("dag.three_modes", id_node(json!({"command":"a","prompt":"b","bash":"c"}))));
    // ── zero mode-fields → reject ──
    out.push(dag_case("dag.no_mode", json!({"id":"n1"})));
    out.push(dag_case("dag.empty_command", id_node(json!({"command":""}))));
    out.push(dag_case("dag.empty_bash", id_node(json!({"bash":""}))));
    out.push(dag_case("dag.empty_prompt", id_node(json!({"prompt":""}))));
    out.push(dag_case("dag.empty_script", id_node(json!({"script":""}))));
    // ── empty-mode-string + real loop → mode count 1, accept ──
    out.push(dag_case("dag.emptybash_loop", id_node(json!({"bash":"","loop":{"prompt":"p","until":"D","max_iterations":2}}))));

    // ── superRefine: command-name validity (incl path traversal) ──
    out.push(dag_case("dag.cmd_traversal", id_node(json!({"command":"../foo"}))));
    out.push(dag_case("dag.cmd_slash", id_node(json!({"command":"foo/bar"}))));
    out.push(dag_case("dag.cmd_dot", id_node(json!({"command":".hidden"}))));
    out.push(dag_case("dag.cmd_backslash", id_node(json!({"command":"foo\\bar"}))));
    out.push(dag_case("dag.cmd_valid", id_node(json!({"command":"valid-cmd"}))));

    // ── superRefine: bash/script timeout (positive; FRACTIONAL ACCEPT — z.number no .int) ──
    out.push(dag_case("dag.bash_timeout_frac", id_node(json!({"bash":"echo","timeout":1500.5}))));
    out.push(dag_case("dag.bash_timeout_zero", id_node(json!({"bash":"echo","timeout":0}))));
    out.push(dag_case("dag.bash_timeout_neg", id_node(json!({"bash":"echo","timeout":-1}))));
    out.push(dag_case("dag.script_timeout_frac", id_node(json!({"script":"x","runtime":"bun","timeout":2.5}))));
    out.push(dag_case("dag.script_timeout_zero", id_node(json!({"script":"x","runtime":"bun","timeout":0}))));
    // ── superRefine: script missing runtime ──
    out.push(dag_case("dag.script_no_runtime", id_node(json!({"script":"print(1)"}))));
    // ── superRefine: loop + retry conflict ──
    out.push(dag_case("dag.loop_retry", id_node(json!({"loop":{"prompt":"p","until":"D","max_iterations":2},"retry":{"max_attempts":2}}))));
    // ── superRefine: idle_timeout (positive; FRACTIONAL ACCEPT) ──
    out.push(dag_case("dag.idle_frac", id_node(json!({"prompt":"hi","idle_timeout":500.5}))));
    out.push(dag_case("dag.idle_zero", id_node(json!({"prompt":"hi","idle_timeout":0}))));
    out.push(dag_case("dag.idle_neg", id_node(json!({"prompt":"hi","idle_timeout":-10}))));

    // ── Agent-ID regex ^[a-z0-9]+(-[a-z0-9]+)*$ ──
    out.push(dag_case("dag.agent_valid_simple", id_node(json!({"prompt":"hi","agents":{"my-agent":{"description":"d","prompt":"p"}}}))));
    out.push(dag_case("dag.agent_valid_a1", id_node(json!({"prompt":"hi","agents":{"a1":{"description":"d","prompt":"p"}}}))));
    out.push(dag_case("dag.agent_lead_hyphen", id_node(json!({"prompt":"hi","agents":{"-x":{"description":"d","prompt":"p"}}}))));
    out.push(dag_case("dag.agent_trail_hyphen", id_node(json!({"prompt":"hi","agents":{"x-":{"description":"d","prompt":"p"}}}))));
    out.push(dag_case("dag.agent_upper", id_node(json!({"prompt":"hi","agents":{"My-Agent":{"description":"d","prompt":"p"}}}))));
    out.push(dag_case("dag.agent_double_hyphen", id_node(json!({"prompt":"hi","agents":{"a--b":{"description":"d","prompt":"p"}}}))));

    // ── int vs float: budget/max_attempts/max_turns ARE .int() (fractional REJECT) ──
    out.push(dag_case("dag.agent_maxturns_frac", id_node(json!({"prompt":"hi","agents":{"a":{"description":"d","prompt":"p","maxTurns":1.5}}}))));
    out.push(dag_case("dag.agent_maxturns_zero", id_node(json!({"prompt":"hi","agents":{"a":{"description":"d","prompt":"p","maxTurns":0}}}))));
    out.push(dag_case("dag.agent_maxturns_ok", id_node(json!({"prompt":"hi","agents":{"a":{"description":"d","prompt":"p","maxTurns":5}}}))));
    out.push(dag_case("dag.approval_maxatt_frac", id_node(json!({"approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":1.5}}}))));
    out.push(dag_case("dag.approval_maxatt_zero", id_node(json!({"approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":0}}}))));
    out.push(dag_case("dag.approval_maxatt_over", id_node(json!({"approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":11}}}))));
    out.push(dag_case("dag.approval_maxatt_ok", id_node(json!({"approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":5}}}))));

    // ── maxBudgetUsd z.number().positive() (no .int): 0.5 accept, 0 reject ──
    out.push(dag_case("dag.maxbudget_frac", id_node(json!({"prompt":"hi","maxBudgetUsd":0.5}))));
    out.push(dag_case("dag.maxbudget_zero", id_node(json!({"prompt":"hi","maxBudgetUsd":0}))));

    // ── zod string non-empty / collection non-empty constraints ──
    out.push(dag_case("dag.betas_empty", id_node(json!({"prompt":"hi","betas":[]}))));
    out.push(dag_case("dag.betas_empty_str", id_node(json!({"prompt":"hi","betas":[""]}))));
    out.push(dag_case("dag.betas_ok", id_node(json!({"prompt":"hi","betas":["x"]}))));
    out.push(dag_case("dag.skills_empty", id_node(json!({"prompt":"hi","skills":[]}))));
    out.push(dag_case("dag.skills_empty_str", id_node(json!({"prompt":"hi","skills":[""]}))));
    out.push(dag_case("dag.agents_empty", id_node(json!({"prompt":"hi","agents":{}}))));
    out.push(dag_case("dag.provider_blank", id_node(json!({"prompt":"hi","provider":"   "}))));

    // ════════════════════════════════════════════════════════════════════════
    // WF-02 workflow (cycle 2) — workflow*.safeParse differential
    // ════════════════════════════════════════════════════════════════════════

    out.push(mre_case("mre.minimal", json!("minimal")));
    out.push(mre_case("mre.xhigh", json!("xhigh")));
    out.push(mre_case("mre.bogus", json!("ultra")));
    out.push(wsm_case("wsm.disabled", json!("disabled")));
    out.push(wsm_case("wsm.live", json!("live")));
    out.push(wsm_case("wsm.bogus", json!("offline")));
    out.push(req_case("req.github", json!("github")));
    out.push(req_case("req.gitlab", json!("gitlab")));

    out.push(base_case("base.ok", json!({"name":"n","description":"d"})));
    out.push(base_case("base.empty_name", json!({"name":"","description":"d"})));
    out.push(base_case("base.empty_desc", json!({"name":"n","description":""})));
    out.push(base_case("base.provider_blank", json!({"name":"n","description":"d","provider":"  "})));
    out.push(base_case("base.tags_empty_str", json!({"name":"n","description":"d","tags":[""]})));
    out.push(base_case("base.fallback_empty", json!({"name":"n","description":"d","fallbackModel":""})));
    out.push(base_case("base.full", json!({"name":"n","description":"d","modelReasoningEffort":"xhigh","webSearchMode":"live","effort":"max","thinking":"adaptive","requires":["github"]})));

    out.push(def_case("def.ok", json!({"name":"n","description":"d","nodes":[{"id":"a","prompt":"hi"}]})));
    out.push(def_case("def.empty_nodes", json!({"name":"n","description":"d","nodes":[]})));
    out.push(def_case("def.no_nodes", json!({"name":"n","description":"d"})));
    out.push(def_case("def.node_bad_mode", json!({"name":"n","description":"d","nodes":[{"id":"a"}]})));
    out.push(def_case("def.node_maxbudget_zero", json!({"name":"n","description":"d","nodes":[{"id":"a","prompt":"hi","maxBudgetUsd":0}]})));
    out.push(def_case("def.multi_node", json!({"name":"n","description":"d","nodes":[{"id":"a","prompt":"do a"},{"id":"b","bash":"echo b","depends_on":["a"]}]})));

    // ════════════════════════════════════════════════════════════════════════
    // NEW cycle-2 re-verify adversarial bound-edge cases
    // ════════════════════════════════════════════════════════════════════════
    // max_attempts boundary: 1 accept, 10 accept, (0/11 reject already covered above)
    out.push(dag_case("dag.approval_maxatt_one", id_node(json!({"approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":1}}}))));
    out.push(dag_case("dag.approval_maxatt_ten", id_node(json!({"approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":10}}}))));
    // provider trim-edge: '   x   ' trims to non-empty 'x' → ACCEPT; ' '/'\t\t' → REJECT
    out.push(dag_case("dag.provider_pad_nonblank", id_node(json!({"prompt":"hi","provider":"   x   "}))));
    out.push(dag_case("dag.provider_single_space", id_node(json!({"prompt":"hi","provider":" "}))));
    out.push(dag_case("dag.provider_tabs", id_node(json!({"prompt":"hi","provider":"\t\t"}))));
    // mcp trim-transform lock: '  m  ' → output 'm' (dag-node.ts:598 `data.mcp.trim()`)
    out.push(dag_case("dag.mcp_pad_nonblank", id_node(json!({"prompt":"hi","mcp":"  m  "}))));
    // skills-element trim-transform lock: ['  s  '] → output ['s'] (dag-node.ts:599 `.map(s=>s.trim())`)
    out.push(dag_case("dag.skills_pad_nonblank", id_node(json!({"prompt":"hi","skills":["  s  ","  t  "]}))));
    // thinking budget 1 (smallest positive) accept; maxBudgetUsd tiny-positive accept; neg reject
    out.push(dag_case("dag.budget_one", id_node(json!({"prompt":"hi","thinking":{"type":"enabled","budgetTokens":1}}))));
    out.push(dag_case("dag.maxbudget_tiny", id_node(json!({"prompt":"hi","maxBudgetUsd":0.0001}))));
    out.push(dag_case("dag.maxbudget_neg", id_node(json!({"prompt":"hi","maxBudgetUsd":-1.5}))));
    // skills single non-empty element accept
    out.push(dag_case("dag.skills_one", id_node(json!({"prompt":"hi","skills":["x"]}))));
    // agent maxTurns:1 (smallest positive) accept
    out.push(dag_case("dag.agent_maxturns_one", id_node(json!({"prompt":"hi","agents":{"a":{"description":"d","prompt":"p","maxTurns":1}}}))));
    // WorkflowBase: name single-space → z.string().min(1) (NO trim) → len 1 → ACCEPT
    out.push(base_case("base.name_single_space", json!({"name":" ","description":"d"})));
    out.push(base_case("base.name_blank_multi", json!({"name":"  ","description":"d"})));
    out.push(base_case("base.provider_pad_nonblank", json!({"name":"n","description":"d","provider":"  x  "})));
    out.push(base_case("base.tags_ok_one", json!({"name":"n","description":"d","tags":["ci"]})));
    // WorkflowDefinition deep-node rejects: nested thinking budget 0 → node invalid → def reject
    out.push(def_case("def.node_thinkbudget_zero", json!({"name":"n","description":"d","nodes":[{"id":"a","prompt":"hi","thinking":{"type":"enabled","budgetTokens":0}}]})));
    // WorkflowDefinition deep-node max_attempts:11 → reject through workflow; 10 → accept
    out.push(def_case("def.node_maxatt_over", json!({"name":"n","description":"d","nodes":[{"id":"a","approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":11}}}]})));
    out.push(def_case("def.node_maxatt_ten", json!({"name":"n","description":"d","nodes":[{"id":"a","approval":{"message":"m","on_reject":{"prompt":"p","max_attempts":10}}}]})));
    // WorkflowDefinition base-error + node-error combined → reject
    out.push(def_case("def.base_and_node_err", json!({"name":"","description":"d","nodes":[{"id":"a","prompt":"hi","maxBudgetUsd":0}]})));

    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
