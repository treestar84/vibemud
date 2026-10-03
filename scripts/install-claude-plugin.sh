#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Install the local VibeMUD Claude Code plugin.

Recommended easier entry point:
  scripts/install.sh --for claude --scope local

Advanced direct usage:
  scripts/install-claude-plugin.sh [--scope user|project|local] [--debug] [--skip-build]

Default scope: user

What this does:
  1. Builds VibeMUD release binaries unless --skip-build is set.
  2. Creates stable shims in ~/.local/share/vibemud/bin/ for Claude plugin wrappers.
  3. Validates claude-marketplace/ with `claude plugin validate`.
  4. Adds/updates the local `vibemud-local` marketplace.
  5. Installs/enables `vibemud@vibemud-local` in the selected scope.
USAGE
}

scope="user"
skip_build="0"

while [[ $# -gt 0 ]]; do
  case "$1" in
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

case "$scope" in
  user|project|local) ;;
  *) echo "Invalid --scope: $scope (expected user, project, or local)" >&2; exit 2 ;;
esac

if ! command -v claude >/dev/null 2>&1; then
  echo "Claude Code CLI not found: install Claude Code before installing the plugin." >&2
  exit 127
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
marketplace_dir="${repo_root}/claude-marketplace"
marketplace_name="vibemud-local"
plugin_id="vibemud@${marketplace_name}"
shim_dir="${HOME}/.local/share/vibemud/bin"

if [[ ! -d "$marketplace_dir" ]]; then
  echo "Marketplace directory not found: $marketplace_dir" >&2
  exit 1
fi

if [[ "$skip_build" != "1" ]]; then
  echo "==> Building VibeMUD release binaries"
  (cd "$repo_root" && cargo build --workspace --release)
fi

mkdir -p "$shim_dir"
for bin in vibemud mudctl vibemud-hud vibemud-runtime; do
  target="${repo_root}/target/release/${bin}"
  if [[ ! -x "$target" ]]; then
    echo "Missing built binary: $target" >&2
    echo "Run without --skip-build, or build with: cargo build --workspace --release" >&2
    exit 1
  fi
  cat > "${shim_dir}/${bin}" <<SHIM
#!/usr/bin/env bash
exec "${target}" "\$@"
SHIM
  chmod +x "${shim_dir}/${bin}"
done

printf '%s\n' "${repo_root}/target/release" > "${shim_dir}/native-dir.txt"

echo "==> Validating Claude marketplace/plugin"
claude plugin validate "$marketplace_dir"

marketplace_exists() {
  claude plugin marketplace list --json | python3 -c '
import json, sys
name = sys.argv[1]
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)
sys.exit(0 if any(item.get("name") == name for item in data) else 1)
' "$marketplace_name"
}

plugin_installed() {
  claude plugin list --json | python3 -c '
import json, sys
plugin_id, scope, repo_root = sys.argv[1:4]
try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)
installed = data.get("installed", []) if isinstance(data, dict) else data
for item in installed:
    if item.get("id") != plugin_id:
        continue
    if item.get("scope") != scope:
        continue
    if scope in {"project", "local"} and item.get("projectPath") not in {None, repo_root}:
        continue
    sys.exit(0)
sys.exit(1)
' "$plugin_id" "$scope" "$repo_root"
}

if marketplace_exists; then
  echo "==> Updating existing marketplace: ${marketplace_name}"
  claude plugin marketplace update "$marketplace_name" || true
else
  echo "==> Adding marketplace: ${marketplace_dir}"
  claude plugin marketplace add "$marketplace_dir" --scope "$scope"
fi

if plugin_installed; then
  echo "==> Refreshing installed plugin in ${scope} scope"
  # Claude skips cache refresh when a local marketplace keeps the same plugin version.
  # Reinstall with data preserved so local development changes are actually applied.
  claude plugin uninstall "$plugin_id" --scope "$scope" --keep-data
  claude plugin install "$plugin_id" --scope "$scope"
else
  echo "==> Installing plugin: ${plugin_id} (${scope} scope)"
  claude plugin install "$plugin_id" --scope "$scope"
fi

cat <<DONE

VibeMUD Claude Code plugin installed.

Restart Claude Code, then use the Claude quick start:
  /vibemud:mud start
  /vibemud:mud c
  /vibemud:mud i
  /vibemud:mud m
  /vibemud:mud q
  /vibemud:mud end

Optional guide/helpers:
  /vibemud:mud guide
  /vibemud:mud next
  /vibemud:mud log
  /vibemud:mud stats
  /vibemud:mud stats close
  /vibemud:mud panel
  /vibemud:mud broadcast
  /vibemud:mud watch

Stable binary shims:
  ${shim_dir}

To uninstall:
  scripts/uninstall-claude-plugin.sh --scope ${scope}
DONE
