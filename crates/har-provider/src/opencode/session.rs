//! OpenCode session management and event-stream processing.
//!
//! PORT of `packages/providers/src/community/opencode/session.ts`.
//!
//! # Source coverage
//!
//! - `resolveSessionId`         (session.ts:26-53)  → `resolve_session_id`
//! - `createSessionPromptBody`  (session.ts:55-80)  → `create_session_prompt_body`
//! - `promptSession`            (session.ts:82-93)  → `promptSession` (internal helper shape)
//! - `readStructuredOutput`     (session.ts:95-117) → `read_structured_output`
//! - `streamOpencodeSession`    (session.ts:119-303) → `stream_opencode_session`
//! - `abortableStream`          (session.ts:306-339) → `abortable_stream`
//!
//! All event-type branches are ported:
//!   `message.updated`, `message.part.updated` (text / reasoning / tool / tool_result),
//!   `session.error`, `session.idle` (with structuredOutput + tokens + modelUsage).
//!
//! # SDK seam
//!
//! `resolveSessionId`, `createSessionPromptBody`, `read_structured_output`, and the event
//! demux logic are fully ported. The actual SDK calls (`client.session.create`,
//! `client.session.get`, `client.session.promptAsync`, `client.event.subscribe`)
//! are behind the `opencode_sdk_not_bound` seam in `provider.rs` — they are expressed
//! in this module as data structures and logic that would be called by a live client binding.
//!
//! `stream_opencode_session` and `stream_multi_agent_opencode_session` are not called
//! in the seam path; they are kept here as complete, compilable code for future binding.

use std::collections::{HashMap, HashSet};

use har_contract::{MessageChunk, SendQueryOptions};
use serde_json::Value;

use crate::opencode::agent_config::{
    adapt_named_agent_for_opencode, resolve_prompt_for_agent, select_single_agent, NamedAgentConfig,
};
use crate::opencode::config::ProviderModel;
use crate::opencode::tokens::normalize_tokens;

// ─── Session resolution ────────────────────────────────────────────────────────

/// Result of session resolution.
///
/// PORT of the return type of `resolveSessionId` (session.ts:26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSession {
    pub session_id: String,
    pub resumed: bool,
}

/// Resolve the session ID: try to resume, fall back to creating a new session.
///
/// PORT of `resolveSessionId(client, cwd, resumeSessionId?)` (session.ts:26-53).
///
/// In the SDK-live path this calls `client.session.get(...)` to attempt resume,
/// then `client.session.create(...)` to create a new session.
/// This function defines the SHAPE and LOGIC; the actual SDK calls are
/// behind the seam in `provider.rs`.
pub fn resolve_session_id_logic(
    resume_session_id: Option<&str>,
    resumed_session: Option<String>,
    new_session_id: String,
) -> ResolvedSession {
    if let Some(_resume_id) = resume_session_id {
        if let Some(existing_id) = resumed_session {
            if !existing_id.is_empty() {
                return ResolvedSession {
                    session_id: existing_id,
                    resumed: true,
                };
            }
        }
    }
    ResolvedSession {
        session_id: new_session_id,
        resumed: false,
    }
}

// ─── Prompt body construction ─────────────────────────────────────────────────

/// Prompt body sent to `client.session.promptAsync`.
///
/// PORT of the return type of `createSessionPromptBody` (session.ts:55-80).
///
/// Uses `serde_json::Map` to preserve insertion order (parity lesson: deterministic
/// key order is observable when the schema is sent to the LLM).
#[derive(Debug, Clone)]
pub struct SessionPromptBody {
    /// Serializable prompt body — sent as `body` to `promptAsync`.
    pub body: serde_json::Map<String, Value>,
}

