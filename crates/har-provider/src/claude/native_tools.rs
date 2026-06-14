//! Claude native-tools bridge — JSON Schema → SDK tool definition conversion.
//!
//! PORT of `packages/providers/src/claude/native-tools.ts`.
//!
//! The TS source (`native-tools.ts`) has two concerns:
//!
//! 1. `jsonSchemaToZodShape(schema)` — validates the schema and converts it to Zod
//!    field definitions (string / string-enum / boolean). This is deterministic and pure.
//! 2. `buildArchonMcpServer(tools)` — uses the Claude Agent SDK's `createSdkMcpServer`
//!    and `tool()` to build an in-process MCP server object. This is SDK-specific and has
//!    no direct equivalent in the Rust CLI-delegation model (MAP→provider CLIs, ADR-0001).
//!
//! Rust port strategy:
//!   - `validate_and_convert_schema(schema)` — ports `jsonSchemaToZodShape` exactly:
//!     validates the schema shape, fails-fast on unsupported types, produces a
//!     `Vec<ToolField>` describing each parameter (the Rust equivalent of ZodRawShape).
//!   - `build_archon_mcp_server_descriptor(tools)` — produces a `McpServerDescriptor`
//!     (a serializable representation of the tool set) rather than an opaque SDK object.
//!     This descriptor is what a spawned MCP server process would use to register tools.
//!     NEEDS-HUMAN: The PR-03 Claude provider implementation must decide how to start the
//!     MCP server process and wire it to the Claude CLI (`--mcp-server` flag or equivalent).
//!   - `ARCHON_TOOL_SERVER` constant — same value as source.
//!
//! Behavioral invariants preserved:
//!   - `validate_and_convert_schema` fails-fast on any unsupported type (not just string/enum/boolean).
//!   - Empty enum arrays throw an error.
//!   - Non-object schemas throw an error.
//!   - `required` fields are marked required vs optional exactly as in source.
//!   - `description` is forwarded if present.

use har_contract::NativeTool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// The in-process MCP server name; tools are callable as `mcp__archon__<name>`.
///
/// Source: `packages/providers/src/claude/native-tools.ts:14`
pub const ARCHON_TOOL_SERVER: &str = "archon";

/// The kind of a tool field — mirrors the Zod types the TS source supports.
///
/// Source: `packages/providers/src/claude/native-tools.ts:39-58` (supported branches).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolFieldKind {
    /// `z.string()` — a plain string field.
    String,
    /// `z.enum([...])` — a string enum. The values list must be non-empty.
    StringEnum { values: Vec<String> },
    /// `z.boolean()` — a boolean field.
    Boolean,
}

/// A single field in a converted tool schema.
///
/// Corresponds to one entry in the Zod `RawShape` the TS source builds.
/// Source: `packages/providers/src/claude/native-tools.ts:37-57`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolField {
    /// Field name (key in the JSON Schema `properties` object).
    pub name: String,
    /// The field type (string / string-enum / boolean).
    pub kind: ToolFieldKind,
    /// Optional description forwarded from the JSON Schema property.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether this field is in the schema's `required` array.
    pub required: bool,
}

/// Converted tool definition — one `NativeTool` in a form suitable for an MCP server.
///
/// This is the serializable equivalent of what the TS source passes to `tool()` from the SDK.
/// Source: `packages/providers/src/claude/native-tools.ts:70-88`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SdkToolDef {
    /// Tool name (matches `NativeTool::name`).
    pub name: String,
    /// Tool description (matches `NativeTool::description`).
    pub description: String,
    /// Converted schema fields (one per property).
    pub fields: Vec<ToolField>,
}

