import type { EngineInterface, On, RenderElement } from 'claude-code';
import { decode, decodeSnapshot, earnedSince, hpBar, parseCommand, RejectedAction, SHOP } from './model';
import type { Earnings, Intent, Payload, Snapshot, Tab } from './model';
import { cellWidth, clip, columns, duration, label, number, rarityColor } from './view';

const PANE = 'vibemud';
let snapshot: Snapshot | null = null;
let tab: Tab = 'adventure';
let opened = false;
let interactive = false;
let ownsShortCommand = false;
let attached = false;
let session = '';
let activeTurn = '';
let graceUntil = 0;
let now = 0;
let connecting = false;
let heartbeatError = '';
let expedition: { turn: string; started: number; generation: string; baseline?: Earnings } | null = null;
let outcome: { earnings: Earnings; duration: number; aborted: boolean } | null = null;
const pages: Record<string, number> = {};
let dataDir = '';
let migration = '';
let error = '';
let pollError = '';
let notice = '';
let draft = '';
let loading = false;
let mutating = false;
let sequence = 0;
let epoch = 0;
let confirm: Intent | null = null;
let selection = '';
let pending: { request: string; generation: string; kind: string; payload: Payload; id?: string } | null = null;

async function bridge($: EngineInterface, args: string[], timeoutMs = 18000): Promise<unknown> {
  const result = await $.process.run(['node', $.plugin.root + '/scripts/vibemud-mod-launcher.cjs', ...args, '--json'], {
    timeoutMs, ...(dataDir ? { env: { VIBEMUD_HOME: dataDir } } : {}),
  });
  if (!result.stdout.trim()) throw new Error('VibeMUD CLI not available. Install the current native CLI and Node.js 18+.');
  const data = decode(result.stdout);
  if (result.exitCode !== 0) throw new Error('VibeMUD native process failed.');
  return data;
}

async function initialize($: EngineInterface): Promise<void> {
  if (!session) session = await $.session.id();
  const info = await bridge($, ['info']) as { data_dir: string; legacy_data_dir?: string; can_migrate?: boolean };
  dataDir = info.data_dir;
  if (info.can_migrate && info.legacy_data_dir) {
    migration = info.legacy_data_dir; $.ui.invalidate('ui.render'); return;
  }
  await bridge($, ['init']);
  await refresh($);
}

async function refresh($: EngineInterface): Promise<void> {
  if (loading || migration) return;
  loading = true;
  const currentEpoch = epoch;
  try {
    const next = decodeSnapshot(await bridge($, ['snapshot']));
    if (currentEpoch !== epoch) return;
    if (snapshot && snapshot.generation !== next.generation) {
      pending = null; confirm = null; selection = ''; notice = ''; error = '';
      expedition = null; outcome = null; graceUntil = 0;
    }
    snapshot = next;
    if (next.runtime.status !== 'running') attached = false;
    if (pending?.id) {
      const result = await bridge($, ['result', '--id', pending.id]) as { status: string; result?: { message: string }; error?: string };
      if (currentEpoch !== epoch) return;
      if (result.status === 'done' || result.status === 'failed') {
        notice = result.status === 'done' ? (snapshot.settings.language === 'en' ? 'Action complete' : '명령 처리 완료') : result.error || '실패 / Failed';
        pending = null;
      }
    }
    pollError = '';
  } catch (e) {
    pollError = e instanceof Error ? e.message : String(e);
  } finally {
    loading = false;
    $.ui.invalidate('ui.render');
  }
}

async function renew($: EngineInterface): Promise<void> {
  if (!attached || !session) return;
  try {
    now = await $.clock.now();
    const state = await bridge($, ['session', 'renew', '--session-id', session, ...(activeTurn || now < graceUntil ? ['--active'] : [])]) as { status?: string };
    if (state.status === 'stopped') { attached = false; graceUntil = 0; expedition = null; }
    heartbeatError = '';
  } catch (e) {
    heartbeatError = e instanceof Error ? e.message : String(e);
  }
  $.ui.invalidate('ui.render');
}

async function beginExpedition($: EngineInterface): Promise<void> {
  if (!attached || !activeTurn || expedition?.turn === activeTurn) return;
  const run = { turn: activeTurn, started: await $.clock.now(), generation: snapshot?.generation || '', baseline: undefined as Earnings | undefined };
  expedition = run;
  try {
    const fresh = decodeSnapshot(await bridge($, ['snapshot']));
    if (expedition === run && activeTurn === run.turn && fresh.generation === run.generation) run.baseline = fresh.earnings;
  } catch { /* Connection status is surfaced by refresh/renew; never invent earnings. */ }
}

async function show($: EngineInterface, focus: boolean): Promise<void> {
  const args = { id: PANE, title: 'VibeMUD', columns: 52, rows: 26 };
  await $.ui.open(focus ? { ...args, focus: true } : args);
  opened = true;
  await $.store.set('pane-open', true);
  $.ui.invalidate('ui.render');
}