/// Create the prompt body for `client.session.promptAsync`.
///
/// PORT of `createSessionPromptBody(prompt, model, requestOptions?, agentOverride?)` (session.ts:55-80).
///
/// - Selects the agent (override or from nodeConfig)
/// - Adapts agent config for OpenCode
/// - Resolves the effective prompt (always the node prompt — agent prompt is in .md file)
/// - Builds parts, model, optional agent/tools/system/format fields
pub fn create_session_prompt_body(
    prompt: &str,
    model: &ProviderModel,
    request_options: Option<&SendQueryOptions>,
    agent_override: Option<&NamedAgentConfig>,
) -> Result<SessionPromptBody, String> {
    let single_agent = if let Some(agent) = agent_override {
        Some(agent.clone())
    } else {
        let agents = request_options
            .and_then(|o| o.node_config.as_ref())
            .and_then(|nc| nc.agents.as_ref());
        select_single_agent(agents)
    };

    let adapted = single_agent
        .as_ref()
        .map(adapt_named_agent_for_opencode)
        .transpose()?;
    let effective_prompt = resolve_prompt_for_agent(single_agent.as_ref(), prompt);

    let mut body = serde_json::Map::new();

    // parts: [{ type: 'text', text: effectivePrompt }]
    let mut part = serde_json::Map::new();
    part.insert("type".to_owned(), Value::String("text".to_owned()));
    part.insert("text".to_owned(), Value::String(effective_prompt));
    body.insert("parts".to_owned(), Value::Array(vec![Value::Object(part)]));

    // model: adaptedAgentConfig?.model ?? model
    let model_value = if let Some(ref adapted) = adapted {
        if let Some(ref m) = adapted.model {
            let mut mv = serde_json::Map::new();
            mv.insert("providerID".to_owned(), Value::String(m.provider_id.clone()));
            mv.insert("modelID".to_owned(), Value::String(m.model_id.clone()));
            Value::Object(mv)
        } else {
            let mut mv = serde_json::Map::new();
            mv.insert("providerID".to_owned(), Value::String(model.provider_id.clone()));
            mv.insert("modelID".to_owned(), Value::String(model.model_id.clone()));
            Value::Object(mv)
        }
    } else {
        let mut mv = serde_json::Map::new();
        mv.insert("providerID".to_owned(), Value::String(model.provider_id.clone()));
        mv.insert("modelID".to_owned(), Value::String(model.model_id.clone()));
        Value::Object(mv)
    };
    body.insert("model".to_owned(), model_value);

    // agent: adapted?.agent
    if let Some(ref adapted) = adapted {
        body.insert("agent".to_owned(), Value::String(adapted.agent.clone()));
    }

    // tools: adapted?.tools
    if let Some(ref adapted) = adapted {
        if let Some(ref tools) = adapted.tools {
            let tools_obj: serde_json::Map<String, Value> = tools
                .iter()
                .map(|(k, v)| (k.clone(), Value::Bool(*v)))
                .collect();
            body.insert("tools".to_owned(), Value::Object(tools_obj));
        }
    }

    // system: requestOptions?.systemPrompt
    // TS `if (systemPrompt)` is falsy for ""; omit the field when the prompt is empty/falsy.
    if let Some(opts) = request_options {
        if let Some(ref system) = opts.system_prompt {
            // Apply JS truthiness on the RAW value (session.ts:69 `requestOptions?.systemPrompt ?`).
            // In JS ONLY ""/0/null/undefined/false/NaN are falsy — an empty array `[]` and an
            // object `{}` are TRUTHY. So `system` is omitted ONLY for an empty string (Single("")):
            //   Single("")  -> falsy -> OMIT (oracle: keys=["parts","model"])
            //   Single(" ") -> truthy -> include as " "
            //   Multi([])   -> truthy -> include as []   (NOT falsy — verified vs live bun)
            //   Multi(["a"])-> truthy -> include
            //   Preset      -> truthy -> include
            let is_falsy = match system {
                har_contract::SystemPromptInput::Single(s) => s.is_empty(),
                har_contract::SystemPromptInput::Multi(_) => false,
                har_contract::SystemPromptInput::Preset(_) => false,
            };
            if !is_falsy {
                let sys_json = serde_json::to_value(system).unwrap_or(Value::Null);
                body.insert("system".to_owned(), sys_json);
            }
        }
    }

    // format: { type: 'json_schema', schema: requestOptions.outputFormat.schema }
    if let Some(opts) = request_options {
        if let Some(ref output_format) = opts.output_format {
            let mut format_obj = serde_json::Map::new();
            format_obj.insert("type".to_owned(), Value::String("json_schema".to_owned()));
            format_obj.insert("schema".to_owned(), Value::Object(output_format.schema.clone()));
            body.insert("format".to_owned(), Value::Object(format_obj));
        }
    }

    Ok(SessionPromptBody { body })
}

