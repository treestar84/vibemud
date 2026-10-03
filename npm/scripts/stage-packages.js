#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');

const BINS = ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud'];
const ROOT = path.resolve(__dirname, '..', '..');
const NPM_ROOT = path.join(ROOT, 'npm');
const VERSION = JSON.parse(fs.readFileSync(path.join(NPM_ROOT, 'package.json'), 'utf8')).version;
const DIST = path.resolve(ROOT, argValue('--dist') || 'dist/npm');
const STAGE = argValue('--stage') || 'stage1';
const ALLOW_MISSING = process.argv.includes('--allow-missing');
const HOST_ONLY = process.argv.includes('--host-only');

const MATRIX = [
  { pkg: '@vibemud/native-darwin-x64', platform: 'darwin', arch: 'x64', target: 'x86_64-apple-darwin' },
  { pkg: '@vibemud/native-darwin-arm64', platform: 'darwin', arch: 'arm64', target: 'aarch64-apple-darwin' },
  { pkg: '@vibemud/native-linux-x64-gnu', platform: 'linux', arch: 'x64', target: 'x86_64-unknown-linux-gnu' },
  { pkg: '@vibemud/native-linux-arm64-gnu', platform: 'linux', arch: 'arm64', target: 'aarch64-unknown-linux-gnu' },
  { pkg: '@vibemud/native-win32-x64-msvc', platform: 'win32', arch: 'x64', target: 'x86_64-pc-windows-msvc' },
  { pkg: '@vibemud/native-win32-arm64-msvc', platform: 'win32', arch: 'arm64', target: 'aarch64-pc-windows-msvc' },
];

function argValue(flag) {
  const index = process.argv.indexOf(flag);
  return index >= 0 ? process.argv[index + 1] : null;
}

function exe(bin, platform) {
  return platform === 'win32' ? `${bin}.exe` : bin;
}

function copyRecursive(src, dest) {
  const stat = fs.statSync(src);
  if (stat.isDirectory()) {
    fs.mkdirSync(dest, { recursive: true });
    for (const entry of fs.readdirSync(src)) {
      copyRecursive(path.join(src, entry), path.join(dest, entry));
    }
    return;
  }
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  fs.copyFileSync(src, dest);
  fs.chmodSync(dest, stat.mode);
}

function copyRootPackage(dest) {
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of ['bin', 'scripts', 'README.md', 'package.json']) {
    copyRecursive(path.join(NPM_ROOT, entry), path.join(dest, entry));
  }
  copyRecursive(path.join(ROOT, 'LICENSE'), path.join(dest, 'LICENSE'));
}

function patchRootPackageForHostOnly(dest, item) {
  const packageJson = path.join(dest, 'package.json');
  const pkg = JSON.parse(fs.readFileSync(packageJson, 'utf8'));
  pkg.os = [item.platform];
  pkg.cpu = [item.arch];
  pkg.optionalDependencies = {};
  pkg.optionalDependenciesNote =
    `Host-only monolithic package: bundles ${item.platform}-${item.arch} native binaries. ` +
    'Use CI-produced @vibemud/native-* optional packages for a full multi-platform release.';
  pkg.vibemudDistribution = {
    mode: 'host-only-monolithic',
    platform: item.platform,
    arch: item.arch,
  };
  fs.writeFileSync(packageJson, `${JSON.stringify(pkg, null, 2)}\n`);
}

function binarySource(bin, item) {
  const artifactRoot = process.env.VIBEMUD_BINARY_ROOT;
  const candidates = [
    artifactRoot && path.join(artifactRoot, item.target, exe(bin, item.platform)),
    artifactRoot && path.join(artifactRoot, exe(bin, item.platform)),
    path.join(ROOT, 'target', item.target, 'release', exe(bin, item.platform)),
  ];
  if (item.platform === process.platform && item.arch === process.arch) {
    candidates.push(path.join(ROOT, 'target', 'release', exe(bin, item.platform)));
  }
  return candidates.filter(Boolean).find((candidate) => fs.existsSync(candidate));
}

function copyNativeSet(dest, item) {
  for (const bin of BINS) {
    const src = binarySource(bin, item);
    if (!src) {
      const message = `missing ${bin} for ${item.target}`;
      if (ALLOW_MISSING) {
        console.warn(`[vibemud-stage] ${message}`);
        continue;
      }
      throw new Error(message);
    }
    const out = path.join(dest, 'bin', exe(bin, item.platform));
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.copyFileSync(src, out);
    if (item.platform !== 'win32') fs.chmodSync(out, 0o755);
  }
}

function stageRootMonolithic() {
  const dest = path.join(DIST, 'vibemud');
  copyRootPackage(dest);
  const item = MATRIX.find((entry) => entry.platform === process.platform && entry.arch === process.arch);
  if (!item) throw new Error(`unsupported host ${process.platform}-${process.arch}`);
  if (HOST_ONLY) patchRootPackageForHostOnly(dest, item);
  const nativeDest = path.join(dest, 'native', `${item.platform}-${item.arch}`);
  copyNativeSet(nativeDest, item);
  console.log(`[vibemud-stage] staged root package at ${dest}${HOST_ONLY ? ` (${item.platform}-${item.arch} host-only)` : ''}`);
}

function stageOptionalPackages() {
  for (const item of MATRIX) {
    const template = path.join(NPM_ROOT, 'platform-packages', ...item.pkg.split('/'));
    const dest = path.join(DIST, ...item.pkg.split('/'));
    fs.rmSync(dest, { recursive: true, force: true });
    copyRecursive(template, dest);
    copyRecursive(path.join(ROOT, 'LICENSE'), path.join(dest, 'LICENSE'));
    copyNativeSet(dest, item);
    console.log(`[vibemud-stage] staged optional package ${item.pkg} at ${dest}`);
  }
}

fs.mkdirSync(DIST, { recursive: true });
if (STAGE === 'stage1') stageRootMonolithic();
else if (STAGE === 'stage2') stageOptionalPackages();
else if (STAGE === 'all') { stageRootMonolithic(); stageOptionalPackages(); }
else throw new Error(`unknown --stage ${STAGE}`);

console.log(`[vibemud-stage] version ${VERSION}`);
