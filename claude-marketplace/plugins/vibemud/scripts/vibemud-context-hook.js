#!/usr/bin/env node
// Context firewall for VibeMUD Claude Code slash commands.
//
// The hook intercepts `/vibemud:mud ...` / `/mud ...` before the prompt reaches
// Claude, executes the local VibeMUD dispatcher with stdout/stderr captured, and
// blocks the prompt expansion/submission so game commands and game output do not
// enter the main coding context.
//
// This is the cross-platform (Node.js) port of vibemud-context-hook.py.
// On POSIX it delegates to scripts/vibemud-claude.sh (bash), on native Windows
// to scripts/vibemud-claude.ps1 (PowerShell).
'use strict';

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const VIBEMUD_COMMAND_RE = /^\/(?:vibemud:)?mud(?:\s+([\s\S]*))?$/i;
const DANGEROUS_LIVE_COMMANDS = new Set([
  'watch', '보기', 'hud', '화면', 'broadcast', '브로드캐스트', '방송',
  'live', '라이브', 'preview', '프리뷰',
]);
const ACTIVE_CODING_EVENTS = new Set(['UserPromptSubmit', 'PreToolUse', 'PostToolUse']);
const STOP_CODING_EVENTS = new Set(['Stop']);
const SESSION_ACTIVITY_TTL_SECONDS = 120;
const DISPATCHER_TIMEOUT_MS = 15000;

function pluginRoot() {
  const envRoot = process.env.CLAUDE_PLUGIN_ROOT;
  if (envRoot) return envRoot;
  return path.resolve(__dirname, '..');
}

