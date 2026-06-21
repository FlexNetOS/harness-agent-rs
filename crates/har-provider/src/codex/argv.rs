//! Build the `codex exec --experimental-json [options]` argv for the Codex CLI.
//!
//! PORT of the argv-building logic inside `@openai/codex-sdk`'s `CodexExec.run()`.
//!
//! The SDK spawns: `codex exec --experimental-json [--config K=V...] [--model M] [--sandbox S]
//!   [--cd DIR] [--add-dir DIR...] [--skip-git-repo-check] [--output-schema PATH]
//!   [--config model_reasoning_effort="..."] [--config sandbox_workspace_write.network_access=...]
//!   [--config web_search="..."] [--config approval_policy="..."] [resume THREAD_ID]`
//!
//! Then writes the prompt to stdin.
//!
//! MCP config overrides are serialized via TOML flattening:
//!   `{ mcp_servers: { figma: { url: "..." } } }` → `--config mcp_servers.figma.url="..."`
//!
//! Source: `@openai/codex-sdk/dist/index.js` — `CodexExec.run()`, `serializeConfigOverrides()`,
//!   `flattenConfigOverrides()`, `toTomlValue()`
//! Source for thread options: `packages/providers/src/codex/provider.ts:78-95`
//! Source for hardcoded thread options: `buildThreadOptions` — sandboxMode, networkAccessEnabled,
//!   approvalPolicy, skipGitRepoCheck are FIXED values.

use har_contract::{CodexProviderDefaults, ModelReasoningEffortCodex, WebSearchModeCodex};
use serde_json::Value;

/// Hardcoded thread options from `buildThreadOptions` (provider.ts:84-94).
const SANDBOX_MODE: &str = "danger-full-access";
const APPROVAL_POLICY: &str = "never";
const NETWORK_ACCESS_ENABLED: bool = true;
const SKIP_GIT_REPO_CHECK: bool = true;

/// Build the `codex exec --experimental-json ...` argv.
///
/// Port of `CodexExec.run()` (sdk index.js) + `buildThreadOptions` (provider.ts:78-95).
///
/// Returns `(argv, config_flags_for_serialization)`.
/// `argv[0]` is empty (will be replaced by the binary path at spawn time).
///
/// Parameters:
/// - `config_overrides`: MCP config overrides as a JSON value (from `buildCodexMcpConfigOverrides`).
///   Serialized as `--config dotted.path=toml_value` flags.
/// - `model`: per-request model override
/// - `defaults`: provider defaults from assistantConfig
/// - `resume_session_id`: if Some, appends `resume <id>` at end
/// - `output_schema_path`: if Some, appends `--output-schema <path>`
pub fn build_codex_argv(
    model: Option<&str>,
    defaults: &CodexProviderDefaults,
    resume_session_id: Option<&str>,
    output_schema_path: Option<&str>,
    cwd: &str,
    config_overrides: Option<&Value>,
) -> Vec<String> {
    let mut argv: Vec<String> = Vec::new();

    // Sub-command: `exec --experimental-json`
    argv.push("exec".to_owned());
    argv.push("--experimental-json".to_owned());

    // MCP config overrides: `--config mcp_servers.<name>.<key>=<toml_value>`
    // Source: `serializeConfigOverrides` → `flattenConfigOverrides` in sdk index.js
    if let Some(overrides) = config_overrides {
        let mut flat: Vec<String> = Vec::new();
        if let Value::Object(obj) = overrides {
            flatten_config_overrides(obj, "", &mut flat);
        }
        for kv in flat {
            argv.push("--config".to_owned());
            argv.push(kv);
        }
    }

    // `--model <model>` — per-request model or config model
    // Source: provider.ts:83 — `model: model ?? config.model`
    let effective_model = model.or(defaults.model.as_deref());
    if let Some(m) = effective_model {
        argv.push("--model".to_owned());
        argv.push(m.to_owned());
    }

    // `--sandbox danger-full-access` (hardcoded in buildThreadOptions)
    argv.push("--sandbox".to_owned());
    argv.push(SANDBOX_MODE.to_owned());

    // `--cd <workingDirectory>`
    argv.push("--cd".to_owned());
    argv.push(cwd.to_owned());

    // `--add-dir <dir>` for each additional directory
    if let Some(dirs) = &defaults.additional_directories {
        for dir in dirs {
            argv.push("--add-dir".to_owned());
            argv.push(dir.clone());
        }
    }

    // `--skip-git-repo-check` (hardcoded in buildThreadOptions)
    if SKIP_GIT_REPO_CHECK {
        argv.push("--skip-git-repo-check".to_owned());
    }

    // `--output-schema <path>` when structured output is requested
    if let Some(schema_path) = output_schema_path {
        argv.push("--output-schema".to_owned());
        argv.push(schema_path.to_owned());
    }

    // `--config model_reasoning_effort="<effort>"`
    if let Some(effort) = &defaults.model_reasoning_effort {
        let effort_str = match effort {
            ModelReasoningEffortCodex::Minimal => "minimal",
            ModelReasoningEffortCodex::Low => "low",
            ModelReasoningEffortCodex::Medium => "medium",
            ModelReasoningEffortCodex::High => "high",
            ModelReasoningEffortCodex::Xhigh => "xhigh",
        };
        argv.push("--config".to_owned());
        argv.push(format!("model_reasoning_effort=\"{}\"", effort_str));
    }

    // `--config sandbox_workspace_write.network_access=true` (hardcoded in buildThreadOptions)
    // Source: sdk: `if (args.networkAccessEnabled !== void 0) { commandArgs.push ... }`
    // provider.ts:88: `networkAccessEnabled: true` (always set)
    argv.push("--config".to_owned());
    argv.push(format!(
        "sandbox_workspace_write.network_access={}",
        NETWORK_ACCESS_ENABLED
    ));

    // `--config web_search="<mode>"`
    if let Some(mode) = &defaults.web_search_mode {
        let mode_str = match mode {
            WebSearchModeCodex::Disabled => "disabled",
            WebSearchModeCodex::Cached => "cached",
            WebSearchModeCodex::Live => "live",
        };
        argv.push("--config".to_owned());
        argv.push(format!("web_search=\"{}\"", mode_str));
    }

    // `--config approval_policy="never"` (hardcoded in buildThreadOptions)
    argv.push("--config".to_owned());
    argv.push(format!("approval_policy=\"{}\"", APPROVAL_POLICY));

    // `resume <thread_id>` — placed after all flags
    if let Some(thread_id) = resume_session_id {
        argv.push("resume".to_owned());
        argv.push(thread_id.to_owned());
    }

    argv
}

