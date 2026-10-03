import { expect, test, mock } from 'claude-code/testing';
import type { On } from 'claude-code';
import { parseCommand, decode, hpBar } from '../hooks/model';
import { cellWidth, clip, columns } from '../hooks/view';

const PANE = { plugin: 'vibemud', component: 'Pane', requestId: 'vibemud', viewport: { columns: 100, rows: 30 }, props: { title: 'VibeMUD', isFocused: true, bodyColumns: 48, placement: 'inline', scroll: { offset: 0, bodyRows: 18 }, view: {} } } as const;
function setup(on: On, collision = false) {
  const calls: string[][] = []; const opens: unknown[] = [];
  const clock = mock.clock(on); mock.store(on);
  const data = {
    generation: 'save-1', runtime: { status: 'stopped' }, fever: false,
    earnings: { kills: 0, xp: 0, gold: 0, items: 0, last_item: null as string | null },
    game: { player: { name: 'Arin', class_id: 'warrior', level: 1, hp: 100, max_hp: 100, mp: 20, max_mp: 20, xp: 0, xp_to_next: 40, gold: 200, attack: 10, defense: 5, accuracy: 3, evasion: 0, speed: 5, regen: 3, luck: 2, mode: 'idle', current_area_id: 'training-ground', current_dungeon_id: null }, clock_tick: 0, state_version: 1,
      inventory: [{ id: 'sword-1', item_id: 'basic-sword', item_type: 'weapon', name: 'Sword', rarity: 'Common', quantity: 1, enhancement_level: 0, equipped_slot: null, locked: false, stat1_type: 'attack', stat1_value: 3, stat2_type: null, stat2_value: null, stat3_type: null, stat3_value: null }], party: [], combat: { in_combat: false, monster_name: null, monster_hp: null, monster_max_hp: null, turn_index: 0 }, recent_log: ['Ready'] },
    quests: [{ id: 'daily-1', title: 'First hunt', progress: 1, target: 1, status: 'completed', reward_kind: 'gold', reward_amount: 20, fever_minutes: 5 }], areas: [{ id: 'training-ground', name: 'Training', level: 1 }], dungeons: [{ id: 'goblin-cave', name: 'Cave', level: 3 }], companions: [], commands: [],
    settings: { language: 'ko', intro_seen: true, animations: true }, progress: { recovery_until: null, dungeon_point: null, dungeon_kills: null }, enhancements: {},
  };
  let actionFailures = 0;
  on('session.start', () => ({ cwd: '/work' }));
  on('session.end', () => ({ sessionId: 'session-1' }));
  on('session.id', () => ({ value: 'session-1' }));
  on('env.set', () => ({ value: undefined }));
  on('command.list', () => ({ value: collision ? [{ name: 'mud', description: 'other', plugin: 'other', source: 'plugin' }] : [] }));
  on('command.register', () => ({ value: { command: 'mud' } }));
  on('command.run', () => ({ text: 'other plugin' }));
  on('ui.open', (_$, e) => { opens.push(e); return { value: { isPlaced: true } }; });
  on('ui.close', () => ({ value: undefined }));
  on('ui.log', () => ({ value: undefined }));
  on('ui.toast', () => ({ value: undefined }));
  on('ui.render', ($, e) => { const { Text } = $.ui.resolve(e); return Text({ children: [] }); });
  on('turn.start', (_$, e) => ({ turnId: e.turnId }));
  on('turn.complete', () => ({ text: '' }));
  on('classic.SessionStart', () => ({}));
  on('process.run', (_$, e) => {
    const args = e.argv.slice(2); calls.push(args);
    let answer: unknown = {};
    if (args[0] === 'info') answer = { data_dir: '/game data/한글' };
    if (args[0] === 'snapshot') answer = data;
    if (args[0] === 'session' && args[1] === 'ensure') data.runtime.status = 'running';
    if (args[0] === 'session' && args[1] === 'stop') data.runtime.status = 'stopped';
    if (args[0] === 'action') {
      if (actionFailures-- > 0) return { value: { exitCode: 1, stdout: JSON.stringify({ protocol_version: 1, ok: false, retryable: true, error: 'Simulated lost response' }), stderr: '', isStdoutTruncated: false, isStderrTruncated: false } };
      answer = { id: 'queued-1', status: 'pending' };
    }
    if (args[0] === 'result') answer = { id: 'queued-1', status: 'done', result: { message: 'Done' } };
    if (args[0] === 'setting' && args[1] === 'language') data.settings.language = args[2]!;
    if (args[0] === 'reset') { data.generation = 'save-2'; data.runtime.status = 'stopped'; }
    return { value: { exitCode: 0, stdout: JSON.stringify({ protocol_version: 1, ok: true, data: answer }), stderr: '', isStdoutTruncated: false, isStderrTruncated: false } };
  });
  return { calls, opens, clock, data, failNextAction: () => { actionFailures = 1; } };
}

