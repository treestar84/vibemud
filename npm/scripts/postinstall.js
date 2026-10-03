#!/usr/bin/env node
const { chmodSync, existsSync, statSync } = require('node:fs');
const { spawnSync } = require('node:child_process');
const path = require('node:path');
const { resolveBin, packageNameFor } = require('../bin/resolve');

const repoRoot = path.resolve(__dirname, '..', '..');
const cargoToml = path.join(repoRoot, 'Cargo.toml');
const packageRoot = path.resolve(__dirname, '..');

const requiredBins = ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud'];
function ensureExecutable(file) {
  if (process.platform === 'win32') return;
  const mode = statSync(file).mode;
  if ((mode & 0o111) === 0o111) return;
  chmodSync(file, mode | 0o755);
}

try {
  for (const bin of requiredBins) {
    ensureExecutable(resolveBin(bin, { includeLocalTargets: false, packageRoot }));
  }
  process.exit(0);
} catch (_) {
  // Continue to local-source fallback below.
}

if (existsSync(cargoToml)) {
  const allReleaseBinsExist = requiredBins.every((bin) =>
    existsSync(path.join(repoRoot, 'target', 'release', process.platform === 'win32' ? `${bin}.exe` : bin))
  );
  if (allReleaseBinsExist) {
    process.exit(0);
  }
  const cargo = spawnSync('cargo', ['build', '--workspace', '--release'], { cwd: repoRoot, stdio: 'inherit' });
  process.exit(cargo.status ?? 1);
}

const optionalPackage = packageNameFor();
console.error('[vibemud] Native binary not bundled for this platform.');
if (optionalPackage) {
  console.error(`[vibemud] Expected optional platform package: ${optionalPackage}`);
}
console.error('[vibemud] Install the matching platform package or set VIBEMUD_BIN/MUDCTL_BIN/VIBEMUD_RUNTIME_BIN/VIBEMUD_HUD_BIN.');
process.exit(1);
