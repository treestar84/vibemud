// Pure game input/view data. No Claude conversation, filesystem or host APIs.
export type Tab = 'adventure' | 'character' | 'inventory' | 'map' | 'quests' | 'settings' | 'party' | 'skills' | 'shop' | 'help' | 'intro';
export type Payload = Record<string, string | number>;
export type Intent = { type: 'tab'; tab: Tab } | { type: 'action'; kind: string; payload: Payload } |
  { type: 'start' | 'stop' | 'close' | 'reset' | 'migrate' | 'use-global' } | { type: 'legacy'; args: string[] } |
  { type: 'setting'; key: string; value: string };
export interface Item {
  id: string; item_id: string; item_type: string; name: string; rarity: string; quantity: number; power_score: number | null;
  equipped_slot: string | null; locked: boolean; enhancement_level: number;
  stat1_type: string | null; stat1_value: number | null;
  stat2_type: string | null; stat2_value: number | null;
  stat3_type: string | null; stat3_value: number | null;
}
export interface Player {
  name: string; class_id: string; level: number; hp: number; max_hp: number; mp: number; max_mp: number;
  xp: number; xp_to_next: number; gold: number; attack: number; defense: number; accuracy: number;
  evasion: number; speed: number; regen: number; luck: number; mode: string;
  current_area_id: string | null; current_dungeon_id: string | null;
}
export interface Snapshot {
  generation: string;
  scene: string[]; intro: string[]; location: string;
  runtime: { status: string }; fever: boolean;
  earnings?: Earnings;
  game: { player: Player; clock_tick: number; state_version: number; inventory: Item[];
    party: { id: string; name: string; role: string }[];
    combat: { in_combat: boolean; monster_name: string | null; monster_hp: number | null; monster_max_hp: number | null; turn_index: number };
    recent_log: string[] };
  quests: { id: string; title: string; progress: number; target: number; status: string; reward_kind: string; reward_amount: number; fever_minutes: number }[];
  areas: { id: string; name: string; level: number }[];
  dungeons: { id: string; name: string; level: number }[];
  companions: { id: string; name: string; role: string; unlocked: boolean }[];
  commands: { id: string; kind: string; status: string; error: string | null; result: { message: string } | null }[];
  settings: { language: string; intro_seen: boolean; animations: boolean };
  enhancements: Record<string, { cost: number | null; success_rate: number | null; failure_drop: number }>;
  progress: { recovery_until: string | null; dungeon_point: string | null; dungeon_kills: string | null };
}

export interface Earnings { kills: number; xp: number; gold: number; items: number; last_item: string | null }
export function earnedSince(before: Earnings, after: Earnings): Earnings {
  return { kills: Math.max(0, after.kills - before.kills), xp: Math.max(0, after.xp - before.xp),
    gold: Math.max(0, after.gold - before.gold), items: Math.max(0, after.items - before.items),
    last_item: after.items > before.items ? after.last_item : null };
}

export function decode(stdout: string): unknown {
  const envelope = JSON.parse(stdout);
  if (envelope.protocol_version !== 1) throw new Error('VibeMUD CLI / mod protocol mismatch. Update both together.');
  if (envelope.ok !== true) {
    const message = String(envelope.error || 'VibeMUD operation failed');
    if (envelope.retryable === true) throw new Error(message);
    throw new RejectedAction(message);
  }
  return envelope.data;
}

export class RejectedAction extends Error {}

export function decodeSnapshot(data: unknown): Snapshot {
  const s = data as Snapshot;
  if (!s || typeof s.generation !== 'string' || !s.game?.player || !s.game.combat ||
    !Array.isArray(s.game.recent_log) || !Array.isArray(s.game.party) || !Array.isArray(s.game.inventory) ||
    !Array.isArray(s.quests) || !Array.isArray(s.areas) || !Array.isArray(s.dungeons) ||
    !Array.isArray(s.companions) || !Array.isArray(s.commands) || !s.settings || !s.runtime || !s.progress) {
    throw new Error('Invalid VibeMUD snapshot. Update the native CLI and mod together.');
  }
  return s;
}