test('aliases and shell-looking input are parsed only as data', () => {
  expect(parseCommand('/vibemud:mud i')).toEqual({ type: 'tab', tab: 'inventory' });
  expect(parseCommand('hunt start training-ground')).toEqual({ type: 'action', kind: 'hunt_start', payload: { area_id: 'training-ground' } });
  expect(() => parseCommand('/mudblah')).toThrow();
  expect(() => parseCommand('hunt nonsense')).toThrow();
  expect(() => parseCommand('legacy start; echo bad')).toThrow();
  expect(() => decode('{"protocol_version":999,"ok":true}')).toThrow();
  expect(hpBar(500, 100, 4)).toBe('████');
});

test('commands and pane input perform game actions with no model context', async ($, on) => {
  const h = setup(on);
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' });
  expect(await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'vibemud:mud', args: 'start' })).toEqual({});
  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' });
  expect(await ui.find({ type: 'Text', text: /Lv.1 Arin/ })).toBeDefined();
  await ui.input({ key: 'game-command', text: 'draft stays', kind: 'change' });
  await h.clock.advance(1000);
  expect((await ui.find({ key: 'game-command' }))?.props.value).toBe('draft stays');
  await ui.press({ key: 'tab-inventory' });
  await ui.press({ key: 'item-sword-1' });
  await ui.press({ key: 'equip' });
  expect(h.calls.filter(c => c[0] === 'action').map(c => c[c.indexOf('--kind') + 1])).toEqual(['hunt_start', 'equip']);
  await ui.input({ key: 'game-command', text: 'quest claim-all' });
  expect(h.calls.some(c => c.includes('quest_claim_all'))).toBe(true);
  await ui.press({ key: 'back-adventure' });
  const before = h.calls.length;
  await ui.press({ key: 'close' });
  await h.clock.advance(1000);
  expect(h.calls.length).toBe(before); // hidden pane does not poll snapshots
  await ui.unmount();
});

test('lost acknowledgement retries the same id; reset and selling require confirmation', async ($, on) => {
  const h = setup(on); h.failNextAction();
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' });
  await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'mud', args: 'start' });
  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' });
  await ui.press({ key: 'retry' });
  const actions = h.calls.filter(c => c[0] === 'action');
  expect(actions.length).toBe(2); expect(actions[0]).toEqual(actions[1]);
  await ui.press({ key: 'tab-settings' }); await ui.press({ key: 'reset' });
  expect(h.calls.some(c => c[0] === 'reset')).toBe(false);
  await ui.press({ key: 'cancel' });
  await ui.press({ key: 'reset' }); await ui.press({ key: 'confirm' });
  expect(h.calls.filter(c => c[0] === 'reset').length).toBe(1);
  await ui.press({ key: 'start' });
  await ui.press({ key: 'tab-inventory' }); await ui.press({ key: 'item-sword-1' }); await ui.press({ key: 'sell' });
  expect(h.calls.some(c => c.includes('shop_sell'))).toBe(false);
  await ui.press({ key: 'confirm' });
  expect(h.calls.some(c => c.includes('shop_sell'))).toBe(true);
  await ui.unmount();
});

