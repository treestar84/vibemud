use anyhow::{Context, Result};
pub mod mod_bridge;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use uuid::Uuid;
use vibemud_core::{
    default_areas, default_companions, default_dungeons, CommandKind, CommandPayload,
    CommandStatus, EventKind, GameEvent, GameSnapshot, InventoryItem, PlayerState, DEFAULT_HUD_ID,
    DEFAULT_PLAYER_ID, DEFAULT_SESSION_ID,
};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub config: PathBuf,
    pub logs: PathBuf,
    pub backups: PathBuf,
    pub vibe_activity: PathBuf,
}

pub const PROJECT_STORAGE_MARKER_FILE: &str = ".vibemud-project";
const RUNTIME_CLOCK_TICK_KEY: &str = "runtime.clock_tick";

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let root = if let Ok(path) = std::env::var("VIBEMUD_HOME") {
            PathBuf::from(path)
        } else if let Some(path) = discover_project_vibemud_root()? {
            path
        } else {
            default_vibemud_root()?
        };
        Ok(Self::at(root))
    }

    pub fn for_mod() -> Result<Self> {
        let root = match std::env::var_os("VIBEMUD_HOME").filter(|p| !p.is_empty()) {
            Some(path) => PathBuf::from(path),
            None => default_vibemud_root()?,
        };
        anyhow::ensure!(
            root.is_absolute(),
            "VIBEMUD_HOME must be an absolute data directory"
        );
        Ok(Self::at(root))
    }

    pub fn legacy_project() -> Result<Option<Self>> {
        Ok(discover_project_vibemud_root()?.map(Self::at))
    }

    pub fn at(root: PathBuf) -> Self {
        Self {
            db: root.join("vibemud.db"),
            config: root.join("config.toml"),
            logs: root.join("logs"),
            backups: root.join("backups"),
            vibe_activity: root.join("vibe-activity.json"),
            root,
        }
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.logs)?;
        fs::create_dir_all(&self.backups)?;
        restrict_dir(&self.root)?;
        Ok(())
    }
}

fn discover_project_vibemud_root() -> Result<Option<PathBuf>> {
    let current = std::env::current_dir().context("failed to read current directory")?;
    Ok(current
        .ancestors()
        .map(project_vibemud_root)
        .find(|root| root.join(PROJECT_STORAGE_MARKER_FILE).is_file()))
}

fn project_vibemud_root(dir: &Path) -> PathBuf {
    dir.join(".vibemud")
}

fn default_vibemud_root() -> Result<PathBuf> {
    if cfg!(windows) {
        if let Some(local_app_data) = non_empty_env_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local_app_data).join("VibeMUD"));
        }
        if let Some(user_profile) = non_empty_env_os("USERPROFILE") {
            return Ok(PathBuf::from(user_profile).join(".vibemud"));
        }
    }
    let home = std::env::var_os("HOME")
        .context("HOME is not set; set VIBEMUD_HOME or, on Windows, LOCALAPPDATA/USERPROFILE")?;
    Ok(PathBuf::from(home).join(".vibemud"))
}

fn non_empty_env_os(name: &str) -> Option<std::ffi::OsString> {
    let value = std::env::var_os(name)?;
    if value.as_os_str().is_empty() {
        None
    } else {
        Some(value)
    }
}

