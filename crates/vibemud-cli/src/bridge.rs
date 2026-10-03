//! Shell-free, versioned JSON boundary for the Claude Code pane.
use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use rusqlite::Connection;
use serde_json::{json, Value};
use vibemud_core::{CommandKind, CommandPayload};
use vibemud_db::{mod_bridge as db, AppPaths};

#[derive(Args, Debug)]
pub struct BridgeArgs {
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: BridgeCommand,
}

#[derive(Subcommand, Debug)]
enum BridgeCommand {
    Info,
    Init,
    /// Copy a legacy project save to the user data directory; preserve the original.
    Migrate,
    Snapshot,
    Action {
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        generation: String,
        #[arg(long)]
        kind: String,
        #[arg(long, default_value = "{}")]
        payload_json: String,
    },
    Result {
        #[arg(long)]
        id: String,
    },
    Session {
        #[arg(value_parser=["ensure","renew","detach","stop","release"])]
        operation: String,
        #[arg(long)]
        session_id: String,
        #[arg(long)]
        active: bool,
    },
    Setting {
        #[arg(value_parser=["language","intro_seen","animations"])]
        key: String,
        value: String,
    },
    Reset {
        #[arg(long)]
        generation: String,
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(args: BridgeArgs) -> Result<()> {
    // JSON remains the sole stdout record, including failed operations.
    match execute(args.command) {
        Ok(data) => println!(
            "{}",
            json!({"protocol_version":db::PROTOCOL_VERSION,"ok":true,"data":data})
        ),
        Err(error) => {
            println!(
                "{}",
                json!({"protocol_version":db::PROTOCOL_VERSION,"ok":false,"error":error.to_string()})
            );
            bail!("VibeMUD bridge operation failed");
        }
    }
    Ok(())
}

fn execute(command: BridgeCommand) -> Result<Value> {
    let paths = AppPaths::for_mod()?;
    let legacy = if matches!(&command, BridgeCommand::Info | BridgeCommand::Migrate) {
        AppPaths::legacy_project()?.filter(|p| p.db.exists() && p.root != paths.root)
    } else {
        None
    };
    // Pin every child process to the same user-selected, absolute data directory.
    std::env::set_var("VIBEMUD_HOME", &paths.root);
    match command {
        BridgeCommand::Info => Ok(
            json!({"version":vibemud_core::VERSION,"data_dir":paths.root,"legacy_data_dir":legacy.as_ref().map(|p|&p.root),"can_migrate":legacy.is_some()&&!paths.db.exists(),"capabilities":["snapshot","actions","idempotency","leases"]}),
        ),
        BridgeCommand::Migrate => {
            let old = legacy.context("no legacy project save found")?;
            anyhow::ensure!(
                !paths.db.exists(),
                "destination already has a save; no files were overwritten"
            );
            std::env::set_var("VIBEMUD_HOME", &old.root);
            let stopped = vibemud_runtime::stop_runtime();
            std::env::set_var("VIBEMUD_HOME", &paths.root);
            stopped?;
            db::copy_save(&old, &paths)?;
            Ok(json!({"data_dir":paths.root,"original":old.root}))
        }
        BridgeCommand::Init => {
            let (paths, conn) = vibemud_db::open_app()?;
            vibemud_db::load_daily_quests(&conn)?;
            Ok(json!({"data_dir":paths.root,"generation":db::generation(&conn)?}))
        }
        BridgeCommand::Snapshot => {
            let (paths, conn) = db::open_readonly()?;
            snapshot(&paths, &conn)
        }
        BridgeCommand::Action {
            request_id,
            generation,
            kind,
            payload_json,
        } => {
            if payload_json.len() > 4096 {
                bail!("action payload is too large");
            }
            let mut kind = action_kind(&kind)?;
            let raw: Value = serde_json::from_str(&payload_json)?;
            let allowed = [
                "area_id",
                "dungeon_id",
                "item_id",
                "rarity",
                "companion_id",
                "slot",
                "equip_slot",
                "skill_id",
                "quest_id",
            ];
            if !raw.is_object()
                || raw
                    .as_object()
                    .unwrap()
                    .keys()
                    .any(|k| !allowed.contains(&k.as_str()))
            {
                bail!("unknown action payload field");
            }
            let mut payload: CommandPayload = serde_json::from_value(raw)?;
            let (_, conn) = db::open_existing()?;
            // Preserve the old `a <area-or-dungeon>` / `m <destination>` shortcut.
            if matches!(kind, CommandKind::HuntStart) {
                if let Some(id) = payload.area_id.as_deref() {
                    let is_dungeon: bool = conn.query_row(
                        "SELECT EXISTS(SELECT 1 FROM dungeons WHERE id=?1)",
                        [id],
                        |r| r.get(0),
                    )?;
                    if is_dungeon {
                        kind = CommandKind::DungeonEnter;
                        payload.dungeon_id = payload.area_id.take();
                    }
                }
            }
            validate_action(&conn, &kind, &payload)?;
            // A stopped runtime does not accept work that would surprise the user later.
            let exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM command_queue WHERE id=?1)",
                [format!("mod:{generation}:{request_id}")],
                |r| r.get(0),
            )?;
            if !exists && runtime_status(&conn)? != "running" {
                bail!("runtime is stopped; use start first");
            }
            let id = db::enqueue_once(&conn, &generation, &request_id, kind, &payload)?;
            db::result(&conn, &id)
        }
        BridgeCommand::Result { id } => {
            let (_, conn) = db::open_readonly()?;
            db::result(&conn, &id)
        }
        BridgeCommand::Session {
            operation,
            session_id,
            active,
        } => {
            db::validate_id(&session_id)?;
            if operation == "stop" {
                vibemud_runtime::stop_runtime()?;
                return Ok(json!({"status":"stopped"}));
            }
            let (_, conn) = if operation == "ensure" {
                vibemud_db::open_app()?
            } else {
                db::open_existing()?
            };
            if operation == "release" {
                vibemud_db::set_setting(&conn, "mod.runtime_owned", "false")?;
            } else if operation == "detach" {
                db::detach(&conn, &session_id)?;
            } else {
                db::renew(&conn, &session_id, active)?;
                if operation == "ensure" && super::effective_runtime_status(&conn)? != "running" {
                    if let Err(error) = super::start_background_runtime_mode(None, true) {
                        if super::effective_runtime_status(&conn)? != "running" {
                            return Err(error);
                        }
                    }
                }
            }
            Ok(json!({"status":runtime_status(&conn)?}))
        }
        BridgeCommand::Setting { key, value } => {
            let (_, conn) = db::open_existing()?;
            match key.as_str() {
                "language" if ["ko", "en"].contains(&value.as_str()) => {}
                "intro_seen" | "animations" if ["true", "false"].contains(&value.as_str()) => {}
                _ => bail!("invalid setting value"),
            }
            let key = match key.as_str() {
                "intro_seen" => "onboarding.intro_seen",
                "language" => "mod.language",
                _ => "mod.animations",
            };
            vibemud_db::set_setting(&conn, key, &value)?;
            Ok(json!({"saved":true}))
        }
        BridgeCommand::Reset { generation, yes } => {
            if !yes {
                bail!("reset requires confirmation");
            }
            let (paths, mut conn) = db::open_existing()?;
            if db::generation(&conn)? != generation {
                bail!("save changed; refresh before resetting");
            }
            vibemud_runtime::stop_runtime()?;
            let backup = paths.backups.join(format!(
                "before-mod-reset-{}.db",
                time::OffsetDateTime::now_utc().unix_timestamp_nanos()
            ));
            // SQLite's own backup statement includes committed WAL data on Windows too.
            conn.execute("VACUUM INTO ?1", [backup.to_string_lossy().as_ref()])?;
            // Recheck inside the write transaction, rather than reset a newer save.
            vibemud_db::reset_game_state_if_generation(&mut conn, Some(&generation))?;
            if paths.vibe_activity.exists() {
                std::fs::remove_file(&paths.vibe_activity)?;
            }
            Ok(json!({"generation":db::generation(&conn)?,"backup":backup}))
        }
    }
}

