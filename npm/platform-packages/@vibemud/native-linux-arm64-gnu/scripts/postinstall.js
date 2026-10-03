#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');

if (process.platform === 'win32') process.exit(0);

const binDir = path.resolve(__dirname, '..', 'bin');
for (const name of ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud']) {
  const file = path.join(binDir, name);
  if (!fs.existsSync(file)) continue;
  const mode = fs.statSync(file).mode | 0o755;
  fs.chmodSync(file, mode);
}