const VIBE_FEVER_HEARTBEAT_TTL_SECONDS: i64 = 30;
const VIBE_ACTIVITY_SOURCES: &[&str] = &["claude", "codex", "manual"];
const EVENT_LOG_RETENTION_ROWS: i64 = 10_000;
const STATE_SNAPSHOT_RETENTION_ROWS: i64 = 1_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VibeActivity {
    pub active: bool,
    pub source: String,
    pub updated_at: String,
    #[serde(default)]
    pub reward_until: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DailyQuest {
    pub quest_id: String,
    pub title: String,
    pub progress: i64,
    pub target: i64,
    pub status: String,
    pub reward_kind: String,
    pub reward_amount: i64,
    pub fever_minutes: i64,
}

#[derive(Debug, Clone, Copy)]
struct QuestDefinition {
    id: &'static str,
    title_ko: &'static str,
    metric: &'static str,
    slot: Option<&'static str>,
    target: i64,
    reward_kind: &'static str,
    reward_amount: i64,
    fever_min: i64,
    fever_max: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub ui: UiConfig,
    pub runtime: RuntimeConfig,
    pub game: GameConfig,
    pub integrations: IntegrationConfig,
    pub privacy: PrivacyConfig,
    pub database: DatabaseConfig,
    pub packaging: PackagingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub language: String,
    pub hud_mode: String,
    pub hud_refresh_seconds: u64,
    pub statusline_enabled: bool,
    pub unicode_borders: bool,
    pub compact_mode: bool,
    #[serde(default = "default_popup_pane_enabled")]
    pub popup_pane_enabled: bool,
    #[serde(default = "default_message_printline")]
    pub message_printline: usize,
    pub ultra_compact_below_columns: usize,
    pub compact_below_columns: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub tick_interval_ms: u64,
    pub session_only_progress: bool,
    pub offline_progress: bool,
    pub background_daemon_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameConfig {
    pub auto_hunt_after_area_select: bool,
    pub death_penalty_mode: String,
    pub growth_curve: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationConfig {
    #[serde(default = "default_terminal_integration")]
    pub terminal: String,
    pub tmux_enabled: bool,
    pub codex_enabled: bool,
    pub claude_enabled: bool,
    pub coding_event_rewards: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub read_code_content: bool,
    pub store_prompts: bool,
    pub store_file_paths: bool,
    pub store_commit_messages: bool,
    pub redact_home_path_in_diagnostics: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub journal_mode: String,
    pub synchronous: String,
    pub busy_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackagingConfig {
    pub prefer_multicall_binary: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            ui: UiConfig {
                language: "ko".to_string(),
                hud_mode: "side".to_string(),
                hud_refresh_seconds: 2,
                statusline_enabled: true,
                unicode_borders: true,
                compact_mode: true,
                popup_pane_enabled: default_popup_pane_enabled(),
                message_printline: default_message_printline(),
                ultra_compact_below_columns: 60,
                compact_below_columns: 100,
            },
            runtime: RuntimeConfig {
                tick_interval_ms: 1000,
                session_only_progress: true,
                offline_progress: false,
                background_daemon_enabled: false,
            },
            game: GameConfig {
                auto_hunt_after_area_select: true,
                death_penalty_mode: "scaling".to_string(),
                growth_curve: "fast_then_slow".to_string(),
            },
            integrations: IntegrationConfig {
                terminal: default_terminal_integration(),
                tmux_enabled: true,
                codex_enabled: false,
                claude_enabled: false,
                coding_event_rewards: false,
            },
            privacy: PrivacyConfig {
                read_code_content: false,
                store_prompts: false,
                store_file_paths: false,
                store_commit_messages: false,
                redact_home_path_in_diagnostics: true,
            },
            database: DatabaseConfig {
                journal_mode: "WAL".to_string(),
                synchronous: "NORMAL".to_string(),
                busy_timeout_ms: 3000,
            },
            packaging: PackagingConfig {
                prefer_multicall_binary: true,
            },
        }
    }
}

fn default_popup_pane_enabled() -> bool {
    true
}

fn default_terminal_integration() -> String {
    "auto".to_string()
}

fn default_message_printline() -> usize {
    7
}

pub fn init_app() -> Result<AppPaths> {
    let paths = AppPaths::discover()?;
    paths.ensure_dirs()?;
    if !paths.config.exists() {
        let text = toml::to_string_pretty(&AppConfig::default())?;
        fs::write(&paths.config, text)?;
        restrict_file(&paths.config)?;
    } else {
        ensure_config_defaults(&paths.config)?;
    }
    let conn = open_connection(&paths)?;
    migrate(&conn)?;
    seed_initial_state(&conn)?;
    Ok(paths)
}

fn ensure_config_defaults(config_path: &std::path::Path) -> Result<()> {
    let text = fs::read_to_string(config_path)?;
    let mut value: toml::Value = toml::from_str(&text)
        .with_context(|| format!("failed to parse config {}", config_path.display()))?;
    let defaults_text = toml::to_string_pretty(&AppConfig::default())?;
    let defaults: toml::Value = toml::from_str(&defaults_text)?;

    if merge_missing_config_values(&mut value, &defaults) {
        fs::write(config_path, toml::to_string_pretty(&value)?)?;
        restrict_file(config_path)?;
    }

    Ok(())
}

fn merge_missing_config_values(value: &mut toml::Value, defaults: &toml::Value) -> bool {
    match (value, defaults) {
        (toml::Value::Table(value_table), toml::Value::Table(default_table)) => {
            let mut changed = false;
            for (key, default_value) in default_table {
                match value_table.get_mut(key) {
                    Some(current_value) => {
                        changed |= merge_missing_config_values(current_value, default_value);
                    }
                    None => {
                        value_table.insert(key.clone(), default_value.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        _ => false,
    }
}

pub fn load_config() -> Result<AppConfig> {
    let paths = init_app()?;
    let text = fs::read_to_string(paths.config)?;
    toml::from_str(&text).map_err(Into::into)
}

pub fn runtime_tick_interval_ms() -> Result<u64> {
    Ok(load_config()?.runtime.tick_interval_ms.clamp(1_000, 60_000))
}

pub fn runtime_clock_tick(conn: &Connection) -> Result<u64> {
    Ok(setting_value(conn, RUNTIME_CLOCK_TICK_KEY)?
        .as_deref()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0))
}

pub fn advance_runtime_clock(conn: &Connection) -> Result<u64> {
    let tick = runtime_clock_tick(conn)?.saturating_add(1);
    set_setting(conn, RUNTIME_CLOCK_TICK_KEY, &tick.to_string())?;
    Ok(tick)
}

pub fn write_vibe_activity(source: &str, active: bool) -> Result<()> {
    let source = source.to_ascii_lowercase();
    if !VIBE_ACTIVITY_SOURCES.contains(&source.as_str()) {
        anyhow::bail!("unsupported vibe activity source: {source}");
    }
    let paths = init_app()?;
    let reward_until = read_vibe_activity_file(&paths).and_then(|activity| activity.reward_until);
    let activity = VibeActivity {
        active,
        source,
        updated_at: OffsetDateTime::now_utc().format(&Rfc3339)?,
        reward_until,
    };
    fs::write(
        &paths.vibe_activity,
        serde_json::to_string_pretty(&activity)?,
    )?;
    restrict_file(&paths.vibe_activity)?;
    Ok(())
}

pub fn clear_vibe_activity(source: &str) -> Result<()> {
    write_vibe_activity(source, false)
}

pub fn vibe_fever_active() -> bool {
    let legacy = vibe_activity()
        .map(|activity| activity.active)
        .unwrap_or(false);
    legacy
        || mod_bridge::open_readonly()
            .and_then(|(_, conn)| mod_bridge::fever_active(&conn))
            .unwrap_or(false)
}

pub fn vibe_activity() -> Option<VibeActivity> {
    let paths = AppPaths::discover().ok()?;
    let activity = read_vibe_activity_file(&paths)?;
    let now = OffsetDateTime::now_utc();
    let heartbeat_active = activity
        .active
        .then(|| OffsetDateTime::parse(&activity.updated_at, &Rfc3339).ok())
        .flatten()
        .map(|updated_at| now - updated_at <= Duration::seconds(VIBE_FEVER_HEARTBEAT_TTL_SECONDS))
        .unwrap_or(false);
    let reward_active = activity
        .reward_until
        .as_deref()
        .and_then(|until| OffsetDateTime::parse(until, &Rfc3339).ok())
        .map(|until| until > now)
        .unwrap_or(false);
    if heartbeat_active || reward_active {
        Some(VibeActivity {
            active: true,
            source: if heartbeat_active {
                activity.source
            } else {
                "reward".to_string()
            },
            updated_at: activity.updated_at,
            reward_until: activity.reward_until,
        })
    } else {
        Some(VibeActivity {
            active: false,
            ..activity
        })
    }
}

fn read_vibe_activity_file(paths: &AppPaths) -> Option<VibeActivity> {
    let text = fs::read_to_string(&paths.vibe_activity).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn grant_vibe_fever_minutes(minutes: i64) -> Result<String> {
    let minutes = minutes.clamp(1, 24 * 60);
    let paths = init_app()?;
    let now = OffsetDateTime::now_utc();
    let existing = read_vibe_activity_file(&paths);
    let base_until = existing
        .as_ref()
        .and_then(|activity| activity.reward_until.as_deref())
        .and_then(|until| OffsetDateTime::parse(until, &Rfc3339).ok())
        .filter(|until| *until > now)
        .unwrap_or(now);
    let reward_until = (base_until + Duration::minutes(minutes)).format(&Rfc3339)?;
    let heartbeat_active = existing
        .as_ref()
        .and_then(|activity| {
            activity
                .active
                .then(|| OffsetDateTime::parse(&activity.updated_at, &Rfc3339).ok())
                .flatten()
                .map(|updated_at| {
                    now - updated_at <= Duration::seconds(VIBE_FEVER_HEARTBEAT_TTL_SECONDS)
                })
        })
        .unwrap_or(false);
    let activity = VibeActivity {
        active: heartbeat_active,
        source: existing
            .as_ref()
            .map(|activity| activity.source.clone())
            .unwrap_or_else(|| "reward".to_string()),
        updated_at: existing
            .as_ref()
            .map(|activity| activity.updated_at.clone())
            .unwrap_or_else(|| {
                now.format(&Rfc3339)
                    .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
            }),
        reward_until: Some(reward_until.clone()),
    };
    fs::write(
        &paths.vibe_activity,
        serde_json::to_string_pretty(&activity)?,
    )?;
    restrict_file(&paths.vibe_activity)?;
    Ok(reward_until)
}

pub fn open_app() -> Result<(AppPaths, Connection)> {
    let paths = AppPaths::discover()?;
    if !paths.db.exists() {
        init_app()?;
    }
    let conn = open_connection(&paths)?;
    migrate(&conn)?;
    seed_initial_state(&conn)?;
    Ok((paths, conn))
}

pub fn open_connection(paths: &AppPaths) -> Result<Connection> {
    paths.ensure_dirs()?;
    let conn = Connection::open(&paths.db)?;
    apply_pragmas(&conn)?;
    restrict_file(&paths.db)?;
    Ok(conn)
}

pub fn apply_pragmas(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(3000))?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<()> {
    let sql = r#"
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  applied_at TEXT NOT NULL,
  checksum TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS player_state (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  class_id TEXT NOT NULL,
  level INTEGER NOT NULL,
  xp INTEGER NOT NULL,
  xp_to_next INTEGER NOT NULL,
  hp INTEGER NOT NULL,
  max_hp INTEGER NOT NULL,
  mp INTEGER NOT NULL,
  max_mp INTEGER NOT NULL,
  attack INTEGER NOT NULL,
  defense INTEGER NOT NULL,
  accuracy INTEGER NOT NULL,
  evasion INTEGER NOT NULL,
  speed INTEGER NOT NULL,
  regen INTEGER NOT NULL,
  luck INTEGER NOT NULL,
  gold INTEGER NOT NULL,
  current_area_id TEXT,
  current_dungeon_id TEXT,
  mode TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS companions (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  role TEXT NOT NULL,
  rarity TEXT NOT NULL,
  level INTEGER NOT NULL,
  xp INTEGER NOT NULL,
  hp INTEGER NOT NULL,
  max_hp INTEGER NOT NULL,
  mp INTEGER NOT NULL,
  max_mp INTEGER NOT NULL,
  attack INTEGER NOT NULL,
  defense INTEGER NOT NULL,
  accuracy INTEGER NOT NULL,
  evasion INTEGER NOT NULL,
  speed INTEGER NOT NULL,
  regen INTEGER NOT NULL,
  skill_ids TEXT NOT NULL,
  affinity INTEGER NOT NULL,
  unlocked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS party_slots (
  slot_index INTEGER PRIMARY KEY,
  companion_id TEXT,
  locked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS inventory_items (
  id TEXT PRIMARY KEY,
  item_id TEXT NOT NULL,
  item_type TEXT NOT NULL,
  name TEXT NOT NULL,
  rarity TEXT NOT NULL,
  quantity INTEGER NOT NULL,
  durability INTEGER,
  max_durability INTEGER,
  equipped_slot TEXT,
  locked INTEGER NOT NULL DEFAULT 0,
  enhancement_level INTEGER NOT NULL DEFAULT 0,
  acquired_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS equipment_items (
  item_id TEXT PRIMARY KEY,
  slot TEXT NOT NULL,
  name TEXT NOT NULL,
  rarity TEXT NOT NULL,
  rarity_color TEXT NOT NULL,
  tier INTEGER NOT NULL,
  stat1_type TEXT NOT NULL,
  stat1_value INTEGER NOT NULL,
  stat2_type TEXT NOT NULL,
  stat2_value INTEGER NOT NULL,
  stat3_type TEXT,
  stat3_value INTEGER,
  power_score INTEGER NOT NULL,
  flavor TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS equipment_enhancement_rules (
  enhancement_level INTEGER PRIMARY KEY,
  stat_multiplier_bps INTEGER NOT NULL,
  upgrade_gold_cost INTEGER,
  success_rate REAL,
  failure_level_drop INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS equipment_enhancement_rarity_modifiers (
  rarity TEXT PRIMARY KEY,
  cost_multiplier_bps INTEGER NOT NULL,
  success_delta_bps INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS equipment_enhancement_slot_modifiers (
  slot TEXT PRIMARY KEY,
  cost_multiplier_bps INTEGER NOT NULL,
  success_delta_bps INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS monster_equipment_drops (
  monster_id TEXT NOT NULL,
  monster_grade TEXT NOT NULL,
  rarity TEXT NOT NULL,
  drop_chance REAL NOT NULL,
  weight INTEGER NOT NULL,
  min_tier INTEGER NOT NULL DEFAULT 1,
  max_tier INTEGER NOT NULL DEFAULT 3,
  PRIMARY KEY (monster_id, rarity)
);
CREATE TABLE IF NOT EXISTS regions (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  name_ko TEXT NOT NULL,
  sort_order INTEGER NOT NULL,
  unlocked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS areas (
  id TEXT PRIMARY KEY,
  region_id TEXT NOT NULL DEFAULT 'training-plains',
  name TEXT NOT NULL,
  recommended_level INTEGER NOT NULL,
  danger_rating TEXT NOT NULL,
  difficulty_rank INTEGER NOT NULL DEFAULT 1,
  encounter_rate REAL NOT NULL,
  unlocked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS dungeons (
  id TEXT PRIMARY KEY,
  region_id TEXT NOT NULL DEFAULT 'greenwood',
  name TEXT NOT NULL,
  recommended_level INTEGER NOT NULL,
  difficulty_rank INTEGER NOT NULL DEFAULT 5,
  floors INTEGER NOT NULL,
  boss_id TEXT NOT NULL,
  unlocked INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS monsters (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  name_ko TEXT NOT NULL,
  monster_grade TEXT NOT NULL,
  recommended_level INTEGER NOT NULL,
  difficulty_bonus INTEGER NOT NULL,
  flavor TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS area_monsters (
  area_id TEXT NOT NULL,
  monster_id TEXT NOT NULL,
  weight INTEGER NOT NULL,
  PRIMARY KEY (area_id, monster_id)
);
CREATE TABLE IF NOT EXISTS dungeon_monsters (
  dungeon_id TEXT NOT NULL,
  monster_id TEXT NOT NULL,
  weight INTEGER NOT NULL,
  PRIMARY KEY (dungeon_id, monster_id)
);
CREATE TABLE IF NOT EXISTS daily_quests (
  id TEXT PRIMARY KEY,
  quest_date TEXT NOT NULL,
  quest_id TEXT NOT NULL,
  progress INTEGER NOT NULL DEFAULT 0,
  target INTEGER NOT NULL,
  status TEXT NOT NULL,
  reward_kind TEXT NOT NULL,
  reward_amount INTEGER NOT NULL,
  fever_minutes INTEGER NOT NULL,
  assigned_at TEXT NOT NULL,
  completed_at TEXT,
  claimed_at TEXT,
  UNIQUE(quest_date, quest_id)
);
CREATE TABLE IF NOT EXISTS combat_state (
  id TEXT PRIMARY KEY,
  in_combat INTEGER NOT NULL,
  encounter_id TEXT,
  monster_group_json TEXT,
  encounter_seed INTEGER,
  turn_index INTEGER NOT NULL,
  started_at TEXT,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS command_queue (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  processed_at TEXT,
  source TEXT NOT NULL,
  command_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  status TEXT NOT NULL,
  error_message TEXT,
  result_json TEXT
);
CREATE TABLE IF NOT EXISTS mod_sessions (
  id TEXT PRIMARY KEY,
  expires_at INTEGER NOT NULL,
  active_until INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS event_log (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  event_type TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  state_version INTEGER NOT NULL,
  rng_seed INTEGER,
  random_draw_json TEXT
);
CREATE TABLE IF NOT EXISTS state_snapshots (
  id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL,
  state_version INTEGER NOT NULL,
  snapshot_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS hud_state (
  id TEXT PRIMARY KEY,
  updated_at TEXT NOT NULL,
  state_version INTEGER NOT NULL,
  one_line TEXT NOT NULL,
  compact_json TEXT NOT NULL,
  full_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS session_state (
  id TEXT PRIMARY KEY,
  runtime_pid INTEGER,
  started_at TEXT,
  stopped_at TEXT,
  last_seen_state_version INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS localization_aliases (
  locale TEXT NOT NULL,
  alias TEXT NOT NULL,
  canonical_type TEXT NOT NULL,
  canonical_id TEXT NOT NULL,
  PRIMARY KEY (locale, alias)
);
CREATE INDEX IF NOT EXISTS idx_command_queue_status_created ON command_queue(status, created_at);
CREATE INDEX IF NOT EXISTS idx_event_log_state_version ON event_log(state_version);
CREATE INDEX IF NOT EXISTS idx_event_log_created_at ON event_log(created_at);
CREATE INDEX IF NOT EXISTS idx_state_snapshots_state_version ON state_snapshots(state_version);
CREATE INDEX IF NOT EXISTS idx_inventory_equipped_slot ON inventory_items(equipped_slot) WHERE equipped_slot IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_equipment_items_slot_rarity ON equipment_items(slot, rarity, tier);
CREATE INDEX IF NOT EXISTS idx_monster_equipment_drops_monster ON monster_equipment_drops(monster_id, monster_grade);
CREATE INDEX IF NOT EXISTS idx_daily_quests_date_status ON daily_quests(quest_date, status);
"#;
    conn.execute_batch(sql)?;
    ensure_column(
        conn,
        "inventory_items",
        "enhancement_level",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(
        conn,
        "inventory_items",
        "locked",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    ensure_column(conn, "equipment_items", "stat3_type", "TEXT")?;
    ensure_column(conn, "equipment_items", "stat3_value", "INTEGER")?;
    ensure_column(
        conn,
        "monster_equipment_drops",
        "min_tier",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "monster_equipment_drops",
        "max_tier",
        "INTEGER NOT NULL DEFAULT 3",
    )?;
    ensure_column(
        conn,
        "areas",
        "region_id",
        "TEXT NOT NULL DEFAULT 'training-plains'",
    )?;
    ensure_column(
        conn,
        "areas",
        "difficulty_rank",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "dungeons",
        "region_id",
        "TEXT NOT NULL DEFAULT 'greenwood'",
    )?;
    ensure_column(
        conn,
        "dungeons",
        "difficulty_rank",
        "INTEGER NOT NULL DEFAULT 5",
    )?;
    ensure_column(
        conn,
        "monster_equipment_drops",
        "min_tier",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    ensure_column(
        conn,
        "monster_equipment_drops",
        "max_tier",
        "INTEGER NOT NULL DEFAULT 3",
    )?;
    let exists: Option<i64> = conn
        .query_row(
            "SELECT version FROM schema_migrations WHERE version = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        conn.execute(
            "INSERT INTO schema_migrations(version, name, applied_at, checksum) VALUES (1, 'initial', ?1, 'vibemud-initial-v1')",
            [now()],
        )?;
    }
    let equipment_exists: Option<i64> = conn
        .query_row(
            "SELECT version FROM schema_migrations WHERE version = 2",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if equipment_exists.is_none() {
        conn.execute(
            "INSERT INTO schema_migrations(version, name, applied_at, checksum) VALUES (2, 'equipment', ?1, 'vibemud-equipment-v2')",
            [now()],
        )?;
    }
    let regen_backfill_exists: Option<i64> = conn
        .query_row(
            "SELECT version FROM schema_migrations WHERE version = 3",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if regen_backfill_exists.is_none() {
        conn.execute(
            "UPDATE player_state SET regen = MAX(regen, 3 + level / 3)",
            [],
        )?;
        conn.execute(
            "INSERT INTO schema_migrations(version, name, applied_at, checksum) VALUES (3, 'regen_backfill', ?1, 'vibemud-regen-backfill-v3')",
            [now()],
        )?;
    }
    Ok(())
}

fn ensure_column(conn: &Connection, table: &str, column: &str, column_sql: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(());
        }
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {column_sql}"),
        [],
    )?;
    Ok(())
}

pub fn seed_initial_state(conn: &Connection) -> Result<()> {
    let now = now();
    let player_exists: Option<String> = conn
        .query_row(
            "SELECT id FROM player_state WHERE id = ?1",
            [DEFAULT_PLAYER_ID],
            |row| row.get(0),
        )
        .optional()?;
    if player_exists.is_none() {
        let p = PlayerState::default();
        upsert_player(conn, &p)?;
        for c in default_companions() {
            conn.execute(
                "INSERT OR IGNORE INTO companions(id, name, role, rarity, level, xp, hp, max_hp, mp, max_mp, attack, defense, accuracy, evasion, speed, regen, skill_ids, affinity, unlocked) VALUES (?1, ?2, ?3, ?4, 1, 0, 80, 80, 30, 30, 10, 8, 70, 8, 10, 2, '[]', 0, ?5)",
                params![c.id, c.name, c.role, c.rarity, if c.unlocked { 1_i64 } else { 0_i64 }],
            )?;
        }
        conn.execute("INSERT OR IGNORE INTO party_slots(slot_index, companion_id, locked) VALUES (1, 'borin', 0), (2, NULL, 0), (3, NULL, 0)", [])?;
        seed_aliases(conn)?;
        conn.execute("INSERT OR IGNORE INTO combat_state(id, in_combat, turn_index, updated_at) VALUES ('main', 0, 0, ?1)", [now.clone()])?;
        conn.execute("INSERT OR IGNORE INTO session_state(id, status, last_seen_state_version) VALUES (?1, 'stopped', 0)", [DEFAULT_SESSION_ID])?;
        append_event(conn, EventKind::Initialized, "Initialized VibeMUD", None)?;
        write_snapshot_and_hud(conn)?;
    }
    seed_world_catalog(conn)?;
    seed_aliases(conn)?;
    seed_equipment_catalog(conn)?;
    conn.execute(
        "INSERT OR IGNORE INTO settings(key, value, updated_at) VALUES ('mod.save_generation', ?1, ?2)",
        params![Uuid::new_v4().to_string(), now],
    )?;
    Ok(())
}

pub fn seed_world_catalog(conn: &Connection) -> Result<()> {
    seed_regions(conn)?;
    seed_areas(conn)?;
    seed_dungeons(conn)?;
    seed_monsters(conn)?;
    seed_area_monsters(conn)?;
    seed_dungeon_monsters(conn)?;
    Ok(())
}

fn seed_regions(conn: &Connection) -> Result<()> {
    let rows = [
        ("training-plains", "Training Plains", "훈련 평원", 1, 1),
        ("greenwood", "Greenwood Border", "초록숲 경계", 2, 1),
        ("deep-delves", "Deep Delves", "깊은 굴길", 3, 1),
        ("cursed-frontier", "Cursed Frontier", "저주받은 전선", 4, 1),
        ("atlas-shore", "Atlas Shore", "아틀라스 해안", 5, 1),
        ("titan-highlands", "Titan Highlands", "티탄 고원", 6, 1),
        (
            "olympus-frontier",
            "Olympus Frontier",
            "올림포스 전선",
            7,
            1,
        ),
    ];
    for (id, name, name_ko, sort_order, unlocked) in rows {
        conn.execute(
            "INSERT INTO regions(id, name, name_ko, sort_order, unlocked) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, name_ko=excluded.name_ko, sort_order=excluded.sort_order, unlocked=excluded.unlocked",
            params![id, name, name_ko, sort_order, unlocked],
        )?;
    }
    Ok(())
}

fn seed_areas(conn: &Connection) -> Result<()> {
    let region_for_area = |id: &str| match id {
        "training-field" => ("training-plains", 1),
        "forest-edge" => ("greenwood", 3),
        "old-mine" => ("deep-delves", 6),
        "misty-swamp" => ("cursed-frontier", 10),
        "fallen-fortress" => ("cursed-frontier", 15),
        "obsidian-coast" => ("atlas-shore", 20),
        "titan-steppe" => ("titan-highlands", 24),
        "oracle-ruins" => ("titan-highlands", 28),
        "styx-marsh" => ("olympus-frontier", 32),
        "olympus-gate" => ("olympus-frontier", 36),
        _ => ("training-plains", 1),
    };
    for area in default_areas() {
        let (region_id, difficulty_rank) = region_for_area(&area.id);
        conn.execute(
            "INSERT INTO areas(id, region_id, name, recommended_level, danger_rating, difficulty_rank, encounter_rate, unlocked) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
             ON CONFLICT(id) DO UPDATE SET region_id=excluded.region_id, name=excluded.name, recommended_level=excluded.recommended_level, danger_rating=excluded.danger_rating, difficulty_rank=excluded.difficulty_rank, encounter_rate=excluded.encounter_rate, unlocked=excluded.unlocked",
            params![area.id, region_id, area.name, area.recommended_level, area.danger_rating, difficulty_rank, area.encounter_rate],
        )?;
    }
    Ok(())
}

fn seed_dungeons(conn: &Connection) -> Result<()> {
    let region_for_dungeon = |id: &str| match id {
        "goblin-den" => ("greenwood", 5),
        "crystal-cave" => ("deep-delves", 10),
        "lich-tomb" => ("cursed-frontier", 16),
        "cyclops-forge" => ("atlas-shore", 22),
        "medusa-temple" => ("titan-highlands", 30),
        "titan-vault" => ("olympus-frontier", 38),
        _ => ("greenwood", 5),
    };
    for dungeon in default_dungeons() {
        let (region_id, difficulty_rank) = region_for_dungeon(&dungeon.id);
        conn.execute(
            "INSERT INTO dungeons(id, region_id, name, recommended_level, difficulty_rank, floors, boss_id, unlocked) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)
             ON CONFLICT(id) DO UPDATE SET region_id=excluded.region_id, name=excluded.name, recommended_level=excluded.recommended_level, difficulty_rank=excluded.difficulty_rank, floors=excluded.floors, boss_id=excluded.boss_id, unlocked=excluded.unlocked",
            params![dungeon.id, region_id, dungeon.name, dungeon.recommended_level, difficulty_rank, dungeon.floors, dungeon.boss_id],
        )?;
    }
    Ok(())
}

fn seed_monsters(conn: &Connection) -> Result<()> {
    let rows = [
        (
            "training-scarab",
            "Scarab",
            "딱정벌레",
            "normal",
            1,
            0,
            "약한 등껍질을 가진 훈련용 몬스터",
        ),
        (
            "target-golem",
            "Golem",
            "골렘",
            "normal",
            1,
            1,
            "느리지만 단단한 훈련 표적",
        ),
        (
            "twig-imp",
            "Imp",
            "임프",
            "normal",
            3,
            2,
            "숲 경계에서 장난을 치는 작은 악마",
        ),
        (
            "moss-wolf",
            "Wolf",
            "늑대",
            "normal",
            4,
            3,
            "이끼에 뒤덮인 빠른 늑대",
        ),
        (
            "goblin-scout",
            "Goblin",
            "고블린",
            "normal",
            5,
            4,
            "고블린 소굴 바깥을 배회하는 척후병",
        ),
        (
            "mine-bat",
            "Bat",
            "박쥐",
            "normal",
            6,
            5,
            "낡은 광산 천장에 숨어드는 박쥐",
        ),
        (
            "ore-crawler",
            "Crawler",
            "크롤러",
            "normal",
            7,
            6,
            "수정 가루를 먹고 자라는 벌레",
        ),
        (
            "crystal-wisp",
            "Wisp",
            "위습",
            "elite",
            10,
            9,
            "수정 동굴의 마력 잔광",
        ),
        (
            "swamp-toad",
            "Toad",
            "두꺼비",
            "normal",
            10,
            9,
            "독 안개 속에서 뛰어드는 두꺼비",
        ),
        (
            "bog-witchling",
            "Witch",
            "마녀",
            "elite",
            12,
            11,
            "저주 물약을 던지는 늪의 견습 마녀",
        ),
        (
            "bone-guard",
            "Bone",
            "해골",
            "normal",
            15,
            14,
            "무너진 요새를 지키는 해골 병사",
        ),
        (
            "fallen-knight",
            "Knight",
            "기사",
            "elite",
            16,
            16,
            "죽은 맹세에 묶인 기사",
        ),
        (
            "goblin-brute",
            "Brute",
            "브루트",
            "elite",
            5,
            5,
            "소굴 깊은 곳의 둔기병",
        ),
        (
            "crystal-gazer",
            "Gazer",
            "응시자",
            "elite",
            10,
            10,
            "수정 동굴의 감시자",
        ),
        (
            "lich-acolyte",
            "Lich",
            "리치",
            "elite",
            16,
            17,
            "리치 무덤의 주문 시종",
        ),
        (
            "ash-siren",
            "Siren",
            "세이렌",
            "normal",
            20,
            18,
            "흑요 해안의 검은 파도 위에서 노래하는 세이렌",
        ),
        (
            "obsidian-crab",
            "Crab",
            "게",
            "normal",
            21,
            19,
            "화산 유리 껍질을 두른 해안 괴수",
        ),
        (
            "cyclops-apprentice",
            "Cyclops",
            "키클롭스",
            "elite",
            22,
            21,
            "거인 대장간의 불씨를 관리하는 외눈 장인",
        ),
        (
            "titan-raider",
            "Raider",
            "약탈자",
            "normal",
            24,
            23,
            "티탄 초원을 가로지르는 거구의 약탈자",
        ),
        (
            "bronze-hoplite",
            "Hoplite",
            "중장병",
            "normal",
            26,
            25,
            "고대 전열을 지키는 청동 병사",
        ),
        (
            "oracle-sphinx",
            "Sphinx",
            "스핑크스",
            "elite",
            28,
            28,
            "예언자 유적에서 수수께끼를 던지는 파수꾼",
        ),
        (
            "gorgon-sentinel",
            "Gorgon",
            "고르곤",
            "elite",
            30,
            31,
            "메두사 신전을 지키는 석화의 수호자",
        ),
        (
            "styx-ferryman",
            "Ferryman",
            "사공",
            "normal",
            32,
            33,
            "검은 늪을 오가는 침묵의 사공",
        ),
        (
            "eidolon-guard",
            "Eidolon",
            "에이돌론",
            "elite",
            34,
            35,
            "망자의 그림자를 갑옷처럼 두른 수문장",
        ),
        (
            "olympus-sentinel",
            "Sentinel",
            "파수기",
            "elite",
            36,
            38,
            "올림포스 관문에 세워진 신성한 파수기",
        ),
        (
            "titan-warden",
            "Warden",
            "감시자",
            "elite",
            38,
            42,
            "티탄 금고의 봉인을 지키는 고대 감시자",
        ),
    ];
    for (id, name, name_ko, grade, level, difficulty, flavor) in rows {
        conn.execute(
            "INSERT INTO monsters(id, name, name_ko, monster_grade, recommended_level, difficulty_bonus, flavor) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, name_ko=excluded.name_ko, monster_grade=excluded.monster_grade, recommended_level=excluded.recommended_level, difficulty_bonus=excluded.difficulty_bonus, flavor=excluded.flavor",
            params![id, name, name_ko, grade, level, difficulty, flavor],
        )?;
    }
    Ok(())
}

fn seed_area_monsters(conn: &Connection) -> Result<()> {
    let rows = [
        ("training-field", "training-scarab", 70),
        ("training-field", "target-golem", 30),
        ("forest-edge", "twig-imp", 55),
        ("forest-edge", "moss-wolf", 35),
        ("forest-edge", "goblin-scout", 10),
        ("old-mine", "mine-bat", 45),
        ("old-mine", "ore-crawler", 40),
        ("old-mine", "crystal-wisp", 15),
        ("misty-swamp", "swamp-toad", 50),
        ("misty-swamp", "bog-witchling", 35),
        ("misty-swamp", "crystal-wisp", 15),
        ("fallen-fortress", "bone-guard", 45),
        ("fallen-fortress", "fallen-knight", 35),
        ("fallen-fortress", "lich-acolyte", 20),
        ("obsidian-coast", "ash-siren", 45),
        ("obsidian-coast", "obsidian-crab", 40),
        ("obsidian-coast", "cyclops-apprentice", 15),
        ("titan-steppe", "titan-raider", 45),
        ("titan-steppe", "bronze-hoplite", 35),
        ("titan-steppe", "cyclops-apprentice", 20),
        ("oracle-ruins", "oracle-sphinx", 45),
        ("oracle-ruins", "bronze-hoplite", 30),
        ("oracle-ruins", "gorgon-sentinel", 25),
        ("styx-marsh", "styx-ferryman", 45),
        ("styx-marsh", "eidolon-guard", 35),
        ("styx-marsh", "gorgon-sentinel", 20),
        ("olympus-gate", "olympus-sentinel", 45),
        ("olympus-gate", "eidolon-guard", 30),
        ("olympus-gate", "titan-warden", 25),
    ];
    for (area_id, monster_id, weight) in rows {
        conn.execute(
            "INSERT INTO area_monsters(area_id, monster_id, weight) VALUES (?1, ?2, ?3)
             ON CONFLICT(area_id, monster_id) DO UPDATE SET weight=excluded.weight",
            params![area_id, monster_id, weight],
        )?;
    }
    Ok(())
}

fn seed_dungeon_monsters(conn: &Connection) -> Result<()> {
    let rows = [
        ("goblin-den", "goblin-scout", 45),
        ("goblin-den", "goblin-brute", 40),
        ("goblin-den", "moss-wolf", 15),
        ("crystal-cave", "ore-crawler", 35),
        ("crystal-cave", "crystal-wisp", 40),
        ("crystal-cave", "crystal-gazer", 25),
        ("lich-tomb", "bone-guard", 35),
        ("lich-tomb", "fallen-knight", 30),
        ("lich-tomb", "lich-acolyte", 35),
        ("cyclops-forge", "obsidian-crab", 25),
        ("cyclops-forge", "cyclops-apprentice", 45),
        ("cyclops-forge", "titan-raider", 30),
        ("medusa-temple", "oracle-sphinx", 35),
        ("medusa-temple", "gorgon-sentinel", 45),
        ("medusa-temple", "eidolon-guard", 20),
        ("titan-vault", "olympus-sentinel", 35),
        ("titan-vault", "eidolon-guard", 25),
        ("titan-vault", "titan-warden", 40),
    ];
    for (dungeon_id, monster_id, weight) in rows {
        conn.execute(
            "INSERT INTO dungeon_monsters(dungeon_id, monster_id, weight) VALUES (?1, ?2, ?3)
             ON CONFLICT(dungeon_id, monster_id) DO UPDATE SET weight=excluded.weight",
            params![dungeon_id, monster_id, weight],
        )?;
    }
    Ok(())
}

pub fn seed_aliases(conn: &Connection) -> Result<()> {
    let aliases = [
        ("ko", "숲가장자리", "area", "forest-edge"),
        ("ko", "숲길", "area", "forest-edge"),
        ("ko", "숲", "area", "forest-edge"),
        ("ko", "초보훈련장", "area", "training-field"),
        ("ko", "훈련장", "area", "training-field"),
        ("ko", "훈련", "area", "training-field"),
        ("ko", "낡은광산", "area", "old-mine"),
        ("ko", "광산", "area", "old-mine"),
        ("ko", "안개늪", "area", "misty-swamp"),
        ("ko", "늪지", "area", "misty-swamp"),
        ("ko", "늪", "area", "misty-swamp"),
        ("ko", "무너진요새", "area", "fallen-fortress"),
        ("ko", "요새", "area", "fallen-fortress"),
        ("ko", "흑요해안", "area", "obsidian-coast"),
        ("ko", "흑요", "area", "obsidian-coast"),
        ("ko", "해안", "area", "obsidian-coast"),
        ("ko", "거인초원", "area", "titan-steppe"),
        ("ko", "티탄초원", "area", "titan-steppe"),
        ("ko", "초원", "area", "titan-steppe"),
        ("ko", "예언자유적", "area", "oracle-ruins"),
        ("ko", "예언유적", "area", "oracle-ruins"),
        ("ko", "유적", "area", "oracle-ruins"),
        ("ko", "스틱스늪", "area", "styx-marsh"),
        ("ko", "스틱스", "area", "styx-marsh"),
        ("ko", "올림포스관문", "area", "olympus-gate"),
        ("ko", "올림포스", "area", "olympus-gate"),
        ("ko", "관문", "area", "olympus-gate"),
        ("ko", "고블린소굴", "dungeon", "goblin-den"),
        ("ko", "고블굴", "dungeon", "goblin-den"),
        ("ko", "고블", "dungeon", "goblin-den"),
        ("ko", "수정동굴", "dungeon", "crystal-cave"),
        ("ko", "수정굴", "dungeon", "crystal-cave"),
        ("ko", "수정", "dungeon", "crystal-cave"),
        ("ko", "리치무덤", "dungeon", "lich-tomb"),
        ("ko", "리치묘", "dungeon", "lich-tomb"),
        ("ko", "리치", "dungeon", "lich-tomb"),
        ("ko", "키클롭스대장간", "dungeon", "cyclops-forge"),
        ("ko", "키클롭스", "dungeon", "cyclops-forge"),
        ("ko", "대장간", "dungeon", "cyclops-forge"),
        ("ko", "메두사신전", "dungeon", "medusa-temple"),
        ("ko", "메두사", "dungeon", "medusa-temple"),
        ("ko", "신전", "dungeon", "medusa-temple"),
        ("ko", "티탄금고", "dungeon", "titan-vault"),
        ("ko", "티탄", "dungeon", "titan-vault"),
        ("ko", "금고", "dungeon", "titan-vault"),
        ("ko", "보린", "companion", "borin"),
        ("ko", "라이라", "companion", "lyra"),
        ("ko", "강화", "command", "enhance"),
    ];
    for (locale, alias, kind, id) in aliases {
        conn.execute(
            "INSERT OR IGNORE INTO localization_aliases(locale, alias, canonical_type, canonical_id) VALUES (?1, ?2, ?3, ?4)",
            params![locale, alias, kind, id],
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct EncounterMonster {
    pub id: String,
    pub name: String,
    pub name_ko: String,
    pub monster_grade: String,
    pub difficulty_bonus: i32,
}

pub fn pick_area_monster(conn: &Connection, area_id: &str, seed: u64) -> Result<EncounterMonster> {
    pick_weighted_monster(
        conn,
        "SELECT m.id, m.name, m.name_ko, m.monster_grade, m.difficulty_bonus, am.weight
         FROM area_monsters am
         JOIN monsters m ON m.id = am.monster_id
         WHERE am.area_id = ?1
         ORDER BY m.id",
        area_id,
        seed,
    )
}

pub fn pick_dungeon_monster(
    conn: &Connection,
    dungeon_id: &str,
    seed: u64,
) -> Result<EncounterMonster> {
    pick_weighted_monster(
        conn,
        "SELECT m.id, m.name, m.name_ko, m.monster_grade, m.difficulty_bonus, dm.weight
         FROM dungeon_monsters dm
         JOIN monsters m ON m.id = dm.monster_id
         WHERE dm.dungeon_id = ?1
         ORDER BY m.id",
        dungeon_id,
        seed,
    )
}

fn pick_weighted_monster(
    conn: &Connection,
    sql: &str,
    scope_id: &str,
    seed: u64,
) -> Result<EncounterMonster> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([scope_id], |row| {
        Ok((
            EncounterMonster {
                id: row.get(0)?,
                name: row.get(1)?,
                name_ko: row.get(2)?,
                monster_grade: row.get(3)?,
                difficulty_bonus: row.get(4)?,
            },
            row.get::<_, i64>(5)?.max(1) as u64,
        ))
    })?;
    let entries: Vec<(EncounterMonster, u64)> = rows.collect::<rusqlite::Result<_>>()?;
    let total_weight: u64 = entries.iter().map(|(_, weight)| *weight).sum();
    if total_weight == 0 {
        anyhow::bail!("no monsters configured for {scope_id}");
    }
    let mut pick = seed % total_weight;
    for (monster, weight) in entries {
        if pick < weight {
            return Ok(monster);
        }
        pick -= weight;
    }
    anyhow::bail!("no monster selected for {scope_id}")
}

#[derive(Debug, Clone)]
pub struct EquipmentDefinition {
    pub item_id: String,
    pub slot: String,
    pub name: String,
    pub rarity: String,
    pub rarity_color: String,
    pub tier: u8,
    pub stat1_type: String,
    pub stat1_value: i32,
    pub stat2_type: String,
    pub stat2_value: i32,
    pub stat3_type: Option<String>,
    pub stat3_value: Option<i32>,
    pub power_score: i32,
}

#[derive(Debug, Clone)]
pub struct EnhancementRule {
    pub enhancement_level: u8,
    pub stat_multiplier_bps: i32,
    pub upgrade_gold_cost: Option<i64>,
    pub success_rate: Option<f64>,
    pub failure_level_drop: u8,
}

pub fn seed_equipment_catalog(conn: &Connection) -> Result<()> {
    seed_enhancement_rules(conn)?;
    seed_enhancement_modifiers(conn)?;
    seed_equipment_items(conn)?;
    seed_monster_equipment_drops(conn)?;
    Ok(())
}

fn seed_enhancement_rules(conn: &Connection) -> Result<()> {
    let rules = [
        (0, 10000, Some(35), Some(0.95), 1),
        (1, 10800, Some(55), Some(0.90), 1),
        (2, 11600, Some(80), Some(0.84), 1),
        (3, 12500, Some(120), Some(0.76), 1),
        (4, 13500, Some(175), Some(0.66), 1),
        (5, 14600, Some(245), Some(0.55), 1),
        (6, 15800, Some(340), Some(0.43), 1),
        (7, 17100, Some(460), Some(0.32), 1),
        (8, 18500, Some(620), Some(0.22), 1),
        (9, 20000, Some(820), Some(0.14), 1),
        (10, 21600, None, None, 0),
    ];
    for (level, multiplier, cost, success, drop) in rules {
        conn.execute(
            "INSERT INTO equipment_enhancement_rules(enhancement_level, stat_multiplier_bps, upgrade_gold_cost, success_rate, failure_level_drop) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(enhancement_level) DO UPDATE SET stat_multiplier_bps=excluded.stat_multiplier_bps, upgrade_gold_cost=excluded.upgrade_gold_cost, success_rate=excluded.success_rate, failure_level_drop=excluded.failure_level_drop",
            params![level, multiplier, cost, success, drop],
        )?;
    }
    Ok(())
}

fn seed_enhancement_modifiers(conn: &Connection) -> Result<()> {
    let rarities = [
        ("일반", 10000, 400),
        ("고급", 12000, 150),
        ("희귀", 14500, -150),
        ("영웅", 18000, -450),
        ("전설", 23000, -800),
    ];
    for (rarity, cost_bps, success_delta_bps) in rarities {
        conn.execute(
            "INSERT INTO equipment_enhancement_rarity_modifiers(rarity, cost_multiplier_bps, success_delta_bps) VALUES (?1, ?2, ?3)
             ON CONFLICT(rarity) DO UPDATE SET cost_multiplier_bps=excluded.cost_multiplier_bps, success_delta_bps=excluded.success_delta_bps",
            params![rarity, cost_bps, success_delta_bps],
        )?;
    }

    let slots = [
        ("weapon", 12000, -200),
        ("subweapon", 9000, 150),
        ("armor_top", 11000, 0),
        ("armor_bottom", 10500, 50),
        ("trinket", 11500, -100),
        ("boots", 10800, 50),
        ("pet", 12500, -150),
        ("special", 13000, -250),
    ];
    for (slot, cost_bps, success_delta_bps) in slots {
        conn.execute(
            "INSERT INTO equipment_enhancement_slot_modifiers(slot, cost_multiplier_bps, success_delta_bps) VALUES (?1, ?2, ?3)
             ON CONFLICT(slot) DO UPDATE SET cost_multiplier_bps=excluded.cost_multiplier_bps, success_delta_bps=excluded.success_delta_bps",
            params![slot, cost_bps, success_delta_bps],
        )?;
    }
    Ok(())
}

fn seed_monster_equipment_drops(conn: &Connection) -> Result<()> {
    let rows = [
        // Training: learn equipment loops from common gear; uncommon is the early chase.
        ("training-scarab", "normal", "일반", 0.026_f64, 780, 1, 1),
        ("training-scarab", "normal", "고급", 0.007_f64, 220, 1, 1),
        ("target-golem", "normal", "일반", 0.028_f64, 760, 1, 1),
        ("target-golem", "normal", "고급", 0.009_f64, 240, 1, 1),
        // Greenwood / first dungeon: common remains useful, uncommon upgrades bridge into rare.
        ("twig-imp", "normal", "일반", 0.030_f64, 620, 1, 1),
        ("twig-imp", "normal", "고급", 0.018_f64, 320, 1, 2),
        ("twig-imp", "normal", "희귀", 0.004_f64, 60, 2, 2),
        ("moss-wolf", "normal", "일반", 0.032_f64, 600, 1, 1),
        ("moss-wolf", "normal", "고급", 0.020_f64, 320, 1, 2),
        ("moss-wolf", "normal", "희귀", 0.006_f64, 80, 2, 2),
        ("goblin-scout", "normal", "일반", 0.034_f64, 560, 1, 1),
        ("goblin-scout", "normal", "고급", 0.022_f64, 320, 1, 2),
        ("goblin-scout", "normal", "희귀", 0.010_f64, 120, 2, 3),
        ("goblin-brute", "elite", "고급", 0.026_f64, 680, 2, 2),
        ("goblin-brute", "elite", "희귀", 0.018_f64, 320, 2, 3),
        // Deep Delves / Crystal Cave: rare tier-3 becomes the main upgrade target.
        ("mine-bat", "normal", "고급", 0.024_f64, 700, 2, 2),
        ("mine-bat", "normal", "희귀", 0.018_f64, 300, 2, 3),
        ("ore-crawler", "normal", "고급", 0.026_f64, 640, 2, 2),
        ("ore-crawler", "normal", "희귀", 0.022_f64, 360, 2, 3),
        ("crystal-wisp", "elite", "고급", 0.028_f64, 560, 2, 2),
        ("crystal-wisp", "elite", "희귀", 0.030_f64, 440, 3, 3),
        ("crystal-gazer", "elite", "고급", 0.030_f64, 500, 2, 2),
        ("crystal-gazer", "elite", "희귀", 0.034_f64, 500, 3, 3),
        // Cursed Frontier / Lich Tomb: rare stays common enough, epic begins as elite/dungeon chase.
        ("swamp-toad", "normal", "고급", 0.026_f64, 520, 2, 2),
        ("swamp-toad", "normal", "희귀", 0.030_f64, 480, 3, 3),
        ("bog-witchling", "elite", "희귀", 0.036_f64, 760, 3, 3),
        ("bog-witchling", "elite", "영웅", 0.006_f64, 240, 1, 1),
        ("bone-guard", "normal", "희귀", 0.034_f64, 840, 3, 3),
        ("bone-guard", "normal", "영웅", 0.004_f64, 160, 1, 1),
        ("fallen-knight", "elite", "희귀", 0.038_f64, 700, 3, 3),
        ("fallen-knight", "elite", "영웅", 0.010_f64, 300, 1, 2),
        ("lich-acolyte", "elite", "희귀", 0.040_f64, 620, 3, 3),
        ("lich-acolyte", "elite", "영웅", 0.014_f64, 380, 1, 2),
        // Atlas Shore / Cyclops Forge: epic tier-2 is the expected dungeon-prep upgrade.
        ("ash-siren", "normal", "희귀", 0.036_f64, 720, 3, 3),
        ("ash-siren", "normal", "영웅", 0.008_f64, 280, 1, 1),
        ("obsidian-crab", "normal", "희귀", 0.038_f64, 680, 3, 3),
        ("obsidian-crab", "normal", "영웅", 0.010_f64, 320, 1, 2),
        ("cyclops-apprentice", "elite", "희귀", 0.040_f64, 520, 3, 3),
        ("cyclops-apprentice", "elite", "영웅", 0.018_f64, 480, 1, 2),
        // Titan Highlands / Medusa Temple: epic tier-3 is stable before legendary unlocks.
        ("titan-raider", "normal", "희귀", 0.036_f64, 440, 3, 3),
        ("titan-raider", "normal", "영웅", 0.020_f64, 560, 2, 2),
        ("bronze-hoplite", "normal", "희귀", 0.034_f64, 360, 3, 3),
        ("bronze-hoplite", "normal", "영웅", 0.024_f64, 640, 2, 3),
        ("oracle-sphinx", "elite", "영웅", 0.030_f64, 820, 2, 3),
        ("gorgon-sentinel", "elite", "영웅", 0.034_f64, 760, 2, 3),
        // Olympus Frontier / Titan Vault: legendary becomes visible while epic remains the floor.
        ("styx-ferryman", "normal", "영웅", 0.030_f64, 820, 3, 3),
        ("styx-ferryman", "normal", "전설", 0.010_f64, 220, 1, 2),
        ("eidolon-guard", "elite", "영웅", 0.036_f64, 720, 3, 3),
        ("olympus-sentinel", "elite", "영웅", 0.038_f64, 620, 3, 3),
        ("olympus-sentinel", "elite", "전설", 0.016_f64, 380, 2, 3),
        ("titan-warden", "elite", "영웅", 0.040_f64, 520, 3, 3),
        ("titan-warden", "elite", "전설", 0.024_f64, 480, 2, 3),
    ];
    conn.execute("DELETE FROM monster_equipment_drops", [])?;
    for (monster_id, grade, rarity, chance, weight, min_tier, max_tier) in rows {
        conn.execute(
            "INSERT INTO monster_equipment_drops(monster_id, monster_grade, rarity, drop_chance, weight, min_tier, max_tier) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(monster_id, rarity) DO UPDATE SET monster_grade=excluded.monster_grade, drop_chance=excluded.drop_chance, weight=excluded.weight, min_tier=excluded.min_tier, max_tier=excluded.max_tier",
            params![monster_id, grade, rarity, chance, weight, min_tier, max_tier],
        )?;
    }
    Ok(())
}

fn seed_equipment_items(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE inventory_items SET equipped_slot = 'armor_top' WHERE equipped_slot = 'armor'",
        [],
    )?;
    conn.execute(
        "UPDATE inventory_items SET equipped_slot = 'subweapon' WHERE equipped_slot = 'offhand'",
        [],
    )?;
    conn.execute(
        "UPDATE inventory_items SET equipped_slot = NULL WHERE equipped_slot = 'bag'",
        [],
    )?;
    conn.execute(
        "DELETE FROM equipment_items WHERE slot IN ('armor', 'offhand', 'bag')",
        [],
    )?;
    let rarities = [
        ("일반", "white", 1_i64, 1.00_f64),
        ("고급", "green", 3_i64, 1.28_f64),
        ("희귀", "blue", 3_i64, 1.70_f64),
        ("영웅", "purple", 3_i64, 2.30_f64),
        ("전설", "yellow", 3_i64, 3.10_f64),
    ];
    let slots = [
        EquipmentSeedSlot {
            slot: "weapon",
            label_ko: "무기",
            stat1: "attack",
            stat2: "accuracy",
            stat3: Some("speed"),
            base1: 7,
            base2: 4,
            base3: Some(2),
            roots: [
                "아레스",
                "아킬레우스",
                "헤라클레스",
                "페르세우스",
                "알렉산드로스",
            ],
        },
        EquipmentSeedSlot {
            slot: "subweapon",
            label_ko: "부무기",
            stat1: "attack",
            stat2: "accuracy",
            stat3: Some("speed"),
            base1: 6,
            base2: 4,
            base3: Some(1),
            roots: [
                "아이아스",
                "오디세우스",
                "파트로클로스",
                "솔론",
                "리쿠르고스",
            ],
        },
        EquipmentSeedSlot {
            slot: "armor_top",
            label_ko: "상의",
            stat1: "defense",
            stat2: "max_hp",
            stat3: Some("evasion"),
            base1: 5,
            base2: 24,
            base3: Some(2),
            roots: ["아테나", "헥토르", "레오니다스", "테세우스", "미노스"],
        },
        EquipmentSeedSlot {
            slot: "armor_bottom",
            label_ko: "하의",
            stat1: "max_hp",
            stat2: "defense",
            stat3: Some("regen"),
            base1: 28,
            base2: 4,
            base3: Some(1),
            roots: ["스파르타", "마라톤", "테르모필레", "코린토스", "아르고스"],
        },
        EquipmentSeedSlot {
            slot: "trinket",
            label_ko: "장신구",
            stat1: "attack",
            stat2: "max_hp",
            stat3: Some("accuracy"),
            base1: 4,
            base2: 18,
            base3: Some(3),
            roots: [
                "헤르메스",
                "아폴론",
                "오르페우스",
                "피타고라스",
                "히포크라테스",
            ],
        },
        EquipmentSeedSlot {
            slot: "boots",
            label_ko: "신발",
            stat1: "speed",
            stat2: "max_hp",
            stat3: Some("max_mp"),
            base1: 3,
            base2: 20,
            base3: Some(10),
            roots: ["니케", "아탈란테", "이카로스", "페가수스", "페이디피데스"],
        },
        EquipmentSeedSlot {
            slot: "pet",
            label_ko: "펫",
            stat1: "luck",
            stat2: "regen",
            stat3: Some("xp_bonus"),
            base1: 3,
            base2: 1,
            base3: Some(2),
            roots: ["아르테미스", "케르베로스", "키르케", "칼립소", "판"],
        },
        EquipmentSeedSlot {
            slot: "special",
            label_ko: "특수장비",
            stat1: "evasion",
            stat2: "max_hp",
            stat3: Some("defense"),
            base1: 3,
            base2: 18,
            base3: Some(3),
            roots: ["헤카테", "프로메테우스", "델포이", "크로노스", "가이아"],
        },
    ];
    let tier_titles = ["I", "II", "III"];
    for spec in slots {
        for (rarity_index, (rarity, color, max_tier, rarity_mul)) in rarities.iter().enumerate() {
            for tier in 1..=*max_tier {
                let tier_mul = 1.0 + ((tier - 1) as f64 * 0.125);
                let stat1_value = scaled_seed_stat(spec.base1, *rarity_mul, tier_mul);
                let stat2_value = scaled_seed_stat(spec.base2, *rarity_mul, tier_mul);
                let legendary = *rarity == "전설";
                let stat3_type = if spec.slot == "pet" && legendary && tier % 2 == 0 {
                    Some("gold_bonus")
                } else {
                    legendary.then_some(spec.stat3).flatten()
                };
                let stat3_value = stat3_type
                    .map(|_| scaled_seed_stat(spec.base3.unwrap_or(0), *rarity_mul, tier_mul));
                let power_score = stat_power(spec.stat1, stat1_value)
                    + stat_power(spec.stat2, stat2_value)
                    + stat3_type
                        .zip(stat3_value)
                        .map(|(stat, value)| stat_power(stat, value))
                        .unwrap_or(0);
                let root = spec.roots[rarity_index];
                let name = equipment_name(spec.slot, root, rarity, tier as u8);
                let item_id = format!("eq-{}-{}-t{}", spec.slot, rarity_slug(rarity), tier);
                let flavor = format!(
                    "{} 장비. 명칭: {root}. 등급: {rarity}. 단계: {title}.",
                    spec.label_ko,
                    title = tier_titles[(tier - 1) as usize]
                );
                conn.execute(
                    "INSERT INTO equipment_items(item_id, slot, name, rarity, rarity_color, tier, stat1_type, stat1_value, stat2_type, stat2_value, stat3_type, stat3_value, power_score, flavor) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(item_id) DO UPDATE SET slot=excluded.slot, name=excluded.name, rarity=excluded.rarity, rarity_color=excluded.rarity_color, tier=excluded.tier, stat1_type=excluded.stat1_type, stat1_value=excluded.stat1_value, stat2_type=excluded.stat2_type, stat2_value=excluded.stat2_value, stat3_type=excluded.stat3_type, stat3_value=excluded.stat3_value, power_score=excluded.power_score, flavor=excluded.flavor",
                    params![item_id, spec.slot, name, rarity, color, tier, spec.stat1, stat1_value, spec.stat2, stat2_value, stat3_type, stat3_value, power_score, flavor],
                )?;
            }
        }
    }
    seed_legacy_equipment_items(conn)?;
    Ok(())
}

fn seed_legacy_equipment_items(conn: &Connection) -> Result<()> {
    let rows = [
        (
            "basic-sword",
            "weapon",
            "Ares Sword",
            "일반",
            "white",
            1_i64,
            "attack",
            7,
            "accuracy",
            4,
            None,
            None,
            18,
            "상점 보급형 무기",
        ),
        (
            "basic-staff",
            "weapon",
            "Hermes Staff",
            "일반",
            "white",
            1_i64,
            "attack",
            6,
            "accuracy",
            5,
            None,
            None,
            18,
            "상점 보급형 지팡이",
        ),
        (
            "leather-armor",
            "armor_top",
            "Leonidas Armor",
            "일반",
            "white",
            1_i64,
            "defense",
            5,
            "max_hp",
            24,
            None,
            None,
            17,
            "상점 보급형 상의",
        ),
        (
            "goblin-chief-axe",
            "weapon",
            "Hector Axe",
            "고급",
            "green",
            1_i64,
            "attack",
            9,
            "accuracy",
            5,
            None,
            None,
            23,
            "고블린 소굴 보스 보상 무기",
        ),
        (
            "crystal-blade",
            "weapon",
            "Perseus Blade",
            "희귀",
            "blue",
            1_i64,
            "attack",
            12,
            "accuracy",
            7,
            None,
            None,
            31,
            "수정 동굴 보스 보상 무기",
        ),
        (
            "lich-amulet",
            "trinket",
            "Hecate Amulet",
            "영웅",
            "purple",
            1_i64,
            "attack",
            9,
            "max_hp",
            41,
            None,
            None,
            29,
            "리치 무덤 보스 보상 장신구",
        ),
        (
            "cyclops-hammer",
            "weapon",
            "Hephaestus Hammer",
            "영웅",
            "purple",
            1_i64,
            "attack",
            16,
            "accuracy",
            9,
            None,
            None,
            40,
            "키클롭스 대장간 보스 보상 무기",
        ),
        (
            "gorgon-seal",
            "special",
            "Athena Aegis",
            "영웅",
            "purple",
            1_i64,
            "evasion",
            7,
            "max_hp",
            41,
            None,
            None,
            27,
            "메두사 신전 보스 보상 특수장비",
        ),
        (
            "titan-key",
            "trinket",
            "Kronos Key",
            "전설",
            "yellow",
            1_i64,
            "attack",
            12,
            "max_hp",
            56,
            Some("accuracy"),
            Some(9),
            43,
            "티탄 금고 보스 보상 장신구",
        ),
    ];
    for (
        item_id,
        slot,
        name,
        rarity,
        color,
        tier,
        stat1_type,
        stat1_value,
        stat2_type,
        stat2_value,
        stat3_type,
        stat3_value,
        power_score,
        flavor,
    ) in rows
    {
        conn.execute(
            "INSERT INTO equipment_items(item_id, slot, name, rarity, rarity_color, tier, stat1_type, stat1_value, stat2_type, stat2_value, stat3_type, stat3_value, power_score, flavor)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
             ON CONFLICT(item_id) DO UPDATE SET slot=excluded.slot, name=excluded.name, rarity=excluded.rarity, rarity_color=excluded.rarity_color, tier=excluded.tier, stat1_type=excluded.stat1_type, stat1_value=excluded.stat1_value, stat2_type=excluded.stat2_type, stat2_value=excluded.stat2_value, stat3_type=excluded.stat3_type, stat3_value=excluded.stat3_value, power_score=excluded.power_score, flavor=excluded.flavor",
            params![
                item_id,
                slot,
                name,
                rarity,
                color,
                tier,
                stat1_type,
                stat1_value,
                stat2_type,
                stat2_value,
                stat3_type,
                stat3_value,
                power_score,
                flavor
            ],
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct EquipmentSeedSlot {
    slot: &'static str,
    label_ko: &'static str,
    stat1: &'static str,
    stat2: &'static str,
    stat3: Option<&'static str>,
    base1: i32,
    base2: i32,
    base3: Option<i32>,
    roots: [&'static str; 5],
}

fn scaled_seed_stat(base: i32, rarity_mul: f64, tier_mul: f64) -> i32 {
    ((base as f64) * rarity_mul * tier_mul).round().max(1.0) as i32
}

fn equipment_name(slot: &str, root: &str, _rarity: &str, tier: u8) -> String {
    let noun = match slot {
        "weapon" => ["검", "창", "월도"][(tier as usize - 1).min(2)],
        "subweapon" => ["단검", "방패", "성표"][(tier as usize - 1).min(2)],
        "armor_top" => ["흉갑", "갑주", "외투"][(tier as usize - 1).min(2)],
        "armor_bottom" => ["각반", "갑각", "전포"][(tier as usize - 1).min(2)],
        "trinket" => ["반지", "귀걸이", "목걸이"][(tier as usize - 1).min(2)],
        "boots" => ["신발", "장화", "발걸이"][(tier as usize - 1).min(2)],
        "pet" => ["동료", "정령", "수호수"][(tier as usize - 1).min(2)],
        _ => ["토템", "성핵", "기관"][(tier as usize - 1).min(2)],
    };
    format!("{root} {noun}")
}

fn rarity_slug(rarity: &str) -> &'static str {
    match rarity {
        "일반" => "common",
        "고급" => "uncommon",
        "희귀" => "rare",
        "영웅" => "epic",
        "전설" => "legendary",
        _ => "unknown",
    }
}

fn stat_power(stat: &str, value: i32) -> i32 {
    let weight = match stat {
        "max_hp" => 1.0 / 6.0,
        "max_mp" => 1.0 / 4.0,
        "accuracy" | "evasion" | "speed" | "luck" | "regen" => 2.0,
        _ => 4.0,
    };
    ((value as f64) * weight).round() as i32
}

pub fn load_equipment_definition(
    conn: &Connection,
    item_id: &str,
) -> Result<Option<EquipmentDefinition>> {
    conn.query_row(
        "SELECT item_id, slot, name, rarity, rarity_color, tier, stat1_type, stat1_value, stat2_type, stat2_value, stat3_type, stat3_value, power_score FROM equipment_items WHERE item_id = ?1",
        [item_id],
        |row| {
            Ok(EquipmentDefinition {
                item_id: row.get(0)?,
                slot: row.get(1)?,
                name: row.get(2)?,
                rarity: row.get(3)?,
                rarity_color: row.get(4)?,
                tier: row.get::<_, i64>(5)?.max(0) as u8,
                stat1_type: row.get(6)?,
                stat1_value: row.get(7)?,
                stat2_type: row.get(8)?,
                stat2_value: row.get(9)?,
                stat3_type: row.get(10)?,
                stat3_value: row.get(11)?,
                power_score: row.get(12)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn load_enhancement_rule(conn: &Connection, level: u8) -> Result<Option<EnhancementRule>> {
    conn.query_row(
        "SELECT enhancement_level, stat_multiplier_bps, upgrade_gold_cost, success_rate, failure_level_drop FROM equipment_enhancement_rules WHERE enhancement_level = ?1",
        [level as i64],
        |row| {
            Ok(EnhancementRule {
                enhancement_level: row.get::<_, i64>(0)?.max(0) as u8,
                stat_multiplier_bps: row.get(1)?,
                upgrade_gold_cost: row.get(2)?,
                success_rate: row.get(3)?,
                failure_level_drop: row.get::<_, i64>(4)?.max(0) as u8,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

pub fn adjusted_enhancement_rule(
    conn: &Connection,
    level: u8,
    rarity: &str,
    slot: &str,
) -> Result<Option<EnhancementRule>> {
    let Some(mut rule) = load_enhancement_rule(conn, level)? else {
        return Ok(None);
    };
    if let Some(cost) = rule.upgrade_gold_cost {
        let rarity_cost = enhancement_cost_multiplier(
            conn,
            "equipment_enhancement_rarity_modifiers",
            "rarity",
            rarity,
        )?;
        let slot_cost = enhancement_cost_multiplier(
            conn,
            "equipment_enhancement_slot_modifiers",
            "slot",
            slot,
        )?;
        rule.upgrade_gold_cost =
            Some((((cost * rarity_cost) + 9_999) / 10_000 * slot_cost + 9_999) / 10_000);
    }
    if let Some(rate) = rule.success_rate {
        let rarity_delta = enhancement_success_delta(
            conn,
            "equipment_enhancement_rarity_modifiers",
            "rarity",
            rarity,
        )?;
        let slot_delta =
            enhancement_success_delta(conn, "equipment_enhancement_slot_modifiers", "slot", slot)?;
        rule.success_rate =
            Some((rate + ((rarity_delta + slot_delta) as f64 / 10_000.0)).clamp(0.05, 0.98));
    }
    Ok(Some(rule))
}

fn enhancement_cost_multiplier(
    conn: &Connection,
    table: &str,
    column: &str,
    value: &str,
) -> Result<i64> {
    let sql = format!("SELECT cost_multiplier_bps FROM {table} WHERE {column} = ?1");
    conn.query_row(&sql, [value], |row| row.get::<_, i64>(0))
        .optional()
        .map(|value| value.unwrap_or(10_000))
        .map_err(Into::into)
}

fn enhancement_success_delta(
    conn: &Connection,
    table: &str,
    column: &str,
    value: &str,
) -> Result<i64> {
    let sql = format!("SELECT success_delta_bps FROM {table} WHERE {column} = ?1");
    conn.query_row(&sql, [value], |row| row.get::<_, i64>(0))
        .optional()
        .map(|value| value.unwrap_or(0))
        .map_err(Into::into)
}

pub fn effective_equipment_stats(
    def: &EquipmentDefinition,
    multiplier_bps: i32,
) -> Vec<(String, i32)> {
    effective_equipment_stats_for_slot(def, multiplier_bps, def.slot.as_str())
}

pub fn effective_equipment_stats_for_slot(
    def: &EquipmentDefinition,
    multiplier_bps: i32,
    equipped_slot: &str,
) -> Vec<(String, i32)> {
    let multiplier_bps = effective_equipment_multiplier_bps(equipped_slot, multiplier_bps);
    let mut stats = vec![
        (
            def.stat1_type.clone(),
            scale_stat(def.stat1_value, multiplier_bps),
        ),
        (
            def.stat2_type.clone(),
            scale_stat(def.stat2_value, multiplier_bps),
        ),
    ];
    if let (Some(stat), Some(value)) = (&def.stat3_type, def.stat3_value) {
        stats.push((stat.clone(), scale_stat(value, multiplier_bps)));
    }
    stats
}

pub fn effective_equipment_multiplier_bps(slot: &str, multiplier_bps: i32) -> i32 {
    if slot == "subweapon" {
        (multiplier_bps + 1) / 2
    } else {
        multiplier_bps
    }
}

pub fn scale_stat(value: i32, multiplier_bps: i32) -> i32 {
    ((value * multiplier_bps) + 9_999) / 10_000
}

pub fn upsert_player(conn: &Connection, p: &PlayerState) -> Result<()> {
    let now = now();
    conn.execute(
        "INSERT INTO player_state(id, name, class_id, level, xp, xp_to_next, hp, max_hp, mp, max_mp, attack, defense, accuracy, evasion, speed, regen, luck, gold, current_area_id, current_dungeon_id, mode, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?22)
         ON CONFLICT(id) DO UPDATE SET name=excluded.name, class_id=excluded.class_id, level=excluded.level, xp=excluded.xp, xp_to_next=excluded.xp_to_next, hp=excluded.hp, max_hp=excluded.max_hp, mp=excluded.mp, max_mp=excluded.max_mp, attack=excluded.attack, defense=excluded.defense, accuracy=excluded.accuracy, evasion=excluded.evasion, speed=excluded.speed, regen=excluded.regen, luck=excluded.luck, gold=excluded.gold, current_area_id=excluded.current_area_id, current_dungeon_id=excluded.current_dungeon_id, mode=excluded.mode, updated_at=excluded.updated_at",
        params![p.id, p.name, p.class_id, p.level, p.xp, p.xp_to_next, p.hp, p.max_hp, p.mp, p.max_mp, p.attack, p.defense, p.accuracy, p.evasion, p.speed, p.regen, p.luck, p.gold, p.current_area_id, p.current_dungeon_id, p.mode, now],
    )?;
    Ok(())
}

pub fn load_player(conn: &Connection) -> Result<PlayerState> {
    conn.query_row(
        "SELECT id, name, class_id, level, xp, xp_to_next, hp, max_hp, mp, max_mp, attack, defense, accuracy, evasion, speed, regen, luck, gold, current_area_id, current_dungeon_id, mode FROM player_state WHERE id = ?1",
        [DEFAULT_PLAYER_ID],
        |row| {
            Ok(PlayerState {
                id: row.get(0)?,
                name: row.get(1)?,
                class_id: row.get(2)?,
                level: row.get::<_, i64>(3)? as u32,
                xp: row.get::<_, i64>(4)? as u64,
                xp_to_next: row.get::<_, i64>(5)? as u64,
                hp: row.get(6)?,
                max_hp: row.get(7)?,
                mp: row.get(8)?,
                max_mp: row.get(9)?,
                attack: row.get(10)?,
                defense: row.get(11)?,
                accuracy: row.get(12)?,
                evasion: row.get(13)?,
                speed: row.get(14)?,
                regen: row.get(15)?,
                luck: row.get(16)?,
                gold: row.get(17)?,
                current_area_id: row.get(18)?,
                current_dungeon_id: row.get(19)?,
                mode: row.get(20)?,
            })
        },
    ).context("player_state missing; run vibemud init")
}

pub fn load_daily_quests(conn: &Connection) -> Result<Vec<DailyQuest>> {
    ensure_daily_quests(conn)?;
    load_daily_quests_readonly(conn)
}

pub fn load_daily_quests_readonly(conn: &Connection) -> Result<Vec<DailyQuest>> {
    let today = quest_today();
    let mut stmt = conn.prepare(
        "SELECT quest_id, progress, target, status, reward_kind, reward_amount, fever_minutes
         FROM daily_quests
         WHERE quest_date = ?1
         ORDER BY assigned_at, quest_id",
    )?;
    let rows = stmt.query_map([today], |row| {
        let quest_id = row.get::<_, String>(0)?;
        Ok(DailyQuest {
            title: quest_title(&quest_id),
            quest_id,
            progress: row.get(1)?,
            target: row.get(2)?,
            status: row.get(3)?,
            reward_kind: row.get(4)?,
            reward_amount: row.get(5)?,
            fever_minutes: row.get(6)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn claim_daily_quest(conn: &Connection, quest_id: &str) -> Result<String> {
    ensure_daily_quests(conn)?;
    let today = quest_today();
    let row: Option<(String, i64, String, i64)> = conn
        .query_row(
            "SELECT status, reward_amount, reward_kind, fever_minutes
             FROM daily_quests
             WHERE quest_date = ?1 AND (quest_id = ?2 OR id = ?2)
             LIMIT 1",
            params![today, quest_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((status, reward_amount, reward_kind, fever_minutes)) = row else {
        anyhow::bail!("quest {quest_id} not found");
    };
    match status.as_str() {
        "claimed" => anyhow::bail!("quest {quest_id} reward already claimed"),
        "completed" => {}
        _ => anyhow::bail!("quest {quest_id} is not complete"),
    }

    let mut player = load_player(conn)?;
    let reward_label = apply_quest_reward(&mut player, &reward_kind, reward_amount);
    upsert_player(conn, &player)?;
    let fever_until = mod_bridge::grant_reward(conn, fever_minutes)?;
    conn.execute(
        "UPDATE daily_quests SET status = 'claimed', claimed_at = ?1 WHERE quest_date = ?2 AND (quest_id = ?3 OR id = ?3)",
        params![now(), today, quest_id],
    )?;
    append_event(
        conn,
        EventKind::CommandProcessed,
        format!(
            "Daily quest claimed: {}. Reward: {reward_label}, FEVERTIME +{fever_minutes}m.",
            quest_title(quest_id)
        ),
        None,
    )?;
    write_snapshot_and_hud(conn)?;
    Ok(format!(
        "{} | {} | FEVERTIME +{}분 (~{})",
        quest_title(quest_id),
        reward_label,
        fever_minutes,
        fever_until
    ))
}

pub fn claim_all_daily_quests(conn: &Connection) -> Result<Vec<String>> {
    ensure_daily_quests(conn)?;
    let today = quest_today();
    let ids = {
        let mut stmt = conn.prepare(
            "SELECT quest_id FROM daily_quests WHERE quest_date = ?1 AND status = 'completed' ORDER BY assigned_at, quest_id",
        )?;
        let rows = stmt.query_map([today], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    if ids.is_empty() {
        return Ok(vec!["수령 가능한 완료 퀘스트가 없습니다.".to_string()]);
    }
    ids.into_iter()
        .map(|quest_id| claim_daily_quest(conn, &quest_id))
        .collect()
}

pub fn record_quest_progress(
    conn: &Connection,
    metric: &str,
    slot: Option<&str>,
    amount: i64,
) -> Result<()> {
    if amount <= 0 {
        return Ok(());
    }
    ensure_daily_quests(conn)?;
    let today = quest_today();
    let active = {
        let mut stmt = conn.prepare(
            "SELECT quest_id, progress, target FROM daily_quests WHERE quest_date = ?1 AND status = 'active'",
        )?;
        let rows = stmt.query_map([today.clone()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (quest_id, progress, target) in active {
        let Some(def) = quest_definition(&quest_id) else {
            continue;
        };
        if def.metric != metric {
            continue;
        }
        if let Some(required_slot) = def.slot {
            if slot != Some(required_slot) {
                continue;
            }
        }
        let next = progress.saturating_add(amount).min(target);
        if next >= target {
            conn.execute(
                "UPDATE daily_quests SET progress = ?1, status = 'completed', completed_at = COALESCE(completed_at, ?2) WHERE quest_date = ?3 AND quest_id = ?4",
                params![next, now(), today, quest_id],
            )?;
        } else {
            conn.execute(
                "UPDATE daily_quests SET progress = ?1 WHERE quest_date = ?2 AND quest_id = ?3",
                params![next, today, quest_id],
            )?;
        }
    }
    Ok(())
}

fn ensure_daily_quests(conn: &Connection) -> Result<()> {
    let today = quest_today();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM daily_quests WHERE quest_date = ?1",
        [today.as_str()],
        |row| row.get(0),
    )?;
    if count > 0 {
        return Ok(());
    }
    for def in select_daily_quest_definitions(&today) {
        let fever_minutes = quest_fever_minutes(&today, &def);
        conn.execute(
            "INSERT INTO daily_quests(id, quest_date, quest_id, progress, target, status, reward_kind, reward_amount, fever_minutes, assigned_at)
             VALUES (?1, ?2, ?3, 0, ?4, 'active', ?5, ?6, ?7, ?8)",
            params![
                format!("{today}:{}", def.id),
                today,
                def.id,
                def.target,
                def.reward_kind,
                def.reward_amount,
                fever_minutes,
                now()
            ],
        )?;
    }
    Ok(())
}

fn select_daily_quest_definitions(date: &str) -> Vec<QuestDefinition> {
    const BUCKETS: [&[&str]; 5] = [
        &["kill-5", "kill-10", "kill-15", "kill-20", "boss-1"],
        &["spend-1000", "spend-3000", "spend-5000", "spend-10000"],
        &[
            "enhance-weapon-1",
            "enhance-weapon-3",
            "enhance-top-2",
            "enhance-bottom-2",
            "enhance-trinket-2",
            "enhance-boots-2",
            "enhance-pet-1",
            "enhance-special-2",
        ],
        &[
            "enhance-weapon-success-1",
            "enhance-weapon-fail-1",
            "enhance-top-fail-2",
            "enhance-bottom-success-1",
            "enhance-trinket-success-1",
            "enhance-boots-success-1",
            "enhance-pet-success-1",
            "enhance-special-fail-1",
        ],
        &[
            "enhance-any-5",
            "enhance-any-success-2",
            "enhance-any-fail-2",
            "sell-common-3",
            "sell-item-2",
        ],
    ];
    BUCKETS
        .iter()
        .filter_map(|bucket| {
            bucket
                .iter()
                .filter_map(|id| quest_definition(id))
                .min_by_key(|def| quest_hash(&format!("{date}:{}", def.id)))
        })
        .collect()
}

fn apply_quest_reward(player: &mut PlayerState, reward_kind: &str, amount: i64) -> String {
    match reward_kind {
        "xp" => {
            let logs = vibemud_core::apply_xp(player, amount.max(0) as u64);
            if logs.is_empty() {
                format!("+{amount} XP")
            } else {
                format!("+{amount} XP, {}", logs.join(" / "))
            }
        }
        _ => {
            player.gold += amount.max(0);
            format!("+{amount} gold")
        }
    }
}

fn quest_today() -> String {
    (OffsetDateTime::now_utc() + Duration::hours(9))
        .date()
        .to_string()
}

fn quest_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn quest_fever_minutes(date: &str, def: &QuestDefinition) -> i64 {
    let min = def.fever_min.min(def.fever_max);
    let max = def.fever_min.max(def.fever_max);
    min + (quest_hash(&format!("{date}:{}:fever", def.id)) % ((max - min + 1) as u64)) as i64
}

fn quest_title(quest_id: &str) -> String {
    quest_definition(quest_id)
        .map(|def| def.title_ko.to_string())
        .unwrap_or_else(|| quest_id.to_string())
}

fn quest_definition(quest_id: &str) -> Option<QuestDefinition> {
    daily_quest_definitions()
        .iter()
        .copied()
        .find(|def| def.id == quest_id)
}

fn daily_quest_definitions() -> &'static [QuestDefinition] {
    &DAILY_QUEST_DEFINITIONS
}

const DAILY_QUEST_DEFINITIONS: [QuestDefinition; 30] = [
    QuestDefinition {
        id: "kill-5",
        title_ko: "몬스터 5회 처치",
        metric: "monster_kill",
        slot: None,
        target: 5,
        reward_kind: "xp",
        reward_amount: 80,
        fever_min: 10,
        fever_max: 15,
    },
    QuestDefinition {
        id: "kill-10",
        title_ko: "몬스터 10회 처치",
        metric: "monster_kill",
        slot: None,
        target: 10,
        reward_kind: "gold",
        reward_amount: 350,
        fever_min: 12,
        fever_max: 18,
    },
    QuestDefinition {
        id: "kill-15",
        title_ko: "몬스터 15회 처치",
        metric: "monster_kill",
        slot: None,
        target: 15,
        reward_kind: "xp",
        reward_amount: 180,
        fever_min: 15,
        fever_max: 22,
    },
    QuestDefinition {
        id: "kill-20",
        title_ko: "몬스터 20회 처치",
        metric: "monster_kill",
        slot: None,
        target: 20,
        reward_kind: "gold",
        reward_amount: 800,
        fever_min: 18,
        fever_max: 30,
    },
    QuestDefinition {
        id: "boss-1",
        title_ko: "보스 1회 처치",
        metric: "boss_kill",
        slot: None,
        target: 1,
        reward_kind: "xp",
        reward_amount: 220,
        fever_min: 20,
        fever_max: 30,
    },
    QuestDefinition {
        id: "spend-1000",
        title_ko: "머니 1,000 사용",
        metric: "gold_spend",
        slot: None,
        target: 1_000,
        reward_kind: "xp",
        reward_amount: 100,
        fever_min: 10,
        fever_max: 15,
    },
    QuestDefinition {
        id: "spend-3000",
        title_ko: "머니 3,000 사용",
        metric: "gold_spend",
        slot: None,
        target: 3_000,
        reward_kind: "gold",
        reward_amount: 500,
        fever_min: 12,
        fever_max: 18,
    },
    QuestDefinition {
        id: "spend-5000",
        title_ko: "머니 5,000 사용",
        metric: "gold_spend",
        slot: None,
        target: 5_000,
        reward_kind: "xp",
        reward_amount: 180,
        fever_min: 15,
        fever_max: 24,
    },
    QuestDefinition {
        id: "spend-10000",
        title_ko: "머니 10,000 사용",
        metric: "gold_spend",
        slot: None,
        target: 10_000,
        reward_kind: "gold",
        reward_amount: 1_500,
        fever_min: 20,
        fever_max: 30,
    },
    QuestDefinition {
        id: "enhance-weapon-1",
        title_ko: "무기 1회 강화",
        metric: "enhance_attempt",
        slot: Some("weapon"),
        target: 1,
        reward_kind: "xp",
        reward_amount: 90,
        fever_min: 10,
        fever_max: 15,
    },
    QuestDefinition {
        id: "enhance-weapon-3",
        title_ko: "무기 3회 강화",
        metric: "enhance_attempt",
        slot: Some("weapon"),
        target: 3,
        reward_kind: "gold",
        reward_amount: 450,
        fever_min: 12,
        fever_max: 20,
    },
    QuestDefinition {
        id: "enhance-weapon-success-1",
        title_ko: "무기 강화 1회 성공",
        metric: "enhance_success",
        slot: Some("weapon"),
        target: 1,
        reward_kind: "xp",
        reward_amount: 120,
        fever_min: 10,
        fever_max: 18,
    },
    QuestDefinition {
        id: "enhance-weapon-fail-1",
        title_ko: "무기 강화 1회 실패",
        metric: "enhance_fail",
        slot: Some("weapon"),
        target: 1,
        reward_kind: "gold",
        reward_amount: 300,
        fever_min: 10,
        fever_max: 18,
    },
    QuestDefinition {
        id: "enhance-top-2",
        title_ko: "상의 2회 강화",
        metric: "enhance_attempt",
        slot: Some("armor_top"),
        target: 2,
        reward_kind: "xp",
        reward_amount: 110,
        fever_min: 10,
        fever_max: 18,
    },
    QuestDefinition {
        id: "enhance-top-fail-2",
        title_ko: "상의 강화 2회 실패",
        metric: "enhance_fail",
        slot: Some("armor_top"),
        target: 2,
        reward_kind: "gold",
        reward_amount: 650,
        fever_min: 15,
        fever_max: 25,
    },
    QuestDefinition {
        id: "enhance-bottom-2",
        title_ko: "하의 2회 강화",
        metric: "enhance_attempt",
        slot: Some("armor_bottom"),
        target: 2,
        reward_kind: "xp",
        reward_amount: 110,
        fever_min: 10,
        fever_max: 18,
    },
    QuestDefinition {
        id: "enhance-bottom-success-1",
        title_ko: "하의 강화 1회 성공",
        metric: "enhance_success",
        slot: Some("armor_bottom"),
        target: 1,
        reward_kind: "gold",
        reward_amount: 350,
        fever_min: 10,
        fever_max: 18,
    },
    QuestDefinition {
        id: "enhance-trinket-2",
        title_ko: "장신구 2회 강화",
        metric: "enhance_attempt",
        slot: Some("trinket"),
        target: 2,
        reward_kind: "xp",
        reward_amount: 120,
        fever_min: 10,
        fever_max: 20,
    },
    QuestDefinition {
        id: "enhance-trinket-success-1",
        title_ko: "장신구 강화 1회 성공",
        metric: "enhance_success",
        slot: Some("trinket"),
        target: 1,
        reward_kind: "gold",
        reward_amount: 400,
        fever_min: 10,
        fever_max: 20,
    },
    QuestDefinition {
        id: "enhance-boots-2",
        title_ko: "신발 2회 강화",
        metric: "enhance_attempt",
        slot: Some("boots"),
        target: 2,
        reward_kind: "xp",
        reward_amount: 120,
        fever_min: 10,
        fever_max: 20,
    },
    QuestDefinition {
        id: "enhance-boots-success-1",
        title_ko: "신발 강화 1회 성공",
        metric: "enhance_success",
        slot: Some("boots"),
        target: 1,
        reward_kind: "gold",
        reward_amount: 400,
        fever_min: 10,
        fever_max: 20,
    },
    QuestDefinition {
        id: "enhance-pet-1",
        title_ko: "펫 1회 강화",
        metric: "enhance_attempt",
        slot: Some("pet"),
        target: 1,
        reward_kind: "xp",
        reward_amount: 130,
        fever_min: 12,
        fever_max: 20,
    },
    QuestDefinition {
        id: "enhance-pet-success-1",
        title_ko: "펫 강화 1회 성공",
        metric: "enhance_success",
        slot: Some("pet"),
        target: 1,
        reward_kind: "gold",
        reward_amount: 450,
        fever_min: 12,
        fever_max: 20,
    },
    QuestDefinition {
        id: "enhance-special-2",
        title_ko: "특수장비 2회 강화",
        metric: "enhance_attempt",
        slot: Some("special"),
        target: 2,
        reward_kind: "xp",
        reward_amount: 140,
        fever_min: 12,
        fever_max: 22,
    },
    QuestDefinition {
        id: "enhance-special-fail-1",
        title_ko: "특수장비 강화 1회 실패",
        metric: "enhance_fail",
        slot: Some("special"),
        target: 1,
        reward_kind: "gold",
        reward_amount: 450,
        fever_min: 12,
        fever_max: 22,
    },
    QuestDefinition {
        id: "enhance-any-5",
        title_ko: "장비 5회 강화",
        metric: "enhance_attempt",
        slot: None,
        target: 5,
        reward_kind: "xp",
        reward_amount: 220,
        fever_min: 15,
        fever_max: 25,
    },
    QuestDefinition {
        id: "enhance-any-success-2",
        title_ko: "장비 강화 2회 성공",
        metric: "enhance_success",
        slot: None,
        target: 2,
        reward_kind: "gold",
        reward_amount: 700,
        fever_min: 15,
        fever_max: 25,
    },
    QuestDefinition {
        id: "enhance-any-fail-2",
        title_ko: "장비 강화 2회 실패",
        metric: "enhance_fail",
        slot: None,
        target: 2,
        reward_kind: "xp",
        reward_amount: 240,
        fever_min: 15,
        fever_max: 25,
    },
    QuestDefinition {
        id: "sell-common-3",
        title_ko: "일반 아이템 3개 판매",
        metric: "sell_common",
        slot: None,
        target: 3,
        reward_kind: "gold",
        reward_amount: 300,
        fever_min: 10,
        fever_max: 18,
    },
    QuestDefinition {
        id: "sell-item-2",
        title_ko: "아이템 2회 판매",
        metric: "item_sell",
        slot: None,
        target: 2,
        reward_kind: "xp",
        reward_amount: 100,
        fever_min: 10,
        fever_max: 18,
    },
];

pub fn load_snapshot(conn: &Connection) -> Result<GameSnapshot> {
    let latest: Option<String> = conn
        .query_row(
            "SELECT snapshot_json FROM state_snapshots ORDER BY state_version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(json) = latest {
        let mut snapshot: GameSnapshot = serde_json::from_str(&json)?;
        snapshot.clock_tick = runtime_clock_tick(conn)?;
        snapshot.combat = load_combat_state(conn)?;
        Ok(snapshot)
    } else {
        build_snapshot(conn)
    }
}

pub fn build_snapshot(conn: &Connection) -> Result<GameSnapshot> {
    let player = load_player(conn)?;
    let state_version = latest_state_version(conn)?;
    let clock_tick = runtime_clock_tick(conn)?;
    let party = load_party(conn)?;
    let inventory = load_inventory(conn)?;
    let combat = load_combat_state(conn)?;
    let recent_log = recent_logs(conn, 10)?;
    Ok(GameSnapshot {
        state_version,
        clock_tick,
        player,
        party,
        inventory,
        combat,
        recent_log,
    })
}

fn load_combat_state(conn: &Connection) -> Result<vibemud_core::CombatState> {
    conn.query_row(
        "SELECT in_combat, encounter_id, encounter_seed, turn_index, monster_group_json FROM combat_state WHERE id = 'main'",
        [],
        |row| {
            let seed = row
                .get::<_, Option<i64>>(2)?
                .and_then(|value| (value >= 0).then_some(value as u64));
            let monster_group_json = row.get::<_, Option<String>>(4)?;
            let monster_group = monster_group_json
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .and_then(|value| serde_json::from_str::<serde_json::Value>(value).ok());
            let monster_name = monster_group
                .as_ref()
                .and_then(|value| value.get("monster_name"))
                .and_then(|value| value.as_str())
                .map(str::to_string);
            let monster_hp = monster_group
                .as_ref()
                .and_then(|value| value.get("monster_hp"))
                .and_then(|value| value.as_i64())
                .map(|value| value as i32);
            let monster_max_hp = monster_group
                .as_ref()
                .and_then(|value| value.get("monster_max_hp"))
                .and_then(|value| value.as_i64())
                .map(|value| value as i32);
            Ok(vibemud_core::CombatState {
                in_combat: row.get::<_, i64>(0)? != 0,
                encounter_id: row.get(1)?,
                encounter_seed: seed,
                turn_index: row.get::<_, i64>(3)?.max(0) as u32,
                monster_name,
                monster_hp,
                monster_max_hp,
            })
        },
    )
    .optional()
    .map(|combat| combat.unwrap_or_default())
    .map_err(Into::into)
}

pub fn load_party(conn: &Connection) -> Result<Vec<vibemud_core::CompanionState>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.name, c.role, c.rarity, c.unlocked FROM party_slots p JOIN companions c ON p.companion_id = c.id ORDER BY p.slot_index",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(vibemud_core::CompanionState {
            id: row.get(0)?,
            name: row.get(1)?,
            role: row.get(2)?,
            rarity: row.get(3)?,
            unlocked: row.get::<_, i64>(4)? != 0,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn load_inventory(conn: &Connection) -> Result<Vec<InventoryItem>> {
    let mut stmt = conn.prepare(
        "SELECT i.id, i.item_id, i.item_type, COALESCE(e.name, i.name), COALESCE(e.rarity, i.rarity), i.quantity, i.durability, i.max_durability, i.equipped_slot,
                COALESCE(i.locked, 0), COALESCE(i.enhancement_level, 0), e.rarity_color, e.tier,
                e.stat1_type, e.stat1_value, e.stat2_type, e.stat2_value, e.stat3_type, e.stat3_value,
                e.power_score, COALESCE(r.stat_multiplier_bps, 10000), COALESCE(i.equipped_slot, e.slot)
         FROM inventory_items i
         LEFT JOIN equipment_items e ON e.item_id = i.item_id
         LEFT JOIN equipment_enhancement_rules r ON r.enhancement_level = COALESCE(i.enhancement_level, 0)
         ORDER BY CASE WHEN i.equipped_slot IS NULL THEN 1 ELSE 0 END, i.equipped_slot, i.acquired_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let multiplier_bps = effective_equipment_multiplier_bps(
            row.get::<_, Option<String>>(21)?.as_deref().unwrap_or(""),
            row.get::<_, i32>(20)?,
        );
        Ok(InventoryItem {
            id: row.get(0)?,
            item_id: row.get(1)?,
            item_type: row.get(2)?,
            name: row.get(3)?,
            rarity: row.get(4)?,
            quantity: row.get::<_, i64>(5)? as u32,
            durability: row.get(6)?,
            max_durability: row.get(7)?,
            equipped_slot: row.get(8)?,
            locked: row.get::<_, i64>(9).unwrap_or(0) != 0,
            enhancement_level: row.get::<_, i64>(10).unwrap_or(0).max(0) as u8,
            rarity_color: row.get(11)?,
            tier: row
                .get::<_, Option<i64>>(12)?
                .map(|value| value.max(0) as u8),
            stat1_type: row.get(13)?,
            stat1_value: row
                .get::<_, Option<i32>>(14)?
                .map(|value| scale_stat(value, multiplier_bps)),
            stat2_type: row.get(15)?,
            stat2_value: row
                .get::<_, Option<i32>>(16)?
                .map(|value| scale_stat(value, multiplier_bps)),
            stat3_type: row.get(17)?,
            stat3_value: row
                .get::<_, Option<i32>>(18)?
                .map(|value| scale_stat(value, multiplier_bps)),
            power_score: row
                .get::<_, Option<i32>>(19)?
                .map(|value| scale_stat(value, multiplier_bps)),
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn recent_logs(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    Ok(recent_log_entries(conn, limit)?
        .into_iter()
        .map(|entry| entry.message)
        .collect())
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub state_version: u64,
    pub created_at: String,
    pub event_type: String,
    pub message: String,
}

pub fn recent_log_entries(conn: &Connection, limit: usize) -> Result<Vec<LogEntry>> {
    let mut stmt =
        conn.prepare("SELECT created_at, event_type, payload_json, state_version FROM event_log ORDER BY state_version DESC LIMIT ?1")?;
    let rows = stmt.query_map([limit as i64], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (created_at, event_type, json, state_version) = row?;
        if let Ok(event) = serde_json::from_str::<GameEvent>(&json) {
            out.push(LogEntry {
                state_version: state_version.max(0) as u64,
                created_at,
                event_type,
                message: event.message,
            });
        }
    }
    Ok(out)
}

pub fn latest_state_version(conn: &Connection) -> Result<u64> {
    let value: Option<i64> =
        conn.query_row("SELECT MAX(state_version) FROM event_log", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?;
    Ok(value.unwrap_or(0).max(0) as u64)
}
pub fn append_event(
    conn: &Connection,
    kind: EventKind,
    message: impl Into<String>,
    rng_seed: Option<u64>,
) -> Result<u64> {
    let state_version = latest_state_version(conn)? + 1;
    let event = GameEvent {
        kind: kind.clone(),
        message: message.into(),
        state_version,
        rng_seed,
    };
    conn.execute(
        "INSERT INTO event_log(id, created_at, event_type, payload_json, state_version, rng_seed, random_draw_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)",
        params![Uuid::new_v4().to_string(), now(), kind.as_str(), serde_json::to_string(&event)?, state_version as i64, rng_seed.map(|v| v as i64)],
    )?;
    Ok(state_version)
}

pub fn write_snapshot_and_hud(conn: &Connection) -> Result<GameSnapshot> {
    let snapshot = build_snapshot(conn)?;
    let json = serde_json::to_string(&snapshot)?;
    conn.execute(
        "DELETE FROM state_snapshots WHERE state_version = ?1",
        [snapshot.state_version as i64],
    )?;
    conn.execute(
        "INSERT INTO state_snapshots(id, created_at, state_version, snapshot_json) VALUES (?1, ?2, ?3, ?4)",
        params![Uuid::new_v4().to_string(), now(), snapshot.state_version as i64, json],
    )?;
    let hud = vibemud_hud_state(&snapshot);
    conn.execute(
        "INSERT INTO hud_state(id, updated_at, state_version, one_line, compact_json, full_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET updated_at=excluded.updated_at, state_version=excluded.state_version, one_line=excluded.one_line, compact_json=excluded.compact_json, full_json=excluded.full_json",
        params![DEFAULT_HUD_ID, now(), hud.state_version as i64, hud.one_line, hud.compact_json, hud.full_json],
    )?;
    conn.execute(
        "UPDATE session_state SET last_seen_state_version = ?1 WHERE id = ?2",
        params![snapshot.state_version as i64, DEFAULT_SESSION_ID],
    )?;
    prune_history(conn)?;
    Ok(snapshot)
}

fn prune_history(conn: &Connection) -> Result<()> {
    prune_history_with_limits(
        conn,
        EVENT_LOG_RETENTION_ROWS,
        STATE_SNAPSHOT_RETENTION_ROWS,
    )
}

fn prune_history_with_limits(
    conn: &Connection,
    event_log_limit: i64,
    snapshot_limit: i64,
) -> Result<()> {
    let event_log_limit = event_log_limit.max(1);
    let snapshot_limit = snapshot_limit.max(1);
    conn.execute(
        "DELETE FROM event_log
         WHERE id NOT IN (
             SELECT id FROM event_log
             ORDER BY state_version DESC
             LIMIT ?1
         )",
        [event_log_limit],
    )?;
    conn.execute(
        "DELETE FROM state_snapshots
         WHERE id NOT IN (
             SELECT id FROM state_snapshots
             ORDER BY state_version DESC, created_at DESC, id DESC
             LIMIT ?1
         )",
        [snapshot_limit],
    )?;
    Ok(())
}

fn vibemud_hud_state(snapshot: &GameSnapshot) -> vibemud_core::HudStateDto {
    let dto = vibemud_core::StatusLineDto::from(snapshot);
    vibemud_core::HudStateDto {
        state_version: snapshot.state_version,
        one_line: format!(
            "[VibeMUD] Lv.{} {} | HP {}/{} | {} | {} | Party {}/4 | Danger: {} | Loot {}",
            dto.level,
            dto.class_label,
            dto.hp,
            dto.max_hp,
            dto.area_label,
            dto.mode_label,
            dto.party_count,
            dto.danger_label,
            dto.loot_count
        ),
        compact_json: format!("{{\"text\":\"Lv{} {}\"}}", dto.level, dto.class_label),
        full_json: serde_json::to_string(snapshot).unwrap_or_else(|_| "{}".to_string()),
    }
}

pub fn enqueue_command(
    conn: &Connection,
    source: &str,
    kind: CommandKind,
    payload: &CommandPayload,
) -> Result<String> {
    if kind.class() == vibemud_core::CommandClass::ReadOnly {
        anyhow::bail!("read-only command {} must not be enqueued", kind.as_str());
    }
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO command_queue(id, created_at, source, command_type, payload_json, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, now(), source, kind.as_str(), serde_json::to_string(payload)?, CommandStatus::Pending.as_str()],
    )?;
    Ok(id)
}

#[derive(Debug, Clone)]
pub struct QueuedCommand {
    pub id: String,
    pub kind: String,
    pub payload: CommandPayload,
}

pub fn pending_commands(conn: &Connection, limit: usize) -> Result<Vec<QueuedCommand>> {
    let mut stmt = conn.prepare("SELECT id, command_type, payload_json FROM command_queue WHERE status = 'pending' ORDER BY created_at LIMIT ?1")?;
    let mut rows = stmt.query([limit as i64])?;
    let mut commands = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let payload_json: String = row.get(2)?;
        let payload = serde_json::from_str::<CommandPayload>(&payload_json)
            .with_context(|| format!("invalid payload_json for queued command {id}"))?;
        commands.push(QueuedCommand { id, kind, payload });
    }
    Ok(commands)
}

pub fn mark_processing(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE command_queue SET status = 'processing' WHERE id = ?1 AND status = 'pending'",
        [id],
    )?;
    Ok(())
}

pub fn mark_done(conn: &Connection, id: &str, result: &str) -> Result<()> {
    conn.execute("UPDATE command_queue SET status = 'done', processed_at = ?1, result_json = ?2 WHERE id = ?3", params![now(), result, id])?;
    Ok(())
}

pub fn mark_failed(conn: &Connection, id: &str, error: &str) -> Result<()> {
    conn.execute("UPDATE command_queue SET status = 'failed', processed_at = ?1, error_message = ?2 WHERE id = ?3", params![now(), error, id])?;
    Ok(())
}

pub fn reset_game_state(conn: &mut Connection) -> Result<GameSnapshot> {
    reset_game_state_if_generation(conn, None)
}

pub fn reset_game_state_if_generation(
    conn: &mut Connection,
    expected: Option<&str>,
) -> Result<GameSnapshot> {
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if let Some(expected) = expected {
        anyhow::ensure!(
            mod_bridge::generation(&tx)? == expected,
            "save changed; refresh before resetting"
        );
    }
    tx.execute_batch(
        r#"
DELETE FROM command_queue;
DELETE FROM mod_sessions;
DELETE FROM settings;
DELETE FROM hud_state;
DELETE FROM state_snapshots;
DELETE FROM event_log;
DELETE FROM combat_state;
DELETE FROM daily_quests;
DELETE FROM party_slots;
DELETE FROM companions;
DELETE FROM inventory_items;
DELETE FROM player_state;
DELETE FROM session_state;
"#,
    )?;
    seed_initial_state(&tx)?;
    let snapshot = build_snapshot(&tx)?;
    tx.commit()?;
    Ok(snapshot)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        params![key, value, now()],
    )?;
    Ok(())
}

pub fn setting_value(conn: &Connection, key: &str) -> Result<Option<String>> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
        row.get(0)
    })
    .optional()
    .map_err(Into::into)
}

pub fn clear_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}

pub fn set_session_status(conn: &Connection, status: &str, pid: Option<u32>) -> Result<()> {
    let now = now();
    match status {
        "running" => conn.execute(
            "INSERT INTO session_state(id, runtime_pid, started_at, status, last_seen_state_version) VALUES (?1, ?2, ?3, 'running', ?4)
             ON CONFLICT(id) DO UPDATE SET runtime_pid=excluded.runtime_pid, started_at=excluded.started_at, status='running', last_seen_state_version=excluded.last_seen_state_version",
            params![DEFAULT_SESSION_ID, pid.map(i64::from), now, latest_state_version(conn)? as i64],
        )?,
        _ => conn.execute(
            "INSERT INTO session_state(id, stopped_at, status, last_seen_state_version) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET stopped_at=excluded.stopped_at, status=excluded.status, last_seen_state_version=excluded.last_seen_state_version",
            params![DEFAULT_SESSION_ID, now, status, latest_state_version(conn)? as i64],
        )?,
    };
    Ok(())
}

pub fn session_status(conn: &Connection) -> Result<String> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM session_state WHERE id = ?1",
            [DEFAULT_SESSION_ID],
            |row| row.get(0),
        )
        .optional()?;
    Ok(status.unwrap_or_else(|| "stopped".to_string()))
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub status: String,
    pub runtime_pid: Option<i64>,
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub last_seen_state_version: u64,
}

type SessionInfoRow = (String, Option<i64>, Option<String>, Option<String>, i64);

pub fn session_info(conn: &Connection) -> Result<SessionInfo> {
    let row: Option<SessionInfoRow> = conn
        .query_row(
            "SELECT status, runtime_pid, started_at, stopped_at, last_seen_state_version FROM session_state WHERE id = ?1",
            [DEFAULT_SESSION_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()?;
    let Some((status, runtime_pid, started_at, stopped_at, last_seen_state_version)) = row else {
        return Ok(SessionInfo {
            status: "stopped".to_string(),
            runtime_pid: None,
            started_at: None,
            stopped_at: None,
            last_seen_state_version: 0,
        });
    };
    Ok(SessionInfo {
        status,
        runtime_pid,
        started_at,
        stopped_at,
        last_seen_state_version: last_seen_state_version.max(0) as u64,
    })
}

#[derive(Debug, Clone)]
pub struct QueueCounts {
    pub pending: u64,
    pub processing: u64,
    pub done: u64,
    pub failed: u64,
}

pub fn command_queue_counts(conn: &Connection) -> Result<QueueCounts> {
    let mut counts = QueueCounts {
        pending: 0,
        processing: 0,
        done: 0,
        failed: 0,
    };
    let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM command_queue GROUP BY status")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (status, count) = row?;
        let count = count.max(0) as u64;
        match status.as_str() {
            "pending" => counts.pending = count,
            "processing" => counts.processing = count,
            "done" => counts.done = count,
            "failed" => counts.failed = count,
            _ => {}
        }
    }
    Ok(counts)
}

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub id: String,
    pub created_at: String,
    pub processed_at: Option<String>,
    pub source: String,
    pub command_type: String,
    pub status: String,
    pub error_message: Option<String>,
    pub result_json: Option<String>,
}

pub fn recent_queue_entries(conn: &Connection, limit: usize) -> Result<Vec<QueueEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, created_at, processed_at, source, command_type, status, error_message, result_json FROM command_queue ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit as i64], |row| {
        Ok(QueueEntry {
            id: row.get(0)?,
            created_at: row.get(1)?,
            processed_at: row.get(2)?,
            source: row.get(3)?,
            command_type: row.get(4)?,
            status: row.get(5)?,
            error_message: row.get(6)?,
            result_json: row.get(7)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

#[derive(Debug, Clone)]
pub struct HudInfo {
    pub updated_at: Option<String>,
    pub state_version: u64,
}

pub fn hud_info(conn: &Connection) -> Result<HudInfo> {
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT updated_at, state_version FROM hud_state WHERE id = ?1",
            [DEFAULT_HUD_ID],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        Some((updated_at, state_version)) => HudInfo {
            updated_at: Some(updated_at),
            state_version: state_version.max(0) as u64,
        },
        None => HudInfo {
            updated_at: None,
            state_version: 0,
        },
    })
}

pub fn command_queue_count(conn: &Connection) -> Result<u64> {
    let value: i64 = conn.query_row("SELECT COUNT(*) FROM command_queue", [], |row| row.get(0))?;
    Ok(value as u64)
}

pub fn pragma_value(conn: &Connection, name: &str) -> Result<String> {
    let sql = format!("PRAGMA {name}");
    let value: String = conn
        .query_row(&sql, [], |row| row.get::<_, String>(0))
        .unwrap_or_else(|_| "".to_string());
    Ok(value)
}

pub fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(unix)]
fn restrict_file(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_file(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_dir(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if path.exists() {
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o700);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_dir(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn init_is_idempotent_and_sets_pragmas() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        init_app().unwrap();
        let (_paths, conn) = open_app().unwrap();
        assert_eq!(command_queue_count(&conn).unwrap(), 0);
        assert!(pragma_value(&conn, "journal_mode")
            .unwrap()
            .to_lowercase()
            .contains("wal"));
        let migrations: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(migrations, 3);

        let equipment_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM equipment_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(equipment_count, 113);
        let obsolete_slots: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM equipment_items WHERE slot IN ('armor', 'offhand', 'bag')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(obsolete_slots, 0);
        let enhancement_rules: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM equipment_enhancement_rules",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(enhancement_rules, 11);
        let normal_drop_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM monster_equipment_drops WHERE monster_id = 'training-scarab'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(normal_drop_rows, 2);
        let region_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM regions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(region_count, 7);
        let area_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM areas", [], |row| row.get(0))
            .unwrap();
        assert_eq!(area_count, 10);
        let dungeon_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM dungeons", [], |row| row.get(0))
            .unwrap();
        assert_eq!(dungeon_count, 6);
        let forest_monsters: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM area_monsters WHERE area_id = 'forest-edge'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(forest_monsters, 3);
        let goblin_monster = pick_dungeon_monster(&conn, "goblin-den", 7).unwrap();
        assert!(goblin_monster.difficulty_bonus > 0);
    }

    #[test]
    fn regen_backfill_migration_raises_stale_regen_without_lowering_existing() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        let (_paths, conn) = open_app().unwrap();

        // Simulate two pre-migration saves: one whose regen is stale relative
        // to the level-based curve, and one whose regen already exceeds it.
        conn.execute(
            "INSERT INTO player_state(id, name, class_id, level, xp, xp_to_next, hp, max_hp, mp, max_mp, attack, defense, accuracy, evasion, speed, regen, luck, gold, current_area_id, current_dungeon_id, mode, created_at, updated_at)
             VALUES ('stale-player', 'Stale', 'warrior', 30, 0, 100, 100, 100, 30, 30, 10, 8, 70, 8, 10, 3, 5, 0, NULL, NULL, 'field', ?1, ?1)",
            [now()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO player_state(id, name, class_id, level, xp, xp_to_next, hp, max_hp, mp, max_mp, attack, defense, accuracy, evasion, speed, regen, luck, gold, current_area_id, current_dungeon_id, mode, created_at, updated_at)
             VALUES ('ahead-player', 'Ahead', 'warrior', 3, 0, 100, 100, 100, 30, 30, 10, 8, 70, 8, 10, 10, 5, 0, NULL, NULL, 'field', ?1, ?1)",
            [now()],
        )
        .unwrap();
        conn.execute("DELETE FROM schema_migrations WHERE version = 3", [])
            .unwrap();

        migrate(&conn).unwrap();

        let stale_regen: i64 = conn
            .query_row(
                "SELECT regen FROM player_state WHERE id = 'stale-player'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stale_regen, 13);

        let ahead_regen: i64 = conn
            .query_row(
                "SELECT regen FROM player_state WHERE id = 'ahead-player'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ahead_regen, 10);
    }

    #[test]
    fn monster_equipment_drop_seed_balance_locks_early_and_next_region_contracts() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        let (_paths, conn) = open_app().unwrap();

        let uncovered_progression_monsters: i64 = conn
            .query_row(
                "WITH progression_monsters AS (
                   SELECT DISTINCT m.id, m.monster_grade
                   FROM monsters m
                   WHERE m.id IN (
                     SELECT monster_id FROM area_monsters
                     UNION
                     SELECT monster_id FROM dungeon_monsters
                   )
                 )
                 SELECT COUNT(*)
                 FROM progression_monsters pm
                 WHERE NOT EXISTS (
                   SELECT 1
                   FROM monster_equipment_drops d
                   WHERE d.monster_id = pm.id
                     AND d.monster_grade = pm.monster_grade
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            uncovered_progression_monsters, 0,
            "every area/dungeon monster should have at least one equipment drop rule"
        );

        let drop_rows_without_full_slot_coverage: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM monster_equipment_drops d
                 WHERE (
                   SELECT COUNT(DISTINCT e.slot)
                   FROM equipment_items e
                   WHERE e.rarity = d.rarity
                     AND e.tier BETWEEN d.min_tier AND d.max_tier
                 ) != 8",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            drop_rows_without_full_slot_coverage, 0,
            "each drop rule must resolve to a complete seeded equipment slot set"
        );

        let early_heroic_or_better: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM monster_equipment_drops d
                 WHERE d.rarity IN ('영웅', '전설')
                   AND d.monster_id IN (
                     SELECT monster_id FROM area_monsters
                     WHERE area_id IN ('training-field', 'forest-edge')
                     UNION
                     SELECT monster_id FROM dungeon_monsters
                     WHERE dungeon_id = 'goblin-den'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            early_heroic_or_better, 0,
            "early field and first-dungeon monsters must not directly drop heroic+ equipment"
        );

        let early_common_uncommon_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM monster_equipment_drops d
                 WHERE d.rarity IN ('일반', '고급')
                   AND d.monster_id IN (
                     SELECT monster_id FROM area_monsters
                     WHERE area_id IN ('training-field', 'forest-edge')
                     UNION
                     SELECT monster_id FROM dungeon_monsters
                     WHERE dungeon_id = 'goblin-den'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            early_common_uncommon_rows >= 10,
            "early monsters should be covered by common/uncommon weighted rows"
        );

        let early_slot_coverage: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT e.slot)
                 FROM monster_equipment_drops d
                 JOIN equipment_items e
                   ON e.rarity = d.rarity
                  AND e.tier BETWEEN d.min_tier AND d.max_tier
                 WHERE d.monster_id IN (
                   SELECT monster_id FROM area_monsters
                   WHERE area_id IN ('training-field', 'forest-edge')
                   UNION
                   SELECT monster_id FROM dungeon_monsters
                   WHERE dungeon_id = 'goblin-den'
                 )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            early_slot_coverage, 8,
            "early common/uncommon/rare drops should cover every equipment slot"
        );

        let next_region_rare_chance: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(d.drop_chance), 0.0)
                 FROM monster_equipment_drops d
                 WHERE d.rarity = '희귀'
                   AND d.monster_id IN (
                     SELECT monster_id FROM area_monsters
                     WHERE area_id = 'old-mine'
                     UNION
                     SELECT monster_id FROM dungeon_monsters
                     WHERE dungeon_id = 'crystal-cave'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            (0.10..=0.12).contains(&next_region_rare_chance),
            "deep-delves rare drop chance should be visible but not flood progression, got {next_region_rare_chance}"
        );

        let cursed_frontier_epic_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM monster_equipment_drops d
                 WHERE d.rarity = '영웅'
                   AND d.monster_id IN (
                     SELECT monster_id FROM area_monsters
                     WHERE area_id IN ('misty-swamp', 'fallen-fortress')
                     UNION
                     SELECT monster_id FROM dungeon_monsters
                     WHERE dungeon_id = 'lich-tomb'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            cursed_frontier_epic_rows >= 4,
            "cursed frontier should introduce epic equipment before the next major region"
        );

        let pre_final_legendary_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM monster_equipment_drops d
                 WHERE d.rarity = '전설'
                   AND d.monster_id IN (
                     SELECT monster_id FROM area_monsters
                     WHERE area_id IN (
                       'training-field', 'forest-edge', 'old-mine', 'misty-swamp',
                       'fallen-fortress', 'obsidian-coast', 'titan-steppe', 'oracle-ruins'
                     )
                     UNION
                     SELECT monster_id FROM dungeon_monsters
                     WHERE dungeon_id IN (
                       'goblin-den', 'crystal-cave', 'lich-tomb', 'cyclops-forge',
                       'medusa-temple'
                     )
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            pre_final_legendary_rows, 0,
            "legendary equipment should not appear before olympus/titan-vault progression"
        );

        let olympus_legendary_chance: f64 = conn
            .query_row(
                "SELECT COALESCE(SUM(d.drop_chance), 0.0)
                 FROM monster_equipment_drops d
                 WHERE d.rarity = '전설'
                   AND d.monster_id IN (
                     SELECT monster_id FROM area_monsters
                     WHERE area_id IN ('styx-marsh', 'olympus-gate')
                     UNION
                     SELECT monster_id FROM dungeon_monsters
                     WHERE dungeon_id = 'titan-vault'
                   )",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            olympus_legendary_chance >= 0.05,
            "olympus/titan-vault should make legendary gear visible, got {olympus_legendary_chance}"
        );
    }

    #[test]
    fn project_marker_selects_project_storage_without_env() {
        let _guard = test_lock();
        let old_home = std::env::var_os("VIBEMUD_HOME");
        std::env::remove_var("VIBEMUD_HOME");
        let old_dir = std::env::current_dir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let nested = project.join("nested");
        let project_home = project.join(".vibemud");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&project_home).unwrap();
        std::fs::write(
            project_home.join(PROJECT_STORAGE_MARKER_FILE),
            "VibeMUD project storage\n",
        )
        .unwrap();
        std::env::set_current_dir(&nested).unwrap();

        let paths = AppPaths::discover().unwrap();

        assert_eq!(
            std::fs::canonicalize(paths.root).unwrap(),
            std::fs::canonicalize(project_home).unwrap()
        );
        std::env::set_current_dir(old_dir).unwrap();
        match old_home {
            Some(value) => std::env::set_var("VIBEMUD_HOME", value),
            None => std::env::remove_var("VIBEMUD_HOME"),
        }
    }

    #[test]
    fn vibemud_home_overrides_project_marker() {
        let _guard = test_lock();
        let old_home = std::env::var_os("VIBEMUD_HOME");
        let old_dir = std::env::current_dir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let env_home = tmp.path().join("env-home");
        let project = tmp.path().join("project");
        let project_home = project.join(".vibemud");
        std::fs::create_dir_all(&project_home).unwrap();
        std::fs::write(
            project_home.join(PROJECT_STORAGE_MARKER_FILE),
            "VibeMUD project storage\n",
        )
        .unwrap();
        std::env::set_current_dir(&project).unwrap();
        std::env::set_var("VIBEMUD_HOME", &env_home);

        let paths = AppPaths::discover().unwrap();

        assert_eq!(paths.root, env_home);
        std::env::set_current_dir(old_dir).unwrap();
        match old_home {
            Some(value) => std::env::set_var("VIBEMUD_HOME", value),
            None => std::env::remove_var("VIBEMUD_HOME"),
        }
    }

    #[test]
    fn init_backfills_popup_selector_defaults_for_existing_config() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        std::fs::create_dir_all(tmp.path()).unwrap();
        std::fs::write(
            tmp.path().join("config.toml"),
            r#"[ui]
language = "ko"
hud_mode = "side"
hud_refresh_seconds = 2
statusline_enabled = true
unicode_borders = true
compact_mode = true
ultra_compact_below_columns = 60
compact_below_columns = 100

[runtime]
tick_interval_ms = 1000
session_only_progress = true
offline_progress = false
background_daemon_enabled = false

[game]
auto_hunt_after_area_select = true
death_penalty_mode = "scaling"
growth_curve = "fast_then_slow"

[integrations]
tmux_enabled = true
codex_enabled = false
claude_enabled = false
coding_event_rewards = false

[privacy]
read_code_content = false
store_prompts = false
store_file_paths = false
store_commit_messages = false
redact_home_path_in_diagnostics = true

[database]
journal_mode = "WAL"
synchronous = "NORMAL"
busy_timeout_ms = 3000

[packaging]
prefer_multicall_binary = true
"#,
        )
        .unwrap();

        init_app().unwrap();

        let config = load_config().unwrap();
        assert!(config.ui.popup_pane_enabled);
        assert_eq!(config.ui.message_printline, 7);
        assert_eq!(config.integrations.terminal, "auto");
        let text = std::fs::read_to_string(tmp.path().join("config.toml")).unwrap();
        assert!(text.contains("popup_pane_enabled = true"));
        assert!(text.contains("message_printline = 7"));
        assert!(text.contains("terminal = \"auto\""));
    }

    #[test]
    fn vibe_activity_uses_fresh_heartbeat_only() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        assert!(!vibe_fever_active());

        write_vibe_activity("codex", true).unwrap();
        assert!(vibe_fever_active());

        let paths = AppPaths::discover().unwrap();
        let stale = VibeActivity {
            active: true,
            source: "codex".to_string(),
            updated_at: (OffsetDateTime::now_utc()
                - Duration::seconds(VIBE_FEVER_HEARTBEAT_TTL_SECONDS + 1))
            .format(&Rfc3339)
            .unwrap(),
            reward_until: None,
        };
        std::fs::write(
            &paths.vibe_activity,
            serde_json::to_string_pretty(&stale).unwrap(),
        )
        .unwrap();
        assert!(!vibe_fever_active());

        clear_vibe_activity("codex").unwrap();
        assert!(!vibe_fever_active());
        assert!(write_vibe_activity("prompt-text", true).is_err());
    }

    #[test]
    fn daily_quests_seed_five_progress_and_grant_fever_rewards() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        let (_paths, conn) = open_app().unwrap();

        let quests = load_daily_quests(&conn).unwrap();
        assert_eq!(quests.len(), 5);

        let today = quest_today();
        conn.execute(
            "DELETE FROM daily_quests WHERE quest_date = ?1",
            [today.as_str()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO daily_quests(id, quest_date, quest_id, progress, target, status, reward_kind, reward_amount, fever_minutes, assigned_at)
             VALUES (?1, ?2, 'kill-5', 0, 5, 'active', 'gold', 123, 10, ?3)",
            params![format!("{today}:kill-5"), today, now()],
        )
        .unwrap();

        record_quest_progress(&conn, "monster_kill", None, 5).unwrap();
        let quest = load_daily_quests(&conn)
            .unwrap()
            .into_iter()
            .find(|quest| quest.quest_id == "kill-5")
            .unwrap();
        assert_eq!(quest.status, "completed");
        assert_eq!(quest.progress, 5);

        let before = load_player(&conn).unwrap().gold;
        let message = claim_daily_quest(&conn, "kill-5").unwrap();
        assert!(message.contains("FEVERTIME +10분"));
        assert_eq!(load_player(&conn).unwrap().gold, before + 123);
        assert!(vibe_fever_active());
        assert_eq!(
            load_daily_quests(&conn)
                .unwrap()
                .into_iter()
                .find(|quest| quest.quest_id == "kill-5")
                .unwrap()
                .status,
            "claimed"
        );
    }

    #[test]
    fn snapshots_replace_same_version_and_history_prunes_old_rows() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        let (_paths, conn) = open_app().unwrap();

        for _ in 0..5 {
            write_snapshot_and_hud(&conn).unwrap();
        }
        let snapshot_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM state_snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(snapshot_count, 1);

        for index in 0..5 {
            append_event(
                &conn,
                EventKind::TickAdvanced,
                format!("retention test {index}"),
                None,
            )
            .unwrap();
            write_snapshot_and_hud(&conn).unwrap();
        }
        prune_history_with_limits(&conn, 3, 2).unwrap();

        let event_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM event_log", [], |row| row.get(0))
            .unwrap();
        let snapshot_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM state_snapshots", [], |row| row.get(0))
            .unwrap();
        assert_eq!(event_count, 3);
        assert_eq!(snapshot_count, 2);
        assert_eq!(latest_state_version(&conn).unwrap(), 6);
        assert_eq!(load_snapshot(&conn).unwrap().state_version, 6);
    }

    #[test]
    fn snapshot_includes_live_combat_state() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        let (_paths, conn) = open_app().unwrap();

        conn.execute(
            "UPDATE combat_state SET in_combat = 1, encounter_id = 'goblin-den-point-3', encounter_seed = 123, turn_index = 3, updated_at = ?1 WHERE id = 'main'",
            [now()],
        )
        .unwrap();
        write_snapshot_and_hud(&conn).unwrap();

        let snapshot = load_snapshot(&conn).unwrap();
        assert!(snapshot.combat.in_combat);
        assert_eq!(
            snapshot.combat.encounter_id.as_deref(),
            Some("goblin-den-point-3")
        );
        assert_eq!(snapshot.combat.encounter_seed, Some(123));
        assert_eq!(snapshot.combat.turn_index, 3);
    }

    #[test]
    fn reset_game_state_restores_initial_progress_but_keeps_catalogs() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        let (_paths, mut conn) = open_app().unwrap();

        let mut player = load_player(&conn).unwrap();
        player.level = 12;
        player.gold = 9_999;
        player.mode = "auto_hunt".to_string();
        upsert_player(&conn, &player).unwrap();
        set_setting(&conn, "ui.view", "stats").unwrap();
        set_setting(&conn, "onboarding.intro_seen", "true").unwrap();
        set_setting(&conn, RUNTIME_CLOCK_TICK_KEY, "480").unwrap();
        conn.execute(
            "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, acquired_at)
             VALUES ('test-loot', 'test-loot', 'material', 'Test Loot', 'Common', 3, ?1)",
            [now()],
        )
        .unwrap();

        let snapshot = reset_game_state(&mut conn).unwrap();

        assert_eq!(snapshot.player.level, 1);
        assert_eq!(snapshot.player.gold, 100);
        assert_eq!(snapshot.player.mode, "idle");
        assert_eq!(snapshot.clock_tick, 0);
        assert!(setting_value(&conn, "ui.view").unwrap().is_none());
        assert!(setting_value(&conn, "onboarding.intro_seen")
            .unwrap()
            .is_none());
        let inventory_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM inventory_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(inventory_count, 0);
        let equipment_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM equipment_items", [], |row| row.get(0))
            .unwrap();
        assert_eq!(equipment_count, 113);
        let event_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM event_log", [], |row| row.get(0))
            .unwrap();
        assert_eq!(event_count, 1);
        assert_eq!(session_status(&conn).unwrap(), "stopped");
    }

    #[test]
    fn read_only_commands_are_not_enqueueable() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        init_app().unwrap();
        let (_paths, conn) = open_app().unwrap();
        let err = enqueue_command(
            &conn,
            "test",
            CommandKind::Status,
            &CommandPayload::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("read-only"));
    }
}
