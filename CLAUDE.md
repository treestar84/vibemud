# Claude Code instructions for VibeMUD

VibeMUD is a Rust-based local idle MUD/RPG that runs beside Claude Code as a sidecar game. Keep coding context and game state separate.

## Current support boundary

- Default integration: Claude Code 2.1.287+ mod pane through `/vibemud:mud ...` and immediate `/mud ...` (unless another plugin owns the short name).
- Native CLI and plugin source are 0.2.0, unreleased. Use the source installation until a matching release is published; do not claim this is npm latest.
- Node.js 18+ is required. Windows defaults to shell-free native executable calls, without Bash, tmux, WSL or wt.exe.
- macOS Claude Code 2.1.288 has an interactive smoke covering start, tables, hide, reopen, and stop. Windows/Linux native CI tests are available through manual workflow dispatch; Windows real-host/IME and Desktop real-app smoke remain unverified.
- Existing tmux, cmux and macOS Ghostty panes remain explicit `legacy` compatibility options. Retire them only after stabilization evidence.
- Codex, iTerm2 and `~mud` routing remain unsupported.

Quick start: `/mud start` (or `/vibemud:mud start` if `/mud` is taken), then `c|i|m|q|set`. `close` hides the pane while the game continues; menus have a separate Back to Adventure action. `end` stops the shared game runtime. `/vibemud:mud legacy start` selects the previous external pane.

Preserve native/root npm executable-bit repair and package checks.

## Product and privacy contract

- Never use source files, prompts, transcripts, editor buffers, commit text, or agent conversation content as game state.
- Store game state only in the configured VibeMUD data directory and SQLite DB.
- Game mutations should go through the runtime command queue unless a command is documented as read-only or immediate.
- Plugin hooks should intercept VibeMUD slash commands before they enter Claude's main prompt.
- Mod commands return `{}` and show results/errors only in the pane; never inject game state into model context. Classic hooks fail closed when the mod is unavailable.

## User experience rules

- Use the Claude-owned pane with tabs/buttons/Input; preserve coding focus and drafts across refreshes.
- Reuse Rust rules, sprites, story and persistent state. Keep game changes in the command queue; connection/settings/migration/confirmed reset are documented immediate operations.
- Snapshot queries must be read-only and consistent. Do not migrate/seed on every UI poll.
- Preserve idempotency keys across transport failures. A reset changes save generation and backs up the database first.
- Track fever by session/turn identifiers only; do not inspect turn text. Lease expiry stops only mod-owned runtimes.
- Keep expedition earnings tied to real, transactionally recorded combat rewards. Purchases, sales, and level spending must not distort totals. Keep Korean/CJK terminal columns aligned and show full names in detail views.
- Keep legacy pane identity checks and avoid closing unrelated terminal surfaces.

## Game balance contract

- Every monster reachable from `area_monsters` or `dungeon_monsters` must have a matching `monster_equipment_drops` row for its `monster_grade`.
- Every equipment drop rule must resolve to seeded candidates across all eight equipment slots.
- Rarity progression should remain gradual: early common/uncommon, deep-delves rare, cursed/atlas epic introduction, titan epic stabilization, olympus/titan-vault legendary visibility.
- Legendary equipment should stay out of pre-final field/dungeon progression unless a deliberate balance change updates tests and documentation.
- Keep DB seed coverage tests and runtime deterministic drop simulations aligned with any new area, dungeon, monster, rarity, or tier changes.

## Repository surface map

Public tracked source should stay minimal:

- Root policy/docs: `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `LICENSE`, `AGENTS.md`, `CLAUDE.md`.
- Rust workspace: `Cargo.toml`, `Cargo.lock`, `crates/vibemud-*`.
- Claude plugin/marketplace source: `.claude-plugin/`, `claude-marketplace/`.
- npm package source: `npm/`.
- Release/validation automation: `.github/workflows/`.
- The six-target package dry-run workflow is manual-only; do not trigger it on ordinary pushes or PRs. Its uploaded artifacts expire after three days.
- Supported installers only: `scripts/install.sh`, `scripts/install.ps1`, `scripts/install-claude-plugin.sh`, `scripts/install-claude-plugin.ps1`, `scripts/uninstall-claude-plugin.sh`.

Local-only or generated folders/files are intentionally ignored and should not be added to Git:

- Agent/orchestration state: `.omx/`, `.omc/`, `.claude/`, `.agents/`, `.codex/`, `_bmad/`, `codex/`, `tasks/`, `skills-lock.json`.
- Build/package output: `target/`, `dist/`, `artifacts/`, `coverage/`, `*.tgz`.
- Runtime data: `vibemud.db*`, `config.toml`, `panel-pane`, `logs/`, `backups/`.
- Private planning/reference docs: `docs/`.
- Old external-pane media and preview assets: `assets/`, `preview-assets/`.
- Local unsupported helper scripts such as Codex hook prototypes must remain untracked unless the support boundary changes.

If public guidance is needed, update `README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `AGENTS.md`, or this file instead of adding new public docs.

## Development rules

- Prefer Rust core changes for behavior that must work across platforms.
- Keep shell, PowerShell, npm, and plugin changes small and syntax-checkable.
- Avoid new dependencies unless necessary for the shipped CLI/plugin and reflected in the relevant manifest.
- Do not reintroduce Codex or iTerm2 support claims without new smoke evidence and explicit maintainer approval.
- If a platform cannot be smoke-tested locally, state that gap instead of claiming support.

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
python3 -m py_compile claude-marketplace/plugins/vibemud/scripts/vibemud-context-hook.py
node --check claude-marketplace/plugins/vibemud/scripts/vibemud-context-hook.js
bash -n scripts/install.sh scripts/install-claude-plugin.sh claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh
```

When PowerShell files change (`scripts/install.ps1`, `scripts/install-claude-plugin.ps1`, `claude-marketplace/plugins/vibemud/scripts/vibemud-claude.ps1`) and `pwsh` is available, also run a PowerShell parser syntax check on each changed `.ps1` file.

For npm/package changes, also run:

```bash
node npm/scripts/check-release-metadata.js
(cd npm && npm run test:resolve)
(cd npm && npm pack --dry-run --json | node scripts/check-pack-contents.js)
```
