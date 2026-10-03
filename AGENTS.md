# Agent instructions for VibeMUD

## Scope

These instructions apply to the whole repository.

VibeMUD is a Rust-based local idle RPG that runs beside Claude Code through a Claude Code plugin. The public repository is intentionally minimal: source code, npm packaging, Claude plugin files, GitHub automation, OSS policy files, README, and this agent guidance only.

## Product contract

- Primary integration: Claude Code 2.1.287+ mod pane via `/vibemud:mud ...` and immediate `/mud ...`. Native CLI and plugin must be upgraded together.
- Keep tmux, cmux, and macOS Ghostty as explicit `legacy` compatibility options during stabilization. Historical external-pane smoke does not verify the new mod or Windows.
- Unsupported targets: Codex and iTerm2. Do not reintroduce Codex `$mud`, Codex hook installers, or iTerm2 support claims without fresh smoke evidence and an explicit maintainer decision.
- Preserve the privacy boundary: never use source files, prompts, transcripts, editor buffers, or agent conversation content as game state.
- Game state belongs only in the configured VibeMUD data directory and SQLite DB, not in project files.
- Game mutations should go through the runtime command queue unless a command is documented as read-only or immediate.
- Keep menu Back, pane Hide, and Game Stop distinct. Use bounded, CJK-aware columns for item, shop, map, quest, and party views; show full names on selection.
- Expedition earnings must reflect committed combat rewards, not wallet or XP balance changes. Use turn lifecycle metadata only, never turn text or files.
- Do not add `~mud` routing. Supported Claude command surfaces are `/mud` and `/vibemud:mud` only.

## Repository hygiene

- Keep the public GitHub tree minimal. Do not add `docs/`, PRD drafts, screenshots, local agent folders, runtime DBs, logs, generated package artifacts, or local terminal helper scripts to Git.
- Do not add old external-pane demo assets, private reference notes, or stale issue/PR templates to the public repository. Keep only GitHub validation/release workflows.
- Keep the six-target package dry-run workflow manual-only; ordinary pushes and PRs must not start it automatically. Keep uploaded artifacts short-lived.
- Root `AGENTS.md` and `CLAUDE.md` are intentional public guidance files and should stay at repository root.
- Prefer updating `README.md`, `CONTRIBUTING.md`, or `SECURITY.md` over adding new public docs.
- Avoid new dependencies unless necessary for the shipped CLI/plugin and documented in the relevant manifest.

## Development rules

- Prefer Rust core changes for behavior that must work across platforms.
- Keep shell/PowerShell/plugin changes small and syntax-checkable.
- Keep coding/game UI separated; side panes should not occupy the main Claude Code pane when a supported pane target is available.
- If a platform cannot be smoke-tested locally, state that gap explicitly instead of claiming support.

## Verification

For CLI/plugin changes, run the smallest relevant checks and report results:

```bash
cargo fmt --check
cargo test -p vibemud-db
cargo test -p vibemud-runtime
cargo test -p vibemud-cli
cargo build --workspace
node --test npm/test/bridge.test.js
claude plugin validate ./claude-marketplace/plugins/vibemud --strict
claude plugin test ./claude-marketplace/plugins/vibemud
node --check claude-marketplace/plugins/vibemud/scripts/vibemud-context-hook.js
python3 -m py_compile claude-marketplace/plugins/vibemud/scripts/vibemud-context-hook.py
bash -n scripts/install.sh scripts/install-claude-plugin.sh claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh
```

For npm/package changes, also run:

```bash
node npm/scripts/check-release-metadata.js
(cd npm && npm run test:resolve)
(cd npm && npm pack --dry-run --json | node scripts/check-pack-contents.js)
```
