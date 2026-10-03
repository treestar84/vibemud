#!/usr/bin/env node
'use strict';
// Mods have no Node API. This small launcher resolves native executables without
// invoking npm.cmd, PowerShell, a shell, or interpolating game input into code.
const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

function resolveNative(options = {}) {
  const env = options.env || process.env;
  const platform = options.platform || process.platform;
  const exists = options.exists || fs.existsSync;
  const exe = platform === 'win32' ? 'vibemud.exe' : 'vibemud';
  const explicit = env.VIBEMUD_BIN || (env.VIBEMUD_BIN_DIR && path.join(env.VIBEMUD_BIN_DIR, exe));
  if (explicit) {
    if (!path.isAbsolute(explicit) || !exists(explicit)) throw new Error('VIBEMUD_BIN / VIBEMUD_BIN_DIR must name an existing absolute native executable');
    return explicit;
  }
  const repo = path.resolve(__dirname, '../../../..');
  if (exists(path.join(repo, 'Cargo.toml'))) {
    for (const mode of ['debug', 'release']) {
      const candidate = path.join(repo, 'target', mode, exe);
      if (exists(candidate)) return candidate;
    }
  }
  const dirs = (env.PATH || env.Path || '').split(platform === 'win32' ? ';' : ':').filter(Boolean);
  const roots = [];
  const installDirs = [env.LOCALAPPDATA && path.join(env.LOCALAPPDATA, 'VibeMUD', 'bin'),
    env.HOME && path.join(env.HOME, '.local', 'share', 'vibemud', 'bin')].filter(Boolean);
  for (const dir of installDirs) {
    try {
      const nativeDir = fs.readFileSync(path.join(dir, 'native-dir.txt'), 'utf8').trim();
      const candidate = path.join(nativeDir, exe);
      if (path.isAbsolute(candidate) && exists(candidate)) return candidate;
    } catch (_) { /* Installed through npm or an older source installer. */ }
  }
  if (env.APPDATA) roots.push(path.join(env.APPDATA, 'npm', 'node_modules', 'vibemud'));
  for (const dir of dirs) {
    roots.push(path.join(dir, 'node_modules', 'vibemud'), path.resolve(dir, '../lib/node_modules/vibemud'));
    const command = path.join(dir, exe);
    if (exists(command)) {
      // npm's Unix bin is a symlink to bin/vibemud.js; never recursively call it.
      try {
        const real = fs.realpathSync(command);
        if (real.endsWith('.js')) roots.push(path.resolve(real, '../..'));
        else return real;
      } catch (_) { /* Try other documented install locations. */ }
    }
  }
  for (const root of [...new Set(roots)]) {
    const resolver = path.join(root, 'bin', 'resolve.js');
    if (exists(resolver)) {
      try { return require(resolver).resolveBin('vibemud', { includeLocalTargets: false, packageRoot: root }); }
      catch (_) { /* A broken optional package should still allow native installs. */ }
    }
  }
  if (env.LOCALAPPDATA) {
    const candidate = path.join(env.LOCALAPPDATA, 'VibeMUD', 'bin', exe);
    if (exists(candidate)) return candidate;
  }
  throw new Error('VibeMUD native CLI not found. Install the bridge-capable VibeMUD release or set VIBEMUD_BIN_DIR, then restart Claude Code.');
}

function main(argv) {
  try {
    if (argv[0] === '--legacy') {
      const name = argv[1];
      if (argv.length !== 2 || !['start', 'end', 'c', 'i', 'm', 'q', 'set', 'panel', 'help'].includes(name)) throw new Error('Invalid legacy operation');
      const isWindows = process.platform === 'win32';
      const executable = isWindows ? 'powershell.exe' : 'bash';
      const script = path.join(__dirname, isWindows ? 'vibemud-claude.ps1' : 'vibemud-claude.sh');
      const args = isWindows ? ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', script, name] : [script, name];
      const result = spawnSync(executable, args, { env: { ...process.env, VIBEMUD_CONTEXT_MODE: 'hook' }, shell: false, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'], timeout: 15000 });
      if (result.error) throw result.error;
      return result.status === null ? 1 : result.status;
    }
    const binary = resolveNative();
    const env = { ...process.env };
    delete env.NODE_OPTIONS;
    delete env.VSCODE_INSPECTOR_OPTIONS;
    const result = spawnSync(binary, ['bridge', ...argv], {
      env, shell: false, windowsHide: true, encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'], timeout: 15000, maxBuffer: 2 * 1024 * 1024,
    });
    if (result.error) throw result.error;
    if (result.stdout) process.stdout.write(result.stdout);
    if (result.stderr) process.stderr.write(result.stderr);
    return result.status === null ? 1 : result.status;
  } catch (error) {
    // A timeout/pipe failure can occur after SQLite committed. The mod must
    // retain its request id until a retry obtains the durable receipt.
    process.stdout.write(JSON.stringify({ protocol_version: 1, ok: false, retryable: true, error: error.message }) + '\n');
    return 1;
  }
}
if (require.main === module) process.exitCode = main(process.argv.slice(2));
module.exports = { resolveNative };
