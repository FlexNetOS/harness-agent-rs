//! har-paths — Path resolution, env loading, structured logging, telemetry, and update-check.
//!
//! Ports Archon `packages/paths/src/*`:
//!   - `archon-paths.ts`   → `archon_paths::*`   (UNIT PA-01, cycle 7)
//!   - `env-loader.ts`     → `env_loader::*`      (UNIT PA-06, cycle 7)
//!   - `strip-cwd-env.ts`  → `strip_cwd_env::*`   (UNIT PA-07, cycle 7)
//!   - `logger.ts`         → MAP→`tracing`         (UNIT PA-02, not yet ported)
//!   - `telemetry.ts`      → telemetry capture fns (UNIT PA-03, not yet ported)
//!   - `update-check.ts`   → async version-check   (UNIT PA-04, not yet ported)
//!   - `bundled-build.ts`  → `is_binary_build()`  (UNIT PA-05, not yet ported)

// Cycle 7: PA-01 archon paths, PA-06 env loader, PA-07 strip-cwd-env.
pub mod archon_paths;
pub mod env_loader;
pub mod strip_cwd_env;

// Public re-exports — convenience surface for downstream crates.
pub use archon_paths::{
    expand_tilde,
    get_archon_config_path,
    get_archon_env_path,
    // Archon home
    get_archon_home,
    // Global paths
    get_archon_workspaces_path,
    get_archon_worktrees_path,
    // Command/workflow search paths
    get_command_folder_search_paths,
    // App defaults paths
    get_default_commands_path,
    get_default_workflows_path,
    get_home_commands_path,
    get_home_scripts_path,
    get_home_workflows_path,
    get_legacy_home_workflows_path,
    get_project_artifacts_path,
    get_project_logs_path,
    get_project_root,
    get_project_source_path,
    get_project_worktrees_path,
    get_repo_archon_env_path,
    get_run_artifacts_path,
    get_run_log_path,
    get_web_dist_dir,
    get_workflow_folder_search_paths,
    // Docker + tilde
    is_docker,
    // Project-centric paths
    parse_owner_repo,
    resolve_project_root_from_cwd,
    ArchonPathError,
    Result as PathResult,
};
pub use env_loader::{is_verbose_boot, load_archon_env, EnvLoadResult};
pub use strip_cwd_env::{
    strip_cwd_env, strip_cwd_env_boot, BUN_AUTO_LOADED_ENV_FILES, CLAUDE_CODE_AUTH_VARS,
    NESTED_CLAUDE_WARNING,
};
