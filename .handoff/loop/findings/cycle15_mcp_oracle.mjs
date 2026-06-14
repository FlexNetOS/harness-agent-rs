// Live SDK oracle for cycle-15 differential parity.
// Drives the REAL in-process SDK MCP server (createSdkMcpServer + tool) over an
// in-memory transport pair, issues the same requests the Rust McpSidecar handles,
// captures exact wire JSON. Run with bun from packages/providers (workspace resolve).
import { createSdkMcpServer, tool } from '@anthropic-ai/claude-agent-sdk';
import { z } from 'zod';
import { Client } from '@modelcontextprotocol/sdk/client/index.js';
import { InMemoryTransport } from '@modelcontextprotocol/sdk/inMemory.js';

// ── Replicate jsonSchemaToZodShape (native-tools.ts:24-59) exactly ──
const isString = (v) => typeof v === 'string';
function jsonSchemaToZodShape(schema) {
  if (schema.type !== 'object' || typeof schema.properties !== 'object' || schema.properties === null) {
    throw new Error('native tool inputSchema must be an object schema with `properties`');
  }
  const props = schema.properties;
  const required = new Set(Array.isArray(schema.required) ? schema.required.filter(isString) : []);
  const shape = {};
  for (const [key, prop] of Object.entries(props)) {
    let field;
    if (Array.isArray(prop.enum)) {
      const values = prop.enum.filter(isString);
      if (values.length === 0) throw new Error(`enum '${key}' empty`);
      field = z.enum(values);
    } else if (prop.type === 'string') field = z.string();
    else if (prop.type === 'boolean') field = z.boolean();
    else throw new Error(`unsupported '${key}'`);
    if (typeof prop.description === 'string') field = field.describe(prop.description);
    shape[key] = required.has(key) ? field : field.optional();
  }
  return shape;
}

// ── The REAL manage_run INPUT_SCHEMA (manage-run-tool.ts:54-89) ──
const ACTIONS = ['help','list','get','start','resume','cancel','abandon','approve','reject'];
const INPUT_SCHEMA = {
  type: 'object',
  properties: {
    action: { type: 'string', enum: [...ACTIONS], description: "What to do. Call action='help' (optionally with subtool=<action>) to see exactly what each action needs before using it." },
    subtool: { type: 'string', description: "For action=help: the action to describe (e.g. 'approve'). Omit for an overview." },
    runId: { type: 'string', description: 'Run id — required for get/resume/cancel/abandon/approve/reject. Accepts the short (8-char) or full id.' },
    workflow: { type: 'string', description: 'Workflow name to launch — required for action=start.' },
    message: { type: 'string', description: 'Free text whose meaning depends on the action: start=the prompt/instructions; approve=optional comment; reject=the reason.' },
    confirm: { type: 'boolean', description: 'Required (true) to actually perform a destructive action (cancel/abandon/approve/reject). Omit first to get a preview.' },
  },
  required: ['action'],
};

const DESC = "Inspect and operate this project's workflow runs.";

// behavior flags via env
const MODE = process.env.ORACLE_MODE || 'normal'; // normal | throw

const handler = async (args) => {
  if (MODE === 'throw') throw new Error('handler exploded');
  return JSON.stringify({ ok: true, got: args });
};

const cfg = createSdkMcpServer({
  name: 'archon',
  version: '1.0.0',
  alwaysLoad: true,
  tools: [ tool('manage_run', DESC, jsonSchemaToZodShape(INPUT_SCHEMA), async (args) => ({ content: [{ type: 'text', text: await handler(args) }] })) ],
});

// cfg.instance is an McpServer; connect its underlying server to in-memory transport.
const mcpServer = cfg.instance;
const [clientTransport, serverTransport] = InMemoryTransport.createLinkedPair();

const client = new Client({ name: 'oracle', version: '1.0.0' }, { capabilities: {} });

// connect server side (McpServer has .connect)
await mcpServer.connect(serverTransport);
await client.connect(clientTransport);

const out = {};

// 1. initialize — captured via the client's stored server result
out.initialize = {
  serverInfo: client.getServerVersion(),
  capabilities: client.getServerCapabilities(),
  // protocolVersion is negotiated internally; capture via a raw request below
};

// raw initialize to see exact wire (client.connect already did handshake; do a raw request for protocolVersion echo check)
// Instead: capture serverCapabilities + serverVersion which come straight from the initialize result.

// 2. tools/list — raw
out.tools_list = await client.request({ method: 'tools/list' }, z.any());

// 3. tools/call happy
if (MODE === 'normal') {
  out.tools_call_happy = await client.request(
    { method: 'tools/call', params: { name: 'manage_run', arguments: { action: 'list' } } }, z.any());
  // 5. bad args (invalid enum)
  out.tools_call_badargs = await client.request(
    { method: 'tools/call', params: { name: 'manage_run', arguments: { action: 'NOPE' } } }, z.any());
  // missing required
  out.tools_call_missing = await client.request(
    { method: 'tools/call', params: { name: 'manage_run', arguments: {} } }, z.any());
  // unknown tool
  try {
    out.tools_call_unknown = await client.request(
      { method: 'tools/call', params: { name: 'no_such', arguments: {} } }, z.any());
  } catch (e) { out.tools_call_unknown_threw = { code: e.code, message: e.message }; }
}
if (MODE === 'throw') {
  // 4. handler throw
  out.tools_call_throw = await client.request(
    { method: 'tools/call', params: { name: 'manage_run', arguments: { action: 'list' } } }, z.any());
}

// 6. ping
out.ping = await client.request({ method: 'ping' }, z.any());

// unknown method
try {
  out.unknown_method = await client.request({ method: 'methods/unknown' }, z.any());
} catch (e) { out.unknown_method_threw = { code: e.code, message: e.message }; }

console.log(JSON.stringify(out, null, 2));
await client.close();
process.exit(0);
