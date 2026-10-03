// Terminal cell geometry: Korean/CJK occupy two columns, combining marks none.
// Layout uses Box widths (also on Desktop); this only clips plain button labels.
function glyphWidth(char: string): number {
  const n = char.codePointAt(0)!;
  if (/\p{Mark}/u.test(char) || n === 0x200d || n < 32 || (n >= 0x7f && n < 0xa0)) return 0;
  return n >= 0x1100 && (n <= 0x115f || n === 0x2329 || n === 0x232a ||
    (n >= 0x2e80 && n <= 0xa4cf && n !== 0x303f) || (n >= 0xac00 && n <= 0xd7a3) ||
    (n >= 0xf900 && n <= 0xfaff) || (n >= 0xfe10 && n <= 0xfe6f) ||
    (n >= 0xff01 && n <= 0xff60) || (n >= 0xffe0 && n <= 0xffe6) ||
    (n >= 0x1f000 && n <= 0x1faff) || n >= 0x20000) ? 2 : 1;
}
export function cellWidth(value: string): number {
  return Array.from(value.normalize('NFC')).reduce((sum, c) => sum + glyphWidth(c), 0);
}
export function clip(value: string, width: number): string {
  const clean = value.normalize('NFC').replace(/[\x00-\x1f\x7f-\x9f]/g, ' ');
  if (cellWidth(clean) <= width) return clean;
  let out = ''; let used = 0;
  for (const char of clean) {
    const size = glyphWidth(char);
    if (used + size > width - 1) break;
    out += char; used += size;
  }
  return width > 0 ? out + '…' : '';
}
export function columns(width: number, tails: number[]): number[] {
  const available = Math.max(tails.length + 1, Math.floor(width) - tails.length);
  const fixed = tails.map(n => Math.min(n, Math.floor(available / (tails.length + 2))));
  return [available - fixed.reduce((a, b) => a + b, 0), ...fixed];
}
export function number(value: number): string { return Math.floor(value).toLocaleString('en-US'); }
export function duration(ms: number): string {
  const seconds = Math.max(0, Math.floor(ms / 1000));
  return Math.floor(seconds / 60) + ':' + String(seconds % 60).padStart(2, '0');
}
const labels: Record<string, string> = {
  Common: '일반', Uncommon: '고급', Rare: '희귀', Epic: '영웅', Legendary: '전설', Mythic: '신화',
  warrior: '전사', healer: '치유사', mage: '마법사', tank: '수호자', damage: '공격', support: '지원',
  idle: '대기', auto_hunt: '자동 사냥', hunting: '사냥', dungeon: '던전 탐험', resting: '휴식', recovery: '회복', town: '마을',
  attack: '공격', defense: '방어', accuracy: '명중', evasion: '회피', speed: '속도', regen: '재생', luck: '행운',
  slash: '베기', guard: '방어', taunt: '도발', heal: '치유', firebolt: '화염탄',
  weapon: '주무기', subweapon: '보조 무기', armor: '갑옷', helmet: '투구', accessory: '장신구', consumable: '소모품',
  active: '진행 중', completed: '달성', claimed: '수령함', gold: '골드', xp: '경험치',
};
export function label(value: string, ko: boolean): string { return ko ? labels[value] || value : value; }
export function rarityColor(rarity: string): string {
  return ({ Rare: 'cyan', Epic: 'magenta', Legendary: 'yellow', Mythic: 'red', Uncommon: 'green' } as Record<string, string>)[rarity] || 'white';
}
