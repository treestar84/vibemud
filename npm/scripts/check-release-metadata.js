#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');

const ROOT = path.resolve(__dirname, '..', '..');
const errors = [];
const readJson = (file) => JSON.parse(fs.readFileSync(path.join(ROOT, file), 'utf8'));
const cargo = fs.readFileSync(path.join(ROOT, 'Cargo.toml'), 'utf8');
const cargoVersion = /\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"/.exec(cargo)?.[1];
const npmPkg = readJson('npm/package.json');
const pluginPkg = readJson('claude-marketplace/plugins/vibemud/.claude-plugin/plugin.json');
const rootMarketplacePath = path.join(ROOT, '.claude-plugin/marketplace.json');
const expectedRepositoryUrl = 'https://github.com/treestar84/vibemud';

function expect(condition, message) {
  if (!condition) errors.push(message);
}

expect(Boolean(cargoVersion), 'workspace package version not found in Cargo.toml');
expect(npmPkg.version === cargoVersion, `npm/package.json version ${npmPkg.version} != Cargo ${cargoVersion}`);
expect(pluginPkg.version === cargoVersion, `plugin version ${pluginPkg.version} != Cargo ${cargoVersion}`);
expect(npmPkg.homepage === `${expectedRepositoryUrl}#readme`, 'npm homepage must point to the public repository');
expect(npmPkg.bugs?.url === `${expectedRepositoryUrl}/issues`, 'npm bugs URL must point to the public repository');
expect(npmPkg.repository?.url === `git+${expectedRepositoryUrl}.git`, 'npm repository URL must point to the public repository');
expect(fs.existsSync(rootMarketplacePath), 'repo-root .claude-plugin/marketplace.json is missing');
if (fs.existsSync(rootMarketplacePath)) {
  const market = readJson('.claude-plugin/marketplace.json');
  expect(market.metadata?.version === cargoVersion, `root marketplace version ${market.metadata?.version} != Cargo ${cargoVersion}`);
  const source = market.plugins?.find((plugin) => plugin.name === 'vibemud')?.source;
  expect(source === './claude-marketplace/plugins/vibemud', `root marketplace source must be ./claude-marketplace/plugins/vibemud, got ${source}`);
}
const localMarketplacePath = path.join(ROOT, 'claude-marketplace/.claude-plugin/marketplace.json');
if (fs.existsSync(localMarketplacePath)) {
  const local = readJson('claude-marketplace/.claude-plugin/marketplace.json');
  expect(local.metadata?.version === cargoVersion, `local marketplace version ${local.metadata?.version} != Cargo ${cargoVersion}`);
}
for (const pkgPath of findPackageJsons(path.join(ROOT, 'npm/platform-packages'))) {
  const pkg = JSON.parse(fs.readFileSync(pkgPath, 'utf8'));
  expect(pkg.version === cargoVersion, `${path.relative(ROOT, pkgPath)} version ${pkg.version} != Cargo ${cargoVersion}`);
  if (pkg.name.startsWith('@')) {
    expect(pkg.publishConfig?.access === 'public', `${pkg.name} must set publishConfig.access=public`);
    expect(
      pkg.repository?.url === expectedRepositoryUrl,
      `${pkg.name} repository.url must be ${expectedRepositoryUrl} for npm provenance`
    );
  }
}
if (npmPkg.name.startsWith('@')) {
  expect(npmPkg.publishConfig?.access === 'public', `${npmPkg.name} must set publishConfig.access=public`);
}
if (errors.length) {
  console.error(errors.map((error) => `- ${error}`).join('\n'));
  process.exit(1);
}
console.log(`release metadata OK (${cargoVersion})`);

function findPackageJsons(dir) {
  if (!fs.existsSync(dir)) return [];
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...findPackageJsons(full));
    else if (entry.name === 'package.json') out.push(full);
  }
  return out;
}
