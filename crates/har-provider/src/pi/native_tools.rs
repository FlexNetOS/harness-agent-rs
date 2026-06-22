//! Pi native tool definitions.
//!
//! PORT of `packages/providers/src/community/pi/native-tools.ts`.
//!
//! Adapts Archon `NativeTool`s to Pi tool definition descriptors for the
//! `customTools` array. The schema validation and mapping logic is fully ported.
//! At runtime, these descriptors are passed to `run_pi_rpc_session` which
//! injects them into the native-tools bridge extension for round-trip dispatch.

use har_contract::NativeTool;
use serde_json::Value;

/// A validated Pi native tool definition (SDK-seam-free descriptor).
///
/// PORT of the `ToolDefinition` result from `buildPiNativeToolDefinitions`
/// (native-tools.ts:58-76).
///
/// In the live RPC path, each descriptor is serialized into `NATIVE_TOOLS_BRIDGE_NAMES`
/// and the native-tools bridge registers it with Pi via `ctx.registerTool(...)`.
/// The bridge's `execute` proxies calls back to Rust via the
/// `extension_ui_request`/`extension_ui_response` round-trip.
#[derive(Debug, Clone)]
pub struct PiNativeToolDef {
    /// Tool name (Pi `defineTool.name` and `.label`).
    pub name: String,
    /// Human-readable label (derived from name, matching source behavior).
    pub label: String,
    /// Tool description.
    pub description: String,
    /// Validated TypeBox-compatible schema (as serde_json Value for Rust parity).
    pub schema: Value,
}

/// Convert a NativeTool's JSON Schema to the subset Pi's `defineTool` expects.
///
/// Supports flat object schemas with string / string-enum / boolean properties.
/// Throws on unsupported shapes (fail-fast, matching source behavior).
///
/// PORT of `jsonSchemaToTypeBox(schema)` (native-tools.ts:15-52).
fn validate_and_normalize_schema(schema: &Value) -> Result<Value, String> {
    if schema.get("type").and_then(|v| v.as_str()) != Some("object") {
        return Err(
            "native tool inputSchema must be an object schema with `properties`".to_owned(),
        );
    }

    let props = schema
        .get("properties")
        .and_then(|v| v.as_object())
        .ok_or("native tool inputSchema must be an object schema with `properties`")?;

    let required_set: std::collections::HashSet<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let mut normalized_props = serde_json::Map::new();
    for (key, prop) in props {
        if let Some(enum_vals) = prop.get("enum").and_then(|v| v.as_array()) {
            let values: Vec<&str> = enum_vals.iter().filter_map(|v| v.as_str()).collect();
            if values.is_empty() {
                return Err(format!(
                    "native tool schema: enum for '{key}' must be non-empty strings"
                ));
            }
            // Represent as enum schema (TypeBox Union of Literals)
            let literals: Vec<Value> = values
                .iter()
                .map(|&v| Value::String(v.to_owned()))
                .collect();
            let mut field = serde_json::json!({ "type": "string", "enum": literals });
            if let Some(desc) = prop.get("description").and_then(|v| v.as_str()) {
                field["description"] = Value::String(desc.to_owned());
            }
            if !required_set.contains(key) {
                field["optional"] = Value::Bool(true);
            }
            normalized_props.insert(key.clone(), field);
        } else if prop.get("type").and_then(|v| v.as_str()) == Some("string") {
            let mut field = serde_json::json!({ "type": "string" });
            if let Some(desc) = prop.get("description").and_then(|v| v.as_str()) {
                field["description"] = Value::String(desc.to_owned());
            }
            if !required_set.contains(key) {
                field["optional"] = Value::Bool(true);
            }
            normalized_props.insert(key.clone(), field);
        } else if prop.get("type").and_then(|v| v.as_str()) == Some("boolean") {
            let mut field = serde_json::json!({ "type": "boolean" });
            if let Some(desc) = prop.get("description").and_then(|v| v.as_str()) {
                field["description"] = Value::String(desc.to_owned());
            }
            if !required_set.contains(key) {
                field["optional"] = Value::Bool(true);
            }
            normalized_props.insert(key.clone(), field);
        } else {
            return Err(format!(
                "native tool schema: unsupported type for '{key}' (only string / string-enum / boolean)"
            ));
        }
    }

    Ok(Value::Object(normalized_props))
}

