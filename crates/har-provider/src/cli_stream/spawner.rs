//! Spawner trait — the injected seam for CLI-stream subprocess tests.
//!
//! The `Spawner` trait abstracts over `tokio::process::Command::spawn()` so tests
//! can inject a fake subprocess (canned NDJSON stream) without invoking a real CLI.
//!
//! Port note: this is new infrastructure (no direct TS equivalent) referenced by the
//! architect decision in `target-architecture.md §6.4` and `§6.6 cli_stream/spawner.rs`.

use std::pin::Pin;

use futures_core::Stream;
use tokio::process::Child;

use crate::cli_stream::ChildOutput;

/// A boxed stream of bytes from a fake subprocess's stdout.
pub type FakeByteStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send>>;

/// Outcome of a single spawn attempt.
pub enum SpawnOutcome {
    /// Real or fake child process.
    Real(Child),
    /// A fake subprocess: its stdout is replaced by `stream`, its exit code by `exit_code`.
    Fake {
        stdout_stream: FakeByteStream,
        exit_code: i32,
    },
}

/// Trait that abstracts subprocess spawning for testability.
///
/// The real impl calls `tokio::process::Command::spawn()`.
/// A `FakeSpawner` yields a scripted sequence of NDJSON lines then exits.
pub trait Spawner: Send + Sync {
    /// Spawn the program with the given args, env, and cwd.
    ///
    /// Returns `SpawnOutcome::Real` (real process) or `SpawnOutcome::Fake` (injected).
    fn spawn(
        &self,
        program: &str,
        args: &[String],
        env: &std::collections::HashMap<String, String>,
        cwd: &str,
    ) -> Result<SpawnOutcome, std::io::Error>;
}

// ─── Real spawner ─────────────────────────────────────────────────────────────

/// Production spawner — delegates to `tokio::process::Command`.
pub struct RealSpawner;

impl Spawner for RealSpawner {
    fn spawn(
        &self,
        program: &str,
        args: &[String],
        env: &std::collections::HashMap<String, String>,
        cwd: &str,
    ) -> Result<SpawnOutcome, std::io::Error> {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // Inject env: start from current process env, then overlay the given map.
        // This mirrors `buildSubprocessEnv` — start from process.env, then overlay requestOptions.env.
        cmd.envs(env);

        let child = cmd.spawn()?;
        Ok(SpawnOutcome::Real(child))
    }
}

// ─── Fake spawner ─────────────────────────────────────────────────────────────

/// Script entry for the fake spawner.
#[derive(Debug, Clone)]
pub enum FakeSpawnScript {
    /// Yield these NDJSON lines then exit with 0.
    Success(Vec<String>),
    /// Crash: emit nothing, exit with the given code, optionally write to stderr.
    Crash {
        exit_code: i32,
        stderr: Option<String>,
    },
}

/// A fake spawner that replays a pre-scripted sequence of spawn outcomes.
///
/// Each call to `spawn()` pops one entry from the front of `scripts`.
/// If `scripts` is exhausted, subsequent calls return `Crash { exit_code: 1 }`.
pub struct FakeSpawner {
    scripts: std::sync::Mutex<std::collections::VecDeque<FakeSpawnScript>>,
}

impl FakeSpawner {
    pub fn new(scripts: Vec<FakeSpawnScript>) -> Self {
        Self {
            scripts: std::sync::Mutex::new(scripts.into()),
        }
    }

    /// Convenience: one success outcome.
    pub fn success(lines: Vec<String>) -> Self {
        Self::new(vec![FakeSpawnScript::Success(lines)])
    }

    /// Convenience: N crash outcomes followed by one success.
    pub fn crash_then_success(
        crash_count: usize,
        crash_exit_code: i32,
        stderr_msg: Option<&str>,
        success_lines: Vec<String>,
    ) -> Self {
        let mut scripts = vec![];
        for _ in 0..crash_count {
            scripts.push(FakeSpawnScript::Crash {
                exit_code: crash_exit_code,
                stderr: stderr_msg.map(|s| s.to_owned()),
            });
        }
        scripts.push(FakeSpawnScript::Success(success_lines));
        Self::new(scripts)
    }
}

impl Spawner for FakeSpawner {
    fn spawn(
        &self,
        _program: &str,
        _args: &[String],
        _env: &std::collections::HashMap<String, String>,
        _cwd: &str,
    ) -> Result<SpawnOutcome, std::io::Error> {
        let script = {
            let mut guard = self.scripts.lock().unwrap();
            guard.pop_front()
        };

        let script = match script {
            Some(s) => s,
            None => FakeSpawnScript::Crash {
                exit_code: 1,
                stderr: Some("FakeSpawner: script queue exhausted".to_owned()),
            },
        };

        match script {
            FakeSpawnScript::Success(lines) => {
                // Build an in-memory byte stream from the NDJSON lines.
                let data: Vec<u8> = lines.into_iter().flat_map(|l| {
                    let mut b = l.into_bytes();
                    b.push(b'\n');
                    b
                }).collect();

                let stream: FakeByteStream = Box::pin(futures::stream::once(async move {
                    Ok::<bytes::Bytes, std::io::Error>(bytes::Bytes::from(data))
                }));
                Ok(SpawnOutcome::Fake { stdout_stream: stream, exit_code: 0 })
            }
            FakeSpawnScript::Crash { exit_code, stderr: _ } => {
                let stream: FakeByteStream = Box::pin(futures::stream::empty());
                Ok(SpawnOutcome::Fake { stdout_stream: stream, exit_code })
            }
        }
    }
}

/// Dummy struct to make `ChildOutput` usable with fake spawners.
///
/// When `SpawnOutcome::Fake`, callers don't have a real `Child` — the `ChildOutput`
/// is reconstructed from the fake streams.
pub struct FakeChildOutput {
    pub stdout_stream: FakeByteStream,
    pub exit_code: i32,
}

impl FakeChildOutput {
    pub fn into_child_output(self) -> ChildOutput {
        ChildOutput::Fake(self)
    }
}
