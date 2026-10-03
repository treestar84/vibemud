//! Persistence shared by native mod clients. No project or conversation input.
use crate::{setting_value, AppPaths};
use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};

/// Monotonic combat earnings within one save. Purchases, sales and XP spent on
/// levelling cannot distort a coding expedition's result. Written in the same
/// transaction as the actual reward, and cleared with the rest of game state.
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AdventureTotals {
    pub kills: u64,
    pub xp: u64,
    pub gold: u64,
    pub items: u64,
    pub last_item: Option<String>,
}

pub fn adventure_totals(conn: &Connection) -> Result<AdventureTotals> {
    Ok(setting_value(conn, "game.adventure_totals")?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_default())
}

pub fn record_adventure_rewards(
    conn: &Connection,
    kills: u64,
    xp: u64,
    gold: u64,
    item: Option<&str>,
) -> Result<()> {
    let mut totals = adventure_totals(conn)?;
    totals.kills = totals.kills.saturating_add(kills);
    totals.xp = totals.xp.saturating_add(xp);
    totals.gold = totals.gold.saturating_add(gold);
    if let Some(name) = item {
        totals.items = totals.items.saturating_add(1);
        totals.last_item = Some(name.to_string());
    }
    crate::set_setting(
        conn,
        "game.adventure_totals",
        &serde_json::to_string(&totals)?,
    )
}
use vibemud_core::{CommandClass, CommandKind, CommandPayload};

pub const PROTOCOL_VERSION: u32 = 1;
pub const LEASE_SECONDS: i64 = 30;

/// Copy a consistent SQLite snapshot (including WAL), never overwrite either save.
pub fn copy_save(source: &AppPaths, destination: &AppPaths) -> Result<()> {
    let conn = Connection::open_with_flags(&source.db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let config = std::fs::read(&source.config)?;
    let _: crate::AppConfig = toml::from_str(std::str::from_utf8(&config)?)?;
    destination.ensure_dirs()?;
    // Reserve the destination atomically. VACUUM INTO accepts an empty file.
    let reservation = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&destination.db)
        .context("destination already exists; no save was overwritten")?;
    drop(reservation);
    let mut wrote_config = false;
    let copied = (|| -> Result<()> {
        conn.execute(
            "VACUUM INTO ?1",
            [destination.db.to_string_lossy().as_ref()],
        )?;
        let check = Connection::open(&destination.db)?;
        let integrity: String = check.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        anyhow::ensure!(integrity == "ok", "copied save failed integrity check");
        crate::load_player(&check)?;
        // Carry only the time-based legacy reward, never an active heartbeat.
        let reward = std::fs::read(&source.vibe_activity)
            .ok()
            .and_then(|raw| serde_json::from_slice::<crate::VibeActivity>(&raw).ok())
            .and_then(|activity| activity.reward_until)
            .and_then(|value| OffsetDateTime::parse(&value, &Rfc3339).ok());
        if let Some(reward) = reward {
            let stored = setting_value(&check, "mod.reward_until")?
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            crate::set_setting(
                &check,
                "mod.reward_until",
                &stored.max(reward.unix_timestamp()).to_string(),
            )?;
        }
        // Config is also reserved; an existing user's configuration is preserved.
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination.config)?;
        wrote_config = true;
        file.write_all(&config)?;
        Ok(())
    })();
    if copied.is_err() {
        let _ = std::fs::remove_file(&destination.db);
        if wrote_config {
            let _ = std::fs::remove_file(&destination.config);
        }
    }
    copied
}

