// Run after cargo build --workspace; CI points VIBEMUD_TEST_BIN_DIR at each target.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync, execFile } = require('node:child_process');
const execFileAsync = require('node:util').promisify(execFile);
const repo = path.resolve(__dirname, '../..');
const launcher = path.join(repo, 'claude-marketplace/plugins/vibemud/scripts/vibemud-mod-launcher.cjs');
const { resolveNative } = require(launcher);
const suffix = process.platform === 'win32' ? '.exe' : '';
const binDir = process.env.VIBEMUD_TEST_BIN_DIR || path.join(repo, 'target/debug');
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

test('native bridge in paths with spaces and Korean: readonly, retry, leases, reset backup', { timeout: 65000 }, async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud 한글 space '));
  const game = path.join(root, '저장 data'); const binaries = path.join(root, 'native bins');
  fs.mkdirSync(binaries);
  for (const name of ['vibemud', 'vibemud-runtime']) fs.copyFileSync(path.join(binDir, name + suffix), path.join(binaries, name + suffix));
  const env = { ...process.env, VIBEMUD_HOME: game, VIBEMUD_BIN_DIR: binaries, VIBEMUD_BIN: '', VIBEMUD_RUNTIME_BIN: path.join(binaries, 'vibemud-runtime' + suffix) };
  const run = (...args) => {
    const r = spawnSync(process.execPath, [launcher, ...args, '--json'], { env, encoding: 'utf8', timeout: 18000, windowsHide: true });
    assert.ifError(r.error); const result = JSON.parse(r.stdout);
    assert.equal(result.protocol_version, 1); return { ...result, exit: r.status };
  };
  const ok = (...args) => { const r = run(...args); assert.equal(r.ok, true, r.error); assert.equal(r.exit, 0); return r.data; };
  try {
    assert.equal(ok('info').data_dir, game); assert.equal(fs.existsSync(game), false);
    ok('init'); const before = ok('snapshot'); const g = before.generation;
    assert.deepEqual(ok('snapshot').game, before.game);
    assert.equal(run('action', '--generation', g, '--request-id', 'stopped', '--kind', 'rest').ok, false);
    ok('session', 'ensure', '--session-id', 'one', '--active');
    ok('session', 'ensure', '--session-id', 'two', '--active');
    assert.equal(ok('snapshot').runtime.status, 'running');
    assert.equal(ok('snapshot').fever, true);
    ok('session', 'detach', '--session-id', 'one'); assert.equal(ok('snapshot').fever, true);
    ok('session', 'renew', '--session-id', 'two'); assert.equal(ok('snapshot').fever, false);
    const args = ['action', '--generation', g, '--request-id', 'exactly-once', '--kind', 'shop_buy', '--payload-json', JSON.stringify({ item_id: 'potion-small' })];
    const receipts = await Promise.all(Array.from({ length: 4 }, async () => {
      const r = await execFileAsync(process.execPath, [launcher, ...args, '--json'], { env, encoding: 'utf8', timeout: 18000, windowsHide: true });
      const envelope = JSON.parse(r.stdout); assert.equal(envelope.ok, true, envelope.error); return envelope.data;
    }));
    const a = receipts[0]; assert.ok(receipts.every(r => r.id === a.id)); assert.equal(ok(...args).id, a.id);
    let result;
    for (let i = 0; i < 30; i++) { result = ok('result', '--id', a.id); if (result.status === 'done') break; await sleep(100); }
    assert.equal(result.status, 'done', JSON.stringify(result));
    const after = ok('snapshot');
    assert.equal(after.game.player.gold, before.game.player.gold - 25);
    assert.deepEqual(after.earnings, before.earnings); // Spending is never expedition income.
    const stock = after.game.inventory.find(i => i.item_id === 'potion-small'); assert.equal(stock.quantity, 1);
    assert.equal(run('action', '--generation', g, '--request-id', 'exactly-once', '--kind', 'rest').ok, false);
    assert.equal(run('action', '--generation', g, '--request-id', 'inject', '--kind', 'shop_buy', '--payload-json', '{"item_id":"potion-small","prompt":"secret"}').ok, false);
    assert.equal(run('action', '--generation', g, '--request-id', 'missing', '--kind', 'equip').ok, false);
    ok('session', 'stop', '--session-id', 'two');
    assert.equal(ok(...args).id, a.id); // acknowledgement survives stop
    const reset = ok('reset', '--generation', g, '--yes');
    assert.equal(fs.existsSync(reset.backup), true); assert.notEqual(reset.generation, g);
    assert.equal(run('reset', '--generation', g, '--yes').ok, false);
    assert.equal(ok('snapshot').game.clock_tick, 0);
    assert.equal(ok('snapshot').earnings.kills, 0);
    ok('session', 'ensure', '--session-id', 'lease');
    ok('session', 'detach', '--session-id', 'lease');
    for (let i = 0; i < 30 && ok('snapshot').runtime.status === 'running'; i++) await sleep(100);
    assert.equal(ok('snapshot').runtime.status, 'stopped');
  } finally {
    run('session', 'stop', '--session-id', 'cleanup');
    fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  }
});

