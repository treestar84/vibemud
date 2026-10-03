#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const BINS = ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud'];
const MATRIX = [
  { pkg: '@vibemud/native-darwin-x64', target: 'x86_64-apple-darwin', platform: 'darwin' },
  { pkg: '@vibemud/native-darwin-arm64', target: 'aarch64-apple-darwin', platform: 'darwin' },
  { pkg: '@vibemud/native-linux-x64-gnu', target: 'x86_64-unknown-linux-gnu', platform: 'linux' },
  { pkg: '@vibemud/native-linux-arm64-gnu', target: 'aarch64-unknown-linux-gnu', platform: 'linux' },
  { pkg: '@vibemud/native-win32-x64-msvc', target: 'x86_64-pc-windows-msvc', platform: 'win32' },
  { pkg: '@vibemud/native-win32-arm64-msvc', target: 'aarch64-pc-windows-msvc', platform: 'win32' },
];

const nativeRoot = path.resolve(argValue('--native-root') || 'artifacts/native');
const packageRoot = path.resolve(argValue('--packages-root') || 'dist/npm');
const manifestPath = path.join(nativeRoot, 'checksums.json');
const errors = [];

function argValue(flag) {
  const index = process.argv.indexOf(flag);
  return index >= 0 ? process.argv[index + 1] : null;
}

function exe(bin, platform) {
  return platform === 'win32' ? `${bin}.exe` : bin;
}

function sha256(file) {
  const hash = crypto.createHash('sha256');
  hash.update(fs.readFileSync(file));
  return hash.digest('hex');
}

if (!fs.existsSync(manifestPath)) {
  console.error(`missing checksum manifest: ${manifestPath}`);
  process.exit(1);
}
const entries = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
if (!Array.isArray(entries) || entries.length !== MATRIX.length * BINS.length) {
  errors.push(`expected ${MATRIX.length * BINS.length} checksum entries, got ${Array.isArray(entries) ? entries.length : 'non-array'}`);
}
const byKey = new Map();
for (const entry of Array.isArray(entries) ? entries : []) {
  byKey.set(`${entry.target}:${entry.bin}`, entry);
}
for (const item of MATRIX) {
  const pkgRoot = path.join(packageRoot, ...item.pkg.split('/'));
  for (const bin of BINS) {
    const key = `${item.target}:${bin}`;
    const entry = byKey.get(key);
    if (!entry) {
      errors.push(`missing checksum entry for ${key}`);
      continue;
    }
    const normalizedPath = path.join(nativeRoot, item.target, exe(bin, item.platform));
    const stagedPath = path.join(pkgRoot, 'bin', exe(bin, item.platform));
    for (const file of [normalizedPath, stagedPath]) {
      if (!fs.existsSync(file)) {
        errors.push(`missing binary for ${key}: ${file}`);
        continue;
      }
      const actual = sha256(file);
      if (actual !== entry.sha256) {
        errors.push(`checksum mismatch for ${key}: ${file}`);
      }
    }
  }
}
if (errors.length) {
  console.error(errors.map((error) => `- ${error}`).join('\n'));
  process.exit(1);
}
console.log(`native checksums OK (${entries.length} entries)`);
