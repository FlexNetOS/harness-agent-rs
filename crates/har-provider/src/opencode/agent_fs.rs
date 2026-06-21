//! OpenCode agent file materialization.
//!
//! PORT of `packages/providers/src/community/opencode/agent-fs.ts`.
//!
//! # Source coverage
//!
//! - `buildAgentFileContent(agentConfig)` (agent-fs.ts:18-64) → `build_agent_file_content`
//! - `materializeAgents(cwd, agents)`     (agent-fs.ts:66-98) → `materialize_agents`
//!
//! The file format is YAML frontmatter + optional prompt body, written to
//! `<cwd>/.opencode/agents/archon-<kebab-key>.md`.
//! Stale `archon-*` files not in the current request are removed.
//! User-authored files (not starting with `archon-`) are preserved.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::opencode::agent_config::{to_kebab_case, AgentConfig};

/// Build the YAML frontmatter + body content for an agent `.md` file.
///
/// PORT of `buildAgentFileContent(agentConfig)` (agent-fs.ts:18-64).
///
/// Format:
/// ```yaml
/// ---
/// mode: subagent
/// description: "..."   # if set
/// model: "..."          # if set
/// steps: N              # if maxTurns is set
/// skills:               # if skills is non-empty
/// - "skill1"
/// tools:                # if any tools/disallowedTools
///   tool1: true
///   tool2: false
/// ---
///
/// <prompt>              # if prompt is set
/// ```
///
/// Note: values are JSON-serialized strings (equivalent to JSON.stringify in TS for string fields).
/// `serde_json::to_string` is used for string fields to get JSON-exact quoting,
/// matching `JSON.stringify` behaviour: `lines.push(\`description: ${JSON.stringify(agentConfig.description)}\`)`.
pub fn build_agent_file_content(agent_config: &AgentConfig) -> String {
    let mut lines: Vec<String> = vec!["---".to_owned()];

    lines.push("mode: subagent".to_owned());

    // JS `if (agentConfig.description)` is falsy for "" — omit the line when empty.
    if !agent_config.description.is_empty() {
        let json_desc = serde_json::to_string(&agent_config.description.as_str())
            .unwrap_or_else(|_| format!("{:?}", agent_config.description));
        lines.push(format!("description: {}", json_desc));
    }

    if let Some(model) = &agent_config.model {
        let json_model = serde_json::to_string(model.as_str()).unwrap_or_else(|_| format!("{:?}", model));
        lines.push(format!("model: {}", json_model));
    }

    if let Some(max_turns) = agent_config.max_turns {
        lines.push(format!("steps: {}", max_turns));
    }

    if let Some(skills) = &agent_config.skills {
        if !skills.is_empty() {
            lines.push("skills:".to_owned());
            for skill in skills {
                let json_skill = serde_json::to_string(skill.as_str()).unwrap_or_else(|_| format!("{:?}", skill));
                lines.push(format!("- {}", json_skill));
            }
        }
    }

    // Build tools map: allowed=true, denied=false (agent-fs.ts:42-53).
    // TS uses a plain object and iterates in insertion order: tools first, then disallowedTools.
    // We preserve that insertion order with a Vec of pairs (tools first, disallowed_tools second),
    // de-duplicating by only inserting each key once (last writer wins, matching JS object semantics).
    let mut tools_vec: Vec<(String, bool)> = Vec::new();
    let mut seen_tools: HashMap<String, usize> = HashMap::new();
    for tool in agent_config.tools.as_deref().unwrap_or(&[]) {
        if let Some(&idx) = seen_tools.get(tool) {
            tools_vec[idx] = (tool.clone(), true);
        } else {
            seen_tools.insert(tool.clone(), tools_vec.len());
            tools_vec.push((tool.clone(), true));
        }
    }
    for tool in agent_config.disallowed_tools.as_deref().unwrap_or(&[]) {
        if let Some(&idx) = seen_tools.get(tool) {
            tools_vec[idx] = (tool.clone(), false);
        } else {
            seen_tools.insert(tool.clone(), tools_vec.len());
            tools_vec.push((tool.clone(), false));
        }
    }
    if !tools_vec.is_empty() {
        lines.push("tools:".to_owned());
        for (tool, allowed) in &tools_vec {
            lines.push(format!("  {}: {}", tool, allowed));
        }
    }

    lines.push("---".to_owned());

    // prompt is always set (required field in InlineAgentDefinition)
    if !agent_config.prompt.is_empty() {
        lines.push(String::new()); // blank line between frontmatter and body
        lines.push(agent_config.prompt.clone());
    }

    lines.join("\n")
}

