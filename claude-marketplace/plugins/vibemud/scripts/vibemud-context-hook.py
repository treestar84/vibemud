#!/usr/bin/env python3
"""Context firewall for VibeMUD Claude Code slash commands.

The hook intercepts `/vibemud:mud ...` / `/mud ...` before the prompt reaches
Claude, executes the local VibeMUD dispatcher with stdout/stderr captured, and
blocks the prompt expansion/submission so game commands and game output do not
enter the main coding context.
"""
from __future__ import annotations

import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

VIBEMUD_COMMAND_RE = re.compile(r"^/(?:vibemud:)?mud(?:\s+(.*))?$", re.IGNORECASE | re.DOTALL)
DANGEROUS_LIVE_COMMANDS = {"watch", "보기", "hud", "화면", "broadcast", "브로드캐스트", "방송", "live", "라이브", "preview", "프리뷰"}
ACTIVE_CODING_EVENTS = {"UserPromptSubmit", "PreToolUse", "PostToolUse"}
STOP_CODING_EVENTS = {"Stop"}
SESSION_ACTIVITY_TTL_SECONDS = 120


def plugin_root() -> Path:
    env_root = os.environ.get("CLAUDE_PLUGIN_ROOT")
    if env_root:
        return Path(env_root)
    return Path(__file__).resolve().parents[1]


def home_dir() -> Path:
    return Path(os.environ.get("VIBEMUD_HOME", str(Path.home() / ".vibemud"))).expanduser()


def ensure_private_dir(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    try:
        path.chmod(0o700)
    except OSError:
        pass
    return path


def write_private_text(path: Path, text: str) -> None:
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(text)
    try:
        path.chmod(0o600)
    except OSError:
        pass


def append_private_text(path: Path, text: str) -> None:
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    with os.fdopen(fd, "a", encoding="utf-8") as f:
        f.write(text)
    try:
        path.chmod(0o600)
    except OSError:
        pass


def log_dir() -> Path:
    return ensure_private_dir(home_dir() / "logs")


def activity_path() -> Path:
    return ensure_private_dir(home_dir()) / "vibe-activity.json"


def parse_timestamp(value: Any) -> datetime | None:
    if not isinstance(value, str) or not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def safe_session_value(prefix: str, value: str) -> str:
    raw = f"{prefix}:{value}"
    return re.sub(r"[^A-Za-z0-9_.:-]", "_", raw)[:160]


def session_key(event: dict[str, Any]) -> str:
    for key in ("session_id", "sessionId", "conversation_id", "conversationId"):
        value = event.get(key)
        if isinstance(value, str) and value.strip():
            return safe_session_value(key, value.strip())
    transcript = event.get("transcript_path") or event.get("transcriptPath")
    if isinstance(transcript, str) and transcript.strip():
        return "transcript-" + hashlib.sha256(transcript.encode("utf-8")).hexdigest()[:24]
    for name in (
        "CLAUDE_SESSION_ID",
        "CLAUDECODE_SESSION_ID",
        "CLAUDE_PROJECT_DIR",
        "TMUX_PANE",
        "CMUX_TAB_ID",
        "CMUX_WORKSPACE_ID",
    ):
        value = os.environ.get(name)
        if value:
            return safe_session_value(name, value)
    return f"pid:{os.getppid()}"


def write_vibe_activity(active: bool, source: str = "claude", event: dict[str, Any] | None = None) -> None:
    path = activity_path()
    existing: dict[str, Any] = {}
    try:
        existing_raw = path.read_text(encoding="utf-8")
        parsed = json.loads(existing_raw)
        if isinstance(parsed, dict):
            existing = parsed
    except (OSError, json.JSONDecodeError):
        existing = {}

    now = datetime.now(timezone.utc)
    now_text = now.isoformat().replace("+00:00", "Z")
    raw_sessions = existing.get("sessions")
    sessions = raw_sessions if isinstance(raw_sessions, dict) else {}
    session_id = session_key(event or {})
    sessions[session_id] = {
        "active": active,
        "source": source,
        "updated_at": now_text,
    }

    fresh_sessions: dict[str, Any] = {}
    aggregate_active = False
    aggregate_source = source
    for key, value in sessions.items():
        if not isinstance(value, dict):
            continue
        updated_at = parse_timestamp(value.get("updated_at"))
        if updated_at is None or (now - updated_at).total_seconds() > SESSION_ACTIVITY_TTL_SECONDS:
            continue
        fresh_sessions[str(key)] = value
        if value.get("active") is True:
            aggregate_active = True
            if isinstance(value.get("source"), str):
                aggregate_source = str(value["source"])

    payload = {
        "active": aggregate_active,
        "source": aggregate_source,
        "updated_at": now_text,
        "sessions": fresh_sessions,
    }
    reward_until = existing.get("reward_until")
    if isinstance(reward_until, str) and reward_until:
        payload["reward_until"] = reward_until

    tmp_path = path.with_name(f"{path.name}.{os.getpid()}.tmp")
    write_private_text(tmp_path, json.dumps(payload, ensure_ascii=False, indent=2))
    os.replace(tmp_path, path)
    try:
        path.chmod(0o600)
    except OSError:
        pass


def read_event() -> dict[str, Any]:
    raw = sys.stdin.read()
    if not raw.strip():
        return {}
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return {}


def extract_vibemud_args(event: dict[str, Any]) -> tuple[bool, list[str], str]:
    event_name = event.get("hook_event_name", "")

    if event_name == "UserPromptExpansion":
        command_name = str(event.get("command_name") or "")
        command_args = str(event.get("command_args") or "")
        prompt = str(event.get("prompt") or "")
        normalized = command_name.lstrip("/").lower()
        if normalized in {"mud", "vibemud:mud"}:
            return True, split_args(command_args), f"/{normalized} {command_args}".strip()
        match = VIBEMUD_COMMAND_RE.match(prompt.strip())
        if match:
            return True, split_args(match.group(1) or ""), prompt.strip()
        return False, [], ""

    if event_name == "UserPromptSubmit":
        prompt = str(event.get("prompt") or "").strip()
        match = VIBEMUD_COMMAND_RE.match(prompt)
        if match:
            return True, split_args(match.group(1) or ""), prompt
        return False, [], ""

    return False, [], ""


def split_args(arg_text: str) -> list[str]:
    if not arg_text.strip():
        return []
    try:
        return shlex.split(arg_text)
    except ValueError:
        # Fall back to whitespace splitting rather than letting malformed quotes
        # reach Claude as ordinary prompt context.
        return arg_text.split()


def should_soft_deny(args: list[str]) -> str | None:
    if not args:
        return None
    first = args[0]
    if first in DANGEROUS_LIVE_COMMANDS and "--unsafe-context" not in args:
        return "live-output 명령은 컨텍스트 보호를 위해 차단했습니다. HUD pane은 /vibemud:mud panel을 사용하세요."
    return None


def run_dispatcher(args: list[str]) -> tuple[int, str, str]:
    root = plugin_root()
    dispatcher = root / "scripts" / "vibemud-claude.sh"
    if not dispatcher.exists():
        return 127, f"dispatcher not found: {dispatcher}", ""

    env = os.environ.copy()
    env.setdefault("VIBEMUD_CONTEXT_MODE", "hook")
    env.setdefault("VIBEMUD_CONTEXT_HOOK", "1")

    command = ["bash", str(dispatcher), *args]
    proc = subprocess.run(
        command,
        env=env,
        cwd=str(Path.cwd()),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=15,
        check=False,
    )
    command_log = log_dir() / "claude-hook-commands.log"
    log_text = f"$ {' '.join(shlex.quote(part) for part in command)}\n"
    if proc.stdout:
        log_text += proc.stdout
        if not proc.stdout.endswith("\n"):
            log_text += "\n"
    log_text += f"exit={proc.returncode}\n\n"
    append_private_text(command_log, log_text)
    safe_summary = safe_dispatcher_summary(args, proc.stdout)
    return proc.returncode, str(command_log), safe_summary


def is_help_args(args: list[str]) -> bool:
    if not args:
        return True
    return args[0] in {"help", "--help", "-h", "guide", "가이드", "도움말"}


def safe_dispatcher_summary(args: list[str], stdout: str) -> str:
    if is_help_args(args):
        return safe_help_summary(stdout)

    lines: list[str] = []
    for raw in stdout.splitlines():
        line = raw.strip()
        if not line.startswith("[VibeMUD]"):
            continue
        if len(line) > 240:
            line = line[:237] + "..."
        lines.append(line)
        if len(lines) >= 3:
            break
    return "\n".join(lines)


def safe_help_summary(stdout: str) -> str:
    # Help is static command documentation, not game state/log output, so it is
    # safe and useful to show it directly in the slash-command result.
    text = stdout.strip()
    if len(text) > 12_000:
        return text[:11_997] + "..."
    return text


def block(reason: str) -> None:
    # reason is shown to the user by Claude Code and is not added as prompt
    # context for UserPromptSubmit/UserPromptExpansion decision control.
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))