// ─── TOML config flattening ───────────────────────────────────────────────────
//
// Port of `serializeConfigOverrides` / `flattenConfigOverrides` / `toTomlValue`
// from the @openai/codex-sdk (index.js).

/// Convert a JSON `Value` to a TOML-compatible string representation.
///
/// Port of `toTomlValue(value, path)` from sdk index.js.
///
/// String values use `JSON.stringify`-compatible escaping (the SDK calls
/// `JSON.stringify(value)` for string nodes at dist/index.js:330-331), which
/// escapes `\n`, `\t`, `\r`, and all other C0 control chars as `\uXXXX`.
/// Using `serde_json::to_string` produces the exact same escaping behaviour.
pub fn to_toml_value(value: &Value, path: &str) -> Result<String, String> {
    match value {
        Value::String(s) => {
            // serde_json::to_string produces JSON.stringify-compatible output:
            // control chars are escaped (\n → \\n, \t → \\t, etc.), which
            // matches the SDK's toTomlValue string branch exactly.
            serde_json::to_string(s)
                .map_err(|e| format!("string serialization failed at {}: {}", path, e))
        }
        Value::Number(n) => {
            let f = n
                .as_f64()
                .ok_or_else(|| format!("non-finite number at {}", path))?;
            if !f.is_finite() {
                return Err(format!(
                    "Codex config override at {} must be a finite number",
                    path
                ));
            }
            Ok(n.to_string())
        }
        Value::Bool(b) => Ok(if *b { "true" } else { "false" }.to_owned()),
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                parts.push(to_toml_value(item, &format!("{}[{}]", path, i))?);
            }
            Ok(format!("[{}]", parts.join(", ")))
        }
        Value::Object(obj) => {
            let mut parts = Vec::new();
            for (key, child) in obj {
                let formatted_key = format_toml_key(key);
                let child_val = to_toml_value(child, &format!("{}.{}", path, key))?;
                parts.push(format!("{} = {}", formatted_key, child_val));
            }
            Ok(format!("{{{}}}", parts.join(", ")))
        }
        Value::Null => Err(format!("Codex config override at {} cannot be null", path)),
    }
}

