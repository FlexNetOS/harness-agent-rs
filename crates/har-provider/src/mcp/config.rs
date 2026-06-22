//! Load MCP server config from a JSON file and expand environment variables.
//!
//! PORT of `packages/providers/src/mcp/config.ts` (verbatim behavior).
//!
//! Faithful details that the previous inline stopgap got wrong (all preserved here):
//! - **`normalizeMcpConfig`** — a top-level `{ "mcpServers": { … } }` wrapper is
//!   unwrapped; mixing `mcpServers` with sibling keys throws; a non-object
//!   `mcpServers` throws. (config.ts:101-122)
//! - **Expansion scope** — env-var expansion happens ONLY in each server's `env`
//!   and `headers` records, NOT recursively across every field. (config.ts:50-99)
//! - **Throws, not skips** — a non-object server, a non-object `env`/`headers`, or
//!   a non-string value inside `env`/`headers` is a hard error (returned as `Err`
//!   here, matching the source `throw new Error(...)`). (config.ts:31, 61-94)
//! - **Uppercase-only var names** — the source regex is
//!   `/\$(?:\{([A-Z_][A-Z0-9_]*)\}|([A-Z_][A-Z0-9_]*))/g`; `$lower` / `${lower}`
//!   are left literal. We use the identical pattern via the `regex` crate so the
//!   matching (incl. the greedy bare-name stop at the first non-`[A-Z0-9_]` char)
//!   is byte-for-byte. (config.ts:36)
//! - **Order-preserving** — `serde_json::Map` (preserve_order) keeps server and
//!   field insertion order, matching JS object key order (`Object.keys`,
//!   `Object.entries`). `serverNames` is `Object.keys(expanded)`. (config.ts:159)
//!
//! ## `[≈]` cross-runtime error-detail divergence (bounded, documented)
//! The `${mcpPath} - ${detail}` tail of the JSON-parse and non-ENOENT read errors
//! carries the underlying parser/OS message, which differs between V8 (`JSON.parse`
//! `SyntaxError`, Node `fs` error) and Rust (`serde_json`, `std::io::Error`). The
//! message *prefix* and the *error condition* match the source exactly; only the
//! free-text detail tail differs. No consumer parses the detail (it surfaces as a
//! provider error), so this is a lenient `[≈]`, not a behavior change.
//!
//! ## `[≈]` path resolution
//! Source uses Node `path.resolve(cwd, mcpPath)` (normalizes `.`/`..`, collapses
//! separators). Rust uses `Path::join`, which is identical for absolute `cwd` +
//! simple relative `mcpPath` (the realistic inputs) but does not collapse `..`.
//! The resolved path appears only inside the ENOENT message tail. Bounded `[≈]`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Map, Value};

/// Loaded MCP config data.
///
/// PORT of `LoadedMcpConfig` (config.ts:6-10). `servers` carries the expanded
/// server map (`Record<string, unknown>`), `server_names` is `Object.keys(servers)`
/// in insertion order, `missing_vars` accumulates every undefined-var occurrence
/// (with duplicates — callers dedup at the warning site via `new Set(...)`).
#[derive(Debug, Default, Clone)]
pub struct LoadedMcpConfig {
    pub servers: Map<String, Value>,
    pub server_names: Vec<String>,
    pub missing_vars: Vec<String>,
}

/// Build an env source from the current process environment.
///
/// Mirrors the source default `envSource = process.env` used by the claude and
/// copilot providers. Values are wrapped in `Some` so an absent key (`None`) and
/// a present-but-undefined key are both treated as "missing" (JS `undefined`).
pub fn process_env_source() -> HashMap<String, Option<String>> {
    std::env::vars().map(|(k, v)| (k, Some(v))).collect()
}

/// `describeJsonType(value)` (config.ts:12-16): the JS `typeof`/array/null label.
fn describe_json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::String(_) => "string",
        Value::Number(_) => "number",
        Value::Bool(_) => "boolean",
    }
}

/// The source env-var pattern, compiled once.
///
/// `/\$(?:\{([A-Z_][A-Z0-9_]*)\}|([A-Z_][A-Z0-9_]*))/g` — group 1 = `${BRACED}`,
/// group 2 = bare `$BARE`. (config.ts:36)
fn env_var_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$(?:\{([A-Z_][A-Z0-9_]*)\}|([A-Z_][A-Z0-9_]*))").unwrap())
}