def main() -> int:
    event = read_event()
    matched, args, original = extract_vibemud_args(event)
    event_name = str(event.get("hook_event_name") or "")
    if not matched:
        if os.environ.get("VIBEMUD_MOD_ACTIVE") == "1":
            return 0
        if event_name in ACTIVE_CODING_EVENTS:
            write_vibe_activity(True, event=event)
        elif event_name in STOP_CODING_EVENTS:
            write_vibe_activity(False, event=event)
        return 0

    if not args or args[0] != "legacy":
        block("VibeMUD mod를 사용할 수 없습니다. Claude Code 2.1.287+에서 플러그인을 다시 로드하세요. 외부 창 호환: /vibemud:mud legacy start")
        return 0
    args = args[1:]
    soft_deny = should_soft_deny(args)
    if soft_deny:
        block(f"VibeMUD: {soft_deny}")
        return 0

    try:
        code, detail, summary = run_dispatcher(args)
    except subprocess.TimeoutExpired:
        block("VibeMUD 명령이 제한 시간 내 끝나지 않아 중단했습니다. HUD pane 상태를 확인하세요.")
        return 0
    except Exception as exc:  # pragma: no cover - defensive hook boundary
        block(f"VibeMUD 명령 처리 실패: {exc}")
        return 0

    command_text = original or "/vibemud:mud"
    if code == 0:
        extra = f"\n{summary}" if summary else ""
        if is_help_args(args):
            block(f"VibeMUD 조작 가이드: {command_text}{extra}")
        else:
            block(f"VibeMUD 명령 처리 완료: {command_text}{extra}\n게임 출력은 메인 컨텍스트에 넣지 않고 HUD pane/로컬 DB에만 반영했습니다.")
    else:
        extra = f"\n{summary}" if summary else ""
        block(f"VibeMUD 명령 처리 실패(exit {code}).{extra}\n세부 로그: {detail}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
