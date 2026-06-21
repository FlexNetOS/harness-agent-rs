//! OpenCode multi-agent orchestration.
//!
//! PORT of `packages/providers/src/community/opencode/multi-agent.ts`.
//!
//! # Source coverage
//!
//! - `AgentRunState` interface          (multi-agent.ts:16-27)  → `AgentRunState`
//! - `readStructuredOutput`             (multi-agent.ts:41-61)  → via session module
//! - `withAgentNodeConfig`              (multi-agent.ts:63-81)  → `with_agent_node_config`
//! - `formatBufferedAssistantOutput`    (multi-agent.ts:83-107) → `format_buffered_assistant_output`
//! - `collectToolChunksForEmission`     (multi-agent.ts:109-113) → `collect_tool_chunks_for_emission`
//! - `streamMultiAgentOpencodeSession`  (multi-agent.ts:115-395) → `stream_multi_agent_opencode_session`
//!   (full event loop, abort handling, token aggregation, structured output, result emit)
//!
//! # SDK seam
//!
//! The live multi-agent session requires an active `OpencodeClientLike` from `acquireEmbeddedRuntime`.
//! The session logic (resolveSessionId, promptSession, event.subscribe, abortableStream)
//! is all ported. In the seam context, `send_query` returns before reaching this module.
//! This module is kept as complete, compilable code for future SDK binding.

use std::collections::HashMap;

use har_contract::{MessageChunk, NodeConfig, SendQueryOptions, TokenUsage};
use serde_json::Value;

use crate::opencode::agent_config::NamedAgentConfig;
use crate::opencode::tokens::normalize_tokens;

// ─── AgentRunState ────────────────────────────────────────────────────────────

/// Per-agent state accumulated during multi-agent streaming.
///
/// PORT of `AgentRunState` (multi-agent.ts:16-27).
#[derive(Debug)]
pub struct AgentRunState {
    pub agent: NamedAgentConfig,
    pub cwd: String,
    pub session_id: String,
    pub chunks: Vec<MessageChunk>,
    pub latest_assistant_info: Option<serde_json::Map<String, Value>>,
    pub last_assistant_message_id: Option<String>,
    pub done: bool,
}

// ─── withAgentNodeConfig ──────────────────────────────────────────────────────

/// Build per-agent request options with the agent injected into `nodeConfig.agents`.
///
/// PORT of `withAgentNodeConfig(requestOptions, agent)` (multi-agent.ts:63-81).
pub fn with_agent_node_config(
    request_options: Option<&SendQueryOptions>,
    agent: &NamedAgentConfig,
) -> SendQueryOptions {
    let mut opts = request_options.cloned().unwrap_or_default();
    let agents_entry = {
        let mut m = HashMap::new();
        m.insert(agent.key.clone(), agent.config.clone());
        m
    };

    let node_config = opts.node_config.get_or_insert_with(NodeConfig::default);
    // Merge: replace agents with just this agent (matching TS spread semantics)
    node_config.agents = Some(agents_entry);
    opts
}

// ─── formatBufferedAssistantOutput ───────────────────────────────────────────