test('launcher honors explicit native paths and rejects missing/relative paths', () => {
  const native = path.join(binDir, 'vibemud' + suffix);
  assert.equal(resolveNative({ env: { VIBEMUD_BIN: native } }), native);
  assert.throws(() => resolveNative({ env: { VIBEMUD_BIN: 'relative' } }), /absolute/);
  assert.throws(() => resolveNative({ env: { VIBEMUD_BIN: path.join(binDir, 'missing') } }), /existing/);
});

test('cached plugin resolves a global npm native package without executing a shell shim', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud npm cache 한글 '));
  try {
    const cached = path.join(root, 'cache/plugin/scripts/launcher.cjs');
    const prefix = path.join(root, 'npm prefix');
    const pkg = path.join(prefix, 'node_modules/vibemud');
    fs.mkdirSync(path.dirname(cached), { recursive: true });
    fs.copyFileSync(launcher, cached);
    fs.mkdirSync(path.join(pkg, 'bin'), { recursive: true });
    fs.copyFileSync(path.join(repo, 'npm/bin/resolve.js'), path.join(pkg, 'bin/resolve.js'));
    fs.mkdirSync(path.join(pkg, 'native'), { recursive: true });
    fs.copyFileSync(path.join(binDir, 'vibemud' + suffix), path.join(pkg, 'native/vibemud' + suffix));
    const env = { ...process.env, PATH: prefix, Path: prefix, HOME: '', APPDATA: '', LOCALAPPDATA: '', VIBEMUD_BIN: '', VIBEMUD_BIN_DIR: '', VIBEMUD_HOME: path.join(root, 'save') };
    const r = spawnSync(process.execPath, [cached, 'info', '--json'], { env, encoding: 'utf8', timeout: 10000, windowsHide: true });
    assert.equal(r.status, 0, r.stderr || r.stdout);
    assert.equal(JSON.parse(r.stdout).ok, true);
    assert.equal(fs.existsSync(env.VIBEMUD_HOME), false);
  } finally { fs.rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }); }
});

test('classic fallback blocks game context even when the mod activity flag is set', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'vibemud firewall '));
  const data = path.join(root, 'data');
  try {
    const hook = path.join(repo, 'claude-marketplace/plugins/vibemud/scripts/vibemud-context-hook.js');
    const r = spawnSync(process.execPath, [hook], { encoding: 'utf8', env: { ...process.env, VIBEMUD_HOME: data, VIBEMUD_MOD_ACTIVE: '1' }, input: JSON.stringify({ hook_event_name: 'UserPromptSubmit', prompt: '/vibemud:mud start', session_id: 'test' }) });
    assert.equal(r.status, 0, r.stderr);
    assert.equal(JSON.parse(r.stdout).decision, 'block');
    assert.equal(fs.existsSync(data), false);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