// Mirrors AppPaths::discover in crates/vibemud-db so the activity file and hook
// logs land where the runtime actually reads them (project storage marker and
// %LOCALAPPDATA%\VibeMUD on Windows included).
function projectVibemudRoot() {
  let dir = process.cwd();
  for (;;) {
    const root = path.join(dir, '.vibemud');
    if (fs.existsSync(path.join(root, '.vibemud-project'))) return root;
    const parent = path.dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

function homeDir() {
  if (process.env.VIBEMUD_HOME) return process.env.VIBEMUD_HOME;
  const projectRoot = projectVibemudRoot();
  if (projectRoot) return projectRoot;
  if (process.platform === 'win32') {
    if (process.env.LOCALAPPDATA) return path.join(process.env.LOCALAPPDATA, 'VibeMUD');
    if (process.env.USERPROFILE) return path.join(process.env.USERPROFILE, '.vibemud');
  }
  return path.join(os.homedir(), '.vibemud');
}

function ensurePrivateDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
  try {
    fs.chmodSync(dirPath, 0o700);
  } catch (_) {
    // Best effort; chmod is a no-op on Windows.
  }
  return dirPath;
}

function writePrivateText(filePath, text) {
  fs.writeFileSync(filePath, text, { encoding: 'utf8', mode: 0o600 });
  try {
    fs.chmodSync(filePath, 0o600);
  } catch (_) {}
}

function appendPrivateText(filePath, text) {
  fs.appendFileSync(filePath, text, { encoding: 'utf8', mode: 0o600 });
  try {
    fs.chmodSync(filePath, 0o600);
  } catch (_) {}
}

function logDir() {
  return ensurePrivateDir(path.join(homeDir(), 'logs'));
}

function activityPath() {
  return path.join(ensurePrivateDir(homeDir()), 'vibe-activity.json');
}

function parseTimestamp(value) {
  if (typeof value !== 'string' || !value) return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function safeSessionValue(prefix, value) {
  const raw = `${prefix}:${value}`;
  return raw.replace(/[^A-Za-z0-9_.:-]/g, '_').slice(0, 160);
}

function sessionKey(event) {
  for (const key of ['session_id', 'sessionId', 'conversation_id', 'conversationId']) {
    const value = event[key];
    if (typeof value === 'string' && value.trim()) {
      return safeSessionValue(key, value.trim());
    }
  }
  const transcript = event.transcript_path || event.transcriptPath;
  if (typeof transcript === 'string' && transcript.trim()) {
    return 'transcript-' + crypto.createHash('sha256').update(transcript, 'utf8').digest('hex').slice(0, 24);
  }
  for (const name of [
    'CLAUDE_SESSION_ID',
    'CLAUDECODE_SESSION_ID',
    'CLAUDE_PROJECT_DIR',
    'TMUX_PANE',
    'CMUX_TAB_ID',
    'CMUX_WORKSPACE_ID',
  ]) {
    const value = process.env[name];
    if (value) return safeSessionValue(name, value);
  }
  return `pid:${process.ppid}`;
}

function writeVibeActivity(active, source = 'claude', event = null) {
  const filePath = activityPath();
  let existing = {};
  try {
    const parsed = JSON.parse(fs.readFileSync(filePath, 'utf8'));
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      existing = parsed;
    }
  } catch (_) {
    existing = {};
  }

  const now = Date.now();
  const nowText = new Date(now).toISOString().replace(/\.\d{3}Z$/, 'Z');
  const rawSessions = existing.sessions;
  const sessions = rawSessions && typeof rawSessions === 'object' && !Array.isArray(rawSessions)
    ? rawSessions
    : {};
  const sessionId = sessionKey(event || {});
  sessions[sessionId] = {
    active,
    source,
    updated_at: nowText,
  };

  const freshSessions = {};
  let aggregateActive = false;
  let aggregateSource = source;
  for (const [key, value] of Object.entries(sessions)) {
    if (!value || typeof value !== 'object') continue;
    const updatedAt = parseTimestamp(value.updated_at);
    if (updatedAt === null || (now - updatedAt) / 1000 > SESSION_ACTIVITY_TTL_SECONDS) {
      continue;
    }
    freshSessions[String(key)] = value;
    if (value.active === true) {
      aggregateActive = true;
      if (typeof value.source === 'string') {
        aggregateSource = value.source;
      }
    }
  }

  const payload = {
    active: aggregateActive,
    source: aggregateSource,
    updated_at: nowText,
    sessions: freshSessions,
  };
  const rewardUntil = existing.reward_until;
  if (typeof rewardUntil === 'string' && rewardUntil) {
    payload.reward_until = rewardUntil;
  }

  const tmpPath = path.join(
    path.dirname(filePath),
    `${path.basename(filePath)}.${process.pid}.tmp`
  );
  const text = JSON.stringify(payload, null, 2);
  writePrivateText(tmpPath, text);
  try {
    fs.renameSync(tmpPath, filePath);
  } catch (_) {
    // Windows refuses rename-over-file while another process holds the target
    // open (EPERM/EACCES); fall back to a direct (non-atomic) write.
    writePrivateText(filePath, text);
    try {
      fs.unlinkSync(tmpPath);
    } catch (_) {}
  }
  try {
    fs.chmodSync(filePath, 0o600);
  } catch (_) {}
}

// Activity tracking is best-effort telemetry for the game runtime; it must
// never break an ordinary coding prompt on filesystem/permission errors.
function writeVibeActivitySafe(active, source, event) {
  try {
    writeVibeActivity(active, source, event);
  } catch (_) {}
}

function readEvent() {
  let raw = '';
  try {
    raw = fs.readFileSync(0, 'utf8');
  } catch (_) {
    return {};
  }
  if (!raw.trim()) return {};
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : {};
  } catch (_) {
    return {};
  }
}

// shlex.split-compatible enough for slash-command arguments: handles single
// quotes, double quotes, and backslash escapes outside single quotes.
function splitArgs(argText) {
  if (!argText || !argText.trim()) return [];
  const args = [];
  let current = '';
  let hasToken = false;
  let mode = 'plain';
  for (let i = 0; i < argText.length; i += 1) {
    const ch = argText[i];
    if (mode === 'plain') {
      if (/\s/.test(ch)) {
        if (hasToken) {
          args.push(current);
          current = '';
          hasToken = false;
        }
      } else if (ch === "'") {
        mode = 'single';
        hasToken = true;
      } else if (ch === '"') {
        mode = 'double';
        hasToken = true;
      } else if (ch === '\\') {
        if (i + 1 >= argText.length) return argText.trim().split(/\s+/);
        current += argText[i + 1];
        i += 1;
        hasToken = true;
      } else {
        current += ch;
        hasToken = true;
      }
    } else if (mode === 'single') {
      if (ch === "'") {
        mode = 'plain';
      } else {
        current += ch;
      }
    } else if (mode === 'double') {
      if (ch === '"') {
        mode = 'plain';
      } else if (ch === '\\' && i + 1 < argText.length && '"\\'.includes(argText[i + 1])) {
        current += argText[i + 1];
        i += 1;
      } else {
        current += ch;
      }
    }
  }
  if (mode !== 'plain') {
    // Malformed quoting falls back to whitespace splitting rather than letting
    // it reach Claude as ordinary prompt context.
    return argText.trim().split(/\s+/);
  }
  if (hasToken) args.push(current);
  return args;
}