fn action_kind(value: &str) -> Result<CommandKind> {
    use CommandKind::*;
    [
        AreaEnter,
        HuntStart,
        HuntStop,
        DungeonEnter,
        DungeonRetreat,
        PartyRecruit,
        PartySwap,
        Equip,
        Unequip,
        Enhance,
        SkillUse,
        ShopBuy,
        ShopSell,
        SellCommon,
        QuestClaim,
        QuestClaimAll,
        ItemLock,
        ItemUnlock,
        Rest,
        Town,
    ]
    .into_iter()
    .find(|k| k.as_str() == value)
    .context("unknown game action")
}

fn validate_action(conn: &Connection, kind: &CommandKind, p: &CommandPayload) -> Result<()> {
    use CommandKind::*;
    let required = match kind {
        AreaEnter => p.area_id.as_deref(),
        DungeonEnter => p.dungeon_id.as_deref(),
        Equip | Enhance | ShopBuy | ShopSell | ItemLock | ItemUnlock => p.item_id.as_deref(),
        Unequip => p.equip_slot.as_deref(),
        PartySwap => p.companion_id.as_deref(),
        SkillUse => p.skill_id.as_deref(),
        QuestClaim => p.quest_id.as_deref(),
        _ => Some("valid"),
    };
    if required.is_none_or(str::is_empty) {
        bail!("missing action argument");
    }
    for (table, id) in [
        ("areas", p.area_id.as_deref()),
        ("dungeons", p.dungeon_id.as_deref()),
    ] {
        if let Some(id) = id {
            let found: bool = conn.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id=?1)"),
                [id],
                |r| r.get(0),
            )?;
            if !found {
                bail!("unknown destination");
            }
        }
    }
    if matches!(kind, CommandKind::SkillUse)
        && !["slash", "guard", "taunt", "heal", "firebolt"]
            .contains(&p.skill_id.as_deref().unwrap_or(""))
    {
        bail!("unknown skill");
    }
    if matches!(kind, CommandKind::PartySwap) && !matches!(p.slot, Some(1..=3)) {
        bail!("party slot must be 1, 2 or 3");
    }
    Ok(())
}