async function submitAction($: EngineInterface): Promise<void> {
  if (!pending) return;
  const p = pending;
  let result: { id: string };
  try {
    result = await bridge($, ['action', '--request-id', p.request, '--generation', p.generation, '--kind', p.kind, '--payload-json', JSON.stringify(p.payload)]) as { id: string };
  } catch (e) {
    // A structured rejection did not commit. Unknown transport failures retain
    // the durable id so a retry can never charge/reward the player twice.
    if (e instanceof RejectedAction) pending = null;
    throw e;
  }
  p.id = result.id;
  notice = '명령 접수 / Queued';
  await refresh($);
}

async function perform($: EngineInterface, intent: Intent, confirmed = false): Promise<void> {
  // Opening tabs is allowed while a queued mutation is still being processed.
  if (intent.type === 'tab') {
    tab = intent.tab; confirm = null;
    try {
      await show($, true);
      if (!snapshot) await initialize($);
      else await refresh($);
      if (snapshot?.runtime.status === 'running' && !attached) {
        attached = true; await renew($); await $.store.set('reconnect', true);
        await beginExpedition($);
      }
    } catch (e) { error = String(e); $.ui.invalidate('ui.render'); }
    return;
  }
  if (intent.type === 'close') { await $.ui.close({ id: PANE }); opened = false; await $.store.set('pane-open', false); return; }
  if (mutating) { notice = '처리 중입니다 / Working…'; $.ui.invalidate('ui.render'); return; }
  if (!confirmed && (intent.type === 'stop' || intent.type === 'reset' || (intent.type === 'action' && ['sell_common', 'shop_sell'].includes(intent.kind)))) {
    confirm = intent; $.ui.invalidate('ui.render'); return;
  }
  mutating = true;
  connecting = intent.type === 'start';
  error = '';
  $.ui.invalidate('ui.render');
  try {
    if (intent.type === 'migrate' || intent.type === 'use-global') {
      if (intent.type === 'migrate') await bridge($, ['migrate']);
      await bridge($, ['init']); migration = ''; await refresh($); return;
    }
    if (intent.type === 'legacy') {
      const r = await $.process.run(['node', $.plugin.root + '/scripts/vibemud-mod-launcher.cjs', '--legacy', ...intent.args], {
        timeoutMs: 20000, ...(dataDir ? { env: { VIBEMUD_HOME: dataDir } } : {}),
      });
      if (r.exitCode !== 0) throw new Error('외부 창을 열지 못했습니다. / Legacy pane unavailable.');
      if (intent.args[0] === 'start' && session) await bridge($, ['session', 'release', '--session-id', session]);
      attached = false; graceUntil = 0; expedition = null;
      await $.store.set('reconnect', false);
      notice = '외부 창에서 확인하세요. / See the external pane.';
      if (opened) await $.ui.close({ id: PANE });
      opened = false;
      await $.store.set('pane-open', false);
      return;
    }
    if (!snapshot) await initialize($);
    if (migration) return;
    if (intent.type === 'start') {
      await bridge($, ['session', 'ensure', '--session-id', session, ...(activeTurn ? ['--active'] : [])]);
      attached = true;
      await $.store.set('reconnect', true);
      await refresh($);
      tab = snapshot?.settings.intro_seen ? 'adventure' : 'intro';
      await show($, false);
      await beginExpedition($);
      if (!pending && snapshot) {
        pending = { request: session + '-' + (++sequence) + '-' + await $.clock.now(), generation: snapshot.generation, kind: 'hunt_start', payload: {} };
        await submitAction($);
      }
    } else if (intent.type === 'stop') {
      await bridge($, ['session', 'stop', '--session-id', session]);
      attached = false; pending = null; confirm = null; graceUntil = 0; expedition = null;
      await $.store.set('reconnect', false);
      await refresh($);
      notice = '게임 정지 / Game stopped';
    } else if (intent.type === 'setting') {
      await bridge($, ['setting', intent.key, intent.value]);
      if (intent.key === 'intro_seen') tab = 'adventure';
      await refresh($);
    } else if (intent.type === 'reset' && snapshot) {
      epoch++;
      await bridge($, ['reset', '--generation', snapshot.generation, '--yes']);
      attached = false; pending = null; confirm = null; selection = ''; expedition = null; outcome = null; graceUntil = 0; notice = '초기화 완료 / Reset complete';
      await $.store.set('reconnect', false);
      await refresh($); tab = 'intro';
    } else if (intent.type === 'action' && snapshot) {
      if (pending) throw new Error('이전 명령 처리 후 다시 시도하세요. / Wait for the pending action.');
      pending = { request: session + '-' + (++sequence) + '-' + await $.clock.now(), generation: snapshot.generation, kind: intent.kind, payload: intent.payload };
      await submitAction($);
    }
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
  } finally {
    mutating = false; connecting = false; confirm = null;
    $.ui.invalidate('ui.render');
  }
}