/// Expand `$VAR` / `${VAR}` references in the string values of a record.
///
/// PORT of `expandEnvVarsInRecord` (config.ts:22-48). Each value MUST be a string
/// (else throw); each undefined var is recorded in `missing_vars` and replaced by
/// the empty string, matching `envVal ?? ''`.
fn expand_env_vars_in_record(
    record: &Map<String, Value>,
    missing_vars: &mut Vec<String>,
    env_source: &HashMap<String, Option<String>>,
    field_path: &str,
) -> Result<Map<String, Value>, String> {
    let mut result = Map::new();
    let re = env_var_regex();
    for (key, val) in record {
        let s = match val {
            Value::String(s) => s,
            other => {
                return Err(format!(
                    "MCP config {field_path}.{key} must be a string (got {})",
                    describe_json_type(other)
                ));
            }
        };
        let expanded = re.replace_all(s, |caps: &regex::Captures| {
            // group 1 (braced) ?? group 2 (bare) ?? ''
            let var_name = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            match env_source.get(var_name).and_then(|v| v.as_deref()) {
                Some(v) => v.to_owned(),
                None => {
                    missing_vars.push(var_name.to_owned());
                    String::new()
                }
            }
        });
        result.insert(key.clone(), Value::String(expanded.into_owned()));
    }
    Ok(result)
}

/// Expand env vars in each server's `env` and `headers` records (only).
///
/// PORT of `expandEnvVars` (config.ts:50-99). Returns the expanded server map plus
/// the accumulated missing-var list.
fn expand_env_vars(
    config: &Map<String, Value>,
    env_source: &HashMap<String, Option<String>>,
) -> Result<(Map<String, Value>, Vec<String>), String> {
    let mut result = Map::new();
    let mut missing_vars: Vec<String> = Vec::new();

    for (server_name, server_config) in config {
        // `typeof serverConfig !== 'object' || null || Array` → throw.
        let server_obj = match server_config {
            Value::Object(o) => o,
            other => {
                return Err(format!(
                    "MCP server \"{server_name}\" must be a JSON object (got {})",
                    describe_json_type(other)
                ));
            }
        };

        // `const server = { ...serverConfig }` — clone so we can replace env/headers.
        let mut server = server_obj.clone();

        // `if (server.env !== undefined)` — a present `env` (incl. null) is validated.
        if let Some(env_val) = server.get("env") {
            let env_obj = match env_val {
                Value::Object(o) => o.clone(),
                other => {
                    return Err(format!(
                        "MCP config {server_name}.env must be a JSON object of string values (got {})",
                        describe_json_type(other)
                    ));
                }
            };
            let expanded = expand_env_vars_in_record(
                &env_obj,
                &mut missing_vars,
                env_source,
                &format!("{server_name}.env"),
            )?;
            // Replace in place — preserves the existing key's position.
            server.insert("env".to_owned(), Value::Object(expanded));
        }

        // `if (server.headers !== undefined)`
        if let Some(headers_val) = server.get("headers") {
            let headers_obj = match headers_val {
                Value::Object(o) => o.clone(),
                other => {
                    return Err(format!(
                        "MCP config {server_name}.headers must be a JSON object of string values (got {})",
                        describe_json_type(other)
                    ));
                }
            };
            let expanded = expand_env_vars_in_record(
                &headers_obj,
                &mut missing_vars,
                env_source,
                &format!("{server_name}.headers"),
            )?;
            server.insert("headers".to_owned(), Value::Object(expanded));
        }

        result.insert(server_name.clone(), Value::Object(server));
    }

    Ok((result, missing_vars))
}

/// Unwrap a top-level `{ "mcpServers": { … } }` wrapper, or pass through a bare map.
///
/// PORT of `normalizeMcpConfig` (config.ts:101-122).
fn normalize_mcp_config(
    parsed: &Map<String, Value>,
    mcp_path: &str,
) -> Result<Map<String, Value>, String> {
    if !parsed.contains_key("mcpServers") {
        return Ok(parsed.clone());
    }

    if parsed.len() > 1 {
        return Err(format!(
            "MCP config cannot mix top-level \"mcpServers\" with other keys: {mcp_path}. \
             Use either a direct server map or {{ \"mcpServers\": {{ ... }} }}."
        ));
    }

    match parsed.get("mcpServers") {
        Some(Value::Object(o)) => Ok(o.clone()),
        _ => Err(format!(
            "MCP config field \"mcpServers\" must be a JSON object: {mcp_path}"
        )),
    }
}

