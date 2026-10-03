use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use std::collections::hash_map::DefaultHasher;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration as StdDuration;
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use vibemud_core::{
    apply_xp, area_by_id, crit_chance, damage, encounter_interval_ticks, hit_chance,
    luck_drop_multiplier, resistance_skill_reduction, should_trigger_encounter, CommandResult,
    EventKind,
};
use vibemud_db::{
    adjusted_enhancement_rule, append_event, clear_setting, effective_equipment_stats_for_slot,
    load_enhancement_rule, load_equipment_definition, load_player, mark_done, mark_failed,
    mark_processing, pending_commands, session_status, set_session_status, set_setting,
    setting_value, upsert_player, write_snapshot_and_hud, EquipmentDefinition,
};

const INVENTORY_CAPACITY: i64 = 20;
const RECOVERY_UNTIL_KEY: &str = "game.recovery_until";
const LAST_ADVENTURE_MODE_KEY: &str = "game.last_adventure_mode";
const LAST_AREA_KEY: &str = "game.last_area_id";
const LAST_DUNGEON_KEY: &str = "game.last_dungeon_id";
const AUTO_HUNT_POINT_KEY: &str = "game.auto_hunt_point";
const DUNGEON_SCOUT_STEP_KEY: &str = "game.dungeon_scout_step";
const DUNGEON_POINT_KEY: &str = "game.dungeon_point";
const DUNGEON_NORMAL_KILL_COUNT_KEY: &str = "game.dungeon_normal_kills";
const DUNGEON_ENCOUNTER_POINTS: u32 = 10;
const DUNGEON_BOSS_KILLS_REQUIRED: u32 = 5;
const RECOVERY_DURATION: Duration = Duration::minutes(1);
const TOWN_RECOVERY_HP_PER_TICK: i32 = 10;
const TOWN_RECOVERY_MP_PER_TICK: i32 = 4;
const PASSIVE_REGEN_INTERVAL_TICKS: u64 = 1;
const PASSIVE_REGEN_HP_BONUS: i32 = 2;
const PASSIVE_REGEN_MP_BONUS: i32 = 1;
#[cfg(test)]
const STAGED_COMBAT_ROUNDS: u32 = 5;
const RUNTIME_LOCK_FILE: &str = "runtime.lock";

pub fn start_runtime(ticks: Option<u32>) -> Result<()> {
    let (paths, conn) = vibemud_db::open_app()?;
    let _guard = RuntimeProcessGuard::acquire(&paths.root)?;
    // Serialize runtime election even if two callers race on a stale lock file.
    let election =
        rusqlite::Transaction::new_unchecked(&conn, rusqlite::TransactionBehavior::Immediate)?;
    if session_status(&election)? == "running" {
        let info = vibemud_db::session_info(&election)?;
        if info.runtime_pid.is_some_and(runtime_pid_is_vibemud) {
            anyhow::bail!(
                "VibeMUD runtime is already running{}",
                info.runtime_pid
                    .map(|pid| format!(" pid={pid}"))
                    .unwrap_or_default()
            );
        }
        if ticks.is_none() {
            set_session_status(&election, "stopped", None)?;
        }
    }
    set_session_status(&election, "running", Some(std::process::id()))?;
    let mut mod_owned = std::env::var("VIBEMUD_RUNTIME_OWNER").as_deref() == Ok("mod");
    set_setting(
        &election,
        "mod.runtime_owned",
        if mod_owned { "true" } else { "false" },
    )?;
    // Older runtimes could leave a command half applied. Never replay it blindly.
    election.execute("UPDATE command_queue SET status='failed', error_message='Runtime interrupted before completion; inspect state before retrying' WHERE status='processing'", [])?;
    election.commit()?;
    append_event(&conn, EventKind::SessionStarted, "Session started", None)?;
    write_snapshot_and_hud(&conn)?;
    let tick_count = ticks.unwrap_or(u32::MAX);
    let tick_interval_ms = vibemud_db::runtime_tick_interval_ms()?;
    for index in 0..tick_count {
        // Explicit legacy/CLI ownership can outlive mod connections.
        mod_owned =
            mod_owned && setting_value(&conn, "mod.runtime_owned")?.as_deref() == Some("true");
        if ticks.is_none() && session_status(&conn)? != "running" {
            break;
        }
        if ticks.is_none() && mod_owned && !vibemud_db::mod_bridge::has_clients(&conn)? {
            set_session_status(&conn, "stopped", None)?;
            vibemud_db::mod_bridge::cancel_pending(&conn)?;
            break;
        }
        run_one_tick_with_interval(&conn, index as u64, tick_interval_ms)?;
        if ticks.is_none() {
            thread::sleep(StdDuration::from_millis(tick_interval_ms));
        }
    }
    if ticks.is_some() {
        set_session_status(&conn, "stopped", None)?;
        append_event(
            &conn,
            EventKind::SessionStopped,
            "Bounded session stopped",
            None,
        )?;
        write_snapshot_and_hud(&conn)?;
    }
    Ok(())
}

struct RuntimeProcessGuard {
    path: PathBuf,
    pid: u32,
}

impl RuntimeProcessGuard {
    fn acquire(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let path = root.join(RUNTIME_LOCK_FILE);
        let pid = std::process::id();

        loop {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut file) => {
                    writeln!(file, "{pid}")?;
                    return Ok(Self { path, pid });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    let existing_pid = fs::read_to_string(&path)
                        .ok()
                        .and_then(|value| value.trim().parse::<i64>().ok());
                    if existing_pid.is_some_and(runtime_pid_is_vibemud) {
                        anyhow::bail!(
                            "VibeMUD runtime is already running{}",
                            existing_pid
                                .map(|pid| format!(" pid={pid}"))
                                .unwrap_or_default()
                        );
                    }
                    let _ = fs::remove_file(&path);
                }
                Err(err) => {
                    return Err(err)
                        .with_context(|| format!("failed to create {}", path.display()));
                }
            }
        }
    }
}

impl Drop for RuntimeProcessGuard {
    fn drop(&mut self) {
        let lock_pid = fs::read_to_string(&self.path)
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok());
        if lock_pid == Some(self.pid) {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn stop_runtime() -> Result<()> {
    let (paths, conn) = vibemud_db::open_app()?;
    let runtime_pid = vibemud_db::session_info(&conn)?.runtime_pid;
    let lock_pid = runtime_lock_pid(&paths.root);
    let mut pids = Vec::new();
    if let Some(pid) = runtime_pid {
        pids.push(pid);
    }
    if let Some(pid) = lock_pid {
        pids.push(pid);
    }
    pids.sort_unstable();
    pids.dedup();

    set_session_status(&conn, "stopped", None)?;
    vibemud_db::mod_bridge::cancel_pending(&conn)?;
    append_event(&conn, EventKind::SessionStopped, "Session stopped", None)?;
    write_snapshot_and_hud(&conn)?;
    for pid in pids {
        terminate_runtime_pid(pid);
        remove_runtime_lock_for_pid(&paths.root, pid);
    }
    Ok(())
}

pub fn status_runtime() -> Result<String> {
    let (paths, conn) = vibemud_db::open_app()?;
    let status = session_status(&conn)?;
    let info = vibemud_db::session_info(&conn)?;
    if status == "running" {
        if !info.runtime_pid.is_some_and(runtime_pid_is_vibemud) {
            if let Some(pid) =
                runtime_lock_pid(&paths.root).filter(|pid| runtime_pid_is_vibemud(*pid))
            {
                set_session_status(&conn, "running", Some(pid as u32))?;
                return Ok("running".to_string());
            }
            set_session_status(&conn, "stopped", None)?;
            if let Some(pid) = info.runtime_pid {
                remove_runtime_lock_for_pid(&paths.root, pid);
            }
            return Ok("stopped".to_string());
        }
    } else if let Some(pid) = runtime_lock_pid(&paths.root) {
        if runtime_pid_is_vibemud(pid) {
            set_session_status(&conn, "running", Some(pid as u32))?;
            return Ok("running".to_string());
        }
        remove_runtime_lock_for_pid(&paths.root, pid);
    }
    Ok(status)
}

fn runtime_pid_is_vibemud(pid: i64) -> bool {
    pid > 0 && pid_is_running(pid) && runtime_pid_matches_binary(pid)
}

#[cfg(unix)]
fn runtime_pid_matches_binary(pid: i64) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map(|output| {
            output.status.success()
                && runtime_command_is_vibemud(&String::from_utf8_lossy(&output.stdout))
        })
        .unwrap_or(false)
}

fn runtime_command_is_vibemud(command: &str) -> bool {
    // ps comm= gives only the executable, including spaces in its path.
    Path::new(command.trim().trim_matches('"'))
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "vibemud-runtime" || name == "vibemud-runtime.exe")
}

#[cfg(windows)]
fn runtime_pid_matches_binary(pid: i64) -> bool {
    hidden_windows_command("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    line.to_ascii_lowercase()
                        .starts_with("\"vibemud-runtime.exe\"")
                })
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn runtime_pid_matches_binary(_pid: i64) -> bool {
    true
}

fn runtime_lock_pid(root: &Path) -> Option<i64> {
    fs::read_to_string(root.join(RUNTIME_LOCK_FILE))
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
}

fn remove_runtime_lock_for_pid(root: &Path, pid: i64) {
    let path = root.join(RUNTIME_LOCK_FILE);
    let lock_pid = runtime_lock_pid(root);
    if lock_pid == Some(pid) && !runtime_pid_is_vibemud(pid) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(unix)]
fn pid_is_running(pid: i64) -> bool {
    pid > 0
        && std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        && !pid_is_zombie(pid)
}

#[cfg(unix)]
fn pid_is_zombie(pid: i64) -> bool {
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .trim_start()
                    .starts_with('Z')
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn pid_is_running(pid: i64) -> bool {
    process_is_alive(pid)
}

#[cfg(windows)]
pub fn process_is_alive(pid: i64) -> bool {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    if pid <= 0 || pid > u32::MAX as i64 {
        return false;
    }
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 0x0000_0102;
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid as u32) };
    if handle.is_null() {
        return false;
    }
    let alive = unsafe { WaitForSingleObject(handle, 0) == WAIT_TIMEOUT };
    unsafe { CloseHandle(handle) };
    alive
}

#[cfg(not(any(unix, windows)))]
fn pid_is_running(pid: i64) -> bool {
    pid > 0
}

#[cfg(unix)]
fn terminate_runtime_pid(pid: i64) {
    if pid <= 0 || pid as u32 == std::process::id() || !runtime_pid_is_vibemud(pid) {
        return;
    }

    let _ = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();

    for _ in 0..20 {
        if !pid_is_running(pid) {
            return;
        }
        thread::sleep(StdDuration::from_millis(100));
    }

    let _ = std::process::Command::new("kill")
        .arg("-KILL")
        .arg(pid.to_string())
        .status();
    for _ in 0..20 {
        if !pid_is_running(pid) {
            return;
        }
        thread::sleep(StdDuration::from_millis(50));
    }
}

