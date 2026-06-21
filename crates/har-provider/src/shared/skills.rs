//! Skill-directory resolver.
//!
//! PORT of `packages/providers/src/shared/skills.ts`.
//!
//! Resolves named skills to absolute directory paths. Each skill is expected to be a
//! directory containing a `SKILL.md` file (agentskills.io standard layout).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Result of resolving a set of skill names.
///
/// Port of `ResolvedSkills` (skills.ts:5-10).
pub struct ResolvedSkills {
    /// Absolute paths to resolved skill directories. Each contains a SKILL.md.
    pub paths: Vec<String>,
    /// Skill names that couldn't be resolved in any search location.
    pub missing: Vec<String>,
}

/// Search roots for skill discovery, ordered by priority.
///
/// Port of `skillSearchRoots(cwd)` (skills.ts:28-39).
///
/// Order (first match wins per name):
///  1. `<cwd>/.agents/skills/`   — project-local, agentskills.io standard
///  2. `<cwd>/.claude/skills/`   — project-local, Claude convention
///  3. `~/.agents/skills/`       — user-global, agentskills.io standard
///  4. `~/.claude/skills/`       — user-global, Claude convention
fn skill_search_roots(cwd: &str) -> Vec<PathBuf> {
    // Prefer HOME env var over system lookup (mirrors TS `process.env.HOME ?? homedir()`).
    let home = std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("~"));

    let cwd_path = Path::new(cwd);
    vec![
        cwd_path.join(".agents").join("skills"),
        cwd_path.join(".claude").join("skills"),
        home.join(".agents").join("skills"),
        home.join(".claude").join("skills"),
    ]
}

/// Resolve Archon's name-based `skills:` nodeConfig references to absolute directory paths.
///
/// Duplicate names are de-duped; empty/non-string entries (after trim) are skipped.
/// Unresolved names are returned in `missing` for caller-side warning.
///
/// Port of `resolveSkillDirectories(cwd, skillNames)` (skills.ts:49-91).
pub fn resolve_skill_directories(cwd: &str, skill_names: &[String]) -> ResolvedSkills {
    if skill_names.is_empty() {
        return ResolvedSkills {
            paths: vec![],
            missing: vec![],
        };
    }

    let roots = skill_search_roots(cwd);
    let mut paths: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for raw_name in skill_names {
        let name = raw_name.trim();
        if name.is_empty() {
            continue;
        }

        // Name-only contract: reject path traversal, nested paths, and absolute paths.
        // Port of: `isAbsolute(name) || basename(name) !== name || name === '.' || name === '..'`
        // (skills.ts:67).
        if is_bad_skill_name(name) {
            missing.push(raw_name.clone());
            continue;
        }

        if seen.contains(name) {
            continue;
        }
        seen.insert(name.to_owned());

        let mut found: Option<String> = None;
        for root in &roots {
            let candidate = root.join(name);
            let skill_md = candidate.join("SKILL.md");
            if skill_md.exists() {
                found = Some(candidate.to_string_lossy().into_owned());
                break;
            }
        }

        if let Some(path) = found {
            paths.push(path);
        } else {
            missing.push(raw_name.clone());
        }
    }

    ResolvedSkills { paths, missing }
}

