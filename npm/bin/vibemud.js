#!/usr/bin/env node
const { spawnSync } = require('node:child_process');
const { resolveBin } = require('./resolve');
let bin;
try {
  bin = resolveBin('vibemud');
} catch (error) {
  console.error(error.message);
  process.exit(127);
}
const result = spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