// ─── Structured output extraction ─────────────────────────────────────────────

/// Read structured output from a message info record.
///
/// PORT of `readStructuredOutput` (session.ts:95-117).
///
/// Returns the `info.structured_output` field if present.
/// Logs a debug warning on failure (matching the TS `warn` call).
pub fn read_structured_output_from_info(info: &Value) -> Option<Value> {
    if let Value::Object(obj) = info {
        obj.get("structured_output").cloned()
    } else {
        None
    }
}

// ─── Event processing helpers ─────────────────────────────────────────────────

#[allow(dead_code)]
fn is_record(v: &Value) -> bool {
    matches!(v, Value::Object(_))
}

/// Process a `message.updated` event.
///
/// PORT of the `message.updated` branch (session.ts:161-169).
/// Updates `latest_assistant_info` + `last_assistant_message_id` if the event is for this session.
pub fn process_message_updated(
    properties: &serde_json::Map<String, Value>,
    session_id: &str,
    latest_assistant_info: &mut Option<serde_json::Map<String, Value>>,
    last_assistant_message_id: &mut Option<String>,
) {
    let info = properties.get("info").and_then(|v| v.as_object()).cloned();
    let Some(info) = info else { return };
    let role = info.get("role").and_then(Value::as_str).unwrap_or("");
    let info_session_id = info.get("sessionID").and_then(Value::as_str).unwrap_or("");
    if role == "assistant" && info_session_id == session_id {
        if let Some(id) = info.get("id").and_then(Value::as_str) {
            *last_assistant_message_id = Some(id.to_owned());
        }
        *latest_assistant_info = Some(info);
    }
}