async function command($: EngineInterface, input: string): Promise<void> {
  try {
    if (!interactive) { $.ui.log('VibeMUD: open /mud in an interactive Claude Code terminal.'); return; }
    const intent = parseCommand(input);
    if (intent.type !== 'close' && intent.type !== 'legacy') await show($, intent.type !== 'start');
    await perform($, intent, intent.type === 'stop');
  } catch (e) {
    error = e instanceof Error ? e.message : String(e);
    $.ui.invalidate('ui.render');
  }
}

export function register(on: On) {
  on('session.start', async ($, e, next) => {
    interactive = e.isInteractive && (e.surface === 'terminal' || e.surface === 'desktop');
    session = await $.session.id();
    // This flag suppresses only duplicate legacy activity heartbeats, never its firewall.
    await $.env.set('VIBEMUD_MOD_ACTIVE', '1');
    $.clock.every(1000, async () => { now = await $.clock.now(); if (opened && snapshot) await refresh($); });
    $.clock.every(5000, async () => { await renew($); });
    const commands = await $.command.list();
    const existing = commands.find(c => c.name === 'mud');
    ownsShortCommand = !existing || existing.plugin === 'vibemud';
    if (ownsShortCommand) {
      await $.command.register({ name: 'mud', description: 'VibeMUD 게임 창 / game pane', argumentHint: 'start|c|i|m|q|set|end|legacy', immediate: true });
    }
    // Store only interface preferences. A reload reconnects to an existing
    // runtime but never starts a stopped game without the user's Start action.
    if (interactive && (await $.store.get('pane-open') || await $.store.get('reconnect'))) {
      try {
        await initialize($);
        attached = snapshot?.runtime.status === 'running';
        await renew($);
        if (await $.store.get('pane-open')) await show($, false);
      } catch (e) { error = String(e); }
    }
    return next(e);
  });

  on('command.run', { command: ['mud', 'vibemud:mud'] }, async ($, e, next) => {
    if (e.command === 'mud' && !ownsShortCommand) return next(e);
    await command($, e.args);
    return {};
  }).catch(async ($) => { $.ui.toast('VibeMUD 오류: /mud로 다시 열어 주세요.'); return {}; });

  on('turn.start', async ($, e, next) => {
    activeTurn = e.turnId; graceUntil = 0;
    // Metadata only: never read e.text, model replies, tool arguments or files.
    $.clock.after(0, async () => { await renew($); await beginExpedition($); });
    return next(e);
  });
  on('turn.complete', async ($, e, next) => {
    if (e.turnId === activeTurn && !e.agentId) {
      activeTurn = '';
      const run = expedition;
      now = await $.clock.now();
      const finishedAt = now;
      graceUntil = attached && !e.isAborted && e.reason === 'answer' ? now + 15000 : 0;
      $.clock.after(0, async () => {
        await renew($);
        if (!run?.baseline) return;
        try {
          const fresh = decodeSnapshot(await bridge($, ['snapshot']));
          if (expedition === run && fresh.generation === run.generation && fresh.earnings) {
            outcome = { earnings: earnedSince(run.baseline, fresh.earnings), duration: Math.max(0, finishedAt - run.started), aborted: e.isAborted };
            expedition = null;
            $.ui.invalidate('ui.render');
          }
        } catch { /* The game keeps its durable counters; no synthetic rewards. */ }
      });
      const deadline = graceUntil;
      if (deadline) $.clock.after(15000, async () => {
        if (graceUntil === deadline && !activeTurn) { graceUntil = 0; await renew($); }
      });
    }
    return next(e);
  });
  on('session.end', async ($, e, next) => {
    activeTurn = ''; graceUntil = 0; expedition = null;
    if (attached && session) {
      // A killed process is also recovered by the 30-second lease expiry.
      try { await bridge($, ['session', 'detach', '--session-id', session], 700); } catch { /* expiry */ }
    }
    attached = false;
    return next(e);
  });
  on('classic.SessionStart', { source: ['clear', 'resume', 'fork'] }, async ($, e, next) => {
    epoch++; session = await $.session.id(); activeTurn = ''; graceUntil = 0; expedition = null; outcome = null; pending = null; confirm = null;
    if (snapshot || await $.store.get('reconnect')) {
      try {
        await initialize($);
        attached = snapshot?.runtime.status === 'running';
        await renew($);
        if (await $.store.get('pane-open')) await show($, false);
      } catch (e) { error = String(e); }
    }
    return next(e);
  });
  on('ui.close', { id: PANE }, async ($, e, next) => {
    opened = false;
    if (e.origin.kind !== 'unload') await $.store.set('pane-open', false);
    return next(e);
  });

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const original = await next(e);
    if (e.props.hasSurvey || opened || !attached || !snapshot || (e.surface !== 'terminal' && e.surface !== 'desktop')) return original;
    const { Box, Text, Button } = $.ui.resolve(e);
    const ko = snapshot.settings.language !== 'en';
    return Box({ flexDirection: 'column', children: [original, Box({ flexDirection: 'row', columnGap: 1, children: [
      Text({ color: 'cyan', children: [heartbeatError ? 'VibeMUD · 연결 확인 필요' : `VibeMUD · ${label(snapshot.game.player.mode, ko)}${activeTurn ? ' · FEVER' : ''}`] }),
      Button({ key: 'reopen-game', label: ko ? '모험 펼치기' : 'Open adventure', onPress: async () => { await perform($, { type: 'tab', tab: 'adventure' }); } }),
    ] })] });
  });

  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== PANE || (e.surface !== 'terminal' && e.surface !== 'desktop')) return next(e);
    const { Box, Text, Button, Input } = $.ui.resolve(e);
    const ko = snapshot?.settings.language !== 'en';
    const word = (a: string, b: string) => ko ? a : b;
    const width = Math.max(16, Math.floor(e.props.bodyColumns));
    const narrow = width < 38;
    const text = (s: string, color?: string) => Text({ ...(color ? { color } : {}), children: [s] });
    const muted = (s: string) => Text({ dimColor: true, children: [s] });
    const button = (key: string, title: string, intent: Intent) => Button({ key, label: title, onPress: async () => { await perform($, intent); } });
    const action = (key: string, title: string, kind: string, payload: Payload = {}) => pending || mutating || !attached
      ? muted(title) : button(key, title, { type: 'action', kind, payload });
    const row = (cells: (string | RenderElement)[], sizes: number[], heading = false) => Box({ flexDirection: 'row', columnGap: 1, children: cells.map((cell, i) => Box({
      width: sizes[i], flexShrink: 0, overflow: 'hidden', justifyContent: i ? 'flex-end' : 'flex-start',
      children: [typeof cell === 'string' ? Text({ bold: heading, dimColor: heading, wrap: 'truncate-end', children: [cell] }) : cell],
    })) });
    const rule = (title: string) => {
      const head = clip('─ ' + title + ' ', width);
      return Text({ bold: true, color: 'cyan', children: [head + '─'.repeat(Math.max(0, width - cellWidth(head)))] });
    };
    const controls = (children: RenderElement[]) => Box({ flexDirection: 'row', flexWrap: 'wrap', columnGap: 1, children });
    const nameButton = (key: string, title: string, size: number, press: () => void | Promise<void>) => Button({ key, plain: true, label: clip(title, size), onPress: press });
    const chooseAction = (key: string, title: string, size: number, kind: string, payload: Payload) => pending || mutating || !attached
      ? muted(clip(title, size)) : nameButton(key, title, size, async () => { await perform($, { type: 'action', kind, payload }); });
    const pager = (key: string, count: number, size: number) => {
      const total = Math.max(1, Math.ceil(count / size));
      const page = Math.max(0, Math.min(pages[key] || 0, total - 1)); pages[key] = page;
      const buttons = [muted(`${page + 1}/${total}`)];
      for (const [step, title] of [[-1, word('이전', 'Prev')], [1, word('다음', 'Next')]] as const) {
        if (page + step >= 0 && page + step < total) buttons.push(Button({ key: key + '-page-' + step, label: title, onPress: () => { pages[key] = page + step; $.ui.invalidate('ui.render'); } }));
      }
      return { start: page * size, element: total > 1 ? controls(buttons) : null };
    };
    const body: RenderElement[] = [];
    const s = snapshot; const p = s?.game.player;
    if (s && p) {
      const health = p.max_hp > 0 ? p.hp / p.max_hp : 0;
      const state = connecting ? word('연결 중…', 'Connecting…') : heartbeatError || pollError ? word('연결 확인 필요', 'Connection lost')
        : !attached ? word('정지 · 시작 필요', 'Stopped · press Start') : activeTurn ? word('코딩 감지 · 피버 ×1.5', 'Coding · FEVER ×1.5')
        : now < graceUntil ? word('피버 여운 ', 'Afterglow ') + Math.max(0, Math.ceil((graceUntil - now) / 1000)) + 's'
        : s.fever ? word('보너스 피버 ×1.5', 'Bonus FEVER ×1.5') : word('연결됨 · 코딩 대기', 'Connected · ready');
      body.push(Text({ bold: true, children: [clip(`Lv.${p.level} ${p.name} · ${label(p.class_id, ko)}`, width)] }));
      body.push(text(clip((heartbeatError || pollError ? '! ' : '◆ ') + state, width), heartbeatError || pollError ? 'red' : activeTurn || s.fever ? 'yellow' : 'cyan'));
      const meters = columns(width, [12]);
      body.push(row([text(`HP ${hpBar(p.hp, p.max_hp, Math.max(4, meters[0]! - 4))}`, health < .3 ? 'red' : 'green'), `${p.hp}/${p.max_hp}`], meters));
      body.push(row([text(`XP ${hpBar(p.xp, p.xp_to_next, Math.max(4, meters[0]! - 4))}`, 'cyan'), `${Math.min(100, Math.floor(p.xp / Math.max(1, p.xp_to_next) * 100))}% · ${number(p.gold)}G`], meters));
    }
    const tabs: [Tab, string, string][] = [['adventure', '모험', 'Play'], ['character', '캐릭터', 'Hero'], ['inventory', '장비', 'Items'], ['map', '지도', 'Map'], ['quests', '퀘스트', 'Quests'], ['settings', '설정', 'Settings']];
    const tabColumns = narrow ? 2 : 3;
    const tabSize = Math.floor((width - tabColumns + 1) / tabColumns);
    for (let i = 0; i < tabs.length; i += tabColumns) body.push(row(tabs.slice(i, i + tabColumns).map(([t, a, b]) => nameButton('tab-' + t, (tab === t ? '> ' : '  ') + word(a, b), tabSize, async () => { await perform($, { type: 'tab', tab: t }); })), Array(tabColumns).fill(tabSize)));
    if (migration) {
      body.push(rule(word('기존 모험 이어하기', 'Continue your adventure')), text(word('기존 저장을 사용자 데이터 폴더로 복사할 수 있습니다. 원본은 보존합니다.', 'Copy the existing save to your user data folder. The original is kept.')),
        muted(migration), button('migrate', word('기존 게임 복사', 'Copy existing game'), { type: 'migrate' }), button('use-global', word('새 게임', 'New game'), { type: 'use-global' }));
    } else if (!s || !p) {
      body.push(rule('PLANET 64'), text(word('코딩하는 동안 함께 떠나는 모험', 'An adventure alongside your coding')),
        connecting ? text(word('저장 불러오는 중…', 'Loading your save…'), 'yellow') : button('start', word('모험 시작', 'Start adventure'), { type: 'start' }));
    } else {
      if (!attached && tab !== 'adventure') body.push(button('start', connecting ? word('연결 중…', 'Connecting…') : word('모험 시작 / 다시 연결', 'Start / reconnect'), { type: 'start' }));
      if (tab === 'adventure') {
        const combat = s.game.combat;
        body.push(rule(word('탐험', 'Adventure')));
        body.push(text(clip(s.location || p.current_dungeon_id || p.current_area_id || word('마을', 'Town'), width)));
        body.push(muted(label(p.mode, ko) + ` · MP ${p.mp}/${p.max_mp}`));
        if (combat.in_combat) {
          const cols = columns(width, [12]);
          body.push(row([combat.monster_name || 'Monster', `${combat.monster_hp}/${combat.monster_max_hp}`], cols));
          body.push(text(hpBar(combat.monster_hp || 0, combat.monster_max_hp || 1, Math.min(width, 24)), 'red'));
        }
        // A framed scene keeps the geometry stable as combat phases change.
        if (s.scene?.length && width >= 30) body.push(Box({ borderStyle: 'round', borderColor: s.fever ? 'yellow' : 'cyan', width, flexDirection: 'column', children: s.scene.map(line => text(clip(line, width - 2), s.fever ? 'yellow' : 'cyan')) }));
        if (p.current_dungeon_id) {
          const kills = Number(s.progress.dungeon_kills || 0);
          body.push(text(word('보스 접근 ', 'Boss approach ') + hpBar(kills, 5, 8) + ` ${Math.min(kills, 5)}/5`, 'magenta'));
        }
        if (s.progress.recovery_until) body.push(text(word('회복 중 · 체력이 차면 모험을 재개합니다.', 'Recovering · adventure resumes when ready.'), 'yellow'));
        body.push(controls([...(attached ? [] : [button('start', connecting ? word('연결 중…', 'Connecting…') : word('모험 시작', 'Start'), { type: 'start' })]),
          ...(attached ? [action('hunt', word('사냥', 'Hunt'), 'hunt_start')] : []), action('rest', word('휴식', 'Rest'), 'rest'), action('town', word('귀환', 'Town'), 'town')]));
        const earnings = activeTurn ? (expedition?.baseline && s.earnings && expedition.generation === s.generation ? earnedSince(expedition.baseline, s.earnings) : undefined) : outcome?.earnings;
        body.push(rule(activeTurn ? word('이번 원정 · 진행 중', 'This expedition · live') : word('지난 원정', 'Last expedition')));
        if (earnings) {
          const cols = columns(width, [12]);
          body.push(row([word('처치 / 획득', 'Kills / loot'), `${earnings.kills} / ${earnings.items}`], cols));
          body.push(row([`XP +${number(earnings.xp)}`, `+${number(earnings.gold)}G`], cols));
          if (earnings.last_item) body.push(text('◇ ' + earnings.last_item, 'magenta'));
          if (!activeTurn && outcome) body.push(muted(duration(outcome.duration) + word(outcome.aborted ? ' · 중단한 작업' : ' · 코딩 중 획득', outcome.aborted ? ' · interrupted' : ' · earned while coding')));
        } else body.push(muted(word('Claude 작업이 시작되면 원정을 기록합니다.', 'Your next Claude turn begins an expedition.')));
        body.push(rule(word('최근 소식', 'Recent events')));
        body.push(...s.game.recent_log.slice(0, 3).reverse().map(line => text(clip(line, width))));
      } else if (tab === 'character') {
        body.push(rule(word('능력치', 'Attributes')));
        const cols = columns(width, [10]);
        body.push(row([word('능력', 'Attribute'), word('수치', 'Value')], cols, true));
        for (const [key, value] of [['attack', p.attack], ['defense', p.defense], ['accuracy', p.accuracy], ['evasion', p.evasion], ['speed', p.speed], ['regen', p.regen], ['luck', p.luck]] as const) body.push(row([label(key, ko), number(value)], cols));
        body.push(row(['MP', `${p.mp}/${p.max_mp}`], cols), row(['XP', `${number(p.xp)}/${number(p.xp_to_next)}`], cols));
        body.push(controls([button('party', word('파티', 'Party'), { type: 'tab', tab: 'party' }), button('skills', word('스킬', 'Skills'), { type: 'tab', tab: 'skills' }), button('shop', word('상점', 'Shop'), { type: 'tab', tab: 'shop' })]));
      } else if (tab === 'inventory') {
        body.push(rule(word('소지품 ', 'Inventory ') + `${s.game.inventory.length}/20`));
        const cols = columns(width, narrow ? [6] : [6, 5]);
        body.push(row(narrow ? [word('장비 · 선택하여 상세 보기', 'Item · select for details'), word('등급', 'Rarity')] : [word('장비', 'Item'), word('등급', 'Rarity'), word('강화', 'Boost')], cols, true));
        const page = pager('inventory', s.game.inventory.length, 6);
        for (const item of s.game.inventory.slice(page.start, page.start + 6)) {
          const marker = selection === item.id ? '> ' : item.equipped_slot ? '* ' : item.locked ? '# ' : '  ';
          const cells = [nameButton('item-' + item.id, marker + item.name, cols[0]!, () => { selection = item.id; $.ui.invalidate('ui.render'); }), text(label(item.rarity, ko), rarityColor(item.rarity))];
          if (!narrow) cells.push(text('+' + item.enhancement_level));
          body.push(row(cells, cols));
        }
        if (!s.game.inventory.length) body.push(muted(word('아직 소지품이 없습니다.', 'Your bag is empty.')));
        if (page.element) body.push(page.element);
        body.push(muted(word('* 착용   # 잠금   > 선택', '* Equipped   # Locked   > Selected')));
        const item = s.game.inventory.find(i => i.id === selection);
        if (item) {
          body.push(rule(word('장비 상세', 'Item details')), Text({ bold: true, color: rarityColor(item.rarity), children: [item.name] }));
          const detail = columns(width, [12]);
          for (const pair of [[word('종류', 'Type'), label(item.item_type, ko)], [word('등급 / 강화', 'Rarity / boost'), `${label(item.rarity, ko)} +${item.enhancement_level}`], [word('수량', 'Quantity'), number(item.quantity)]]) body.push(row(pair, detail));
          for (const [key, value] of [[item.stat1_type, item.stat1_value], [item.stat2_type, item.stat2_value], [item.stat3_type, item.stat3_value]] as const) if (key && value != null) body.push(row([label(key, ko), '+' + value], detail));
          const equipped = s.game.inventory.find(i => i.equipped_slot === item.item_type);
          if (equipped && equipped.id !== item.id) body.push(row([word('전투력 비교', 'Power comparison'), `${equipped.power_score || 0} → ${item.power_score || 0}`], detail));
          const upgrade = s.enhancements?.[item.id];
          if (upgrade?.cost != null && upgrade.success_rate != null) {
            body.push(row([word('강화 비용', 'Upgrade cost'), number(upgrade.cost) + 'G'], detail), row([word('성공 / 실패 하락', 'Success / loss'), `${Math.round(upgrade.success_rate * 100)}% / -${upgrade.failure_drop}`], detail));
          }
          body.push(controls([action('equip', word('착용', 'Equip'), 'equip', { item_id: item.id }), action('enhance', word('강화', 'Enhance'), 'enhance', { item_id: item.id }), action('lock', word(item.locked ? '잠금 해제' : '잠금', item.locked ? 'Unlock' : 'Lock'), item.locked ? 'item_unlock' : 'item_lock', { item_id: item.id }), action('sell', word('판매', 'Sell'), 'shop_sell', { item_id: item.id })]));
          if (item.item_type === 'weapon') body.push(action('equip-subweapon', word('보조 무기 착용', 'Equip offhand'), 'equip', { item_id: item.id, equip_slot: 'subweapon' }));
          if (item.equipped_slot) body.push(action('unequip', word('착용 해제', 'Unequip'), 'unequip', { equip_slot: item.equipped_slot }));
        }
        body.push(action('sell-common', word('일반 장비 정리', 'Sell common'), 'sell_common', { rarity: 'Common' }));
      } else if (tab === 'map') {
        const cols = columns(width, [6, 6]);
        for (const [id, title, destinations, kind] of [['areas', word('사냥터', 'Fields'), s.areas, 'hunt_start'], ['dungeons', word('던전', 'Dungeons'), s.dungeons, 'dungeon_enter']] as const) {
          body.push(rule(title), row([word('목적지', 'Destination'), 'Lv.', word('이동', 'Go')], cols, true));
          const page = pager(id, destinations.length, 5);
          for (const place of destinations.slice(page.start, page.start + 5)) body.push(row([
            nameButton(id + '-detail-' + place.id, (p.current_area_id === place.id || p.current_dungeon_id === place.id ? '* ' : '') + place.name, cols[0]!, () => { selection = place.id; $.ui.invalidate('ui.render'); }),
            text(String(place.level), place.level > p.level ? 'yellow' : 'green'), chooseAction((id === 'areas' ? 'area-' : 'dungeon-') + place.id, word('입장', 'Enter'), cols[2]!, kind, id === 'areas' ? { area_id: place.id } : { dungeon_id: place.id }),
          ], cols));
          if (page.element) body.push(page.element);
        }
        const place = [...s.areas, ...s.dungeons].find(a => a.id === selection);
        if (place) body.push(rule(word('목적지 상세', 'Destination details')), text(place.name), text(word('권장 레벨 ', 'Recommended level ') + place.level));
        body.push(action('retreat', word('던전 퇴각', 'Retreat'), 'dungeon_retreat'));
      } else if (tab === 'quests') {
        body.push(rule(word('일일 퀘스트', 'Daily quests')));
        const cols = columns(width, [8, 6]);
        body.push(row([word('목표', 'Goal'), word('진행', 'Progress'), word('상태', 'State')], cols, true));
        for (const q of s.quests) body.push(row([nameButton('quest-detail-' + q.id, q.title, cols[0]!, () => { selection = q.id; $.ui.invalidate('ui.render'); }), `${q.progress}/${q.target}`, q.status === 'completed' ? chooseAction('quest-' + q.id, word('수령', 'Claim'), cols[2]!, 'quest_claim', { quest_id: q.id }) : label(q.status, ko)], cols));
        const q = s.quests.find(q => q.id === selection);
        if (q) body.push(rule(word('퀘스트 상세', 'Quest details')), text(q.title), text(hpBar(q.progress, q.target, Math.min(width, 20)), 'green'), text(`${label(q.reward_kind, ko)} +${q.reward_amount} · FEVER +${q.fever_minutes}m`));
        body.push(action('claim-all', word('완료 보상 모두 받기', 'Claim all completed'), 'quest_claim_all'));
      } else if (tab === 'party') {
        body.push(rule(word('동행하는 파티', 'Active party')));
        const cols = columns(width, [8]);
        body.push(row([word('이름', 'Name'), word('역할', 'Role')], cols, true));
        for (const c of s.game.party) body.push(row([c.name, label(c.role, ko)], cols));
        body.push(action('recruit', word('동료 모집', 'Recruit'), 'party_recruit'), rule(word('동료 배치 · 슬롯 선택', 'Assign a companion · slot')));
        const slots = columns(width, [3, 3, 3]);
        body.push(row([word('동료', 'Companion'), '1', '2', '3'], slots, true));
        for (const c of s.companions.filter(c => c.unlocked)) body.push(row([nameButton('companion-' + c.id, c.name, slots[0]!, () => { selection = c.id; $.ui.invalidate('ui.render'); }), ...[1, 2, 3].map((slot, i) => chooseAction(`swap-${c.id}-${slot}`, String(slot), slots[i + 1]!, 'party_swap', { slot, companion_id: c.id }))], slots));
        const c = s.companions.find(c => c.id === selection);
        if (c) body.push(text(c.name + ' · ' + label(c.role, ko)));
      } else if (tab === 'skills') {
        body.push(rule(word('전투 스킬', 'Combat skills')));
        const cols = columns(width, [8]);
        body.push(row([word('스킬', 'Skill'), word('행동', 'Action')], cols, true));
        for (const id of ['slash', 'guard', 'taunt', 'heal', 'firebolt']) body.push(row([label(id, ko), chooseAction('skill-' + id, word('사용', 'Use'), cols[1]!, 'skill_use', { skill_id: id })], cols));
      } else if (tab === 'shop') {
        body.push(rule(word('상점', 'Shop')));
        const cols = columns(width, [7, 6]);
        body.push(row([word('상품', 'Product'), word('가격', 'Price'), word('행동', 'Action')], cols, true));
        for (const [id, korean, english, price] of SHOP) body.push(row([nameButton('shop-detail-' + id, word(korean, english), cols[0]!, () => { selection = id; $.ui.invalidate('ui.render'); }), text(number(price) + 'G', p.gold < price ? 'red' : 'yellow'), chooseAction('buy-' + id, word('구매', 'Buy'), cols[2]!, 'shop_buy', { item_id: id })], cols));
        const item = SHOP.find(i => i[0] === selection);
        if (item) body.push(rule(word('상품 상세', 'Product details')), text(word(item[1], item[2])), text(`${item[3]}G`));
      } else if (tab === 'settings') {
        body.push(rule(word('화면 설정', 'Display')));
        const cols = columns(width, [Math.min(24, Math.floor(width / 2))]);
        body.push(row([word('언어', 'Language'), controls([button('language-ko', '한국어', { type: 'setting', key: 'language', value: 'ko' }), button('language-en', 'EN', { type: 'setting', key: 'language', value: 'en' })])], cols));
        body.push(row([word('전투 연출', 'Animation'), button('animations', s.settings.animations ? 'ON' : 'OFF', { type: 'setting', key: 'animations', value: String(!s.settings.animations) })], cols));
        body.push(button('intro', word('오프닝 다시보기', 'Replay intro'), { type: 'tab', tab: 'intro' }));
        body.push(rule(word('게임 관리', 'Game controls')), button('stop', word('게임 정지', 'Stop game'), { type: 'stop' }), button('reset', word('진행 초기화', 'Reset progress'), { type: 'reset' }));
        body.push(muted(word('창 접기는 사냥을 멈추지 않습니다.', 'Hiding the pane keeps the game running.')), muted(word('외부 창: legacy start', 'External pane: legacy start')));
      } else if (tab === 'intro') {
        body.push(rule('PLANET 64 · 2226'), ...(s.intro || []).map(line => text(line)), button('intro-done', word('모험으로', 'Continue'), { type: 'setting', key: 'intro_seen', value: 'true' }));
      } else if (tab === 'help') {
        body.push(rule(word('게임 명령', 'Game commands')));
        const cols = columns(width, [Math.floor(width / 2)]);
        for (const pair of [['start', word('시작 / 다시 연결', 'Start / reconnect')], ['c · i · m · q', word('캐릭터 · 장비 · 지도 · 퀘스트', 'Hero · Items · Map · Quests')], ['shop · party · skill', word('상점 · 파티 · 스킬', 'Shop · Party · Skills')], ['close / hide', word('창 접기 · 게임 계속', 'Hide · game continues')], ['end', word('게임 정지', 'Stop game')], ['Esc', word('코딩으로 돌아가기', 'Return to coding')]]) body.push(row(pair, cols));
      }
    }
    if (error || pollError || heartbeatError) body.push(rule(word('연결 / 명령 안내', 'Connection / action')), text((error || heartbeatError || pollError).slice(0, 600), 'red'));
    if (pending || notice || connecting) body.push(muted(clip(connecting ? word('모험에 연결 중…', 'Connecting…') : pending ? word('명령 처리 중…', 'Processing action…') : notice, width)));
    if (pending && !pending.id && !mutating) body.push(Button({ key: 'retry', label: word('같은 요청 재시도', 'Retry same request'), onPress: async () => {
      if (mutating) return; mutating = true; error = '';
      try { await submitAction($); } catch (e) { error = String(e); } finally { mutating = false; $.ui.invalidate('ui.render'); }
    } }));
    if (confirm) {
      const choice = confirm;
      const question = choice.type === 'stop' ? word('게임을 정지할까요? 진행은 저장됩니다.', 'Stop the game? Progress is saved.') : choice.type === 'reset' ? word('백업 후 모든 진행을 초기화할까요?', 'Back up and reset all progress?')
        : choice.type === 'action' && choice.kind === 'sell_common' ? word('잠금·착용 장비를 제외하고 판매할까요?', 'Sell spare unlocked equipment?') : word('선택한 아이템을 판매할까요?', 'Sell the selected item?');
      body.push(Box({ borderStyle: 'round', borderColor: 'yellow', width, flexDirection: 'column', children: [text(question), controls([
        Button({ key: 'confirm', label: word('확인', 'Confirm'), onPress: async () => { await perform($, choice, true); } }),
        Button({ key: 'cancel', label: word('취소', 'Cancel'), onPress: () => { confirm = null; $.ui.invalidate('ui.render'); } }),
      ])] }));
    }
    body.push(rule(word('조작', 'Controls')));
    if (tab !== 'adventure') body.push(button('back-adventure', word('← 모험으로 돌아가기', '← Back to adventure'), { type: 'tab', tab: 'adventure' }));
    body.push(Input({ key: 'game-command', label: word('명령', 'Command'), value: draft, placeholder: 'help', onInput: value => { draft = value; }, onSubmit: async value => { draft = ''; await command($, value); } }));
    body.push(muted(word('Esc: 코딩으로 · /mud: 작업 중에도 조작', 'Esc: coding · /mud: works during a turn')));
    if (tab === 'adventure') body.push(button('close', word(attached ? '창 접기 · 게임 계속' : '창 접기', attached ? 'Hide · game continues' : 'Hide pane'), { type: 'close' }));
    return Box({ width, flexDirection: 'column', children: body });
  });
}