/// True if a skill name contains path traversal, is nested, or is absolute.
///
/// Port of `isAbsolute(name) || basename(name) !== name || name === '.' || name === '..'`
/// (skills.ts:67).
fn is_bad_skill_name(name: &str) -> bool {
    if name == "." || name == ".." {
        return true;
    }
    let p = Path::new(name);
    // Absolute path
    if p.is_absolute() {
        return true;
    }
    // Has multiple components (nested path like "a/b")
    let components: Vec<_> = p.components().collect();
    if components.len() != 1 {
        return true;
    }
    // basename == name check passes by construction if single-component
    false
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_skill(base: &Path, name: &str) {
        let skill_dir = base.join(name);
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# skill").unwrap();
    }

    // ── resolve_skill_directories ────────────────────────────────────────────

    #[test]
    fn empty_skills_returns_empty() {
        let result = resolve_skill_directories("/tmp", &[]);
        assert!(result.paths.is_empty());
        assert!(result.missing.is_empty());
    }

    #[test]
    fn finds_skill_in_cwd_agents_skills() {
        let tmp = TempDir::new().unwrap();
        let skill_root = tmp.path().join(".agents").join("skills");
        make_skill(&skill_root, "my-skill");

        let result =
            resolve_skill_directories(tmp.path().to_str().unwrap(), &["my-skill".to_owned()]);
        assert_eq!(result.paths.len(), 1);
        assert!(result.paths[0].contains("my-skill"));
        assert!(result.missing.is_empty());
    }

    #[test]
    fn finds_skill_in_cwd_claude_skills() {
        let tmp = TempDir::new().unwrap();
        let skill_root = tmp.path().join(".claude").join("skills");
        make_skill(&skill_root, "claude-skill");

        let result =
            resolve_skill_directories(tmp.path().to_str().unwrap(), &["claude-skill".to_owned()]);
        assert_eq!(result.paths.len(), 1);
        assert!(result.missing.is_empty());
    }

    #[test]
    fn missing_skill_goes_to_missing() {
        let result =
            resolve_skill_directories("/tmp/nonexistent-cwd-42", &["no-such-skill".to_owned()]);
        assert!(result.paths.is_empty());
        assert_eq!(result.missing, vec!["no-such-skill"]);
    }

    #[test]
    fn rejects_path_traversal_skill_name() {
        let result = resolve_skill_directories("/tmp", &["../secret".to_owned()]);
        assert!(result.paths.is_empty());
        assert_eq!(result.missing, vec!["../secret"]);
    }

    #[test]
    fn rejects_nested_skill_name() {
        let result = resolve_skill_directories("/tmp", &["foo/bar".to_owned()]);
        assert!(result.paths.is_empty());
        assert_eq!(result.missing, vec!["foo/bar"]);
    }

    #[test]
    fn rejects_absolute_skill_name() {
        let result = resolve_skill_directories("/tmp", &["/absolute/path".to_owned()]);
        assert!(result.paths.is_empty());
        assert_eq!(result.missing, vec!["/absolute/path"]);
    }

    #[test]
    fn deduplicates_skill_names() {
        let tmp = TempDir::new().unwrap();
        let skill_root = tmp.path().join(".agents").join("skills");
        make_skill(&skill_root, "dup-skill");

        let result = resolve_skill_directories(
            tmp.path().to_str().unwrap(),
            &["dup-skill".to_owned(), "dup-skill".to_owned()],
        );
        // Should resolve once, not twice
        assert_eq!(result.paths.len(), 1);
    }

    #[test]
    fn dot_skill_name_is_rejected() {
        let result = resolve_skill_directories("/tmp", &[".".to_owned()]);
        assert!(result.paths.is_empty());
        assert_eq!(result.missing, vec!["."]);
    }

    #[test]
    fn dotdot_skill_name_is_rejected() {
        let result = resolve_skill_directories("/tmp", &["..".to_owned()]);
        assert!(result.paths.is_empty());
        assert_eq!(result.missing, vec![".."]);
    }

    #[test]
    fn agents_skills_takes_precedence_over_claude_skills() {
        let tmp = TempDir::new().unwrap();
        let agents_root = tmp.path().join(".agents").join("skills");
        let claude_root = tmp.path().join(".claude").join("skills");
        make_skill(&agents_root, "shared-skill");
        make_skill(&claude_root, "shared-skill");

        let result =
            resolve_skill_directories(tmp.path().to_str().unwrap(), &["shared-skill".to_owned()]);
        assert_eq!(result.paths.len(), 1);
        // Should prefer .agents/skills
        assert!(result.paths[0].contains(".agents"));
    }

    #[test]
    fn empty_skill_name_after_trim_is_skipped() {
        let result = resolve_skill_directories("/tmp", &["   ".to_owned()]);
        // Whitespace-only name is skipped silently (not added to missing)
        // Source: skills.ts:65 `if (name.length === 0) continue;`
        assert!(result.paths.is_empty());
        assert!(result.missing.is_empty());
    }
}