/// MCP server descriptor — the serializable representation of an in-process MCP server.
///
/// NEEDS-HUMAN (PR-03): The Claude CLI delegation model must decide how to start an MCP
/// server subprocess from this descriptor and pass it to the Claude CLI via `--mcp-server`
/// or equivalent. The TS source calls `createSdkMcpServer()` which builds an in-process
/// object; in Rust CLI mode, this becomes an external process.
///
/// Source: `packages/providers/src/claude/native-tools.ts:70-87`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerDescriptor {
    /// Server name (always `ARCHON_TOOL_SERVER = "archon"`).
    pub name: String,
    /// Server version (always `"1.0.0"` matching the TS source).
    pub version: String,
    /// Whether tools are always loaded without tool-search (matches `alwaysLoad: true`).
    pub always_load: bool,
    /// The tool definitions, one per `NativeTool` in the input.
    pub tools: Vec<SdkToolDef>,
}

/// Convert a `NativeTool`'s JSON Schema into a list of `ToolField` descriptors.
///
/// Validates the schema fail-fast (mirrors `jsonSchemaToZodShape`):
/// - Schema must be `{ type: 'object', properties: { ... } }`.
/// - Each property must be `string`, `string-enum`, or `boolean`.
/// - Enum arrays must be non-empty and contain strings.
///
/// Source: `packages/providers/src/claude/native-tools.ts:24-59`
pub fn validate_and_convert_schema(
    schema: &serde_json::Map<String, Value>,
    tool_name: &str,
) -> Result<Vec<ToolField>, String> {
    // `schema.type !== 'object'` check
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err("native tool inputSchema must be an object schema with `properties`".to_owned());
    }
    // `typeof schema.properties !== 'object' || schema.properties === null`
    let props = match schema.get("properties") {
        Some(Value::Object(m)) => m,
        _ => {
            return Err(
                "native tool inputSchema must be an object schema with `properties`".to_owned(),
            )
        }
    };

    // Build `required` set from `schema.required` array.
    // `Array.isArray(schema.required) ? schema.required.filter(isString) : []`
    let required: HashSet<String> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let mut fields = Vec::new();

    for (key, prop) in props {
        let prop_obj = match prop {
            Value::Object(m) => m,
            _ => {
                return Err(format!(
                    "native tool schema: unsupported type for '{key}' (only string / string-enum / boolean)"
                ))
            }
        };

        // `if (Array.isArray(prop.enum))` — check for string-enum first.
        let kind = if let Some(Value::Array(enum_vals)) = prop_obj.get("enum") {
            let values: Vec<String> = enum_vals
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            if values.is_empty() {
                return Err(format!(
                    "native tool schema: enum for '{key}' must be non-empty strings"
                ));
            }
            ToolFieldKind::StringEnum { values }
        } else if prop_obj.get("type").and_then(Value::as_str) == Some("string") {
            ToolFieldKind::String
        } else if prop_obj.get("type").and_then(Value::as_str) == Some("boolean") {
            ToolFieldKind::Boolean
        } else {
            return Err(format!(
                "native tool schema: unsupported type for '{key}' (only string / string-enum / boolean)"
            ));
        };

        // `if (typeof prop.description === 'string') field = field.describe(prop.description)`
        let description = prop_obj
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned);

        fields.push(ToolField {
            name: key.clone(),
            kind,
            description,
            required: required.contains(key),
        });
    }

    // Suppress unused variable warning (tool_name used only for error context — future use).
    let _ = tool_name;

    Ok(fields)
}

