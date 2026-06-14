//! `build_claude_argv` — deterministic Claude CLI argv builder.
//!
//! Port of `buildBaseClaudeOptions` (provider.ts:496-561) and `applyNodeConfig`
//! (provider.ts:275-442) — the option→flag mapping table from target-architecture.md §6.2.
//!
//! This is the DETERMINISTIC unit: no live processes, no I/O, no env reads.
//! Tests feed `(SendQueryOptions, NodeConfig)` inputs and assert exact argv.
//!
//! # Option → CLI flag mapping (target-arch §6.2 table)
//!
//! | Field (source)                          | CLI flag/value                                      |
//! |-----------------------------------------|-----------------------------------------------------|
//! | model (provider.ts:516)                 | `--model <id>`                                      |
//! | fallbackModel (524)                     | `--fallback-model <id>`                             |
//! | resume=resumeSessionId (936)            | `--resume <session_id>`                             |
//! | forkSession (530)                       | `--fork-session`                                    |
//! | persistSession: false (527)             | `--no-session-persistence` (only when explicitly false)|
//! | permissionMode:'bypassPermissions'      | `--permission-mode bypassPermissions`               |
//! | allowDangerouslySkipPermissions (534)   | `--dangerously-skip-permissions`                    |
//! | systemPrompt preset/append (535)        | `--system-prompt` / `--append-system-prompt`        |
//! | excludeDynamicSections: true (types.ts:233)| `--exclude-dynamic-system-prompt-sections` (only when true)|
//! | settingSources (536)                    | `--setting-sources project,user`                    |
//! | allowed_tools/tools (282-284)           | → `options.tools` (agent roster, no direct CLI flag)|
//! | denied_tools/disallowedTools (287-289)  | `--disallowed-tools a,b,c`                          |
//! | MCP wildcards (324) + Skill (367)       | `--allowed-tools a,b,c` (permission allowlist)      |
//! | mcp config (319-333)                    | `--mcp-config <file>` + add `mcp__<s>__*` to tools |
//! | agents/skills (345-396)                 | `--agents <json>`                                   |
//! | effort (399)                            | `--effort <level>`                                  |
//! | thinking (404)                          | `--thinking <json>`                                 |
//! | sandbox (409)                           | `--sandbox <json>`                                  |
//! | betas (414)                             | `--betas a,b,c`                                     |
//! | output_format json_schema (418-424)     | `--output-format-schema <json>`                     |
//! | maxBudgetUsd (521)                      | `--max-budget-usd <n>`                              |
//! | hooks (declarative, 291-316)            | NOT in argv — written to --settings file (seam)     |
//! | executableArgs:['--no-env-file'] (514)  | prepended to argv when CLI path is a JS file        |
//! | pathToClaudeCodeExecutable (513)        | the spawned program path (separate from argv)       |
//! | nativeTools (R8 NEEDS-HUMAN sidecar)   | DEFERRED — documented seam below                    |
//!
//! # Transport flags (always added by build_claude_argv)
//! - `--print` — non-interactive mode
//! - `--output-format stream-json` — NDJSON event stream transport
//! - `--verbose`
//! - `--input-format text`

use serde_json::Value;

use har_contract::{ClaudeProviderDefaults, NodeConfig, SendQueryOptions, SystemPromptInput};

use crate::claude::native_tools::ARCHON_TOOL_SERVER;

/// A structured warning yielded by `build_claude_argv` before streaming starts.
///
/// Port of `ProviderWarning` (provider.ts:263-266) and the warning emission at
/// provider.ts:879-882.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderWarning {
    pub code: String,
    pub message: String,
}

/// The transport flags always appended by this builder.
///
/// Source: target-architecture.md §6.0:
/// `--print --output-format stream-json --verbose --input-format text`
pub const TRANSPORT_FLAGS: &[&str] = &[
    "--print",
    "--output-format",
    "stream-json",
    "--verbose",
    "--input-format",
    "text",
];