/// Materialize all agents for this request into `<cwd>/.opencode/agents/`.
///
/// PORT of `materializeAgents(cwd, agents)` (agent-fs.ts:66-98).
///
/// - Creates the agents directory recursively.
/// - Removes stale `archon-*` files not in the current request.
/// - Writes fresh agent files for every agent in the request.
pub async fn materialize_agents(
    cwd: &str,
    agents: &HashMap<String, AgentConfig>,
) -> Result<(), std::io::Error> {
    let agents_dir = PathBuf::from(cwd).join(".opencode").join("agents");
    tokio::fs::create_dir_all(&agents_dir).await?;

    // Compute set of archon-owned filenames for the current request
    let current_archon_files: std::collections::HashSet<String> = agents
        .keys()
        .map(|key| format!("archon-{}.md", to_kebab_case(key)))
        .collect();

    // Remove stale archon-* files not in current request
    match tokio::fs::read_dir(&agents_dir).await {
        Ok(mut entries) => {
            let mut to_remove: Vec<PathBuf> = Vec::new();
            while let Some(entry) = entries.next_entry().await? {
                let file_name = entry.file_name();
                let name = file_name.to_string_lossy().into_owned();
                if name.starts_with("archon-") && !current_archon_files.contains(&name) {
                    to_remove.push(agents_dir.join(name));
                }
            }
            for path in to_remove {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    tracing::debug!(
                        err = %e,
                        path = %path.display(),
                        "opencode.agent_fs_remove_stale_failed"
                    );
                }
            }
        }
        Err(e) => {
            // mkdir above ensures the dir exists; other errors are non-fatal for cleanup
            tracing::debug!(
                err = %e,
                agents_dir = %agents_dir.display(),
                "opencode.agent_fs_readdir_failed"
            );
        }
    }

    // Write all agent files for this request
    let mut write_tasks = Vec::new();
    for (key, config) in agents {
        let filename = format!("archon-{}.md", to_kebab_case(key));
        let content = build_agent_file_content(config);
        let path = agents_dir.join(&filename);
        write_tasks.push(tokio::fs::write(path, content));
    }

    let results = futures_util::future::join_all(write_tasks).await;
    for r in results {
        r?;
    }

    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use har_contract::InlineAgentDefinition;
    use std::path::Path;
    use tempfile::TempDir;

    fn make_config(
        description: &str,
        prompt: &str,
        model: Option<&str>,
        tools: Option<Vec<&str>>,
        disallowed_tools: Option<Vec<&str>>,
        skills: Option<Vec<&str>>,
        max_turns: Option<u32>,
    ) -> AgentConfig {
        InlineAgentDefinition {
            description: description.to_owned(),
            prompt: prompt.to_owned(),
            model: model.map(str::to_owned),
            tools: tools.map(|v| v.into_iter().map(str::to_owned).collect()),
            disallowed_tools: disallowed_tools.map(|v| v.into_iter().map(str::to_owned).collect()),
            skills: skills.map(|v| v.into_iter().map(str::to_owned).collect()),
            max_turns,
        }
    }

    // ── build_agent_file_content ──────────────────────────────────────────────

    #[test]
    fn basic_subagent_mode_is_set() {
        let config = make_config("Test agent", "You are helpful", None, None, None, None, None);
        let content = build_agent_file_content(&config);
        // Exact byte check: mode line is present and description is JSON-quoted.
        assert_eq!(
            content,
            "---\nmode: subagent\ndescription: \"Test agent\"\n---\n\nYou are helpful"
        );
    }

    #[test]
    fn description_is_json_quoted() {
        let config = make_config("Code review specialist", "Review the patch carefully", None, None, None, None, None);
        let content = build_agent_file_content(&config);
        assert_eq!(
            content,
            "---\nmode: subagent\ndescription: \"Code review specialist\"\n---\n\nReview the patch carefully"
        );
    }

    /// D1: empty description must be OMITTED — JS `if (agentConfig.description)` is falsy for "".
    #[test]
    fn empty_description_is_omitted() {
        let config = make_config("", "has prompt", None, None, None, None, None);
        assert_eq!(
            build_agent_file_content(&config),
            "---\nmode: subagent\n---\n\nhas prompt"
        );
    }

    /// D1: both empty — no description line, no blank line, no prompt body.
    #[test]
    fn empty_description_and_empty_prompt() {
        let config = make_config("", "", None, None, None, None, None);
        assert_eq!(build_agent_file_content(&config), "---\nmode: subagent\n---");
    }

    #[test]
    fn model_is_json_quoted() {
        let config = make_config("Agent", "prompt", Some("anthropic/claude-3-5-sonnet"), None, None, None, None);
        let content = build_agent_file_content(&config);
        assert_eq!(
            content,
            "---\nmode: subagent\ndescription: \"Agent\"\nmodel: \"anthropic/claude-3-5-sonnet\"\n---\n\nprompt"
        );
    }

    #[test]
    fn max_turns_maps_to_steps() {
        let config = make_config("Agent", "prompt", None, None, None, None, Some(7));
        let content = build_agent_file_content(&config);
        assert_eq!(
            content,
            "---\nmode: subagent\ndescription: \"Agent\"\nsteps: 7\n---\n\nprompt"
        );
    }

    #[test]
    fn skills_are_yaml_listed() {
        let config = make_config("Agent", "prompt", None, None, None, Some(vec!["review-work"]), None);
        let content = build_agent_file_content(&config);
        assert_eq!(
            content,
            "---\nmode: subagent\ndescription: \"Agent\"\nskills:\n- \"review-work\"\n---\n\nprompt"
        );
    }

    /// D2: tools must follow insertion order (tools first, disallowed_tools second).
    #[test]
    fn tools_and_disallowed_tools_insertion_order() {
        let config = make_config(
            "Agent", "prompt",
            None,
            Some(vec!["read", "grep"]),
            Some(vec!["bash"]),
            None, None,
        );
        assert_eq!(
            build_agent_file_content(&config),
            "---\nmode: subagent\ndescription: \"Agent\"\ntools:\n  read: true\n  grep: true\n  bash: false\n---\n\nprompt"
        );
    }

    #[test]
    fn prompt_body_appended_after_frontmatter() {
        let config = make_config("Agent", "Review the patch carefully", None, None, None, None, None);
        let content = build_agent_file_content(&config);
        assert_eq!(
            content,
            "---\nmode: subagent\ndescription: \"Agent\"\n---\n\nReview the patch carefully"
        );
    }

    /// D2: Full reviewer config — exact byte match including insertion-order tools.
    #[test]
    fn full_reviewer_config_matches_test_spec() {
        // Mirrors provider.test.ts: "materializes workflow agents ..."
        let config = make_config(
            "Code review specialist",
            "Review the patch carefully",
            Some("anthropic/claude-3-5-sonnet"),
            Some(vec!["read", "grep"]),
            Some(vec!["bash"]),
            Some(vec!["review-work"]),
            Some(7),
        );
        assert_eq!(
            build_agent_file_content(&config),
            "---\nmode: subagent\ndescription: \"Code review specialist\"\nmodel: \"anthropic/claude-3-5-sonnet\"\nsteps: 7\nskills:\n- \"review-work\"\ntools:\n  read: true\n  grep: true\n  bash: false\n---\n\nReview the patch carefully"
        );
    }

    // ── materialize_agents ────────────────────────────────────────────────────

    #[tokio::test]
    async fn materialize_creates_agents_dir_and_writes_files() {
        let tmp = TempDir::new().unwrap();
        let cwd = tmp.path().to_str().unwrap();
        let mut agents = HashMap::new();
        agents.insert(
            "Reviewer".to_owned(),
            make_config("Reviewer", "Review the code", None, None, None, None, None),
        );

        materialize_agents(cwd, &agents).await.unwrap();

        let agent_path = Path::new(cwd).join(".opencode").join("agents").join("archon-reviewer.md");
        assert!(agent_path.exists());
        let content = tokio::fs::read_to_string(&agent_path).await.unwrap();
        assert!(content.contains("mode: subagent"));
        assert!(content.contains("Review the code"));
    }

    #[tokio::test]
    async fn materialize_removes_stale_archon_files() {
        let tmp = TempDir::new().unwrap();
        let cwd = tmp.path().to_str().unwrap();
        let agents_dir = Path::new(cwd).join(".opencode").join("agents");
        tokio::fs::create_dir_all(&agents_dir).await.unwrap();

        // Create a stale archon file and a user file
        tokio::fs::write(agents_dir.join("archon-stale-agent.md"), "stale").await.unwrap();
        tokio::fs::write(agents_dir.join("custom-agent.md"), "# user agent").await.unwrap();
        tokio::fs::write(agents_dir.join("archon-keep-agent.md"), "keep").await.unwrap();

        let mut agents = HashMap::new();
        agents.insert(
            "Keep Agent".to_owned(),
            make_config("Fresh agent", "Fresh prompt", None, None, None, None, None),
        );

        materialize_agents(cwd, &agents).await.unwrap();

        // Custom user file untouched
        assert!(agents_dir.join("custom-agent.md").exists());
        // Keep agent refreshed
        let keep_content = tokio::fs::read_to_string(agents_dir.join("archon-keep-agent.md")).await.unwrap();
        assert!(keep_content.contains("Fresh prompt"));
        // Stale archon file removed
        assert!(!agents_dir.join("archon-stale-agent.md").exists());
    }
}