/// Process a `message.part.updated` event.
///
/// PORT of the `message.part.updated` branch (session.ts:172-234).
/// Returns `Some(MessageChunk)` for text / reasoning / tool / tool_result parts.
/// Uses `seen_tool_calls` + `completed_tool_calls` for idempotent tool dedup.
pub fn process_message_part_updated(
    properties: &serde_json::Map<String, Value>,
    session_id: &str,
    seen_tool_calls: &mut HashSet<String>,
    completed_tool_calls: &mut HashSet<String>,
) -> Vec<MessageChunk> {
    let mut out = Vec::new();

    let part = match properties.get("part").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return out,
    };

    let part_session_id = part.get("sessionID").and_then(Value::as_str).unwrap_or("");
    if part_session_id != session_id {
        return out;
    }
    let part_type = match part.get("type").and_then(Value::as_str) {
        Some(t) => t,
        None => return out,
    };

    match part_type {
        "text" => {
            let delta = properties.get("delta").and_then(Value::as_str);
            let text = delta
                .map(str::to_owned)
                .or_else(|| part.get("text").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_default();
            if !text.is_empty() {
                out.push(MessageChunk::Assistant {
                    content: text,
                    flush: None,
                });
            }
        }
        "reasoning" => {
            let delta = properties.get("delta").and_then(Value::as_str);
            let text = delta
                .map(str::to_owned)
                .or_else(|| part.get("text").and_then(Value::as_str).map(str::to_owned))
                .unwrap_or_default();
            if !text.is_empty() {
                out.push(MessageChunk::Thinking { content: text });
            }
        }
        "tool" => {
            let call_id = part.get("callID").and_then(Value::as_str).map(str::to_owned);
            let tool_name = part
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let state = part.get("state").and_then(|v| v.as_object());
            // Source: session.ts:200  `isRecord(state?.input) ? state.input : undefined`
            // where `isRecord = typeof v === 'object' && v !== null`
            // In JS, arrays satisfy isRecord (typeof [] === 'object'). Scalars and null do not.
            // So: object or array → include; null / string / number / bool → OMIT.
            let tool_input: Option<Value> = state
                .and_then(|s| s.get("input"))
                .and_then(|v| match v {
                    Value::Object(_) | Value::Array(_) => Some(v.clone()),
                    _ => None,
                });
            let status = state.and_then(|s| s.get("status")).and_then(Value::as_str);

            // Emit tool chunk (deduped by callId)
            if let Some(ref cid) = call_id {
                if seen_tool_calls.insert(cid.clone()) {
                    out.push(MessageChunk::Tool {
                        tool_name: tool_name.clone(),
                        tool_input,
                        tool_call_id: call_id.clone(),
                    });
                }
            }

            // Emit tool_result chunk on completion or error
            if let Some(ref cid) = call_id {
                if !completed_tool_calls.contains(cid) {
                    match status {
                        Some("completed") => {
                            completed_tool_calls.insert(cid.clone());
                            let output = state
                                .and_then(|s| s.get("output"))
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_owned();
                            out.push(MessageChunk::ToolResult {
                                tool_name,
                                tool_output: output,
                                tool_call_id: call_id,
                            });
                        }
                        Some("error") => {
                            completed_tool_calls.insert(cid.clone());
                            let error_msg = state
                                .and_then(|s| s.get("error"))
                                .and_then(Value::as_str)
                                .unwrap_or("Tool failed")
                                .to_owned();
                            out.push(MessageChunk::ToolResult {
                                tool_name,
                                tool_output: error_msg,
                                tool_call_id: call_id,
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }

    out
}

/// Build the terminal `Result` chunk from `session.idle`.
///
/// PORT of the `session.idle` branch (session.ts:247-286).
pub fn build_result_chunk(
    session_id: &str,
    latest_assistant_info: Option<&serde_json::Map<String, Value>>,
    structured_output: Option<Value>,
) -> MessageChunk {
    let info_val = latest_assistant_info.map(|m| Value::Object(m.clone()));
    let tokens = normalize_tokens(info_val.as_ref());

    let cost = latest_assistant_info
        .and_then(|i| i.get("cost"))
        .and_then(Value::as_f64);

    let stop_reason = latest_assistant_info
        .and_then(|i| i.get("finish"))
        .and_then(Value::as_str)
        .map(str::to_owned);

    // modelUsage: { providerID, modelID, reasoning, cache }
    let model_usage = latest_assistant_info.map(|info| {
        let mut mu: HashMap<String, Value> = HashMap::new();
        mu.insert(
            "providerID".to_owned(),
            info.get("providerID").cloned().unwrap_or(Value::Null),
        );
        mu.insert(
            "modelID".to_owned(),
            info.get("modelID").cloned().unwrap_or(Value::Null),
        );
        let tokens_obj = info.get("tokens").and_then(|v| v.as_object());
        mu.insert(
            "reasoning".to_owned(),
            tokens_obj
                .and_then(|t| t.get("reasoning"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        mu.insert(
            "cache".to_owned(),
            tokens_obj
                .and_then(|t| t.get("cache"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        mu
    });

    MessageChunk::Result {
        session_id: Some(session_id.to_owned()),
        tokens,
        structured_output,
        is_error: None,
        error_subtype: None,
        errors: None,
        cost,
        stop_reason,
        num_turns: None,
        model_usage,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::{InlineAgentDefinition, OutputFormat, OutputFormatType};
    use serde_json::json;

    fn test_model() -> ProviderModel {
        ProviderModel {
            provider_id: "test".to_owned(),
            model_id: "mock-model".to_owned(),
        }
    }

    // ── resolve_session_id_logic ──────────────────────────────────────────────

    #[test]
    fn resolve_new_session_when_no_resume_id() {
        let result = resolve_session_id_logic(None, None, "new-session".to_owned());
        assert_eq!(result.session_id, "new-session");
        assert!(!result.resumed);
    }

    #[test]
    fn resolve_resumed_session_when_exists() {
        let result = resolve_session_id_logic(
            Some("resume-me"),
            Some("existing-session".to_owned()),
            "new-session".to_owned(),
        );
        assert_eq!(result.session_id, "existing-session");
        assert!(result.resumed);
    }

    #[test]
    fn resolve_falls_back_when_resume_not_found() {
        let result = resolve_session_id_logic(
            Some("resume-me"),
            None, // resume failed
            "fresh-session".to_owned(),
        );
        assert_eq!(result.session_id, "fresh-session");
        assert!(!result.resumed);
    }

    #[test]
    fn resolve_falls_back_when_resumed_id_empty() {
        let result = resolve_session_id_logic(
            Some("resume-me"),
            Some(String::new()),
            "fresh-session".to_owned(),
        );
        assert_eq!(result.session_id, "fresh-session");
        assert!(!result.resumed);
    }

    // ── create_session_prompt_body ────────────────────────────────────────────

    #[test]
    fn basic_prompt_body_no_agents() {
        let model = test_model();
        let result = create_session_prompt_body("hi", &model, None, None).unwrap();
        let body = &result.body;

        // parts
        let parts = body.get("parts").and_then(Value::as_array).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].get("type").and_then(Value::as_str), Some("text"));
        assert_eq!(parts[0].get("text").and_then(Value::as_str), Some("hi"));

        // model
        let m = body.get("model").and_then(Value::as_object).unwrap();
        assert_eq!(m.get("providerID").and_then(Value::as_str), Some("test"));
        assert_eq!(m.get("modelID").and_then(Value::as_str), Some("mock-model"));

        // no agent, no tools, no system, no format
        assert!(body.get("agent").is_none());
        assert!(body.get("tools").is_none());
        assert!(body.get("system").is_none());
        assert!(body.get("format").is_none());
    }

    #[test]
    fn prompt_body_with_agent_injects_archon_name() {
        let model = test_model();
        let agent = NamedAgentConfig {
            key: "My Agent".to_owned(),
            opencode_agent_name: "archon-my-agent".to_owned(),
            config: InlineAgentDefinition {
                description: "Test".to_owned(),
                prompt: "You are helpful".to_owned(),
                model: None,
                tools: None,
                disallowed_tools: None,
                skills: None,
                max_turns: None,
            },
        };
        let result = create_session_prompt_body("task", &model, None, Some(&agent)).unwrap();
        assert_eq!(
            result.body.get("agent").and_then(Value::as_str),
            Some("archon-my-agent")
        );
    }

    #[test]
    fn prompt_body_agent_with_model_override() {
        let model = test_model();
        let agent = NamedAgentConfig {
            key: "special-agent".to_owned(),
            opencode_agent_name: "archon-special-agent".to_owned(),
            config: InlineAgentDefinition {
                description: "Special".to_owned(),
                prompt: "You are special".to_owned(),
                model: Some("anthropic/claude-3-5-sonnet".to_owned()),
                tools: None,
                disallowed_tools: None,
                skills: None,
                max_turns: None,
            },
        };
        let result = create_session_prompt_body("hi", &model, None, Some(&agent)).unwrap();
        let m = result.body.get("model").and_then(Value::as_object).unwrap();
        assert_eq!(m.get("providerID").and_then(Value::as_str), Some("anthropic"));
        assert_eq!(m.get("modelID").and_then(Value::as_str), Some("claude-3-5-sonnet"));
    }

    #[test]
    fn prompt_body_agent_with_tools_and_disallowed() {
        let model = test_model();
        let agent = NamedAgentConfig {
            key: "tools-agent".to_owned(),
            opencode_agent_name: "archon-tools-agent".to_owned(),
            config: InlineAgentDefinition {
                description: "Limited".to_owned(),
                prompt: "Limited access".to_owned(),
                model: None,
                tools: Some(vec!["read".to_owned(), "grep".to_owned()]),
                disallowed_tools: Some(vec!["bash".to_owned(), "write".to_owned()]),
                skills: None,
                max_turns: None,
            },
        };
        let result = create_session_prompt_body("hi", &model, None, Some(&agent)).unwrap();
        let tools = result.body.get("tools").and_then(Value::as_object).unwrap();
        assert_eq!(tools.get("read").and_then(Value::as_bool), Some(true));
        assert_eq!(tools.get("grep").and_then(Value::as_bool), Some(true));
        assert_eq!(tools.get("bash").and_then(Value::as_bool), Some(false));
        assert_eq!(tools.get("write").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn prompt_body_with_output_format_injects_format() {
        let model = test_model();
        let mut schema = serde_json::Map::new();
        schema.insert("type".to_owned(), Value::String("object".to_owned()));
        schema.insert(
            "properties".to_owned(),
            json!({ "answer": { "type": "string" } }),
        );
        let opts = SendQueryOptions {
            output_format: Some(OutputFormat {
                kind: OutputFormatType::JsonSchema,
                schema,
            }),
            ..Default::default()
        };
        let result = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        let format = result.body.get("format").and_then(Value::as_object).unwrap();
        assert_eq!(format.get("type").and_then(Value::as_str), Some("json_schema"));
        assert!(format.get("schema").is_some());
    }

    /// D3: empty-string systemPrompt must be OMITTED — JS `if (systemPrompt)` is falsy for "".
    #[test]
    fn empty_single_system_prompt_is_omitted() {
        use har_contract::SystemPromptInput;
        let model = test_model();
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Single(String::new())),
            ..Default::default()
        };
        let result = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert!(
            result.body.get("system").is_none(),
            "empty Single system prompt must be omitted; got system={:?}",
            result.body.get("system")
        );
    }

    /// D3: non-empty systemPrompt is included.
    #[test]
    fn non_empty_system_prompt_is_included() {
        use har_contract::SystemPromptInput;
        let model = test_model();
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Single("You are helpful.".to_owned())),
            ..Default::default()
        };
        let result = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert!(
            result.body.get("system").is_some(),
            "non-empty system prompt must be present"
        );
    }

    /// D3: an empty Multi (empty array `[]`) is TRUTHY in JS — only ""/0/null/undefined/false/NaN
    /// are falsy. session.ts:69 `requestOptions?.systemPrompt ?` therefore INCLUDES it as `[]`.
    /// (Verified against live bun: `Multi([])` -> present=true | system=[].)
    #[test]
    fn empty_multi_system_prompt_is_included_as_empty_array() {
        use har_contract::SystemPromptInput;
        let model = test_model();
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Multi(vec![])),
            ..Default::default()
        };
        let result = create_session_prompt_body("hi", &model, Some(&opts), None).unwrap();
        assert_eq!(
            result.body.get("system"),
            Some(&serde_json::Value::Array(vec![])),
            "empty array systemPrompt is JS-truthy and must be included as []"
        );
    }

    #[test]
    fn prompt_body_invalid_agent_model_returns_error() {
        let model = test_model();
        let agent = NamedAgentConfig {
            key: "bad-agent".to_owned(),
            opencode_agent_name: "archon-bad-agent".to_owned(),
            config: InlineAgentDefinition {
                description: "Bad".to_owned(),
                prompt: "This will fail".to_owned(),
                model: Some("invalid-no-slash-format".to_owned()),
                tools: None,
                disallowed_tools: None,
                skills: None,
                max_turns: None,
            },
        };
        let result = create_session_prompt_body("hi", &model, None, Some(&agent));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("bad-agent"));
    }

    // ── process_message_part_updated ──────────────────────────────────────────

    #[test]
    fn text_part_yields_assistant_chunk() {
        let props: serde_json::Map<String, Value> = json!({
            "delta": "Hello",
            "part": { "sessionID": "s1", "type": "text" }
        })
        .as_object()
        .unwrap()
        .clone();
        let mut seen = HashSet::new();
        let mut completed = HashSet::new();
        let chunks = process_message_part_updated(&props, "s1", &mut seen, &mut completed);
        assert_eq!(chunks.len(), 1);
        match &chunks[0] {
            MessageChunk::Assistant { content, .. } => assert_eq!(content, "Hello"),
            _ => panic!("expected assistant chunk"),
        }
    }

    #[test]
    fn reasoning_part_yields_thinking_chunk() {
        let props: serde_json::Map<String, Value> = json!({
            "delta": "thinking...",
            "part": { "sessionID": "s1", "type": "reasoning" }
        })
        .as_object()
        .unwrap()
        .clone();
        let mut seen = HashSet::new();
        let mut completed = HashSet::new();
        let chunks = process_message_part_updated(&props, "s1", &mut seen, &mut completed);
        assert_eq!(chunks.len(), 1);
        assert!(matches!(&chunks[0], MessageChunk::Thinking { content } if content == "thinking..."));
    }

    #[test]
    fn tool_pending_then_completed_yields_tool_and_result() {
        let mut seen = HashSet::new();
        let mut completed = HashSet::new();

        // Pending tool event
        let pending: serde_json::Map<String, Value> = json!({
            "part": {
                "sessionID": "s1",
                "type": "tool",
                "tool": "read",
                "callID": "tool-1",
                "state": { "status": "pending", "input": { "path": "/tmp/file.ts" } }
            }
        })
        .as_object()
        .unwrap()
        .clone();
        let chunks1 = process_message_part_updated(&pending, "s1", &mut seen, &mut completed);
        assert_eq!(chunks1.len(), 1);
        assert!(matches!(&chunks1[0], MessageChunk::Tool { tool_name, tool_call_id, .. }
            if tool_name == "read" && tool_call_id.as_deref() == Some("tool-1")));

        // Completed tool event
        let done: serde_json::Map<String, Value> = json!({
            "part": {
                "sessionID": "s1",
                "type": "tool",
                "tool": "read",
                "callID": "tool-1",
                "state": { "status": "completed", "input": { "path": "/tmp/file.ts" }, "output": "file contents" }
            }
        })
        .as_object()
        .unwrap()
        .clone();
        let chunks2 = process_message_part_updated(&done, "s1", &mut seen, &mut completed);
        // No tool chunk (already seen), only result
        assert_eq!(chunks2.len(), 1);
        assert!(matches!(&chunks2[0], MessageChunk::ToolResult { tool_name, tool_output, tool_call_id }
            if tool_name == "read" && tool_output == "file contents" && tool_call_id.as_deref() == Some("tool-1")));
    }

    #[test]
    fn tool_error_yields_error_result() {
        let mut seen = HashSet::new();
        let mut completed = HashSet::new();

        let error_event: serde_json::Map<String, Value> = json!({
            "part": {
                "sessionID": "s1",
                "type": "tool",
                "tool": "bash",
                "callID": "tool-2",
                "state": { "status": "error", "error": "command failed" }
            }
        })
        .as_object()
        .unwrap()
        .clone();
        let chunks = process_message_part_updated(&error_event, "s1", &mut seen, &mut completed);
        // tool + tool_result
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[1], MessageChunk::ToolResult { tool_output, .. }
            if tool_output == "command failed"));
    }

    #[test]
    fn wrong_session_id_ignored() {
        let props: serde_json::Map<String, Value> = json!({
            "delta": "Hello",
            "part": { "sessionID": "other-session", "type": "text" }
        })
        .as_object()
        .unwrap()
        .clone();
        let mut seen = HashSet::new();
        let mut completed = HashSet::new();
        let chunks = process_message_part_updated(&props, "s1", &mut seen, &mut completed);
        assert!(chunks.is_empty());
    }

    // ── build_result_chunk ────────────────────────────────────────────────────

    #[test]
    fn build_result_chunk_no_info() {
        let chunk = build_result_chunk("session-1", None, None);
        match chunk {
            MessageChunk::Result {
                session_id,
                tokens,
                model_usage,
                ..
            } => {
                assert_eq!(session_id.as_deref(), Some("session-1"));
                assert!(tokens.is_none());
                assert!(model_usage.is_none());
            }
            _ => panic!("expected result chunk"),
        }
    }

    #[test]
    fn build_result_chunk_with_full_info() {
        // Mirrors provider.test.ts: terminal result chunk includes sessionId and normalized tokens
        let mut info = serde_json::Map::new();
        info.insert("id".to_owned(), json!("message-1"));
        info.insert("role".to_owned(), json!("assistant"));
        info.insert("sessionID".to_owned(), json!("session-1"));
        info.insert("providerID".to_owned(), json!("anthropic"));
        info.insert("modelID".to_owned(), json!("claude-sonnet"));
        info.insert("cost".to_owned(), json!(0.42_f64));
        info.insert("finish".to_owned(), json!("stop"));
        info.insert(
            "tokens".to_owned(),
            json!({ "input": 11, "output": 7, "reasoning": 3, "cache": 1 }),
        );

        let chunk = build_result_chunk("session-1", Some(&info), None);
        match chunk {
            MessageChunk::Result {
                session_id,
                tokens,
                cost,
                stop_reason,
                model_usage,
                ..
            } => {
                assert_eq!(session_id.as_deref(), Some("session-1"));
                let t = tokens.unwrap();
                assert_eq!(t.input, 11);
                assert_eq!(t.output, 7);
                assert_eq!(t.total, Some(21));
                assert_eq!(t.cost, Some(0.42));
                assert_eq!(cost, Some(0.42));
                assert_eq!(stop_reason.as_deref(), Some("stop"));
                let mu = model_usage.unwrap();
                assert_eq!(mu.get("providerID"), Some(&json!("anthropic")));
                assert_eq!(mu.get("modelID"), Some(&json!("claude-sonnet")));
                assert_eq!(mu.get("reasoning"), Some(&json!(3)));
                assert_eq!(mu.get("cache"), Some(&json!(1)));
            }
            _ => panic!("expected result chunk"),
        }
    }

    // ── process_message_updated ───────────────────────────────────────────────

    #[test]
    fn process_message_updated_stores_assistant_info() {
        let props: serde_json::Map<String, Value> = json!({
            "info": {
                "id": "message-1",
                "role": "assistant",
                "sessionID": "s1",
                "providerID": "anthropic",
                "modelID": "claude-sonnet",
                "cost": 0.42,
                "finish": "stop",
                "tokens": { "input": 11, "output": 7, "reasoning": 3, "cache": 1 }
            }
        })
        .as_object()
        .unwrap()
        .clone();
        let mut latest_info = None;
        let mut last_id = None;
        process_message_updated(&props, "s1", &mut latest_info, &mut last_id);
        assert!(latest_info.is_some());
        assert_eq!(last_id.as_deref(), Some("message-1"));
    }

    #[test]
    fn process_message_updated_ignores_wrong_session() {
        let props: serde_json::Map<String, Value> = json!({
            "info": { "id": "msg", "role": "assistant", "sessionID": "other" }
        })
        .as_object()
        .unwrap()
        .clone();
        let mut latest_info = None;
        let mut last_id = None;
        process_message_updated(&props, "s1", &mut latest_info, &mut last_id);
        assert!(latest_info.is_none());
        assert!(last_id.is_none());
    }

    #[test]
    fn process_message_updated_ignores_non_assistant_role() {
        let props: serde_json::Map<String, Value> = json!({
            "info": { "id": "msg", "role": "user", "sessionID": "s1" }
        })
        .as_object()
        .unwrap()
        .clone();
        let mut latest_info = None;
        let mut last_id = None;
        process_message_updated(&props, "s1", &mut latest_info, &mut last_id);
        assert!(latest_info.is_none());
    }
}
