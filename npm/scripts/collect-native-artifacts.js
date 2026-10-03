#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const BINS = ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud'];
const MATRIX = [
  { target: 'x86_64-apple-darwin', platform: 'darwin' },
  { target: 'aarch64-apple-darwin', platform: 'darwin' },
  { target: 'x86_64-unknown-linux-gnu', platform: 'linux' },
  { target: 'aarch64-unknown-linux-gnu', platform: 'linux' },
  { target: 'x86_64-pc-windows-msvc', platform: 'win32' },
  { target: 'aarch64-pc-windows-msvc', platform: 'win32' },
];

const input = path.resolve(argValue('--input') || 'artifacts/native-download');
const output = path.resolve(argValue('--output') || 'artifacts/native');
const allowMissing = process.argv.includes('--allow-missing');
const errors = [];

function argValue(flag) {
  const index = process.argv.indexOf(flag);
  return index >= 0 ? process.argv[index + 1] : null;
}

function exe(bin, platform) {
  return platform === 'win32' ? `${bin}.exe` : bin;
}

function walk(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else out.push(full);
  }
  return out;
}

function findBinary(target, bin, platform, files) {
  const expected = exe(bin, platform);
  return files.find((file) => {
    const normalized = file.split(path.sep).join('/');
    return path.basename(file) === expected && normalized.includes(target);
  }) || files.find((file) => path.basename(file) === expected && file.includes(target));
}

function sha256(file) {
  const hash = crypto.createHash('sha256');
  hash.update(fs.readFileSync(file));
  return hash.digest('hex');
}

fs.rmSync(output, { recursive: true, force: true });
fs.mkdirSync(output, { recursive: true });
const files = walk(input);
const manifest = [];
for (const item of MATRIX) {
  const targetDir = path.join(output, item.target);
  fs.mkdirSync(targetDir, { recursive: true });
  for (const bin of BINS) {
    const src = findBinary(item.target, bin, item.platform, files);
    if (!src) {
      const message = `missing ${bin} for ${item.target} in ${input}`;
      if (allowMissing) {
        console.warn(`[vibemud-collect] ${message}`);
        continue;
      }
      errors.push(message);
      continue;
    }
    const dest = path.join(targetDir, exe(bin, item.platform));
    fs.copyFileSync(src, dest);
    fs.chmodSync(dest, item.platform === 'win32' ? 0o644 : 0o755);
    manifest.push({ target: item.target, bin, path: path.relative(output, dest), sha256: sha256(dest) });
  }
}
fs.writeFileSync(path.join(output, 'checksums.json'), JSON.stringify(manifest, null, 2) + '\n');
if (errors.length) {
  console.error(errors.map((error) => `- ${error}`).join('\n'));
  process.exit(1);
}
console.log(`[vibemud-collect] collected ${manifest.length} binaries into ${output}`);
