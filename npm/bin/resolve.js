const { existsSync } = require('node:fs');
const path = require('node:path');

const PLATFORM_PACKAGES = {
  'darwin-arm64': '@vibemud/native-darwin-arm64',
  'darwin-x64': '@vibemud/native-darwin-x64',
  'linux-arm64': '@vibemud/native-linux-arm64-gnu',
  'linux-x64': '@vibemud/native-linux-x64-gnu',
  'win32-arm64': '@vibemud/native-win32-arm64-msvc',
  'win32-x64': '@vibemud/native-win32-x64-msvc',
};

function exe(name, platform = process.platform) {
  return platform === 'win32' ? `${name}.exe` : name;
}

function envName(name) {
  return name.toUpperCase().replace(/-/g, '_') + '_BIN';
}

function packageNameFor(platform = process.platform, arch = process.arch) {
  return PLATFORM_PACKAGES[`${platform}-${arch}`];
}

function optionalPackageCandidate(name, options = {}) {
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const packageRoot = options.packageRoot || path.resolve(__dirname, '..');
  const pkg = options.packageName || packageNameFor(platform, arch);
  if (!pkg) return null;

  try {
    const packageJson = require.resolve(`${pkg}/package.json`, {
      paths: [packageRoot, __dirname],
    });
    return path.join(path.dirname(packageJson), 'bin', exe(name, platform));
  } catch (_) {
    return null;
  }
}

function candidates(name, options = {}) {
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const root = options.repoRoot || path.resolve(__dirname, '..', '..');
  const packageRoot = options.packageRoot || path.resolve(__dirname, '..');
  const platformArch = `${platform}-${arch}`;
  const env = process.env[envName(name)];
  const includeLocalTargets = options.includeLocalTargets !== false;
  const localTargets = includeLocalTargets
    ? [
        path.join(root, 'target', 'release', exe(name, platform)),
        path.join(root, 'target', 'debug', exe(name, platform)),
      ]
    : [];

  return [
    env,
    optionalPackageCandidate(name, { ...options, platform, arch, packageRoot }),
    path.join(packageRoot, 'native', platformArch, 'bin', exe(name, platform)),
    path.join(packageRoot, 'native', platformArch, exe(name, platform)),
    path.join(packageRoot, 'native', exe(name, platform)),
    ...localTargets,
  ].filter(Boolean);
}

function resolveBin(name, options = {}) {
  const found = candidates(name, options).find((candidate) => existsSync(candidate));
  if (found) return found;
  const hint = candidates(name, options).map((candidate) => `  - ${candidate}`).join('\n');
  const platformPackage = packageNameFor(options.platform, options.arch);
  const optionalHint = platformPackage ? `Optional package: ${platformPackage}\n` : '';
  throw new Error(
    `Cannot find ${name} native binary.\n` +
    optionalHint +
    `Set ${envName(name)} or install the matching VibeMUD platform package/bundled binary.\n` +
    `Checked:\n${hint}\n` +
    `For local development, run: cargo build --workspace --release`
  );
}

module.exports = { PLATFORM_PACKAGES, candidates, envName, exe, packageNameFor, resolveBin };