pub fn open_readonly() -> Result<(AppPaths, Connection)> {
    let paths = AppPaths::discover()?;
    let conn = Connection::open_with_flags(&paths.db, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .context("VibeMUD is not initialized; run /mud start")?;
    conn.busy_timeout(std::time::Duration::from_millis(1000))?;
    Ok((paths, conn))
}

pub fn open_existing() -> Result<(AppPaths, Connection)> {
    let paths = AppPaths::discover()?;
    let conn = Connection::open_with_flags(&paths.db, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .context("VibeMUD is not initialized; run /mud start")?;
    conn.busy_timeout(std::time::Duration::from_millis(1000))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok((paths, conn))
}

pub fn generation(conn: &Connection) -> Result<String> {
    setting_value(conn, "mod.save_generation")?.context("Bridge upgrade required; run /mud start")
}

pub fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.:".contains(&c))
    {
        bail!("invalid request or session identifier");
    }
    Ok(())
}

/// The caller's request id is durable, unlike a newly generated queue id on retry.
pub fn enqueue_once(
    conn: &Connection,
    save: &str,
    request: &str,
    kind: CommandKind,
    payload: &CommandPayload,
) -> Result<String> {
    validate_id(request)?;
    if kind.class() != CommandClass::Mutation {
        bail!("read-only action cannot be queued");
    }
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    if generation(&tx)? != save {
        bail!("save changed; refresh before sending another action");
    }
    let id = format!("mod:{save}:{request}");
    let json = serde_json::to_string(payload)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM command_queue WHERE id=?1)",
        [&id],
        |r| r.get(0),
    )?;
    if !exists {
        anyhow::ensure!(
            crate::session_status(&tx)? == "running",
            "runtime is stopped; use start first"
        );
    }
    tx.execute("INSERT OR IGNORE INTO command_queue(id,created_at,source,command_type,payload_json,status) VALUES (?1,?2,'claude-mod',?3,?4,'pending')",
        params![id, crate::now(), kind.as_str(), json])?;
    let stored: (String, String) = tx.query_row(
        "SELECT command_type,payload_json FROM command_queue WHERE id=?1",
        [&id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if stored != (kind.as_str().to_string(), json) {
        bail!("request id already belongs to a different action");
    }
    tx.commit()?;
    Ok(id)
}

pub fn renew(conn: &Connection, session: &str, active: bool) -> Result<()> {
    validate_id(session)?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM mod_sessions WHERE expires_at < ?1", [now])?;
    tx.execute("INSERT INTO mod_sessions(id,expires_at,active_until) VALUES (?1,?2,?3) ON CONFLICT(id) DO UPDATE SET expires_at=excluded.expires_at, active_until=excluded.active_until",
        params![session, now + LEASE_SECONDS, if active { now + 15 } else { 0 }])?;
    tx.commit()?;
    Ok(())
}

pub fn detach(conn: &Connection, session: &str) -> Result<()> {
    validate_id(session)?;
    conn.execute("DELETE FROM mod_sessions WHERE id=?1", [session])?;
    Ok(())
}

pub fn cancel_pending(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE command_queue SET status='failed', error_message='Game stopped before the action ran', processed_at=?1 WHERE source='claude-mod' AND status='pending'", [crate::now()])?;
    Ok(())
}

pub fn has_clients(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mod_sessions WHERE expires_at > ?1)",
        [OffsetDateTime::now_utc().unix_timestamp()],
        |r| r.get(0),
    )?)
}

pub fn fever_active(conn: &Connection) -> Result<bool> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let active: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM mod_sessions WHERE active_until > ?1 AND expires_at > ?1)",
        [now],
        |r| r.get(0),
    )?;
    let reward = setting_value(conn, "mod.reward_until")?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    Ok(active || reward > now)
}

/// Reward and quest completion commit together with the queued command.
pub fn grant_reward(conn: &Connection, minutes: i64) -> Result<String> {
    let now = OffsetDateTime::now_utc();
    let stored = setting_value(conn, "mod.reward_until")?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    let legacy = crate::vibe_activity()
        .and_then(|a| a.reward_until)
        .and_then(|v| OffsetDateTime::parse(&v, &Rfc3339).ok())
        .map(|t| t.unix_timestamp())
        .unwrap_or(0);
    let base = OffsetDateTime::from_unix_timestamp(now.unix_timestamp().max(stored).max(legacy))?;
    let until = base + Duration::minutes(minutes.clamp(1, 1440));
    crate::set_setting(
        conn,
        "mod.reward_until",
        &until.unix_timestamp().to_string(),
    )?;
    Ok(until.format(&Rfc3339)?)
}