function extractVibemudArgs(event) {
  const eventName = event.hook_event_name || '';

  if (eventName === 'UserPromptExpansion') {
    const commandName = String(event.command_name || '');
    const commandArgs = String(event.command_args || '');
    const prompt = String(event.prompt || '');
    const normalized = commandName.replace(/^\/+/, '').toLowerCase();
    if (normalized === 'mud' || normalized === 'vibemud:mud') {
      return {
        matched: true,
        args: splitArgs(commandArgs),
        original: `/${normalized} ${commandArgs}`.trim(),
      };
    }
    const match = VIBEMUD_COMMAND_RE.exec(prompt.trim());
    if (match) {
      return { matched: true, args: splitArgs(match[1] || ''), original: prompt.trim() };
    }
    return { matched: false, args: [], original: '' };
  }

  if (eventName === 'UserPromptSubmit') {
    const prompt = String(event.prompt || '').trim();
    const match = VIBEMUD_COMMAND_RE.exec(prompt);
    if (match) {
      return { matched: true, args: splitArgs(match[1] || ''), original: prompt };
    }
    return { matched: false, args: [], original: '' };
  }

  return { matched: false, args: [], original: '' };
}

function shouldSoftDeny(args) {
  if (!args.length) return null;
  const first = args[0];
  if (DANGEROUS_LIVE_COMMANDS.has(first) && !args.includes('--unsafe-context')) {
    return 'live-output 명령은 컨텍스트 보호를 위해 차단했습니다. HUD pane은 /vibemud:mud panel을 사용하세요.';
  }
  return null;
}

function shellQuoteForLog(value) {
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(value)) return value;
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function dispatcherInvocation(root, args) {
  if (process.platform === 'win32') {
    const dispatcher = path.join(root, 'scripts', 'vibemud-claude.ps1');
    if (!fs.existsSync(dispatcher)) {
      return { missing: dispatcher };
    }
    const shell = 'powershell.exe';
    return {
      command: shell,
      argv: ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', dispatcher, ...args],
      dispatcher,
    };
  }
  const dispatcher = path.join(root, 'scripts', 'vibemud-claude.sh');
  if (!fs.existsSync(dispatcher)) {
    return { missing: dispatcher };
  }
  return { command: 'bash', argv: [dispatcher, ...args], dispatcher };
}

