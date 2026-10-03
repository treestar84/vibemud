use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_PLAYER_ID: &str = "player";
pub const DEFAULT_HUD_ID: &str = "main";
pub const DEFAULT_SESSION_ID: &str = "main";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandClass {
    ReadOnly,
    Mutation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandKind {
    Status,
    FullStatus,
    AreaList,
    AreaEnter,
    HuntStart,
    HuntStop,
    DungeonList,
    DungeonEnter,
    DungeonRetreat,
    Party,
    PartyRecruit,
    PartySwap,
    Inventory,
    Equip,
    Unequip,
    Enhance,
    SkillList,
    SkillUse,
    Shop,
    ShopBuy,
    ShopSell,
    SellCommon,
    QuestClaim,
    QuestClaimAll,
    ItemLock,
    ItemUnlock,
    Rest,
    Town,
    Log,
    AliasList,
}

impl CommandKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::FullStatus => "full_status",
            Self::AreaList => "area_list",
            Self::AreaEnter => "area_enter",
            Self::HuntStart => "hunt_start",
            Self::HuntStop => "hunt_stop",
            Self::DungeonList => "dungeon_list",
            Self::DungeonEnter => "dungeon_enter",
            Self::DungeonRetreat => "dungeon_retreat",
            Self::Party => "party",
            Self::PartyRecruit => "party_recruit",
            Self::PartySwap => "party_swap",
            Self::Inventory => "inventory",
            Self::Equip => "equip",
            Self::Unequip => "unequip",
            Self::Enhance => "enhance",
            Self::SkillList => "skill_list",
            Self::SkillUse => "skill_use",
            Self::Shop => "shop",
            Self::ShopBuy => "shop_buy",
            Self::ShopSell => "shop_sell",
            Self::SellCommon => "sell_common",
            Self::QuestClaim => "quest_claim",
            Self::QuestClaimAll => "quest_claim_all",
            Self::ItemLock => "item_lock",
            Self::ItemUnlock => "item_unlock",
            Self::Rest => "rest",
            Self::Town => "town",
            Self::Log => "log",
            Self::AliasList => "alias_list",
        }
    }

    pub fn class(&self) -> CommandClass {
        match self {
            Self::Status
            | Self::FullStatus
            | Self::AreaList
            | Self::DungeonList
            | Self::Party
            | Self::Inventory
            | Self::SkillList
            | Self::Shop
            | Self::Log
            | Self::AliasList => CommandClass::ReadOnly,
            _ => CommandClass::Mutation,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandStatus {
    Pending,
    Processing,
    Done,
    Failed,
}

impl CommandStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Processing => "processing",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommandPayload {
    pub area_id: Option<String>,
    pub dungeon_id: Option<String>,
    pub item_id: Option<String>,
    pub rarity: Option<String>,
    pub companion_id: Option<String>,
    pub slot: Option<u8>,
    pub equip_slot: Option<String>,
    pub skill_id: Option<String>,
    pub quest_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub message: String,
    pub state_version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Initialized,
    SessionStarted,
    SessionStopped,
    CommandProcessed,
    TickAdvanced,
    EncounterResolved,
    PlayerDied,
    LevelUp,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Initialized => "initialized",
            Self::SessionStarted => "session_started",
            Self::SessionStopped => "session_stopped",
            Self::CommandProcessed => "command_processed",
            Self::TickAdvanced => "tick_advanced",
            Self::EncounterResolved => "encounter_resolved",
            Self::PlayerDied => "player_died",
            Self::LevelUp => "level_up",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEvent {
    pub kind: EventKind,
    pub message: String,
    pub state_version: u64,
    pub rng_seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerState {
    pub id: String,
    pub name: String,
    pub class_id: String,
    pub level: u32,
    pub xp: u64,
    pub xp_to_next: u64,
    pub hp: i32,
    pub max_hp: i32,
    pub mp: i32,
    pub max_mp: i32,
    pub attack: i32,
    pub defense: i32,
    pub accuracy: i32,
    pub evasion: i32,
    pub speed: i32,
    pub regen: i32,
    pub luck: i32,
    pub gold: i64,
    pub current_area_id: Option<String>,
    pub current_dungeon_id: Option<String>,
    pub mode: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepresentativeStats {
    pub combat_power: i32,
    pub attack: i32,
    pub defense: i32,
    pub vitality: i32,
    pub speed: i32,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            id: DEFAULT_PLAYER_ID.to_string(),
            name: "Arin".to_string(),
            class_id: "warrior".to_string(),
            level: 1,
            xp: 0,
            xp_to_next: xp_to_next(1),
            hp: 120,
            max_hp: 120,
            mp: 30,
            max_mp: 30,
            attack: 18,
            defense: 10,
            accuracy: 70,
            evasion: 8,
            speed: 10,
            regen: 3,
            luck: 5,
            gold: 100,
            current_area_id: Some("training-field".to_string()),
            current_dungeon_id: None,
            mode: "idle".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompanionState {
    pub id: String,
    pub name: String,
    pub role: String,
    pub rarity: String,
    pub unlocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    pub name: String,
    pub recommended_level: u32,
    pub danger_rating: String,
    pub encounter_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dungeon {
    pub id: String,
    pub name: String,
    pub recommended_level: u32,
    pub floors: u32,
    pub boss_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryItem {
    pub id: String,
    pub item_id: String,
    pub item_type: String,
    pub name: String,
    pub rarity: String,
    pub rarity_color: Option<String>,
    pub tier: Option<u8>,
    pub quantity: u32,
    pub durability: Option<i32>,
    pub max_durability: Option<i32>,
    pub equipped_slot: Option<String>,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub enhancement_level: u8,
    pub stat1_type: Option<String>,
    pub stat1_value: Option<i32>,
    pub stat2_type: Option<String>,
    pub stat2_value: Option<i32>,
    #[serde(default)]
    pub stat3_type: Option<String>,
    #[serde(default)]
    pub stat3_value: Option<i32>,
    pub power_score: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CombatState {
    pub in_combat: bool,
    pub encounter_id: Option<String>,
    pub encounter_seed: Option<u64>,
    pub turn_index: u32,
    pub monster_name: Option<String>,
    pub monster_hp: Option<i32>,
    pub monster_max_hp: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameSnapshot {
    pub state_version: u64,
    #[serde(default)]
    pub clock_tick: u64,
    pub player: PlayerState,
    pub party: Vec<CompanionState>,
    pub inventory: Vec<InventoryItem>,
    pub combat: CombatState,
    pub recent_log: Vec<String>,
}

impl GameSnapshot {
    pub fn initial() -> Self {
        Self {
            state_version: 0,
            clock_tick: 0,
            player: PlayerState::default(),
            party: vec![],
            inventory: vec![],
            combat: CombatState::default(),
            recent_log: vec!["Welcome to VibeMUD.".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HudStateDto {
    pub state_version: u64,
    pub one_line: String,
    pub compact_json: String,
    pub full_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusLineDto {
    pub level: u32,
    pub class_label: String,
    pub hp: i32,
    pub max_hp: i32,
    pub area_label: String,
    pub mode_label: String,
    pub party_count: usize,
    pub danger_label: String,
    pub loot_count: usize,
}

impl From<&GameSnapshot> for StatusLineDto {
    fn from(snapshot: &GameSnapshot) -> Self {
        let area_id = snapshot.player.current_area_id.as_deref().unwrap_or("town");
        let (area_label, danger_label) = if snapshot.player.mode == "dungeon" {
            snapshot
                .player
                .current_dungeon_id
                .as_deref()
                .map(|dungeon_id| (dungeon_label(dungeon_id).to_string(), "Dungeon".to_string()))
                .unwrap_or_else(|| {
                    (
                        area_label(area_id).to_string(),
                        danger_for_area(area_id).to_string(),
                    )
                })
        } else {
            (
                area_label(area_id).to_string(),
                danger_for_area(area_id).to_string(),
            )
        };
        Self {
            level: snapshot.player.level,
            class_label: visible_class_label(snapshot).to_string(),
            hp: snapshot.player.hp,
            max_hp: snapshot.player.max_hp,
            area_label,
            mode_label: mode_label(&snapshot.player.mode).to_string(),
            party_count: 1 + snapshot.party.len(),
            danger_label,
            loot_count: snapshot.inventory.len(),
        }
    }
}

pub fn xp_to_next(level: u32) -> u64 {
    (100.0 * (level as f64).powf(1.55)).floor() as u64
}

pub fn hit_chance(accuracy: i32, evasion: i32) -> f64 {
    (0.75 + accuracy as f64 * 0.003 - evasion as f64 * 0.003).clamp(0.35, 0.95)
}

pub fn crit_chance(accuracy: i32) -> f64 {
    (0.05 + accuracy as f64 * 0.001).clamp(0.05, 0.30)
}

pub fn luck_drop_multiplier(luck: i32) -> f64 {
    (1.0 + luck.max(0) as f64 * 0.005).clamp(1.0, 2.0)
}

pub fn resistance_skill_reduction(resistance: i32) -> f64 {
    (resistance.max(0) as f64 * 0.004).clamp(0.0, 0.45)
}

pub fn mitigation(defense: i32) -> f64 {
    defense as f64 / (defense as f64 + 100.0)
}

pub fn damage(attack: i32, skill_multiplier: f64, defense: i32, random_factor: f64) -> i32 {
    let raw = attack as f64 * skill_multiplier;
    let value = (raw * (1.0 - mitigation(defense)) * random_factor).floor() as i32;
    value.max(1)
}

pub fn representative_stats(player: &PlayerState) -> RepresentativeStats {
    RepresentativeStats {
        combat_power: combat_power(player),
        attack: player.attack,
        defense: player.defense,
        vitality: player.max_hp,
        speed: player.speed,
    }
}

pub fn combat_power(player: &PlayerState) -> i32 {
    let offense = player.attack * 5 + player.accuracy * 3;
    let survival = player.defense * 4 + player.max_hp + player.regen * 6;
    let tempo = player.speed * 3 + player.evasion * 3;
    let fortune = player.luck * 2;
    (offense + survival + tempo + fortune + player.max_mp / 2).max(1)
}

pub fn encounter_interval_ms(speed: i32) -> u64 {
    let speed_over_baseline = speed.saturating_sub(10).max(0) as u64;
    let reduction_ms = (speed_over_baseline * 200).min(8_000);
    20_000_u64.saturating_sub(reduction_ms).max(12_000)
}

pub fn encounter_interval_ticks(speed: i32, tick_interval_ms: u64) -> u32 {
    let tick = tick_interval_ms.max(250);
    let interval = encounter_interval_ms(speed);
    interval.div_ceil(tick).max(1) as u32
}

pub fn encounter_chance(area: &Area, dungeon_mode: bool) -> f64 {
    let danger_modifier = match area.danger_rating.as_str() {
        "Safe" => -0.10,
        "Low" => -0.05,
        "Normal" => 0.0,
        "High" => 0.08,
        "Deadly" => 0.15,
        _ => 0.0,
    };
    let dungeon_modifier = if dungeon_mode { 0.10 } else { 0.0 };
    ((area.encounter_rate + danger_modifier + dungeon_modifier) * 0.80).clamp(0.10, 0.85)
}

pub fn should_trigger_encounter(area: &Area, dungeon_mode: bool, seed: u64) -> bool {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0xEC0A_7A11_D15C_A11E);
    rng.gen::<f64>() <= encounter_chance(area, dungeon_mode)
}

pub fn area_by_id(area_id: &str) -> Area {
    default_areas()
        .into_iter()
        .find(|area| area.id == area_id)
        .unwrap_or_else(|| area("training-field", "Training Field", 1, "Safe", 0.35))
}

pub fn apply_xp(player: &mut PlayerState, gained: u64) -> Vec<String> {
    let mut logs = Vec::new();
    player.xp += gained;
    while player.xp >= player.xp_to_next {
        player.xp -= player.xp_to_next;
        player.level += 1;
        player.max_hp += 18;
        player.max_mp += 5;
        player.attack += 3;
        player.defense += 2;
        player.accuracy += 1;
        player.evasion += 1;
        // Monster damage grows linearly with level; regen must grow too or
        // late-game field sustain collapses into forced death/recovery loops.
        if player.level % 3 == 0 {
            player.regen += 1;
        }
        player.hp = (player.hp + player.max_hp / 3).min(player.max_hp);
        player.mp = (player.mp + player.max_mp / 3).min(player.max_mp);
        player.xp_to_next = xp_to_next(player.level);
        logs.push(format!("Level up! Lv.{}", player.level));
    }
    logs
}

pub fn death_penalty(player: &mut PlayerState) -> Vec<String> {
    let mut logs = vec![
        "You fell in battle and returned to town.".to_string(),
        "Recovery started for 1 minute in town.".to_string(),
    ];
    player.mode = "recovering".to_string();
    player.current_dungeon_id = None;
    player.current_area_id = Some("town".to_string());
    player.hp = 1;
    player.mp = 0;
    if (6..=15).contains(&player.level) {
        let loss = ((player.gold as f64) * 0.03).floor() as i64;
        player.gold = (player.gold - loss).max(0);
        logs.push(format!("Lost {loss} gold."));
    } else if player.level >= 16 {
        let loss = ((player.gold as f64) * 0.06).floor() as i64;
        player.gold = (player.gold - loss).max(0);
        logs.push(format!("Lost {loss} gold and current dungeon progress."));
    }
    logs
}

pub fn resolve_auto_hunt_tick(player: &mut PlayerState, seed: u64) -> Vec<String> {
    let area_id = player
        .current_area_id
        .clone()
        .unwrap_or_else(|| "training-field".to_string());
    resolve_auto_hunt_tick_with_monster(
        player,
        seed,
        normal_monster_for_seed(seed),
        danger_bonus(&area_id),
    )
}

pub fn resolve_auto_hunt_tick_with_monster(
    player: &mut PlayerState,
    seed: u64,
    monster: &str,
    difficulty_bonus: i32,
) -> Vec<String> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let monster_def = 5 + player.level as i32;
    let monster_attack = (8 + player.level as i32 * 2 + difficulty_bonus).max(1);
    let hit_roll: f64 = rng.gen();
    let mut logs = vec![format!("{monster} appeared.")];
    if hit_roll <= hit_chance(player.accuracy, 8 + difficulty_bonus) {
        let crit = rng.gen::<f64>() <= crit_chance(player.accuracy);
        let random_factor = rng.gen_range(0.85..=1.15);
        let mut dealt = damage(player.attack, 1.0, monster_def, random_factor);
        if crit {
            dealt = ((dealt as f64) * 1.5).floor() as i32;
        }
        logs.push(format!("Warrior hit {monster} for {dealt}."));
        let xp = 24 + player.level as u64 * 4 + difficulty_bonus.max(0) as u64;
        let gold = 8 + player.level as i64 * 2 + difficulty_bonus.max(0) as i64;
        player.gold += gold;
        logs.push(format!("{monster} defeated. +{xp} XP, +{gold} gold."));
        logs.extend(apply_xp(player, xp));
    } else {
        logs.push(format!("Warrior missed {monster}."));
    }
    let incoming = damage(
        monster_attack,
        1.0,
        player.defense,
        rng.gen_range(0.85..=1.15),
    );
    player.hp -= incoming;
    logs.push(format!("{monster} hit Warrior for {incoming}."));
    if player.hp <= 0 {
        logs.extend(death_penalty(player));
    } else {
        player.hp = (player.hp + player.regen).min(player.max_hp);
        player.mp = (player.mp + (player.regen / 2)).min(player.max_mp);
    }
    logs
}

pub fn simulate(area_id: &str, hours: u32, runs: u32, seed: u64) -> SimulationSummary {
    let mut deaths = 0u32;
    let mut total_xp = 0u64;
    let mut total_gold = 0i64;
    let ticks = (hours.max(1) * 3600).min(86_400);
    let runs = runs.max(1);
    let mut hash_input = String::new();
    for run in 0..runs {
        let mut player = PlayerState {
            current_area_id: Some(area_id.to_string()),
            mode: "auto_hunt".to_string(),
            ..PlayerState::default()
        };
        let area = area_by_id(area_id);
        let start_gold = player.gold;
        for tick in 0..ticks {
            let tick_seed = seed ^ ((run as u64) << 32) ^ tick as u64;
            if !should_trigger_encounter(&area, false, tick_seed) {
                continue;
            }
            let before_level = player.level;
            let before_xp = player.xp;
            let logs = resolve_auto_hunt_tick(&mut player, tick_seed);
            if logs.iter().any(|line| line.contains("returned to town")) {
                deaths += 1;
                player.mode = "auto_hunt".to_string();
                player.current_area_id = Some(area_id.to_string());
            }
            if player.level > before_level {
                total_xp += xp_to_next(before_level).saturating_sub(before_xp) + player.xp;
            }
        }
        total_gold += player.gold - start_gold;
        hash_input.push_str(&format!(
            "{}:{}:{}:{};",
            run, player.level, player.xp, player.gold
        ));
    }
    let mut hasher = DefaultHasher::new();
    hash_input.hash(&mut hasher);
    let survival_rate = 1.0 - (deaths as f64 / (runs as f64 * ticks as f64)).min(1.0);
    SimulationSummary {
        area_id: area_id.to_string(),
        hours,
        runs,
        seed,
        survival_rate,
        average_xp: total_xp as f64 / runs as f64,
        average_gold: total_gold as f64 / runs as f64,
        deaths,
        result_hash: format!("{:016x}", hasher.finish()),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationSummary {
    pub area_id: String,
    pub hours: u32,
    pub runs: u32,
    pub seed: u64,
    pub survival_rate: f64,
    pub average_xp: f64,
    pub average_gold: f64,
    pub deaths: u32,
    pub result_hash: String,
}

pub fn default_areas() -> Vec<Area> {
    vec![
        area("training-field", "Training Field", 1, "Safe", 0.35),
        area("forest-edge", "Forest Edge", 3, "Low", 0.45),
        area("old-mine", "Old Mine", 6, "Normal", 0.55),
        area("misty-swamp", "Misty Swamp", 10, "High", 0.65),
        area("fallen-fortress", "Fallen Fortress", 15, "Deadly", 0.75),
        area("obsidian-coast", "Obsidian Coast", 20, "Deadly", 0.78),
        area("titan-steppe", "Titan Steppe", 24, "Deadly", 0.80),
        area("oracle-ruins", "Oracle Ruins", 28, "Deadly", 0.82),
        area("styx-marsh", "Styx Marsh", 32, "Deadly", 0.84),
        area("olympus-gate", "Olympus Gate", 36, "Deadly", 0.85),
    ]
}

pub fn default_dungeons() -> Vec<Dungeon> {
    vec![
        dungeon("goblin-den", "Goblin Den", 5, 3, "goblin-chief"),
        dungeon("crystal-cave", "Crystal Cave", 10, 5, "crystal-golem"),
        dungeon("lich-tomb", "Lich Tomb", 16, 7, "ancient-lich"),
        dungeon("cyclops-forge", "Cyclops Forge", 22, 5, "cyclops-smith"),
        dungeon("medusa-temple", "Medusa Temple", 30, 7, "medusa"),
        dungeon("titan-vault", "Titan Vault", 38, 9, "titan-warden"),
    ]
}

pub fn default_companions() -> Vec<CompanionState> {
    vec![
        companion("borin", "Borin", "Tank", "Common", true),
        companion("lyra", "Lyra", "Healer", "Common", false),
        companion("kael", "Kael", "Rogue DPS", "Common", false),
        companion("mira", "Mira", "Mage DPS", "Common", false),
        companion("thorne", "Thorne", "Warrior DPS", "Rare", false),
        companion("elowen", "Elowen", "Buffer", "Rare", false),
        companion("nyx", "Nyx", "Debuffer", "Rare", false),
        companion("seraphina", "Seraphina", "High Healer", "Epic", false),
    ]
}

pub fn normalize_alias_tokens(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let current = args[i].as_str();
        let merged = if i + 1 < args.len() {
            Some(format!("{}{}", current, args[i + 1]))
        } else {
            None
        };
        if let Some(ref value) = merged {
            if let Some(mapped) = alias_map(value) {
                out.push(mapped.to_string());
                i += 2;
                continue;
            }
        }
        out.push(alias_map(current).unwrap_or(current).to_string());
        i += 1;
    }
    out
}

pub fn alias_map(input: &str) -> Option<&'static str> {
    match input {
        "상태" => Some("status"),
        "상세상태" => Some("full-status"),
        "스탯" | "능력치" | "캐릭터" => Some("stats"),
        "열기" => Some("open"),
        "닫기" => Some("close"),
        "토글" => Some("toggle"),
        "지도" | "메뉴" => Some("map"),
        "사냥" => Some("hunt"),
        "시작" => Some("start"),
        "중지" => Some("stop"),
        "지역" => Some("area"),
        "입장" => Some("enter"),
        "던전" => Some("dungeon"),
        "후퇴" => Some("retreat"),
        "파티" => Some("party"),
        "영입" => Some("recruit"),
        "가방" => Some("inventory"),
        "장착" => Some("equip"),
        "강화" => Some("enhance"),
        "스킬" => Some("skill"),
        "상점" => Some("shop"),
        "구매" => Some("buy"),
        "판매" => Some("sell"),
        "휴식" => Some("rest"),
        "마을" => Some("town"),
        "로그" => Some("log"),
        "숲가장자리" | "숲길" | "숲" => Some("forest-edge"),
        "초보훈련장" | "훈련장" | "훈련" => Some("training-field"),
        "낡은광산" | "광산" => Some("old-mine"),
        "안개늪" | "늪" | "늪지" => Some("misty-swamp"),
        "무너진요새" | "요새" => Some("fallen-fortress"),
        "흑요해안" | "흑요" | "해안" => Some("obsidian-coast"),
        "거인초원" | "티탄초원" | "초원" => Some("titan-steppe"),
        "예언자유적" | "예언유적" | "유적" => Some("oracle-ruins"),
        "스틱스늪" | "스틱스" => Some("styx-marsh"),
        "올림포스관문" | "올림포스" | "관문" => Some("olympus-gate"),
        "고블린소굴" | "고블굴" | "고블" => Some("goblin-den"),
        "수정동굴" | "수정굴" | "수정" => Some("crystal-cave"),
        "리치무덤" | "리치묘" | "리치" => Some("lich-tomb"),
        "키클롭스대장간" | "키클롭스" | "대장간" => Some("cyclops-forge"),
        "메두사신전" | "메두사" | "신전" => Some("medusa-temple"),
        "티탄금고" | "티탄" | "금고" => Some("titan-vault"),
        "보린" => Some("borin"),
        "라이라" => Some("lyra"),
        _ => None,
    }
}

fn visible_class_label(snapshot: &GameSnapshot) -> &'static str {
    if snapshot.player.class_id == "adventurer"
        || (snapshot.player.class_id == "warrior" && snapshot.player.level <= 10)
    {
        "Adventurer"
    } else {
        class_label(&snapshot.player.class_id)
    }
}

pub fn class_label(class_id: &str) -> &'static str {
    match class_id {
        "adventurer" => "Adventurer",
        "fighter" | "warrior" => "Fighter",
        "robot" | "mage" | "glyph-sage" | "burrower-miner" => "Robot",
        "priest" | "healer" => "Priest",
        "gangster" | "rogue" => "Gangster",
        "capitalist" | "wanderer" | "portrait-keeper" => "Capitalist",
        _ => "Adventurer",
    }
}

pub fn area_label(area_id: &str) -> &'static str {
    match area_id {
        "training-field" => "Training Field",
        "forest-edge" => "Forest Edge",
        "old-mine" => "Old Mine",
        "misty-swamp" => "Misty Swamp",
        "fallen-fortress" => "Fallen Fortress",
        "obsidian-coast" => "Obsidian Coast",
        "titan-steppe" => "Titan Steppe",
        "oracle-ruins" => "Oracle Ruins",
        "styx-marsh" => "Styx Marsh",
        "olympus-gate" => "Olympus Gate",
        "town" => "Town",
        _ => "Unknown",
    }
}

pub fn dungeon_label(dungeon_id: &str) -> &'static str {
    match dungeon_id {
        "goblin-den" => "Goblin Den",
        "crystal-cave" => "Crystal Cave",
        "lich-tomb" => "Lich Tomb",
        "cyclops-forge" => "Cyclops Forge",
        "medusa-temple" => "Medusa Temple",
        "titan-vault" => "Titan Vault",
        _ => "Unknown Dungeon",
    }
}

pub fn danger_for_area(area_id: &str) -> &'static str {
    match area_id {
        "training-field" => "Safe",
        "forest-edge" => "Low",
        "old-mine" => "Normal",
        "misty-swamp" => "High",
        "fallen-fortress" | "obsidian-coast" | "titan-steppe" | "oracle-ruins" | "styx-marsh"
        | "olympus-gate" => "Deadly",
        _ => "Safe",
    }
}

pub fn mode_label(mode: &str) -> &'static str {
    match mode {
        "auto_hunt" => "Auto Hunt",
        "dungeon" => "Dungeon",
        "rest" => "Rest",
        "recovering" => "Recovering",
        _ => "Idle",
    }
}

fn area(
    id: &str,
    name: &str,
    recommended_level: u32,
    danger_rating: &str,
    encounter_rate: f64,
) -> Area {
    Area {
        id: id.to_string(),
        name: name.to_string(),
        recommended_level,
        danger_rating: danger_rating.to_string(),
        encounter_rate,
    }
}

fn dungeon(id: &str, name: &str, recommended_level: u32, floors: u32, boss_id: &str) -> Dungeon {
    Dungeon {
        id: id.to_string(),
        name: name.to_string(),
        recommended_level,
        floors,
        boss_id: boss_id.to_string(),
    }
}

fn companion(id: &str, name: &str, role: &str, rarity: &str, unlocked: bool) -> CompanionState {
    CompanionState {
        id: id.to_string(),
        name: name.to_string(),
        role: role.to_string(),
        rarity: rarity.to_string(),
        unlocked,
    }
}

fn normal_monster_for_seed(seed: u64) -> &'static str {
    if seed & 1 == 0 {
        "Imp"
    } else {
        "Burrower"
    }
}

#[allow(dead_code)]
fn boss_monster_for_area(area_id: &str) -> &'static str {
    match area_id {
        "forest-edge" => "Goblin",
        "old-mine" => "Bat",
        "misty-swamp" => "Toad",
        "fallen-fortress" => "Warden",
        "obsidian-coast" => "Leviathan",
        "titan-steppe" => "Raider",
        "oracle-ruins" => "Sphinx",
        "styx-marsh" => "Ferryman",
        "olympus-gate" => "Sentinel",
        _ => "Golem",
    }
}

fn danger_bonus(area_id: &str) -> i32 {
    match area_id {
        "forest-edge" => 2,
        "old-mine" => 5,
        "misty-swamp" => 9,
        "fallen-fortress" => 14,
        "obsidian-coast" => 18,
        "titan-steppe" => 22,
        "oracle-ruins" => 27,
        "styx-marsh" => 32,
        "olympus-gate" => 38,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_up_grows_regen_every_third_level() {
        let mut player = PlayerState::default();
        let base_regen = player.regen;
        while player.level < 6 {
            let needed = player.xp_to_next;
            apply_xp(&mut player, needed);
        }
        assert_eq!(player.level, 6);
        assert_eq!(player.regen, base_regen + 2, "levels 3 and 6 add regen");
    }

    #[test]
    fn xp_curve_is_monotonic() {
        assert_eq!(xp_to_next(1), 100);
        assert!(xp_to_next(10) > xp_to_next(5));
    }

    #[test]
    fn combat_formula_clamps() {
        assert_eq!(hit_chance(-1000, 1000), 0.35);
        assert_eq!(hit_chance(1000, -1000), 0.95);
        assert_eq!(crit_chance(1000), 0.30);
        assert_eq!(luck_drop_multiplier(-1), 1.0);
        assert!(luck_drop_multiplier(1000) <= 2.0);
        assert_eq!(resistance_skill_reduction(-1), 0.0);
        assert!(resistance_skill_reduction(1000) <= 0.45);
        assert!(damage(1, 1.0, 10_000, 0.85) >= 1);
    }

    #[test]
    fn representative_stats_are_derived_from_detailed_stats() {
        let player = PlayerState::default();
        let stats = representative_stats(&player);
        assert_eq!(stats.attack, player.attack);
        assert_eq!(stats.defense, player.defense);
        assert_eq!(stats.vitality, player.max_hp);
        assert_eq!(stats.speed, player.speed);
        assert!(stats.combat_power > player.max_hp);
    }

    #[test]
    fn speed_shortens_encounter_interval_from_ten_second_baseline() {
        assert_eq!(encounter_interval_ms(10), 20_000);
        assert_eq!(encounter_interval_ms(30), 16_000);
        assert_eq!(encounter_interval_ms(100), 12_000);
        assert_eq!(encounter_interval_ticks(10, 2_000), 10);
        assert_eq!(encounter_interval_ticks(30, 2_000), 8);
        assert_eq!(encounter_interval_ticks(10, 4_000), 5);
        assert_eq!(encounter_interval_ticks(30, 4_000), 4);
    }

    #[test]
    fn encounter_chance_respects_area_danger_and_dungeon_modifier() {
        let training = area_by_id("training-field");
        let fortress = area_by_id("fallen-fortress");
        assert!(encounter_chance(&training, false) < encounter_chance(&fortress, false));
        assert!(encounter_chance(&fortress, true) > encounter_chance(&fortress, false));
        assert!((0.10..=0.85).contains(&encounter_chance(&training, false)));
        assert!((0.10..=0.85).contains(&encounter_chance(&fortress, true)));
        assert!(encounter_chance(&training, false) < 0.30);
    }

    #[test]
    fn encounter_rolls_can_fail_for_scouting_ticks() {
        let forest = area_by_id("forest-edge");
        assert!((0..64).any(|seed| !should_trigger_encounter(&forest, false, seed)));
    }

    #[test]
    fn status_line_uses_dungeon_as_location_while_dungeon_mode() {
        let mut snapshot = GameSnapshot::initial();
        snapshot.player.mode = "dungeon".to_string();
        snapshot.player.current_dungeon_id = Some("crystal-cave".to_string());
        snapshot.player.current_area_id = Some("old-mine".to_string());

        let dto = StatusLineDto::from(&snapshot);

        assert_eq!(dto.area_label, "Crystal Cave");
        assert_eq!(dto.danger_label, "Dungeon");
    }

    #[test]
    fn death_penalty_bands() {
        let mut low = PlayerState {
            level: 3,
            gold: 1000,
            hp: 0,
            ..PlayerState::default()
        };
        death_penalty(&mut low);
        assert_eq!(low.gold, 1000);
        assert_eq!(low.mode, "recovering");
        assert_eq!(low.current_area_id.as_deref(), Some("town"));
        assert_eq!(low.current_dungeon_id, None);
        assert_eq!(low.hp, 1);
        let mut mid = PlayerState {
            level: 10,
            gold: 1000,
            hp: 0,
            ..PlayerState::default()
        };
        death_penalty(&mut mid);
        assert_eq!(mid.gold, 970);
    }

    #[test]
    fn alias_normalizes_korean() {
        let args = vec!["사냥".into(), "시작".into(), "숲".into(), "가장자리".into()];
        assert_eq!(
            normalize_alias_tokens(&args),
            vec!["hunt", "start", "forest-edge"]
        );
    }

    #[test]
    fn simulation_is_deterministic() {
        let a = simulate("forest-edge", 1, 3, 42);
        let b = simulate("forest-edge", 1, 3, 42);
        assert_eq!(a.result_hash, b.result_hash);
    }
}
