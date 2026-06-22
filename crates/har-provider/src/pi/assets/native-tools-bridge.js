// Native Tools Bridge Extension for Pi RPC mode
// This extension proxies Pi native tool (customTool) execution back to Rust
// via Pi's extension_ui_request/response round-trip (RPC mode).
// The Rust RPC client detects extension_ui_request{method:"input",title:"native_tool_dispatch"}
// and executes the corresponding NativeTool handler, returning the result via extension_ui_response.

export default async function setup(ctx) {
  const toolDefsJson = process.env.NATIVE_TOOLS_BRIDGE_NAMES;
  if (!toolDefsJson) return;

  let toolDefs;
  try {
    toolDefs = JSON.parse(toolDefsJson);
  } catch (e) {
    return;
  }

  for (const toolDef of toolDefs) {
    ctx.registerTool({
      name: toolDef.name,
      label: toolDef.name,
      description: toolDef.description || '',
      parameters: toolDef.schema,
      async execute(_toolCallId, params, _signal, _onUpdate, _ctx) {
        const result = await ctx.ui.input(
          'native_tool_dispatch',
          JSON.stringify({ tool: toolDef.name, params })
        );
        if (result === undefined || result === null) {
          return { content: [{ type: 'text', text: 'Tool execution failed: no response from host' }], details: undefined };
        }
        return { content: [{ type: 'text', text: result }], details: undefined };
      }
    });
  }
}