/// Build a single in-process MCP server descriptor exposing the given `NativeTool`s.
///
/// `alwaysLoad: true` keeps the tools visible without tool-search (which Haiku lacks).
/// Each tool's handler maps its text result into a `CallToolResult`.
///
/// NEEDS-HUMAN (PR-03): In the Rust CLI model, the caller must spawn a real MCP server
/// process from this descriptor and pass the server endpoint to the Claude CLI. The TS
/// source calls `createSdkMcpServer()` for an in-process server; that API is SDK-internal.
///
/// Source: `packages/providers/src/claude/native-tools.ts:70-87`
pub fn build_archon_mcp_server(
    native_tools: &[NativeTool],
) -> Result<McpServerDescriptor, String> {
    let mut tool_defs = Vec::new();

    for tool in native_tools {
        // `jsonSchemaToZodShape(spec.inputSchema)` — validate and convert.
        let schema_obj = match &serde_json::to_value(&tool.input_schema) {
            Ok(Value::Object(m)) => m.clone(),
            _ => {
                return Err(format!(
                    "native tool '{}' inputSchema is not a JSON object",
                    tool.name
                ))
            }
        };
        let fields = validate_and_convert_schema(&schema_obj, &tool.name)?;

        tool_defs.push(SdkToolDef {
            name: tool.name.clone(),
            description: tool.description.clone(),
            fields,
        });
    }

    Ok(McpServerDescriptor {
        name: ARCHON_TOOL_SERVER.to_owned(),
        version: "1.0.0".to_owned(),
        always_load: true,
        tools: tool_defs,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::NativeTool;
    use serde_json::json;
    use std::collections::HashMap;

    fn make_tool(name: &str, input_schema: serde_json::Value) -> NativeTool {
        let schema_map: HashMap<String, serde_json::Value> =
            serde_json::from_value(input_schema).unwrap();
        NativeTool {
            name: name.to_owned(),
            description: "test tool".to_owned(),
            input_schema: schema_map,
            handler: None,
        }
    }

    fn valid_schema() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["list", "get"], "description": "the action" },
                "runId": { "type": "string" },
                "confirm": { "type": "boolean", "description": "guard" }
            },
            "required": ["action"]
        })
    }

    // ── ARCHON_TOOL_SERVER constant ───────────────────────────────────────────

    #[test]
    fn archon_tool_server_is_archon() {
        assert_eq!(ARCHON_TOOL_SERVER, "archon");
    }

    // ── validate_and_convert_schema — happy path ──────────────────────────────

    #[test]
    fn valid_schema_produces_three_fields() {
        let schema = valid_schema();
        let obj = match schema {
            Value::Object(m) => m,
            _ => panic!(),
        };
        let fields = validate_and_convert_schema(&obj, "test_tool").unwrap();
        assert_eq!(fields.len(), 3);

        let action = fields.iter().find(|f| f.name == "action").unwrap();
        assert_eq!(
            action.kind,
            ToolFieldKind::StringEnum {
                values: vec!["list".to_owned(), "get".to_owned()]
            }
        );
        assert_eq!(action.description.as_deref(), Some("the action"));
        assert!(action.required);

        let run_id = fields.iter().find(|f| f.name == "runId").unwrap();
        assert_eq!(run_id.kind, ToolFieldKind::String);
        assert!(!run_id.required);
        assert!(run_id.description.is_none());

        let confirm = fields.iter().find(|f| f.name == "confirm").unwrap();
        assert_eq!(confirm.kind, ToolFieldKind::Boolean);
        assert_eq!(confirm.description.as_deref(), Some("guard"));
        assert!(!confirm.required);
    }

    // ── validate_and_convert_schema — error paths ─────────────────────────────

    #[test]
    fn non_object_schema_type_fails() {
        let schema = json!({"type": "string"});
        let obj = match schema {
            Value::Object(m) => m,
            _ => panic!(),
        };
        let err = validate_and_convert_schema(&obj, "t").unwrap_err();
        assert!(err.contains("must be an object schema"), "error: {err}");
    }

    #[test]
    fn schema_without_properties_fails() {
        let schema = json!({"type": "object"});
        let obj = match schema {
            Value::Object(m) => m,
            _ => panic!(),
        };
        let err = validate_and_convert_schema(&obj, "t").unwrap_err();
        assert!(err.contains("must be an object schema"), "error: {err}");
    }

    #[test]
    fn unsupported_type_number_fails() {
        let schema = json!({
            "type": "object",
            "properties": { "n": { "type": "number" } },
            "required": []
        });
        let obj = match schema {
            Value::Object(m) => m,
            _ => panic!(),
        };
        let err = validate_and_convert_schema(&obj, "t").unwrap_err();
        assert!(err.contains("unsupported type"), "error: {err}");
    }

    #[test]
    fn empty_enum_fails() {
        let schema = json!({
            "type": "object",
            "properties": { "a": { "enum": [] } },
            "required": ["a"]
        });
        let obj = match schema {
            Value::Object(m) => m,
            _ => panic!(),
        };
        let err = validate_and_convert_schema(&obj, "t").unwrap_err();
        assert!(err.contains("non-empty strings"), "error: {err}");
    }

    // ── build_archon_mcp_server ───────────────────────────────────────────────

    #[test]
    fn builds_descriptor_with_correct_metadata() {
        let tool = make_tool("manage_run", valid_schema());
        let desc = build_archon_mcp_server(&[tool]).unwrap();
        assert_eq!(desc.name, "archon");
        assert_eq!(desc.version, "1.0.0");
        assert!(desc.always_load);
    }

    #[test]
    fn builds_descriptor_with_one_tool_def() {
        let tool = make_tool("manage_run", valid_schema());
        let desc = build_archon_mcp_server(&[tool]).unwrap();
        assert_eq!(desc.tools.len(), 1);
        assert_eq!(desc.tools[0].name, "manage_run");
        assert_eq!(desc.tools[0].description, "test tool");
    }

    #[test]
    fn rejects_non_object_schema_via_build() {
        let schema = json!({"type": "string"});
        let tool = make_tool("bad_tool", schema);
        let err = build_archon_mcp_server(&[tool]).unwrap_err();
        assert!(err.contains("must be an object schema"), "error: {err}");
    }

    #[test]
    fn rejects_unsupported_field_type_via_build() {
        let schema = json!({
            "type": "object",
            "properties": { "count": { "type": "number" } },
            "required": []
        });
        let tool = make_tool("bad_tool", schema);
        let err = build_archon_mcp_server(&[tool]).unwrap_err();
        assert!(err.contains("unsupported type"), "error: {err}");
    }

    #[test]
    fn rejects_empty_enum_via_build() {
        let schema = json!({
            "type": "object",
            "properties": { "a": { "enum": [] } },
            "required": ["a"]
        });
        let tool = make_tool("bad_tool", schema);
        let err = build_archon_mcp_server(&[tool]).unwrap_err();
        assert!(err.contains("non-empty strings"), "error: {err}");
    }

    #[test]
    fn empty_tools_list_produces_empty_descriptor() {
        let desc = build_archon_mcp_server(&[]).unwrap();
        assert_eq!(desc.tools.len(), 0);
        assert_eq!(desc.name, "archon");
    }

    #[test]
    fn multiple_tools_are_all_included() {
        let t1 = make_tool(
            "tool_one",
            json!({
                "type": "object",
                "properties": { "x": { "type": "string" } },
                "required": ["x"]
            }),
        );
        let t2 = make_tool(
            "tool_two",
            json!({
                "type": "object",
                "properties": { "flag": { "type": "boolean" } },
                "required": []
            }),
        );
        let desc = build_archon_mcp_server(&[t1, t2]).unwrap();
        assert_eq!(desc.tools.len(), 2);
        assert_eq!(desc.tools[0].name, "tool_one");
        assert_eq!(desc.tools[1].name, "tool_two");
    }

    // ── required vs optional ─────────────────────────────────────────────────

    #[test]
    fn required_fields_are_marked_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "req": { "type": "string" },
                "opt": { "type": "string" }
            },
            "required": ["req"]
        });
        let obj = match schema {
            Value::Object(m) => m,
            _ => panic!(),
        };
        let fields = validate_and_convert_schema(&obj, "t").unwrap();
        let req = fields.iter().find(|f| f.name == "req").unwrap();
        let opt = fields.iter().find(|f| f.name == "opt").unwrap();
        assert!(req.required);
        assert!(!opt.required);
    }

    // ── no required array ────────────────────────────────────────────────────

    #[test]
    fn schema_without_required_array_all_fields_optional() {
        let schema = json!({
            "type": "object",
            "properties": {
                "x": { "type": "string" },
                "y": { "type": "boolean" }
            }
        });
        let obj = match schema {
            Value::Object(m) => m,
            _ => panic!(),
        };
        let fields = validate_and_convert_schema(&obj, "t").unwrap();
        for f in &fields {
            assert!(!f.required, "field '{}' should not be required", f.name);
        }
    }
}