pub fn result(conn: &Connection, id: &str) -> Result<serde_json::Value> {
    conn.query_row("SELECT status,result_json,error_message FROM command_queue WHERE id=?1", [id], |r| {
        let raw: Option<String> = r.get(1)?;
        Ok(serde_json::json!({"id":id,"status":r.get::<_,String>(0)?,"result":raw.and_then(|s|serde_json::from_str::<serde_json::Value>(&s).ok()),"error":r.get::<_,Option<String>>(2)?}))
    }).optional()?.context("command not found (the save may have been reset)")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        crate::migrate(&c).unwrap();
        crate::seed_initial_state(&c).unwrap();
        c
    }
    #[test]
    fn project_copy_preserves_wal_data_and_refuses_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let source = AppPaths::at(tmp.path().join("project/.vibemud"));
        let destination = AppPaths::at(tmp.path().join("user data"));
        source.ensure_dirs().unwrap();
        let c = Connection::open(&source.db).unwrap();
        c.pragma_update(None, "journal_mode", "WAL").unwrap();
        crate::migrate(&c).unwrap();
        crate::seed_initial_state(&c).unwrap();
        c.execute("UPDATE player_state SET gold=4321", []).unwrap();
        std::fs::write(
            &source.config,
            toml::to_string(&crate::AppConfig::default()).unwrap(),
        )
        .unwrap();
        copy_save(&source, &destination).unwrap();
        let copied = Connection::open(&destination.db).unwrap();
        assert_eq!(crate::load_player(&copied).unwrap().gold, 4321);
        assert_eq!(crate::load_player(&c).unwrap().gold, 4321);
        assert!(copy_save(&source, &destination).is_err());
        assert_eq!(crate::load_player(&copied).unwrap().gold, 4321);
    }

    #[test]
    fn reset_checks_generation_inside_transaction_and_rewards_roll_back() {
        let mut c = db();
        let generation_before = generation(&c).unwrap();
        {
            let tx = c.unchecked_transaction().unwrap();
            grant_reward(&tx, 5).unwrap();
            tx.rollback().unwrap();
        }
        assert!(!fever_active(&c).unwrap());
        crate::reset_game_state_if_generation(&mut c, Some(&generation_before)).unwrap();
        let generation_after = generation(&c).unwrap();
        assert_ne!(generation_after, generation_before);
        assert!(crate::reset_game_state_if_generation(&mut c, Some(&generation_before)).is_err());
        assert_eq!(generation(&c).unwrap(), generation_after);
    }

    #[test]
    fn expedition_counters_follow_reward_transactions_and_reset() {
        let mut c = db();
        {
            let tx = c.unchecked_transaction().unwrap();
            record_adventure_rewards(&tx, 1, 40, 20, Some("Sword")).unwrap();
            tx.rollback().unwrap();
        }
        assert_eq!(adventure_totals(&c).unwrap().kills, 0);
        {
            let tx = c.unchecked_transaction().unwrap();
            record_adventure_rewards(&tx, 1, 40, 20, Some("Sword")).unwrap();
            record_adventure_rewards(&tx, 1, 60, 30, None).unwrap();
            tx.commit().unwrap();
        }
        // Wallet changes and history pruning do not change earned rewards.
        c.execute("UPDATE player_state SET gold=1", []).unwrap();
        crate::prune_history_with_limits(&c, 1, 1).unwrap();
        let totals = adventure_totals(&c).unwrap();
        assert_eq!(
            (totals.kills, totals.xp, totals.gold, totals.items),
            (2, 100, 50, 1)
        );
        assert_eq!(totals.last_item.as_deref(), Some("Sword"));
        crate::reset_game_state(&mut c).unwrap();
        assert_eq!(adventure_totals(&c).unwrap().xp, 0);
    }

    #[test]
    fn retries_and_reset_generation_are_safe() {
        let c = db();
        crate::set_session_status(&c, "running", Some(1)).unwrap();
        let g = generation(&c).unwrap();
        let p = CommandPayload::default();
        let a = enqueue_once(&c, &g, "a", CommandKind::Rest, &p).unwrap();
        assert_eq!(enqueue_once(&c, &g, "a", CommandKind::Rest, &p).unwrap(), a);
        assert!(enqueue_once(&c, &g, "a", CommandKind::Town, &p).is_err());
        assert!(enqueue_once(&c, "old-save", "b", CommandKind::Rest, &p).is_err());
        assert_eq!(crate::command_queue_count(&c).unwrap(), 1);
    }
    #[test]
    fn multiple_clients_and_reward_lifetimes_are_independent() {
        let c = db();
        renew(&c, "a", true).unwrap();
        renew(&c, "b", true).unwrap();
        detach(&c, "a").unwrap();
        assert!(fever_active(&c).unwrap());
        renew(&c, "b", false).unwrap();
        assert!(!fever_active(&c).unwrap());
        assert!(has_clients(&c).unwrap());
        c.execute("UPDATE mod_sessions SET expires_at=0,active_until=0", [])
            .unwrap();
        assert!(!has_clients(&c).unwrap());
        crate::set_setting(
            &c,
            "mod.reward_until",
            &(OffsetDateTime::now_utc().unix_timestamp() + 60).to_string(),
        )
        .unwrap();
        assert!(fever_active(&c).unwrap());
    }
}