test('metadata-only fever, clear, and close preserve session ownership', async ($, on) => {
  const h = setup(on);
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' });
  await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'mud', args: 'start' });
  await $.turn.start({ turnId: 'turn-1', text: 'PRIVATE PROMPT NEVER SEND TO GAME' });
  await h.clock.settle();
  expect(h.calls.some(c => c.includes('--active'))).toBe(true);
  expect(JSON.stringify(h.calls).includes('PRIVATE')).toBe(false);
  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' });
  await ui.input({ key: 'game-command', text: 'rest' });
  expect(h.calls.some(c => c.includes('rest'))).toBe(true);
  await ui.unmount();
  await $.turn.complete({ turnId: 'turn-1', answer: 'PRIVATE REPLY', durationMs: 1, isAborted: false, reason: 'answer' });
  await h.clock.settle();
  expect(h.calls.filter(c => c[0] === 'session').at(-1)?.includes('--active')).toBe(true);
  await h.clock.advance(15000);
  expect(h.calls.filter(c => c[0] === 'session').at(-1)?.includes('--active')).toBe(false);
  await $.classic.SessionStart({ source: 'clear' });
  await $.session.end({ reason: 'prompt_input_exit', sessionId: 'session-1', resume: { id: 'session-1' } });
  expect(h.calls[h.calls.length - 1]?.slice(0, 2)).toEqual(['session', 'detach']);
});

test('another plugin owns /mud; namespaced command remains available', async ($, on) => {
  const h = setup(on, true);
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' });
  expect(await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'mud', args: 'start' })).toEqual({ text: 'other plugin' });
  expect(h.calls.length).toBe(0);
  expect(await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'vibemud:mud', args: 'c' })).toEqual({});
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ ...PANE, surface });
    for (const key of ['tab-map', 'tab-quests', 'tab-settings', 'tab-character', 'party', 'tab-character', 'skills', 'tab-character', 'shop']) await ui.press({ key });
    await ui.unmount();
  }
});

test('menu back keeps the pane; hide renews for 35s; stop requires an explicit choice', async ($, on) => {
  const h = setup(on);
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' });
  await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'mud', args: 'start' });
  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' });
  expect(await ui.find({ key: 'stop' })).toBeUndefined();
  await ui.press({ key: 'tab-character' }); await ui.press({ key: 'shop' });
  expect(await ui.find({ key: 'close' })).toBeUndefined();
  await ui.press({ key: 'back-adventure' }); await ui.press({ key: 'close' });
  const before = h.calls.length;
  for (let i = 0; i < 7; i++) await h.clock.advance(5000);
  expect(h.calls.slice(before).filter(c => c[0] === 'session' && c[1] === 'renew').length).toBe(7);
  expect(h.calls.slice(before).some(c => c[0] === 'snapshot' || c[1] === 'stop' || c[1] === 'detach')).toBe(false);
  const band = await $.ui.mount({ plugin: 'vibemud', component: 'AbovePrompt', surface: 'terminal', viewport: PANE.viewport,
    props: { hasSurvey: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} } });
  expect(await band.find({ key: 'reopen-game' })).toBeDefined();
  await band.press({ key: 'reopen-game' }); await ui.press({ key: 'tab-settings' });
  await ui.press({ key: 'stop' });
  expect(h.calls.some(c => c[1] === 'stop')).toBe(false);
  await ui.press({ key: 'cancel' }); await ui.press({ key: 'stop' }); await ui.press({ key: 'confirm' });
  expect(h.calls.filter(c => c[1] === 'stop').length).toBe(1);
  await h.clock.advance(5000);
  expect(await band.find({ key: 'reopen-game' })).toBeUndefined();
  await band.unmount(); await ui.unmount();
});