#[cfg(windows)]
fn hidden_windows_command(name: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(name);
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

#[cfg(windows)]
fn terminate_runtime_pid(pid: i64) {
    if pid <= 0 || pid as u32 == std::process::id() || !runtime_pid_is_vibemud(pid) {
        return;
    }

    let _ = hidden_windows_command("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .output();

    for _ in 0..20 {
        if !pid_is_running(pid) {
            return;
        }
        thread::sleep(StdDuration::from_millis(100));
    }

    let _ = hidden_windows_command("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
    for _ in 0..20 {
        if !pid_is_running(pid) {
            return;
        }
        thread::sleep(StdDuration::from_millis(50));
    }
}

#[cfg(not(any(unix, windows)))]
fn terminate_runtime_pid(_pid: i64) {}

pub fn run_one_tick(conn: &rusqlite::Connection, tick_index: u64) -> Result<()> {
    run_one_tick_with_interval(conn, tick_index, vibemud_db::runtime_tick_interval_ms()?)
}

fn run_one_tick_with_interval(
    conn: &rusqlite::Connection,
    tick_index: u64,
    tick_interval_ms: u64,
) -> Result<()> {
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    run_tick_transaction(&tx, tick_index, tick_interval_ms)?;
    tx.commit()?;
    Ok(())
}

fn run_tick_transaction(
    conn: &rusqlite::Connection,
    tick_index: u64,
    tick_interval_ms: u64,
) -> Result<()> {
    vibemud_db::advance_runtime_clock(conn)?;
    vibemud_db::load_daily_quests(conn)?;
    let mut player = load_player(conn)?;
    if player.mode == "recovering" {
        match recovery_remaining_seconds(conn)? {
            Some(remaining) if remaining > 0 => {
                apply_town_recovery_rest(&mut player);
                upsert_player(conn, &player)?;
                append_event(
                    conn,
                    EventKind::TickAdvanced,
                    format!("Recovery in progress. Actions unlock in {remaining}s."),
                    None,
                )?;
                write_snapshot_and_hud(conn)?;
                return Ok(());
            }
            _ => {
                player.mode = "idle".to_string();
                player.current_area_id = Some("town".to_string());
                clear_setting(conn, RECOVERY_UNTIL_KEY)?;
                clear_dungeon_progress(conn)?;
                upsert_player(conn, &player)?;
                append_event(
                    conn,
                    EventKind::TickAdvanced,
                    "Recovery complete. Actions unlocked.",
                    None,
                )?;
            }
        }
    }

    for command in pending_commands(conn, 25)? {
        conn.execute_batch("SAVEPOINT game_command")?;
        mark_processing(conn, &command.id)?;
        match apply_command(conn, &command.kind, &command.payload) {
            Ok(message) => {
                let version =
                    append_event(conn, EventKind::CommandProcessed, message.clone(), None)?;
                let result = serde_json::to_string(&CommandResult {
                    message,
                    state_version: version,
                })?;
                mark_done(conn, &command.id, &result)?;
            }
            Err(error) => {
                conn.execute_batch("ROLLBACK TO game_command")?;
                mark_failed(conn, &command.id, &error.to_string())?;
            }
        }
        conn.execute_batch("RELEASE game_command")?;
    }

    let mut player = load_player(conn)?;
    if process_active_combat(conn, &mut player)? {
        write_snapshot_and_hud(conn)?;
        return Ok(());
    }

    match player.mode.as_str() {
        "auto_hunt" => {
            let seed = latest_seed(conn)? ^ tick_index;
            let step = advance_auto_hunt_step(conn)?;
            let interval = encounter_interval_ticks(player.speed, tick_interval_ms);
            let logs = if step % interval == 0 {
                let encounter_point = advance_auto_hunt_point(conn)?;
                let area = player
                    .current_area_id
                    .as_deref()
                    .map(area_by_id)
                    .unwrap_or_else(|| area_by_id("training-field"));
                if should_trigger_encounter(&area, false, seed) {
                    let monster = vibemud_db::pick_area_monster(conn, &area.id, seed)?;
                    let encounter_id = format!("{}-wild-{encounter_point}", area.id);
                    start_staged_combat(
                        conn,
                        StagedCombatStart {
                            encounter_id: &encounter_id,
                            seed,
                            monster_name: &monster.name,
                            difficulty_bonus: monster.difficulty_bonus,
                            combat_kind: "field",
                            metadata: Some(serde_json::json!({
                                "area_id": area.id,
                                "monster_id": monster.id,
                            })),
                            player_level: player.level,
                        },
                    )?
                } else {
                    if encounter_point >= DUNGEON_ENCOUNTER_POINTS {
                        reset_auto_hunt_point(conn)?;
                    }
                    conn.execute(
                        "UPDATE combat_state SET in_combat = 0, encounter_id = NULL, encounter_seed = ?1, updated_at = ?2 WHERE id = 'main'",
                        params![seed as i64, vibemud_db::now()],
                    )?;
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            upsert_player(conn, &player)?;
            append_tick_logs(conn, logs, Some(seed))?;
        }
        "dungeon" => run_dungeon_tick(conn, &mut player, tick_index, tick_interval_ms)?,
        "rest" => {
            player.hp =
                (player.hp + (player.regen * 4).max(TOWN_RECOVERY_HP_PER_TICK)).min(player.max_hp);
            player.mp =
                (player.mp + (player.regen * 2).max(TOWN_RECOVERY_MP_PER_TICK)).min(player.max_mp);
            upsert_player(conn, &player)?;
            append_event(
                conn,
                EventKind::TickAdvanced,
                "Resting recovered HP/MP",
                None,
            )?;
        }
        _ => {}
    }
    if apply_passive_regen(&mut player, tick_index) {
        upsert_player(conn, &player)?;
    }
    write_snapshot_and_hud(conn)?;
    Ok(())
}

fn append_tick_logs(
    conn: &rusqlite::Connection,
    logs: impl IntoIterator<Item = String>,
    seed: Option<u64>,
) -> Result<()> {
    for log in logs {
        append_event(conn, EventKind::TickAdvanced, log, seed)?;
    }
    Ok(())
}

fn apply_town_recovery_rest(player: &mut vibemud_core::PlayerState) {
    player.current_area_id = Some("town".to_string());
    player.current_dungeon_id = None;
    player.hp = (player.hp + TOWN_RECOVERY_HP_PER_TICK).min(player.max_hp);
    player.mp = (player.mp + TOWN_RECOVERY_MP_PER_TICK).min(player.max_mp);
}

fn apply_passive_regen(player: &mut vibemud_core::PlayerState, tick_index: u64) -> bool {
    if player.mode == "recovering"
        || PASSIVE_REGEN_INTERVAL_TICKS == 0
        || !(tick_index + 1).is_multiple_of(PASSIVE_REGEN_INTERVAL_TICKS)
    {
        return false;
    }

    let before_hp = player.hp;
    let before_mp = player.mp;
    player.hp = (player.hp + player.regen.max(0) + PASSIVE_REGEN_HP_BONUS).min(player.max_hp);
    player.mp = (player.mp + (player.regen.max(0) / 2) + PASSIVE_REGEN_MP_BONUS).min(player.max_mp);

    player.hp != before_hp || player.mp != before_mp
}

struct StagedCombatStart<'a> {
    encounter_id: &'a str,
    seed: u64,
    monster_name: &'a str,
    difficulty_bonus: i32,
    combat_kind: &'a str,
    metadata: Option<serde_json::Value>,
    player_level: u32,
}

fn start_staged_combat(
    conn: &rusqlite::Connection,
    start: StagedCombatStart<'_>,
) -> Result<Vec<String>> {
    let StagedCombatStart {
        encounter_id,
        seed,
        monster_name,
        difficulty_bonus,
        combat_kind,
        metadata,
        player_level,
    } = start;
    let boss = combat_kind == "dungeon_boss" || monster_name.starts_with("Boss ");
    let monster_max_hp = if boss {
        80 + player_level as i32 * 12 + difficulty_bonus.max(0) * 4
    } else {
        36 + player_level as i32 * 8 + difficulty_bonus.max(0) * 3
    }
    .max(20);
    let payload = serde_json::json!({
        "kind": combat_kind,
        "monster_name": monster_name,
        "difficulty_bonus": difficulty_bonus,
        "seed": seed,
        "round": 0,
        "turn": 0,
        "monster_hp": monster_max_hp,
        "monster_max_hp": monster_max_hp,
        "metadata": metadata.unwrap_or_else(|| serde_json::json!({})),
    });
    conn.execute(
        "UPDATE combat_state SET in_combat = 1, encounter_id = ?1, monster_group_json = ?2, encounter_seed = ?3, updated_at = ?4 WHERE id = 'main'",
        params![encounter_id, payload.to_string(), seed as i64, vibemud_db::now()],
    )?;
    Ok(vec![format!("{monster_name} appeared.")])
}

fn active_combat(conn: &rusqlite::Connection) -> Result<Option<serde_json::Value>> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT monster_group_json FROM combat_state WHERE id = 'main' AND in_combat = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    payload
        .filter(|value| !value.trim().is_empty())
        .map(|value| serde_json::from_str(&value).map_err(Into::into))
        .transpose()
}

fn clear_active_combat(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute(
        "UPDATE combat_state SET in_combat = 0, encounter_id = NULL, monster_group_json = NULL, encounter_seed = NULL, updated_at = ?1 WHERE id = 'main'",
        [vibemud_db::now()],
    )?;
    Ok(())
}

fn process_active_combat(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
) -> Result<bool> {
    let Some(mut combat) = active_combat(conn)? else {
        return Ok(false);
    };
    let monster_name = combat_string(&combat, "monster_name", "Unknown Monster");
    let combat_kind = combat_string(&combat, "kind", "normal");
    let boss = combat_kind == "dungeon_boss" || monster_name.starts_with("Boss ");
    let difficulty_bonus = combat_i32(&combat, "difficulty_bonus", 0);
    let seed = combat_u64(&combat, "seed", latest_seed(conn)?);
    let turn = combat_u32(&combat, "turn", 0).saturating_add(1);
    let monster_max_hp = combat_i32(&combat, "monster_max_hp", 40).max(1);
    let mut monster_hp = combat_i32(&combat, "monster_hp", monster_max_hp).clamp(1, monster_max_hp);
    let mut logs = Vec::new();
    let hero_turn = turn % 2 == 1;

    if hero_turn {
        let hit_roll = deterministic_roll01(&format!("combat:hit:{seed}:{turn}"));
        if hit_roll <= hit_chance(player.accuracy, 8 + difficulty_bonus) {
            let crit = deterministic_roll01(&format!("combat:crit:{seed}:{turn}"))
                <= crit_chance(player.accuracy);
            let random_factor =
                0.85 + deterministic_roll01(&format!("combat:damage:{seed}:{turn}")) * 0.30;
            let monster_def = 5 + player.level as i32 + difficulty_bonus.max(0) / 3;
            let mut dealt = damage(player.attack, 1.0, monster_def, random_factor);
            if crit {
                dealt = ((dealt as f64) * 1.5).floor() as i32;
            }
            dealt = dealt.max(1);
            monster_hp = (monster_hp - dealt).max(0);
            logs.push(format!(
                "Warrior hit {monster_name} for {dealt}. {monster_name} HP {monster_hp}/{monster_max_hp}."
            ));
        } else {
            logs.push(format!("Warrior missed {monster_name}."));
        }

        if monster_hp <= 0 {
            finish_staged_combat(conn, player, &combat, &mut logs, seed)?;
        } else {
            combat["turn"] = serde_json::json!(turn);
            combat["round"] = serde_json::json!(turn.div_ceil(2));
            combat["monster_hp"] = serde_json::json!(monster_hp);
            persist_active_combat_turn(conn, &combat, turn)?;
        }
    } else {
        let incoming = staged_monster_damage(player, difficulty_bonus, seed, turn, boss);
        player.hp -= incoming;
        if boss {
            logs.push(format!(
                "{monster_name} used a skill for {incoming}. Warrior HP {}/{}.",
                player.hp.max(0),
                player.max_hp
            ));
        } else {
            logs.push(format!(
                "{monster_name} hit Warrior for {incoming}. Warrior HP {}/{}.",
                player.hp.max(0),
                player.max_hp
            ));
        }
        if player.hp <= 0 {
            logs.extend(vibemud_core::death_penalty(player));
            start_recovery_if_needed(conn, player)?;
            clear_active_combat(conn)?;
        } else {
            combat["turn"] = serde_json::json!(turn);
            combat["round"] = serde_json::json!(turn / 2);
            combat["monster_hp"] = serde_json::json!(monster_hp);
            persist_active_combat_turn(conn, &combat, turn)?;
        }
    }

    upsert_player(conn, player)?;
    append_tick_logs(conn, logs, Some(seed))?;
    Ok(true)
}

fn persist_active_combat_turn(
    conn: &rusqlite::Connection,
    combat: &serde_json::Value,
    turn: u32,
) -> Result<()> {
    conn.execute(
        "UPDATE combat_state SET monster_group_json = ?1, turn_index = ?2, updated_at = ?3 WHERE id = 'main'",
        params![combat.to_string(), turn, vibemud_db::now()],
    )?;
    Ok(())
}

fn finish_staged_combat(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    combat: &serde_json::Value,
    logs: &mut Vec<String>,
    seed: u64,
) -> Result<()> {
    let monster_name = combat_string(combat, "monster_name", "Unknown Monster");
    let kind = combat_string(combat, "kind", "field");
    let difficulty_bonus = combat_i32(combat, "difficulty_bonus", 0);
    let xp_multiplier =
        vibe_fever_multiplier() * equipped_reward_bonus_multiplier(conn, "xp_bonus")?;
    let gold_multiplier =
        vibe_fever_multiplier() * equipped_reward_bonus_multiplier(conn, "gold_bonus")?;
    let base_xp = 24 + player.level as u64 * 4 + difficulty_bonus.max(0) as u64;
    let base_gold = 8 + player.level as i64 * 2 + difficulty_bonus.max(0) as i64;
    let xp = boosted_u64(base_xp, xp_multiplier);
    let gold = boosted_i64(base_gold, gold_multiplier);
    player.gold += gold;
    vibemud_db::mod_bridge::record_adventure_rewards(conn, 1, xp, gold.max(0) as u64, None)?;
    vibemud_db::record_quest_progress(conn, "monster_kill", None, 1)?;
    if kind == "dungeon_boss" {
        vibemud_db::record_quest_progress(conn, "boss_kill", None, 1)?;
    }
    logs.push(format!("{monster_name} defeated."));
    logs.push(format!("Reward: +{xp} XP."));
    logs.push(format!("Reward: +{gold} gold."));
    logs.extend(apply_xp(player, xp));
    clear_active_combat(conn)?;

    match kind.as_str() {
        "dungeon_normal" => finish_dungeon_normal_combat(conn, player, combat, logs, seed)?,
        "dungeon_boss" => finish_dungeon_boss_combat(conn, player, combat, logs)?,
        _ => {
            let (monster_id, monster_grade) = combat_drop_source(combat);
            if let Some(drop_log) =
                maybe_drop_equipment(conn, player, &monster_id, &monster_grade, seed)?
            {
                logs.push(drop_log);
            }
        }
    }
    Ok(())
}

fn finish_dungeon_normal_combat(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    combat: &serde_json::Value,
    logs: &mut Vec<String>,
    seed: u64,
) -> Result<()> {
    let encounter_point = combat
        .get("metadata")
        .and_then(|metadata| metadata.get("encounter_point"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as u32;
    let kills = increment_dungeon_normal_kills(conn)?;
    logs.push(format!(
        "Dungeon normal monster defeated {kills}/{DUNGEON_BOSS_KILLS_REQUIRED}."
    ));
    if kills >= DUNGEON_BOSS_KILLS_REQUIRED {
        logs.push("The dungeon boss is approaching.".to_string());
    } else if encounter_point >= DUNGEON_ENCOUNTER_POINTS {
        logs.push(format!(
            "Dungeon entry reset: only {kills}/{DUNGEON_BOSS_KILLS_REQUIRED} normal monsters defeated."
        ));
        clear_dungeon_progress(conn)?;
    }
    let (monster_id, monster_grade) = combat_drop_source(combat);
    if let Some(drop_log) = maybe_drop_equipment(
        conn,
        player,
        &monster_id,
        &monster_grade,
        seed ^ encounter_point as u64,
    )? {
        logs.push(drop_log);
    }
    Ok(())
}

fn combat_drop_source(combat: &serde_json::Value) -> (String, String) {
    let metadata = combat.get("metadata").unwrap_or(&serde_json::Value::Null);
    let monster_id = metadata
        .get("monster_id")
        .and_then(|value| value.as_str())
        .unwrap_or("training-scarab")
        .to_string();
    let monster_grade = metadata
        .get("monster_grade")
        .and_then(|value| value.as_str())
        .unwrap_or("normal")
        .to_string();
    (monster_id, monster_grade)
}

fn finish_dungeon_boss_combat(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    combat: &serde_json::Value,
    logs: &mut Vec<String>,
) -> Result<()> {
    let metadata = combat
        .get("metadata")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let dungeon_id = metadata
        .get("dungeon_id")
        .and_then(|value| value.as_str())
        .unwrap_or("goblin-den")
        .to_string();
    let reward = dungeon_reward(&dungeon_id);
    let mut eviction_note = String::new();
    let tx = if conn.is_autocommit() {
        Some(conn.unchecked_transaction()?)
    } else {
        None
    };
    let write_conn = tx.as_deref().unwrap_or(conn);
    if !inventory_has_room_for_one(write_conn)? {
        if let Some((evicted_name, evicted_rarity)) =
            evict_lower_rarity_equipment(write_conn, reward.3)?
        {
            eviction_note = format!(" ({evicted_name} [{evicted_rarity}] 아이템을 정리.)");
        }
    }
    let stored = add_item(
        write_conn,
        reward.0,
        reward.1,
        reward.2,
        reward.3,
        Some(100),
        Some(100),
    )?;
    if stored {
        vibemud_db::mod_bridge::record_adventure_rewards(write_conn, 0, 0, 0, Some(reward.1))?;
    }
    if let Some(tx) = tx {
        tx.commit()?;
    }
    let reward_gold = boosted_i64(
        reward.4,
        vibe_fever_multiplier() * equipped_reward_bonus_multiplier(conn, "gold_bonus")?,
    );
    player.gold += reward_gold;
    vibemud_db::mod_bridge::record_adventure_rewards(conn, 0, 0, reward_gold.max(0) as u64, None)?;
    player.current_dungeon_id = Some(dungeon_id.clone());
    player.mode = "dungeon".to_string();
    clear_dungeon_progress(conn)?;
    if stored {
        logs.push(format!("Boss reward: {}.{eviction_note}", reward.1));
    } else {
        logs.push(format!(
            "Boss reward discarded: 소지품창이 가득 차 {} 아이템이 삭제되었습니다.",
            reward.1
        ));
    }
    logs.push(format!("Boss reward: +{reward_gold} gold."));
    logs.push(format!(
        "Dungeon {dungeon_id} cleared. Restarting dungeon run."
    ));
    let restart_seed = latest_seed(conn)? ^ 0xD06E_B055 ^ reward_gold as u64;
    let encounter_point = advance_dungeon_point(conn)?;
    logs.extend(start_dungeon_normal_encounter(
        conn,
        player,
        &dungeon_id,
        encounter_point,
        restart_seed,
    )?);
    Ok(())
}

fn staged_monster_damage(
    player: &vibemud_core::PlayerState,
    difficulty_bonus: i32,
    seed: u64,
    round: u32,
    boss_skill: bool,
) -> i32 {
    let monster_attack = (8 + player.level as i32 * 2 + difficulty_bonus).max(1);
    let random_factor =
        0.85 + deterministic_roll01(&format!("combat:incoming:{seed}:{round}")) * 0.30;
    let skill_multiplier = if boss_skill { 1.05 } else { 0.75 };
    let incoming = damage(
        monster_attack,
        skill_multiplier,
        player.defense,
        random_factor,
    );
    if boss_skill {
        ((incoming as f64) * (1.0 - resistance_skill_reduction(player.evasion))).max(1.0) as i32
    } else {
        incoming
    }
}

fn combat_string(value: &serde_json::Value, key: &str, fallback: &str) -> String {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .unwrap_or(fallback)
        .to_string()
}

fn combat_i32(value: &serde_json::Value, key: &str, fallback: i32) -> i32 {
    value
        .get(key)
        .and_then(|value| value.as_i64())
        .map(|value| value as i32)
        .unwrap_or(fallback)
}

fn combat_u32(value: &serde_json::Value, key: &str, fallback: u32) -> u32 {
    value
        .get(key)
        .and_then(|value| value.as_u64())
        .map(|value| value as u32)
        .unwrap_or(fallback)
}

fn combat_u64(value: &serde_json::Value, key: &str, fallback: u64) -> u64 {
    value
        .get(key)
        .and_then(|value| value.as_u64())
        .unwrap_or(fallback)
}

fn advance_auto_hunt_step(conn: &rusqlite::Connection) -> Result<u32> {
    let next = dungeon_floor(conn)?.max(0) as u32 + 1;
    conn.execute(
        "UPDATE combat_state SET in_combat = 0, encounter_id = NULL, turn_index = ?1, updated_at = ?2 WHERE id = 'main'",
        params![next, vibemud_db::now()],
    )?;
    Ok(next)
}

fn auto_hunt_point(conn: &rusqlite::Connection) -> Result<u32> {
    setting_u32(conn, AUTO_HUNT_POINT_KEY)
}

fn advance_auto_hunt_point(conn: &rusqlite::Connection) -> Result<u32> {
    let next = auto_hunt_point(conn)?
        .saturating_add(1)
        .min(DUNGEON_ENCOUNTER_POINTS);
    set_setting(conn, AUTO_HUNT_POINT_KEY, &next.to_string())?;
    Ok(next)
}

fn reset_auto_hunt_point(conn: &rusqlite::Connection) -> Result<()> {
    clear_setting(conn, AUTO_HUNT_POINT_KEY)
}

fn run_dungeon_tick(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    tick_index: u64,
    tick_interval_ms: u64,
) -> Result<()> {
    let dungeon_id = player
        .current_dungeon_id
        .clone()
        .unwrap_or_else(|| "goblin-den".to_string());
    let boss_id: String = conn
        .query_row(
            "SELECT boss_id FROM dungeons WHERE id = ?1",
            [&dungeon_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| "goblin-chief".to_string());
    let current_point = dungeon_progress_point(conn)?.saturating_add(1);
    player.current_area_id = Some(dungeon_area_id(&dungeon_id).to_string());
    let area = player
        .current_area_id
        .as_deref()
        .map(area_by_id)
        .unwrap_or_else(|| area_by_id("forest-edge"));
    let scout_step = advance_setting_counter(conn, DUNGEON_SCOUT_STEP_KEY)?;
    let interval = encounter_interval_ticks(player.speed, tick_interval_ms);
    let normal_kills = dungeon_normal_kills(conn)?;
    let boss_ready = normal_kills >= DUNGEON_BOSS_KILLS_REQUIRED;
    let seed =
        latest_seed(conn)? ^ tick_index ^ current_point as u64 ^ ((normal_kills as u64) << 8);
    if scout_step % interval != 0 {
        conn.execute(
            "UPDATE combat_state SET in_combat = 0, encounter_id = NULL, encounter_seed = ?1, updated_at = ?2 WHERE id = 'main'",
            params![seed as i64, vibemud_db::now()],
        )?;
        upsert_player(conn, player)?;
        return Ok(());
    }

    if boss_ready {
        let boss_name = dungeon_boss_display_name(&boss_id);
        let encounter_id = format!("{dungeon_id}-boss");
        let monster_name = format!("Boss {boss_name}");
        let logs = start_staged_combat(
            conn,
            StagedCombatStart {
                encounter_id: &encounter_id,
                seed,
                monster_name: &monster_name,
                difficulty_bonus: dungeon_boss_difficulty(&dungeon_id),
                combat_kind: "dungeon_boss",
                metadata: Some(serde_json::json!({
                    "dungeon_id": dungeon_id,
                    "boss_id": boss_id,
                    "boss_name": boss_name,
                })),
                player_level: player.level,
            },
        )?;
        upsert_player(conn, player)?;
        append_tick_logs(conn, logs, Some(seed))?;
        return Ok(());
    }

    let encounter_point = advance_dungeon_point(conn)?;
    if !should_trigger_encounter(&area, true, seed) {
        let exhausted = encounter_point >= DUNGEON_ENCOUNTER_POINTS;
        let normal_kills = dungeon_normal_kills(conn)?;
        conn.execute(
            "UPDATE combat_state SET in_combat = 0, encounter_id = NULL, encounter_seed = ?1, updated_at = ?2 WHERE id = 'main'",
            params![seed as i64, vibemud_db::now()],
        )?;
        if exhausted && normal_kills < DUNGEON_BOSS_KILLS_REQUIRED {
            clear_dungeon_progress(conn)?;
        }
        upsert_player(conn, player)?;
        if exhausted && normal_kills < DUNGEON_BOSS_KILLS_REQUIRED {
            append_event(
                conn,
                EventKind::TickAdvanced,
                format!(
                    "Dungeon entry reset: only {normal_kills}/{DUNGEON_BOSS_KILLS_REQUIRED} normal monsters defeated."
                ),
                Some(seed),
            )?;
        }
        return Ok(());
    }
    let logs = start_dungeon_normal_encounter(conn, player, &dungeon_id, encounter_point, seed)?;
    upsert_player(conn, player)?;
    append_tick_logs(conn, logs, Some(seed))?;
    Ok(())
}

fn dungeon_area_id(dungeon_id: &str) -> &'static str {
    match dungeon_id {
        "crystal-cave" => "old-mine",
        "lich-tomb" => "fallen-fortress",
        "cyclops-forge" => "obsidian-coast",
        "medusa-temple" => "oracle-ruins",
        "titan-vault" => "olympus-gate",
        _ => "forest-edge",
    }
}

fn start_dungeon_normal_encounter(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    dungeon_id: &str,
    encounter_point: u32,
    seed: u64,
) -> Result<Vec<String>> {
    player.current_area_id = Some(dungeon_area_id(dungeon_id).to_string());
    let area = player
        .current_area_id
        .as_deref()
        .map(area_by_id)
        .unwrap_or_else(|| area_by_id("forest-edge"));
    conn.execute("UPDATE combat_state SET in_combat = 1, encounter_id = ?1, encounter_seed = ?2, turn_index = ?3, updated_at = ?4 WHERE id = 'main'", params![format!("{dungeon_id}-point-{encounter_point}"), seed as i64, encounter_point, vibemud_db::now()])?;
    let monster = vibemud_db::pick_dungeon_monster(conn, dungeon_id, seed)
        .or_else(|_| vibemud_db::pick_area_monster(conn, &area.id, seed))?;
    let mut logs = vec![format!(
        "Dungeon encounter point {encounter_point}/{DUNGEON_ENCOUNTER_POINTS}."
    )];
    let encounter_id = format!("{dungeon_id}-point-{encounter_point}");
    logs.extend(start_staged_combat(
        conn,
        StagedCombatStart {
            encounter_id: &encounter_id,
            seed,
            monster_name: &monster.name,
            difficulty_bonus: monster.difficulty_bonus,
            combat_kind: "dungeon_normal",
            metadata: Some(serde_json::json!({
                "dungeon_id": dungeon_id,
                "encounter_point": encounter_point,
                "monster_id": monster.id,
            })),
            player_level: player.level,
        },
    )?);
    Ok(logs)
}

fn advance_setting_counter(conn: &rusqlite::Connection, key: &str) -> Result<u32> {
    let current = setting_value(conn, key)?
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0);
    let next = current.saturating_add(1);
    set_setting(conn, key, &next.to_string())?;
    Ok(next)
}

fn setting_u32(conn: &rusqlite::Connection, key: &str) -> Result<u32> {
    Ok(setting_value(conn, key)?
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0))
}

fn dungeon_progress_point(conn: &rusqlite::Connection) -> Result<u32> {
    setting_u32(conn, DUNGEON_POINT_KEY)
}

fn dungeon_normal_kills(conn: &rusqlite::Connection) -> Result<u32> {
    setting_u32(conn, DUNGEON_NORMAL_KILL_COUNT_KEY)
}

fn advance_dungeon_point(conn: &rusqlite::Connection) -> Result<u32> {
    let next = dungeon_progress_point(conn)?
        .saturating_add(1)
        .min(DUNGEON_ENCOUNTER_POINTS);
    set_setting(conn, DUNGEON_POINT_KEY, &next.to_string())?;
    Ok(next)
}

fn increment_dungeon_normal_kills(conn: &rusqlite::Connection) -> Result<u32> {
    let next = dungeon_normal_kills(conn)?
        .saturating_add(1)
        .min(DUNGEON_BOSS_KILLS_REQUIRED);
    set_setting(conn, DUNGEON_NORMAL_KILL_COUNT_KEY, &next.to_string())?;
    Ok(next)
}

fn clear_dungeon_progress(conn: &rusqlite::Connection) -> Result<()> {
    clear_setting(conn, DUNGEON_SCOUT_STEP_KEY)?;
    clear_setting(conn, DUNGEON_POINT_KEY)?;
    clear_setting(conn, DUNGEON_NORMAL_KILL_COUNT_KEY)?;
    Ok(())
}

fn dungeon_boss_display_name(boss_id: &str) -> String {
    match boss_id {
        "goblin-chief" => "Goblin".to_string(),
        "crystal-golem" => "Golem".to_string(),
        "ancient-lich" => "Lich".to_string(),
        "cyclops-smith" => "Cyclops".to_string(),
        "medusa" => "Medusa".to_string(),
        "titan-warden" => "Warden".to_string(),
        _ => boss_id
            .split('-')
            .rev()
            .find(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .unwrap_or_else(|| "Boss".to_string()),
    }
}

fn dungeon_boss_difficulty(dungeon_id: &str) -> i32 {
    match dungeon_id {
        "titan-vault" => 52,
        "medusa-temple" => 40,
        "cyclops-forge" => 30,
        "lich-tomb" => 24,
        "crystal-cave" => 16,
        _ => 10,
    }
}

fn start_recovery_if_needed(
    conn: &rusqlite::Connection,
    player: &vibemud_core::PlayerState,
) -> Result<()> {
    if player.mode == "recovering" && setting_value(conn, RECOVERY_UNTIL_KEY)?.is_none() {
        clear_dungeon_progress(conn)?;
        let until = OffsetDateTime::now_utc() + RECOVERY_DURATION;
        let value = until.format(&Rfc3339)?;
        set_setting(conn, RECOVERY_UNTIL_KEY, &value)?;
    }
    Ok(())
}

fn recovery_remaining_seconds(conn: &rusqlite::Connection) -> Result<Option<i64>> {
    let Some(until) = setting_value(conn, RECOVERY_UNTIL_KEY)? else {
        let until = OffsetDateTime::now_utc() + RECOVERY_DURATION;
        set_setting(conn, RECOVERY_UNTIL_KEY, &until.format(&Rfc3339)?)?;
        return Ok(Some(RECOVERY_DURATION.whole_seconds()));
    };
    let parsed =
        OffsetDateTime::parse(&until, &Rfc3339).unwrap_or_else(|_| OffsetDateTime::now_utc());
    Ok(Some(
        (parsed - OffsetDateTime::now_utc()).whole_seconds().max(0),
    ))
}

fn apply_command(
    conn: &rusqlite::Connection,
    kind: &str,
    payload: &vibemud_core::CommandPayload,
) -> Result<String> {
    let mut player = load_player(conn)?;
    let message = match kind {
        "hunt_start" => {
            match resolve_hunt_start_target(conn, &player, payload.area_id.as_deref())? {
                AdventureTarget::Area(area) => {
                    player.current_area_id = Some(area.clone());
                    player.current_dungeon_id = None;
                    player.mode = "auto_hunt".to_string();
                    remember_area_target(conn, &area)?;
                    clear_dungeon_progress(conn)?;
                    conn.execute("UPDATE combat_state SET in_combat = 0, encounter_id = NULL, encounter_seed = NULL, turn_index = 0, updated_at = ?1 WHERE id = 'main'", [vibemud_db::now()])?;
                    upsert_player(conn, &player)?;
                    format!("Auto hunt started in {area}")
                }
                AdventureTarget::Dungeon(dungeon) => {
                    player.current_dungeon_id = Some(dungeon.clone());
                    player.mode = "dungeon".to_string();
                    remember_dungeon_target(conn, &dungeon)?;
                    conn.execute("UPDATE combat_state SET in_combat = 0, encounter_id = NULL, encounter_seed = NULL, turn_index = 0, updated_at = ?1 WHERE id = 'main'", [vibemud_db::now()])?;
                    upsert_player(conn, &player)?;
                    format!("Resumed dungeon {dungeon}")
                }
            }
        }
        "hunt_stop" => {
            player.mode = "idle".to_string();
            conn.execute("UPDATE combat_state SET in_combat = 0, encounter_id = NULL, turn_index = 0, updated_at = ?1 WHERE id = 'main'", [vibemud_db::now()])?;
            upsert_player(conn, &player)?;
            "Auto hunt stopped".to_string()
        }
        "area_enter" => {
            let area = payload
                .area_id
                .clone()
                .unwrap_or_else(|| "training-field".to_string());
            player.current_area_id = Some(area.clone());
            player.current_dungeon_id = None;
            player.mode = "idle".to_string();
            remember_area_target(conn, &area)?;
            conn.execute("UPDATE combat_state SET in_combat = 0, encounter_id = NULL, turn_index = 0, updated_at = ?1 WHERE id = 'main'", [vibemud_db::now()])?;
            upsert_player(conn, &player)?;
            format!("Entered area {area}")
        }
        "dungeon_enter" => {
            let dungeon = payload
                .dungeon_id
                .clone()
                .unwrap_or_else(|| "goblin-den".to_string());
            player.current_dungeon_id = Some(dungeon.clone());
            player.mode = "dungeon".to_string();
            remember_dungeon_target(conn, &dungeon)?;
            clear_dungeon_progress(conn)?;
            conn.execute("UPDATE combat_state SET in_combat = 0, encounter_id = NULL, turn_index = 0, updated_at = ?1 WHERE id = 'main'", [vibemud_db::now()])?;
            upsert_player(conn, &player)?;
            format!(
                "Entered dungeon {}",
                player.current_dungeon_id.as_deref().unwrap_or("unknown")
            )
        }
        "dungeon_retreat" => {
            player.current_dungeon_id = None;
            player.mode = "idle".to_string();
            clear_dungeon_progress(conn)?;
            conn.execute("UPDATE combat_state SET in_combat = 0, turn_index = 0, updated_at = ?1 WHERE id = 'main'", [vibemud_db::now()])?;
            upsert_player(conn, &player)?;
            "Retreated from dungeon; current run progress reset".to_string()
        }
        "rest" => {
            player.hp = player.max_hp;
            player.mp = player.max_mp;
            player.mode = "rest".to_string();
            upsert_player(conn, &player)?;
            "Rested to full HP/MP".to_string()
        }
        "town" => {
            player.current_area_id = Some("town".to_string());
            player.current_dungeon_id = None;
            player.mode = "rest".to_string();
            clear_dungeon_progress(conn)?;
            conn.execute("UPDATE combat_state SET in_combat = 0, encounter_id = NULL, turn_index = 0, updated_at = ?1 WHERE id = 'main'", [vibemud_db::now()])?;
            upsert_player(conn, &player)?;
            "Returned to town and started resting".to_string()
        }
        "party_recruit" => recruit_companion(conn, &mut player)?,
        "party_swap" => swap_party(
            conn,
            payload.slot.unwrap_or(1),
            payload
                .companion_id
                .as_deref()
                .context("companion id required")?,
        )?,
        "equip" => equip_item(
            conn,
            payload.item_id.as_deref().context("item id required")?,
            payload.equip_slot.as_deref(),
            &mut player,
        )?,
        "unequip" => unequip_item(
            conn,
            payload
                .equip_slot
                .as_deref()
                .or(payload.item_id.as_deref())
                .context("slot or item id required")?,
            &mut player,
        )?,
        "enhance" => enhance_item(
            conn,
            payload
                .item_id
                .as_deref()
                .context("item id or slot required")?,
            &mut player,
        )?,
        "skill_use" => use_skill(
            conn,
            payload.skill_id.as_deref().unwrap_or("slash"),
            &mut player,
        )?,
        "shop_buy" => buy_item(
            conn,
            payload.item_id.as_deref().context("item id required")?,
            &mut player,
        )?,
        "shop_sell" => sell_item(
            conn,
            payload.item_id.as_deref().context("item id required")?,
            &mut player,
        )?,
        "sell_common" => sell_common_items(conn, &mut player, payload.rarity.as_deref())?,
        "quest_claim" => vibemud_db::claim_daily_quest(
            conn,
            payload.quest_id.as_deref().context("quest id required")?,
        )?,
        "quest_claim_all" => vibemud_db::claim_all_daily_quests(conn)?.join("\n"),
        "item_lock" => set_item_lock(
            conn,
            payload.item_id.as_deref().context("item id required")?,
            true,
        )?,
        "item_unlock" => set_item_lock(
            conn,
            payload.item_id.as_deref().context("item id required")?,
            false,
        )?,
        other => anyhow::bail!("unsupported mutation command: {other}"),
    };
    Ok(message)
}

enum AdventureTarget {
    Area(String),
    Dungeon(String),
}

fn resolve_hunt_start_target(
    conn: &rusqlite::Connection,
    player: &vibemud_core::PlayerState,
    requested_area: Option<&str>,
) -> Result<AdventureTarget> {
    if let Some(area) = requested_area.filter(|area| !area.trim().is_empty()) {
        return Ok(AdventureTarget::Area(area.to_string()));
    }

    if matches!(
        setting_value(conn, LAST_ADVENTURE_MODE_KEY)?.as_deref(),
        Some("dungeon")
    ) {
        if let Some(dungeon) =
            setting_value(conn, LAST_DUNGEON_KEY)?.filter(|dungeon| !dungeon.trim().is_empty())
        {
            return Ok(AdventureTarget::Dungeon(dungeon));
        }
    }

    if let Some(dungeon) = player
        .current_dungeon_id
        .as_deref()
        .filter(|dungeon| !dungeon.trim().is_empty())
    {
        return Ok(AdventureTarget::Dungeon(dungeon.to_string()));
    }

    if let Some(area) =
        setting_value(conn, LAST_AREA_KEY)?.filter(|area| !matches!(area.as_str(), "" | "town"))
    {
        return Ok(AdventureTarget::Area(area));
    }

    if let Some(area) = player
        .current_area_id
        .as_deref()
        .filter(|area| !matches!(*area, "" | "town"))
    {
        return Ok(AdventureTarget::Area(area.to_string()));
    }

    Ok(AdventureTarget::Area("forest-edge".to_string()))
}

fn remember_area_target(conn: &rusqlite::Connection, area: &str) -> Result<()> {
    if area.trim().is_empty() || area == "town" {
        return Ok(());
    }
    set_setting(conn, LAST_ADVENTURE_MODE_KEY, "area")?;
    set_setting(conn, LAST_AREA_KEY, area)?;
    clear_setting(conn, LAST_DUNGEON_KEY)?;
    Ok(())
}

fn remember_dungeon_target(conn: &rusqlite::Connection, dungeon: &str) -> Result<()> {
    if dungeon.trim().is_empty() {
        return Ok(());
    }
    set_setting(conn, LAST_ADVENTURE_MODE_KEY, "dungeon")?;
    set_setting(conn, LAST_DUNGEON_KEY, dungeon)?;
    Ok(())
}

fn recruit_companion(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
) -> Result<String> {
    let cost = 50;
    if player.gold < cost {
        anyhow::bail!("not enough gold to recruit; need {cost}");
    }
    let candidate: Option<(String, String, String)> = conn.query_row(
        "SELECT id, name, rarity FROM companions WHERE unlocked = 0 ORDER BY CASE rarity WHEN 'Common' THEN 1 WHEN 'Rare' THEN 2 ELSE 3 END, id LIMIT 1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional()?;
    let Some((id, name, rarity)) = candidate else {
        return Ok("All companions already recruited".to_string());
    };
    player.gold -= cost;
    vibemud_db::record_quest_progress(conn, "gold_spend", None, cost)?;
    conn.execute(
        "UPDATE companions SET unlocked = 1, affinity = affinity + 1 WHERE id = ?1",
        [&id],
    )?;
    let empty_slot: Option<i64> = conn.query_row("SELECT slot_index FROM party_slots WHERE companion_id IS NULL ORDER BY slot_index LIMIT 1", [], |row| row.get(0)).optional()?;
    if let Some(slot) = empty_slot {
        conn.execute(
            "UPDATE party_slots SET companion_id = ?1 WHERE slot_index = ?2",
            params![id, slot],
        )?;
    }
    upsert_player(conn, player)?;
    Ok(format!("Recruited {name} ({rarity})"))
}

fn swap_party(conn: &rusqlite::Connection, slot: u8, companion_id: &str) -> Result<String> {
    let unlocked: Option<i64> = conn
        .query_row(
            "SELECT unlocked FROM companions WHERE id = ?1",
            [companion_id],
            |row| row.get(0),
        )
        .optional()?;
    if unlocked.unwrap_or(0) == 0 {
        anyhow::bail!("companion {companion_id} is not recruited");
    }
    if !(1..=3).contains(&slot) {
        anyhow::bail!("party slot must be 1..3");
    }
    conn.execute(
        "UPDATE party_slots SET companion_id = NULL WHERE companion_id = ?1",
        [companion_id],
    )?;
    conn.execute(
        "UPDATE party_slots SET companion_id = ?1 WHERE slot_index = ?2",
        params![companion_id, slot],
    )?;
    Ok(format!("Moved {companion_id} to party slot {slot}"))
}

fn buy_item(
    conn: &rusqlite::Connection,
    item_id: &str,
    player: &mut vibemud_core::PlayerState,
) -> Result<String> {
    let (name, item_type, rarity, cost, durability) =
        shop_item(item_id).with_context(|| format!("unknown shop item {item_id}"))?;
    if player.gold < cost {
        anyhow::bail!("not enough gold for {name}; need {cost}");
    }
    if !inventory_has_room_for_one(conn)? {
        return Ok(format!("소지품창이 가득 차 {name} 구매가 취소되었습니다."));
    }
    player.gold -= cost;
    let stored = add_item(
        conn, item_id, name, item_type, rarity, durability, durability,
    )?;
    if !stored {
        player.gold += cost;
        upsert_player(conn, player)?;
        return Ok(format!("소지품창이 가득 차 {name} 구매가 취소되었습니다."));
    }
    vibemud_db::record_quest_progress(conn, "gold_spend", None, cost)?;
    upsert_player(conn, player)?;
    Ok(format!("Bought {name} for {cost} gold"))
}

fn sell_item(
    conn: &rusqlite::Connection,
    item_id: &str,
    player: &mut vibemud_core::PlayerState,
) -> Result<String> {
    struct SellItemRow {
        instance_id: String,
        base_item_id: String,
        name: String,
        rarity: String,
        quantity: i64,
        equipped_slot: Option<String>,
        level: i64,
    }

    let row: Option<SellItemRow> = conn
        .query_row(
            "SELECT i.id, i.item_id, COALESCE(e.name, i.name), COALESCE(e.rarity, i.rarity), i.quantity, i.equipped_slot, COALESCE(i.enhancement_level, 0)
             FROM inventory_items i
             LEFT JOIN equipment_items e ON e.item_id = i.item_id
             WHERE i.id = ?1 OR i.item_id = ?1
            LIMIT 1",
            [item_id],
            |row| {
                Ok(SellItemRow {
                    instance_id: row.get(0)?,
                    base_item_id: row.get(1)?,
                    name: row.get(2)?,
                    rarity: row.get(3)?,
                    quantity: row.get(4)?,
                    equipped_slot: row.get(5)?,
                    level: row.get(6)?,
                })
            },
        )
        .optional()?;
    let Some(row) = row else {
        return Ok(format!(
            "Sale skipped: item {item_id} is no longer in inventory"
        ));
    };
    let definition = load_equipment_definition(conn, &row.base_item_id)?;
    if let Some(slot) = row.equipped_slot.as_deref() {
        if let Some(def) = definition.as_ref() {
            apply_equipment_delta(conn, player, def, row.level.max(0) as u8, -1, slot)?;
        }
    }
    let sale_price = sale_price_for_rarity(&row.rarity);
    if row.quantity > 1 {
        conn.execute(
            "UPDATE inventory_items SET quantity = quantity - 1 WHERE id = ?1 OR item_id = ?1",
            [item_id],
        )?;
    } else {
        conn.execute(
            "DELETE FROM inventory_items WHERE id = ?1",
            [row.instance_id],
        )?;
    }
    player.gold += sale_price;
    vibemud_db::record_quest_progress(conn, "item_sell", None, 1)?;
    upsert_player(conn, player)?;
    Ok(format!("Sold {} for {sale_price} gold", row.name))
}

fn sell_common_items(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    rarity_threshold: Option<&str>,
) -> Result<String> {
    if let Some(value) = rarity_threshold {
        let _legacy_rank = parse_bulk_sell_rarity(value)?.rank;
    }
    let mut stmt = conn.prepare(
        "SELECT i.id, i.quantity, COALESCE(e.rarity, i.rarity), COALESCE(i.locked, 0)
         FROM inventory_items i
         LEFT JOIN equipment_items e ON e.item_id = i.item_id
         WHERE i.equipped_slot IS NULL
         ORDER BY acquired_at, id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)? != 0,
        ))
    })?;
    let all_items = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let locked_count = all_items.iter().filter(|(_, _, _, locked)| *locked).count();
    let items = all_items
        .into_iter()
        .filter(|(_, _, _, locked)| !locked)
        .collect::<Vec<_>>();
    if items.is_empty() {
        if locked_count > 0 {
            return Ok(format!(
                "No unlocked items to empty; skipped {locked_count} locked items"
            ));
        }
        return Ok("No items to empty from inventory".to_string());
    }

    let mut sold_count = 0_i64;
    let mut common_sold_count = 0_i64;
    let mut total_gold = 0_i64;
    for (instance_id, quantity, rarity, _locked) in items {
        let quantity = quantity.max(1);
        let sale_price = sale_price_for_rarity(&rarity);
        sold_count += quantity;
        if item_rarity_rank(&rarity) == Some(1) {
            common_sold_count += quantity;
        }
        total_gold += sale_price * quantity;
        conn.execute("DELETE FROM inventory_items WHERE id = ?1", [instance_id])?;
    }

    player.gold += total_gold;
    if common_sold_count > 0 {
        vibemud_db::record_quest_progress(conn, "sell_common", None, common_sold_count)?;
    }
    vibemud_db::record_quest_progress(conn, "item_sell", None, sold_count)?;
    upsert_player(conn, player)?;
    let skipped = if locked_count > 0 {
        format!("; skipped {locked_count} locked")
    } else {
        String::new()
    };
    Ok(format!(
        "Emptied inventory: sold {sold_count} unlocked items for {total_gold} gold{skipped}"
    ))
}

fn set_item_lock(conn: &rusqlite::Connection, item_id: &str, locked: bool) -> Result<String> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT i.id, COALESCE(e.name, i.name)
             FROM inventory_items i
             LEFT JOIN equipment_items e ON e.item_id = i.item_id
             WHERE i.id = ?1 OR i.item_id = ?1
             LIMIT 1",
            [item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((instance_id, name)) = row else {
        anyhow::bail!("item {item_id} not found");
    };
    conn.execute(
        "UPDATE inventory_items SET locked = ?1 WHERE id = ?2",
        params![if locked { 1_i64 } else { 0_i64 }, instance_id],
    )?;
    Ok(if locked {
        format!("Locked {name}")
    } else {
        format!("Unlocked {name}")
    })
}

fn sale_price_for_rarity(rarity: &str) -> i64 {
    match item_rarity_rank(rarity).unwrap_or(0) {
        1 => 10,
        2 => 25,
        3 => 75,
        4 => 200,
        5 => 500,
        _ => 5,
    }
}

struct BulkSellRarity {
    rank: u8,
}

fn parse_bulk_sell_rarity(value: &str) -> Result<BulkSellRarity> {
    let normalized = value.trim().to_ascii_lowercase();
    let rank = match normalized.as_str() {
        "" | "common" | "일반" | "normal" | "white" | "sell_common" | "sell_upto_common"
        | "empty_inventory" => 1,
        "uncommon" | "고급" | "green" | "sell_upto_uncommon" => 2,
        "rare" | "희귀" | "blue" | "sell_upto_rare" => 3,
        "epic" | "영웅" | "hero" | "purple" | "sell_upto_epic" => 4,
        "legendary" | "전설" | "yellow" => {
            anyhow::bail!("bulk selling legendary items is not supported; choose up to Epic/영웅")
        }
        other => anyhow::bail!("unknown bulk sell rarity threshold: {other}"),
    };
    Ok(BulkSellRarity { rank })
}

fn item_rarity_rank(value: &str) -> Option<u8> {
    match value.trim() {
        "Common" | "common" | "일반" | "white" => Some(1),
        "Uncommon" | "uncommon" | "고급" | "green" => Some(2),
        "Rare" | "rare" | "희귀" | "blue" => Some(3),
        "Epic" | "epic" | "영웅" | "purple" => Some(4),
        "Legendary" | "legendary" | "전설" | "yellow" => Some(5),
        _ => None,
    }
}

fn equip_item(
    conn: &rusqlite::Connection,
    item_id: &str,
    requested_slot: Option<&str>,
    player: &mut vibemud_core::PlayerState,
) -> Result<String> {
    let row: Option<(String, String, String, i64)> = conn
        .query_row(
            "SELECT id, item_id, item_type, COALESCE(enhancement_level, 0) FROM inventory_items WHERE id = ?1 OR item_id = ?1 LIMIT 1",
            [item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let Some((instance_id, base_item_id, item_type, level)) = row else {
        anyhow::bail!("item {item_id} not found");
    };
    if item_type == "consumable" {
        anyhow::bail!("consumables cannot be equipped");
    }
    let definition = load_equipment_definition(conn, &base_item_id)?;
    let default_slot = definition
        .as_ref()
        .map(|def| def.slot.as_str())
        .unwrap_or(match item_type.as_str() {
            "weapon" => "weapon",
            "subweapon" => "subweapon",
            "armor" | "armor_top" => "armor_top",
            "armor_bottom" => "armor_bottom",
            "trinket" => "trinket",
            "boots" => "boots",
            "pet" => "pet",
            "special" => "special",
            _ => "trinket",
        })
        .to_string();
    let slot = canonical_equip_slot(requested_slot.unwrap_or(&default_slot));
    if requested_slot.is_some()
        && !can_equip_item_in_slot(definition.as_ref(), item_type.as_str(), &slot)
    {
        anyhow::bail!("{item_id} cannot be equipped in {slot}");
    }

    let old = equipped_equipment(conn, &slot)?;
    if let Some((old_id, old_item_id, old_level, _old_name)) = old {
        if old_id == instance_id {
            return Ok(format!("{item_id} is already equipped in {slot}"));
        }
        if old_id != instance_id {
            if let Some(old_def) = load_equipment_definition(conn, &old_item_id)? {
                apply_equipment_delta(conn, player, &old_def, old_level, -1, &slot)?;
            }
            conn.execute(
                "UPDATE inventory_items SET equipped_slot = NULL WHERE id = ?1",
                [old_id],
            )?;
        }
    }
    conn.execute(
        "UPDATE inventory_items SET equipped_slot = ?1 WHERE id = ?2",
        params![slot, instance_id],
    )?;
    if let Some(def) = definition.as_ref() {
        apply_equipment_delta(conn, player, def, level.max(0) as u8, 1, &slot)?;
    } else if slot == "weapon" {
        player.attack += 3;
    } else if slot == "armor_top" {
        player.defense += 3;
    }
    upsert_player(conn, player)?;
    Ok(format!("Equipped {item_id} in {slot}"))
}

fn unequip_item(
    conn: &rusqlite::Connection,
    item_or_slot: &str,
    player: &mut vibemud_core::PlayerState,
) -> Result<String> {
    let target = canonical_equip_slot(item_or_slot);
    let row: Option<(String, String, String, i64, String)> = conn
        .query_row(
            "SELECT id, item_id, name, COALESCE(enhancement_level, 0), COALESCE(equipped_slot, '')
             FROM inventory_items
             WHERE equipped_slot = ?1 OR ((id = ?1 OR item_id = ?1) AND equipped_slot IS NOT NULL)
             LIMIT 1",
            [target.as_str()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((instance_id, base_item_id, name, level, slot)) = row else {
        anyhow::bail!("unequip target {item_or_slot} is not equipped equipment");
    };
    if let Some(def) = load_equipment_definition(conn, &base_item_id)? {
        apply_equipment_delta(conn, player, &def, level.max(0) as u8, -1, &slot)?;
    }
    conn.execute(
        "UPDATE inventory_items SET equipped_slot = NULL WHERE id = ?1",
        [instance_id],
    )?;
    upsert_player(conn, player)?;
    Ok(format!("Unequipped {name} from {slot}"))
}

fn enhance_item(
    conn: &rusqlite::Connection,
    item_or_slot: &str,
    player: &mut vibemud_core::PlayerState,
) -> Result<String> {
    let item_or_slot = item_or_slot.trim();
    let target = canonical_equipment_target(item_or_slot);
    let row: Option<(String, String, String, i64, String, String)> = conn
        .query_row(
            "SELECT i.id, i.item_id, i.name, COALESCE(i.enhancement_level, 0), COALESCE(i.equipped_slot, ''), COALESCE(e.rarity, i.rarity)
             FROM inventory_items i
             LEFT JOIN equipment_items e ON e.item_id = i.item_id
             WHERE i.id = ?1 OR i.item_id = ?1 OR i.equipped_slot = ?1
             ORDER BY CASE WHEN i.equipped_slot IS NULL THEN 1 ELSE 0 END
             LIMIT 1",
            [target.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .optional()?;
    let Some((instance_id, base_item_id, name, level, equipped_slot, rarity)) = row else {
        anyhow::bail!("enhance target {item_or_slot} is not equipped equipment");
    };
    if !equipped_slot.is_empty() {
        anyhow::bail!("equipped items cannot be enhanced; unequip {name} first");
    }
    let level = level.max(0) as u8;
    let Some(def) = load_equipment_definition(conn, &base_item_id)? else {
        anyhow::bail!("{name} cannot be enhanced because it is not equipment");
    };
    let slot = if equipped_slot.is_empty() {
        def.slot.as_str()
    } else {
        equipped_slot.as_str()
    };
    let rule = adjusted_enhancement_rule(conn, level, &rarity, slot)?
        .context("enhancement rule missing")?;
    let Some(cost) = rule.upgrade_gold_cost else {
        anyhow::bail!("{name} is already at max enhancement");
    };
    let Some(success_rate) = rule.success_rate else {
        anyhow::bail!("{name} is already at max enhancement");
    };
    if player.gold < cost {
        anyhow::bail!("not enough gold to enhance {name}; need {cost}");
    }
    player.gold -= cost;
    vibemud_db::record_quest_progress(conn, "gold_spend", None, cost)?;
    vibemud_db::record_quest_progress(conn, "enhance_attempt", Some(slot), 1)?;
    let success = deterministic_roll01(&format!(
        "enhance:{instance_id}:{level}:{}",
        latest_seed(conn)?
    )) <= success_rate;
    let new_level = if success {
        level.saturating_add(1)
    } else {
        level.saturating_sub(rule.failure_level_drop)
    };
    conn.execute(
        "UPDATE inventory_items SET enhancement_level = ?1 WHERE id = ?2",
        params![new_level as i64, instance_id],
    )?;
    vibemud_db::record_quest_progress(
        conn,
        if success {
            "enhance_success"
        } else {
            "enhance_fail"
        },
        Some(slot),
        1,
    )?;
    upsert_player(conn, player)?;
    if success {
        Ok(format!(
            "{name} 강화성공 +{new_level} ( {level} > {new_level} )"
        ))
    } else {
        Ok(format!(
            "{name} 강화실패 +{new_level} ( {level} > {new_level} )"
        ))
    }
}

fn canonical_equipment_target(target: &str) -> String {
    canonical_equip_slot(target)
}

fn canonical_equip_slot(target: &str) -> String {
    match target.trim() {
        "무기" => "weapon",
        "주무기" => "weapon",
        "부무기" => "subweapon",
        "상의" | "갑옷" | "방어구" | "armor" => "armor_top",
        "하의" => "armor_bottom",
        "장신구" => "trinket",
        "신발" => "boots",
        "펫" => "pet",
        "특수장비" | "특수" => "special",
        other => other,
    }
    .to_string()
}

fn can_equip_item_in_slot(
    definition: Option<&EquipmentDefinition>,
    item_type: &str,
    slot: &str,
) -> bool {
    let base_slot = definition.map(|def| def.slot.as_str()).unwrap_or(item_type);
    match (base_slot, slot) {
        ("weapon" | "subweapon", "weapon" | "subweapon") => true,
        (base, target) => base == target,
    }
}

fn use_skill(
    conn: &rusqlite::Connection,
    skill_id: &str,
    player: &mut vibemud_core::PlayerState,
) -> Result<String> {
    let message = match skill_id {
        "heal" => {
            player.hp = (player.hp + 35).min(player.max_hp);
            "Cast Heal".to_string()
        }
        "guard" => {
            player.defense += 1;
            "Used Guard".to_string()
        }
        "firebolt" => {
            player.mp = (player.mp - 5).max(0);
            "Cast Firebolt".to_string()
        }
        "taunt" => "Used Taunt".to_string(),
        _ => "Used Slash".to_string(),
    };
    upsert_player(conn, player)?;
    Ok(message)
}

#[derive(Debug, Clone)]
struct EquipmentDropRule {
    rarity: String,
    min_tier: u8,
    max_tier: u8,
}

fn maybe_drop_equipment(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    monster_id: &str,
    monster_grade: &str,
    seed: u64,
) -> Result<Option<String>> {
    let drop_multiplier = vibe_fever_multiplier() * luck_drop_multiplier(player.luck);
    let Some(rule) =
        select_equipment_drop_rule(conn, monster_id, monster_grade, drop_multiplier, seed)?
    else {
        return Ok(None);
    };
    let candidates = equipment_candidates(conn, &rule.rarity, rule.min_tier, rule.max_tier)?;
    if candidates.is_empty() {
        return Ok(None);
    }
    let index = deterministic_index(
        &format!(
            "item:{monster_id}:{}:{}:{}:{seed}",
            rule.rarity, rule.min_tier, rule.max_tier
        ),
        candidates.len(),
    );
    let def = candidates[index].clone();
    Ok(Some(acquire_equipment_drop(conn, player, &def, seed)?))
}

fn select_equipment_drop_rule(
    conn: &rusqlite::Connection,
    monster_id: &str,
    monster_grade: &str,
    drop_multiplier: f64,
    seed: u64,
) -> Result<Option<EquipmentDropRule>> {
    let mut stmt = conn.prepare(
        "SELECT rarity, drop_chance, min_tier, max_tier
         FROM monster_equipment_drops
         WHERE monster_id = ?1 AND monster_grade = ?2
         ORDER BY drop_chance ASC, rarity",
    )?;
    let rows = stmt.query_map(params![monster_id, monster_grade], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, f64>(1)?,
            row.get::<_, i64>(2)?.clamp(1, 3) as u8,
            row.get::<_, i64>(3)?.clamp(1, 3) as u8,
        ))
    })?;
    for row in rows {
        let (rarity, chance, min_tier, max_tier) = row?;
        let chance = (chance * drop_multiplier).clamp(0.0, 1.0);
        let roll = deterministic_roll01(&format!("drop:{monster_id}:{rarity}:{seed}"));
        if chance > 0.0 && roll <= chance {
            return Ok(Some(EquipmentDropRule {
                rarity,
                min_tier: min_tier.min(max_tier),
                max_tier: max_tier.max(min_tier),
            }));
        }
    }
    Ok(None)
}

fn equipment_candidates(
    conn: &rusqlite::Connection,
    rarity: &str,
    min_tier: u8,
    max_tier: u8,
) -> Result<Vec<EquipmentDefinition>> {
    let mut stmt = conn.prepare(
        "SELECT item_id FROM equipment_items WHERE rarity = ?1 AND tier BETWEEN ?2 AND ?3 ORDER BY slot, tier, item_id",
    )?;
    let rows = stmt.query_map(
        params![rarity, min_tier.min(max_tier), max_tier.max(min_tier)],
        |row| row.get::<_, String>(0),
    )?;
    let mut out = Vec::new();
    for row in rows {
        if let Some(def) = load_equipment_definition(conn, &row?)? {
            out.push(def);
        }
    }
    Ok(out)
}

fn acquire_equipment_drop(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    def: &EquipmentDefinition,
    seed: u64,
) -> Result<String> {
    if !inventory_has_room_for_one(conn)? {
        let tx = if conn.is_autocommit() {
            Some(conn.unchecked_transaction()?)
        } else {
            None
        };
        let write_conn = tx.as_deref().unwrap_or(conn);
        let Some((evicted_name, evicted_rarity)) =
            evict_lower_rarity_equipment(write_conn, &def.rarity)?
        else {
            return Ok(format!(
                "장비 획득 실패: 소지품창이 가득 차 {} [{}] 아이템이 삭제되었습니다.",
                def.name, def.rarity
            ));
        };
        insert_equipment_drop(write_conn, player, def, seed)?;
        if let Some(tx) = tx {
            tx.commit()?;
        }
        let eviction_note = format!(" ({evicted_name} [{evicted_rarity}] 아이템을 정리.)");
        return Ok(format!(
            "장비 획득: {} [{} {} 티어{}] 소지품창에 보관.{eviction_note}",
            def.name, def.rarity, def.rarity_color, def.tier
        ));
    }

    insert_equipment_drop(conn, player, def, seed)?;
    Ok(format!(
        "장비 획득: {} [{} {} 티어{}] 소지품창에 보관.",
        def.name, def.rarity, def.rarity_color, def.tier
    ))
}

fn insert_equipment_drop(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    def: &EquipmentDefinition,
    seed: u64,
) -> Result<()> {
    let instance_id = format!(
        "{}-{}-{}",
        def.item_id,
        vibemud_db::latest_state_version(conn)? + 1,
        seed & 0xffff
    );
    conn.execute(
        "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, durability, max_durability, equipped_slot, enhancement_level, acquired_at) VALUES (?1, ?2, ?3, ?4, ?5, 1, 100, 100, NULL, 0, ?6)",
        params![instance_id, def.item_id, def.slot, def.name, def.rarity, vibemud_db::now()],
    )?;
    vibemud_db::mod_bridge::record_adventure_rewards(conn, 0, 0, 0, Some(&def.name))?;
    upsert_player(conn, player)
}

/// When the bag is full, frees one slot by deleting the lowest-rarity spare
/// equipment piece, but only if it is strictly lower rarity than the incoming
/// drop. Locked, equipped, enhanced, and consumable items are never evicted.
fn evict_lower_rarity_equipment(
    conn: &rusqlite::Connection,
    incoming_rarity: &str,
) -> Result<Option<(String, String)>> {
    let Some(incoming_rank) = item_rarity_rank(incoming_rarity) else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT id, name, rarity FROM inventory_items
         WHERE equipped_slot IS NULL AND locked = 0 AND enhancement_level = 0
           AND item_type != 'consumable'
         ORDER BY acquired_at ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut worst: Option<(u8, String, String, String)> = None;
    for row in rows {
        let (id, name, rarity) = row?;
        let Some(rank) = item_rarity_rank(&rarity) else {
            continue;
        };
        if worst
            .as_ref()
            .is_none_or(|(worst_rank, ..)| rank < *worst_rank)
        {
            worst = Some((rank, id, name, rarity));
        }
    }
    match worst {
        Some((rank, id, name, rarity)) if rank < incoming_rank => {
            conn.execute("DELETE FROM inventory_items WHERE id = ?1", [id])?;
            Ok(Some((name, rarity)))
        }
        _ => Ok(None),
    }
}

fn unequipped_inventory_count(conn: &rusqlite::Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COALESCE(SUM(quantity), 0) FROM inventory_items WHERE equipped_slot IS NULL",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|count| count.max(0))
    .map_err(Into::into)
}

fn inventory_has_room_for_one(conn: &rusqlite::Connection) -> Result<bool> {
    Ok(unequipped_inventory_count(conn)? < INVENTORY_CAPACITY)
}

type EquippedRow = (String, String, u8, String);

fn equipped_equipment(conn: &rusqlite::Connection, slot: &str) -> Result<Option<EquippedRow>> {
    conn.query_row(
        "SELECT id, item_id, COALESCE(enhancement_level, 0), name FROM inventory_items WHERE equipped_slot = ?1 LIMIT 1",
        [slot],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get::<_, i64>(2)?.max(0) as u8,
                row.get(3)?,
            ))
        },
    )
    .optional()
    .map_err(Into::into)
}

fn apply_equipment_delta(
    conn: &rusqlite::Connection,
    player: &mut vibemud_core::PlayerState,
    def: &EquipmentDefinition,
    level: u8,
    sign: i32,
    equipped_slot: &str,
) -> Result<()> {
    let multiplier = load_enhancement_rule(conn, level)?
        .map(|rule| rule.stat_multiplier_bps)
        .unwrap_or(10_000);
    for (stat, value) in effective_equipment_stats_for_slot(def, multiplier, equipped_slot) {
        apply_stat_delta(player, &stat, value * sign);
    }
    Ok(())
}

fn apply_stat_delta(player: &mut vibemud_core::PlayerState, stat: &str, delta: i32) {
    match stat {
        "attack" => player.attack = (player.attack + delta).max(1),
        "defense" => player.defense = (player.defense + delta).max(0),
        "accuracy" => player.accuracy = (player.accuracy + delta).max(1),
        "evasion" => player.evasion = (player.evasion + delta).max(0),
        "speed" => player.speed = (player.speed + delta).max(1),
        "regen" => player.regen = (player.regen + delta).max(0),
        "luck" => player.luck = (player.luck + delta).max(0),
        "max_hp" => {
            player.max_hp = (player.max_hp + delta).max(1);
            player.hp = player.hp.min(player.max_hp).max(1);
        }
        "max_mp" => {
            player.max_mp = (player.max_mp + delta).max(0);
            player.mp = player.mp.min(player.max_mp).max(0);
        }
        _ => {}
    }
}

fn deterministic_roll01(input: &str) -> f64 {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    (hasher.finish() % 10_000) as f64 / 10_000.0
}

fn deterministic_index(input: &str, len: usize) -> usize {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    (hasher.finish() as usize) % len.max(1)
}

fn vibe_fever_multiplier() -> f64 {
    if vibemud_db::vibe_fever_active() {
        1.5
    } else {
        1.0
    }
}

fn equipped_reward_bonus_multiplier(conn: &rusqlite::Connection, stat_type: &str) -> Result<f64> {
    let bonus_pct: i32 = vibemud_db::load_inventory(conn)?
        .into_iter()
        .filter(|item| item.equipped_slot.is_some())
        .flat_map(|item| {
            [
                (item.stat1_type, item.stat1_value),
                (item.stat2_type, item.stat2_value),
                (item.stat3_type, item.stat3_value),
            ]
        })
        .filter_map(|(stat, value)| match (stat.as_deref(), value) {
            (Some(stat), Some(value)) if stat == stat_type => Some(value.max(0)),
            _ => None,
        })
        .sum();
    Ok((1.0 + bonus_pct as f64 / 100.0).clamp(1.0, 1.75))
}

fn boosted_u64(value: u64, multiplier: f64) -> u64 {
    ((value as f64) * multiplier).floor() as u64
}

fn boosted_i64(value: i64, multiplier: f64) -> i64 {
    ((value as f64) * multiplier).floor() as i64
}

fn dungeon_floor(conn: &rusqlite::Connection) -> Result<i64> {
    Ok(conn
        .query_row(
            "SELECT turn_index FROM combat_state WHERE id = 'main'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0))
}

fn add_item(
    conn: &rusqlite::Connection,
    item_id: &str,
    name: &str,
    item_type: &str,
    rarity: &str,
    durability: Option<i32>,
    max_durability: Option<i32>,
) -> Result<bool> {
    if !inventory_has_room_for_one(conn)? {
        return Ok(false);
    }

    let stackable = item_type == "consumable";
    if stackable {
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM inventory_items WHERE item_id = ?1 AND equipped_slot IS NULL LIMIT 1",
                [item_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = existing {
            conn.execute(
                "UPDATE inventory_items SET quantity = quantity + 1 WHERE id = ?1",
                [id],
            )?;
            return Ok(true);
        }
    }

    let owned_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM inventory_items WHERE item_id = ?1",
        [item_id],
        |row| row.get(0),
    )?;
    let id = format!(
        "{}-{}-{}",
        item_id,
        vibemud_db::latest_state_version(conn)? + 1,
        owned_count + 1
    );
    conn.execute("INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, durability, max_durability, enhancement_level, acquired_at) VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, 0, ?8)", params![id, item_id, item_type, name, rarity, durability, max_durability, vibemud_db::now()])?;
    Ok(true)
}

fn shop_item(
    item_id: &str,
) -> Option<(&'static str, &'static str, &'static str, i64, Option<i32>)> {
    match item_id {
        "potion-small" => Some(("Asclepius Potion", "consumable", "Common", 25, None)),
        "potion-medium" => Some(("Hygieia Potion", "consumable", "Common", 55, None)),
        "basic-sword" => Some(("Ares Sword", "weapon", "Common", 80, Some(100))),
        "basic-staff" => Some(("Hermes Staff", "weapon", "Common", 80, Some(100))),
        "leather-armor" => Some(("Leonidas Armor", "armor", "Common", 90, Some(100))),
        "repair-kit" => Some(("Daedalus Kit", "consumable", "Common", 40, None)),
        _ => None,
    }
}

fn dungeon_reward(
    dungeon_id: &str,
) -> (&'static str, &'static str, &'static str, &'static str, i64) {
    match dungeon_id {
        "crystal-cave" => ("crystal-blade", "Perseus Blade", "weapon", "Rare", 180),
        "lich-tomb" => ("lich-amulet", "Hecate Amulet", "trinket", "Epic", 350),
        "cyclops-forge" => ("cyclops-hammer", "Hephaestus Hammer", "weapon", "Epic", 520),
        "medusa-temple" => ("gorgon-seal", "Athena Aegis", "special", "Epic", 760),
        "titan-vault" => ("titan-key", "Kronos Key", "trinket", "Legendary", 1200),
        _ => ("goblin-chief-axe", "Hector Axe", "weapon", "Rare", 140),
    }
}

fn latest_seed(conn: &rusqlite::Connection) -> Result<u64> {
    let version = vibemud_db::latest_state_version(conn)?;
    Ok(0x5EED_u64 ^ version.rotate_left(13))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[cfg(unix)]
    fn spawn_fake_runtime(root: &Path) -> std::process::Child {
        let runtime_path = root.join("vibemud-runtime");
        let _ = std::fs::remove_file(&runtime_path);
        std::os::unix::fs::symlink("/bin/sleep", &runtime_path).unwrap();
        std::process::Command::new(runtime_path)
            .arg("30")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }

    #[test]
    fn interrupted_tick_rolls_back_purchase_and_queue_ack_together() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        let (_, conn) = vibemud_db::open_app().unwrap();
        let before = load_player(&conn).unwrap().gold;
        let id = vibemud_db::enqueue_command(
            &conn,
            "test",
            vibemud_core::CommandKind::ShopBuy,
            &vibemud_core::CommandPayload {
                item_id: Some("potion-small".into()),
                ..Default::default()
            },
        )
        .unwrap();
        conn.execute_batch("CREATE TRIGGER fail_event BEFORE INSERT ON event_log BEGIN SELECT RAISE(ABORT, 'simulated disk error'); END;").unwrap();
        assert!(run_one_tick(&conn, 0).is_err());
        assert_eq!(load_player(&conn).unwrap().gold, before);
        assert_eq!(
            vibemud_db::mod_bridge::result(&conn, &id).unwrap()["status"],
            "pending"
        );
        conn.execute_batch("DROP TRIGGER fail_event").unwrap();
        run_one_tick(&conn, 0).unwrap();
        assert_eq!(load_player(&conn).unwrap().gold, before - 25);
        assert_eq!(
            vibemud_db::mod_bridge::result(&conn, &id).unwrap()["status"],
            "done"
        );
        run_one_tick(&conn, 1).unwrap();
        assert_eq!(load_player(&conn).unwrap().gold, before - 25);
    }

    #[cfg(unix)]
    #[test]
    fn runtime_lock_rejects_live_pid_and_replaces_stale_pid() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        let paths = vibemud_db::init_app().unwrap();
        let lock_path = paths.root.join(RUNTIME_LOCK_FILE);

        let mut runtime = spawn_fake_runtime(tmp.path());
        std::fs::write(&lock_path, runtime.id().to_string()).unwrap();
        assert!(RuntimeProcessGuard::acquire(&paths.root).is_err());
        let _ = runtime.kill();
        let _ = runtime.wait();

        std::fs::write(&lock_path, "999999999").unwrap();
        let guard = RuntimeProcessGuard::acquire(&paths.root).unwrap();
        assert_eq!(
            std::fs::read_to_string(&lock_path).unwrap().trim(),
            std::process::id().to_string()
        );
        drop(guard);
        assert!(!lock_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn stop_runtime_terminates_recorded_runtime_pid() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (paths, conn) = vibemud_db::open_app().unwrap();
        let mut child = spawn_fake_runtime(tmp.path());
        vibemud_db::set_session_status(&conn, "running", Some(child.id())).unwrap();
        std::fs::write(paths.root.join(RUNTIME_LOCK_FILE), child.id().to_string()).unwrap();

        stop_runtime().unwrap();

        for _ in 0..20 {
            if child.try_wait().unwrap().is_some() {
                assert!(
                    !paths.root.join(RUNTIME_LOCK_FILE).exists(),
                    "runtime lock should be removed after stopping the recorded pid"
                );
                return;
            }
            thread::sleep(StdDuration::from_millis(100));
        }
        let _ = child.kill();
        panic!("recorded runtime pid was not terminated");
    }

    #[cfg(unix)]
    #[test]
    fn stop_runtime_terminates_lock_only_runtime_pid() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        let paths = vibemud_db::init_app().unwrap();
        let mut child = spawn_fake_runtime(tmp.path());
        std::fs::write(paths.root.join(RUNTIME_LOCK_FILE), child.id().to_string()).unwrap();

        assert_eq!(status_runtime().unwrap(), "running");
        stop_runtime().unwrap();

        for _ in 0..20 {
            if child.try_wait().unwrap().is_some() {
                assert!(
                    !paths.root.join(RUNTIME_LOCK_FILE).exists(),
                    "runtime lock should be removed after stopping the lock-only pid"
                );
                return;
            }
            thread::sleep(StdDuration::from_millis(100));
        }
        let _ = child.kill();
        panic!("lock-only runtime pid was not terminated");
    }

    #[cfg(unix)]
    #[test]
    fn status_runtime_recovers_lock_pid_when_db_pid_is_stale() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        let paths = vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut child = spawn_fake_runtime(tmp.path());
        vibemud_db::set_session_status(&conn, "running", Some(999999999)).unwrap();
        std::fs::write(paths.root.join(RUNTIME_LOCK_FILE), child.id().to_string()).unwrap();

        assert_eq!(status_runtime().unwrap(), "running");
        assert_eq!(
            vibemud_db::session_info(&conn).unwrap().runtime_pid,
            Some(child.id() as i64)
        );

        stop_runtime().unwrap();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn stop_runtime_does_not_terminate_unrelated_lock_pid() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (paths, conn) = vibemud_db::open_app().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        vibemud_db::set_session_status(&conn, "running", Some(child.id())).unwrap();
        std::fs::write(paths.root.join(RUNTIME_LOCK_FILE), child.id().to_string()).unwrap();

        assert_eq!(status_runtime().unwrap(), "stopped");
        stop_runtime().unwrap();
        assert!(
            child.try_wait().unwrap().is_none(),
            "plain sleep process must not be terminated from a stale runtime.lock"
        );
        assert!(
            !paths.root.join(RUNTIME_LOCK_FILE).exists(),
            "stale runtime lock should be removed without killing unrelated pid"
        );

        let _ = child.kill();
        let _ = child.wait();
    }

    #[cfg(unix)]
    #[test]
    fn stop_runtime_does_not_match_runtime_substrings_in_unrelated_commands() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (paths, conn) = vibemud_db::open_app().unwrap();
        let mut child = std::process::Command::new("bash")
            .args(["-c", "sleep 30 # vibemud-runtime-marker"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        vibemud_db::set_session_status(&conn, "running", Some(child.id())).unwrap();
        std::fs::write(paths.root.join(RUNTIME_LOCK_FILE), child.id().to_string()).unwrap();

        stop_runtime().unwrap();
        assert!(
            child.try_wait().unwrap().is_none(),
            "commands merely containing vibemud-runtime must not be killed"
        );
        assert!(!paths.root.join(RUNTIME_LOCK_FILE).exists());

        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn bounded_runtime_processes_hunt() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let payload = vibemud_core::CommandPayload {
            area_id: Some("training-field".to_string()),
            ..Default::default()
        };
        vibemud_db::enqueue_command(
            &conn,
            "test",
            vibemud_core::CommandKind::HuntStart,
            &payload,
        )
        .unwrap();
        run_one_tick(&conn, 0).unwrap();
        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.mode, "auto_hunt");
    }

    #[test]
    fn runtime_clock_advances_every_tick_even_without_game_logs() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let before_state = vibemud_db::latest_state_version(&conn).unwrap();

        assert_eq!(vibemud_db::runtime_clock_tick(&conn).unwrap(), 0);

        run_one_tick_with_interval(&conn, 0, 4_000).unwrap();
        run_one_tick_with_interval(&conn, 1, 4_000).unwrap();

        assert_eq!(vibemud_db::runtime_clock_tick(&conn).unwrap(), 2);
        let snapshot = vibemud_db::load_snapshot(&conn).unwrap();
        assert_eq!(snapshot.clock_tick, 2);
        assert_eq!(
            vibemud_db::latest_state_version(&conn).unwrap(),
            before_state
        );
    }

    #[test]
    fn no_arg_hunt_start_resumes_last_area() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();

        apply_command(
            &conn,
            vibemud_core::CommandKind::HuntStart.as_str(),
            &vibemud_core::CommandPayload {
                area_id: Some("old-mine".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        apply_command(
            &conn,
            vibemud_core::CommandKind::HuntStop.as_str(),
            &vibemud_core::CommandPayload::default(),
        )
        .unwrap();
        apply_command(
            &conn,
            vibemud_core::CommandKind::HuntStart.as_str(),
            &vibemud_core::CommandPayload::default(),
        )
        .unwrap();

        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.mode, "auto_hunt");
        assert_eq!(player.current_area_id.as_deref(), Some("old-mine"));
        assert_eq!(player.current_dungeon_id, None);
    }

    #[test]
    fn no_arg_hunt_start_resumes_last_dungeon_without_resetting_progress() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();

        apply_command(
            &conn,
            vibemud_core::CommandKind::DungeonEnter.as_str(),
            &vibemud_core::CommandPayload {
                dungeon_id: Some("crystal-cave".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        vibemud_db::set_setting(&conn, DUNGEON_POINT_KEY, "4").unwrap();
        apply_command(
            &conn,
            vibemud_core::CommandKind::HuntStop.as_str(),
            &vibemud_core::CommandPayload::default(),
        )
        .unwrap();
        apply_command(
            &conn,
            vibemud_core::CommandKind::HuntStart.as_str(),
            &vibemud_core::CommandPayload::default(),
        )
        .unwrap();

        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.mode, "dungeon");
        assert_eq!(player.current_dungeon_id.as_deref(), Some("crystal-cave"));
        assert_eq!(
            vibemud_db::setting_value(&conn, DUNGEON_POINT_KEY)
                .unwrap()
                .as_deref(),
            Some("4")
        );
    }

    #[test]
    fn vibe_fever_multiplier_follows_fresh_activity() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();

        assert_eq!(vibe_fever_multiplier(), 1.0);
        vibemud_db::write_vibe_activity("codex", true).unwrap();
        assert_eq!(vibe_fever_multiplier(), 1.5);
        assert_eq!(boosted_u64(31, vibe_fever_multiplier()), 46);
        assert_eq!(boosted_i64(11, vibe_fever_multiplier()), 16);
        vibemud_db::clear_vibe_activity("codex").unwrap();
        assert_eq!(vibe_fever_multiplier(), 1.0);
    }

    #[test]
    fn resistance_reduces_boss_skill_damage_only() {
        let low = vibemud_core::PlayerState {
            evasion: 0,
            ..Default::default()
        };
        let mut high = low.clone();
        high.evasion = 100;
        let normal_low = staged_monster_damage(&low, 20, 42, 2, false);
        let normal_high = staged_monster_damage(&high, 20, 42, 2, false);
        let boss_low = staged_monster_damage(&low, 20, 42, 2, true);
        let boss_high = staged_monster_damage(&high, 20, 42, 2, true);
        assert_eq!(normal_low, normal_high);
        assert!(boss_high < boss_low);
    }

    #[test]
    fn auto_hunt_waits_for_speed_based_encounter_cadence() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let payload = vibemud_core::CommandPayload {
            area_id: Some("training-field".to_string()),
            ..Default::default()
        };
        vibemud_db::enqueue_command(
            &conn,
            "test",
            vibemud_core::CommandKind::HuntStart,
            &payload,
        )
        .unwrap();
        run_one_tick_with_interval(&conn, 0, 4_000).unwrap();
        let logs = vibemud_db::recent_log_entries(&conn, 5).unwrap();
        assert!(!logs.iter().any(|entry| entry.message.contains("scouting")));
        assert!(!logs.iter().any(|entry| entry.message.contains("appeared")));
        for tick in 1..5 {
            run_one_tick_with_interval(&conn, tick, 4_000).unwrap();
        }
        let logs = vibemud_db::recent_log_entries(&conn, 10).unwrap();
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("No monster found")));
    }

    #[test]
    fn dungeon_waits_for_speed_based_encounter_cadence() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let payload = vibemud_core::CommandPayload {
            dungeon_id: Some("goblin-den".to_string()),
            ..Default::default()
        };
        vibemud_db::enqueue_command(
            &conn,
            "test",
            vibemud_core::CommandKind::DungeonEnter,
            &payload,
        )
        .unwrap();
        run_one_tick_with_interval(&conn, 0, 4_000).unwrap();
        let logs = vibemud_db::recent_log_entries(&conn, 8).unwrap();
        assert!(!logs.iter().any(|entry| entry.message.contains("scouting")));
        assert!(!logs.iter().any(|entry| entry.message.contains("appeared")));

        for tick in 1..5 {
            run_one_tick_with_interval(&conn, tick, 4_000).unwrap();
        }
        let logs = vibemud_db::recent_log_entries(&conn, 15).unwrap();
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("had no monster")));
    }

    #[test]
    fn dungeon_boss_appears_after_five_normal_monster_defeats() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.mode = "dungeon".to_string();
        player.current_dungeon_id = Some("goblin-den".to_string());
        player.current_area_id = Some("forest-edge".to_string());
        player.hp = player.max_hp;
        player.attack = 999;
        player.accuracy = 999;
        vibemud_db::upsert_player(&conn, &player).unwrap();
        vibemud_db::set_setting(&conn, DUNGEON_POINT_KEY, "5").unwrap();
        vibemud_db::set_setting(
            &conn,
            DUNGEON_NORMAL_KILL_COUNT_KEY,
            &DUNGEON_BOSS_KILLS_REQUIRED.to_string(),
        )
        .unwrap();
        vibemud_db::set_setting(&conn, DUNGEON_SCOUT_STEP_KEY, "4").unwrap();

        run_one_tick_with_interval(&conn, 0, 4_000).unwrap();

        let logs = vibemud_db::recent_log_entries(&conn, 12).unwrap();
        assert!(logs
            .iter()
            .any(|entry| entry.message.contains("Boss Goblin appeared.")));
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("Boss Goblin defeated.")));

        for tick in 1..=64 {
            run_one_tick_with_interval(&conn, tick as u64, 4_000).unwrap();
            if vibemud_db::recent_log_entries(&conn, 30)
                .unwrap()
                .iter()
                .any(|entry| entry.message.contains("Boss Goblin defeated."))
            {
                break;
            }
        }
        let logs = vibemud_db::recent_log_entries(&conn, 20).unwrap();
        assert!(logs
            .iter()
            .any(|entry| entry.message.contains("Boss Goblin defeated.")));
        assert!(logs.iter().any(|entry| entry
            .message
            .contains("Dungeon goblin-den cleared. Restarting dungeon run.")));
        let player = vibemud_db::load_player(&conn).unwrap();
        let earned = vibemud_db::mod_bridge::adventure_totals(&conn).unwrap();
        assert_eq!(earned.kills, 1);
        assert!(earned.xp > 0);
        assert!(earned.gold > 0);
        assert_eq!(earned.items, 1);
        assert_eq!(player.mode, "dungeon");
        assert_eq!(player.current_dungeon_id, Some("goblin-den".to_string()));
        assert_eq!(dungeon_normal_kills(&conn).unwrap(), 0);
        assert_eq!(dungeon_progress_point(&conn).unwrap(), 1);
        let combat: (i64, String, String) = conn
            .query_row(
                "SELECT in_combat, encounter_id, monster_group_json FROM combat_state WHERE id = 'main'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(combat.0, 1);
        assert_eq!(combat.1, "goblin-den-point-1");
        let payload: serde_json::Value = serde_json::from_str(&combat.2).unwrap();
        assert_eq!(payload["kind"], "dungeon_normal");
        assert_eq!(payload["metadata"]["encounter_point"], 1);
        assert!(logs
            .iter()
            .any(|entry| entry.message.contains("Dungeon encounter point 1/10.")));
    }

    #[test]
    fn combat_does_not_force_finish_at_round_limit() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.attack = 1;
        player.accuracy = 999;
        player.max_hp = 999;
        player.hp = 999;
        player.defense = 999;
        vibemud_db::upsert_player(&conn, &player).unwrap();

        start_staged_combat(
            &conn,
            StagedCombatStart {
                encounter_id: "test-slow-fight",
                seed: 42,
                monster_name: "Training Dummy",
                difficulty_bonus: 0,
                combat_kind: "field",
                metadata: None,
                player_level: player.level,
            },
        )
        .unwrap();
        let monster_max_hp: i32 = conn
            .query_row(
                "SELECT monster_group_json FROM combat_state WHERE id = 'main'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map(|json| {
                let payload: serde_json::Value = serde_json::from_str(&json).unwrap();
                payload["monster_max_hp"].as_i64().unwrap() as i32
            })
            .unwrap();

        for tick in 1..=STAGED_COMBAT_ROUNDS {
            run_one_tick_with_interval(&conn, tick as u64, 4_000).unwrap();
        }

        let (in_combat, payload): (i64, String) = conn
            .query_row(
                "SELECT in_combat, monster_group_json FROM combat_state WHERE id = 'main'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let monster_hp = payload["monster_hp"].as_i64().unwrap() as i32;
        assert_eq!(in_combat, 1);
        assert!(monster_hp > 0, "combat should not be time-forced to finish");
        assert!(
            monster_hp < monster_max_hp,
            "monster HP should still decrease from real attacks"
        );
        let logs = vibemud_db::recent_log_entries(&conn, 20).unwrap();
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("Training Dummy defeated.")));
    }

    #[test]
    fn combat_ping_pong_persists_monster_and_player_hp_loss() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.attack = 1;
        player.accuracy = 999;
        player.max_hp = 200;
        player.hp = 200;
        player.regen = 50;
        vibemud_db::upsert_player(&conn, &player).unwrap();

        start_staged_combat(
            &conn,
            StagedCombatStart {
                encounter_id: "test-ping-pong",
                seed: 7,
                monster_name: "Training Imp",
                difficulty_bonus: 0,
                combat_kind: "field",
                metadata: None,
                player_level: player.level,
            },
        )
        .unwrap();

        run_one_tick_with_interval(&conn, 1, 4_000).unwrap();
        let after_hero: serde_json::Value = conn
            .query_row(
                "SELECT monster_group_json FROM combat_state WHERE id = 'main'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map(|json| serde_json::from_str(&json).unwrap())
            .unwrap();
        assert!(
            after_hero["monster_hp"].as_i64().unwrap()
                < after_hero["monster_max_hp"].as_i64().unwrap()
        );

        run_one_tick_with_interval(&conn, 2, 4_000).unwrap();
        let player_after = vibemud_db::load_player(&conn).unwrap();
        assert!(
            player_after.hp < player_after.max_hp,
            "monster turn should persist real player HP loss without hidden combat regen"
        );
        let turn_index: i64 = conn
            .query_row(
                "SELECT turn_index FROM combat_state WHERE id = 'main'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(turn_index, 2);
    }

    #[test]
    fn high_hp_monster_survives_two_partial_hits() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.attack = 80;
        player.accuracy = 999;
        player.max_hp = 999;
        player.hp = 999;
        player.defense = 999;
        vibemud_db::upsert_player(&conn, &player).unwrap();

        let combat = serde_json::json!({
            "kind": "dungeon_boss",
            "monster_name": "Regression Ogre",
            "difficulty_bonus": 0,
            "seed": 123,
            "round": 0,
            "turn": 0,
            "monster_hp": 400,
            "monster_max_hp": 400,
            "metadata": {},
        });
        conn.execute(
            "UPDATE combat_state SET in_combat = 1, encounter_id = 'test-high-hp', monster_group_json = ?1, encounter_seed = 123, turn_index = 0, updated_at = ?2 WHERE id = 'main'",
            params![combat.to_string(), vibemud_db::now()],
        )
        .unwrap();

        run_one_tick_with_interval(&conn, 1, 4_000).unwrap();
        run_one_tick_with_interval(&conn, 2, 4_000).unwrap();
        run_one_tick_with_interval(&conn, 3, 4_000).unwrap();

        let (in_combat, payload): (i64, String) = conn
            .query_row(
                "SELECT in_combat, monster_group_json FROM combat_state WHERE id = 'main'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let monster_hp = payload["monster_hp"].as_i64().unwrap() as i32;
        let monster_max_hp = payload["monster_max_hp"].as_i64().unwrap() as i32;
        assert_eq!(in_combat, 1);
        assert_eq!(monster_max_hp, 400);
        assert!(
            (1..400).contains(&monster_hp),
            "monster should stay alive after two partial hits, got {monster_hp}/400"
        );

        let logs = vibemud_db::recent_log_entries(&conn, 20).unwrap();
        let total_hero_damage = logs
            .iter()
            .filter_map(|entry| {
                let rest = entry
                    .message
                    .strip_prefix("Warrior hit Regression Ogre for ")?;
                rest.split_once('.')
                    .and_then(|(damage, _)| damage.parse::<i32>().ok())
            })
            .sum::<i32>();
        assert_eq!(monster_hp, 400 - total_hero_damage);
        assert!(total_hero_damage < 400);
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("Regression Ogre defeated.")));

        let snapshot = vibemud_db::load_snapshot(&conn).unwrap();
        assert_eq!(snapshot.combat.monster_hp, Some(monster_hp));
        assert_eq!(snapshot.combat.monster_max_hp, Some(400));
    }

    #[test]
    fn dungeon_resets_to_entry_if_ten_points_end_before_five_kills() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.mode = "dungeon".to_string();
        player.current_dungeon_id = Some("goblin-den".to_string());
        player.current_area_id = Some("forest-edge".to_string());
        player.hp = player.max_hp;
        vibemud_db::upsert_player(&conn, &player).unwrap();
        vibemud_db::set_setting(&conn, DUNGEON_POINT_KEY, "9").unwrap();
        vibemud_db::set_setting(&conn, DUNGEON_NORMAL_KILL_COUNT_KEY, "4").unwrap();
        vibemud_db::set_setting(&conn, DUNGEON_SCOUT_STEP_KEY, "4").unwrap();

        let mut found_reset_seed = None;
        for tick in 0..128 {
            let seed = latest_seed(&conn).unwrap() ^ tick ^ 10 ^ (4_u64 << 8);
            if !should_trigger_encounter(&area_by_id("forest-edge"), true, seed) {
                found_reset_seed = Some(tick);
                break;
            }
        }
        let tick = found_reset_seed.expect("expected deterministic no-encounter seed");
        run_one_tick_with_interval(&conn, tick, 4_000).unwrap();

        let logs = vibemud_db::recent_log_entries(&conn, 12).unwrap();
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("had no monster")));
        assert!(logs.iter().any(|entry| {
            entry
                .message
                .contains("Dungeon entry reset: only 4/5 normal monsters defeated.")
        }));
        assert_eq!(dungeon_progress_point(&conn).unwrap(), 0);
        assert_eq!(dungeon_normal_kills(&conn).unwrap(), 0);
        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.mode, "dungeon");
        assert_eq!(player.current_dungeon_id.as_deref(), Some("goblin-den"));
    }

    #[test]
    fn passive_regen_restores_hp_every_tick_without_combat() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.hp = player.max_hp - 10;
        player.mp = player.max_mp - 10;
        vibemud_db::upsert_player(&conn, &player).unwrap();

        run_one_tick_with_interval(&conn, 0, 4_000).unwrap();
        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.hp, player.max_hp - 5);
        assert_eq!(player.mp, player.max_mp - 8);

        run_one_tick_with_interval(&conn, 1, 4_000).unwrap();
        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.hp, player.max_hp);
        assert_eq!(player.mp, player.max_mp - 6);
        let logs = vibemud_db::recent_log_entries(&conn, 5).unwrap();
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("Passive regen restored")));
    }

    #[test]
    fn recovery_blocks_actions_until_recovery_time_expires() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.mode = "recovering".to_string();
        player.current_area_id = Some("forest-edge".to_string());
        player.current_dungeon_id = Some("goblin-den".to_string());
        player.hp = 1;
        player.mp = 0;
        vibemud_db::upsert_player(&conn, &player).unwrap();
        let until = (OffsetDateTime::now_utc() + Duration::seconds(60))
            .format(&Rfc3339)
            .unwrap();
        vibemud_db::set_setting(&conn, RECOVERY_UNTIL_KEY, &until).unwrap();
        vibemud_db::enqueue_command(
            &conn,
            "test",
            vibemud_core::CommandKind::HuntStart,
            &vibemud_core::CommandPayload::default(),
        )
        .unwrap();
        run_one_tick_with_interval(&conn, 0, 4_000).unwrap();
        let counts = vibemud_db::command_queue_counts(&conn).unwrap();
        assert_eq!(counts.pending, 1);
        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.current_area_id.as_deref(), Some("town"));
        assert_eq!(player.current_dungeon_id, None);
        assert_eq!(player.hp, 11);
        assert_eq!(player.mp, 4);
        let logs = vibemud_db::recent_log_entries(&conn, 8).unwrap();
        assert!(!logs
            .iter()
            .any(|entry| entry.message.contains("Town recovery restored +10 HP")));

        let past = (OffsetDateTime::now_utc() - Duration::seconds(1))
            .format(&Rfc3339)
            .unwrap();
        vibemud_db::set_setting(&conn, RECOVERY_UNTIL_KEY, &past).unwrap();
        run_one_tick_with_interval(&conn, 1, 4_000).unwrap();
        let counts = vibemud_db::command_queue_counts(&conn).unwrap();
        assert_eq!(counts.pending, 0);
        assert_eq!(counts.done, 1);
        let player = vibemud_db::load_player(&conn).unwrap();
        assert_eq!(player.mode, "auto_hunt");
    }

    #[test]
    fn equipment_drop_goes_to_inventory_and_unequipped_enhancement_applies_on_equip() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let base_attack = player.attack;
        let def = vibemud_db::load_equipment_definition(&conn, "eq-weapon-common-t1")
            .unwrap()
            .unwrap();

        let log = acquire_equipment_drop(&conn, &mut player, &def, 7).unwrap();
        let earned = vibemud_db::mod_bridge::adventure_totals(&conn).unwrap();
        assert_eq!(earned.items, 1);
        assert_eq!(earned.last_item.as_deref(), Some(def.name.as_str()));
        vibemud_db::upsert_player(&conn, &player).unwrap();
        assert!(log.contains("소지품창에 보관"));
        assert_eq!(player.attack, base_attack);
        let stored: (String, Option<String>) = conn
            .query_row(
                "SELECT id, equipped_slot FROM inventory_items WHERE item_id = 'eq-weapon-common-t1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert!(stored.1.is_none());

        conn.execute(
            "UPDATE equipment_enhancement_rules SET success_rate = 1.0 WHERE enhancement_level = 0",
            [],
        )
        .unwrap();
        player.gold = 1_000;
        let message = enhance_item(&conn, &stored.0, &mut player).unwrap();
        assert!(message.contains("강화성공"));
        assert_eq!(player.attack, base_attack);

        let equip_message = equip_item(&conn, &stored.0, None, &mut player).unwrap();
        assert!(equip_message.contains("Equipped"));
        assert!(player.attack > base_attack);
        let level: i64 = conn
            .query_row(
                "SELECT enhancement_level FROM inventory_items WHERE equipped_slot = 'weapon'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(level, 1);
    }

    #[test]
    fn equipment_drop_uses_encounter_monster_balance_row() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        conn.execute("DELETE FROM monster_equipment_drops", [])
            .unwrap();
        conn.execute(
            "INSERT INTO monster_equipment_drops(monster_id, monster_grade, rarity, drop_chance, weight)
             VALUES ('crystal-gazer', 'elite', '희귀', 1.0, 100)",
            [],
        )
        .unwrap();

        let training_drop =
            maybe_drop_equipment(&conn, &mut player, "training-scarab", "normal", 11).unwrap();
        assert!(
            training_drop.is_none(),
            "drop logic must not fall back to training-scarab rows for every encounter"
        );

        let log = maybe_drop_equipment(&conn, &mut player, "crystal-gazer", "elite", 11)
            .unwrap()
            .unwrap();
        assert!(log.contains("[희귀"));
        let rare_inventory_rows: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inventory_items WHERE rarity = '희귀'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rare_inventory_rows, 1);
    }

    #[test]
    fn monster_and_boss_balance_simulation_locks_initial_drop_progression() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();

        let early_sources =
            progression_sources(&conn, &["training-field", "forest-edge"], &["goblin-den"]);
        assert!(!early_sources.is_empty());

        let early_high_rarity_drops = early_sources
            .iter()
            .flat_map(|(monster_id, monster_grade)| {
                (0..2_000).filter_map(|seed| {
                    simulated_drop_rarity(&conn, monster_id, monster_grade, seed)
                })
            })
            .filter(|rarity| rarity == "영웅" || rarity == "전설")
            .count();
        assert_eq!(
            early_high_rarity_drops, 0,
            "initial progression simulation must not directly grant heroic+ equipment"
        );

        let next_sources = progression_sources(&conn, &["old-mine"], &["crystal-cave"]);
        assert!(!next_sources.is_empty());

        let trials = next_sources.len() * 5_000;
        let rare_drops = next_sources
            .iter()
            .flat_map(|(monster_id, monster_grade)| {
                (0..5_000).filter_map(|seed| {
                    simulated_drop_rarity(&conn, monster_id, monster_grade, seed)
                })
            })
            .filter(|rarity| rarity == "희귀")
            .count();
        let rare_rate = rare_drops as f64 / trials as f64;
        assert!(
            (0.015..=0.030).contains(&rare_rate),
            "next region simulation should expose rare gear without flooding progression, got {rare_rate:.4}"
        );

        let tier3_rare_items: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM equipment_items WHERE rarity = '희귀' AND tier = 3",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            tier3_rare_items >= 8,
            "stage top-tier rare set should be available across equipment slots"
        );
        assert_eq!(dungeon_reward("crystal-cave").3, "Rare");

        let mid_sources = progression_sources(
            &conn,
            &["misty-swamp", "fallen-fortress", "obsidian-coast"],
            &["lich-tomb", "cyclops-forge"],
        );
        assert!(!mid_sources.is_empty());
        let mid_trials = mid_sources.len() * 5_000;
        let mid_epic_drops = simulated_rarity_count(&conn, &mid_sources, 5_000, "영웅");
        let mid_epic_rate = mid_epic_drops as f64 / mid_trials as f64;
        assert!(
            (0.006..=0.025).contains(&mid_epic_rate),
            "mid progression should introduce epic gear without flooding it, got {mid_epic_rate:.4}"
        );
        assert_eq!(
            simulated_rarity_count(&conn, &mid_sources, 5_000, "전설"),
            0,
            "mid progression should not grant legendary equipment before final areas"
        );

        let titan_sources =
            progression_sources(&conn, &["titan-steppe", "oracle-ruins"], &["medusa-temple"]);
        assert!(!titan_sources.is_empty());
        let titan_trials = titan_sources.len() * 5_000;
        let titan_epic_rate = simulated_rarity_count(&conn, &titan_sources, 5_000, "영웅") as f64
            / titan_trials as f64;
        assert!(
            (0.015..=0.045).contains(&titan_epic_rate),
            "titan progression should make epic gear stable before legendary, got {titan_epic_rate:.4}"
        );
        assert_eq!(
            simulated_rarity_count(&conn, &titan_sources, 5_000, "전설"),
            0,
            "legendary equipment should remain locked until olympus/titan-vault"
        );

        let final_sources =
            progression_sources(&conn, &["styx-marsh", "olympus-gate"], &["titan-vault"]);
        assert!(!final_sources.is_empty());
        let final_trials = final_sources.len() * 5_000;
        let final_legendary_rate = simulated_rarity_count(&conn, &final_sources, 5_000, "전설")
            as f64
            / final_trials as f64;
        assert!(
            (0.006..=0.018).contains(&final_legendary_rate),
            "final progression should make legendary gear visible but rare, got {final_legendary_rate:.4}"
        );
    }

    fn progression_sources(
        conn: &rusqlite::Connection,
        area_ids: &[&str],
        dungeon_ids: &[&str],
    ) -> Vec<(String, String)> {
        let area_list = sql_literal_list(area_ids);
        let dungeon_list = sql_literal_list(dungeon_ids);
        let sql = format!(
            "SELECT DISTINCT m.id, m.monster_grade
             FROM monsters m
             WHERE m.id IN (
               SELECT monster_id FROM area_monsters WHERE area_id IN ({area_list})
               UNION
               SELECT monster_id FROM dungeon_monsters WHERE dungeon_id IN ({dungeon_list})
             )
             ORDER BY m.id"
        );
        conn.prepare(&sql)
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn sql_literal_list(values: &[&str]) -> String {
        assert!(!values.is_empty());
        values
            .iter()
            .map(|value| {
                assert!(
                    value
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'),
                    "test SQL literals must stay simple identifiers"
                );
                format!("'{value}'")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn simulated_rarity_count(
        conn: &rusqlite::Connection,
        sources: &[(String, String)],
        seeds: u64,
        target_rarity: &str,
    ) -> usize {
        sources
            .iter()
            .flat_map(|(monster_id, monster_grade)| {
                (0..seeds)
                    .filter_map(|seed| simulated_drop_rarity(conn, monster_id, monster_grade, seed))
            })
            .filter(|rarity| rarity == target_rarity)
            .count()
    }

    fn simulated_drop_rarity(
        conn: &rusqlite::Connection,
        monster_id: &str,
        monster_grade: &str,
        seed: u64,
    ) -> Option<String> {
        let mut stmt = conn
            .prepare(
                "SELECT rarity, drop_chance FROM monster_equipment_drops
                 WHERE monster_id = ?1 AND monster_grade = ?2
                 ORDER BY drop_chance ASC",
            )
            .unwrap();
        let rows: Vec<(String, f64)> = stmt
            .query_map(params![monster_id, monster_grade], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        for (rarity, chance) in rows {
            let roll = deterministic_roll01(&format!("drop:{monster_id}:{rarity}:{seed}"));
            if roll <= chance {
                return Some(rarity);
            }
        }
        None
    }

    #[test]
    fn legacy_dungeon_reward_equipment_can_be_enhanced_from_inventory() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        player.gold = 1_000;
        add_item(
            &conn,
            "lich-amulet",
            "Hecate Amulet",
            "trinket",
            "Epic",
            Some(100),
            Some(100),
        )
        .unwrap();
        conn.execute(
            "UPDATE equipment_enhancement_rules SET success_rate = 1.0 WHERE enhancement_level = 0",
            [],
        )
        .unwrap();
        let instance_id: String = conn
            .query_row(
                "SELECT id FROM inventory_items WHERE item_id = 'lich-amulet'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let message = enhance_item(&conn, &instance_id, &mut player).unwrap();

        assert!(message.contains("강화성공"));
        let level: i64 = conn
            .query_row(
                "SELECT enhancement_level FROM inventory_items WHERE id = ?1",
                [instance_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(level, 1);
        let inventory = vibemud_db::load_inventory(&conn).unwrap();
        let item = inventory
            .iter()
            .find(|item| item.item_id == "lich-amulet")
            .unwrap();
        assert_eq!(item.rarity, "영웅");
        assert!(item.power_score.is_some());
    }

    #[test]
    fn equipped_items_must_be_unequipped_before_enhancement() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let def = vibemud_db::load_equipment_definition(&conn, "eq-weapon-rare-t1")
            .unwrap()
            .unwrap();
        let _ = acquire_equipment_drop(&conn, &mut player, &def, 17).unwrap();
        let instance_id: String = conn
            .query_row(
                "SELECT id FROM inventory_items WHERE item_id = 'eq-weapon-rare-t1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        equip_item(&conn, &instance_id, None, &mut player).unwrap();
        player.gold = 1_000;

        let err = enhance_item(&conn, "무기", &mut player).unwrap_err();
        assert!(err
            .to_string()
            .contains("equipped items cannot be enhanced"));

        let message = unequip_item(&conn, "무기", &mut player).unwrap();
        assert!(message.contains("Unequipped"));
        let equipped_slot: Option<String> = conn
            .query_row(
                "SELECT equipped_slot FROM inventory_items WHERE id = ?1",
                [&instance_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(equipped_slot.is_none());
    }

    #[test]
    fn subweapon_applies_half_of_catalog_stats() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let base_attack = player.attack;
        let def = vibemud_db::load_equipment_definition(&conn, "eq-subweapon-common-t1")
            .unwrap()
            .unwrap();
        assert_eq!(def.stat1_value, 6);

        acquire_equipment_drop(&conn, &mut player, &def, 13).unwrap();
        let instance_id: String = conn
            .query_row(
                "SELECT id FROM inventory_items WHERE item_id = 'eq-subweapon-common-t1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        equip_item(&conn, &instance_id, None, &mut player).unwrap();

        assert_eq!(player.attack - base_attack, 3);
        let item = vibemud_db::load_inventory(&conn)
            .unwrap()
            .into_iter()
            .find(|item| item.item_id == "eq-subweapon-common-t1")
            .unwrap();
        assert_eq!(item.equipped_slot.as_deref(), Some("subweapon"));
        assert_eq!(item.stat1_value, Some(3));
    }

    #[test]
    fn weapon_can_be_equipped_as_subweapon_at_choice_time() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let base_attack = player.attack;
        let def = vibemud_db::load_equipment_definition(&conn, "eq-weapon-common-t1")
            .unwrap()
            .unwrap();
        acquire_equipment_drop(&conn, &mut player, &def, 31).unwrap();
        let instance_id: String = conn
            .query_row(
                "SELECT id FROM inventory_items WHERE item_id = 'eq-weapon-common-t1'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let message = equip_item(&conn, &instance_id, Some("subweapon"), &mut player).unwrap();

        assert!(message.contains("subweapon"));
        assert_eq!(player.attack - base_attack, 4);
        let item = vibemud_db::load_inventory(&conn)
            .unwrap()
            .into_iter()
            .find(|item| item.item_id == "eq-weapon-common-t1")
            .unwrap();
        assert_eq!(item.equipped_slot.as_deref(), Some("subweapon"));
        assert_eq!(item.stat1_value, Some(4));
    }

    #[test]
    fn legendary_pet_third_stat_grants_reward_bonus_multiplier() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let def = vibemud_db::load_equipment_definition(&conn, "eq-pet-legendary-t2")
            .unwrap()
            .unwrap();
        assert_eq!(def.stat3_type.as_deref(), Some("gold_bonus"));

        acquire_equipment_drop(&conn, &mut player, &def, 21).unwrap();
        let instance_id: String = conn
            .query_row(
                "SELECT id FROM inventory_items WHERE item_id = 'eq-pet-legendary-t2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        equip_item(&conn, &instance_id, None, &mut player).unwrap();

        assert_eq!(
            equipped_reward_bonus_multiplier(&conn, "xp_bonus").unwrap(),
            1.0
        );
        assert!(equipped_reward_bonus_multiplier(&conn, "gold_bonus").unwrap() > 1.0);
    }

    #[test]
    fn equipment_drop_is_discarded_when_inventory_is_full() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let def = vibemud_db::load_equipment_definition(&conn, "eq-weapon-common-t1")
            .unwrap()
            .unwrap();

        for index in 0..INVENTORY_CAPACITY {
            conn.execute(
                "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, durability, max_durability, equipped_slot, enhancement_level, acquired_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, 100, 100, NULL, 0, ?6)",
                params![
                    format!("filled-{index}"),
                    def.item_id,
                    def.slot,
                    def.name,
                    def.rarity,
                    vibemud_db::now()
                ],
            )
            .unwrap();
        }

        let log = acquire_equipment_drop(&conn, &mut player, &def, 99).unwrap();
        assert!(log.contains("소지품창이 가득"));
        assert_eq!(
            vibemud_db::mod_bridge::adventure_totals(&conn)
                .unwrap()
                .items,
            0
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inventory_items WHERE equipped_slot IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, INVENTORY_CAPACITY);
    }

    #[test]
    fn higher_rarity_drop_evicts_lowest_rarity_spare_when_inventory_is_full() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let common = vibemud_db::load_equipment_definition(&conn, "eq-weapon-common-t1")
            .unwrap()
            .unwrap();
        let rare = vibemud_db::load_equipment_definition(&conn, "eq-weapon-rare-t1")
            .unwrap()
            .unwrap();

        for index in 0..INVENTORY_CAPACITY {
            conn.execute(
                "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, durability, max_durability, equipped_slot, enhancement_level, acquired_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, 100, 100, NULL, 0, ?6)",
                params![
                    format!("filled-{index}"),
                    common.item_id,
                    common.slot,
                    common.name,
                    common.rarity,
                    vibemud_db::now()
                ],
            )
            .unwrap();
        }

        let log = acquire_equipment_drop(&conn, &mut player, &rare, 42).unwrap();
        assert!(
            log.contains("장비 획득"),
            "rare drop must be stored, got {log}"
        );
        assert!(
            log.contains("정리"),
            "log should mention the evicted item, got {log}"
        );
        let (total, rare_rows): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), SUM(CASE WHEN rarity = '희귀' THEN 1 ELSE 0 END)
                 FROM inventory_items WHERE equipped_slot IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(total, INVENTORY_CAPACITY, "capacity must not be exceeded");
        assert_eq!(rare_rows, 1);

        // A drop that is not strictly higher rarity than the worst spare must
        // still be discarded.
        let log = acquire_equipment_drop(&conn, &mut player, &common, 43).unwrap();
        assert!(
            log.contains("소지품창이 가득"),
            "equal rarity must not evict, got {log}"
        );
    }

    #[test]
    fn boss_reward_evicts_lowest_rarity_spare_when_inventory_is_full() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let common = vibemud_db::load_equipment_definition(&conn, "eq-weapon-common-t1")
            .unwrap()
            .unwrap();

        for index in 0..INVENTORY_CAPACITY {
            conn.execute(
                "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, durability, max_durability, equipped_slot, enhancement_level, acquired_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 1, 100, 100, NULL, 0, ?6)",
                params![
                    format!("filled-{index}"),
                    common.item_id,
                    common.slot,
                    common.name,
                    common.rarity,
                    vibemud_db::now()
                ],
            )
            .unwrap();
        }

        let combat = serde_json::json!({"metadata": {"dungeon_id": "goblin-den"}});
        let mut logs = Vec::new();
        finish_dungeon_boss_combat(&conn, &mut player, &combat, &mut logs).unwrap();

        assert!(
            logs.iter()
                .any(|line| line.contains("Boss reward: Hector Axe")),
            "boss reward must be stored, got {logs:?}"
        );
        assert!(
            logs.iter().any(|line| line.contains("정리")),
            "log should mention the evicted item, got {logs:?}"
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inventory_items WHERE equipped_slot IS NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, INVENTORY_CAPACITY, "capacity must not be exceeded");
    }

    #[test]
    fn consumable_stack_does_not_grow_past_inventory_capacity() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();

        for _ in 0..INVENTORY_CAPACITY {
            assert!(add_item(
                &conn,
                "potion-small",
                "Asclepius Potion",
                "consumable",
                "Common",
                None,
                None,
            )
            .unwrap());
        }

        assert!(!add_item(
            &conn,
            "potion-small",
            "Asclepius Potion",
            "consumable",
            "Common",
            None,
            None,
        )
        .unwrap());
        let (rows, quantity): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(quantity), 0) FROM inventory_items WHERE equipped_slot IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(quantity, INVENTORY_CAPACITY);
    }

    #[test]
    fn shop_buy_does_not_spend_gold_or_add_item_when_inventory_is_full() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let starting_gold = player.gold;

        for index in 0..INVENTORY_CAPACITY {
            conn.execute(
                "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, acquired_at)
                 VALUES (?1, ?2, 'material', 'Filled Slot', 'Common', 1, ?3)",
                params![format!("filled-{index}"), format!("filled-{index}"), vibemud_db::now()],
            )
            .unwrap();
        }

        let message = buy_item(&conn, "potion-small", &mut player).unwrap();

        assert!(message.contains("구매가 취소"));
        assert_eq!(player.gold, starting_gold);
        assert_eq!(
            unequipped_inventory_count(&conn).unwrap(),
            INVENTORY_CAPACITY
        );
        let potion_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM inventory_items WHERE item_id = 'potion-small'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(potion_count, 0);
    }

    #[test]
    fn shop_and_party_commands_have_effects() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let buy = vibemud_core::CommandPayload {
            item_id: Some("potion-small".to_string()),
            ..Default::default()
        };
        vibemud_db::enqueue_command(&conn, "test", vibemud_core::CommandKind::ShopBuy, &buy)
            .unwrap();
        vibemud_db::enqueue_command(
            &conn,
            "test",
            vibemud_core::CommandKind::PartyRecruit,
            &Default::default(),
        )
        .unwrap();
        run_one_tick(&conn, 0).unwrap();
        assert!(!vibemud_db::load_inventory(&conn).unwrap().is_empty());
        let before_sell = vibemud_db::load_player(&conn).unwrap().gold;
        vibemud_db::enqueue_command(
            &conn,
            "test",
            vibemud_core::CommandKind::SellCommon,
            &Default::default(),
        )
        .unwrap();
        run_one_tick(&conn, 1).unwrap();
        assert!(vibemud_db::load_inventory(&conn).unwrap().is_empty());
        assert!(vibemud_db::load_player(&conn).unwrap().gold > before_sell);
        let unlocked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM companions WHERE unlocked = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(unlocked >= 2);
    }

    #[test]
    fn bulk_empty_sells_unlocked_items_and_skips_locked_items() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let before_gold = player.gold;

        for (id, rarity, quantity, equipped_slot, locked) in [
            ("bulk-common", "Common", 2_i64, None, 0_i64),
            ("bulk-rare", "Rare", 1, None, 0),
            ("bulk-epic", "영웅", 1, None, 0),
            ("bulk-locked-legendary", "전설", 1, None, 1),
            ("bulk-equipped-epic", "영웅", 1, Some("trinket"), 0),
        ] {
            conn.execute(
                "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, equipped_slot, locked, acquired_at)
                 VALUES (?1, ?1, 'material', ?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, rarity, quantity, equipped_slot, locked, vibemud_db::now()],
            )
            .unwrap();
        }

        let message = sell_common_items(&conn, &mut player, None).unwrap();

        assert!(message.contains("sold 4 unlocked items"));
        assert!(message.contains("skipped 1 locked"));
        assert_eq!(player.gold, before_gold + 2 * 10 + 75 + 200);
        let remaining: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT id FROM inventory_items ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        assert_eq!(
            remaining,
            vec![
                "bulk-equipped-epic".to_string(),
                "bulk-locked-legendary".to_string()
            ]
        );
    }

    #[test]
    fn bulk_sell_accepts_inventory_selector_action_ids() {
        assert_eq!(parse_bulk_sell_rarity("sell_common").unwrap().rank, 1);
        assert_eq!(parse_bulk_sell_rarity("sell_upto_common").unwrap().rank, 1);
        assert_eq!(
            parse_bulk_sell_rarity("sell_upto_uncommon").unwrap().rank,
            2
        );
        assert_eq!(parse_bulk_sell_rarity("sell_upto_rare").unwrap().rank, 3);
        assert_eq!(parse_bulk_sell_rarity("sell_upto_epic").unwrap().rank, 4);
    }

    #[test]
    fn item_lock_toggle_controls_bulk_empty_only() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();

        conn.execute(
            "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, acquired_at)
             VALUES ('lock-me', 'lock-me', 'material', 'Lock Me', 'Rare', 1, ?1)",
            [vibemud_db::now()],
        )
        .unwrap();
        assert_eq!(
            set_item_lock(&conn, "lock-me", true).unwrap(),
            "Locked Lock Me"
        );
        assert!(vibemud_db::load_inventory(&conn).unwrap()[0].locked);

        let before_gold = player.gold;
        let message = sell_common_items(&conn, &mut player, None).unwrap();
        assert!(message.contains("No unlocked items"));
        assert_eq!(player.gold, before_gold);

        assert_eq!(
            set_item_lock(&conn, "lock-me", false).unwrap(),
            "Unlocked Lock Me"
        );
        let message = sell_common_items(&conn, &mut player, None).unwrap();
        assert!(message.contains("sold 1 unlocked items"));
        assert_eq!(player.gold, before_gold + 75);
        assert!(vibemud_db::load_inventory(&conn).unwrap().is_empty());
    }

    #[test]
    fn single_sell_uses_fixed_rarity_price_independent_of_slot_power() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut player = vibemud_db::load_player(&conn).unwrap();
        let before_gold = player.gold;

        for item_id in ["eq-weapon-common-t1", "eq-armor_top-common-t1"] {
            add_item(
                &conn,
                item_id,
                item_id,
                "equipment",
                "Common",
                Some(100),
                Some(100),
            )
            .unwrap();
        }

        let first = sell_item(&conn, "eq-weapon-common-t1", &mut player).unwrap();
        let second = sell_item(&conn, "eq-armor_top-common-t1", &mut player).unwrap();

        assert!(first.contains("for 10 gold"));
        assert!(second.contains("for 10 gold"));
        assert_eq!(player.gold, before_gold + 20);
    }

    #[test]
    fn repeated_fast_sell_commands_skip_missing_item_without_queue_failures() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let before_gold = vibemud_db::load_player(&conn).unwrap().gold;

        conn.execute(
            "INSERT INTO inventory_items(id, item_id, item_type, name, rarity, quantity, acquired_at)
             VALUES ('rapid-sell', 'rapid-sell', 'material', 'Rapid Sell', 'Common', 1, ?1)",
            [vibemud_db::now()],
        )
        .unwrap();

        let payload = vibemud_core::CommandPayload {
            item_id: Some("rapid-sell".to_string()),
            ..Default::default()
        };
        for _ in 0..5 {
            vibemud_db::enqueue_command(
                &conn,
                "test",
                vibemud_core::CommandKind::ShopSell,
                &payload,
            )
            .unwrap();
        }

        run_one_tick(&conn, 0).unwrap();

        let counts = vibemud_db::command_queue_counts(&conn).unwrap();
        assert_eq!(counts.failed, 0);
        assert_eq!(counts.done, 5);
        assert!(vibemud_db::load_inventory(&conn).unwrap().is_empty());
        assert_eq!(
            vibemud_db::load_player(&conn).unwrap().gold,
            before_gold + 10
        );
        let skipped = vibemud_db::recent_log_entries(&conn, 10)
            .unwrap()
            .into_iter()
            .filter(|entry| entry.message.starts_with("Sale skipped:"))
            .count();
        assert_eq!(skipped, 4);
    }

    #[test]
    fn equipment_rewards_create_unique_inventory_rows_instead_of_stacking() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();

        add_item(
            &conn,
            "crystal-blade",
            "Perseus Blade",
            "weapon",
            "Rare",
            Some(100),
            Some(100),
        )
        .unwrap();
        add_item(
            &conn,
            "crystal-blade",
            "Perseus Blade",
            "weapon",
            "Rare",
            Some(100),
            Some(100),
        )
        .unwrap();

        let (rows, total_quantity): (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*), SUM(quantity) FROM inventory_items WHERE item_id = 'crystal-blade'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(rows, 2);
        assert_eq!(total_quantity, 2);
    }
}