/// Build the complete Claude CLI argv from request options, node config, and defaults.
///
/// Returns `(argv, warnings)` where:
/// - `argv` is the argument list to pass after the executable path.
/// - `warnings` are provider warnings to yield as system chunks before streaming.
///
/// `resume_session_id` is passed separately (from `sendQuery` signature, provider.ts:936).
///
/// # Notes on deferred features
///
/// - **MCP config** (`nodeConfig.mcp`): The argv builder adds `--mcp-config <path>` and
///   `mcp__<server>__*` wildcards to `--allowed-tools`. The actual file loading
///   (`loadMcpConfig`) is deferred to the caller — this function takes pre-loaded
///   `mcp_server_names` and `mcp_missing_vars` so it remains pure/deterministic.
///
/// - **R8 Native tools sidecar**: `native_tools_mcp_config_path` is a documented seam.
///   When Some, `--mcp-config <path>` for the archon sidecar server is added, plus
///   `mcp__archon__*` in `--allowed-tools`. The actual sidecar launch is the caller's job.
///   DEFERRED per cycle-13 scope.
///
/// - **Hooks** (declarative YAML): written to a `--settings` file by the caller; not in argv.
#[allow(clippy::too_many_arguments)]
pub fn build_claude_argv(
    request_options: Option<&SendQueryOptions>,
    node_config: Option<&NodeConfig>,
    defaults: &ClaudeProviderDefaults,
    resume_session_id: Option<&str>,
    cli_path: Option<&str>,
    // Pre-loaded MCP info (from loadMcpConfig — caller handles async I/O)
    mcp_server_names: &[String],
    mcp_missing_vars: &[String],
    // R8 seam: path to the archon sidecar MCP config file, if native tools are registered.
    // DEFERRED — not implemented this cycle; pass None until the sidecar is built.
    native_tools_mcp_config_path: Option<&str>,
) -> (Vec<String>, Vec<ProviderWarning>) {
    let mut argv: Vec<String> = Vec::new();
    let mut warnings: Vec<ProviderWarning> = Vec::new();

    // ── Executable args (before argv proper) — only for JS executables ────────
    // provider.ts:514 / shouldPassNoEnvFile
    if let Some(path) = cli_path {
        if crate::claude::binary_resolver::should_pass_no_env_file(Some(path)) {
            argv.push("--no-env-file".to_owned());
        }
    }

    // ── Transport flags (always) ──────────────────────────────────────────────
    for flag in TRANSPORT_FLAGS {
        argv.push(flag.to_string());
    }

    // ── Model ─────────────────────────────────────────────────────────────────
    // provider.ts:516: options.model = requestOptions?.model ?? assistantDefaults.model
    let model = request_options
        .and_then(|o| o.model.as_deref())
        .or(defaults.model.as_deref());
    if let Some(m) = model {
        argv.push("--model".to_owned());
        argv.push(m.to_owned());
    }

    // ── Fallback model ────────────────────────────────────────────────────────
    // provider.ts:524 / applyNodeConfig:437
    let fallback_model = request_options
        .and_then(|o| o.fallback_model.as_deref())
        .or_else(|| node_config.and_then(|n| n.fallback_model.as_deref()));
    if let Some(fm) = fallback_model {
        argv.push("--fallback-model".to_owned());
        argv.push(fm.to_owned());
    }

    // ── Max budget ────────────────────────────────────────────────────────────
    // provider.ts:521 / applyNodeConfig:427
    let max_budget = request_options
        .and_then(|o| o.max_budget_usd)
        .or_else(|| node_config.and_then(|n| n.max_budget_usd));
    if let Some(b) = max_budget {
        argv.push("--max-budget-usd".to_owned());
        argv.push(b.to_string());
    }

    // ── Permission mode (always bypassPermissions) ────────────────────────────
    // provider.ts:533-534
    argv.push("--permission-mode".to_owned());
    argv.push("bypassPermissions".to_owned());
    argv.push("--dangerously-skip-permissions".to_owned());

    // ── System prompt ─────────────────────────────────────────────────────────
    // provider.ts:535: systemPrompt = requestOptions?.systemPrompt ?? { type: 'preset', preset: 'claude_code' }
    // nodeConfig.systemPrompt (applyNodeConfig:432) overrides if present.
    let sys_prompt = node_config
        .and_then(|n| n.system_prompt.as_ref())
        .or_else(|| request_options.and_then(|o| o.system_prompt.as_ref()));
    match sys_prompt {
        Some(SystemPromptInput::Single(s)) => {
            argv.push("--system-prompt".to_owned());
            argv.push(s.clone());
        }
        Some(SystemPromptInput::Multi(parts)) => {
            argv.push("--system-prompt".to_owned());
            argv.push(parts.join("\n"));
        }
        Some(SystemPromptInput::Preset(preset)) => {
            // Preset name is always "claude_code" — default; no explicit flag needed
            // unless there's an append.
            if let Some(append) = &preset.append {
                argv.push("--append-system-prompt".to_owned());
                argv.push(append.clone());
            }
            // excludeDynamicSections: true → --exclude-dynamic-system-prompt-sections
            // Source: types.ts:233 (SystemPromptPreset.excludeDynamicSections).
            // CLI flag confirmed: claude --help 2.1.177.
            // Only emit on true; false/absent is the CLI default (sections included).
            if preset.exclude_dynamic_sections == Some(true) {
                argv.push("--exclude-dynamic-system-prompt-sections".to_owned());
            }
        }
        None => {
            // Default: { type: 'preset', preset: 'claude_code' } — no argv flag needed,
            // it's the CLI's default mode.
        }
    }

    // ── Setting sources ───────────────────────────────────────────────────────
    // provider.ts:536: settingSources = assistantDefaults.settingSources ?? ['project', 'user']
    let setting_sources: Vec<String> = defaults
        .setting_sources
        .as_ref()
        .map(|ss| ss.iter().map(|s| format!("{:?}", s).to_lowercase()).collect())
        .unwrap_or_else(|| vec!["project".to_owned(), "user".to_owned()]);
    argv.push("--setting-sources".to_owned());
    argv.push(setting_sources.join(","));

    // ── Fork session ──────────────────────────────────────────────────────────
    // provider.ts:530
    let fork = request_options.and_then(|o| o.fork_session);
    if fork == Some(true) {
        argv.push("--fork-session".to_owned());
    }

    // ── Persist session ───────────────────────────────────────────────────────
    // provider.ts:527-529: `persistSession` is passed to the SDK when !== undefined.
    // The SDK maps persistSession:false → --no-session-persistence.
    // CLI flag confirmed: claude --help 2.1.177 ("only works with --print"; we pass --print).
    // Only emit on explicit false; true/absent is the CLI default (sessions persisted).
    // Source: types.ts:253 (AgentRequestOptions.persistSession).
    let persist = request_options.and_then(|o| o.persist_session);
    if persist == Some(false) {
        argv.push("--no-session-persistence".to_owned());
    }

    // ── Resume session ────────────────────────────────────────────────────────
    // provider.ts:935-937
    if let Some(session_id) = resume_session_id {
        argv.push("--resume".to_owned());
        argv.push(session_id.to_owned());
    }

    // ── Effort ────────────────────────────────────────────────────────────────
    // applyNodeConfig:399
    let effort = node_config.and_then(|n| n.effort.as_deref());
    if let Some(e) = effort {
        argv.push("--effort".to_owned());
        argv.push(e.to_owned());
    }

    // ── Thinking ─────────────────────────────────────────────────────────────
    // applyNodeConfig:404
    let thinking = node_config.and_then(|n| n.thinking.as_ref());
    if let Some(t) = thinking {
        argv.push("--thinking".to_owned());
        argv.push(serde_json::to_string(t).unwrap_or_default());
    }

    // ── Sandbox ───────────────────────────────────────────────────────────────
    // applyNodeConfig:409
    let sandbox = node_config.and_then(|n| n.sandbox.as_ref());
    if let Some(s) = sandbox {
        argv.push("--sandbox".to_owned());
        argv.push(serde_json::to_string(s).unwrap_or_default());
    }

    // ── Betas ─────────────────────────────────────────────────────────────────
    // applyNodeConfig:414
    let betas = node_config.and_then(|n| n.betas.as_ref());
    if let Some(b) = betas {
        if !b.is_empty() {
            argv.push("--betas".to_owned());
            argv.push(b.join(","));
        }
    }

    // ── Output format schema ──────────────────────────────────────────────────
    // applyNodeConfig:418-424 / provider.ts:518 (requestOptions.outputFormat)
    // Two sources: requestOptions.outputFormat (provider.ts:518) and nodeConfig.output_format
    // NodeConfig takes precedence (applyNodeConfig runs after buildBaseClaudeOptions).
    let output_format_schema: Option<Value> = node_config
        .and_then(|n| n.output_format.as_ref())
        .map(|m| serde_json::to_value(m).unwrap_or(Value::Null))
        .or_else(|| {
            request_options
                .and_then(|o| o.output_format.as_ref())
                .map(|of| serde_json::to_value(&of.schema).unwrap_or(Value::Null))
        });
    if let Some(schema) = output_format_schema {
        if !schema.is_null() {
            argv.push("--output-format-schema".to_owned());
            argv.push(serde_json::to_string(&schema).unwrap_or_default());
        }
    }

    // ── Agent roster tools ────────────────────────────────────────────────────
    // provider.ts:282-284: nodeConfig.allowed_tools → options.tools (the AGENT ROSTER).
    // This is NOT options.allowedTools and has NO direct CLI flag. It flows into
    // agentDef.tools when skills are present (provider.ts:360-361).
    let agent_roster_tools: Vec<String> = node_config
        .and_then(|n| n.allowed_tools.as_ref())
        .cloned()
        .unwrap_or_default();

    // ── Denied tools ──────────────────────────────────────────────────────────
    // applyNodeConfig:287-289
    let denied_tools: Vec<String> = node_config
        .and_then(|n| n.denied_tools.as_ref())
        .cloned()
        .unwrap_or_default();

    // ── Permission allowlist (→ --allowed-tools) ──────────────────────────────
    // provider.ts:324 / 367 / 927: assembled ONLY from MCP wildcards + Skill + sidecar
    // wildcard. nodeConfig.allowed_tools does NOT flow here — it is options.tools, not
    // options.allowedTools.
    //
    // ORDER FIX (cycle-14 `- [!]`): source runs MCP block BEFORE skills block.
    //   provider.ts:324  → MCP wildcards appended first.
    //   provider.ts:367  → 'Skill' appended after.
    // Result: [...mcpWildcards, 'Skill']  (MCP first).
    // Rust now matches this order by handling MCP config before the skills block.
    let mut permission_allowlist: Vec<String> = Vec::new();

    // ── MCP config ────────────────────────────────────────────────────────────
    // applyNodeConfig:319-333; MCP block runs BEFORE skills — wildcards appended first.
    // (provider.ts:319-343, then skills at provider.ts:345-368)
    if let Some(node_cfg) = node_config {
        if let Some(mcp_path) = &node_cfg.mcp {
            argv.push("--mcp-config".to_owned());
            argv.push(mcp_path.clone());
            // provider.ts:323-324: add mcp__<server>__* wildcards to permission_allowlist
            for name in mcp_server_names {
                let wildcard = format!("mcp__{}__*", name);
                if !permission_allowlist.contains(&wildcard) {
                    permission_allowlist.push(wildcard);
                }
            }
            // Haiku warning: provider.ts:335-342
            if model.map(|m| m.to_lowercase().contains("haiku")).unwrap_or(false) {
                warnings.push(ProviderWarning {
                    code: "mcp_haiku_tool_search".to_owned(),
                    message: "Using Haiku model with MCP servers \u{2014} tool search (lazy loading for many tools) is not supported on Haiku. Consider using Sonnet or Opus.".to_owned(),
                });
            }
            // Missing env vars warning: provider.ts:327-333
            if !mcp_missing_vars.is_empty() {
                let unique_vars: Vec<String> = {
                    let mut seen = std::collections::HashSet::new();
                    mcp_missing_vars.iter().filter(|v| seen.insert(*v)).cloned().collect()
                };
                warnings.push(ProviderWarning {
                    code: "mcp_env_vars_missing".to_owned(),
                    message: format!(
                        "MCP config references undefined env vars: {}. These will be empty strings \u{2014} MCP servers may fail to authenticate.",
                        unique_vars.join(", ")
                    ),
                });
            }
        }
    }

    // ── Skills → agents ───────────────────────────────────────────────────────
    // applyNodeConfig:345-368 — runs AFTER MCP block; 'Skill' appended to permission_allowlist
    // AFTER MCP wildcards (see ORDER FIX above).
    let mut agents_map: serde_json::Map<String, Value> = serde_json::Map::new();
    let mut agent_id_for_query: Option<String> = None;

    if let Some(skills) = node_config.and_then(|n| n.skills.as_ref()) {
        if !skills.is_empty() {
            let skill_agent_id = "dag-node-skills";
            let mut agent_def = serde_json::json!({
                "description": "DAG node with skills",
                "prompt": format!("You have preloaded skills: {}. Use them when relevant.", skills.join(", ")),
                "skills": skills,
            });
            // provider.ts:360-361: if options.tools is set (= agent_roster_tools), add Skill to
            // the agent's tools list. options.tools == agent_roster_tools here.
            if !agent_roster_tools.is_empty() {
                let mut tools_with_skill = agent_roster_tools.clone();
                tools_with_skill.push("Skill".to_owned());
                agent_def["tools"] = Value::Array(
                    tools_with_skill.into_iter().map(Value::String).collect()
                );
            }
            if let Some(m) = model {
                agent_def["model"] = Value::String(m.to_owned());
            }
            agents_map.insert(skill_agent_id.to_owned(), agent_def);
            agent_id_for_query = Some(skill_agent_id.to_owned());

            // provider.ts:366-368: add 'Skill' to options.allowedTools AFTER MCP wildcards.
            if !permission_allowlist.contains(&"Skill".to_owned()) {
                permission_allowlist.push("Skill".to_owned());
            }
        }
    }

    // ── Inline agents (after skills — user agents win on id collision) ────────
    // applyNodeConfig:372-396
    if let Some(agents) = node_config.and_then(|n| n.agents.as_ref()) {
        for (id, def) in agents {
            let v = serde_json::to_value(def).unwrap_or(Value::Null);
            agents_map.insert(id.clone(), v);
        }
    }

    // Output agents / agent flags
    if !agents_map.is_empty() {
        let agents_json = Value::Object(agents_map);
        argv.push("--agents".to_owned());
        argv.push(serde_json::to_string(&agents_json).unwrap_or_default());
    }
    if let Some(aid) = agent_id_for_query {
        argv.push("--agent".to_owned());
        argv.push(aid);
    }

    // ── R8 Native tools sidecar seam ──────────────────────────────────────────
    // provider.ts:922-932; DEFERRED — sidecar design is NEEDS-HUMAN.
    // When owner approves option (a), this seam adds:
    //   --mcp-config <native_tools_mcp_config_path>
    //   mcp__archon__* to allowed-tools
    // For now: if a path is provided, we document the intent but do NOT silently drop it.
    if let Some(sidecar_path) = native_tools_mcp_config_path {
        // DEFERRED: R8 sidecar not yet implemented. The path is noted here as a seam.
        // When implemented, this block will add --mcp-config and mcp__archon__*.
        // For now we log that it was requested.
        tracing::warn!(
            sidecar_config = sidecar_path,
            "build_claude_argv: native_tools_mcp_config_path provided but R8 sidecar is DEFERRED (NEEDS-HUMAN). \
             nativeTools NOT silently dropped — awaiting owner decision on option (a)/(b)/(c)."
        );
        // DO NOT set nativeTools=false. The seam is here; the sidecar impl is the follow-up.
        let wildcard = format!("mcp__{}__*", ARCHON_TOOL_SERVER);
        if !permission_allowlist.contains(&wildcard) {
            permission_allowlist.push(wildcard);
        }
        argv.push("--mcp-config".to_owned());
        argv.push(sidecar_path.to_owned());
    }

    // ── Emit allowed-tools / denied-tools ────────────────────────────────────
    // --allowed-tools = MCP wildcards + Skill + sidecar wildcard ONLY.
    // nodeConfig.allowed_tools goes to options.tools (agent roster), NOT here.
    if !permission_allowlist.is_empty() {
        argv.push("--allowed-tools".to_owned());
        argv.push(permission_allowlist.join(","));
    }
    if !denied_tools.is_empty() {
        argv.push("--disallowed-tools".to_owned());
        argv.push(denied_tools.join(","));
    }

    (argv, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::{ClaudeProviderDefaults, NodeConfig, SendQueryOptions};

    fn defaults() -> ClaudeProviderDefaults {
        ClaudeProviderDefaults::default()
    }

    fn assert_argv_contains(argv: &[String], flag: &str) {
        assert!(
            argv.contains(&flag.to_owned()),
            "expected flag {:?} in argv: {:?}",
            flag,
            argv
        );
    }

    fn assert_argv_has_pair(argv: &[String], flag: &str, value: &str) {
        let flag_pos = argv.iter().position(|a| a == flag);
        assert!(
            flag_pos.is_some(),
            "expected flag {:?} in argv: {:?}",
            flag,
            argv
        );
        let pos = flag_pos.unwrap();
        assert_eq!(
            argv.get(pos + 1).map(|s| s.as_str()),
            Some(value),
            "expected {:?} after flag {:?}, argv: {:?}",
            value,
            flag,
            argv
        );
    }

    fn assert_argv_not_contains(argv: &[String], flag: &str) {
        assert!(
            !argv.contains(&flag.to_owned()),
            "unexpected flag {:?} in argv: {:?}",
            flag,
            argv
        );
    }

    // ── Transport flags always present ────────────────────────────────────────

    #[test]
    fn transport_flags_always_present() {
        let (argv, _) = build_claude_argv(None, None, &defaults(), None, None, &[], &[], None);
        assert_argv_contains(&argv, "--print");
        assert_argv_has_pair(&argv, "--output-format", "stream-json");
        assert_argv_contains(&argv, "--verbose");
        assert_argv_has_pair(&argv, "--input-format", "text");
    }

    // ── Permission mode always bypassPermissions ──────────────────────────────

    #[test]
    fn permission_mode_always_bypass() {
        let (argv, _) = build_claude_argv(None, None, &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--permission-mode", "bypassPermissions");
        assert_argv_contains(&argv, "--dangerously-skip-permissions");
    }

    // ── Model ─────────────────────────────────────────────────────────────────

    #[test]
    fn model_from_request_options() {
        let opts = SendQueryOptions { model: Some("claude-opus-4".to_owned()), ..Default::default() };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--model", "claude-opus-4");
    }

    #[test]
    fn model_from_defaults_when_not_in_request() {
        let mut d = defaults();
        d.model = Some("claude-haiku-4".to_owned());
        let (argv, _) = build_claude_argv(None, None, &d, None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--model", "claude-haiku-4");
    }

    #[test]
    fn model_absent_when_not_set() {
        let (argv, _) = build_claude_argv(None, None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--model");
    }

    // ── Fallback model ────────────────────────────────────────────────────────

    #[test]
    fn fallback_model_from_request_options() {
        let opts = SendQueryOptions {
            fallback_model: Some("claude-haiku-3".to_owned()),
            ..Default::default()
        };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--fallback-model", "claude-haiku-3");
    }

    #[test]
    fn fallback_model_from_node_config() {
        let nc = NodeConfig {
            fallback_model: Some("haiku-fallback".to_owned()),
            ..Default::default()
        };
        let (argv, _) =
            build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--fallback-model", "haiku-fallback");
    }

    // ── Resume session ────────────────────────────────────────────────────────

    #[test]
    fn resume_session_id_emits_flag() {
        let (argv, _) = build_claude_argv(
            None,
            None,
            &defaults(),
            Some("sess-abc-123"),
            None,
            &[],
            &[],
            None,
        );
        assert_argv_has_pair(&argv, "--resume", "sess-abc-123");
    }

    #[test]
    fn no_resume_when_session_id_absent() {
        let (argv, _) = build_claude_argv(None, None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--resume");
    }

    // ── Fork session ──────────────────────────────────────────────────────────

    #[test]
    fn fork_session_true_emits_flag() {
        let opts = SendQueryOptions { fork_session: Some(true), ..Default::default() };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_contains(&argv, "--fork-session");
    }

    #[test]
    fn fork_session_false_no_flag() {
        let opts = SendQueryOptions { fork_session: Some(false), ..Default::default() };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--fork-session");
    }

    // ── Setting sources ───────────────────────────────────────────────────────

    #[test]
    fn default_setting_sources_project_user() {
        let (argv, _) = build_claude_argv(None, None, &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--setting-sources", "project,user");
    }

    #[test]
    fn custom_setting_sources_from_defaults() {
        use har_contract::SettingSource;
        let mut d = defaults();
        d.setting_sources = Some(vec![SettingSource::User]);
        let (argv, _) = build_claude_argv(None, None, &d, None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--setting-sources", "user");
    }

    // ── Effort ────────────────────────────────────────────────────────────────

    #[test]
    fn effort_from_node_config() {
        let nc = NodeConfig { effort: Some("high".to_owned()), ..Default::default() };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--effort", "high");
    }

    // ── Betas ─────────────────────────────────────────────────────────────────

    #[test]
    fn betas_from_node_config() {
        let nc = NodeConfig {
            betas: Some(vec!["beta1".to_owned(), "beta2".to_owned()]),
            ..Default::default()
        };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--betas", "beta1,beta2");
    }

    #[test]
    fn empty_betas_no_flag() {
        let nc = NodeConfig { betas: Some(vec![]), ..Default::default() };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--betas");
    }

    // ── Allowed / denied tools ────────────────────────────────────────────────
    //
    // nodeConfig.allowed_tools → options.tools (agent roster), NOT --allowed-tools.
    // --allowed-tools is built from MCP wildcards + Skill + sidecar ONLY.
    // Source: provider.ts:282-284 (options.tools = nodeConfig.allowed_tools),
    //         provider.ts:324 (MCP wildcards → options.allowedTools),
    //         provider.ts:367 (Skill → options.allowedTools).

    #[test]
    fn allowed_tools_from_node_config_goes_to_agent_roster_not_flag() {
        // nodeConfig.allowed_tools → options.tools (agent roster).
        // No skills, no MCP → NO --allowed-tools flag emitted.
        let nc = NodeConfig {
            allowed_tools: Some(vec!["Bash".to_owned(), "Edit".to_owned()]),
            ..Default::default()
        };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        // --allowed-tools must NOT appear: agent_roster_tools has no CLI flag without skills/MCP
        assert_argv_not_contains(&argv, "--allowed-tools");
    }

    #[test]
    fn allowed_tools_with_skills_goes_to_agent_def_tools_and_skill_in_flag() {
        // nodeConfig.allowed_tools → agentDef.tools = [..., Skill]; --allowed-tools = ["Skill"]
        let nc = NodeConfig {
            allowed_tools: Some(vec!["Bash".to_owned()]),
            skills: Some(vec!["skill-a".to_owned()]),
            ..Default::default()
        };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        // --allowed-tools contains only "Skill" (not "Bash")
        let tools_pos = argv.iter().position(|a| a == "--allowed-tools");
        assert!(tools_pos.is_some(), "expected --allowed-tools when skills present");
        let tools_val = &argv[tools_pos.unwrap() + 1];
        assert_eq!(tools_val, "Skill", "--allowed-tools must be exactly 'Skill', got: {}", tools_val);
        // --agents JSON must contain agentDef.tools = ["Bash", "Skill"]
        let agents_pos = argv.iter().position(|a| a == "--agents").unwrap();
        let agents_val: serde_json::Value = serde_json::from_str(&argv[agents_pos + 1]).unwrap();
        let tools = agents_val["dag-node-skills"]["tools"].as_array().unwrap();
        let tool_strs: Vec<&str> = tools.iter().filter_map(|v| v.as_str()).collect();
        assert!(tool_strs.contains(&"Bash"), "agentDef.tools must contain Bash");
        assert!(tool_strs.contains(&"Skill"), "agentDef.tools must contain Skill");
    }

    #[test]
    fn denied_tools_from_node_config() {
        let nc = NodeConfig {
            denied_tools: Some(vec!["bash".to_owned()]),
            ..Default::default()
        };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--disallowed-tools", "bash");
    }

    // ── MCP config ────────────────────────────────────────────────────────────

    #[test]
    fn mcp_config_adds_flag_and_wildcards() {
        let nc = NodeConfig {
            mcp: Some("/path/to/mcp.json".to_owned()),
            ..Default::default()
        };
        let server_names = vec!["my-server".to_owned(), "other-server".to_owned()];
        let (argv, _) =
            build_claude_argv(None, Some(&nc), &defaults(), None, None, &server_names, &[], None);
        assert_argv_has_pair(&argv, "--mcp-config", "/path/to/mcp.json");
        // Wildcards should be in --allowed-tools
        let tools_pos = argv.iter().position(|a| a == "--allowed-tools").unwrap();
        let tools_val = &argv[tools_pos + 1];
        assert!(tools_val.contains("mcp__my-server__*"), "tools: {}", tools_val);
        assert!(tools_val.contains("mcp__other-server__*"), "tools: {}", tools_val);
    }

    #[test]
    fn mcp_haiku_warning_when_haiku_model() {
        let nc = NodeConfig {
            mcp: Some("/mcp.json".to_owned()),
            ..Default::default()
        };
        let opts = SendQueryOptions {
            model: Some("claude-haiku-3-5".to_owned()),
            ..Default::default()
        };
        let (_, warnings) = build_claude_argv(
            Some(&opts),
            Some(&nc),
            &defaults(),
            None,
            None,
            &["srv".to_owned()],
            &[],
            None,
        );
        assert!(warnings.iter().any(|w| w.code == "mcp_haiku_tool_search"));
    }

    #[test]
    fn mcp_missing_vars_warning() {
        let nc = NodeConfig {
            mcp: Some("/mcp.json".to_owned()),
            ..Default::default()
        };
        let missing = vec!["SECRET_KEY".to_owned(), "API_TOKEN".to_owned(), "SECRET_KEY".to_owned()]; // dup
        let (_, warnings) = build_claude_argv(
            None,
            Some(&nc),
            &defaults(),
            None,
            None,
            &["s".to_owned()],
            &missing,
            None,
        );
        let w = warnings.iter().find(|w| w.code == "mcp_env_vars_missing").unwrap();
        // Deduped
        assert!(w.message.contains("SECRET_KEY"));
        assert!(w.message.contains("API_TOKEN"));
        // Dup removed
        let count_secret = w.message.matches("SECRET_KEY").count();
        assert_eq!(count_secret, 1, "SECRET_KEY should appear exactly once after dedup");
    }

    // ── Skills → agents ───────────────────────────────────────────────────────

    #[test]
    fn skills_produce_agents_and_agent_flags() {
        let nc = NodeConfig {
            skills: Some(vec!["skill-a".to_owned(), "skill-b".to_owned()]),
            ..Default::default()
        };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        assert_argv_contains(&argv, "--agents");
        assert_argv_contains(&argv, "--agent");
        // Skill tool added to allowed-tools
        let tools_pos = argv.iter().position(|a| a == "--allowed-tools");
        assert!(tools_pos.is_some(), "expected --allowed-tools for skills");
        let tools_val = &argv[tools_pos.unwrap() + 1];
        assert!(tools_val.contains("Skill"), "expected Skill in allowed-tools: {}", tools_val);
    }

    #[test]
    fn inline_agents_win_over_skills_on_id_collision() {
        // If user defines 'dag-node-skills' in agents, it overrides the internal wrapper.
        let nc = NodeConfig {
            skills: Some(vec!["skill-a".to_owned()]),
            agents: Some({
                let mut m = std::collections::HashMap::new();
                m.insert(
                    "dag-node-skills".to_owned(),
                    har_contract::InlineAgentDefinition {
                        description: "override".to_owned(),
                        prompt: "custom".to_owned(),
                        model: None,
                        tools: None,
                        disallowed_tools: None,
                        skills: None,
                        max_turns: None,
                    },
                );
                m
            }),
            ..Default::default()
        };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        // Both produce --agents, and the output JSON should have dag-node-skills.
        let agents_pos = argv.iter().position(|a| a == "--agents").unwrap();
        let agents_json = &argv[agents_pos + 1];
        let agents_val: serde_json::Value = serde_json::from_str(agents_json).unwrap();
        // User's override wins — description should be "override"
        assert_eq!(agents_val["dag-node-skills"]["description"], "override");
    }

    // ── Output format schema ──────────────────────────────────────────────────

    #[test]
    fn output_format_schema_from_node_config() {
        let schema = {
            let mut m = std::collections::HashMap::new();
            m.insert("type".to_owned(), serde_json::json!("object"));
            m
        };
        let nc = NodeConfig { output_format: Some(schema), ..Default::default() };
        let (argv, _) = build_claude_argv(None, Some(&nc), &defaults(), None, None, &[], &[], None);
        assert_argv_contains(&argv, "--output-format-schema");
        let pos = argv.iter().position(|a| a == "--output-format-schema").unwrap();
        let schema_str = &argv[pos + 1];
        let parsed: serde_json::Value = serde_json::from_str(schema_str).unwrap();
        assert_eq!(parsed["type"], "object");
    }

    // ── Max budget ────────────────────────────────────────────────────────────

    #[test]
    fn max_budget_from_request_options() {
        let opts = SendQueryOptions { max_budget_usd: Some(5.0), ..Default::default() };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_has_pair(&argv, "--max-budget-usd", "5");
    }

    // ── No-env-file for JS paths ──────────────────────────────────────────────

    #[test]
    fn no_env_file_prepended_for_js_cli_path() {
        let (argv, _) = build_claude_argv(
            None,
            None,
            &defaults(),
            None,
            Some("/path/to/cli.js"),
            &[],
            &[],
            None,
        );
        // --no-env-file must appear before the transport flags
        let no_env_pos = argv.iter().position(|a| a == "--no-env-file");
        let print_pos = argv.iter().position(|a| a == "--print");
        assert!(no_env_pos.is_some(), "expected --no-env-file in argv");
        assert!(
            no_env_pos.unwrap() < print_pos.unwrap(),
            "--no-env-file must come before --print"
        );
    }

    #[test]
    fn no_env_file_absent_for_native_binary() {
        let (argv, _) = build_claude_argv(
            None,
            None,
            &defaults(),
            None,
            Some("/usr/local/bin/claude"),
            &[],
            &[],
            None,
        );
        assert_argv_not_contains(&argv, "--no-env-file");
    }

    // ── allowedTools order: MCP wildcards before Skill (cycle-14 fix) ───────────
    //
    // Source: applyNodeConfig MCP block (provider.ts:324) runs BEFORE skills block
    // (provider.ts:367). Resulting order: [...mcpWildcards, 'Skill'].

    #[test]
    fn mcp_wildcards_before_skill_in_allowed_tools() {
        let nc = NodeConfig {
            mcp: Some("/mcp.json".to_owned()),
            skills: Some(vec!["skill-a".to_owned()]),
            ..Default::default()
        };
        let server_names = vec!["my-server".to_owned()];
        let (argv, _) =
            build_claude_argv(None, Some(&nc), &defaults(), None, None, &server_names, &[], None);
        let tools_pos = argv.iter().position(|a| a == "--allowed-tools").unwrap();
        let tools_val = &argv[tools_pos + 1];
        // Both must be present
        assert!(tools_val.contains("mcp__my-server__*"), "missing mcp wildcard: {}", tools_val);
        assert!(tools_val.contains("Skill"), "missing Skill: {}", tools_val);
        // MCP wildcard must appear BEFORE Skill (source order)
        let mcp_pos = tools_val.find("mcp__my-server__*").unwrap();
        let skill_pos = tools_val.find("Skill").unwrap();
        assert!(
            mcp_pos < skill_pos,
            "MCP wildcard must appear before Skill in --allowed-tools, got: {}",
            tools_val
        );
    }

    // ── persistSession → --no-session-persistence ─────────────────────────────
    //
    // Source: provider.ts:527-529; types.ts:253.
    // SDK: persistSession:false → --no-session-persistence.
    // CLI confirmed: claude --help 2.1.177.
    // Only the non-default value (false) emits the flag.

    #[test]
    fn persist_session_false_emits_no_session_persistence() {
        let opts = SendQueryOptions { persist_session: Some(false), ..Default::default() };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_contains(&argv, "--no-session-persistence");
    }

    #[test]
    fn persist_session_true_does_not_emit_flag() {
        let opts = SendQueryOptions { persist_session: Some(true), ..Default::default() };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--no-session-persistence");
    }

    #[test]
    fn persist_session_absent_does_not_emit_flag() {
        // None means caller didn't set it — use CLI default (sessions persisted).
        let opts = SendQueryOptions { persist_session: None, ..Default::default() };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--no-session-persistence");
    }

    // ── excludeDynamicSections → --exclude-dynamic-system-prompt-sections ─────
    //
    // Source: types.ts:233 (SystemPromptPreset.excludeDynamicSections).
    // CLI confirmed: claude --help 2.1.177.
    // Only when the system prompt is a Preset AND excludeDynamicSections is explicitly true.

    #[test]
    fn exclude_dynamic_sections_true_emits_flag() {
        use har_contract::{SystemPromptInput, SystemPromptPreset, SystemPromptPresetName,
            SystemPromptPresetType};
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Preset(SystemPromptPreset {
                kind: SystemPromptPresetType::Preset,
                preset: SystemPromptPresetName::ClaudeCode,
                append: None,
                exclude_dynamic_sections: Some(true),
            })),
            ..Default::default()
        };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_contains(&argv, "--exclude-dynamic-system-prompt-sections");
    }

    #[test]
    fn exclude_dynamic_sections_false_does_not_emit_flag() {
        use har_contract::{SystemPromptInput, SystemPromptPreset, SystemPromptPresetName,
            SystemPromptPresetType};
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Preset(SystemPromptPreset {
                kind: SystemPromptPresetType::Preset,
                preset: SystemPromptPresetName::ClaudeCode,
                append: None,
                exclude_dynamic_sections: Some(false),
            })),
            ..Default::default()
        };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--exclude-dynamic-system-prompt-sections");
    }

    #[test]
    fn exclude_dynamic_sections_absent_does_not_emit_flag() {
        use har_contract::{SystemPromptInput, SystemPromptPreset, SystemPromptPresetName,
            SystemPromptPresetType};
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Preset(SystemPromptPreset {
                kind: SystemPromptPresetType::Preset,
                preset: SystemPromptPresetName::ClaudeCode,
                append: None,
                exclude_dynamic_sections: None,
            })),
            ..Default::default()
        };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--exclude-dynamic-system-prompt-sections");
    }

    #[test]
    fn exclude_dynamic_sections_on_string_prompt_does_not_emit_flag() {
        // excludeDynamicSections only applies to Preset variants; string prompts are unaffected.
        let opts = SendQueryOptions {
            system_prompt: Some(SystemPromptInput::Single("custom system".to_owned())),
            ..Default::default()
        };
        let (argv, _) =
            build_claude_argv(Some(&opts), None, &defaults(), None, None, &[], &[], None);
        assert_argv_not_contains(&argv, "--exclude-dynamic-system-prompt-sections");
    }

    // ── R8 sidecar seam documented ────────────────────────────────────────────

    #[test]
    fn r8_sidecar_seam_adds_mcp_config_and_wildcard() {
        let (argv, _) = build_claude_argv(
            None,
            None,
            &defaults(),
            None,
            None,
            &[],
            &[],
            Some("/tmp/archon-sidecar-mcp.json"),
        );
        assert_argv_has_pair(&argv, "--mcp-config", "/tmp/archon-sidecar-mcp.json");
        let tools_pos = argv.iter().position(|a| a == "--allowed-tools").unwrap();
        let tools_val = &argv[tools_pos + 1];
        assert!(tools_val.contains("mcp__archon__*"), "tools: {}", tools_val);
    }
}