fn snapshot(paths: &AppPaths, conn: &Connection) -> Result<Value> {
    let tx = conn.unchecked_transaction()?;
    let mut game = vibemud_db::build_snapshot(&tx)?;
    let config: vibemud_db::AppConfig = toml::from_str(&std::fs::read_to_string(&paths.config)?)?;
    let language = vibemud_db::setting_value(&tx, "mod.language")?.unwrap_or(config.ui.language);
    let animations = vibemud_db::setting_value(&tx, "mod.animations")?.as_deref() != Some("false");
    let lang = if language == "ko" {
        super::UiLanguage::Ko
    } else {
        super::UiLanguage::En
    };
    game.recent_log = game
        .recent_log
        .iter()
        .map(|line| super::display_message(line, lang))
        .collect();
    let logs = vibemud_db::recent_log_entries(&tx, 10)?;
    let scene = super::render_combat_sprite_rows_at_phase(
        &game,
        &logs,
        36,
        if animations { game.clock_tick } else { 0 },
    )
    .into_iter()
    .map(|row| plain_text(&row))
    .collect::<Vec<_>>();
    let narration = if language == "ko" {
        super::INTRO_NARRATION_KO
    } else {
        super::INTRO_NARRATION_EN
    };
    let dialogue = if language == "ko" {
        super::INTRO_DIALOGUE_KO
    } else {
        super::INTRO_DIALOGUE_EN
    };
    let intro = narration
        .iter()
        .map(|s| s.to_string())
        .chain(dialogue.iter().map(|(_, who, s)| format!("{who}: {s}")))
        .collect::<Vec<_>>();
    let mut enhancements = serde_json::Map::new();
    for item in &game.inventory {
        if let Some(rule) = vibemud_db::adjusted_enhancement_rule(
            &tx,
            item.enhancement_level,
            &item.rarity,
            &item.item_type,
        )? {
            enhancements.insert(item.id.clone(), json!({"cost":rule.upgrade_gold_cost,"success_rate":rule.success_rate,"failure_drop":rule.failure_level_drop}));
        }
    }
    let runtime = runtime_status(&tx)?;
    let quests=vibemud_db::load_daily_quests_readonly(&tx)?.into_iter().map(|q|json!({"id":q.quest_id,"title":q.title,"progress":q.progress,"target":q.target,"status":q.status,"reward_kind":q.reward_kind,"reward_amount":q.reward_amount,"fever_minutes":q.fever_minutes})).collect::<Vec<_>>();
    let catalog = |table: &str| -> Result<Vec<Value>> {
        let mut s = tx.prepare(&format!(
            "SELECT id,name,recommended_level FROM {table} ORDER BY recommended_level,id"
        ))?;
        let rows=s.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"level":r.get::<_,i64>(2)?})))?;
        let mut rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        if language == "ko" {
            for row in &mut rows {
                let id = row["id"].as_str().unwrap_or("");
                let name = if table == "areas" {
                    super::ko_area_label(id)
                } else {
                    super::ko_dungeon_label(id)
                };
                row["name"] = json!(name);
            }
        }
        Ok(rows)
    };
    let queue=vibemud_db::recent_queue_entries(&tx,10)?.into_iter().map(|q|json!({"id":q.id,"kind":q.command_type,"status":q.status,"error":q.error_message,"result":q.result_json.and_then(|s|serde_json::from_str::<Value>(&s).ok())})).collect::<Vec<_>>();
    let companions = {
        let mut stmt = tx.prepare("SELECT id,name,role,unlocked FROM companions ORDER BY id")?;
        let rows=stmt.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,String>(1)?,"role":r.get::<_,String>(2)?,"unlocked":r.get::<_,bool>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let data = json!({
        "generation":db::generation(&tx)?,"game":game,
        "earnings":db::adventure_totals(&tx)?,
        "runtime":{"status":runtime},
        "fever":db::fever_active(&tx)? || vibemud_db::vibe_activity().is_some_and(|a|a.active),
        "quests":quests,"areas":catalog("areas")?,"dungeons":catalog("dungeons")?,"companions":companions,"commands":queue,
        "settings":{"language":language,"animations":animations,"intro_seen":vibemud_db::setting_value(&tx,"onboarding.intro_seen")?.as_deref()==Some("true")},
        "scene":scene,"intro":intro,"location":super::hunt_progress_badge(&tx,&game,lang),
        "enhancements":enhancements,
        "progress":{"recovery_until":vibemud_db::setting_value(&tx,"game.recovery_until")?,"dungeon_point":vibemud_db::setting_value(&tx,"game.dungeon_point")?,"dungeon_kills":vibemud_db::setting_value(&tx,"game.dungeon_normal_kills")?}
    });
    tx.commit()?;
    Ok(data)
}