function runDispatcher(args) {
  const root = pluginRoot();
  const invocation = dispatcherInvocation(root, args);
  if (invocation.missing) {
    return { code: 127, detail: `dispatcher not found: ${invocation.missing}`, summary: '' };
  }

  const env = { ...process.env };
  if (!env.VIBEMUD_CONTEXT_MODE) env.VIBEMUD_CONTEXT_MODE = 'hook';
  if (!env.VIBEMUD_CONTEXT_HOOK) env.VIBEMUD_CONTEXT_HOOK = '1';
  // VS Code/Cursor JS debug terminals inject a debugger bootloader via
  // NODE_OPTIONS; child node processes (e.g. npm .ps1/.cmd shims) would try to
  // attach to the editor's debugger and stall or pollute stderr.
  delete env.NODE_OPTIONS;
  delete env.VSCODE_INSPECTOR_OPTIONS;

  const proc = spawnSync(invocation.command, invocation.argv, {
    env,
    cwd: process.cwd(),
    encoding: 'utf8',
    timeout: DISPATCHER_TIMEOUT_MS,
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  if (proc.error && proc.error.code === 'ETIMEDOUT') {
    const timeoutError = new Error('dispatcher timeout');
    timeoutError.isTimeout = true;
    throw timeoutError;
  }
  if (proc.error) throw proc.error;

  const stdout = `${proc.stdout || ''}${proc.stderr || ''}`;
  const code = typeof proc.status === 'number' ? proc.status : 1;
  let commandLog = '(log unavailable)';
  try {
    commandLog = path.join(logDir(), 'claude-hook-commands.log');
    let logText = `$ ${[invocation.command, ...invocation.argv].map(shellQuoteForLog).join(' ')}\n`;
    if (stdout) {
      logText += stdout;
      if (!stdout.endsWith('\n')) logText += '\n';
    }
    logText += `exit=${code}\n\n`;
    appendPrivateText(commandLog, logText);
  } catch (_) {
    // Logging is best-effort; a read-only or locked log must not fail the command.
  }
  const summary = safeDispatcherSummary(args, stdout);
  return { code, detail: commandLog, summary };
}

function isHelpArgs(args) {
  if (!args.length) return true;
  return ['help', '--help', '-h', 'guide', '가이드', '도움말'].includes(args[0]);
}

function safeDispatcherSummary(args, stdout) {
  if (isHelpArgs(args)) return safeHelpSummary(stdout);

  const lines = [];
  for (const raw of stdout.split('\n')) {
    let line = raw.trim();
    if (!line.startsWith('[VibeMUD]')) continue;
    if (line.length > 240) line = `${line.slice(0, 237)}...`;
    lines.push(line);
    if (lines.length >= 3) break;
  }
  return lines.join('\n');
}

function safeHelpSummary(stdout) {
  // Help is static command documentation, not game state/log output, so it is
  // safe and useful to show it directly in the slash-command result.
  const text = stdout.trim();
  if (text.length > 12000) return `${text.slice(0, 11997)}...`;
  return text;
}

function block(reason) {
  // reason is shown to the user by Claude Code and is not added as prompt
  // context for UserPromptSubmit/UserPromptExpansion decision control.
  process.stdout.write(`${JSON.stringify({ decision: 'block', reason })}\n`);
}

function main() {
  const event = readEvent();
  let { matched, args, original } = extractVibemudArgs(event);
  const eventName = String(event.hook_event_name || '');
  if (!matched) {
    // The mod owns activity signals. Command interception remains fail-closed
    // when the mod does not handle a command; never bypass this firewall.
    if (process.env.VIBEMUD_MOD_ACTIVE === '1') return 0;
    if (ACTIVE_CODING_EVENTS.has(eventName)) {
      writeVibeActivitySafe(true, 'claude', event);
    } else if (STOP_CODING_EVENTS.has(eventName)) {
      writeVibeActivitySafe(false, 'claude', event);
    }
    return 0;
  }

  // The mod handles normal commands first. A missing/disabled mod must not
  // silently switch transports or send game input to the model.
  if (args[0] !== 'legacy') {
    block('VibeMUD mod를 사용할 수 없습니다. Claude Code 2.1.287+에서 플러그인을 다시 로드하세요. 외부 창 호환: /vibemud:mud legacy start');
    return 0;
  }
  args = args.slice(1);
  const softDeny = shouldSoftDeny(args);
  if (softDeny) {
    block(`VibeMUD: ${softDeny}`);
    return 0;
  }

  let result;
  try {
    result = runDispatcher(args);
  } catch (error) {
    if (error && error.isTimeout) {
      block('VibeMUD 명령이 제한 시간 내 끝나지 않아 중단했습니다. HUD pane 상태를 확인하세요.');
      return 0;
    }
    block(`VibeMUD 명령 처리 실패: ${error && error.message ? error.message : error}`);
    return 0;
  }

  const commandText = original || '/vibemud:mud';
  const extra = result.summary ? `\n${result.summary}` : '';
  if (result.code === 0) {
    if (isHelpArgs(args)) {
      block(`VibeMUD 조작 가이드: ${commandText}${extra}`);
    } else {
      block(`VibeMUD 명령 처리 완료: ${commandText}${extra}\n게임 출력은 메인 컨텍스트에 넣지 않고 HUD pane/로컬 DB에만 반영했습니다.`);
    }
  } else {
    block(`VibeMUD 명령 처리 실패(exit ${result.code}).${extra}\n세부 로그: ${result.detail}`);
  }
  return 0;
}

process.exitCode = main();
