#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Uninstall the local VibeMUD Claude Code plugin.

Usage:
  scripts/uninstall-claude-plugin.sh [--scope user|project|local] [--keep-data] [--keep-marketplace] [--keep-shims] [--debug]

Default scope: user

By default this removes:
  - vibemud@vibemud-local from the selected Claude Code scope
  - the local vibemud-local marketplace entry
  - VibeMUD shim files from ~/.local/share/vibemud/bin/

Game saves are preserved unless Claude removes plugin data for its own cache. VibeMUD's own save/state directory is not deleted by this script.
USAGE
}

scope="user"
keep_data="0"
keep_marketplace="0"
keep_shims="0"

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
    --keep-data)
      keep_data="1"
      shift
      ;;
    --keep-marketplace)
      keep_marketplace="1"
      shift
      ;;
    --keep-shims)
      keep_shims="1"
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
  echo "Claude Code CLI not found." >&2
  exit 127
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
marketplace_name="vibemud-local"
plugin_id="vibemud@${marketplace_name}"
shim_dir="${HOME}/.local/share/vibemud/bin"

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

stop_vibemud_runtime() {
  local vibemud_bin=""
  if [[ -x "${shim_dir}/vibemud" ]]; then
    vibemud_bin="${shim_dir}/vibemud"
  elif command -v vibemud >/dev/null 2>&1; then
    vibemud_bin="$(command -v vibemud)"
  fi

  if [[ -n "$vibemud_bin" ]]; then
    echo "==> Stopping VibeMUD runtime/HUD helpers if running"
    "$vibemud_bin" session stop >/dev/null 2>&1 || true
  else
    echo "==> VibeMUD binary not found; skipping runtime stop"
  fi
}

stop_vibemud_runtime

if plugin_installed; then
  args=("plugin" "uninstall" "$plugin_id" "--scope" "$scope")
  if [[ "$keep_data" == "1" ]]; then
    args+=("--keep-data")
  fi
  echo "==> Uninstalling plugin: ${plugin_id} (${scope} scope)"
  claude "${args[@]}"
else
  echo "==> Plugin is not installed in ${scope} scope: ${plugin_id}"
fi

if [[ "$keep_marketplace" != "1" ]]; then
  if marketplace_exists; then
    echo "==> Removing marketplace: ${marketplace_name}"
    claude plugin marketplace remove "$marketplace_name" || true
  else
    echo "==> Marketplace is not configured: ${marketplace_name}"
  fi
fi

if [[ "$keep_shims" != "1" ]]; then
  for bin in vibemud mudctl vibemud-hud vibemud-runtime; do
    rm -f "${shim_dir}/${bin}"
  done
  rmdir "$shim_dir" 2>/dev/null || true
  rmdir "$(dirname "$shim_dir")" 2>/dev/null || true
  echo "==> Removed VibeMUD shim files from ${shim_dir}"
fi

cat <<DONE

VibeMUD Claude Code plugin uninstall complete.
DONE
