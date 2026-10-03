#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install VibeMUD with a small Claude/CLI-oriented workflow.

Usage:
  scripts/install.sh [--for cli|claude|all] [--scope user|project|local] [--skip-build] [--setup] [--debug]

Recommended:
  scripts/install.sh --for claude --scope local   # Claude Code slash command + CLI shims
  scripts/install.sh --for all --scope local      # CLI shims and Claude plugin when available

Defaults:
  --for cli
  --scope local

What this does:
  1. Builds release binaries unless --skip-build is set.
  2. Installs stable CLI shims into ~/.local/share/vibemud/bin.
  3. For Claude, installs/enables the local Claude Code plugin.
  4. With --setup, runs `vibemud setup` for language/terminal/HUD preferences.

Setup options passed to `vibemud setup`:
  --language ko|en
  --agent auto|claude|cli
  --terminal auto|tmux|ghostty|windows-terminal|plain
  --storage user|project
  --yes

Storage defaults to the user data directory unless --storage project is selected.
HUD mode and popup pane behavior keep safe defaults during first-time setup.
Change them later with `vibemud config set ui.hud_mode ...` or
`vibemud config set ui.popup_pane_enabled ...` if needed.
USAGE
}

target="cli"
scope="local"
skip_build="0"
run_setup="0"
setup_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --for|--target)
      target="${2:-}"
      shift 2
      ;;
    --for=*|--target=*)
      target="${1#*=}"
      shift
      ;;
    --scope)
      scope="${2:-}"
      shift 2
      ;;
    --scope=*)
      scope="${1#--scope=}"
      shift
      ;;
    --skip-build)
      skip_build="1"
      shift
      ;;
    --setup)
      run_setup="1"
      shift
      ;;
    --language|--agent|--terminal|--storage)
      run_setup="1"
      setup_args+=("$1" "${2:-}")
      shift 2
      ;;
    --language=*|--agent=*|--terminal=*|--storage=*)
      run_setup="1"
      setup_args+=("$1")
      shift
      ;;
    --yes)
      run_setup="1"
      setup_args+=("$1")
      shift
      ;;
    --debug)
      set -x
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$target" in
  cli|claude|all) ;;
  *) echo "Invalid --for: $target (expected cli, claude, or all)" >&2; exit 2 ;;
esac

case "$scope" in
  user|project|local) ;;
  *) echo "Invalid --scope: $scope (expected user, project, or local)" >&2; exit 2 ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
shim_dir="${HOME}/.local/share/vibemud/bin"

build_release() {
  if [[ "$skip_build" == "1" ]]; then
    return 0
  fi
  echo "==> Building VibeMUD release binaries"
  (cd "$repo_root" && cargo build --workspace --release)
}

install_shims() {
  echo "==> Installing CLI shims: ${shim_dir}"
  mkdir -p "$shim_dir"
  for bin in vibemud mudctl vibemud-hud vibemud-runtime; do
    local target_path="${repo_root}/target/release/${bin}"
    if [[ ! -x "$target_path" ]]; then
      echo "Missing built binary: $target_path" >&2
      echo "Run without --skip-build, or build with: cargo build --workspace --release" >&2
      exit 1
    fi
    cat > "${shim_dir}/${bin}" <<SHIM
#!/usr/bin/env bash
exec "${target_path}" "\$@"
SHIM
    chmod +x "${shim_dir}/${bin}"
  done
  printf '%s\n' "${repo_root}/target/release" > "${shim_dir}/native-dir.txt"
}

run_first_time_setup() {
  if [[ "$run_setup" != "1" ]]; then
    return 0
  fi
  echo "==> Running first-time setup"
  "${shim_dir}/vibemud" setup "${setup_args[@]}"
}

path_hint() {
  case ":${PATH}:" in
    *":${shim_dir}:"*) return 0 ;;
    *)
      cat <<HINT

PATH note:
  Add this to your shell profile if 'vibemud' is not found:
    export PATH="${shim_dir}:\$PATH"
HINT
      ;;
  esac
}


print_cli_quick_start() {
  cat <<'CLI'

CLI quick start:
  vibemud init
  vibemud session start --background
  mudctl hunt start --area forest-edge --auto-start
  vibemud hud --panel --refresh 1 --log-lines 12
CLI
}

print_claude_quick_start() {
  cat <<'CLAUDE'

Claude Code quick start:
  1. Restart Claude Code after plugin installation.
  2. Run game commands from Claude Code:
       /vibemud:mud start
       /vibemud:mud c
       /vibemud:mud i
       /vibemud:mud m
       /vibemud:mud q
       /vibemud:mud end

  If the short slash command is exposed, /mud works too.
CLAUDE
}

build_release
install_shims
run_first_time_setup

case "$target" in
  claude|all)
    if command -v claude >/dev/null 2>&1; then
      echo "==> Installing Claude Code plugin (${scope} scope)"
      "${repo_root}/scripts/install-claude-plugin.sh" --scope "$scope" --skip-build
      print_claude_quick_start
    else
      cat <<'NOCLAUDE'

Claude Code CLI was not found, so the Claude plugin was not installed.
Install Claude Code first, then run:
  scripts/install.sh --for claude --scope local
NOCLAUDE
    fi
    ;;
esac

case "$target" in
  cli) print_cli_quick_start ;;
esac

path_hint

if [[ "$run_setup" != "1" ]]; then
  cat <<'SETUP_HINT'

First-time setup:
  Run `vibemud setup` to choose language, Claude/CLI mode, terminal, and storage location.
  HUD/popup behavior uses safe defaults and remains configurable later.
SETUP_HINT
fi

cat <<DONE

VibeMUD install complete.
Installed shims:
  ${shim_dir}
DONE