test('expeditions use combat earnings, survive subagent completion, and clear on reset', async ($, on) => {
  const h = setup(on);
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' });
  await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'mud', args: 'start' });
  await $.turn.start({ turnId: 'coding', text: 'PRIVATE CONTENT' }); await h.clock.settle();
  h.data.earnings = { kills: 3, xp: 120, gold: 45, items: 1, last_item: '희귀 검' };
  h.data.game.player.gold = 1; // A purchase is unrelated to combat earnings.
  await h.clock.advance(6000);
  await $.turn.complete({ turnId: 'child', agentId: 'agent-1', answer: 'PRIVATE CHILD', durationMs: 6000, isAborted: false, reason: 'answer' });
  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' });
  expect(await ui.find({ type: 'Text', text: /코딩 감지/ })).toBeDefined();
  await $.turn.complete({ turnId: 'coding', answer: 'PRIVATE ANSWER', durationMs: 6000, isAborted: false, reason: 'answer' }); await h.clock.settle();
  expect(await ui.find({ type: 'Text', text: 'XP +120' })).toBeDefined();
  expect(await ui.find({ type: 'Text', text: '+45G' })).toBeDefined();
  expect(await ui.find({ type: 'Text', text: /희귀 검/ })).toBeDefined();
  expect(JSON.stringify(h.calls).includes('PRIVATE')).toBe(false);
  await h.clock.advance(15000);
  expect(h.calls.filter(c => c[0] === 'session').at(-1)?.includes('--active')).toBe(false);
  h.data.generation = 'new-save'; await h.clock.advance(1000);
  expect(await ui.find({ type: 'Text', text: 'XP +120' })).toBeUndefined();
  await ui.unmount();
});

test('CJK names and column budgets remain bounded at narrow and wide widths', () => {
  expect(cellWidth('한글ABC')).toBe(7);
  expect(cellWidth('한')).toBe(2); // decomposed Hangul is normalized
  expect(cellWidth('e\u0301')).toBe(1);
  for (const width of [24, 32, 48, 72]) {
    const sizes = columns(width, [7, 6]);
    expect(sizes.reduce((a, b) => a + b, 0) + 2).toBe(width);
    expect(cellWidth(clip('아주 긴 이름의 전설적인 한글 장비 ABC', sizes[0]!)) <= sizes[0]!).toBe(true);
    expect(clip('short', 8)).toBe('short');
  }
});

test('tables keep headers, pagination, full details and navigation on both surfaces', async ($, on) => {
  const h = setup(on);
  const fullName = '아주 긴 이름의 전설적인 한글 장비 ABC';
  h.data.game.inventory = Array.from({ length: 20 }, (_, i) => ({ ...h.data.game.inventory[0]!, id: 'item-' + i, name: fullName + i }));
  await $.session.start({ surface: 'terminal', isInteractive: true, cwd: '/work' });
  await $.command.run({ origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 100 }, command: 'mud', args: 'start' });
  for (const surface of ['terminal', 'desktop'] as const) for (const width of [28, 48, 72]) {
    const ui = await $.ui.mount({ ...PANE, surface, props: { ...PANE.props, bodyColumns: width } });
    await ui.press({ key: 'tab-inventory' });
    // Page selection persists across mounts; return to the first page if needed.
    while (await ui.find({ key: 'inventory-page--1' })) await ui.press({ key: 'inventory-page--1' });
    const item = await ui.find({ key: 'item-item-0' });
    expect(cellWidth(String(item?.props.label)) < width).toBe(true);
    await ui.press({ key: 'item-item-0' });
    expect(await ui.find({ type: 'Text', text: fullName + '0' })).toBeDefined();
    expect(await ui.find({ key: 'item-item-6' })).toBeUndefined();
    await ui.press({ key: 'inventory-page-1' });
    expect(await ui.find({ key: 'item-item-6' })).toBeDefined();
    for (const key of ['back-adventure', 'tab-map', 'tab-quests', 'tab-character', 'party', 'tab-character', 'skills', 'tab-character', 'shop']) await ui.press({ key });
    expect(await ui.find({ type: 'Text', text: '가격' })).toBeDefined();
    await ui.press({ key: 'shop-detail-potion-small' });
    expect(await ui.find({ type: 'Text', text: '아스클레피오스 물약' })).toBeDefined();
    expect(await ui.find({ key: 'close' })).toBeUndefined();
    await ui.drawn(); // The real host validates all element props per surface.
    await ui.unmount();
  }
});