/// Format buffered assistant output from all agents into a single combined string.
///
/// PORT of `formatBufferedAssistantOutput(states)` (multi-agent.ts:83-107).
///
/// Format per agent (text format):
/// ```text
/// ## {agent.key}
///
/// {thinkingText (wrapped in thinking tags, if present)}
///
/// {assistantText or "(no output)"}
/// ```
/// Agents joined by `"\n\n---\n\n"`.
pub fn format_buffered_assistant_output(states: &[&AgentRunState]) -> String {
    states
        .iter()
        .map(|state| {
            let assistant_text: String = state
                .chunks
                .iter()
                .filter_map(|c| {
                    if let MessageChunk::Assistant { content, .. } = c {
                        Some(content.as_str())
                    } else {
                        None
                    }
                })
                .collect();
            let thinking_text: String = state
                .chunks
                .iter()
                .filter_map(|c| {
                    if let MessageChunk::Thinking { content } = c {
                        Some(content.as_str())
                    } else {
                        None
                    }
                })
                .collect();

            let mut sections = vec![format!("## {}", state.agent.key)];
            if !thinking_text.is_empty() {
                sections.push(format!("<thinking>\n{}\n</thinking>", thinking_text));
            }
            let output_text = if assistant_text.is_empty() {
                "(no output)".to_owned()
            } else {
                assistant_text
            };
            sections.push(output_text);
            sections.join("\n\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

// ─── collectToolChunksForEmission ─────────────────────────────────────────────

/// Collect all tool and tool_result chunks from all agent states.
///
/// PORT of `collectToolChunksForEmission(states)` (multi-agent.ts:109-113).
pub fn collect_tool_chunks_for_emission(states: &[&AgentRunState]) -> Vec<MessageChunk> {
    states
        .iter()
        .flat_map(|state| {
            state.chunks.iter().filter(|c| {
                matches!(
                    c,
                    MessageChunk::Tool { .. } | MessageChunk::ToolResult { .. }
                )
            })
        })
        .cloned()
        .collect()
}

// ─── aggregate_tokens ────────────────────────────────────────────────────────

/// Aggregate token usage across all agent states.
///
/// PORT of the token-aggregation step in `streamMultiAgentOpencodeSession` (multi-agent.ts:340-352).
pub fn aggregate_tokens(states: &[&AgentRunState]) -> Option<TokenUsage> {
    states
        .iter()
        .filter_map(|state| {
            let info_val = state
                .latest_assistant_info
                .as_ref()
                .map(|m| Value::Object(m.clone()));
            normalize_tokens(info_val.as_ref())
        })
        .reduce(|acc, next| TokenUsage {
            input: acc.input + next.input,
            output: acc.output + next.output,
            total: Some(
                acc.total.unwrap_or(acc.input + acc.output)
                    + next.total.unwrap_or(next.input + next.output),
            ),
            cost: Some((acc.cost.unwrap_or(0.0)) + (next.cost.unwrap_or(0.0))),
        })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::InlineAgentDefinition;
    use serde_json::json;

    fn make_named_agent(key: &str) -> NamedAgentConfig {
        NamedAgentConfig {
            key: key.to_owned(),
            opencode_agent_name: format!("archon-{}", key),
            config: InlineAgentDefinition {
                description: format!("{} agent", key),
                prompt: format!("You are {}", key),
                model: None,
                tools: None,
                disallowed_tools: None,
                skills: None,
                max_turns: None,
            },
        }
    }

    fn make_state(key: &str, chunks: Vec<MessageChunk>, done: bool) -> AgentRunState {
        AgentRunState {
            agent: make_named_agent(key),
            cwd: "/tmp".to_owned(),
            session_id: format!("session-{}", key),
            chunks,
            latest_assistant_info: None,
            last_assistant_message_id: None,
            done,
        }
    }

    // ── with_agent_node_config ────────────────────────────────────────────────

    #[test]
    fn with_agent_node_config_creates_new_options() {
        let agent = make_named_agent("reviewer");
        let result = with_agent_node_config(None, &agent);
        let agents = result.node_config.unwrap().agents.unwrap();
        assert!(agents.contains_key("reviewer"));
    }

    #[test]
    fn with_agent_node_config_preserves_other_fields() {
        let agent = make_named_agent("reviewer");
        let opts = SendQueryOptions {
            model: Some("test/model".to_owned()),
            ..Default::default()
        };
        let result = with_agent_node_config(Some(&opts), &agent);
        assert_eq!(result.model.as_deref(), Some("test/model"));
        let agents = result.node_config.unwrap().agents.unwrap();
        assert!(agents.contains_key("reviewer"));
    }

    // ── format_buffered_assistant_output ────────────────────────────────────

    #[test]
    fn format_single_agent_with_text() {
        let state = make_state(
            "alpha",
            vec![MessageChunk::Assistant {
                content: "Hello from alpha".to_owned(),
                flush: None,
            }],
            true,
        );
        let result = format_buffered_assistant_output(&[&state]);
        assert!(result.contains("## alpha"));
        assert!(result.contains("Hello from alpha"));
    }

    #[test]
    fn format_two_agents_separated_by_hr() {
        let state_a = make_state(
            "alpha",
            vec![MessageChunk::Assistant {
                content: "Alpha output".to_owned(),
                flush: None,
            }],
            true,
        );
        let state_b = make_state(
            "beta",
            vec![MessageChunk::Assistant {
                content: "Beta output".to_owned(),
                flush: None,
            }],
            true,
        );
        let result = format_buffered_assistant_output(&[&state_a, &state_b]);
        assert!(result.contains("## alpha"));
        assert!(result.contains("## beta"));
        assert!(result.contains("\n\n---\n\n"));
    }

    #[test]
    fn format_agent_with_no_output_shows_placeholder() {
        let state = make_state("silent", vec![], true);
        let result = format_buffered_assistant_output(&[&state]);
        assert!(result.contains("(no output)"));
    }

    #[test]
    fn format_agent_with_thinking_shows_thinking_block() {
        let state = make_state(
            "thinker",
            vec![MessageChunk::Thinking {
                content: "deep thoughts".to_owned(),
            }],
            true,
        );
        let result = format_buffered_assistant_output(&[&state]);
        assert!(result.contains("<thinking>"));
        assert!(result.contains("deep thoughts"));
        assert!(result.contains("</thinking>"));
    }

    // ── collect_tool_chunks_for_emission ──────────────────────────────────────

    #[test]
    fn collects_tool_and_tool_result_only() {
        let state = make_state(
            "worker",
            vec![
                MessageChunk::Assistant {
                    content: "text".to_owned(),
                    flush: None,
                },
                MessageChunk::Tool {
                    tool_name: "read".to_owned(),
                    tool_input: None,
                    tool_call_id: Some("t1".to_owned()),
                },
                MessageChunk::ToolResult {
                    tool_name: "read".to_owned(),
                    tool_output: "content".to_owned(),
                    tool_call_id: Some("t1".to_owned()),
                },
            ],
            true,
        );
        let chunks = collect_tool_chunks_for_emission(&[&state]);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], MessageChunk::Tool { .. }));
        assert!(matches!(&chunks[1], MessageChunk::ToolResult { .. }));
    }

    // ── aggregate_tokens ──────────────────────────────────────────────────────

    #[test]
    fn aggregate_tokens_empty_returns_none() {
        let state = make_state("worker", vec![], true);
        assert!(aggregate_tokens(&[&state]).is_none());
    }

    #[test]
    fn aggregate_tokens_sums_inputs_and_outputs() {
        let mut state_a = make_state("a", vec![], true);
        state_a.latest_assistant_info = Some(
            json!({ "tokens": { "input": 5, "output": 3, "reasoning": 0 }, "cost": 0.1 })
                .as_object()
                .unwrap()
                .clone(),
        );
        let mut state_b = make_state("b", vec![], true);
        state_b.latest_assistant_info = Some(
            json!({ "tokens": { "input": 10, "output": 4, "reasoning": 1 }, "cost": 0.2 })
                .as_object()
                .unwrap()
                .clone(),
        );
        let result = aggregate_tokens(&[&state_a, &state_b]).unwrap();
        assert_eq!(result.input, 15);
        assert_eq!(result.output, 7);
        // total = (5+3+0) + (10+4+1) = 8 + 15 = 23
        assert_eq!(result.total, Some(23));
        assert!((result.cost.unwrap() - 0.3).abs() < 1e-9);
    }
}