/// Format a TOML key: bare if alphanumeric/underscore/dash, quoted otherwise.
///
/// Port of `formatTomlKey(key)` from sdk index.js.
fn format_toml_key(key: &str) -> String {
    let is_bare = key
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-');
    if is_bare {
        key.to_owned()
    } else {
        format!("\"{}\"", key.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// Recursively flatten a JSON object into `key=toml_value` config override pairs.
///
/// Port of `flattenConfigOverrides(value, prefix, overrides)` from sdk index.js.
pub fn flatten_config_overrides(
    obj: &serde_json::Map<String, Value>,
    prefix: &str,
    overrides: &mut Vec<String>,
) {
    for (key, child) in obj {
        if key.is_empty() {
            tracing::warn!(path = %prefix, "codex.config_override_empty_key_skipped");
            continue;
        }
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{}.{}", prefix, key)
        };

        match child {
            Value::Object(nested) => {
                if nested.is_empty() {
                    overrides.push(format!("{}={{}}", path));
                } else {
                    flatten_config_overrides(nested, &path, overrides);
                }
            }
            Value::Null => {
                // Skip null values (undefined in JS maps to undefined-skip in flattenConfigOverrides)
                continue;
            }
            _ => match to_toml_value(child, &path) {
                Ok(toml_val) => overrides.push(format!("{}={}", path, toml_val)),
                Err(e) => {
                    tracing::warn!(err = %e, path = %path, "codex.config_override_toml_error_skipped");
                }
            },
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::{ModelReasoningEffortCodex, WebSearchModeCodex};
    use serde_json::json;

    fn default_codex_config() -> CodexProviderDefaults {
        CodexProviderDefaults::default()
    }

    // ── basic argv structure ─────────────────────────────────────────────────

    #[test]
    fn argv_starts_with_exec_experimental_json() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        assert_eq!(argv[0], "exec");
        assert_eq!(argv[1], "--experimental-json");
    }

    #[test]
    fn argv_includes_hardcoded_sandbox() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        let idx = argv.iter().position(|a| a == "--sandbox").unwrap();
        assert_eq!(argv[idx + 1], "danger-full-access");
    }

    #[test]
    fn argv_includes_working_directory() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/my/workspace",
            None,
        );
        let idx = argv.iter().position(|a| a == "--cd").unwrap();
        assert_eq!(argv[idx + 1], "/my/workspace");
    }

    #[test]
    fn argv_includes_skip_git_repo_check() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        assert!(argv.contains(&"--skip-git-repo-check".to_owned()));
    }

    #[test]
    fn argv_includes_network_access_config() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        let idx = argv
            .iter()
            .position(|a| a == "sandbox_workspace_write.network_access=true")
            .unwrap();
        assert_eq!(argv[idx - 1], "--config");
    }

    #[test]
    fn argv_includes_approval_policy() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        assert!(argv.contains(&"approval_policy=\"never\"".to_owned()));
    }

    // ── model ───────────────────────────────────────────────────────────────

    #[test]
    fn argv_model_from_request_options() {
        let argv = build_codex_argv(
            Some("gpt-5.2-codex"),
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        let idx = argv.iter().position(|a| a == "--model").unwrap();
        assert_eq!(argv[idx + 1], "gpt-5.2-codex");
    }

    #[test]
    fn argv_model_from_defaults() {
        let mut defaults = default_codex_config();
        defaults.model = Some("gpt-5.2-codex".to_owned());
        let argv = build_codex_argv(None, &defaults, None, None, "/workspace", None);
        let idx = argv.iter().position(|a| a == "--model").unwrap();
        assert_eq!(argv[idx + 1], "gpt-5.2-codex");
    }

    #[test]
    fn argv_request_model_overrides_config_model() {
        let mut defaults = default_codex_config();
        defaults.model = Some("config-model".to_owned());
        let argv = build_codex_argv(
            Some("request-model"),
            &defaults,
            None,
            None,
            "/workspace",
            None,
        );
        let idx = argv.iter().position(|a| a == "--model").unwrap();
        assert_eq!(argv[idx + 1], "request-model");
    }

    #[test]
    fn argv_no_model_flag_when_absent() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        assert!(!argv.contains(&"--model".to_owned()));
    }

    // ── resume session ───────────────────────────────────────────────────────

    #[test]
    fn argv_resume_appended_at_end() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            Some("thread-abc"),
            None,
            "/workspace",
            None,
        );
        let idx = argv.iter().position(|a| a == "resume").unwrap();
        assert_eq!(argv[idx + 1], "thread-abc");
        // resume must be the last two elements
        assert_eq!(argv[argv.len() - 2], "resume");
    }

    #[test]
    fn argv_no_resume_when_none() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        assert!(!argv.contains(&"resume".to_owned()));
    }

    // ── output schema ────────────────────────────────────────────────────────

    #[test]
    fn argv_output_schema_included_when_provided() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            Some("/tmp/schema.json"),
            "/workspace",
            None,
        );
        let idx = argv.iter().position(|a| a == "--output-schema").unwrap();
        assert_eq!(argv[idx + 1], "/tmp/schema.json");
    }

    #[test]
    fn argv_no_output_schema_when_none() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        assert!(!argv.contains(&"--output-schema".to_owned()));
    }

    // ── modelReasoningEffort ─────────────────────────────────────────────────

    #[test]
    fn argv_model_reasoning_effort_medium() {
        let mut defaults = default_codex_config();
        defaults.model_reasoning_effort = Some(ModelReasoningEffortCodex::Medium);
        let argv = build_codex_argv(None, &defaults, None, None, "/workspace", None);
        assert!(argv.contains(&"model_reasoning_effort=\"medium\"".to_owned()));
    }

    #[test]
    fn argv_no_reasoning_effort_when_absent() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        let has_effort = argv.iter().any(|a| a.contains("model_reasoning_effort"));
        assert!(!has_effort);
    }

    // ── webSearchMode ────────────────────────────────────────────────────────

    #[test]
    fn argv_web_search_live() {
        let mut defaults = default_codex_config();
        defaults.web_search_mode = Some(WebSearchModeCodex::Live);
        let argv = build_codex_argv(None, &defaults, None, None, "/workspace", None);
        assert!(argv.contains(&"web_search=\"live\"".to_owned()));
    }

    #[test]
    fn argv_no_web_search_when_absent() {
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            None,
        );
        let has_web_search = argv
            .iter()
            .any(|a| a.starts_with("web_search=") && !a.contains("network_access"));
        assert!(!has_web_search);
    }

    // ── additionalDirectories ────────────────────────────────────────────────

    #[test]
    fn argv_add_dir_for_each_additional_directory() {
        let mut defaults = default_codex_config();
        defaults.additional_directories = Some(vec!["/foo".to_owned(), "/bar".to_owned()]);
        let argv = build_codex_argv(None, &defaults, None, None, "/workspace", None);
        let add_dirs: Vec<_> = argv
            .windows(2)
            .filter(|w| w[0] == "--add-dir")
            .map(|w| w[1].as_str())
            .collect();
        assert_eq!(add_dirs, vec!["/foo", "/bar"]);
    }

    // ── MCP config overrides ─────────────────────────────────────────────────

    #[test]
    fn argv_mcp_config_overrides_flattened_as_config_flags() {
        let overrides = json!({
            "mcp_servers": {
                "figma": {
                    "url": "http://127.0.0.1:3845/mcp"
                }
            }
        });
        let argv = build_codex_argv(
            None,
            &default_codex_config(),
            None,
            None,
            "/workspace",
            Some(&overrides),
        );
        // Should contain --config mcp_servers.figma.url="http://..."
        let has_mcp_url = argv
            .windows(2)
            .any(|w| w[0] == "--config" && w[1].starts_with("mcp_servers.figma.url="));
        assert!(has_mcp_url, "argv={:?}", argv);
    }

    // ── TOML value serialization ─────────────────────────────────────────────

    #[test]
    fn to_toml_value_string() {
        let result = to_toml_value(&Value::String("hello".to_owned()), "key");
        assert_eq!(result.unwrap(), "\"hello\"");
    }

    #[test]
    fn to_toml_value_bool_true() {
        let result = to_toml_value(&Value::Bool(true), "key");
        assert_eq!(result.unwrap(), "true");
    }

    #[test]
    fn to_toml_value_bool_false() {
        let result = to_toml_value(&Value::Bool(false), "key");
        assert_eq!(result.unwrap(), "false");
    }

    #[test]
    fn to_toml_value_number() {
        let result = to_toml_value(&json!(42), "key");
        assert_eq!(result.unwrap(), "42");
    }

    #[test]
    fn to_toml_value_array() {
        let result = to_toml_value(&json!(["a", "b"]), "key");
        assert_eq!(result.unwrap(), "[\"a\", \"b\"]");
    }

    #[test]
    fn to_toml_value_null_errors() {
        let result = to_toml_value(&Value::Null, "key");
        assert!(result.is_err());
    }

    #[test]
    fn flatten_config_overrides_nested() {
        let obj = json!({
            "mcp_servers": {
                "figma": {
                    "url": "http://test.example"
                }
            }
        });
        let mut overrides = Vec::new();
        if let Value::Object(map) = &obj {
            flatten_config_overrides(map, "", &mut overrides);
        }
        assert!(
            overrides
                .iter()
                .any(|s| s == "mcp_servers.figma.url=\"http://test.example\""),
            "overrides={:?}",
            overrides
        );
    }
}
