//! har-cli — Binary entrypoint and command dispatch.
//!
//! Ports Archon `packages/cli/src/*`:
//!   - Top-level command dispatch (`cli/src/index.ts`, `cli/src/cli.ts`)
//!   - Subcommands: `run`, `server`, `workflow`, etc.
//!
//! Uses `clap` (derive) for argument parsing; `anyhow` for top-level error context.
//! Runtime: `#[tokio::main(flavor = "multi_thread")]` — the one place a runtime is started.
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 16.

fn main() {
    // Placeholder: entrypoint will be filled in cycle 16 after har-server is ported.
    eprintln!("harness-agent-rs: not yet implemented — port in progress");
    std::process::exit(1);
}