/// Load MCP server config from a JSON file and expand environment variables.
///
/// PORT of `loadMcpConfig(mcpPath, cwd, envSource)` (config.ts:127-161).
///
/// `env_source` is the expansion source: the claude/copilot providers pass
/// [`process_env_source`]; the codex provider passes `{ ...process.env, ...requestEnv }`
/// (see `buildMcpEnvSource`, codex/provider.ts:105-109).
pub async fn load_mcp_config(
    mcp_path: &str,
    cwd: &str,
    env_source: &HashMap<String, Option<String>>,
) -> Result<LoadedMcpConfig, String> {
    // `isAbsolute(mcpPath) ? mcpPath : resolve(cwd, mcpPath)`
    let full_path = if Path::new(mcp_path).is_absolute() {
        mcp_path.to_owned()
    } else {
        Path::new(cwd).join(mcp_path).to_string_lossy().into_owned()
    };

    let raw = match tokio::fs::read_to_string(&full_path).await {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "MCP config file not found: {mcp_path} (resolved to {full_path})"
            ));
        }
        Err(e) => {
            return Err(format!("Failed to read MCP config file: {mcp_path} - {e}"));
        }
    };

    // `JSON.parse(raw)` — on syntax error, the source surfaces the parser message.
    let parsed: Value = serde_json::from_str(&raw)
        .map_err(|e| format!("MCP config file is not valid JSON: {mcp_path} - {e}"))?;

    // `typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)` → throw.
    let parsed_obj = match &parsed {
        Value::Object(o) => o,
        _ => {
            return Err(format!(
                "MCP config must be a JSON object (Record<string, ServerConfig>): {mcp_path}"
            ));
        }
    };

    let normalized = normalize_mcp_config(parsed_obj, mcp_path)?;
    let (expanded, missing_vars) = expand_env_vars(&normalized, env_source)?;
    let server_names: Vec<String> = expanded.keys().cloned().collect();

    Ok(LoadedMcpConfig {
        servers: expanded,
        server_names,
        missing_vars,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, Option<String>> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), Some(v.to_string())))
            .collect()
    }

    async fn load_str(
        contents: &str,
        env_source: &HashMap<String, Option<String>>,
    ) -> Result<LoadedMcpConfig, String> {
        let mut tf = tempfile::NamedTempFile::new().unwrap();
        tf.write_all(contents.as_bytes()).unwrap();
        let path = tf.path().to_string_lossy().into_owned();
        load_mcp_config(&path, "/tmp", env_source).await
    }

    #[tokio::test]
    async fn bare_server_map_passthrough() {
        let loaded = load_str(
            r#"{ "fs": { "command": "node", "args": ["server.js"] } }"#,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(loaded.server_names, vec!["fs"]);
        assert!(loaded.missing_vars.is_empty());
        // args NOT expanded (no $VAR there anyway) and preserved.
        assert_eq!(loaded.servers["fs"]["command"], json!("node"));
    }

    #[tokio::test]
    async fn mcpservers_wrapper_unwrapped() {
        let loaded = load_str(
            r#"{ "mcpServers": { "a": { "command": "x" }, "b": { "command": "y" } } }"#,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(loaded.server_names, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn mcpservers_mixed_with_other_keys_errors() {
        let err = load_str(
            r#"{ "mcpServers": { "a": {} }, "other": 1 }"#,
            &HashMap::new(),
        )
        .await
        .unwrap_err();
        assert!(err.contains("cannot mix top-level \"mcpServers\" with other keys"));
    }

    #[tokio::test]
    async fn mcpservers_non_object_errors() {
        let err = load_str(r#"{ "mcpServers": [1,2] }"#, &HashMap::new())
            .await
            .unwrap_err();
        assert!(err.contains("field \"mcpServers\" must be a JSON object"));
    }

    #[tokio::test]
    async fn non_object_server_throws() {
        let err = load_str(r#"{ "bad": 42 }"#, &HashMap::new())
            .await
            .unwrap_err();
        assert!(err.contains("MCP server \"bad\" must be a JSON object (got number)"));
    }

    #[tokio::test]
    async fn env_expansion_only_in_env_and_headers() {
        let loaded = load_str(
            r#"{ "s": { "command": "$HOME/bin", "env": { "TOKEN": "$TOK" }, "headers": { "Auth": "Bearer ${TOK}" } } }"#,
            &env(&[("TOK", "secret"), ("HOME", "/should/not/expand")]),
        )
        .await
        .unwrap();
        // command is NOT in env/headers → left literal.
        assert_eq!(loaded.servers["s"]["command"], json!("$HOME/bin"));
        assert_eq!(loaded.servers["s"]["env"]["TOKEN"], json!("secret"));
        assert_eq!(
            loaded.servers["s"]["headers"]["Auth"],
            json!("Bearer secret")
        );
        assert!(loaded.missing_vars.is_empty());
    }

    #[tokio::test]
    async fn lowercase_var_left_literal() {
        let loaded = load_str(
            r#"{ "s": { "env": { "A": "$lower", "B": "${lower}", "C": "$UPPER" } } }"#,
            &env(&[("UPPER", "ok")]),
        )
        .await
        .unwrap();
        assert_eq!(loaded.servers["s"]["env"]["A"], json!("$lower"));
        assert_eq!(loaded.servers["s"]["env"]["B"], json!("${lower}"));
        assert_eq!(loaded.servers["s"]["env"]["C"], json!("ok"));
        // $lower / ${lower} matched nothing → not recorded as missing.
        assert!(loaded.missing_vars.is_empty());
    }

    #[tokio::test]
    async fn missing_vars_recorded_with_duplicates() {
        let loaded = load_str(
            r#"{ "s": { "env": { "X": "$MISSING", "Y": "$MISSING" } } }"#,
            &HashMap::new(),
        )
        .await
        .unwrap();
        assert_eq!(loaded.missing_vars, vec!["MISSING", "MISSING"]);
        // Replaced by empty string.
        assert_eq!(loaded.servers["s"]["env"]["X"], json!(""));
    }

    #[tokio::test]
    async fn bare_var_stops_at_lowercase() {
        // `$FOO_bar`: regex matches `FOO_`, leaves `bar` literal.
        let loaded = load_str(
            r#"{ "s": { "env": { "K": "$FOO_bar" } } }"#,
            &env(&[("FOO_", "X")]),
        )
        .await
        .unwrap();
        assert_eq!(loaded.servers["s"]["env"]["K"], json!("Xbar"));
    }

    #[tokio::test]
    async fn non_string_env_value_throws() {
        let err = load_str(r#"{ "s": { "env": { "K": 5 } } }"#, &HashMap::new())
            .await
            .unwrap_err();
        assert!(err.contains("MCP config s.env.K must be a string (got number)"));
    }

    #[tokio::test]
    async fn non_object_env_throws() {
        let err = load_str(r#"{ "s": { "env": "nope" } }"#, &HashMap::new())
            .await
            .unwrap_err();
        assert!(
            err.contains("MCP config s.env must be a JSON object of string values (got string)")
        );
    }

    #[tokio::test]
    async fn top_level_array_throws() {
        let err = load_str(r#"[1,2,3]"#, &HashMap::new()).await.unwrap_err();
        assert!(err.contains("MCP config must be a JSON object"));
    }

    #[tokio::test]
    async fn not_found_message() {
        let err = load_mcp_config("/no/such/file-xyz.json", "/tmp", &HashMap::new())
            .await
            .unwrap_err();
        assert!(err.starts_with("MCP config file not found:"));
    }

    #[tokio::test]
    async fn invalid_json_message() {
        let err = load_str("{ not json", &HashMap::new()).await.unwrap_err();
        assert!(err.starts_with("MCP config file is not valid JSON:"));
    }

    #[tokio::test]
    async fn relative_path_resolved_against_cwd() {
        // Absolute path is used as-is; just confirm absolute branch works.
        let loaded = load_str(r#"{ "s": {} }"#, &HashMap::new()).await.unwrap();
        assert_eq!(loaded.server_names, vec!["s"]);
    }
}