export function parseCommand(input: string): Intent {
  if (input.length > 1024) throw new Error('명령이 너무 깁니다. / Command too long.');
  // Tokens are data, never interpreted by a shell. IDs intentionally have no spaces.
  const parts = input.trim().replace(/^\/(?:vibemud:)?mud(?:\s+|$)/i, '').split(/\s+/).filter(Boolean);
  const c = (parts.shift() || 'panel').toLowerCase();
  const a = parts[0]; const b = parts[1];
  const required = (v: string | undefined) => { if (!v) throw new Error('인수가 필요합니다. help를 확인하세요. / Missing argument.'); return v; };
  const action = (kind: string, payload: Payload = {}): Intent => ({ type: 'action', kind, payload });
  const tab = (name: Tab): Intent => ({ type: 'tab', tab: name });
  if (['start', '시작', 'play', '플레이'].includes(c)) return { type: 'start' };
  if (['end', 'stop', 's', '종료', '정지', 'pause', '중지'].includes(c)) return { type: 'stop' };
  if (['close', 'hide', 'x', '닫기', '접기'].includes(c)) return { type: 'close' };
  if (['back', '뒤로', '모험'].includes(c)) return tab('adventure');
  if (['panel', 'now', 'status', 'system', '상태', 'log', 'tail', 'queue'].includes(c)) return tab('adventure');
  if (['c', 'character', '캐릭터', 'stats', 'full-status'].includes(c)) return tab('character');
  if (['i', 'inventory', 'item', 'items', '장비', '소지품', '가방'].includes(c)) return tab('inventory');
  if (['m', 'map', 'menu', '지도', '메뉴', '지역', '던전'].includes(c)) return a ? action('hunt_start', { area_id: a }) : tab('map');
  if (['q', 'quest', 'quests', '퀘스트', '일일퀘스트'].includes(c)) {
    if (a === 'claim') return action('quest_claim', { quest_id: required(b) });
    if (a === 'claim-all') return action('quest_claim_all');
    return tab('quests');
  }
  if (['set', 'settings', 'setting', '설정', '환경설정'].includes(c)) {
    if (a === 'reset' || a === '초기화') return { type: 'reset' };
    if (a === 'language' || a === 'animations') return { type: 'setting', key: a, value: required(b) };
    return tab('settings');
  }
  if (['reset', '리셋', '초기화', '새게임', 'newgame'].includes(c)) return { type: 'reset' };
  if (['intro', 'opening', '오프닝', '시나리오', '스토리'].includes(c)) return tab('intro');
  if (['help', '도움말', '가이드'].includes(c)) return tab('help');
  if (c === 'legacy') {
    if (!['start', 'end', 'c', 'i', 'm', 'q', 'set', 'panel', 'help'].includes(a || '') || parts.length !== 1) throw new Error('legacy start|end|c|i|m|q|set|panel|help');
    return { type: 'legacy', args: parts };
  }
  if (c === 'a') return action('hunt_start', a ? { area_id: a } : {});
  if (c === 'hunt') {
    if (a === 'stop') return action('hunt_stop');
    if (a === 'start') return action('hunt_start', b ? { area_id: b } : {});
    throw new Error('hunt start [area] | hunt stop');
  }
  if (c === 'area') return a === 'enter' ? action('area_enter', { area_id: required(b) }) : tab('map');
  if (c === 'dungeon') {
    if (a === 'enter') return action('dungeon_enter', { dungeon_id: required(b) });
    if (a === 'retreat') return action('dungeon_retreat');
    return tab('map');
  }
  if (c === 'rest' || c === '휴식') return action('rest');
  if (c === 'town' || c === '귀환') return action('town');
  if (c === 'equip') return action('equip', { item_id: required(a), ...(b ? { equip_slot: b } : {}) });
  if (c === 'unequip') return action('unequip', { equip_slot: required(a) });
  if (c === 'enhance' || c === '강화') return action('enhance', { item_id: required(a) });
  if (c === 'equipment') {
    if (a === 'lock' || a === 'unlock') return action('item_' + a, { item_id: required(b) });
    if (a === 'sell-common') return action('sell_common', { rarity: b || 'Common' });
    if (a === 'empty') return action('sell_common');
    return tab('inventory');
  }
  if (c === 'party') {
    if (a === 'recruit') return action('party_recruit');
    if (a === 'swap') return action('party_swap', { slot: Number(required(b)), companion_id: required(parts[2]) });
    return tab('party');
  }
  if (c === 'skill') return a === 'use' ? action('skill_use', { skill_id: required(b) }) : tab('skills');
  if (c === 'shop') return a === 'buy' || a === 'sell' ? action('shop_' + a, { item_id: required(b) }) : tab('shop');
  throw new Error('알 수 없는 게임 명령입니다. help를 확인하세요. / Unknown game command.');
}

export function hpBar(current: number, max: number, width = 12): string {
  const n = Math.round(Math.max(0, Math.min(1, max > 0 ? current / max : 0)) * width);
  return '█'.repeat(n) + '░'.repeat(width - n);
}

export const SHOP = [
  ['potion-small', '아스클레피오스 물약', 'Potion', 25], ['potion-medium', '히게이아 물약', 'Potion+', 55],
  ['basic-sword', '아레스 검', 'Sword', 80], ['basic-staff', '헤르메스 지팡이', 'Staff', 80],
  ['leather-armor', '레오니다스 갑옷', 'Armor', 90], ['repair-kit', '다이달로스 도구', 'Repair kit', 40],
] as const;
