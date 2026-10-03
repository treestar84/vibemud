#!/usr/bin/env node
const fs = require('node:fs');

const raw = fs.readFileSync(0, 'utf8').trim();
if (!raw) {
  console.error('npm pack JSON input is empty');
  process.exit(1);
}
const data = JSON.parse(raw);
const packs = Array.isArray(data) ? data : [data];
if (packs.length === 0) {
  console.error('npm pack JSON contains zero packages');
  process.exit(1);
}
const deny = [
  /(^|\/)\.git(\/|$)/,
  /(^|\/)\.omx(\/|$)/,
  /(^|\/)\.claude(\/|$)/,
  /transcript/i,
  /prompt/i,
  /editor-buffer/i,
  /vibemud\.db$/,
  /(^|\/)target\//,
  /(^|\/)crates\//,
  /(^|\/)docs\/.*\.(png|jpg|jpeg)$/i,
];
const required = ['package.json', 'README.md', 'LICENSE'];
const errors = [];
const binaryNames = ['vibemud', 'mudctl', 'vibemud-runtime', 'vibemud-hud'];
for (const pack of packs) {
  const files = (pack.files || []).map((file) => file.path);
  for (const item of required) {
    if (!files.includes(item) && !files.includes(`package/${item}`)) errors.push(`${pack.filename || pack.name}: missing ${item}`);
  }
  for (const file of files) {
    if (deny.some((pattern) => pattern.test(file))) {
      errors.push(`${pack.filename || pack.name}: denied file in package: ${file}`);
    }
  }
  if (pack.name && pack.name.startsWith('@vibemud/native-')) {
    const hasPostinstall = files.some((file) => file === 'scripts/postinstall.js' || file.endsWith('/scripts/postinstall.js'));
    if (!hasPostinstall) errors.push(`${pack.name}: missing native executable-permission postinstall`);
    for (const bin of binaryNames) {
      const hasBinary = files.some((file) => file === `bin/${bin}` || file === `bin/${bin}.exe` || file.endsWith(`/bin/${bin}`) || file.endsWith(`/bin/${bin}.exe`));
      if (!hasBinary) errors.push(`${pack.name}: missing native binary ${bin}`);
    }
  }
}

if (errors.length) {
  console.error(errors.map((error) => `- ${error}`).join('\n'));
  process.exit(1);
}
console.log('pack contents OK');
