//! har-adapters — Multi-surface platform adapters.
//!
//! Ports Archon `packages/adapters/src/*`:
//!   - `chat/slack/`              → `SlackAdapter: IPlatformAdapter` (slack-morphism or reqwest)
//!   - `chat/telegram/`           → `TelegramAdapter` (teloxide or reqwest)
//!   - `community/chat/discord/`  → `DiscordAdapter` (serenity/twilight or reqwest)
//!   - `forge/github/`            → `GitHubAdapter` (octocrab)
//!   - `community/forge/gitea/`   → `GiteaAdapter` (reqwest)
//!   - `community/forge/gitlab/`  → `GitLabAdapter` (reqwest)
//!   - `utils/message-splitting.ts` → `split_message()` (platform message length limits)
//!
//! Each adapter implements `IPlatformAdapter` (the `IWorkflowPlatform` surface in har-dag-executor).
//!
//! Status: STUB — not yet ported. Will be filled in ITERATE cycle 14.
