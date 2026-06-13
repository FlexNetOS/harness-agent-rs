# Loop state — rust-port
session_started: <UTC e.g. 2026-06-13T15:00:00Z>   # you supply it; the runtime can't read the clock
loop: rust-port
branch: <branch>
worktree: <abs path>
source_root: <abs path of the project being ported, e.g. ~/Desktop/meta/Archon>
source_toolchain: <bun | node | python | ...>     # needed so the parity-verifier can RUN the source
rust_target: <abs path / crate of the Rust port>
cycle_budget: 3
cycles_this_session: 0     # reset to 0 on RESUME
cycles_total: 0
ledger: parity 0/<total> verified
last_item: (none — discovery only)
status: DISCOVER complete — parity ledger seeded
last_update: <UTC>