/// Adapt NativeTools to Pi `ToolDefinition` descriptors for the `customTools` array.
///
/// PORT of `buildPiNativeToolDefinitions(nativeTools)` (native-tools.ts:58-76).
///
/// The handler's text result becomes the tool's content. In the live RPC path,
/// these descriptors are serialized via `NATIVE_TOOLS_BRIDGE_NAMES` and the
/// bridge calls `execute(_toolCallId, params, ...)` → `ctx.ui.input("native_tool_dispatch", ...)` →
/// Rust dispatch handler in `rpc_client.rs` → `AgentToolResult { content: [{type:'text', text}] }`.
pub fn build_pi_native_tool_definitions(
    native_tools: &[NativeTool],
) -> Result<Vec<PiNativeToolDef>, String> {
    native_tools
        .iter()
        .map(|spec| {
            // NativeTool.input_schema is HashMap<String,Value>; convert to Value::Object
            // so validate_and_normalize_schema can interrogate "type"/"properties"/"required".
            let schema_value: Value = Value::Object(
                spec.input_schema
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<serde_json::Map<_, _>>(),
            );
            let schema = validate_and_normalize_schema(&schema_value)?;
            Ok(PiNativeToolDef {
                name: spec.name.clone(),
                // Pi shows `label` in its UI; derive it per-tool from the name.
                // Source: "derive it per-tool from the name so a future second
                // native tool doesn't inherit a hardcoded 'Manage runs'." (native-tools.ts:61)
                label: spec.name.clone(),
                description: spec.description.clone(),
                schema,
            })
        })
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::NativeTool;
    use serde_json::json;

    fn make_native_tool(name: &str, schema: Value) -> NativeTool {
        // Convert Value::Object to HashMap<String, Value> as NativeTool expects.
        let input_schema: std::collections::HashMap<String, Value> = match schema {
            Value::Object(map) => map.into_iter().collect(),
            _ => panic!("test schema must be a JSON object"),
        };
        NativeTool {
            name: name.to_owned(),
            description: format!("{name} tool"),
            input_schema,
            handler: Some(std::sync::Arc::new(|_params| {
                Box::pin(async { "result".to_owned() })
            })),
        }
    }

    #[test]
    fn builds_definition_for_string_property() {
        let tool = make_native_tool(
            "manage_run",
            json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "The action" }
                },
                "required": ["action"]
            }),
        );
        let defs = build_pi_native_tool_definitions(&[tool]).unwrap();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "manage_run");
        assert_eq!(defs[0].label, "manage_run");
        assert_eq!(defs[0].description, "manage_run tool");
    }

    #[test]
    fn builds_definition_for_boolean_property() {
        let tool = make_native_tool(
            "toggle",
            json!({
                "type": "object",
                "properties": {
                    "enabled": { "type": "boolean" }
                },
                "required": ["enabled"]
            }),
        );
        let defs = build_pi_native_tool_definitions(&[tool]).unwrap();
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn builds_definition_for_enum_property() {
        let tool = make_native_tool(
            "set_mode",
            json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["fast", "slow"] }
                },
                "required": ["mode"]
            }),
        );
        let defs = build_pi_native_tool_definitions(&[tool]).unwrap();
        assert_eq!(defs.len(), 1);
    }

    #[test]
    fn rejects_non_object_schema() {
        let tool = make_native_tool("bad", json!({ "type": "string" }));
        assert!(build_pi_native_tool_definitions(&[tool]).is_err());
    }

    #[test]
    fn rejects_missing_properties() {
        let tool = make_native_tool("bad", json!({ "type": "object" }));
        assert!(build_pi_native_tool_definitions(&[tool]).is_err());
    }

    #[test]
    fn rejects_unsupported_type() {
        let tool = make_native_tool(
            "bad",
            json!({
                "type": "object",
                "properties": {
                    "x": { "type": "number" }
                }
            }),
        );
        assert!(build_pi_native_tool_definitions(&[tool]).is_err());
    }

    #[test]
    fn rejects_empty_enum() {
        let tool = make_native_tool(
            "bad",
            json!({
                "type": "object",
                "properties": {
                    "x": { "type": "string", "enum": [] }
                }
            }),
        );
        assert!(build_pi_native_tool_definitions(&[tool]).is_err());
    }

    #[test]
    fn empty_native_tools_returns_empty() {
        let defs = build_pi_native_tool_definitions(&[]).unwrap();
        assert!(defs.is_empty());
    }

    #[test]
    fn optional_properties_not_in_required() {
        let tool = make_native_tool(
            "my_tool",
            json!({
                "type": "object",
                "properties": {
                    "required_field": { "type": "string" },
                    "optional_field": { "type": "string" }
                },
                "required": ["required_field"]
            }),
        );
        let defs = build_pi_native_tool_definitions(&[tool]).unwrap();
        assert_eq!(defs.len(), 1);
        let schema = &defs[0].schema;
        // optional_field should have optional=true
        let opt = schema
            .get("optional_field")
            .and_then(|v| v.get("optional"))
            .and_then(|v| v.as_bool());
        assert_eq!(opt, Some(true));
        // required_field should NOT have optional=true
        let req = schema
            .get("required_field")
            .and_then(|v| v.get("optional"))
            .and_then(|v| v.as_bool());
        assert!(req.is_none() || req == Some(false));
    }
}