fn plain_text(value: &str) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < value.len() {
        if let Some(end) = super::ansi_sequence_end(value, index) {
            index = end;
            continue;
        }
        let ch = value[index..].chars().next().unwrap();
        if !ch.is_control() {
            out.push(ch);
        }
        index += ch.len_utf8();
    }
    out
}

fn runtime_status(conn: &Connection) -> Result<&'static str> {
    let session = vibemud_db::session_info(conn)?;
    Ok(
        if session.status == "running" && session.runtime_pid.is_some_and(super::pid_is_running) {
            "running"
        } else {
            "stopped"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_is_readonly_and_contains_complete_views() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            root: dir.path().to_path_buf(),
            db: dir.path().join("vibemud.db"),
            config: dir.path().join("config.toml"),
            logs: dir.path().join("logs"),
            backups: dir.path().join("backups"),
            vibe_activity: dir.path().join("vibe-activity.json"),
        };
        let conn = Connection::open(&paths.db).unwrap();
        vibemud_db::migrate(&conn).unwrap();
        vibemud_db::seed_initial_state(&conn).unwrap();
        vibemud_db::load_daily_quests(&conn).unwrap();
        std::fs::write(
            &paths.config,
            toml::to_string(&vibemud_db::AppConfig::default()).unwrap(),
        )
        .unwrap();
        conn.pragma_update(None, "query_only", true).unwrap();
        let a = snapshot(&paths, &conn).unwrap();
        let b = snapshot(&paths, &conn).unwrap();
        assert_eq!(a["game"], b["game"]);
        assert_eq!(a["earnings"]["kills"], 0);
        assert_eq!(a["earnings"], b["earnings"]);
        assert_eq!(a["quests"].as_array().unwrap().len(), 5);
        assert!(!a["areas"].as_array().unwrap().is_empty());
        assert!(!a["generation"].as_str().unwrap().is_empty());
    }
    #[test]
    fn game_actions_are_allowlisted() {
        assert!(action_kind("rm -rf").is_err());
        assert!(action_kind("quest_claim").is_ok());
    }
}
