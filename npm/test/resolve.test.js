const assert = require('node:assert/strict');
const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const test = require('node:test');
const { candidates, envName, exe, packageNameFor, resolveBin } = require('../bin/resolve');

function touch(file) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, 'binary');
}

test('envName maps binary names to override variables', () => {
  assert.equal(envName('vibemud'), 'VIBEMUD_BIN');
  assert.equal(envName('mudctl'), 'MUDCTL_BIN');
  assert.equal(envName('vibemud-runtime'), 'VIBEMUD_RUNTIME_BIN');
});

test('resolveBin prefers explicit environment override', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud-resolve-env-'));
  const bin = path.join(dir, exe('vibemud'));
  touch(bin);
  const previous = process.env.VIBEMUD_BIN;
  process.env.VIBEMUD_BIN = bin;
  try {
    assert.equal(resolveBin('vibemud', { includeLocalTargets: false }), bin);
  } finally {
    if (previous === undefined) delete process.env.VIBEMUD_BIN;
    else process.env.VIBEMUD_BIN = previous;
  }
});

test('resolveBin finds matching optional platform package', () => {
  const packageRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud-resolve-package-'));
  const pkg = packageNameFor('linux', 'x64');
  const pkgRoot = path.join(packageRoot, 'node_modules', ...pkg.split('/'));
  const bin = path.join(pkgRoot, 'bin', 'mudctl');
  touch(bin);
  fs.writeFileSync(path.join(pkgRoot, 'package.json'), JSON.stringify({ name: pkg, version: '0.0.0' }));
  assert.equal(resolveBin('mudctl', {
    packageRoot,
    platform: 'linux',
    arch: 'x64',
    includeLocalTargets: false,
  }), fs.realpathSync(bin));
});

test('candidate order keeps optional packages before bundled native and local Cargo targets', () => {
  const list = candidates('vibemud', {
    packageRoot: '/pkg',
    repoRoot: '/repo',
    platform: 'darwin',
    arch: 'arm64',
  });
  const nativeIndex = list.indexOf(path.join('/pkg', 'native', 'darwin-arm64', 'bin', 'vibemud'));
  const localIndex = list.indexOf(path.join('/repo', 'target', 'release', 'vibemud'));
  assert(nativeIndex >= 0);
  assert(localIndex > nativeIndex);
});

test('resolveBin reports a useful error when no binary exists', () => {
  assert.throws(
    () => resolveBin('vibemud', {
      packageRoot: fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud-resolve-missing-pkg-')),
      repoRoot: fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud-resolve-missing-repo-')),
      platform: 'win32',
      arch: 'x64',
      includeLocalTargets: false,
    }),
    /@vibemud\/native-win32-x64-msvc[\s\S]*VIBEMUD_BIN/
  );
});


test('postinstall marks optional package binaries executable', { skip: process.platform === 'win32' }, () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud-postinstall-chmod-'));
  const pkgRoot = path.join(root, 'package');
  fs.mkdirSync(path.join(pkgRoot, 'bin'), { recursive: true });
  fs.mkdirSync(path.join(pkgRoot, 'scripts'), { recursive: true });
  fs.copyFileSync(path.join(__dirname, '..', 'bin', 'resolve.js'), path.join(pkgRoot, 'bin', 'resolve.js'));
  fs.copyFileSync(
    path.join(__dirname, '..', 'scripts', 'postinstall.js'),
    path.join(pkgRoot, 'scripts', 'postinstall.js')
  );

  const optionalPkg = packageNameFor();
  assert.ok(optionalPkg, 'current platform must have an optional package mapping');
  const optionalRoot = path.join(pkgRoot, 'node_modules', ...optionalPkg.split('/'));
  fs.mkdirSync(path.join(optionalRoot, 'bin'), { recursive: true });
  fs.writeFileSync(path.join(optionalRoot, 'package.json'), JSON.stringify({ name: optionalPkg, version: '0.0.0' }));
  for (const bin of ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud']) {
    const file = path.join(optionalRoot, 'bin', bin);
    touch(file);
    fs.chmodSync(file, 0o644);
  }

  const result = spawnSync(process.execPath, [path.join(pkgRoot, 'scripts', 'postinstall.js')], {
    cwd: root,
    env: { ...process.env },
    encoding: 'utf8',
  });

  assert.equal(result.status, 0, result.stderr || result.stdout);
  for (const bin of ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud']) {
    const mode = fs.statSync(path.join(optionalRoot, 'bin', bin)).mode;
    assert.equal(mode & 0o111, 0o111, `${bin} should be executable`);
  }
});

test('postinstall fails instead of installing a package without native binaries', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud-postinstall-missing-'));
  const pkg = path.join(root, 'package');
  fs.mkdirSync(path.join(pkg, 'bin'), { recursive: true });
  fs.mkdirSync(path.join(pkg, 'scripts'), { recursive: true });
  fs.copyFileSync(path.join(__dirname, '..', 'bin', 'resolve.js'), path.join(pkg, 'bin', 'resolve.js'));
  fs.copyFileSync(
    path.join(__dirname, '..', 'scripts', 'postinstall.js'),
    path.join(pkg, 'scripts', 'postinstall.js')
  );

  const result = spawnSync(process.execPath, [path.join(pkg, 'scripts', 'postinstall.js')], {
    cwd: root,
    env: { ...process.env },
    encoding: 'utf8',
  });

  assert.equal(result.status, 1);
  assert.match(result.stderr, /Native binary not bundled/);
});
