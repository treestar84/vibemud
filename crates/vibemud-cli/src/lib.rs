use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};
use rusqlite::OptionalExtension;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use unicode_width::UnicodeWidthChar;
use vibemud_core::{CommandKind, CommandPayload, StatusLineDto};
mod bridge;

const DEFEATED_SCENE_TTL_SECONDS: i64 = 3;

#[cfg(unix)]
unsafe extern "C" {
    fn setsid() -> i32;
}

#[derive(Parser, Debug)]
#[command(
    name = "vibemud",
    version,
    about = "Independent local idle MUD RPG runtime"
)]
pub struct VibeMudCli {
    #[command(subcommand)]
    pub command: VibeMudCommand,
}

#[derive(Subcommand, Debug)]
pub enum VibeMudCommand {
    /// Machine-readable local API used by the Claude Code mod.
    Bridge(bridge::BridgeArgs),
    Init,
    Reset {
        #[arg(long)]
        yes: bool,
    },
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Hud(HudArgs),
    Statusline,
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Setup(SetupArgs),
    Intro(IntroArgs),
    Vibe {
        #[command(subcommand)]
        command: VibeCommand,
    },
    Doctor,
    Simulate(SimulateArgs),
    #[command(name = "__dev-tournament", hide = true)]
    DevTournament(DevTournamentArgs),
}

#[derive(Subcommand, Debug)]
pub enum SessionCommand {
    Start {
        #[arg(long)]
        ticks: Option<u32>,
        #[arg(long)]
        background: bool,
    },
    Stop,
    Status,
    Codex,
    Claude,
    WindowsTerminal {
        #[arg(default_value = "claude")]
        cli: String,
    },
}

#[derive(Args, Debug)]
pub struct HudArgs {
    #[arg(long)]
    pub side: bool,
    #[arg(long)]
    pub statusline: bool,
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub once: bool,
    #[arg(long, default_value_t = 2)]
    pub refresh: u64,
    #[arg(long)]
    pub width: Option<usize>,
    #[arg(long)]
    pub ascii: bool,
    #[arg(long)]
    pub live: bool,
    #[arg(long)]
    pub panel: bool,
    #[arg(long, default_value_t = 8)]
    pub log_lines: usize,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    Get { key: String },
    Set { key: String, value: String },
}

#[derive(Args, Debug, Clone, Default)]
pub struct SetupArgs {
    #[arg(long, value_parser = ["ko", "en"])]
    pub language: Option<String>,
    #[arg(long, value_parser = ["auto", "claude", "codex", "cli"])]
    pub agent: Option<String>,
    #[arg(long, value_parser = ["auto", "tmux", "ghostty", "windows-terminal", "plain"])]
    pub terminal: Option<String>,
    #[arg(long, value_parser = ["user", "project"])]
    pub storage: Option<String>,
    #[arg(long, help = "Use defaults for omitted values instead of prompting")]
    pub yes: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct IntroArgs {
    #[arg(long, help = "Replay even if the one-time intro was already seen")]
    pub replay: bool,
    #[arg(
        long = "if-needed",
        help = "Play only when the one-time intro has not been seen; stay silent otherwise"
    )]
    pub if_needed: bool,
    #[arg(long, help = "Skip animation delays for tests and smoke checks")]
    pub fast: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupValues {
    language: String,
    agent: String,
    terminal: String,
    storage: String,
    storage_root: Option<PathBuf>,
    hud_mode: String,
    popup_panes: bool,
}

#[derive(Subcommand, Debug)]
pub enum VibeCommand {
    Heartbeat {
        #[arg(long, default_value = "manual")]
        source: String,
    },
    Stop {
        #[arg(long, default_value = "manual")]
        source: String,
    },
    Status,
}

#[derive(Args, Debug)]
pub struct SimulateArgs {
    #[arg(long, default_value = "forest-edge")]
    pub area: String,
    #[arg(long, default_value_t = 1)]
    pub hours: u32,
    #[arg(long, default_value_t = 10)]
    pub runs: u32,
    #[arg(long, default_value_t = 42)]
    pub seed: u64,
}

#[derive(Args, Debug, Clone)]
pub struct DevTournamentArgs {
    #[arg(long, default_value_t = 20)]
    pub age: u64,
    #[arg(long, default_value_t = 96)]
    pub width: usize,
    #[arg(long, help = "Skip animation delays")]
    pub fast: bool,
}

#[derive(Parser, Debug)]
#[command(name = "mudctl", version, about = "VibeMUD game control CLI")]
pub struct MudCtlCli {
    #[command(subcommand)]
    pub command: MudCommand,
}

#[derive(Subcommand, Debug)]
pub enum MudCommand {
    Status,
    FullStatus,
    Stats {
        #[command(subcommand)]
        command: Option<StatsCommand>,
    },
    Map,
    Area {
        #[command(subcommand)]
        command: AreaCommand,
    },
    Hunt {
        #[command(subcommand)]
        command: HuntCommand,
    },
    Dungeon {
        #[command(subcommand)]
        command: DungeonCommand,
    },
    Party {
        #[command(subcommand)]
        command: Option<PartyCommand>,
    },
    Inventory,
    Equip {
        item_id: String,
        #[arg(long)]
        slot: Option<String>,
    },
    Unequip {
        slot: String,
    },
    Enhance {
        item_id: String,
    },
    Equipment {
        #[command(subcommand)]
        command: EquipmentCommand,
    },
    Quest {
        #[command(subcommand)]
        command: Option<QuestCommand>,
    },
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
    Shop {
        #[command(subcommand)]
        command: Option<ShopCommand>,
    },
    Rest,
    Town,
    Log(LogArgs),
    System,
    Queue(QueueArgs),
    Alias {
        #[command(subcommand)]
        command: AliasCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum AreaCommand {
    List,
    Enter { area_id: String },
}

#[derive(Subcommand, Debug)]
pub enum HuntCommand {
    Start {
        area_id: Option<String>,
        #[arg(long)]
        area: Option<String>,
        #[arg(long)]
        auto_start: bool,
    },
    Stop,
}

#[derive(Subcommand, Debug)]
pub enum DungeonCommand {
    List,
    Enter {
        dungeon_id: String,
        #[arg(long)]
        auto_start: bool,
    },
    Retreat,
}

#[derive(Subcommand, Debug)]
pub enum PartyCommand {
    Recruit,
    Swap { slot: u8, companion_id: String },
}

#[derive(Subcommand, Debug)]
pub enum SkillCommand {
    List,
    Use { skill_id: String },
}

#[derive(Subcommand, Debug)]
pub enum EquipmentCommand {
    Slots {
        #[arg(long)]
        raw: bool,
    },
    Inventory,
    Gold,
    Stats {
        #[arg(long)]
        raw: bool,
    },
    SellCommon {
        rarity: Option<String>,
    },
    Empty,
    Lock {
        item_id: String,
    },
    Unlock {
        item_id: String,
    },
    Tooltip {
        slot: String,
    },
    ItemTooltip {
        item_id: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum QuestCommand {
    List {
        #[arg(long)]
        raw: bool,
    },
    Claim {
        quest_id: String,
    },
    ClaimAll,
}

#[derive(Subcommand, Debug)]
pub enum ShopCommand {
    Buy { item_id: String },
    Sell { item_id: String },
}

#[derive(Subcommand, Debug)]
pub enum StatsCommand {
    Open,
    Close,
    Toggle,
}

#[derive(Subcommand, Debug)]
pub enum AliasCommand {
    List,
}

#[derive(Args, Debug)]
pub struct LogArgs {
    #[arg(long, default_value_t = 10)]
    pub tail: usize,
    #[arg(long)]
    pub follow: bool,
}

#[derive(Args, Debug)]
pub struct QueueArgs {
    #[arg(long, default_value_t = 10)]
    pub tail: usize,
}

pub fn run_vibemud(cli: VibeMudCli) -> Result<()> {
    enable_windows_vt();
    match cli.command {
        VibeMudCommand::Bridge(args) => bridge::run(args)?,
        VibeMudCommand::Init => {
            let paths = vibemud_db::init_app()?;
            let lang = ui_language();
            println!(
                "{} {}",
                label(lang, "VibeMUD 초기화 완료:", "Initialized VibeMUD at"),
                paths.root.display()
            );
        }
        VibeMudCommand::Reset { yes } => reset_game(yes)?,
        VibeMudCommand::Session { command } => match command {
            SessionCommand::Start { ticks, background } => {
                if background {
                    let pid = start_background_runtime(ticks)?;
                    println!(
                        "{} pid={pid}",
                        label(
                            ui_language(),
                            "VibeMUD 백그라운드 런타임 시작",
                            "VibeMUD background runtime started"
                        )
                    );
                } else {
                    vibemud_runtime::start_runtime(ticks)?;
                    let lang = ui_language();
                    if lang == UiLanguage::Ko {
                        println!(
                            "VibeMUD 세션 완료{}",
                            ticks.map(|v| format!(" ({v}틱 후)")).unwrap_or_default()
                        );
                    } else {
                        println!(
                            "VibeMUD session completed{}",
                            ticks
                                .map(|v| format!(" after {v} ticks"))
                                .unwrap_or_default()
                        );
                    }
                }
            }
            SessionCommand::Stop => {
                vibemud_runtime::stop_runtime()?;
                cleanup_hud_processes()?;
                println!(
                    "{}",
                    label(
                        ui_language(),
                        "VibeMUD 세션 중지",
                        "VibeMUD session stopped"
                    )
                );
            }
            SessionCommand::Status => println!("{}", vibemud_runtime::status_runtime()?),
            SessionCommand::Codex => start_agent_layout("codex")?,
            SessionCommand::Claude => start_agent_layout("claude")?,
            SessionCommand::WindowsTerminal { cli } => start_windows_terminal_layout(&cli)?,
        },
        VibeMudCommand::Hud(args) => print_hud(args)?,
        VibeMudCommand::Statusline => print_statusline()?,
        VibeMudCommand::Config { command } => match command {
            ConfigCommand::Get { key } => print_config_value(&key)?,
            ConfigCommand::Set { key, value } => set_config_value(&key, &value)?,
        },
        VibeMudCommand::Setup(args) => run_setup(args)?,
        VibeMudCommand::Intro(args) => run_intro(args)?,
        VibeMudCommand::Vibe { command } => match command {
            VibeCommand::Heartbeat { source } => {
                vibemud_db::write_vibe_activity(&source, true)?;
                println!("FEVERTIME heartbeat: active ({source})");
            }
            VibeCommand::Stop { source } => {
                vibemud_db::clear_vibe_activity(&source)?;
                println!("FEVERTIME heartbeat: stopped ({source})");
            }
            VibeCommand::Status => {
                if vibemud_db::vibe_fever_active() {
                    println!("FEVERTIME");
                } else {
                    println!("idle");
                }
            }
        },
        VibeMudCommand::Doctor => print_doctor()?,
        VibeMudCommand::Simulate(args) => {
            let summary = vibemud_core::simulate(&args.area, args.hours, args.runs, args.seed);
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        VibeMudCommand::DevTournament(args) => run_dev_tournament(args)?,
    }
    Ok(())
}

pub fn run_mudctl_from<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    let raw: Vec<String> = args.into_iter().map(Into::into).collect();
    let normalized = normalize_mudctl_argv(raw);
    let cli = MudCtlCli::parse_from(normalized);
    run_mudctl(cli)
}

pub fn normalize_mudctl_argv(raw: Vec<String>) -> Vec<String> {
    if raw.is_empty() {
        return raw;
    }
    let mut out = vec![raw[0].clone()];
    let rest = expand_top_level_shortcut(vibemud_core::normalize_alias_tokens(&raw[1..]));
    out.extend(rest);
    out
}

fn expand_top_level_shortcut(args: Vec<String>) -> Vec<String> {
    match args.as_slice() {
        [command] if matches!(command.as_str(), "a" | "hunt" | "사냥") => {
            vec!["hunt".into(), "start".into()]
        }
        [command] if matches!(command.as_str(), "c" | "character" | "캐릭터") => {
            vec!["stats".into(), "open".into()]
        }
        [command] if matches!(command.as_str(), "x" | "close" | "닫기") => {
            vec!["stats".into(), "close".into()]
        }
        [command] if matches!(command.as_str(), "s" | "stop" | "정지" | "중지") => {
            vec!["hunt".into(), "stop".into()]
        }
        [command] if matches!(command.as_str(), "m" | "map" | "menu" | "지도" | "메뉴") => {
            vec!["map".into()]
        }
        _ => args,
    }
}

fn start_background_runtime(ticks: Option<u32>) -> Result<u32> {
    start_background_runtime_mode(ticks, false)
}

fn start_background_runtime_mode(ticks: Option<u32>, mod_owned: bool) -> Result<u32> {
    let (_paths, conn) = vibemud_db::open_app()?;
    if effective_runtime_status(&conn)? == "running" {
        let info = vibemud_db::session_info(&conn)?;
        anyhow::bail!(
            "VibeMUD runtime is already running{}",
            info.runtime_pid
                .map(|pid| format!(" pid={pid}"))
                .unwrap_or_default()
        );
    }

    let runtime = runtime_binary_path()?;
    let mut command = Command::new(&runtime);
    if mod_owned {
        command.env("VIBEMUD_RUNTIME_OWNER", "mod");
    } else {
        command.env_remove("VIBEMUD_RUNTIME_OWNER");
    }
    if let Some(ticks) = ticks {
        command.arg("--ticks").arg(ticks.to_string());
    }
    detach_background_command(&mut command);
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to start {}", runtime.display()))?;
    let pid = child.id();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("VibeMUD runtime exited during startup with status {status}");
        }

        let info = vibemud_db::session_info(&conn)?;
        if info.status == "running" && info.runtime_pid == Some(pid as i64) {
            return Ok(pid);
        }
        if info
            .runtime_pid
            .is_some_and(|runtime_pid| runtime_pid != pid as i64 && pid_is_running(runtime_pid))
        {
            let status = vibemud_runtime::status_runtime()?;
            let refreshed = vibemud_db::session_info(&conn)?;
            if status == "running" && refreshed.runtime_pid != Some(pid as i64) {
                let _ = child.kill();
                anyhow::bail!(
                    "VibeMUD runtime is already running{}",
                    refreshed
                        .runtime_pid
                        .map(|runtime_pid| format!(" pid={runtime_pid}"))
                        .unwrap_or_default()
                );
            }
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            anyhow::bail!("VibeMUD runtime did not become ready within 5 seconds");
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(unix)]
fn detach_background_command(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        command.pre_exec(|| {
            if setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn detach_background_command(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    // Console children die with the parent console on Windows: closing the
    // spawning terminal (or the plugin hook's hidden PowerShell console) sends
    // CTRL_CLOSE_EVENT to every attached process, and Ctrl+C in that terminal
    // broadcasts CTRL_C_EVENT the same way. DETACHED_PROCESS starts the
    // runtime without any console (the Windows equivalent of setsid), and
    // CREATE_NEW_PROCESS_GROUP keeps it out of the parent's control group.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn detach_background_command(_command: &mut Command) {}

#[cfg(windows)]
fn enable_windows_vt() {
    // HUD/status views emit ANSI escape sequences. Windows Terminal processes
    // them by default, but the legacy conhost (cmd.exe default console on some
    // setups) prints them raw unless ENABLE_VIRTUAL_TERMINAL_PROCESSING is set
    // on the output handle. Best effort: piped/redirected output has no
    // console mode and is left untouched.
    type Handle = *mut core::ffi::c_void;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(handle: u32) -> Handle;
        fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: Handle, mode: u32) -> i32;
    }
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() {
            return;
        }
        let mut mode = 0u32;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return;
        }
        let _ = SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
    }
}

#[cfg(not(windows))]
fn enable_windows_vt() {}

fn effective_runtime_status(_conn: &rusqlite::Connection) -> Result<String> {
    vibemud_runtime::status_runtime()
}

#[cfg(unix)]
fn pid_is_running(pid: i64) -> bool {
    pid > 0
        && Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
        && !pid_is_zombie(pid)
}

#[cfg(unix)]
fn pid_is_zombie(pid: i64) -> bool {
    Command::new("ps")
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
    use std::os::windows::process::CommandExt;
    if pid <= 0 {
        return false;
    }
    Command::new("tasklist")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                    line.contains(&format!("\"{pid}\"")) || line.contains(&format!(",{pid},"))
                })
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn pid_is_running(pid: i64) -> bool {
    pid > 0
}

struct HudProcessGuard {
    path: PathBuf,
}

impl HudProcessGuard {
    fn register(root: &Path) -> Result<Self> {
        let dir = hud_pid_dir(root);
        std::fs::create_dir_all(&dir)?;
        let pid = std::process::id();
        let path = dir.join(format!("{pid}.pid"));
        std::fs::write(&path, format!("{pid}\n"))?;
        Ok(Self { path })
    }
}

impl Drop for HudProcessGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn hud_pid_dir(root: &Path) -> PathBuf {
    root.join("hud-pids")
}

fn cleanup_hud_processes() -> Result<()> {
    let (paths, _conn) = vibemud_db::open_app()?;
    cleanup_hud_processes_in_root(&paths.root)
}

fn cleanup_hud_processes_in_root(root: &Path) -> Result<()> {
    let dir = hud_pid_dir(root);
    if dir.exists() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            let pid = std::fs::read_to_string(&path)
                .ok()
                .and_then(|value| value.trim().parse::<i64>().ok());
            match pid {
                Some(pid) if hud_pid_is_safe_to_stop(pid) => {
                    terminate_hud_pid(pid);
                    if !pid_is_running(pid) {
                        let _ = std::fs::remove_file(&path);
                    }
                }
                Some(pid) if !pid_is_running(pid) => {
                    let _ = std::fs::remove_file(&path);
                }
                None => {
                    let _ = std::fs::remove_file(&path);
                }
                _ => {}
            }
        }
        let _ = std::fs::remove_dir(&dir);
    }

    Ok(())
}

fn hud_command_is_safe_to_stop(command: &str) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    let Some(first) = tokens.first() else {
        return false;
    };
    if token_basename_is(first, &["vibemud-hud", "vibemud-hud.exe"]) {
        return true;
    }
    if token_basename_is(first, &["vibemud", "vibemud.exe", "vibemud.js"])
        && tokens.get(1).is_some_and(|arg| *arg == "hud")
    {
        return true;
    }
    token_basename_is(first, &["node", "node.exe"])
        && tokens
            .get(1)
            .is_some_and(|arg| token_basename_is(arg, &["vibemud", "vibemud.exe", "vibemud.js"]))
        && tokens.get(2).is_some_and(|arg| *arg == "hud")
}

fn token_basename_is(token: &str, allowed: &[&str]) -> bool {
    let token = token.trim_matches('"');
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| allowed.iter().any(|allowed| name == *allowed))
}

#[cfg(unix)]
fn hud_pid_is_safe_to_stop(pid: i64) -> bool {
    if pid <= 0 || pid as u32 == std::process::id() || !pid_is_running(pid) {
        return false;
    }
    Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .map(|output| {
            if !output.status.success() {
                return false;
            }
            let command = String::from_utf8_lossy(&output.stdout);
            hud_command_is_safe_to_stop(&command)
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn hud_pid_is_safe_to_stop(pid: i64) -> bool {
    if pid <= 0 || pid as u32 == std::process::id() || !pid_is_running(pid) {
        return false;
    }
    Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-CimInstance Win32_Process -Filter \"ProcessId={pid}\").CommandLine"),
        ])
        .output()
        .map(|output| {
            output.status.success()
                && hud_command_is_safe_to_stop(&String::from_utf8_lossy(&output.stdout))
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn hud_pid_is_safe_to_stop(pid: i64) -> bool {
    pid > 0 && pid as u32 != std::process::id() && pid_is_running(pid)
}

#[cfg(unix)]
fn terminate_hud_pid(pid: i64) {
    if !hud_pid_is_safe_to_stop(pid) {
        return;
    }
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status();
    for _ in 0..20 {
        if !pid_is_running(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if hud_pid_is_safe_to_stop(pid) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg(pid.to_string())
            .status();
    }
}

#[cfg(windows)]
fn terminate_hud_pid(pid: i64) {
    if !hud_pid_is_safe_to_stop(pid) {
        return;
    }
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status();
    for _ in 0..20 {
        if !pid_is_running(pid) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if hud_pid_is_safe_to_stop(pid) {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .status();
    }
}

#[cfg(not(any(unix, windows)))]
fn terminate_hud_pid(_pid: i64) {}

fn runtime_binary_path() -> Result<std::path::PathBuf> {
    let current = std::env::current_exe().context("failed to resolve current executable")?;
    let dir = current
        .parent()
        .context("current executable has no parent directory")?;
    let binary = if cfg!(windows) {
        "vibemud-runtime.exe"
    } else {
        "vibemud-runtime"
    };
    let sibling = dir.join(binary);
    if sibling.exists() {
        Ok(sibling)
    } else {
        Ok(std::path::PathBuf::from(binary))
    }
}

pub fn run_mudctl(cli: MudCtlCli) -> Result<()> {
    enable_windows_vt();
    let (_paths, conn) = vibemud_db::open_app()?;
    match cli.command {
        MudCommand::Status => print_status(&conn, false),
        MudCommand::FullStatus => print_status(&conn, true),
        MudCommand::Stats { command } => set_stats_view(&conn, command),
        MudCommand::Map => set_map_view(&conn),
        MudCommand::Area { command } => match command {
            AreaCommand::List => print_area_list(&conn),
            AreaCommand::Enter { area_id } => enqueue(
                &conn,
                CommandKind::AreaEnter,
                CommandPayload {
                    area_id: Some(area_id),
                    ..Default::default()
                },
                false,
            ),
        },
        MudCommand::Hunt { command } => match command {
            HuntCommand::Start {
                area_id,
                area,
                auto_start,
            } => {
                let area_id = area.or(area_id);
                enqueue(
                    &conn,
                    CommandKind::HuntStart,
                    CommandPayload {
                        area_id,
                        ..Default::default()
                    },
                    auto_start,
                )
            }
            HuntCommand::Stop => enqueue(
                &conn,
                CommandKind::HuntStop,
                CommandPayload::default(),
                false,
            ),
        },
        MudCommand::Dungeon { command } => match command {
            DungeonCommand::List => print_dungeon_list(&conn),
            DungeonCommand::Enter {
                dungeon_id,
                auto_start,
            } => enqueue(
                &conn,
                CommandKind::DungeonEnter,
                CommandPayload {
                    dungeon_id: Some(dungeon_id),
                    ..Default::default()
                },
                auto_start,
            ),
            DungeonCommand::Retreat => enqueue(
                &conn,
                CommandKind::DungeonRetreat,
                CommandPayload::default(),
                false,
            ),
        },
        MudCommand::Party { command } => match command {
            None => print_party(&conn),
            Some(PartyCommand::Recruit) => enqueue(
                &conn,
                CommandKind::PartyRecruit,
                CommandPayload::default(),
                false,
            ),
            Some(PartyCommand::Swap { slot, companion_id }) => enqueue(
                &conn,
                CommandKind::PartySwap,
                CommandPayload {
                    slot: Some(slot),
                    companion_id: Some(companion_id),
                    ..Default::default()
                },
                false,
            ),
        },
        MudCommand::Inventory => print_inventory(&conn),
        MudCommand::Equip { item_id, slot } => enqueue(
            &conn,
            CommandKind::Equip,
            CommandPayload {
                item_id: Some(item_id),
                equip_slot: slot,
                ..Default::default()
            },
            false,
        ),
        MudCommand::Unequip { slot } => enqueue(
            &conn,
            CommandKind::Unequip,
            CommandPayload {
                equip_slot: Some(slot),
                ..Default::default()
            },
            false,
        ),
        MudCommand::Enhance { item_id } => enqueue(
            &conn,
            CommandKind::Enhance,
            CommandPayload {
                item_id: Some(item_id),
                ..Default::default()
            },
            false,
        ),
        MudCommand::Equipment { command } => match command {
            EquipmentCommand::Slots { raw } => print_equipment_slots(&conn, raw),
            EquipmentCommand::Inventory => print_equipment_inventory(&conn),
            EquipmentCommand::Gold => print_equipment_gold(&conn),
            EquipmentCommand::Stats { raw } => print_equipment_stats(&conn, raw),
            EquipmentCommand::SellCommon { rarity } => enqueue(
                &conn,
                CommandKind::SellCommon,
                CommandPayload {
                    rarity,
                    ..Default::default()
                },
                false,
            ),
            EquipmentCommand::Empty => enqueue(
                &conn,
                CommandKind::SellCommon,
                CommandPayload::default(),
                false,
            ),
            EquipmentCommand::Lock { item_id } => enqueue(
                &conn,
                CommandKind::ItemLock,
                CommandPayload {
                    item_id: Some(item_id),
                    ..Default::default()
                },
                false,
            ),
            EquipmentCommand::Unlock { item_id } => enqueue(
                &conn,
                CommandKind::ItemUnlock,
                CommandPayload {
                    item_id: Some(item_id),
                    ..Default::default()
                },
                false,
            ),
            EquipmentCommand::Tooltip { slot } => print_equipment_tooltip(&conn, &slot),
            EquipmentCommand::ItemTooltip { item_id } => {
                print_equipment_item_tooltip(&conn, &item_id)
            }
        },
        MudCommand::Quest { command } => match command {
            None => set_quest_view(&conn),
            Some(QuestCommand::List { raw }) => print_quests(&conn, raw),
            Some(QuestCommand::Claim { quest_id }) => enqueue(
                &conn,
                CommandKind::QuestClaim,
                CommandPayload {
                    quest_id: Some(quest_id),
                    ..Default::default()
                },
                false,
            ),
            Some(QuestCommand::ClaimAll) => enqueue(
                &conn,
                CommandKind::QuestClaimAll,
                CommandPayload::default(),
                false,
            ),
        },
        MudCommand::Skill { command } => match command {
            SkillCommand::List => {
                println!("Skills: slash, guard, taunt, heal, firebolt");
                Ok(())
            }
            SkillCommand::Use { skill_id } => enqueue(
                &conn,
                CommandKind::SkillUse,
                CommandPayload {
                    skill_id: Some(skill_id),
                    ..Default::default()
                },
                false,
            ),
        },
        MudCommand::Shop { command } => match command {
            None => print_shop(),
            Some(ShopCommand::Buy { item_id }) => enqueue(
                &conn,
                CommandKind::ShopBuy,
                CommandPayload {
                    item_id: Some(item_id),
                    ..Default::default()
                },
                false,
            ),
            Some(ShopCommand::Sell { item_id }) => enqueue(
                &conn,
                CommandKind::ShopSell,
                CommandPayload {
                    item_id: Some(item_id),
                    ..Default::default()
                },
                false,
            ),
        },
        MudCommand::Rest => enqueue(&conn, CommandKind::Rest, CommandPayload::default(), false),
        MudCommand::Town => enqueue(&conn, CommandKind::Town, CommandPayload::default(), false),
        MudCommand::Log(args) => print_log(&conn, args.tail, args.follow),
        MudCommand::System => print_system(&conn),
        MudCommand::Queue(args) => print_queue(&conn, args.tail),
        MudCommand::Alias {
            command: AliasCommand::List,
        } => print_aliases(),
    }
}

fn print_status(conn: &rusqlite::Connection, full: bool) -> Result<()> {
    let snapshot = vibemud_db::load_snapshot(conn)?;
    let lang = ui_language();
    if full {
        println!("{}", render_full_status(&snapshot, lang));
    } else {
        println!(
            "{}",
            render_status_for_width(&snapshot, terminal_width(), lang)
        );
    }
    Ok(())
}

fn set_stats_view(conn: &rusqlite::Connection, command: Option<StatsCommand>) -> Result<()> {
    let lang = ui_language();
    let enabled = match command.unwrap_or(StatsCommand::Open) {
        StatsCommand::Open => true,
        StatsCommand::Close => false,
        StatsCommand::Toggle => !stats_view_enabled(conn),
    };
    vibemud_db::set_setting(
        conn,
        "ui.stats_view",
        if enabled { "true" } else { "false" },
    )?;
    vibemud_db::set_setting(conn, "ui.view", if enabled { "stats" } else { "normal" })?;
    vibemud_db::append_event(
        conn,
        vibemud_core::EventKind::CommandProcessed,
        if enabled {
            "Action feedback: character stats opened."
        } else {
            "Action feedback: HUD view closed."
        },
        None,
    )?;
    vibemud_db::write_snapshot_and_hud(conn)?;
    println!(
        "{}",
        if enabled {
            label(
                lang,
                "상세 스탯 HUD 보기: 켜짐 (닫기: mudctl stats close)",
                "Detailed stat HUD view: on (close: mudctl stats close)",
            )
        } else {
            label(
                lang,
                "상세 스탯 HUD 보기: 꺼짐",
                "Detailed stat HUD view: off",
            )
        }
    );
    Ok(())
}

fn set_map_view(conn: &rusqlite::Connection) -> Result<()> {
    let lang = ui_language();
    vibemud_db::set_setting(conn, "ui.stats_view", "false")?;
    vibemud_db::set_setting(conn, "ui.view", "map")?;
    vibemud_db::append_event(
        conn,
        vibemud_core::EventKind::CommandProcessed,
        "Action feedback: map opened.",
        None,
    )?;
    vibemud_db::write_snapshot_and_hud(conn)?;
    println!(
        "{}",
        label(
            lang,
            "지도 HUD 보기: 켜짐 (닫기: mudctl x, 사냥: mudctl a <지역/던전>)",
            "Map HUD view: on (close: mudctl x, hunt: mudctl a <area/dungeon>)",
        )
    );
    Ok(())
}

fn set_quest_view(conn: &rusqlite::Connection) -> Result<()> {
    let lang = ui_language();
    vibemud_db::set_setting(conn, "ui.stats_view", "false")?;
    vibemud_db::set_setting(conn, "ui.view", "quest")?;
    vibemud_db::append_event(
        conn,
        vibemud_core::EventKind::CommandProcessed,
        "Action feedback: quest opened.",
        None,
    )?;
    vibemud_db::write_snapshot_and_hud(conn)?;
    println!(
        "{}",
        label(
            lang,
            "퀘스트 HUD 보기: 켜짐 (닫기: mudctl x, 보상: mudctl quest claim <id>)",
            "Quest HUD view: on (close: mudctl x, reward: mudctl quest claim <id>)",
        )
    );
    Ok(())
}

fn print_statusline() -> Result<()> {
    let (_paths, conn) = vibemud_db::open_app()?;
    let snapshot = vibemud_db::load_snapshot(&conn)?;
    println!(
        "{}",
        render_status_for_width(&snapshot, terminal_width(), ui_language())
    );
    Ok(())
}

fn print_hud(args: HudArgs) -> Result<()> {
    let (paths, conn) = vibemud_db::open_app()?;
    if args.once {
        println!("{}", render_hud_once(&conn, &args)?);
        return Ok(());
    }
    let _hud_guard = HudProcessGuard::register(&paths.root)?;
    let mut last = String::new();
    loop {
        let next = render_hud_once(&conn, &args)?;
        if next != last {
            print!("\x1B[2J\x1B[H{next}");
            std::io::stdout().flush()?;
            last = next;
        }
        thread::sleep(Duration::from_secs(args.refresh.max(1)));
    }
}

fn render_hud_once(conn: &rusqlite::Connection, args: &HudArgs) -> Result<String> {
    if args.panel {
        return render_panel_dashboard(conn, args);
    }
    if args.live {
        return render_live_dashboard(conn, args);
    }
    let snapshot = vibemud_db::load_snapshot(conn)?;
    let lang = ui_language();
    let dto = localized_status_dto(&snapshot, lang);
    if args.full {
        Ok(render_full_status(&snapshot, lang))
    } else if args.side {
        Ok(render_side_panel_localized(&dto, !args.ascii, lang))
    } else if args.statusline {
        Ok(vibemud_hud::render_compact(&dto))
    } else {
        Ok(render_status_for_width(
            &snapshot,
            args.width.unwrap_or(100),
            lang,
        ))
    }
}

fn render_panel_dashboard(conn: &rusqlite::Connection, args: &HudArgs) -> Result<String> {
    let height = env_usize("VIBEMUD_PANEL_HEIGHT")
        .or_else(|| env_usize("LINES"))
        .unwrap_or(32)
        .max(18);
    let width = env_usize("VIBEMUD_PANEL_WIDTH")
        .or_else(|| env_usize("COLUMNS"))
        .or(args.width)
        .unwrap_or(48)
        .max(32);
    let height = height.max(30);
    let log_section_lines = ((height as f32 * 0.30).round() as usize)
        .max(6)
        .min(height.saturating_sub(10));
    let log_entry_rows = log_section_lines.saturating_sub(3).max(3);

    let snapshot = vibemud_db::load_snapshot(conn)?;
    let lang = ui_language();
    let dto = localized_status_dto(&snapshot, lang);
    let _session = vibemud_db::session_info(conn)?;
    let _counts = vibemud_db::command_queue_counts(conn)?;
    let inner = width.saturating_sub(2).max(30);
    match hud_view_mode(conn).as_deref() {
        Some("stats") => {
            return Ok(render_stats_dashboard(
                conn, &snapshot, &dto, inner, height, lang,
            ));
        }
        Some("map") => return render_map_dashboard(conn, &snapshot, &dto, inner, height, lang),
        Some("quest") => return render_quest_dashboard(conn, &snapshot, &dto, inner, height, lang),
        _ => {}
    }

    let logs = vibemud_db::recent_log_entries(conn, args.log_lines.max(log_entry_rows).max(12))?;
    let game_logs: Vec<_> = logs
        .iter()
        .filter(|entry| is_game_log_entry(entry))
        .cloned()
        .collect();

    let border = format!("+{}+", "-".repeat(inner));
    let mut lines = render_normal_header(&snapshot, &dto, inner, &border, lang);

    let scene_capacity = height
        .saturating_sub(log_section_lines + lines.len())
        .max(8);
    let scene_section_lines = scene_section_height(height, log_section_lines).min(scene_capacity);
    let scene_start = height.saturating_sub(log_section_lines + scene_section_lines);
    while lines.len() < scene_start {
        lines.push(panel_line_raw("", inner));
    }
    lines.extend(render_auto_hunt_scene(
        &snapshot,
        &logs,
        conn,
        inner,
        scene_section_lines,
        lang,
    ));

    let log_start = height.saturating_sub(log_section_lines);
    while lines.len() < log_start {
        lines.push(panel_line_raw("", inner));
    }

    if lines.last() != Some(&border) {
        lines.push(border.clone());
    }
    lines.push(panel_line_raw(
        label(lang, "게임 / 자동사냥 로그", "GAME / AUTO-HUNT LOG"),
        inner,
    ));
    for entry in game_logs.iter().take(log_entry_rows).rev() {
        lines.push(panel_line_raw(&panel_log_line(entry, lang), inner));
    }
    while lines.len() + 1 < height {
        lines.push(panel_line_raw("", inner));
    }
    lines.push(border);
    Ok(lines.join("\n"))
}

fn stats_view_enabled(conn: &rusqlite::Connection) -> bool {
    if hud_view_mode(conn).as_deref() == Some("stats") {
        return true;
    }
    vibemud_db::setting_value(conn, "ui.stats_view")
        .ok()
        .flatten()
        .map(|value| matches!(value.as_str(), "1" | "true" | "on" | "yes"))
        .unwrap_or(false)
}

fn hud_view_mode(conn: &rusqlite::Connection) -> Option<String> {
    vibemud_db::setting_value(conn, "ui.view").ok().flatten()
}

fn render_normal_header(
    snapshot: &vibemud_core::GameSnapshot,
    dto: &StatusLineDto,
    inner: usize,
    border: &str,
    lang: UiLanguage,
) -> Vec<String> {
    let stats = vibemud_core::representative_stats(&snapshot.player);
    let mut lines = vec![
        border.to_string(),
        panel_line_raw(
            &header_with_right(
                label(lang, "VibeMUD 상태", "VibeMUD HUD"),
                &character_age_line(snapshot.clock_tick, lang),
                inner,
            ),
            inner,
        ),
        panel_line_raw(
            &header_pair_line(
                label(lang, "캐릭터", "Hero"),
                &format!("Lv.{} {}", snapshot.player.level, dto.class_label),
                label(lang, "위치", "Area"),
                &dto.area_label,
                inner,
            ),
            inner,
        ),
        panel_line_raw(
            &header_pair_line(
                label(lang, "상태", "Mode"),
                &dto.mode_label,
                label(lang, "위험", "Danger"),
                &dto.danger_label,
                inner,
            ),
            inner,
        ),
        panel_line_raw(
            &header_pair_line(
                "HP",
                &colorize(
                    &format!("{}/{}", snapshot.player.hp, snapshot.player.max_hp),
                    AnsiColor::Green,
                ),
                "MP",
                &format!("{}/{}", snapshot.player.mp, snapshot.player.max_mp),
                inner,
            ),
            inner,
        ),
        panel_line_raw(
            &header_pair_line(
                label(lang, "경험치", "XP"),
                &format!("{}/{}", snapshot.player.xp, snapshot.player.xp_to_next),
                label(lang, "골드", "Gold"),
                &snapshot.player.gold.to_string(),
                inner,
            ),
            inner,
        ),
        panel_line_raw(
            &header_pair_line(
                label(lang, "전투력", "Power"),
                &colorize(&stats.combat_power.to_string(), AnsiColor::Blue),
                label(lang, "공/방/속", "A/D/S"),
                &colorize(
                    &format!("{}/{}/{}", stats.attack, stats.defense, stats.speed),
                    AnsiColor::Blue,
                ),
                inner,
            ),
            inner,
        ),
        panel_line_raw(
            label(
                lang,
                "Ctrl-C는 HUD 패널만 닫습니다",
                "Ctrl-C closes HUD pane only",
            ),
            inner,
        ),
    ];
    if vibemud_db::vibe_fever_active() {
        lines.insert(2, panel_line_center(&rainbow_fever_label(), inner));
    }
    lines
}

fn render_stats_dashboard(
    _conn: &rusqlite::Connection,
    snapshot: &vibemud_core::GameSnapshot,
    dto: &StatusLineDto,
    inner: usize,
    height: usize,
    lang: UiLanguage,
) -> String {
    let player = &snapshot.player;
    let border = format!("+{}+", "-".repeat(inner));
    let hero = hero_sprite(snapshot, &[]);
    let mut lines = vec![
        border.clone(),
        panel_line_center(
            label(lang, "캐릭터 상세 능력치", "CHARACTER DETAILS"),
            inner,
        ),
        panel_line_center(
            integration_hint(
                lang,
                "닫기: mudctl stats close",
                "Close: mudctl stats close",
                "닫기: /vibemud:mud x  |  mudctl stats close",
                "Close: /vibemud:mud x  |  mudctl stats close",
            ),
            inner,
        ),
        border.clone(),
    ];

    for row in hero {
        lines.push(panel_line_center(row, inner));
    }
    lines.extend([
        panel_line_center(
            &format!(
                "{}: Lv.{}    {}: {}",
                label(lang, "레벨", "Level"),
                player.level,
                label(lang, "직업", "Class"),
                dto.class_label
            ),
            inner,
        ),
        panel_line_raw(
            &format!(
                "{}: {}    {}: {}    {}: {}",
                label(lang, "지역", "Area"),
                dto.area_label,
                label(lang, "상태", "Mode"),
                dto.mode_label,
                label(lang, "위험", "Danger"),
                dto.danger_label
            ),
            inner,
        ),
        border.clone(),
        panel_line_center(label(lang, "장착 장비", "EQUIPMENT"), inner),
    ]);
    lines.extend(
        equipment_rows(snapshot, inner, lang)
            .into_iter()
            .map(|row| panel_line_raw(&row, inner)),
    );
    lines.extend([
        border.clone(),
        panel_line_center(label(lang, "스탯", "STATS"), inner),
    ]);
    lines.extend(
        stat_rows(snapshot, dto, inner, lang)
            .into_iter()
            .map(|row| panel_line_raw(&row, inner)),
    );

    while lines.len() + 1 < height {
        lines.push(panel_line_raw("", inner));
    }
    lines.push(border);
    lines.join("\n")
}

fn render_map_dashboard(
    conn: &rusqlite::Connection,
    snapshot: &vibemud_core::GameSnapshot,
    dto: &StatusLineDto,
    inner: usize,
    height: usize,
    lang: UiLanguage,
) -> Result<String> {
    let border = format!("+{}+", "-".repeat(inner));
    let mut lines = vec![
        border.clone(),
        panel_line_center(label(lang, "지역 / 던전 지도", "AREA / DUNGEON MAP"), inner),
        panel_line_center(
            integration_hint(
                lang,
                "닫기: mudctl x  |  사냥: mudctl a <지역/던전>",
                "Close: mudctl x  |  Hunt: mudctl a <area/dungeon>",
                "닫기: /vibemud:mud x  |  사냥: /vibemud:mud a <지역/던전>",
                "Close: /vibemud:mud x  |  Hunt: /vibemud:mud a <area/dungeon>",
            ),
            inner,
        ),
        panel_line_raw(
            &format!(
                "{}: {} | {}: {} | HP {}/{}",
                label(lang, "현재 지역", "Current"),
                dto.area_label,
                label(lang, "상태", "Mode"),
                dto.mode_label,
                snapshot.player.hp,
                snapshot.player.max_hp
            ),
            inner,
        ),
        border.clone(),
        panel_line_center(label(lang, "세계 연결도", "WORLD ROUTES"), inner),
    ];

    for row in map_ascii_lines(lang) {
        lines.push(panel_line_raw(row, inner));
    }
    lines.extend([
        border.clone(),
        panel_line_center(label(lang, "범례", "LEGEND"), inner),
    ]);
    for row in map_legend_rows(conn, lang)? {
        lines.push(panel_line_raw(
            &clip_display(&row, inner.saturating_sub(2)),
            inner,
        ));
    }

    append_feedback_footer(conn, &mut lines, inner, height, lang);
    while lines.len() + 1 < height {
        lines.push(panel_line_raw("", inner));
    }
    lines.push(border);
    Ok(lines.join("\n"))
}

fn render_quest_dashboard(
    conn: &rusqlite::Connection,
    snapshot: &vibemud_core::GameSnapshot,
    dto: &StatusLineDto,
    inner: usize,
    height: usize,
    lang: UiLanguage,
) -> Result<String> {
    let border = format!("+{}+", "-".repeat(inner));
    let quests = vibemud_db::load_daily_quests(conn)?;
    let completed = quests
        .iter()
        .filter(|quest| quest.status == "completed")
        .count();
    let claimed = quests
        .iter()
        .filter(|quest| quest.status == "claimed")
        .count();
    let mut lines = vec![
        border.clone(),
        panel_line_center(label(lang, "일일 퀘스트", "DAILY QUESTS"), inner),
        panel_line_center(
            integration_hint(
                lang,
                "닫기: mudctl x  |  보상: mudctl quest claim <id>",
                "Close: mudctl x  |  Claim: mudctl quest claim <id>",
                "닫기: /vibemud:mud x  |  보상: /vibemud:mud q 에서 선택 수령",
                "Close: /vibemud:mud x  |  Claim: choose from /vibemud:mud q",
            ),
            inner,
        ),
        panel_line_raw(
            &format!(
                "{}: Lv.{} {} | {}: {completed}/{claimed} | {} {}/{}",
                label(lang, "캐릭터", "Hero"),
                snapshot.player.level,
                dto.class_label,
                label(lang, "완료/수령", "Done/Claimed"),
                "HP",
                snapshot.player.hp,
                snapshot.player.max_hp
            ),
            inner,
        ),
        border.clone(),
    ];

    for quest in quests {
        let state = quest_status_label(&quest.status, lang);
        let progress = quest.progress.min(quest.target);
        lines.push(panel_line_raw(
            &format!("[{}] {}", state, quest.title),
            inner,
        ));
        lines.push(panel_line_raw(
            &format!(
                "  {}/{} · {} · FEVERTIME +{}{}",
                progress,
                quest.target,
                quest_reward_label(&quest.reward_kind, quest.reward_amount, lang),
                quest.fever_minutes,
                label(lang, "분", "m")
            ),
            inner,
        ));
    }

    lines.push(border.clone());
    lines.push(panel_line_raw(
        label(
            lang,
            "완료 보상 일괄 수령: mudctl quest claim-all",
            "Claim all completed rewards: mudctl quest claim-all",
        ),
        inner,
    ));

    append_feedback_footer(conn, &mut lines, inner, height, lang);
    while lines.len() + 1 < height {
        lines.push(panel_line_raw("", inner));
    }
    lines.push(border);
    Ok(lines.join("\n"))
}

fn map_ascii_lines(lang: UiLanguage) -> Vec<&'static str> {
    if lang == UiLanguage::Ko {
        vec![
            "아르카디아: [마을]-[훈련]",
            "           [고블굴]-[숲길]-[광산]-[수정굴]",
            "                     [늪지]-[요새]-[리치묘]",
            "                     ↓ 아틀라스 대륙으로 이동",
            "아틀라스:   [대장간]-[흑요]-[초원]",
            "                     [유적]-[메두사]-[스틱스]",
            "                     [관문]-[금고]",
        ]
    } else {
        vec![
            "Arcadia: [Town]-[Train]",
            "         [GobDen]-[Forest]-[Mine]-[Crystal]",
            "                  [Swamp]-[Fortress]-[Lich]",
            "                  ↓ Travel to Atlas Continent",
            "Atlas:   [Forge]-[Obsidian]-[Steppe]",
            "                  [Oracle]-[Medusa]-[Styx]",
            "                  [Olympus]-[Vault]",
        ]
    }
}

fn map_legend_rows(conn: &rusqlite::Connection, lang: UiLanguage) -> Result<Vec<String>> {
    let arcadia_areas = [
        "training-field",
        "forest-edge",
        "old-mine",
        "misty-swamp",
        "fallen-fortress",
    ];
    let atlas_areas = [
        "obsidian-coast",
        "titan-steppe",
        "oracle-ruins",
        "styx-marsh",
        "olympus-gate",
    ];
    let arcadia_dungeons = ["goblin-den", "crystal-cave", "lich-tomb"];
    let atlas_dungeons = ["cyclops-forge", "medusa-temple", "titan-vault"];
    let arcadia_area_summary = arcadia_areas
        .iter()
        .map(|id| {
            format!(
                "{}{}",
                short_place_label(id, lang),
                area_level(conn, id).unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let atlas_area_summary = atlas_areas
        .iter()
        .map(|id| {
            format!(
                "{}{}",
                short_place_label(id, lang),
                area_level(conn, id).unwrap_or(0)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let arcadia_dungeon_summary = arcadia_dungeons
        .iter()
        .map(|id| {
            let (level, floors) = dungeon_level_floors(conn, id).unwrap_or((0, 0));
            format!("{}{}/{}F", short_place_label(id, lang), level, floors)
        })
        .collect::<Vec<_>>()
        .join(" ");
    let atlas_dungeon_summary = atlas_dungeons
        .iter()
        .map(|id| {
            let (level, floors) = dungeon_level_floors(conn, id).unwrap_or((0, 0));
            format!("{}{}/{}F", short_place_label(id, lang), level, floors)
        })
        .collect::<Vec<_>>()
        .join(" ");

    Ok(vec![
        integration_hint(
            lang,
            "명령: mudctl a 숲길  /  mudctl a 흑요  /  mudctl a 메두사",
            "Command: mudctl a Forest  /  mudctl a Obsidian  /  mudctl a Medusa",
            "명령: /vibemud:mud a 숲길  /  /vibemud:mud a 흑요  /  /vibemud:mud a 메두사",
            "Command: /vibemud:mud a Forest  /  /vibemud:mud a Obsidian  /  /vibemud:mud a Medusa",
        )
        .to_string(),
        format!(
            "{}: {arcadia_area_summary} | {arcadia_dungeon_summary}",
            label(lang, "아르카디아", "Arcadia")
        ),
        format!(
            "{}: {atlas_area_summary} | {atlas_dungeon_summary}",
            label(lang, "아틀라스", "Atlas")
        ),
        format!(
            "{} 숲:{} 광:{} 흑:{} 메:{}",
            label(lang, "몹", "Mob"),
            short_monster_codes_for_area(conn, "forest-edge", lang)?,
            short_monster_codes_for_area(conn, "old-mine", lang)?,
            short_monster_codes_for_area(conn, "obsidian-coast", lang)?,
            short_monster_codes_for_dungeon(conn, "medusa-temple", lang)?
        ),
    ])
}

fn area_level(conn: &rusqlite::Connection, area_id: &str) -> Result<i64> {
    conn.query_row(
        "SELECT recommended_level FROM areas WHERE id = ?1",
        [area_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn dungeon_level_floors(conn: &rusqlite::Connection, dungeon_id: &str) -> Result<(i64, i64)> {
    conn.query_row(
        "SELECT recommended_level, floors FROM dungeons WHERE id = ?1",
        [dungeon_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .map_err(Into::into)
}

fn short_monster_codes_for_area(
    conn: &rusqlite::Connection,
    area_id: &str,
    lang: UiLanguage,
) -> Result<String> {
    short_monster_codes_for_scope(
        conn,
        "SELECT m.id FROM area_monsters am JOIN monsters m ON m.id = am.monster_id WHERE am.area_id = ?1 ORDER BY am.weight DESC, m.id LIMIT 3",
        area_id,
        lang,
    )
}

fn short_monster_codes_for_dungeon(
    conn: &rusqlite::Connection,
    dungeon_id: &str,
    lang: UiLanguage,
) -> Result<String> {
    short_monster_codes_for_scope(
        conn,
        "SELECT m.id FROM dungeon_monsters dm JOIN monsters m ON m.id = dm.monster_id WHERE dm.dungeon_id = ?1 ORDER BY dm.weight DESC, m.id LIMIT 3",
        dungeon_id,
        lang,
    )
}

fn short_monster_codes_for_scope(
    conn: &rusqlite::Connection,
    sql: &str,
    scope_id: &str,
    lang: UiLanguage,
) -> Result<String> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([scope_id], |row| row.get::<_, String>(0))?;
    let ids: Vec<String> = rows.collect::<rusqlite::Result<_>>()?;
    if ids.is_empty() {
        return Ok("-".to_string());
    }
    Ok(ids
        .iter()
        .map(|id| short_monster_label(id, lang))
        .collect::<Vec<_>>()
        .join("/"))
}

fn short_place_label(id: &str, lang: UiLanguage) -> &'static str {
    match (lang, id) {
        (UiLanguage::Ko, "training-field") => "훈련",
        (UiLanguage::Ko, "forest-edge") => "숲길",
        (UiLanguage::Ko, "old-mine") => "광산",
        (UiLanguage::Ko, "misty-swamp") => "늪지",
        (UiLanguage::Ko, "fallen-fortress") => "요새",
        (UiLanguage::Ko, "obsidian-coast") => "흑요",
        (UiLanguage::Ko, "titan-steppe") => "초원",
        (UiLanguage::Ko, "oracle-ruins") => "유적",
        (UiLanguage::Ko, "styx-marsh") => "스틱스",
        (UiLanguage::Ko, "olympus-gate") => "관문",
        (UiLanguage::Ko, "goblin-den") => "고블",
        (UiLanguage::Ko, "crystal-cave") => "수정",
        (UiLanguage::Ko, "lich-tomb") => "리치",
        (UiLanguage::Ko, "cyclops-forge") => "대장",
        (UiLanguage::Ko, "medusa-temple") => "메두",
        (UiLanguage::Ko, "titan-vault") => "금고",
        (_, "training-field") => "Train",
        (_, "forest-edge") => "Forest",
        (_, "old-mine") => "Mine",
        (_, "misty-swamp") => "Swamp",
        (_, "fallen-fortress") => "Fort",
        (_, "obsidian-coast") => "Obs",
        (_, "titan-steppe") => "Steppe",
        (_, "oracle-ruins") => "Oracle",
        (_, "styx-marsh") => "Styx",
        (_, "olympus-gate") => "Olympus",
        (_, "goblin-den") => "Gob",
        (_, "crystal-cave") => "Cry",
        (_, "lich-tomb") => "Lich",
        (_, "cyclops-forge") => "Forge",
        (_, "medusa-temple") => "Medusa",
        (_, "titan-vault") => "Vault",
        _ => "?",
    }
}

fn short_monster_label(id: &str, lang: UiLanguage) -> &'static str {
    match (lang, id) {
        (UiLanguage::Ko, "training-scarab") => "딱",
        (UiLanguage::Ko, "target-golem") => "골",
        (UiLanguage::Ko, "twig-imp") => "임",
        (UiLanguage::Ko, "moss-wolf") => "늑",
        (UiLanguage::Ko, "goblin-scout") => "고",
        (UiLanguage::Ko, "mine-bat") => "박",
        (UiLanguage::Ko, "ore-crawler") => "광",
        (UiLanguage::Ko, "crystal-wisp") => "위",
        (UiLanguage::Ko, "swamp-toad") => "독",
        (UiLanguage::Ko, "bog-witchling") => "마",
        (UiLanguage::Ko, "bone-guard") => "해",
        (UiLanguage::Ko, "fallen-knight") => "기",
        (UiLanguage::Ko, "goblin-brute") => "난",
        (UiLanguage::Ko, "crystal-gazer") => "응",
        (UiLanguage::Ko, "lich-acolyte") => "시",
        (UiLanguage::Ko, "ash-siren") => "세",
        (UiLanguage::Ko, "obsidian-crab") => "게",
        (UiLanguage::Ko, "cyclops-apprentice") => "키",
        (UiLanguage::Ko, "titan-raider") => "약",
        (UiLanguage::Ko, "bronze-hoplite") => "청",
        (UiLanguage::Ko, "oracle-sphinx") => "스",
        (UiLanguage::Ko, "gorgon-sentinel") => "고",
        (UiLanguage::Ko, "styx-ferryman") => "사",
        (UiLanguage::Ko, "eidolon-guard") => "망",
        (UiLanguage::Ko, "olympus-sentinel") => "올",
        (UiLanguage::Ko, "titan-warden") => "금",
        (_, "training-scarab") => "Scrb",
        (_, "target-golem") => "Golem",
        (_, "twig-imp") => "Imp",
        (_, "moss-wolf") => "Wolf",
        (_, "goblin-scout") => "Gob",
        (_, "mine-bat") => "Bat",
        (_, "ore-crawler") => "Ore",
        (_, "crystal-wisp") => "Wisp",
        (_, "swamp-toad") => "Toad",
        (_, "bog-witchling") => "Witch",
        (_, "bone-guard") => "Bone",
        (_, "fallen-knight") => "Knight",
        (_, "goblin-brute") => "Brute",
        (_, "crystal-gazer") => "Gazer",
        (_, "lich-acolyte") => "Acolyte",
        (_, "ash-siren") => "Siren",
        (_, "obsidian-crab") => "Crab",
        (_, "cyclops-apprentice") => "Cyclops",
        (_, "titan-raider") => "Raider",
        (_, "bronze-hoplite") => "Hoplite",
        (_, "oracle-sphinx") => "Sphinx",
        (_, "gorgon-sentinel") => "Gorgon",
        (_, "styx-ferryman") => "Styx",
        (_, "eidolon-guard") => "Eidolon",
        (_, "olympus-sentinel") => "Olympus",
        (_, "titan-warden") => "Warden",
        _ => "?",
    }
}

fn append_feedback_footer(
    conn: &rusqlite::Connection,
    lines: &mut Vec<String>,
    inner: usize,
    height: usize,
    lang: UiLanguage,
) {
    let footer_rows = 5usize.min(height.saturating_sub(lines.len() + 3));
    if footer_rows == 0 {
        return;
    }
    let border = format!("+{}+", "-".repeat(inner));
    while lines.len() + footer_rows + 3 < height {
        lines.push(panel_line_raw("", inner));
    }
    lines.push(border);
    lines.push(panel_line_raw(
        label(lang, "액션 피드백", "ACTION FEEDBACK"),
        inner,
    ));
    if let Ok(logs) = vibemud_db::recent_log_entries(conn, footer_rows.max(3)) {
        for entry in logs.iter().take(footer_rows).rev() {
            lines.push(panel_line_raw(&panel_log_line(entry, lang), inner));
        }
    }
}

fn equipment_rows(
    snapshot: &vibemud_core::GameSnapshot,
    inner: usize,
    lang: UiLanguage,
) -> Vec<String> {
    [
        ("weapon", label(lang, "무기", "Weapon")),
        ("subweapon", label(lang, "부무기", "Subweapon")),
        ("armor_top", label(lang, "상의", "Top")),
        ("armor_bottom", label(lang, "하의", "Bottom")),
        ("trinket", label(lang, "장신구", "Trinket")),
        ("boots", label(lang, "신발", "Boots")),
        ("pet", label(lang, "펫", "Pet")),
        ("special", label(lang, "특수장비", "Special")),
    ]
    .into_iter()
    .map(|(slot, label_text)| equipment_line(snapshot, slot, label_text, inner, lang))
    .collect()
}

fn equipment_line(
    snapshot: &vibemud_core::GameSnapshot,
    slot: &str,
    label_text: &str,
    inner: usize,
    lang: UiLanguage,
) -> String {
    let label_width = if lang == UiLanguage::Ko { 10 } else { 12 };
    let value_width = inner.saturating_sub(label_width + 5).max(12);
    let value = snapshot
        .inventory
        .iter()
        .find(|item| item.equipped_slot.as_deref() == Some(slot))
        .map(|item| equipment_detail_name(item, lang))
        .unwrap_or_else(|| label(lang, "미착용", "Unequipped").to_string());
    format!(
        "  {} {}",
        pad_chars(label_text, label_width),
        pad_chars(&clip_chars(&value, value_width), value_width)
    )
}

fn equipment_detail_name(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> String {
    let mut display = localized_item_name(&item.name, lang);
    display = format!("{} +{}", display, item.enhancement_level);
    if let Some(tier) = item.tier {
        display = format!("{} T{}", display, tier);
    }
    if item.locked {
        display = format!("{} {display}", label(lang, "[잠금]", "[LOCKED]"));
    }
    colorize_by_rarity(
        &display,
        item.rarity_color.as_deref().unwrap_or(&item.rarity),
    )
}

fn stat_rows(
    snapshot: &vibemud_core::GameSnapshot,
    dto: &StatusLineDto,
    inner: usize,
    lang: UiLanguage,
) -> Vec<String> {
    let player = &snapshot.player;
    let stats = vibemud_core::representative_stats(player);
    let rows = [
        (
            label(lang, "전투력", "Power"),
            stats.combat_power.to_string(),
            label(lang, "공격력", "Attack"),
            player.attack.to_string(),
        ),
        (
            label(lang, "방어력", "Defense"),
            player.defense.to_string(),
            label(lang, "속도", "Speed"),
            player.speed.to_string(),
        ),
        (
            "HP",
            format!("{}/{}", player.hp.max(0), player.max_hp),
            "MP",
            format!("{}/{}", player.mp.max(0), player.max_mp),
        ),
        (
            label(lang, "경험치", "XP"),
            format!("{}/{}", player.xp, player.xp_to_next),
            label(lang, "골드", "Gold"),
            player.gold.to_string(),
        ),
        (
            label(lang, "명중", "Accuracy"),
            player.accuracy.to_string(),
            label(lang, "저항", "Resistance"),
            player.evasion.to_string(),
        ),
        (
            label(lang, "행운", "Luck"),
            player.luck.to_string(),
            label(lang, "재생", "Regen"),
            player.regen.to_string(),
        ),
        (
            label(lang, "전리품", "Loot"),
            snapshot.inventory.len().to_string(),
            label(lang, "파티", "Party"),
            format!("{}/4", dto.party_count),
        ),
    ];
    rows.into_iter()
        .map(|(ll, lv, rl, rv)| table_pair_line(ll, &lv, rl, &rv, inner))
        .collect()
}

fn table_pair_line(
    left_label: &str,
    left_value: &str,
    right_label: &str,
    right_value: &str,
    inner: usize,
) -> String {
    let usable = inner.saturating_sub(8).max(24);
    let half = usable / 2;
    format!(
        "  {}  {}",
        table_cell(left_label, left_value, half),
        table_cell(right_label, right_value, half)
    )
}

fn header_pair_line(
    left_label: &str,
    left_value: &str,
    right_label: &str,
    right_value: &str,
    inner: usize,
) -> String {
    let usable = inner.saturating_sub(2).max(24);
    let separator = " │ ";
    let separator_width = display_width(separator);
    let left_width = usable.saturating_sub(separator_width) / 2;
    let right_width = usable
        .saturating_sub(separator_width)
        .saturating_sub(left_width);
    format!(
        "{}{}{}",
        header_cell(left_label, left_value, left_width),
        colorize(separator, AnsiColor::Gray),
        header_cell(right_label, right_value, right_width)
    )
}

fn table_cell(label_text: &str, value: &str, width: usize) -> String {
    table_cell_with_label(label_text, value, width, false)
}

fn header_cell(label_text: &str, value: &str, width: usize) -> String {
    table_cell_with_label(label_text, value, width, true)
}

fn table_cell_with_label(label_text: &str, value: &str, width: usize, dim_label: bool) -> String {
    let label_width = (width / 3).clamp(8, 12);
    let value_width = width.saturating_sub(label_width + 1).max(6);
    let label = clip_chars(label_text, label_width);
    let label = if dim_label {
        colorize(&label, AnsiColor::Gray)
    } else {
        label
    };
    format!(
        "{} {}",
        pad_chars(&label, label_width),
        pad_chars(&clip_chars(value, value_width), value_width)
    )
}

fn localized_item_name(name: &str, lang: UiLanguage) -> String {
    let renamed = match name {
        "Small Potion" | "Asclepius Potion" => Some(("아스클레피오스 물약", "Asclepius Potion")),
        "Medium Potion" | "Hygieia Potion" => Some(("히게이아 물약", "Hygieia Potion")),
        "Basic Sword" | "Ares Sword" => Some(("아레스 검", "Ares Sword")),
        "Basic Staff" | "Hermes Staff" => Some(("헤르메스 지팡이", "Hermes Staff")),
        "Leather Armor" | "Leonidas Armor" => Some(("레오니다스 갑옷", "Leonidas Armor")),
        "Repair Kit" | "Daedalus Kit" => Some(("다이달로스 도구", "Daedalus Kit")),
        "Goblin Chief Axe" | "Hector Axe" => Some(("헥토르 도끼", "Hector Axe")),
        "Crystal Blade" | "Perseus Blade" => Some(("페르세우스 칼날", "Perseus Blade")),
        "Lich Amulet" | "Hecate Amulet" => Some(("헤카테 부적", "Hecate Amulet")),
        "Hephaestus Hammer" => Some(("헤파이스토스 망치", "Hephaestus Hammer")),
        "Athena Aegis" => Some(("아테나 아이기스", "Athena Aegis")),
        "Kronos Key" => Some(("크로노스 열쇠", "Kronos Key")),
        _ => None,
    };
    if let Some((ko, en)) = renamed {
        match lang {
            UiLanguage::Ko => ko,
            UiLanguage::En => en,
        }
    } else {
        name
    }
    .to_string()
}

fn inventory_item_line(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> String {
    let name = equipment_display_name(item, lang);
    let equipped = item
        .equipped_slot
        .as_ref()
        .map(|slot| format!(" [{}]", localized_slot(slot, lang)))
        .unwrap_or_default();
    let stat_parts = equipment_stat_parts(item, lang);
    let stats = if stat_parts.is_empty() {
        String::new()
    } else {
        format!(
            " | {} | power {}",
            stat_parts.join(" / "),
            item.power_score.unwrap_or(0)
        )
    };
    if item.quantity > 1 {
        format!("{name}{equipped} x{}{}", item.quantity, stats)
    } else {
        format!("{name}{equipped}{stats}")
    }
}

fn equipment_display_name(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> String {
    let mut display = localized_item_name(&item.name, lang);
    if item.enhancement_level > 0 {
        display = format!("{} +{}", display, item.enhancement_level);
    }
    if let Some(tier) = item.tier {
        display = format!("{} T{}", display, tier);
    }
    colorize_by_rarity(
        &display,
        item.rarity_color.as_deref().unwrap_or(&item.rarity),
    )
}

fn localized_slot(slot: &str, lang: UiLanguage) -> &'static str {
    match (lang, slot) {
        (UiLanguage::Ko, "weapon") => "무기",
        (UiLanguage::Ko, "subweapon") => "부무기",
        (UiLanguage::Ko, "armor") | (UiLanguage::Ko, "armor_top") => "상의",
        (UiLanguage::Ko, "armor_bottom") => "하의",
        (UiLanguage::Ko, "offhand") => "부무기",
        (UiLanguage::Ko, "trinket") => "장신구",
        (UiLanguage::Ko, "boots") => "신발",
        (UiLanguage::Ko, "pet") => "펫",
        (UiLanguage::Ko, "special") => "특수",
        (UiLanguage::Ko, "bag") => "소지",
        (_, "weapon") => "Weapon",
        (_, "subweapon") => "Subweapon",
        (_, "armor") | (_, "armor_top") => "Top",
        (_, "armor_bottom") => "Bottom",
        (_, "offhand") => "Subweapon",
        (_, "trinket") => "Trinket",
        (_, "boots") => "Boots",
        (_, "pet") => "Pet",
        (_, "special") => "Special",
        (_, "bag") => "Bag",
        _ => "Slot",
    }
}

fn localized_stat(stat: &str, lang: UiLanguage) -> &'static str {
    match (lang, stat) {
        (UiLanguage::Ko, "attack") => "공격",
        (UiLanguage::Ko, "defense") => "방어",
        (UiLanguage::Ko, "accuracy") => "명중",
        (UiLanguage::Ko, "evasion") => "저항",
        (UiLanguage::Ko, "speed") => "속도",
        (UiLanguage::Ko, "regen") => "재생",
        (UiLanguage::Ko, "luck") => "행운",
        (UiLanguage::Ko, "max_hp") => "최대HP",
        (UiLanguage::Ko, "max_mp") => "최대MP",
        (UiLanguage::Ko, "xp_bonus") => "경험치획득",
        (UiLanguage::Ko, "gold_bonus") => "골드획득",
        (_, "attack") => "ATK",
        (_, "defense") => "DEF",
        (_, "accuracy") => "ACC",
        (_, "evasion") => "RES",
        (_, "speed") => "SPD",
        (_, "regen") => "REG",
        (_, "luck") => "LUCK",
        (_, "max_hp") => "Max HP",
        (_, "max_mp") => "Max MP",
        (_, "xp_bonus") => "XP Gain",
        (_, "gold_bonus") => "Gold Gain",
        _ => "STAT",
    }
}

fn character_age_line(clock_tick: u64, lang: UiLanguage) -> String {
    let (years, days, hour) = character_age_parts(clock_tick);
    if lang == UiLanguage::Ko {
        format!("나이 {years}세 {days}일 {hour:02}시")
    } else {
        format!("Age {years}y {days}d {hour:02}h")
    }
}

fn character_age_parts(clock_tick: u64) -> (u64, u64, u64) {
    let total_hours = clock_tick / 20;
    let total_days = total_hours / 24;
    let years = total_days / 360;
    let days = total_days % 360;
    let hour = total_hours % 24;
    (years, days, hour)
}

fn header_with_right(title: &str, right: &str, inner: usize) -> String {
    let available = inner.saturating_sub(2);
    let title_width = display_width(title);
    let right_width = display_width(right);
    if title_width + right_width + 2 <= available {
        format!(
            "{}{}{}",
            title,
            " ".repeat(available - title_width - right_width),
            right
        )
    } else {
        format!("{} {}", title, right)
    }
}

fn panel_line_center(value: &str, inner: usize) -> String {
    let clipped = clip_chars(value, inner.saturating_sub(2));
    let len = display_width(&clipped);
    let width = inner.saturating_sub(2);
    if len >= width {
        return format!("| {clipped} |");
    }
    let left = (width - len) / 2;
    let right = width - len - left;
    format!("| {}{}{} |", " ".repeat(left), clipped, " ".repeat(right))
}

fn scene_section_height(height: usize, log_section_lines: usize) -> usize {
    let available = height.saturating_sub(log_section_lines + 9);
    available.clamp(10, 14)
}

fn render_auto_hunt_scene(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    conn: &rusqlite::Connection,
    inner: usize,
    section_height: usize,
    lang: UiLanguage,
) -> Vec<String> {
    let mut lines = Vec::with_capacity(section_height);
    let border = format!("+{}+", "-".repeat(inner));
    lines.push(border.clone());
    lines.push(panel_line_raw(
        &header_with_right(
            label(lang, "자동 사냥", "AUTO-HUNT SCENE"),
            &hunt_progress_badge(conn, snapshot, lang),
            inner,
        ),
        inner,
    ));
    lines.extend(auto_hunt_scene_rows(
        snapshot,
        logs,
        inner,
        section_height.saturating_sub(3),
        lang,
    ));
    while lines.len() + 1 < section_height {
        lines.push(panel_line_raw("", inner));
    }
    lines.push(border);
    lines
}

fn hunt_progress_badge(
    conn: &rusqlite::Connection,
    snapshot: &vibemud_core::GameSnapshot,
    lang: UiLanguage,
) -> String {
    let location = if snapshot.player.mode == "dungeon" {
        snapshot
            .player
            .current_dungeon_id
            .as_deref()
            .map(|id| short_dungeon_badge(id, lang))
            .unwrap_or_else(|| short_area_badge(snapshot.player.current_area_id.as_deref(), lang))
    } else {
        short_area_badge(snapshot.player.current_area_id.as_deref(), lang)
    };
    let point_key = if snapshot.player.mode == "dungeon" {
        "game.dungeon_point"
    } else {
        "game.auto_hunt_point"
    };
    let point = vibemud_db::setting_value(conn, point_key)
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        .min(10);
    format!("[{location}-{point}/10]")
}

fn short_area_badge(area_id: Option<&str>, lang: UiLanguage) -> String {
    match (lang, area_id.unwrap_or("training-field")) {
        (UiLanguage::Ko, "training-field") => "훈련".to_string(),
        (UiLanguage::Ko, "forest-edge") => "숲길".to_string(),
        (UiLanguage::Ko, "old-mine") => "광산".to_string(),
        (UiLanguage::Ko, "misty-swamp") => "늪지".to_string(),
        (UiLanguage::Ko, "fallen-fortress") => "요새".to_string(),
        (UiLanguage::Ko, "obsidian-coast") => "흑요".to_string(),
        (UiLanguage::Ko, "titan-steppe") => "초원".to_string(),
        (UiLanguage::Ko, "oracle-ruins") => "유적".to_string(),
        (UiLanguage::Ko, "styx-marsh") => "스틱스".to_string(),
        (UiLanguage::Ko, "olympus-gate") => "관문".to_string(),
        (_, "training-field") => "Train".to_string(),
        (_, "forest-edge") => "Forest".to_string(),
        (_, "old-mine") => "Mine".to_string(),
        (_, "misty-swamp") => "Swamp".to_string(),
        (_, "fallen-fortress") => "Fort".to_string(),
        (_, "obsidian-coast") => "Obs".to_string(),
        (_, "titan-steppe") => "Steppe".to_string(),
        (_, "oracle-ruins") => "Oracle".to_string(),
        (_, "styx-marsh") => "Styx".to_string(),
        (_, "olympus-gate") => "Olympus".to_string(),
        (_, other) => other.to_string(),
    }
}

fn short_dungeon_badge(dungeon_id: &str, lang: UiLanguage) -> String {
    match (lang, dungeon_id) {
        (UiLanguage::Ko, "goblin-den") => "고블굴".to_string(),
        (UiLanguage::Ko, "crystal-cave") => "수정굴".to_string(),
        (UiLanguage::Ko, "lich-tomb") => "리치묘".to_string(),
        (UiLanguage::Ko, "cyclops-forge") => "대장간".to_string(),
        (UiLanguage::Ko, "medusa-temple") => "메두사".to_string(),
        (UiLanguage::Ko, "titan-vault") => "금고".to_string(),
        (_, "goblin-den") => "GobDen".to_string(),
        (_, "crystal-cave") => "Crystal".to_string(),
        (_, "lich-tomb") => "Lich".to_string(),
        (_, "cyclops-forge") => "Forge".to_string(),
        (_, "medusa-temple") => "Medusa".to_string(),
        (_, "titan-vault") => "Vault".to_string(),
        (_, other) => other.to_string(),
    }
}

fn auto_hunt_scene_rows(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    inner: usize,
    rows: usize,
    lang: UiLanguage,
) -> Vec<String> {
    let scene_width = inner.saturating_sub(4).max(24);
    let mut out = Vec::new();
    let caption = render_combat_caption(snapshot, logs, lang);
    if rows <= 1 {
        out.push(panel_line_raw(&caption, inner));
        return out;
    }

    let ground = format!("{}{}", "-".repeat(scene_width.saturating_sub(2)), ">");
    let mut sprite_rows = render_combat_sprite_rows(snapshot, logs, scene_width);
    let hp_rows = render_combat_hp_rows(snapshot, logs, scene_width, lang);
    let reserved_rows = 1 + hp_rows.len() + 1;
    let available_sprite_rows = rows.saturating_sub(reserved_rows).max(1);
    if sprite_rows.len() > available_sprite_rows {
        sprite_rows.truncate(available_sprite_rows);
    }
    let row_count_with_caption = sprite_rows.len() + 1 + hp_rows.len() + 1;
    let has_room_for_spacer = rows > row_count_with_caption;
    if rows >= 8 && sprite_rows.len() <= 4 && has_room_for_spacer {
        out.push(panel_line_raw("", inner));
    }
    for row in sprite_rows {
        out.push(panel_line_raw(&row, inner));
    }
    out.push(panel_line_raw(&ground, inner));
    for row in hp_rows {
        out.push(panel_line_raw(&row, inner));
    }
    if rows > out.len() {
        out.push(panel_line_raw(&caption, inner));
    }
    out.truncate(rows);
    out
}

fn render_combat_sprite_rows(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    width: usize,
) -> Vec<String> {
    render_combat_sprite_rows_at_phase(snapshot, logs, width, combat_animation_phase(snapshot))
}

fn render_combat_sprite_rows_at_phase(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    width: usize,
    phase: u64,
) -> Vec<String> {
    let hero = hero_sprite_with_phase(snapshot, logs, phase);
    let monster_state = monster_scene_state(snapshot, logs);
    let monster_kind = monster_scene_kind(snapshot, logs);
    let monster = monster_sprite(monster_state, monster_kind);
    let monster_width = monster
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let hero_width = hero
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let monster_pos = width.saturating_sub(monster_width + 2);
    let hero_pos = hero_scene_position(
        snapshot,
        monster_state,
        monster_pos,
        hero_width,
        monster_width,
        width,
        phase,
    );
    let row_count = hero.len().max(monster.len()).max(4);
    let mut rows = vec![vec![' '; width]; row_count];
    let hero_row_offset = row_count.saturating_sub(hero.len());
    let monster_row_offset = row_count.saturating_sub(monster.len());
    for (row, value) in hero.iter().enumerate() {
        place_chars(&mut rows[hero_row_offset + row], hero_pos, value);
    }
    if monster_state != MonsterSceneState::None {
        for (row, value) in monster.iter().enumerate() {
            place_chars(&mut rows[monster_row_offset + row], monster_pos, value);
        }
    }
    rows.into_iter()
        .map(|row| {
            let rendered = row.into_iter().collect::<String>();
            if monster_kind == MonsterKind::DookkaBurrower {
                style_dookka_sprite_row(&rendered)
            } else {
                rendered
            }
        })
        .collect()
}

fn hero_scene_position(
    snapshot: &vibemud_core::GameSnapshot,
    monster_state: MonsterSceneState,
    monster_pos: usize,
    hero_width: usize,
    monster_width: usize,
    width: usize,
    phase: u64,
) -> usize {
    if monster_state == MonsterSceneState::Alive {
        monster_pos.saturating_sub(hero_width + 4).max(1)
    } else if hero_is_exploring(snapshot) {
        let travel = width.saturating_sub(hero_width + monster_width + 8);
        1 + ((phase as usize) % travel.max(1))
    } else {
        1
    }
}

fn hero_sprite(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
) -> Vec<&'static str> {
    hero_sprite_with_phase(snapshot, logs, snapshot.state_version)
}

fn hero_sprite_with_phase(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    phase: u64,
) -> Vec<&'static str> {
    if hero_is_fallen(snapshot, logs) {
        return fallen_hero_sprite(visible_class_id(snapshot));
    }
    let moving = hero_is_exploring(snapshot);
    hero_sprite_for_class(visible_class_id(snapshot), moving, phase % 4)
}

fn hero_is_exploring(snapshot: &vibemud_core::GameSnapshot) -> bool {
    matches!(snapshot.player.mode.as_str(), "auto_hunt" | "dungeon")
}

fn combat_animation_phase(snapshot: &vibemud_core::GameSnapshot) -> u64 {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    snapshot.state_version.wrapping_add(seconds)
}

fn hero_is_fallen(snapshot: &vibemud_core::GameSnapshot, logs: &[vibemud_db::LogEntry]) -> bool {
    if snapshot.player.hp <= 0 || snapshot.player.mode == "recovering" {
        return true;
    }
    let current = snapshot.state_version;
    logs.iter()
        .filter(|entry| current.saturating_sub(entry.state_version) <= 4)
        .any(|entry| {
            let message = entry.message.to_ascii_lowercase();
            message.contains("fell in battle") || message.contains("recovery started")
        })
}

fn visible_class_id(snapshot: &vibemud_core::GameSnapshot) -> &str {
    if snapshot.player.class_id == "adventurer"
        || (snapshot.player.class_id == "warrior" && snapshot.player.level <= 10)
    {
        "adventurer"
    } else {
        &snapshot.player.class_id
    }
}

fn fallen_hero_sprite(class_id: &str) -> Vec<&'static str> {
    match class_id {
        "fighter" | "warrior" => vec!["  ╭─────╮", "  │▒xx▒│", "  │▒-▒▒│", "  ╰─██─╯", "    ---"],
        "robot" | "mage" | "glyph-sage" | "burrower-miner" => {
            vec!["  ╔════╗", "  ║x  x║", "  ║ -- ║", "  ╚╦══╦╝", "   ----"]
        }
        "priest" | "healer" => vec!["   ▒▒▒▒▒", " ▒▒x▒▒x▒▒", "   ▒ - ▒", "  ▜▒▒✚▒▒▛", "    ---"],
        "gangster" | "rogue" => vec!["   ▒▒▒▒▒", "  ▒x▒x▒", "   ▒-▒", "  ╭▒▒▒╮", "   ---"],
        "capitalist" | "wanderer" | "portrait-keeper" => {
            vec!["  ◎◎◎◎◎◎◎", " ◎▒▒▒▒▒▒▒◎", "  ▒x▒▒x▒", "   ▒ - ▒", "    ---"]
        }
        _ => vec![
            "    ▒▒▒▒▒",
            "  ▒▒x▒▒x▒▒",
            "    ▒ - ▒",
            "   /▒▒▒▒▒\\",
            "     ---",
        ],
    }
}

fn hero_sprite_for_class(class_id: &str, moving: bool, phase: u64) -> Vec<&'static str> {
    match class_id {
        "fighter" | "warrior" => fighter_sprite(moving, phase),
        "robot" | "mage" | "glyph-sage" | "burrower-miner" => robot_sprite(moving, phase),
        "priest" | "healer" => priest_sprite(moving, phase),
        "gangster" | "rogue" => gangster_sprite(moving, phase),
        "capitalist" | "wanderer" | "portrait-keeper" => capitalist_sprite(moving, phase),
        _ => adventurer_sprite(moving, phase),
    }
}

fn adventurer_sprite(moving: bool, phase: u64) -> Vec<&'static str> {
    if !moving {
        return vec![
            "    ▒▒▒▒▒",
            "  ▒▒◕▒▒◕▒▒",
            "    ▒ ▄ ▒",
            "   /▒▒▒▒▒\\",
            "     ▒█▒",
            "    /   \\",
        ];
    }
    match phase {
        0 => vec![
            "    ▒▒▒▒▒",
            "  ▒▒◕▒▒◕▒▒",
            "    ▒ ▄ ▒",
            "   /▒▒▒▒▒\\",
            "     ▒█▒",
            "    /   \\",
        ],
        1 => vec![
            "     ▒▒▒▒▒",
            "   ▒▒◕▒▒◕▒▒",
            "     ▒ ▄ ▒",
            "   ·/▒▒▒▒▒\\›",
            "      ▒█▒",
            "     ╱   ╲",
        ],
        2 => vec![
            "       ▒▒▒▒▒",
            "     ▒▒◕▒▒◕▒▒",
            "       ▒ ▄ ▒",
            "   ··╭▒▒▒▒▒╮»",
            "       ▒█▒",
            "        ╲ ╱",
        ],
        _ => vec![
            "      ▒▒▒▒▒",
            "    ▒▒◕▒▒◕▒▒",
            "      ▒ ▄ ▒",
            "   ·/▒▒▒▒▒\\»",
            "       ▒█▒",
            "      ╱   ╲",
        ],
    }
}

fn fighter_sprite(moving: bool, phase: u64) -> Vec<&'static str> {
    if !moving {
        return vec![
            "  ╭─────╮",
            "  │▒◕◕▒│  ◎",
            "  │▒▿▒▒│ ╱",
            "  ╰─██─╯╱",
            "    ▒▒",
            "   ╱  ╲",
        ];
    }
    match phase {
        0 => vec![
            "  ╭─────╮",
            "  │▒◕◕▒│  ◎",
            "  │▒▿▒▒│ ╱",
            "  ╰─██─╯╱",
            "    ▒▒",
            "   ╱  ╲",
        ],
        1 => vec![
            "   ╭─────╮",
            " · │▒◕◕▒│ ◎›",
            "   │▒▿▒▒│╱",
            "   ╰─██─╯",
            "     ▒▒",
            "    ╱ ╲",
        ],
        2 => vec![
            "     ╭─────╮      ◎",
            "  ·· │▒◕◕▒│    ╱»",
            "     │▒▿▒▒│   ╱",
            "     ╰─██─╯╲ ╱",
            "       ▒▒",
            "        ╲╱",
        ],
        _ => vec![
            "    ╭─────╮",
            " ·· │▒◕◕▒│  ◎»",
            "    │▒▿▒▒│ ╱",
            "    ╰─██─╯╱",
            "      ▒▒",
            "     ╱ ╲",
        ],
    }
}

fn robot_sprite(moving: bool, phase: u64) -> Vec<&'static str> {
    if !moving {
        return vec![
            "  ╔════╗",
            "  ║●  ●║   ✧◇✧",
            "  ║  ▣ ║╱  ◇✦◇",
            "  ╚╦══╦╝   ✧◇✧",
            "   ╨  ╨",
        ];
    }
    match phase {
        0 => vec![
            "  ╔════╗",
            "  ║●  ●║   ✧◇✧",
            "  ║  ▣ ║╱  ◇✦◇",
            "  ╚╦══╦╝   ✧◇✧",
            "   ╨  ╨",
        ],
        1 => vec![
            "   ╔════╗",
            " · ║●  ●║    ✦◇›",
            "   ║  ▣ ║╱   ◇✧",
            "   ╚╦══╦╝     ✦",
            "    ╨ ╨",
        ],
        2 => vec![
            "     ╔════╗",
            "  ·· ║●  ●║     ◇✧◇»",
            "     ║  ▣ ║╱    ✦◇✦",
            "     ╚╦══╦╝     ◇✧◇",
            "      ╨ ╨",
        ],
        _ => vec![
            "    ╔════╗",
            " ·· ║●  ●║    ✧◇»",
            "    ║  ▣ ║╱   ✦◇",
            "    ╚╦══╦╝     ✧",
            "     ╨ ╨",
        ],
    }
}

fn priest_sprite(moving: bool, phase: u64) -> Vec<&'static str> {
    if !moving {
        return vec![
            "   ▒▒▒▒▒",
            " ▒▒●▒▒●▒▒",
            "   ▒ ▿ ▒",
            "  ▜▒▒✚▒▒▛",
            "    ▒█▒☥",
            "   ╱   ╲",
        ];
    }
    match phase {
        0 => vec![
            "   ▒▒▒▒▒",
            " ▒▒●▒▒●▒▒",
            "   ▒ ▿ ▒",
            "  ▜▒▒✚▒▒▛",
            "    ▒█▒☥",
            "   ╱   ╲",
        ],
        1 => vec![
            "    ▒▒▒▒▒",
            "  ▒▒●▒▒●▒▒    ✧",
            "    ▒ ▿ ▒   ✚›",
            " · ▜▒▒✚▒▒▛",
            "     ▒█▒☥",
            "    ╱   ╲",
        ],
        2 => vec![
            "      ▒▒▒▒▒",
            "    ▒▒●▒▒●▒▒     ✧✚»",
            "      ▒ ▿ ▒    ✧",
            "  ·· ▜▒▒✚▒▒▛",
            "       ▒█▒☥",
            "        ╲ ╱",
        ],
        _ => vec![
            "     ▒▒▒▒▒",
            "   ▒▒●▒▒●▒▒   ✚»",
            "     ▒ ▿ ▒",
            " ·· ▜▒▒✚▒▒▛",
            "      ▒█▒☥",
            "     ╱   ╲",
        ],
    }
}

fn gangster_sprite(moving: bool, phase: u64) -> Vec<&'static str> {
    if !moving {
        return vec![
            "   ▒▒▒▒▒",
            "  ▒◕▒◕▒",
            "   ▒○▒",
            "  ╭▒▒▒╮━",
            "   ▒█▒",
            "  ╱ ╲",
        ];
    }
    match phase {
        0 => vec![
            "   ▒▒▒▒▒",
            "  ▒◕▒◕▒",
            "   ▒○▒",
            "  ╭▒▒▒╮━",
            "   ▒█▒",
            "  ╱ ╲",
        ],
        1 => vec![
            "    ▒▒▒▒▒",
            " · ▒◕▒◕▒",
            "    ▒○▒",
            "   ╭▒▒▒╮━›",
            "    ▒█▒",
            "   ╱ ╲",
        ],
        2 => vec![
            "      ▒▒▒▒▒",
            "  ·· ▒◕▒◕▒",
            "      ▒○▒",
            "     ╭▒▒▒╮━━»",
            "      ▒█▒",
            "       ╲╱",
        ],
        _ => vec![
            "     ▒▒▒▒▒",
            " ·· ▒◕▒◕▒",
            "     ▒○▒",
            "    ╭▒▒▒╮━»",
            "     ▒█▒",
            "    ╱ ╲",
        ],
    }
}

fn capitalist_sprite(moving: bool, phase: u64) -> Vec<&'static str> {
    if !moving {
        return vec![
            "  ◎◎◎◎◎◎◎",
            " ◎▒▒▒▒▒▒▒◎",
            "  ▒◕▒▒◕▒",
            "   ▒ ▿ ▒",
            "  ╔▒$▒$▒╗",
            "    ╨ ╨",
        ];
    }
    match phase {
        0 => vec![
            "  ◎◎◎◎◎◎◎",
            " ◎▒▒▒▒▒▒▒◎",
            "  ▒◕▒▒◕▒",
            "   ▒ ▿ ▒",
            "  ╔▒$▒$▒╗",
            "    ╨ ╨",
        ],
        1 => vec![
            "   ◎◎◎◎◎◎◎    $",
            " ·◎▒▒▒▒▒▒▒◎  ◎›",
            "   ▒◕▒▒◕▒",
            "    ▒ ▿ ▒",
            "   ╔▒$▒$▒╗",
            "     ╨ ╨",
        ],
        2 => vec![
            "     ◎◎◎◎◎◎◎     ◎$»",
            "  ··◎▒▒▒▒▒▒▒◎   ◎",
            "     ▒◕▒▒◕▒",
            "      ▒ ▿ ▒",
            "     ╔▒$▒$▒╗",
            "       ╲╱",
        ],
        _ => vec![
            "    ◎◎◎◎◎◎◎   ◎»",
            " ··◎▒▒▒▒▒▒▒◎",
            "    ▒◕▒▒◕▒",
            "     ▒ ▿ ▒",
            "    ╔▒$▒$▒╗",
            "      ╨ ╨",
        ],
    }
}

fn monster_sprite(state: MonsterSceneState, kind: MonsterKind) -> Vec<&'static str> {
    match (state, kind) {
        (MonsterSceneState::None, _) => vec!["", "", "", ""],
        (MonsterSceneState::Alive, MonsterKind::ClaudeGlyphImp) => {
            vec!["  ▒▒▒▒▒▒  ", "▒▒▒▄▒▒▄▒▒▒", "  ▒ ▒ ▒ ▒ "]
        }
        (MonsterSceneState::Defeated, MonsterKind::ClaudeGlyphImp) => {
            vec!["▒▒▒×▒▒×▒▒▒", "  ------  "]
        }
        (MonsterSceneState::Alive, MonsterKind::DookkaBurrower) => vec![
            "  \\|/   ",
            " .-^-.  ",
            "/  o o\\ ",
            "|  '-' |",
            " /|_|\\ ",
            "  / \\  ",
        ],
        (MonsterSceneState::Defeated, MonsterKind::DookkaBurrower) => vec![
            "  \\|/   ",
            " .-^-.  ",
            "/  x x\\ ",
            "|  --- |",
            " /|_|\\ ",
            "        ",
        ],
        (MonsterSceneState::Alive, MonsterKind::WandererBoss) => vec![
            "  .-^-._ ",
            " / o o \\ ",
            "<|  V  |>",
            " /|===|\\ ",
            " _/   \\_ ",
        ],
        (MonsterSceneState::Defeated, MonsterKind::WandererBoss) => vec![
            "  .-^-._ ",
            " / x x \\ ",
            "<| --- |>",
            " /|===|\\ ",
            "         ",
        ],
        (MonsterSceneState::Alive, MonsterKind::PortraitKeeperBoss) => vec![
            "  .-\"\"\"-. ",
            " /  o o  \\",
            "|    ^    |",
            " \\  ---  /",
            " /|     |\\",
            "  /|   |\\ ",
        ],
        (MonsterSceneState::Defeated, MonsterKind::PortraitKeeperBoss) => vec![
            "  .-\"\"\"-. ",
            " /  x x  \\",
            "|   ---   |",
            " \\       /",
            " /|     |\\",
            "          ",
        ],
    }
}

fn style_dookka_sprite_row(row: &str) -> String {
    if std::env::var_os("NO_COLOR").is_some() {
        return row.replace('▒', "█");
    }
    let mut out = String::new();
    for ch in row.chars() {
        match ch {
            '▒' => out.push_str("\x1b[48;2;224;132;100m \x1b[0m"),
            '▄' => out.push_str("\x1b[38;2;224;132;100m▄\x1b[0m"),
            _ => out.push(ch),
        }
    }
    out
}

fn render_combat_hp_rows(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    width: usize,
    lang: UiLanguage,
) -> Vec<String> {
    let half = width.saturating_sub(3).max(20) / 2;
    let hero_label = hp_label(
        label(lang, "영웅", "HERO"),
        snapshot.player.hp.max(0),
        snapshot.player.max_hp.max(1),
    );
    let hero_bar = hp_bar(
        snapshot.player.hp.max(0),
        snapshot.player.max_hp.max(1),
        half,
    );
    let monster = monster_hp_estimate(snapshot, logs).map(|(current, max)| {
        (
            hp_label(label(lang, "몬스터", "MON"), current, max),
            hp_bar(current, max, half),
        )
    });
    if let Some((monster_label, monster_bar)) = monster {
        vec![
            two_edge_columns(&hero_label, &monster_label, width),
            two_edge_columns(&hero_bar, &monster_bar, width),
        ]
    } else {
        vec![hero_label, hero_bar]
    }
}

fn hp_label(label: &str, current: i32, max: i32) -> String {
    let current = current.clamp(0, max.max(1));
    format!("{label} {current}/{max}")
}

fn hp_bar(current: i32, max: i32, width: usize) -> String {
    let current = current.clamp(0, max.max(1));
    let bar_width = width.saturating_sub(2).clamp(6, 18);
    let filled = ((current as f64 / max.max(1) as f64) * bar_width as f64).round() as usize;
    let filled = filled.min(bar_width);
    let empty = bar_width.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

fn two_edge_columns(left: &str, right: &str, width: usize) -> String {
    let right_width = display_width(right);
    let gap = 1;
    let left_limit = width.saturating_sub(right_width + gap);
    let left = clip_display(left, left_limit);
    let left_width = display_width(&left);
    let spaces = width.saturating_sub(left_width + right_width);
    format!("{left}{}{right}", " ".repeat(spaces))
}

fn monster_hp_estimate(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
) -> Option<(i32, i32)> {
    let state = monster_scene_state(snapshot, logs);
    if state == MonsterSceneState::None {
        return None;
    }
    if state == MonsterSceneState::Alive {
        if let (Some(current), Some(max)) =
            (snapshot.combat.monster_hp, snapshot.combat.monster_max_hp)
        {
            let max = max.max(1);
            return Some((current.clamp(0, max), max));
        }
    }
    let max = (32 + snapshot.player.level as i32 * 8).max(20);
    if state == MonsterSceneState::Defeated {
        return Some((0, max));
    }
    let current_version = snapshot.state_version;
    if let Some(hp) = logs
        .iter()
        .filter(|entry| current_version.saturating_sub(entry.state_version) <= 8)
        .filter_map(|entry| parse_monster_hp(&entry.message))
        .next()
    {
        return Some(hp);
    }
    let damage_taken = logs
        .iter()
        .filter(|entry| current_version.saturating_sub(entry.state_version) <= 4)
        .filter_map(|entry| parse_warrior_damage(&entry.message))
        .sum::<i32>();
    Some(((max - damage_taken).clamp(1, max), max))
}

fn parse_warrior_damage(message: &str) -> Option<i32> {
    let rest = message.strip_prefix("Warrior hit ")?;
    let (_, damage) = rest.split_once(" for ")?;
    let damage = damage
        .split_once('.')
        .map(|(value, _)| value)
        .unwrap_or(damage);
    damage.parse().ok()
}

fn parse_monster_hp(message: &str) -> Option<(i32, i32)> {
    let (_, hp) = message.split_once(" HP ")?;
    let hp = hp.trim_end_matches('.');
    let (current, max) = hp.split_once('/')?;
    Some((current.parse().ok()?, max.parse().ok()?))
}

fn pad_chars(value: &str, width: usize) -> String {
    let len = display_width(value);
    if len >= width {
        clip_display(value, width)
    } else {
        format!("{}{}", value, " ".repeat(width - len))
    }
}

fn render_combat_caption(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    lang: UiLanguage,
) -> String {
    if hero_is_fallen(snapshot, logs) {
        return label(
            lang,
            "쓰러짐: 캐릭터가 회복 중입니다.",
            "Fallen: character is recovering.",
        )
        .to_string();
    }
    if snapshot.player.mode != "auto_hunt" && snapshot.player.mode != "dungeon" {
        return label(
            lang,
            "대기: 캐릭터가 준비 중입니다.",
            "Idle: warrior is standing by.",
        )
        .to_string();
    }
    match monster_scene_state(snapshot, logs) {
        MonsterSceneState::Alive => label(
            lang,
            if snapshot.player.mode == "dungeon" {
                "던전: 몬스터와 전투 중."
            } else {
                "자동사냥: 달리고 점프하며 몬스터와 전투 중."
            },
            if snapshot.player.mode == "dungeon" {
                "Dungeon: engaging monster."
            } else {
                "Auto hunt: running, jumping, engaging monster."
            },
        )
        .to_string(),
        MonsterSceneState::Defeated => label(
            lang,
            if snapshot.player.mode == "dungeon" {
                "던전: 몬스터 처치 후 다음 지점으로 이동 중."
            } else {
                "자동사냥: 몬스터 처치 후 다음 지점으로 이동 중."
            },
            if snapshot.player.mode == "dungeon" {
                "Dungeon: monster defeated; moving onward."
            } else {
                "Auto hunt: monster defeated; sprinting onward."
            },
        )
        .to_string(),
        MonsterSceneState::None => {
            latest_progress_message(snapshot, logs, lang).unwrap_or_else(|| {
                label(
                    lang,
                    if snapshot.player.mode == "dungeon" {
                        "던전 탐험 진행 중."
                    } else {
                        "자동 사냥 진행 중."
                    },
                    if snapshot.player.mode == "dungeon" {
                        "Dungeon exploration in progress."
                    } else {
                        "Auto hunt in progress."
                    },
                )
                .to_string()
            })
        }
    }
}

fn latest_progress_message(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
    lang: UiLanguage,
) -> Option<String> {
    let current = snapshot.state_version;
    logs.iter()
        .filter(|entry| current.saturating_sub(entry.state_version) <= 8)
        .find(|entry| entry.message.starts_with("Recovery in progress"))
        .map(|entry| display_message(&entry.message, lang))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MonsterSceneState {
    None,
    Alive,
    Defeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MonsterKind {
    ClaudeGlyphImp,
    DookkaBurrower,
    WandererBoss,
    PortraitKeeperBoss,
}

fn monster_scene_kind(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
) -> MonsterKind {
    let current = snapshot.state_version;
    for entry in logs {
        if current.saturating_sub(entry.state_version) > 4 {
            continue;
        }
        if entry.message.starts_with("Boss ") || entry.message.contains(" Boss ") {
            return boss_monster_kind(&entry.message);
        }
    }
    MonsterKind::ClaudeGlyphImp
}

fn boss_monster_kind(message: &str) -> MonsterKind {
    if message.contains("Lich") || message.contains("Medusa") {
        MonsterKind::PortraitKeeperBoss
    } else if message.contains("Crystal")
        || message.contains("Golem")
        || message.contains("Titan")
        || message.contains("Warden")
    {
        MonsterKind::WandererBoss
    } else {
        MonsterKind::DookkaBurrower
    }
}

fn monster_scene_state(
    snapshot: &vibemud_core::GameSnapshot,
    logs: &[vibemud_db::LogEntry],
) -> MonsterSceneState {
    if snapshot.player.mode != "auto_hunt" && snapshot.player.mode != "dungeon" {
        return MonsterSceneState::None;
    }
    let current = snapshot.state_version;
    let mut saw_alive = false;
    for entry in logs {
        if current.saturating_sub(entry.state_version) > 4 {
            continue;
        }
        let message = entry.message.to_ascii_lowercase();
        if message.contains("defeated")
            || message.contains("died")
            || message.contains("burst apart")
        {
            if defeated_log_is_fresh(entry) {
                return MonsterSceneState::Defeated;
            }
            if !snapshot.combat.in_combat {
                return MonsterSceneState::None;
            }
            continue;
        }
        if message.contains("appeared")
            || message.contains("combat started")
            || message.contains("combat round")
            || message.contains("remains at")
            || message.contains(" hit warrior")
            || message.contains(" used a skill")
            || message.contains("warrior hit")
            || message.contains("warrior missed")
        {
            saw_alive = true;
        }
    }
    if saw_alive || snapshot.combat.in_combat {
        MonsterSceneState::Alive
    } else {
        MonsterSceneState::None
    }
}

fn defeated_log_is_fresh(entry: &vibemud_db::LogEntry) -> bool {
    let Ok(created_at) = OffsetDateTime::parse(&entry.created_at, &Rfc3339) else {
        return true;
    };
    let age = OffsetDateTime::now_utc() - created_at;
    age.whole_seconds() <= DEFEATED_SCENE_TTL_SECONDS
}

fn place_chars(lane: &mut [char], start: usize, value: &str) {
    for (offset, ch) in value.chars().enumerate() {
        if let Some(slot) = lane.get_mut(start + offset) {
            *slot = ch;
        }
    }
}

fn panel_line_raw(value: &str, inner: usize) -> String {
    let width = inner.saturating_sub(2);
    let clipped = clip_display(value, width);
    format!("| {} |", pad_chars(&clipped, width))
}

fn clip_chars(value: &str, max: usize) -> String {
    clip_display(value, max)
}

fn clip_display(value: &str, max: usize) -> String {
    if display_width(value) <= max {
        return value.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let ellipsis = "…";
    let target = max.saturating_sub(display_width(ellipsis));
    let mut out = String::new();
    let mut width = 0;
    let mut index = 0;
    while index < value.len() {
        if let Some(end) = ansi_sequence_end(value, index) {
            out.push_str(&value[index..end]);
            index = end;
            continue;
        }
        let ch = value[index..]
            .chars()
            .next()
            .expect("index is within string");
        let char_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + char_width > target {
            break;
        }
        out.push(ch);
        width += char_width;
        index += ch.len_utf8();
    }
    out.push_str(ellipsis);
    if out.contains('\x1b') {
        out.push_str("\x1b[0m");
    }
    out
}

fn display_width(value: &str) -> usize {
    let mut width = 0;
    let mut index = 0;
    while index < value.len() {
        if let Some(end) = ansi_sequence_end(value, index) {
            index = end;
            continue;
        }
        let ch = value[index..]
            .chars()
            .next()
            .expect("index is within string");
        width += UnicodeWidthChar::width(ch).unwrap_or(0);
        index += ch.len_utf8();
    }
    width
}

fn ansi_sequence_end(value: &str, start: usize) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.get(start) != Some(&0x1B) || bytes.get(start + 1) != Some(&b'[') {
        return None;
    }
    for (offset, byte) in bytes[start + 2..].iter().enumerate() {
        if (0x40..=0x7E).contains(byte) {
            return Some(start + 2 + offset + 1);
        }
    }
    None
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok()?.parse().ok()
}

fn terminal_width() -> usize {
    env_usize("COLUMNS").unwrap_or(100)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiLanguage {
    Ko,
    En,
}

fn localized_status_dto(snapshot: &vibemud_core::GameSnapshot, lang: UiLanguage) -> StatusLineDto {
    let mut dto = StatusLineDto::from(snapshot);
    if lang == UiLanguage::Ko {
        dto.class_label = ko_class_label(visible_class_id(snapshot)).to_string();
        dto.area_label = if snapshot.player.mode == "dungeon" {
            snapshot
                .player
                .current_dungeon_id
                .as_deref()
                .map(ko_dungeon_label)
                .unwrap_or_else(|| {
                    ko_area_label(snapshot.player.current_area_id.as_deref().unwrap_or("town"))
                })
                .to_string()
        } else {
            ko_area_label(snapshot.player.current_area_id.as_deref().unwrap_or("town")).to_string()
        };
        dto.mode_label = ko_mode_label(&snapshot.player.mode).to_string();
        dto.danger_label = ko_danger_label(&dto.danger_label).to_string();
    }
    dto
}

fn ui_language() -> UiLanguage {
    match vibemud_db::load_config()
        .ok()
        .map(|config| config.ui.language.to_ascii_lowercase())
        .as_deref()
    {
        Some("en") | Some("english") => UiLanguage::En,
        _ => UiLanguage::Ko,
    }
}

fn label<'a>(lang: UiLanguage, ko: &'a str, en: &'a str) -> &'a str {
    match lang {
        UiLanguage::Ko => ko,
        UiLanguage::En => en,
    }
}

fn codex_cli_guidance_enabled() -> bool {
    vibemud_db::load_config()
        .ok()
        .map(|config| config.integrations.codex_enabled && !config.integrations.claude_enabled)
        .unwrap_or(false)
}

fn integration_hint<'a>(
    lang: UiLanguage,
    codex_ko: &'a str,
    codex_en: &'a str,
    claude_ko: &'a str,
    claude_en: &'a str,
) -> &'a str {
    integration_hint_for(
        codex_cli_guidance_enabled(),
        lang,
        codex_ko,
        codex_en,
        claude_ko,
        claude_en,
    )
}

fn integration_hint_for<'a>(
    codex_cli_guidance: bool,
    lang: UiLanguage,
    codex_ko: &'a str,
    codex_en: &'a str,
    claude_ko: &'a str,
    claude_en: &'a str,
) -> &'a str {
    if codex_cli_guidance {
        label(lang, codex_ko, codex_en)
    } else {
        label(lang, claude_ko, claude_en)
    }
}

fn localized_status_word(value: &str, lang: UiLanguage) -> String {
    if lang == UiLanguage::En {
        return value.to_string();
    }
    match value {
        "running" => "실행중",
        "stopped" => "중지",
        "pending" => "대기",
        "processing" => "처리중",
        "done" => "완료",
        "failed" => "실패",
        other => other,
    }
    .to_string()
}

fn render_status_for_width(
    snapshot: &vibemud_core::GameSnapshot,
    width: usize,
    lang: UiLanguage,
) -> String {
    let dto = localized_status_dto(snapshot, lang);
    if lang == UiLanguage::En {
        return vibemud_hud::render_for_width(&dto, width);
    }
    if width < 60 {
        return format!(
            "L{} {} HP{} 위:{}",
            dto.level, dto.class_label, dto.hp, dto.danger_label
        );
    }
    if width < 100 {
        return format!(
            "Lv.{} {} | HP {}/{} | {} | {} | 위험 {}",
            dto.level,
            dto.class_label,
            dto.hp,
            dto.max_hp,
            dto.area_label,
            dto.mode_label,
            dto.danger_label
        );
    }
    format!(
        "[VibeMUD] Lv.{} {} | HP {}/{} | {} | {} | 파티 {}/4 | 위험: {} | 전리품 {}",
        dto.level,
        dto.class_label,
        dto.hp,
        dto.max_hp,
        dto.area_label,
        dto.mode_label,
        dto.party_count,
        dto.danger_label,
        dto.loot_count
    )
}

fn compact_stat_line(stats: vibemud_core::RepresentativeStats, lang: UiLanguage) -> String {
    match lang {
        UiLanguage::Ko => format!(
            "전투력 {} | 공 {} | 방 {} | 속 {}",
            stats.combat_power, stats.attack, stats.defense, stats.speed
        ),
        UiLanguage::En => format!(
            "Power {} | ATK {} | DEF {} | SPD {}",
            stats.combat_power, stats.attack, stats.defense, stats.speed
        ),
    }
}

fn render_full_status(snapshot: &vibemud_core::GameSnapshot, lang: UiLanguage) -> String {
    if lang == UiLanguage::En {
        let stats = vibemud_core::representative_stats(&snapshot.player);
        return format!(
            "{}\nRepresentative Stats\n- Power: {}\n- ATK: {}\n- DEF: {}\n- SPD: {}\nDetailed Stats\n- Accuracy: {}\n- Resistance: {}\n- Regen: {}\n- Luck: {}\n- MP: {}/{}",
            vibemud_hud::render_full(snapshot),
            stats.combat_power,
            stats.attack,
            stats.defense,
            stats.speed,
            snapshot.player.accuracy,
            snapshot.player.evasion,
            snapshot.player.regen,
            snapshot.player.luck,
            snapshot.player.mp,
            snapshot.player.max_mp
        );
    }
    let dto = localized_status_dto(snapshot, lang);
    let stats = vibemud_core::representative_stats(&snapshot.player);
    format!(
        "VibeMUD 상세 상태\n레벨: {} {}\n지역: {}\n모드: {}\nHP: {}/{}  MP: {}/{}\n경험치: {}/{}  골드: {}\n대표 능력치\n- 전투력: {}\n- 공격력: {}\n- 방어력: {}\n- 속도: {}\n상세 스탯\n- 명중: {}\n- 저항: {}\n- 재생: {}\n- 행운: {}\n파티: {}/4  위험: {}  전리품: {}",
        snapshot.player.level,
        dto.class_label,
        dto.area_label,
        dto.mode_label,
        snapshot.player.hp,
        snapshot.player.max_hp,
        snapshot.player.mp,
        snapshot.player.max_mp,
        snapshot.player.xp,
        snapshot.player.xp_to_next,
        snapshot.player.gold,
        stats.combat_power,
        stats.attack,
        stats.defense,
        stats.speed,
        snapshot.player.accuracy,
        snapshot.player.evasion,
        snapshot.player.regen,
        snapshot.player.luck,
        dto.party_count,
        dto.danger_label,
        dto.loot_count
    )
}

fn render_side_panel_localized(dto: &StatusLineDto, unicode: bool, lang: UiLanguage) -> String {
    if lang == UiLanguage::En {
        return vibemud_hud::render_side_panel(dto, unicode);
    }
    if unicode {
        format!(
            "┌──── VibeMUD ────┐\n│ Lv.{:<12}│\n│ {:<16}│\n│ HP {:>4}/{:<7}│\n│ {:<16}│\n│ {:<16}│\n│ 파티 {:<9}│\n│ 위험 {:<8}│\n└─────────────────┘",
            dto.level,
            dto.class_label,
            dto.hp,
            dto.max_hp,
            clip_chars(&dto.area_label, 16),
            clip_chars(&dto.mode_label, 16),
            format!("{}/4", dto.party_count),
            dto.danger_label
        )
    } else {
        format!(
            "+---- VibeMUD ----+\n| Lv.{:<12}|\n| {:<16}|\n| HP {:>4}/{:<7}|\n| {:<16}|\n| {:<16}|\n| 파티 {:<9}|\n| 위험 {:<8}|\n+-----------------+",
            dto.level,
            dto.class_label,
            dto.hp,
            dto.max_hp,
            clip_chars(&dto.area_label, 16),
            clip_chars(&dto.mode_label, 16),
            format!("{}/4", dto.party_count),
            dto.danger_label
        )
    }
}

fn ko_class_label(class_id: &str) -> &'static str {
    match class_id {
        "adventurer" => "모험가",
        "fighter" | "warrior" => "투사",
        "robot" | "mage" | "glyph-sage" | "burrower-miner" => "로봇",
        "priest" | "healer" => "사제",
        "gangster" | "rogue" => "깡패",
        "capitalist" | "wanderer" | "portrait-keeper" => "자산가",
        _ => "모험가",
    }
}

fn ko_area_label(area_id: &str) -> &'static str {
    match area_id {
        "training-field" => "훈련장",
        "forest-edge" => "숲 가장자리",
        "old-mine" => "오래된 광산",
        "misty-swamp" => "안개 늪",
        "fallen-fortress" => "몰락한 요새",
        "obsidian-coast" => "흑요 해안",
        "titan-steppe" => "티탄 초원",
        "oracle-ruins" => "예언자 유적",
        "styx-marsh" => "스틱스 늪",
        "olympus-gate" => "올림포스 관문",
        "town" => "마을",
        _ => "미지",
    }
}

fn ko_mode_label(mode: &str) -> &'static str {
    match mode {
        "auto_hunt" => "자동사냥",
        "dungeon" => "던전",
        "rest" => "휴식",
        "recovering" => "회복 중",
        _ => "대기",
    }
}

fn ko_danger_label(danger: &str) -> &'static str {
    match danger {
        "Safe" => "안전",
        "Low" => "낮음",
        "Normal" => "보통",
        "High" => "높음",
        "Deadly" => "치명적",
        "Dungeon" => "던전",
        _ => "안전",
    }
}

fn ko_command_kind(kind: &str) -> &'static str {
    match kind {
        "hunt_start" => "자동사냥 시작",
        "hunt_stop" => "자동사냥 정지",
        "area_enter" => "지역 입장",
        "dungeon_enter" => "던전 입장",
        "dungeon_retreat" => "던전 후퇴",
        "rest" => "휴식",
        "town" => "마을 귀환",
        "party_recruit" => "파티 영입",
        "party_swap" => "파티 교체",
        "equip" => "장착",
        "enhance" => "강화",
        "skill_use" => "스킬 사용",
        "shop_buy" => "상점 구매",
        "shop_sell" => "상점 판매",
        _ => "명령",
    }
}

fn display_event_type(value: &str, lang: UiLanguage) -> String {
    if lang == UiLanguage::En {
        return value.to_string();
    }
    match value {
        "initialized" => "초기화",
        "session_started" => "세션시작",
        "session_stopped" => "세션중지",
        "command_processed" => "명령처리",
        "tick_advanced" => "틱진행",
        "encounter_resolved" => "조우해결",
        "player_died" => "사망",
        "level_up" => "레벨업",
        other => other,
    }
    .to_string()
}

fn panel_log_line(entry: &vibemud_db::LogEntry, lang: UiLanguage) -> String {
    let tag = log_tag(&entry.message, lang);
    format!(
        "{tag} {}",
        color_log_message(&entry.message, &display_message(&entry.message, lang), lang)
    )
}

#[derive(Debug, Clone, Copy)]
enum AnsiColor {
    White,
    Gray,
    Cyan,
    Green,
    Blue,
    Purple,
    Yellow,
    Red,
}

fn colorize(value: &str, color: AnsiColor) -> String {
    if std::env::var_os("NO_COLOR").is_some() {
        return value.to_string();
    }
    let code = match color {
        AnsiColor::White => "37",
        AnsiColor::Gray => "90",
        AnsiColor::Cyan => "36",
        AnsiColor::Green => "32",
        AnsiColor::Blue => "34",
        AnsiColor::Purple => "35",
        AnsiColor::Yellow => "33",
        AnsiColor::Red => "31",
    };
    format!("\x1b[{code}m{value}\x1b[0m")
}

fn rainbow_fever_label() -> String {
    if std::env::var_os("NO_COLOR").is_some()
        || std::env::var("TERM")
            .map(|value| value == "dumb")
            .unwrap_or(false)
    {
        return "FEVERTIME".to_string();
    }
    let colors = ["31", "33", "32", "36", "34", "35"];
    "FEVERTIME"
        .chars()
        .enumerate()
        .map(|(index, ch)| format!("\x1b[{}m{}\x1b[0m", colors[index % colors.len()], ch))
        .collect::<Vec<_>>()
        .join("")
}

fn colorize_by_rarity(value: &str, rarity_color: &str) -> String {
    match rarity_color {
        "white" | "일반" | "Common" => colorize(value, AnsiColor::White),
        "green" | "고급" | "Uncommon" => colorize(value, AnsiColor::Green),
        "blue" | "희귀" | "Rare" => colorize(value, AnsiColor::Blue),
        "purple" | "영웅" | "Epic" => colorize(value, AnsiColor::Purple),
        "yellow" | "전설" | "Legendary" => colorize(value, AnsiColor::Yellow),
        _ => value.to_string(),
    }
}

fn color_log_message(original: &str, display: &str, lang: UiLanguage) -> String {
    if original == "You fell in battle and returned to town." || original.starts_with("Lost ") {
        return colorize(display, AnsiColor::Red);
    }
    if original.starts_with("Boss reward:") {
        if original.starts_with("Boss reward: +") {
            return colorize(display, AnsiColor::Yellow);
        }
        if let Some(rarity) = boss_reward_rarity(original) {
            return colorize_by_rarity(display, rarity);
        }
        return display.to_string();
    }

    let mut out = display.to_string();
    match lang {
        UiLanguage::Ko => {
            out = color_resource_label(&out, "경험치", AnsiColor::Green);
            out = color_resource_label(&out, "골드", AnsiColor::Yellow);
            out = color_number_before_label(&out, "피해", AnsiColor::Red);
        }
        UiLanguage::En => {
            out = color_resource_label(&out, "XP", AnsiColor::Green);
            out = color_resource_label(&out, "gold", AnsiColor::Yellow);
            out = color_number_before_label(&out, "damage", AnsiColor::Red);
        }
    }
    out
}

fn boss_reward_rarity(message: &str) -> Option<&'static str> {
    let item = message
        .strip_prefix("Boss reward: ")
        .and_then(|value| value.strip_suffix('.'))?;
    match item {
        "Hector Axe" => Some("Uncommon"),
        "Perseus Blade" => Some("Rare"),
        "Hecate Amulet" | "Hephaestus Hammer" | "Athena Aegis" => Some("Epic"),
        "Kronos Key" => Some("Legendary"),
        _ => None,
    }
}

fn color_resource_label(input: &str, label_text: &str, color: AnsiColor) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(label_index) = rest.find(label_text) {
        let prefix = &rest[..label_index];
        if let Some(start) = prefix.rfind('+') {
            out.push_str(&prefix[..start]);
            out.push_str(&colorize(
                &format!("{}{}", &prefix[start..], label_text),
                color,
            ));
        } else {
            out.push_str(prefix);
            out.push_str(label_text);
        }
        rest = &rest[label_index + label_text.len()..];
    }
    out.push_str(rest);
    out
}

fn color_number_before_label(input: &str, label_text: &str, color: AnsiColor) -> String {
    let mut out = String::new();
    let mut rest = input;
    while let Some(label_index) = rest.find(label_text) {
        let prefix = &rest[..label_index];
        let trimmed_end = prefix.trim_end_matches(' ').len();
        let digits_start = prefix[..trimmed_end]
            .char_indices()
            .rev()
            .find(|(_, ch)| !ch.is_ascii_digit())
            .map(|(idx, ch)| idx + ch.len_utf8())
            .unwrap_or(0);
        if digits_start < trimmed_end {
            out.push_str(&prefix[..digits_start]);
            out.push_str(&colorize(
                &format!("{}{}", &prefix[digits_start..], label_text),
                color,
            ));
        } else {
            out.push_str(prefix);
            out.push_str(label_text);
        }
        rest = &rest[label_index + label_text.len()..];
    }
    out.push_str(rest);
    out
}

fn log_tag(message: &str, lang: UiLanguage) -> &'static str {
    match log_category(message) {
        LogCategory::System => label(lang, "[시스템]", "[SYSTEM]"),
        LogCategory::Reward => label(lang, "[보 상]", "[REWARD]"),
        LogCategory::Combat => label(lang, "[전 투]", "[COMBAT]"),
        LogCategory::Explore => label(lang, "[탐 색]", "[SCOUT ]"),
        LogCategory::Recovery => label(lang, "[회 복]", "[RECOV ]"),
        LogCategory::Command => label(lang, "[명 령]", "[CMD   ]"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogCategory {
    System,
    Reward,
    Combat,
    Explore,
    Recovery,
    Command,
}

fn log_category(message: &str) -> LogCategory {
    if message == "Initialized VibeMUD"
        || message == "Session started"
        || message == "Session stopped"
        || message == "Bounded session stopped"
        || message == "Tick advanced"
    {
        return LogCategory::System;
    }
    if message.starts_with("Action feedback:")
        || message.starts_with("Auto hunt started")
        || message == "Auto hunt stopped"
        || message.starts_with("Entered ")
        || message.starts_with("Returned to town")
        || message.starts_with("Retreated ")
        || message.starts_with("Rested ")
        || message.starts_with("Bought ")
        || message.starts_with("Sold ")
        || message.starts_with("Sale skipped:")
        || message.starts_with("Equipped ")
        || message.starts_with("Recruited ")
    {
        return LogCategory::Command;
    }
    if message.starts_with("Reward:")
        || message.starts_with("Boss reward:")
        || message.contains("defeated. +")
        || message.starts_with("Level up!")
        || message.starts_with("Lost ")
        || (message.starts_with("Boss ") && message.contains(" reward "))
    {
        return LogCategory::Reward;
    }
    if message.contains("Recovery")
        || message.contains("recovery")
        || message.starts_with("Resting ")
        || message.starts_with("Town recovery")
    {
        return LogCategory::Recovery;
    }
    if message.starts_with("Auto hunt scouting")
        || message.starts_with("No monster found")
        || message.starts_with("Dungeon scouting")
        || message.starts_with("Dungeon encounter point")
        || message.starts_with("Dungeon point")
        || message.starts_with("Dungeon normal monster defeated")
        || message.starts_with("Dungeon entry reset")
        || (message.starts_with("Dungeon ")
            && message.ends_with(" cleared. Restarting dungeon run."))
        || message == "The dungeon boss is approaching."
        || message.starts_with("Advanced to dungeon floor")
    {
        return LogCategory::Explore;
    }
    LogCategory::Combat
}

fn is_game_log_entry(entry: &vibemud_db::LogEntry) -> bool {
    if is_suppressed_log_message(&entry.message) {
        return false;
    }
    if entry.message == "Tick advanced" {
        return false;
    }
    !matches!(
        entry.event_type.as_str(),
        "initialized" | "session_started" | "session_stopped"
    )
}

fn is_suppressed_log_message(message: &str) -> bool {
    message.starts_with("Auto hunt scouting")
        || message.starts_with("Dungeon scouting")
        || message.starts_with("No monster found")
        || (message.starts_with("Dungeon point") && message.ends_with("had no monster."))
        || message.starts_with("Town recovery restored")
        || message.starts_with("Passive regen restored")
}

fn display_message(message: &str, lang: UiLanguage) -> String {
    if lang == UiLanguage::En {
        return message.to_string();
    }
    if message == "Initialized VibeMUD" {
        return "VibeMUD 초기화 완료".to_string();
    }
    if message == "Session started" {
        return "세션 시작".to_string();
    }
    if message == "Session stopped" {
        return "세션 중지".to_string();
    }
    if message == "Bounded session stopped" {
        return "제한 실행 세션 중지".to_string();
    }
    if message == "Tick advanced" {
        return "틱 진행".to_string();
    }
    if message == "Resting recovered HP/MP" {
        return "휴식으로 HP/MP 회복".to_string();
    }
    if message == "Recovery started for 1 minute in town." {
        return "마을에서 1분 강제 휴식에 들어갔습니다".to_string();
    }
    if message == "Recovery complete. Actions unlocked." {
        return "회복 완료. 다시 행동할 수 있습니다".to_string();
    }
    if message == "Auto hunt stopped" {
        return "자동 사냥 중지".to_string();
    }
    if message == "Rested to full HP/MP" {
        return "HP/MP 완전 회복".to_string();
    }
    if message == "Returned to town" {
        return "마을로 귀환".to_string();
    }
    if message == "Returned to town and started resting" {
        return "마을로 귀환. 휴식 시작".to_string();
    }
    if message == "Action feedback: character stats opened." {
        return "액션: stats 열림".to_string();
    }
    if message == "Action feedback: map opened." {
        return "액션: 지도 열림".to_string();
    }
    if message == "Action feedback: quest opened." {
        return "액션: 퀘스트 열림".to_string();
    }
    if message == "Action feedback: HUD view closed." {
        return "액션: HUD 닫힘".to_string();
    }
    if let Some(kind) = message
        .strip_prefix("Action feedback: queued ")
        .and_then(|v| v.strip_suffix('.'))
    {
        return format!("대기열: {}", ko_command_kind(kind));
    }
    if let Some(xp) = message
        .strip_prefix("Reward: +")
        .and_then(|v| v.strip_suffix(" XP."))
    {
        return format!("보상: +{xp} 경험치");
    }
    if let Some(gold) = message
        .strip_prefix("Reward: +")
        .and_then(|v| v.strip_suffix(" gold."))
    {
        return format!("보상: +{gold} 골드");
    }
    if let Some(item) = message
        .strip_prefix("Boss reward: ")
        .and_then(|v| v.strip_suffix('.'))
    {
        if let Some(gold) = item.strip_prefix('+').and_then(|v| v.strip_suffix(" gold")) {
            return format!("보스 보상: +{gold} 골드");
        }
        return format!("보스 보상 획득: {}", localized_item_name(item, lang));
    }
    if let Some(area) = message.strip_prefix("Auto hunt started in ") {
        return format!("{} 자동 사냥 시작", ko_area_label(area));
    }
    if let Some(area) = message.strip_prefix("Entered area ") {
        return format!("{} 입장", ko_area_label(area));
    }
    if let Some(dungeon) = message.strip_prefix("Entered dungeon ") {
        return format!("{} 던전 입장", ko_dungeon_label(dungeon));
    }
    if let Some(dungeon) = message
        .strip_prefix("Dungeon ")
        .and_then(|v| v.strip_suffix(" cleared. Restarting dungeon run."))
    {
        return format!("{} 클리어. 던전 재개", ko_dungeon_label(dungeon));
    }
    if let Some(rest) = message
        .strip_prefix("Auto hunt scouting. Next encounter in ")
        .and_then(|v| v.strip_suffix(" tick(s)."))
    {
        let _ = rest;
        return "자동 사냥 진행 중".to_string();
    }
    if let Some(rest) = message
        .strip_prefix("Dungeon scouting. Next encounter in ")
        .and_then(|v| v.strip_suffix(" tick(s)."))
    {
        let _ = rest;
        return "던전 탐험 진행 중".to_string();
    }
    if let Some(rest) = message
        .strip_prefix("Recovery in progress. Actions unlock in ")
        .and_then(|v| v.strip_suffix("s."))
    {
        return format!("회복 중. {rest}초 후 행동 가능");
    }
    if let Some(rest) = message
        .strip_prefix("Town recovery restored +")
        .and_then(|v| v.strip_suffix('.'))
    {
        return format!("마을 휴식 회복 +{rest}");
    }
    if let Some(area) = message
        .strip_prefix("No monster found in ")
        .and_then(|v| v.strip_suffix(". The hero keeps moving."))
    {
        return format!(
            "{}에서 몬스터를 만나지 못했습니다. 계속 전진합니다",
            ko_area_label(area)
        );
    }
    if let Some(floor) = message
        .strip_prefix("Dungeon scouting found no monster on floor ")
        .and_then(|v| v.strip_suffix('.'))
    {
        return format!("던전 {floor}층에서 몬스터를 만나지 못했습니다");
    }
    if let Some(rest) = message
        .strip_prefix("Dungeon encounter point ")
        .and_then(|v| v.strip_suffix('.'))
    {
        return format!("던전 조우 포인트 {rest}");
    }
    if let Some(rest) = message
        .strip_prefix("Dungeon point ")
        .and_then(|v| v.strip_suffix(" had no monster."))
    {
        return format!("던전 포인트 {rest}: 몬스터 없음");
    }
    if let Some(rest) = message
        .strip_prefix("Dungeon normal monster defeated ")
        .and_then(|v| v.strip_suffix('.'))
    {
        return format!("일반 몬스터 처치 {rest}");
    }
    if let Some(rest) = message
        .strip_prefix("Dungeon entry reset: only ")
        .and_then(|v| v.strip_suffix(" normal monsters defeated."))
    {
        return format!("던전 초입으로 복귀: 일반 몬스터 처치 {rest}");
    }
    if message == "The dungeon boss is approaching." {
        return "보스가 접근 중입니다".to_string();
    }
    if let Some(monster) = message.strip_suffix(" appeared.") {
        return format!("{} 등장", ko_monster_label(monster));
    }
    if let Some(monster) = message.strip_suffix(" combat started: exchanging blows for 5 seconds.")
    {
        return format!("{}와 5초 교전 시작", ko_monster_label(monster));
    }
    if let Some(rest) = message
        .strip_prefix("Combat round ")
        .and_then(|value| value.strip_suffix(" closes in."))
    {
        if let Some((round, monster)) = rest.split_once(": ") {
            return format!("전투 {round}: {} 접근", ko_monster_label(monster));
        }
    }
    if let Some(rest) = message.strip_prefix("Warrior hit ") {
        if let Some((monster, dealt)) = rest.split_once(" for ") {
            let hp_suffix = dealt
                .split_once(". ")
                .and_then(|(_, hp)| hp.strip_prefix(&format!("{monster} HP ")))
                .and_then(|hp| hp.strip_suffix('.'))
                .map(|hp| format!(" (몬스터 HP {hp})"))
                .unwrap_or_default();
            let dealt = dealt
                .split_once('.')
                .map(|(damage, _)| damage)
                .unwrap_or(dealt);
            return format!(
                "투사가 {}에게 {} 피해{}",
                ko_monster_label(monster),
                dealt,
                hp_suffix
            );
        }
    }
    if let Some(rest) = message.strip_prefix("Warrior missed ") {
        return format!(
            "투사의 공격이 {}에게 빗나감",
            ko_monster_label(rest.trim_end_matches('.'))
        );
    }
    if let Some(monster) = message
        .strip_prefix("💥 ")
        .and_then(|value| value.strip_suffix(" burst apart!"))
    {
        return format!("💥 {} 처치!", ko_monster_label(monster));
    }
    if let Some((monster, rest)) = message.split_once(" defeated. +") {
        let rest = rest
            .replace(" XP, +", " 경험치, +")
            .replace(" gold.", " 골드");
        return format!("{} 처치. +{rest}", ko_monster_label(monster));
    }
    if let Some(monster) = message.strip_suffix(" defeated.") {
        return format!("{} 처치", ko_monster_label(monster));
    }
    if let Some((monster, rest)) = message.split_once(" hit Warrior for ") {
        let hp_suffix = rest
            .split_once(". ")
            .and_then(|(_, hp)| hp.strip_prefix("Warrior HP "))
            .and_then(|hp| hp.strip_suffix('.'))
            .map(|hp| format!(" (영웅 HP {hp})"))
            .unwrap_or_default();
        let damage = rest
            .split_once('.')
            .map(|(damage, _)| damage)
            .unwrap_or(rest)
            .trim_end_matches('.');
        return format!(
            "{} 투사에게 {} 피해{}",
            ko_subject(ko_monster_label(monster)),
            damage,
            hp_suffix
        );
    }
    if let Some((monster, rest)) = message.split_once(" used a skill for ") {
        let hp_suffix = rest
            .split_once(". ")
            .and_then(|(_, hp)| hp.strip_prefix("Warrior HP "))
            .and_then(|hp| hp.strip_suffix('.'))
            .map(|hp| format!(" (영웅 HP {hp})"))
            .unwrap_or_default();
        let damage = rest
            .split_once('.')
            .map(|(damage, _)| damage)
            .unwrap_or(rest)
            .trim_end_matches('.');
        return format!(
            "{}의 스킬 공격: {} 피해{}",
            ko_monster_label(monster),
            damage,
            hp_suffix
        );
    }
    if let Some((monster, rest)) = message.split_once(" remains at ") {
        let hp = rest.trim_end_matches('.').trim_end_matches(" HP");
        return format!("{} 잔여 HP {}", ko_monster_label(monster), hp);
    }
    if let Some(level) = message.strip_prefix("Level up! Lv.") {
        return format!("레벨 업! Lv.{level}");
    }
    if message == "You fell in battle and returned to town." {
        return "전투에서 쓰러져 마을로 귀환".to_string();
    }
    if let Some(gold) = message
        .strip_prefix("Lost ")
        .and_then(|v| v.strip_suffix(" gold."))
    {
        return format!("골드 {gold} 손실");
    }
    if let Some(rest) = message
        .strip_prefix("Lost ")
        .and_then(|v| v.strip_suffix(" gold and current dungeon progress."))
    {
        return format!("골드 {rest} 및 현재 던전 진행도 손실");
    }
    if let Some(name) = message
        .strip_prefix("Recruited ")
        .and_then(|v| v.split_once(" (").map(|(name, _)| name))
    {
        return format!("{name} 영입");
    }
    if message.starts_with("Bought ") {
        return message
            .replace("Bought", "구매")
            .replace(" for ", " / ")
            .replace(" gold", " 골드");
    }
    if message.starts_with("Sold ") {
        return message
            .replace("Sold", "판매")
            .replace(" for ", " / ")
            .replace(" gold", " 골드");
    }
    if let Some(item) = message
        .strip_prefix("Sale skipped: item ")
        .and_then(|value| value.strip_suffix(" is no longer in inventory"))
    {
        return format!("판매 건너뜀: {item}은 이미 소지품에 없습니다");
    }
    if message.starts_with("Equipped ") {
        return message
            .replace("Equipped", "장착")
            .replace(" in ", " 슬롯 ");
    }
    if message.starts_with("Advanced to dungeon floor ") {
        return message
            .replace("Advanced to dungeon floor", "던전 층 진행")
            .replace('.', "");
    }
    if message.starts_with("Boss ") && message.contains(" defeated.") {
        return message
            .replace("Boss Goblin Chief", "보스 고블린")
            .replace("Boss Crystal Golem", "보스 골렘")
            .replace("Boss Ancient Lich", "보스 리치")
            .replace("Boss Cyclops Smith", "보스 키클롭스")
            .replace("Boss Titan Warden", "보스 감시자")
            .replace("Boss Goblin", "보스 고블린")
            .replace("Boss Golem", "보스 골렘")
            .replace("Boss Lich", "보스 리치")
            .replace("Boss Cyclops", "보스 키클롭스")
            .replace("Boss Medusa", "보스 메두사")
            .replace("Boss Warden", "보스 감시자")
            .replace("Boss", "보스")
            .replace(
                "defeated. First/repeat reward acquired:",
                "처치. 보상 획득:",
            )
            .replace('.', "");
    }
    if message.starts_with("Boss ") && message.contains(" is still guarding ") {
        return message
            .replace("Boss Goblin Chief", "보스 고블린")
            .replace("Boss Crystal Golem", "보스 골렘")
            .replace("Boss Ancient Lich", "보스 리치")
            .replace("Boss Cyclops Smith", "보스 키클롭스")
            .replace("Boss Titan Warden", "보스 감시자")
            .replace("Boss Goblin", "보스 고블린")
            .replace("Boss Golem", "보스 골렘")
            .replace("Boss Lich", "보스 리치")
            .replace("Boss Cyclops", "보스 키클롭스")
            .replace("Boss Medusa", "보스 메두사")
            .replace("Boss Warden", "보스 감시자")
            .replace("Boss", "보스")
            .replace(
                "is still guarding the dungeon core.",
                "아직 던전 핵을 지키고 있습니다",
            );
    }
    message.to_string()
}

fn ko_monster_label(monster: &str) -> &str {
    match monster {
        "Claude Glyph Imp" | "Imp" => "임프",
        "Dookka Burrower" | "Burrower" => "버로어",
        "Boss Goblin Chief" | "Boss Goblin" => "보스 고블린",
        "Boss Crystal Golem" | "Boss Golem" => "보스 골렘",
        "Boss Ancient Lich" | "Boss Lich" => "보스 리치",
        "Boss Cyclops Smith" | "Boss Cyclops" => "보스 키클롭스",
        "Boss Medusa" => "보스 메두사",
        "Boss Titan Warden" | "Boss Warden" => "보스 감시자",
        "Goblin Captain" | "Goblin Scout" | "Goblin" => "고블린",
        "Cave Bat Matriarch" | "Mine Bat" | "Bat" => "박쥐",
        "Poison Toad Elder" | "Swamp Toad" | "Toad" => "두꺼비",
        "Skeleton Warden" | "Titan Warden" | "Warden" => "감시자",
        "Training Golem" | "Target Golem" | "Golem" => "골렘",
        "Training Scarab" | "Scarab" => "딱정벌레",
        "Twig Imp" => "임프",
        "Moss Wolf" | "Wolf" => "늑대",
        "Ore Crawler" | "Crawler" => "크롤러",
        "Crystal Wisp" | "Wisp" => "위습",
        "Bog Witchling" | "Witch" => "마녀",
        "Bone Guard" | "Bone" => "해골",
        "Fallen Knight" | "Knight" => "기사",
        "Goblin Brute" | "Brute" => "브루트",
        "Crystal Gazer" | "Gazer" => "응시자",
        "Lich Acolyte" | "Lich" => "리치",
        "Ash Siren" | "Siren" => "세이렌",
        "Obsidian Crab" | "Crab" => "게",
        "Cyclops Apprentice" | "Cyclops" => "키클롭스",
        "Titan Raider" | "Raider" => "약탈자",
        "Bronze Hoplite" | "Hoplite" => "중장병",
        "Oracle Sphinx" | "Sphinx" => "스핑크스",
        "Gorgon Sentinel" | "Gorgon" => "고르곤",
        "Styx Ferryman" | "Ferryman" => "사공",
        "Eidolon Guard" | "Eidolon" => "에이돌론",
        "Olympus Sentinel" | "Sentinel" => "파수기",
        other => other,
    }
}

fn ko_dungeon_label(dungeon_id: &str) -> &'static str {
    match dungeon_id {
        "goblin-den" => "고블린 소굴",
        "crystal-cave" => "수정 동굴",
        "lich-tomb" => "리치 무덤",
        "cyclops-forge" => "키클롭스 대장간",
        "medusa-temple" => "메두사 신전",
        "titan-vault" => "티탄 금고",
        _ => "미지",
    }
}

fn ko_subject(label: &str) -> String {
    let particle = label
        .chars()
        .rev()
        .find(|ch| ('가'..='힣').contains(ch))
        .map(|ch| {
            if ((ch as u32) - ('가' as u32)).is_multiple_of(28) {
                "가"
            } else {
                "이"
            }
        })
        .unwrap_or("이");
    format!("{label}{particle}")
}

fn render_live_dashboard(conn: &rusqlite::Connection, args: &HudArgs) -> Result<String> {
    let snapshot = vibemud_db::load_snapshot(conn)?;
    let lang = ui_language();
    let dto = localized_status_dto(&snapshot, lang);
    let age = character_age_line(snapshot.clock_tick, lang);
    let logs = vibemud_db::recent_log_entries(conn, args.log_lines.max(1))?;
    let game_logs: Vec<_> = logs
        .iter()
        .filter(|entry| is_game_log_entry(entry))
        .cloned()
        .collect();
    let stats = vibemud_core::representative_stats(&snapshot.player);
    let hud = if args.side {
        render_side_panel_localized(&dto, !args.ascii, lang)
    } else {
        render_status_for_width(&snapshot, terminal_width(), lang)
    };
    let mut out = String::new();
    out.push_str(label(
        lang,
        "VibeMUD 라이브 대시보드  (Ctrl-C는 보기만 닫고 사냥은 계속 진행)\n",
        "VibeMUD Live Dashboard  (Ctrl-C closes the view; hunt keeps running)\n",
    ));
    out.push_str("------------------------------------------------------------\n");
    out.push_str(&hud);
    out.push('\n');
    out.push_str(&format!(
        "{} {}/{} | {} {} | MP {}/{} | {}\n",
        label(lang, "경험치", "XP"),
        snapshot.player.xp,
        snapshot.player.xp_to_next,
        label(lang, "골드", "Gold"),
        snapshot.player.gold,
        snapshot.player.mp,
        snapshot.player.max_mp,
        age
    ));
    out.push_str(&compact_stat_line(stats, lang));
    out.push('\n');
    out.push_str(label(
        lang,
        "---------------- 최근 게임/전투 메시지 --------------------\n",
        "---------------- Recent game/combat messages ---------------\n",
    ));
    for entry in game_logs.iter().rev() {
        out.push_str(&format!(
            "{:<17} {}\n",
            display_event_type(&entry.event_type, lang),
            display_message(&entry.message, lang)
        ));
    }
    out.push_str("------------------------------------------------------------\n");
    out.push_str(integration_hint(
        lang,
        "명령: mudctl status | mudctl log | vibemud session stop\n",
        "Commands: mudctl status | mudctl log | vibemud session stop\n",
        "명령: /vibemud:mud now | /vibemud:mud log | /vibemud:mud stop\n",
        "Commands: /vibemud:mud now | /vibemud:mud log | /vibemud:mud stop\n",
    ));
    Ok(out)
}

fn enqueue(
    conn: &rusqlite::Connection,
    kind: CommandKind,
    payload: CommandPayload,
    auto_start: bool,
) -> Result<()> {
    let id = vibemud_db::enqueue_command(conn, "mudctl", kind.clone(), &payload)?;
    vibemud_db::append_event(
        conn,
        vibemud_core::EventKind::CommandProcessed,
        format!("Action feedback: queued {}.", kind.as_str()),
        None,
    )?;
    vibemud_db::write_snapshot_and_hud(conn)?;
    let runtime = effective_runtime_status(conn)?;
    let lang = ui_language();
    if runtime == "running" {
        wait_for_immediate_transition(conn, &kind, &id)?;
        println!(
            "{} {}: {id}",
            label(lang, "대기열 등록", "Queued"),
            kind.as_str()
        );
    } else if auto_start {
        let pid = start_background_runtime(None)?;
        wait_for_immediate_transition(conn, &kind, &id)?;
        println!(
            "{} {}: {id}\n{} pid={pid}",
            label(lang, "대기열 등록", "Queued"),
            kind.as_str(),
            label(
                lang,
                "VibeMUD 백그라운드 런타임 시작",
                "VibeMUD background runtime started"
            )
        );
    } else if lang == UiLanguage::Ko {
        println!(
            "대기열 등록 {}: {id}\nVibeMUD 런타임이 실행 중이 아닙니다. 실행: vibemud session start --background",
            kind.as_str()
        );
    } else {
        println!(
            "Queued {}: {id}\nVibeMUD runtime is not running. Run: vibemud session start --background",
            kind.as_str()
        );
    }
    Ok(())
}

fn wait_for_immediate_transition(
    conn: &rusqlite::Connection,
    kind: &CommandKind,
    id: &str,
) -> Result<()> {
    if !matches!(
        kind,
        CommandKind::AreaEnter
            | CommandKind::HuntStart
            | CommandKind::DungeonEnter
            | CommandKind::Equip
            | CommandKind::Unequip
            | CommandKind::Enhance
            | CommandKind::ShopSell
            | CommandKind::SellCommon
            | CommandKind::ItemLock
            | CommandKind::ItemUnlock
    ) {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_millis(2_500);
    loop {
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM command_queue WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if matches!(status.as_deref(), Some("done" | "failed")) || Instant::now() >= deadline {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn print_area_list(conn: &rusqlite::Connection) -> Result<()> {
    let lang = ui_language();
    let mut stmt = conn.prepare(
        "SELECT id, name, recommended_level, danger_rating FROM areas ORDER BY recommended_level",
    )?;
    let rows = stmt.query_map([], |row| {
        let id = row.get::<_, String>(0)?;
        let danger = row.get::<_, String>(3)?;
        Ok(format!(
            "{} | {} | Lv{} | {}",
            id,
            if lang == UiLanguage::Ko {
                ko_area_label(&id).to_string()
            } else {
                row.get::<_, String>(1)?
            },
            row.get::<_, i64>(2)?,
            if lang == UiLanguage::Ko {
                ko_danger_label(&danger).to_string()
            } else {
                danger
            }
        ))
    })?;
    for row in rows {
        println!("{}", row?);
    }
    Ok(())
}

fn print_dungeon_list(conn: &rusqlite::Connection) -> Result<()> {
    let lang = ui_language();
    let mut stmt = conn.prepare("SELECT id, name, recommended_level, floors, boss_id FROM dungeons ORDER BY recommended_level")?;
    let rows = stmt.query_map([], |row| {
        let id = row.get::<_, String>(0)?;
        Ok(format!(
            "{} | {} | Lv{} | {} {} | {} {}",
            id,
            if lang == UiLanguage::Ko {
                ko_dungeon_label(&id).to_string()
            } else {
                row.get::<_, String>(1)?
            },
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            label(lang, "층", "floors"),
            label(lang, "보스", "boss"),
            row.get::<_, String>(4)?
        ))
    })?;
    for row in rows {
        println!("{}", row?);
    }
    Ok(())
}

fn print_party(conn: &rusqlite::Connection) -> Result<()> {
    let party = vibemud_db::load_party(conn)?;
    let lang = ui_language();
    if party.is_empty() {
        println!(
            "{}",
            label(lang, "파티: 플레이어만 있음", "Party: player only")
        );
    } else {
        for c in party {
            println!("{} | {} | {}", c.id, c.name, c.role);
        }
    }
    Ok(())
}

fn print_inventory(conn: &rusqlite::Connection) -> Result<()> {
    let inventory = vibemud_db::load_inventory(conn)?;
    let lang = ui_language();
    if inventory.is_empty() {
        println!(
            "{}",
            label(lang, "가방이 비어 있습니다", "Inventory is empty")
        );
    } else {
        for item in inventory {
            println!("{}", inventory_item_line(&item, lang));
        }
    }
    Ok(())
}

fn equipment_slot_order(lang: UiLanguage) -> [(&'static str, &'static str); 8] {
    [
        ("weapon", label(lang, "무기", "Weapon")),
        ("subweapon", label(lang, "부무기", "Subweapon")),
        ("armor_top", label(lang, "상의", "Top")),
        ("armor_bottom", label(lang, "하의", "Bottom")),
        ("trinket", label(lang, "장신구", "Trinket")),
        ("boots", label(lang, "신발", "Boots")),
        ("pet", label(lang, "펫", "Pet")),
        ("special", label(lang, "특수장비", "Special")),
    ]
}

fn equipped_item_for_slot<'a>(
    inventory: &'a [vibemud_core::InventoryItem],
    slot: &str,
) -> Option<&'a vibemud_core::InventoryItem> {
    inventory
        .iter()
        .find(|item| item.equipped_slot.as_deref() == Some(slot))
}

fn print_equipment_slots(conn: &rusqlite::Connection, raw: bool) -> Result<()> {
    let inventory = vibemud_db::load_inventory(conn)?;
    let lang = ui_language();
    if !raw {
        println!("{}", label(lang, "장착 장비", "Equipped Gear"));
    }
    for (slot, slot_label) in equipment_slot_order(lang) {
        if let Some(item) = equipped_item_for_slot(&inventory, slot) {
            let summary = equipment_colored_summary(item, lang);
            if raw {
                println!("{slot}|{slot_label}|equipped|{summary}");
            } else {
                print_equipment_table_row(slot_label, &summary);
            }
        } else {
            let summary = label(lang, "미착용", "Unequipped");
            if raw {
                println!("{slot}|{slot_label}|empty|{summary}");
            } else {
                print_equipment_table_row(slot_label, summary);
            }
        }
    }
    if raw {
        println!(
            "close|{}|action|{}",
            label(lang, "닫기", "Close"),
            label(lang, "장비창 닫기", "Close equipment pane")
        );
    }
    Ok(())
}

fn print_equipment_table_row(label_text: &str, value: &str) {
    println!("  {} │ {}", pad_chars(label_text, 12), value);
}

fn print_equipment_inventory(conn: &rusqlite::Connection) -> Result<()> {
    let inventory = vibemud_db::load_inventory(conn)?;
    let lang = ui_language();
    let mut count = 0;
    for item in inventory.iter().filter(|item| item.equipped_slot.is_none()) {
        count += 1;
        println!(
            "{}|{}|{}|{}|{}",
            item.id,
            equipment_colored_summary(item, lang),
            item.item_type,
            equipment_inventory_status(item, lang),
            if item.locked { "locked" } else { "unlocked" }
        );
    }
    if count == 0 {
        println!(
            "empty|{}|empty|{}",
            label(lang, "소지품 없음", "No items"),
            label(lang, "소지품창이 비어 있습니다", "Inventory is empty")
        );
    }
    println!(
        "empty_inventory|{}|action|{}|unlocked",
        label(lang, "일괄 비우기", "Empty inventory"),
        label(
            lang,
            "잠금 아이템을 제외하고 소지품을 모두 판매",
            "Sell all unlocked unequipped items"
        )
    );
    println!(
        "close|{}|action|{}",
        label(lang, "닫기", "Close"),
        label(lang, "장비창으로 돌아가기", "Return to equipment")
    );
    Ok(())
}

fn print_equipment_gold(conn: &rusqlite::Connection) -> Result<()> {
    let player = vibemud_db::load_player(conn)?;
    println!("{}", player.gold);
    Ok(())
}

fn print_quests(conn: &rusqlite::Connection, raw: bool) -> Result<()> {
    let quests = vibemud_db::load_daily_quests(conn)?;
    let lang = ui_language();
    if raw {
        for quest in quests {
            println!(
                "{}|{}|{}|{}|{}|{}|{}|{}",
                quest.quest_id,
                quest.status,
                quest.title,
                quest.progress,
                quest.target,
                quest.reward_kind,
                quest.reward_amount,
                quest.fever_minutes
            );
        }
        println!(
            "claim_all|action|{}|0|0|action|0|0",
            label(lang, "완료 보상 일괄 수령", "Claim all completed rewards")
        );
        println!(
            "close|action|{}|0|0|action|0|0",
            label(lang, "닫기", "Close")
        );
        return Ok(());
    }

    println!("{}", label(lang, "일일 퀘스트", "Daily Quests"));
    println!(
        "{}",
        label(
            lang,
            "매일 24:00에 5개 자동 갱신 · 완료 후 보상 수령",
            "5 quests refresh daily at midnight; claim rewards after completion"
        )
    );
    for quest in quests {
        let state = quest_status_label(&quest.status, lang);
        println!(
            "  [{}] {} {}/{} · {} · FEVERTIME +{}분",
            state,
            quest.title,
            quest.progress.min(quest.target),
            quest.target,
            quest_reward_label(&quest.reward_kind, quest.reward_amount, lang),
            quest.fever_minutes
        );
    }
    Ok(())
}

fn quest_status_label(status: &str, lang: UiLanguage) -> &'static str {
    match (status, lang) {
        ("completed", UiLanguage::Ko) => "완료",
        ("completed", UiLanguage::En) => "done",
        ("claimed", UiLanguage::Ko) => "수령",
        ("claimed", UiLanguage::En) => "claimed",
        (_, UiLanguage::Ko) => "진행",
        _ => "active",
    }
}

fn quest_reward_label(kind: &str, amount: i64, lang: UiLanguage) -> String {
    match (kind, lang) {
        ("xp", UiLanguage::Ko) => format!("경험치 +{amount}"),
        ("xp", UiLanguage::En) => format!("XP +{amount}"),
        (_, UiLanguage::Ko) => format!("머니 +{amount}"),
        _ => format!("gold +{amount}"),
    }
}

fn print_equipment_stats(conn: &rusqlite::Connection, raw: bool) -> Result<()> {
    let player = vibemud_db::load_player(conn)?;
    let stats = vibemud_core::representative_stats(&player);
    let lang = ui_language();
    let rows = [
        (
            "gold",
            label(lang, "보유 골드", "Gold"),
            player.gold.to_string(),
            "power",
            label(lang, "전투력", "Power"),
            stats.combat_power.to_string(),
        ),
        (
            "attack",
            label(lang, "공격력", "Attack"),
            player.attack.to_string(),
            "defense",
            label(lang, "방어력", "Defense"),
            player.defense.to_string(),
        ),
        (
            "accuracy",
            label(lang, "명중", "Accuracy"),
            player.accuracy.to_string(),
            "evasion",
            label(lang, "저항", "Resistance"),
            player.evasion.to_string(),
        ),
        (
            "speed",
            label(lang, "속도", "Speed"),
            player.speed.to_string(),
            "luck",
            label(lang, "행운", "Luck"),
            player.luck.to_string(),
        ),
        (
            "regen",
            label(lang, "재생", "Regen"),
            player.regen.to_string(),
            "hp",
            "HP",
            format!("{}/{}", player.hp.max(0), player.max_hp),
        ),
        (
            "mp",
            "MP",
            format!("{}/{}", player.mp.max(0), player.max_mp),
            "xp",
            label(lang, "경험치", "XP"),
            format!("{}/{}", player.xp, player.xp_to_next),
        ),
    ];
    for (left_id, left_label, left_value, right_id, right_label, right_value) in rows {
        if raw {
            println!("{left_id}|{left_label}|{left_value}|{right_id}|{right_label}|{right_value}");
        } else {
            print_equipment_stat_pair(left_label, &left_value, right_label, &right_value);
        }
    }
    Ok(())
}

fn print_equipment_stat_pair(
    left_label: &str,
    left_value: &str,
    right_label: &str,
    right_value: &str,
) {
    let left = format!("{left_label}: {left_value}");
    let right = format!("{right_label}: {right_value}");
    println!("  {} │ {}", pad_chars(&left, 22), pad_chars(&right, 22));
}

fn print_equipment_tooltip(conn: &rusqlite::Connection, slot: &str) -> Result<()> {
    let inventory = vibemud_db::load_inventory(conn)?;
    let lang = ui_language();
    let Some(item) = equipped_item_for_slot(&inventory, slot) else {
        println!(
            "{}",
            label(lang, "미착용 슬롯입니다.", "This slot is empty.")
        );
        return Ok(());
    };
    print_equipment_tooltip_body(conn, item, Some(slot), lang)
}

fn print_equipment_item_tooltip(conn: &rusqlite::Connection, item_id: &str) -> Result<()> {
    let inventory = vibemud_db::load_inventory(conn)?;
    let lang = ui_language();
    let Some(item) = inventory
        .iter()
        .find(|item| item.id == item_id || item.item_id == item_id)
    else {
        println!(
            "{}",
            label(lang, "아이템을 찾을 수 없습니다.", "Item not found.")
        );
        return Ok(());
    };
    print_equipment_tooltip_body(conn, item, item.equipped_slot.as_deref(), lang)
}

fn print_equipment_tooltip_body(
    conn: &rusqlite::Connection,
    item: &vibemud_core::InventoryItem,
    equipped_slot: Option<&str>,
    lang: UiLanguage,
) -> Result<()> {
    let slot = equipped_slot.unwrap_or(item.item_type.as_str());
    println!("{}", equipment_colored_summary(item, lang));
    println!(
        "{}: {} | {}: {} | {}: {}",
        colorize(label(lang, "슬롯", "Slot"), AnsiColor::Gray),
        colorize(localized_slot(slot, lang), AnsiColor::White),
        colorize(label(lang, "등급", "Rarity"), AnsiColor::Gray),
        colorize_by_rarity(
            &item.rarity,
            item.rarity_color.as_deref().unwrap_or(&item.rarity)
        ),
        colorize(label(lang, "강화", "Enhance"), AnsiColor::Gray),
        colorize(&format!("+{}", item.enhancement_level), AnsiColor::White)
    );
    let stats = equipment_stat_summary(item, lang);
    if !stats.is_empty() {
        println!(
            "{}: {}",
            colorize(label(lang, "능력치", "Stats"), AnsiColor::Gray),
            colorize(&stats, AnsiColor::White)
        );
    }
    if let Some(power) = item.power_score {
        println!(
            "{}: {}",
            colorize(label(lang, "전투력", "Power"), AnsiColor::Gray),
            colorize(&power.to_string(), AnsiColor::White)
        );
    }
    if let Some(rule) =
        vibemud_db::adjusted_enhancement_rule(conn, item.enhancement_level, &item.rarity, slot)?
    {
        match (rule.upgrade_gold_cost, rule.success_rate) {
            (Some(cost), Some(rate)) => println!(
                "{}: {} {} | {} {:.0}%",
                colorize(label(lang, "다음 강화 비용", "Next cost"), AnsiColor::Gray),
                colorize(&cost.to_string(), AnsiColor::White),
                colorize(label(lang, "골드", "gold"), AnsiColor::Gray),
                colorize(label(lang, "성공률", "Success"), AnsiColor::Gray),
                rate * 100.0
            ),
            _ => println!(
                "{}",
                label(lang, "최대 강화 단계입니다.", "Max enhancement.")
            ),
        }
    }
    Ok(())
}

fn equipment_plain_summary(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> String {
    let mut display = localized_item_name(&item.name, lang);
    display = format!("{} +{}", display, item.enhancement_level);
    if let Some(tier) = item.tier {
        display = format!("{} T{}", display, tier);
    }
    if item.locked {
        display = format!("{} {display}", label(lang, "[잠금]", "[LOCKED]"));
    }
    display
}

fn equipment_colored_summary(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> String {
    colorize_by_rarity(
        &equipment_plain_summary(item, lang),
        item.rarity_color.as_deref().unwrap_or(&item.rarity),
    )
}

fn equipment_inventory_status(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> String {
    let mut parts = Vec::new();
    parts.extend(equipment_stat_parts(item, lang));
    if let Some(power) = item.power_score {
        parts.push(format!("{} {}", label(lang, "전투력", "Power"), power));
    }
    if parts.is_empty() {
        label(lang, "장비", "Equipment").to_string()
    } else {
        parts.join(" / ")
    }
}

fn equipment_stat_summary(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> String {
    equipment_stat_parts(item, lang).join(", ")
}

fn equipment_stat_parts(item: &vibemud_core::InventoryItem, lang: UiLanguage) -> Vec<String> {
    [
        (&item.stat1_type, item.stat1_value),
        (&item.stat2_type, item.stat2_value),
        (&item.stat3_type, item.stat3_value),
    ]
    .into_iter()
    .filter_map(|(stat, value)| {
        let stat = stat.as_deref()?;
        let value = value?;
        Some(format_stat_bonus(stat, value, lang))
    })
    .collect()
}

fn format_stat_bonus(stat: &str, value: i32, lang: UiLanguage) -> String {
    if matches!(stat, "xp_bonus" | "gold_bonus") {
        format!("{} +{}%", localized_stat(stat, lang), value)
    } else {
        format!("{} +{}", localized_stat(stat, lang), value)
    }
}

fn print_shop() -> Result<()> {
    if ui_language() == UiLanguage::Ko {
        println!("potion-small | 아스클레피오스 물약 | 25 골드\npotion-medium | 히게이아 물약 | 55 골드\nbasic-sword | 아레스 검 | 80 골드\nbasic-staff | 헤르메스 지팡이 | 80 골드\nleather-armor | 레오니다스 갑옷 | 90 골드\nrepair-kit | 다이달로스 도구 | 40 골드");
    } else {
        println!("potion-small | Asclepius Potion | 25 gold\npotion-medium | Hygieia Potion | 55 gold\nbasic-sword | Ares Sword | 80 gold\nbasic-staff | Hermes Staff | 80 gold\nleather-armor | Leonidas Armor | 90 gold\nrepair-kit | Daedalus Kit | 40 gold");
    }
    Ok(())
}

fn print_log(conn: &rusqlite::Connection, tail: usize, follow: bool) -> Result<()> {
    let tail = tail.max(1);
    let lang = ui_language();
    let mut last_seen = 0;
    loop {
        let entries = vibemud_db::recent_log_entries(conn, tail)?;
        for entry in entries.iter().rev() {
            if (!follow || entry.state_version > last_seen)
                && !is_suppressed_log_message(&entry.message)
            {
                println!(
                    "#{:<4} {} {:<18} {}",
                    entry.state_version,
                    entry.created_at,
                    display_event_type(&entry.event_type, lang),
                    display_message(&entry.message, lang)
                );
            }
        }
        if let Some(max) = entries.iter().map(|entry| entry.state_version).max() {
            last_seen = last_seen.max(max);
        }
        if !follow {
            break;
        }
        thread::sleep(Duration::from_secs(1));
    }
    Ok(())
}

fn print_system(conn: &rusqlite::Connection) -> Result<()> {
    let snapshot = vibemud_db::load_snapshot(conn)?;
    let lang = ui_language();
    let dto = localized_status_dto(&snapshot, lang);
    let stats = vibemud_core::representative_stats(&snapshot.player);
    println!("{}", label(lang, "VibeMUD 시스템", "VibeMUD System"));
    println!(
        "{}: Lv.{} {} | {} | {} | HP {}/{} | {} {}/{} | {} {}",
        label(lang, "게임", "Game"),
        snapshot.player.level,
        dto.class_label,
        dto.area_label,
        dto.mode_label,
        snapshot.player.hp,
        snapshot.player.max_hp,
        label(lang, "경험치", "XP"),
        snapshot.player.xp,
        snapshot.player.xp_to_next,
        label(lang, "골드", "Gold"),
        snapshot.player.gold
    );
    println!("{}", character_age_line(snapshot.clock_tick, lang));
    println!("{}", compact_stat_line(stats, lang));
    println!("{}", label(lang, "최근:", "Recent:"));
    for entry in vibemud_db::recent_log_entries(conn, 10)?
        .iter()
        .rev()
        .filter(|entry| !is_suppressed_log_message(&entry.message))
        .take(5)
    {
        println!("- {}", display_message(&entry.message, lang));
    }
    Ok(())
}

fn print_queue(conn: &rusqlite::Connection, tail: usize) -> Result<()> {
    let counts = vibemud_db::command_queue_counts(conn)?;
    let lang = ui_language();
    println!(
        "{}: {}={} {}={} {}={} {}={}",
        label(lang, "큐", "Queue"),
        label(lang, "대기", "pending"),
        counts.pending,
        label(lang, "처리중", "processing"),
        counts.processing,
        label(lang, "완료", "done"),
        counts.done,
        label(lang, "실패", "failed"),
        counts.failed
    );
    let entries = vibemud_db::recent_queue_entries(conn, tail.max(1))?;
    if entries.is_empty() {
        println!(
            "{}",
            label(
                lang,
                "아직 등록된 명령이 없습니다",
                "No queued commands yet"
            )
        );
        return Ok(());
    }
    for entry in entries {
        let short_id = entry.id.chars().take(8).collect::<String>();
        let detail = entry
            .error_message
            .as_deref()
            .or(entry.result_json.as_deref())
            .unwrap_or("");
        println!(
            "{} {} {:<10} {:<14} source={} processed={} {}",
            short_id,
            entry.created_at,
            localized_status_word(&entry.status, lang),
            entry.command_type,
            entry.source,
            entry.processed_at.as_deref().unwrap_or("-"),
            display_message(detail, lang)
        );
    }
    Ok(())
}

fn print_aliases() -> Result<()> {
    println!("상태=status\n사냥=hunt\n캐릭터=stats\n닫기=close\n지도=map\n정지=stop\n숲가장자리=forest-edge\n낡은광산=old-mine\n안개늪=misty-swamp\n무너진요새=fallen-fortress\n흑요해안=obsidian-coast\n티탄초원=titan-steppe\n예언자유적=oracle-ruins\n스틱스늪=styx-marsh\n올림포스관문=olympus-gate\n고블린소굴=goblin-den\n수정동굴=crystal-cave\n리치무덤=lich-tomb\n키클롭스대장간=cyclops-forge\n메두사신전=medusa-temple\n티탄금고=titan-vault");
    Ok(())
}

fn start_agent_layout(cli: &str) -> Result<()> {
    if cfg!(windows) || env_truthy("VIBEMUD_FORCE_WINDOWS_TERMINAL_LAYOUT") {
        start_windows_terminal_layout(cli)
    } else {
        start_tmux_layout(cli)
    }
}

fn start_windows_terminal_layout(cli: &str) -> Result<()> {
    if let Some(reason) = agent_layout_guard_reason(cli) {
        anyhow::bail!("{reason}");
    }

    let wt = command_path("wt")
        .or_else(|| command_path("wt.exe"))
        .context(
            "Windows Terminal (wt.exe) is required for native PowerShell pane layout integration",
        )?;
    let powershell = command_path("powershell")
        .or_else(|| command_path("powershell.exe"))
        .or_else(|| command_path("pwsh"))
        .or_else(|| command_path("pwsh.exe"))
        .unwrap_or_else(|| PathBuf::from("powershell.exe"));
    let vibemud = current_or_path("vibemud");
    let cli_command = powershell_command_invocation(&current_or_path(cli), cli, &[]);
    let hud_command = powershell_command_invocation(
        &vibemud,
        "vibemud",
        &["hud", "--panel", "--refresh", "1", "--log-lines", "999"],
    );

    let _ = vibemud_runtime::start_runtime(Some(1));

    let powershell_text = powershell.to_string_lossy().to_string();
    let cwd = std::env::current_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let mut args: Vec<String> = Vec::new();
    if inside_windows_terminal() {
        args.extend(["-w".into(), "0".into()]);
    }
    args.extend([
        "new-tab".into(),
        "--title".into(),
        format!("{cli} + VibeMUD"),
        "-d".into(),
        cwd.clone(),
        powershell_text.clone(),
        "-NoExit".into(),
        "-Command".into(),
        cli_command,
        ";".into(),
        "split-pane".into(),
        "-H".into(),
        "--title".into(),
        "VibeMUD HUD".into(),
        "-d".into(),
        cwd,
        powershell_text,
        "-NoExit".into(),
        "-Command".into(),
        hud_command,
    ]);

    let output = Command::new(&wt).args(&args).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "wt.exe {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    println!(
        "VibeMUD Windows Terminal layout started for {cli}; HUD is in the right PowerShell pane."
    );
    Ok(())
}

fn inside_windows_terminal() -> bool {
    // WT_SESSION is inherited by nested apps (e.g. VS Code/Cursor launched from
    // a Windows Terminal tab), so it alone does not prove this process renders
    // inside WT. VS Code style integrated terminals set TERM_PROGRAM (e.g.
    // "vscode"); real Windows Terminal leaves it unset or sets
    // "WindowsTerminal". Passing `-w 0` from a VS Code terminal would inject
    // panes into an unrelated WT window.
    if std::env::var_os("WT_SESSION").is_none() {
        return false;
    }
    match std::env::var("TERM_PROGRAM") {
        Ok(value) => value.is_empty() || value.eq_ignore_ascii_case("WindowsTerminal"),
        Err(_) => true,
    }
}

fn start_tmux_layout(cli: &str) -> Result<()> {
    if let Some(reason) = agent_layout_guard_reason(cli) {
        anyhow::bail!("{reason}");
    }

    let tmux = command_path("tmux").context("tmux is required for session layout integration")?;
    let vibemud = current_or_path("vibemud");
    let hud_command = format!(
        "{} hud --panel --refresh 1 --log-lines 999",
        shell_arg(&vibemud.to_string_lossy())
    );
    let status_command = format!("#({} statusline)", shell_arg(&vibemud.to_string_lossy()));

    let _ = vibemud_runtime::start_runtime(Some(1));

    if std::env::var("TMUX").is_ok() {
        run_tmux(&tmux, &["split-window", "-h", "-p", "40", &hud_command])?;
        let _ = run_tmux(
            &tmux,
            &["set-option", "-g", "status-right", &status_command],
        );
        run_tmux(&tmux, &["send-keys", "-t", ":.0", cli, "Enter"])?;
        println!("VibeMUD tmux layout started for {cli}; HUD is in the right pane.");
    } else {
        let session = format!("vibemud-{cli}");
        let _ = Command::new(&tmux)
            .args(["kill-session", "-t", &session])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        run_tmux(&tmux, &["new-session", "-d", "-s", &session, cli])?;
        run_tmux(
            &tmux,
            &[
                "split-window",
                "-t",
                &session,
                "-h",
                "-p",
                "40",
                &hud_command,
            ],
        )?;
        let _ = run_tmux(
            &tmux,
            &[
                "set-option",
                "-t",
                &session,
                "status-right",
                &status_command,
            ],
        );
        if std::env::var("VIBEMUD_TMUX_ATTACH").as_deref() == Ok("0") {
            println!("VibeMUD tmux session {session} created for {cli}.");
        } else {
            println!("Attaching to VibeMUD tmux session {session}.");
            let status = Command::new(&tmux)
                .args(["attach-session", "-t", &session])
                .status()?;
            if !status.success() {
                anyhow::bail!("tmux attach failed with status {status}");
            }
        }
    }
    Ok(())
}

fn agent_layout_guard_reason(cli: &str) -> Option<&'static str> {
    if !matches!(cli, "codex" | "claude") {
        return None;
    }
    if env_truthy("VIBEMUD_ALLOW_NESTED_AGENT_LAYOUT") {
        return None;
    }
    if running_inside_agent_runtime() {
        return Some(
            "Refusing to start a nested Codex/Claude session from an active agent runtime. \
Use VIBEMUD_ALLOW_NESTED_AGENT_LAYOUT=1 only if you intentionally want nesting.",
        );
    }
    None
}

fn running_inside_agent_runtime() -> bool {
    [
        "OMX_SESSION_ID",
        "CODEX_THREAD_ID",
        "CODEX_CI",
        "CLAUDECODE",
        "CLAUDE_CODE",
        "CLAUDE_SESSION_ID",
        "CLAUDE_PLUGIN_ROOT",
    ]
    .iter()
    .any(|name| std::env::var_os(name).is_some())
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.as_str(),
                "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON"
            )
        })
        .unwrap_or(false)
}

fn run_tmux(tmux: &PathBuf, args: &[&str]) -> Result<()> {
    let output = Command::new(tmux).args(args).output()?;
    if !output.status.success() {
        anyhow::bail!(
            "tmux {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

fn command_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if command_candidate_exists(&candidate) {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{name}.exe"));
            if command_candidate_exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn command_candidate_exists(candidate: &Path) -> bool {
    if candidate.is_file() {
        return true;
    }
    #[cfg(windows)]
    {
        // Windows Store app execution aliases (e.g. wt.exe under WindowsApps)
        // are IO_REPARSE_TAG_APPEXECLINK reparse points that fs::metadata may
        // fail to traverse, so is_file() reports false even though the alias
        // launches fine via CreateProcess. symlink_metadata still sees them.
        return std::fs::symlink_metadata(candidate)
            .map(|meta| !meta.is_dir())
            .unwrap_or(false);
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn current_or_path(name: &str) -> PathBuf {
    if let Ok(current) = std::env::current_exe() {
        if let Some(dir) = current.parent() {
            let binary = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_string()
            };
            let sibling = dir.join(binary);
            if sibling.exists() {
                return sibling;
            }
        }
    }
    PathBuf::from(name)
}

fn shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn powershell_command_invocation(
    command_path: &std::path::Path,
    fallback: &str,
    args: &[&str],
) -> String {
    let executable = if command_path.exists() {
        format!("& {}", powershell_arg(&command_path.to_string_lossy()))
    } else {
        fallback.to_string()
    };
    std::iter::once(executable)
        .chain(args.iter().map(|arg| powershell_arg(arg)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn powershell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn print_config_value(key: &str) -> Result<()> {
    let paths = vibemud_db::init_app()?;
    let text = std::fs::read_to_string(paths.config)?;
    let value: toml::Value = toml::from_str(&text)?;
    if let Some(v) = key
        .split('.')
        .try_fold(&value, |current, part| current.get(part))
    {
        println!("{key} = {v}");
        return Ok(());
    }
    let default_text = toml::to_string_pretty(&vibemud_db::load_config()?)?;
    let default_value: toml::Value = toml::from_str(&default_text)?;
    match key
        .split('.')
        .try_fold(&default_value, |current, part| current.get(part))
    {
        Some(v) => println!("{key} = {v}"),
        None => println!("{key} is not set"),
    }
    Ok(())
}

fn set_config_value(key: &str, value: &str) -> Result<()> {
    write_config_value(key, value)?;
    println!("{key} = {value}");
    Ok(())
}

fn write_config_value(key: &str, value: &str) -> Result<()> {
    let paths = vibemud_db::init_app()?;
    let text = std::fs::read_to_string(&paths.config)?;
    let mut document: toml::Value = toml::from_str(&text)?;
    set_toml_path(&mut document, key, parse_toml_scalar(value));
    std::fs::write(&paths.config, toml::to_string_pretty(&document)?)?;
    let (_paths, conn) = vibemud_db::open_app()?;
    conn.execute(
        "INSERT INTO settings(key, value, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        rusqlite::params![key, value, vibemud_db::now()],
    )?;
    Ok(())
}

fn run_setup(args: SetupArgs) -> Result<()> {
    let current = current_config_for_setup();
    let mut values = setup_values_from_args(&args, &current);
    let can_prompt = !args.yes && std::io::stdin().is_terminal() && std::io::stdout().is_terminal();

    if can_prompt {
        values.language = prompt_choice(
            "Language / 언어",
            &[("ko", "한국어"), ("en", "English")],
            &values.language,
        )?;
        let ko = values.language == "ko";
        values.agent = prompt_choice(
            if ko {
                "사용 환경"
            } else {
                "Agent environment"
            },
            &[
                ("auto", "Auto"),
                ("claude", "Claude Code"),
                ("codex", "Codex"),
                ("cli", "CLI only"),
            ],
            &values.agent,
        )?;
        values.terminal = prompt_choice(
            if ko { "터미널" } else { "Terminal" },
            &[
                ("auto", "Auto"),
                ("tmux", "tmux"),
                ("ghostty", "Ghostty"),
                ("windows-terminal", "Windows Terminal"),
                ("plain", "Plain terminal"),
            ],
            &values.terminal,
        )?;
        values.storage = prompt_choice(
            if ko {
                "저장 위치"
            } else {
                "Storage location"
            },
            &[("user", "User default"), ("project", "Project .vibemud")],
            &values.storage,
        )?;
        values.storage_root = storage_root_for(&values.storage);
        values.hud_mode = default_hud_for_terminal(&values.terminal);
        values.popup_panes = default_popup_for(&values.terminal, &values.hud_mode);
    }

    apply_setup_storage(&values)?;
    apply_setup_values(&values)?;
    print_setup_summary(&values);
    if !args.yes && std::io::stdout().is_terminal() && !intro_seen()? {
        run_intro(IntroArgs {
            replay: false,
            if_needed: false,
            fast: false,
        })?;
    }
    Ok(())
}

const INTRO_SEEN_KEY: &str = "onboarding.intro_seen";

const PLANET64_INTRO_FRAMES: &[&[&str]] = &[
    &[
        "        ✦                         ·                  ✦       ",
        "                    .      PLANET 64      .                  ",
        "             _..--''''--.._          EARTH SIGNAL: 09%       ",
        "          .-'   .-''''-.   '-.                              ",
        "        .'     /  .--.  \\     '.          *                 ",
        "       /      |  ( 64 )  |      \\                           ",
        "      ;       |   '--'   |       ;      frontier colony      ",
        "      |        \\  ____  /        |      population: awake    ",
        "      ;         '.___.'         ;                            ",
        "       \\     .-._     _.-.     /        orbital dust: high   ",
        "        '.  /    '---'    \\  .'                              ",
        "          '-.._        _..-'             ·                  ",
        "               '''--'''                                      ",
        "        .          old earth is only a blue myth              ",
    ],
    &[
        "    ·                    ✦                    ·              ",
        "                 P L A N E T   6 4                           ",
        "                                                               ",
        "              .==========================.                    ",
        "          _.-'   .----.    ||    .----.   '-._                ",
        "       .-'      /  64  \\   ||   / EARTH\\      '-.             ",
        "      /        |  ____  |  ||  | SIGNAL |        \\            ",
        "     ;         | /____\\ |  ||  |  27%   |         ;           ",
        "     |          \\      /   ||   \\______/          |           ",
        "     ;       ____'----'____||____'----'____       ;           ",
        "      \\     /___ orbital relay waking ___\\      /             ",
        "       '-._        ten-year gate cycle       _.-'              ",
        "            '---.______________________.---'                  ",
        "                    archive lights online                      ",
    ],
    &[
        "      ✦              THE EARTHBOUND TOURNAMENT              ✦ ",
        "                                                               ",
        "          ___      .-------------------------.      ___        ",
        "         / _ \\____/   O R B I T A L  G A T E \\____/ _ \\       ",
        "        | |_| |  _|   10 YEAR RETURN CYCLE   |_  | |_| |      ",
        "        |  _  | | |___________________________| | |  _  |      ",
        "        |_| |_|_|/     .-.       .-.       .-. \\|_| |_|_|      ",
        "             \\        ( 1 )     ( 0 )     ( 1 )        /       ",
        "              '--------'-'-------'-'-------'-'--------'        ",
        "                    ||        ||        ||                     ",
        "              ______||________||________||______               ",
        "             / challengers age 10 through 90  \\              ",
        "            /__ ten attempts before archive ___\\              ",
        "                 crowd noise behind sealed glass               ",
    ],
    &[
        "   *                  P L A N E T   6 4                  *    ",
        "             T H E   E A R T H B O U N D   T R I A L          ",
        "                                                               ",
        "        ╔══════════════════════════════════════════════╗        ",
        "        ║  ORBITAL QUALIFIER GATE        DEST: EARTH  ║        ",
        "        ║  ┌────┐ ┌────┐ ┌────┐ ┌────┐ ┌────┐        ║        ",
        "        ║  │ 01 │ │ 02 │ │ 03 │ │ 04 │ │ 05 │  ...   ║        ",
        "        ║  └────┘ └────┘ └────┘ └────┘ └────┘        ║        ",
        "        ║         [ AGE 10 ]     ATTEMPTS: 01 / 10      ║        ",
        "        ╚═══════════════╦════════════════╦════════════╝        ",
        "                        ║    ┌────┐      ║                     ",
        "                    ____║____│ 64 │__ ___║____                 ",
        "                   /  archivist desk  / challenger \\           ",
        "                  /__________________/______________\\          ",
    ],
];

const INTRO_NARRATION_KO: &[&str] = &[
    "2226년, 인류의 변방 식민지 플래닛 64.",
    "이 행성에서 태어난 아이들은 지구를 하늘의 전설로만 배운다.",
    "지구로 가는 길은 단 하나. 10년에 한 번 열리는 귀환전.",
    "10살부터 90살까지, 총 10번의 도전권.",
    "100살이 되기 전 지구행 자격을 얻지 못하면 이름은 행성 기록실에만 남는다.",
];

const INTRO_NARRATION_EN: &[&str] = &[
    "Year 2226, the frontier colony known as Planet 64.",
    "Children born here learn of Earth as a blue myth in the sky.",
    "There is only one road back: the tournament held once every ten years.",
    "From age 10 to 90, each challenger receives ten attempts.",
    "Fail before age 100, and your name remains only in the planetary archive.",
];

const INTRO_DIALOGUE_KO: &[(&str, &str, &str)] = &[
    (
        "left",
        "기록관",
        "첫 번째 귀환전이 시작된다. 네 이름을 기록해도 되겠나?",
    ),
    ("right", "도전자", "기록해주세요. 저는 지구로 갈 겁니다."),
    (
        "left",
        "기록관",
        "기회는 열 번뿐이다. 조급함보다 오래 살아남는 법을 배워라.",
    ),
    ("right", "도전자", "그럼 열 번 안에 강해지겠습니다."),
    (
        "system",
        "SYSTEM",
        "첫 번째 도전이 등록되었습니다. AGE 10 · ATTEMPT 01/10",
    ),
];

const INTRO_DIALOGUE_EN: &[(&str, &str, &str)] = &[
    (
        "left",
        "Archivist",
        "The first Earthbound Tournament begins. May I record your name?",
    ),
    ("right", "Challenger", "Record it. I am going to Earth."),
    (
        "left",
        "Archivist",
        "You have only ten chances. Learn endurance before glory.",
    ),
    (
        "right",
        "Challenger",
        "Then I will grow stronger before the tenth.",
    ),
    (
        "system",
        "SYSTEM",
        "First attempt registered. AGE 10 · ATTEMPT 01/10",
    ),
];

fn run_intro(args: IntroArgs) -> Result<()> {
    let (_paths, conn) = vibemud_db::open_app()?;
    if !args.replay && intro_seen_with_conn(&conn)? {
        if !args.if_needed {
            println!(
                "{}",
                label(
                    ui_language(),
                    "오프닝은 이미 재생되었습니다. 다시 보려면 `vibemud intro --replay`를 실행하세요.",
                    "Intro already seen. Run `vibemud intro --replay` to watch it again."
                )
            );
        }
        return Ok(());
    }

    play_intro(
        ui_language(),
        args.fast || std::env::var_os("VIBEMUD_INTRO_FAST").is_some(),
    )?;
    vibemud_db::set_setting(&conn, INTRO_SEEN_KEY, "true")?;
    Ok(())
}

fn intro_seen() -> Result<bool> {
    let (_paths, conn) = vibemud_db::open_app()?;
    intro_seen_with_conn(&conn)
}

fn intro_seen_with_conn(conn: &rusqlite::Connection) -> Result<bool> {
    Ok(matches!(
        vibemud_db::setting_value(conn, INTRO_SEEN_KEY)?.as_deref(),
        Some("true" | "1" | "yes" | "on")
    ))
}

fn play_intro(lang: UiLanguage, fast: bool) -> Result<()> {
    let mut stdout = std::io::stdout();
    let frame_delay = if fast { 0 } else { 430 };
    let line_delay = if fast { 0 } else { 1850 };
    let dialogue_delay = if fast { 0 } else { 2200 };
    let final_delay = if fast { 0 } else { 2400 };

    for frame in PLANET64_INTRO_FRAMES.iter().cycle().take(8) {
        render_intro_screen(&mut stdout, frame, &[])?;
        sleep_ms(frame_delay);
    }

    let narration = if lang == UiLanguage::Ko {
        INTRO_NARRATION_KO
    } else {
        INTRO_NARRATION_EN
    };
    let mut lines = Vec::new();
    for line in narration {
        lines.push(*line);
        render_intro_screen(&mut stdout, PLANET64_INTRO_FRAMES[3], &lines)?;
        sleep_ms(line_delay);
    }

    let dialogue = if lang == UiLanguage::Ko {
        INTRO_DIALOGUE_KO
    } else {
        INTRO_DIALOGUE_EN
    };
    for (side, speaker, text) in dialogue {
        render_dialogue_screen(&mut stdout, PLANET64_INTRO_FRAMES[3], side, speaker, text)?;
        sleep_ms(dialogue_delay);
    }

    render_intro_screen(
        &mut stdout,
        PLANET64_INTRO_FRAMES[3],
        &[label(
            lang,
            "목표: 첫 번째 토너먼트까지 살아남고 강해지세요. `mudctl hunt start --area forest-edge --auto-start`",
            "Goal: survive and grow before the first tournament. `mudctl hunt start --area forest-edge --auto-start`",
        )],
    )?;
    sleep_ms(final_delay);
    Ok(())
}

fn sleep_ms(ms: u64) {
    if ms > 0 {
        thread::sleep(Duration::from_millis(ms));
    }
}

#[derive(Debug, Clone)]
struct DevTournamentParticipant {
    name: &'static str,
    class_id: &'static str,
    level: u64,
    hp: i32,
    max_hp: i32,
    mp: i32,
    max_mp: i32,
    is_player: bool,
}

fn run_dev_tournament(args: DevTournamentArgs) -> Result<()> {
    let lang = ui_language();
    let width = args.width.clamp(80, 120);
    let inner = width.saturating_sub(2);
    let participants = dev_tournament_participants(args.age);
    let frames = vec![
        render_dev_tournament_gate(args.age, inner, lang),
        render_dev_tournament_bracket(args.age, inner, 0, lang),
        render_dev_tournament_bracket(args.age, inner, 1, lang),
        render_dev_tournament_player_match(
            args.age,
            inner,
            &participants[0],
            &participants[1],
            lang,
        ),
        render_dev_tournament_bracket(args.age, inner, 4, lang),
        render_dev_tournament_result(args.age, inner, lang),
    ];

    for (index, frame) in frames.iter().enumerate() {
        if std::io::stdout().is_terminal() {
            print!("\x1b[2J\x1b[H");
        } else if index > 0 {
            println!();
            println!("--- dev tournament frame {} ---", index + 1);
        }
        for line in frame {
            println!("{line}");
        }
        std::io::stdout().flush()?;
        if index + 1 < frames.len() {
            sleep_ms(if args.fast { 0 } else { 900 });
        }
    }
    Ok(())
}

fn dev_tournament_participants(age: u64) -> Vec<DevTournamentParticipant> {
    let scale = (age / 10).max(1) as i32;
    vec![
        DevTournamentParticipant {
            name: "YOU",
            class_id: "warrior",
            level: 10 + age / 10,
            hp: 78 + scale * 3,
            max_hp: 110 + scale * 5,
            mp: 22,
            max_mp: 34,
            is_player: true,
        },
        DevTournamentParticipant {
            name: "루나",
            class_id: "rogue",
            level: 9 + age / 10,
            hp: 54 + scale * 2,
            max_hp: 92 + scale * 4,
            mp: 28,
            max_mp: 42,
            is_player: false,
        },
        DevTournamentParticipant {
            name: "가론",
            class_id: "fighter",
            level: 9 + age / 10,
            hp: 88,
            max_hp: 116,
            mp: 12,
            max_mp: 24,
            is_player: false,
        },
        DevTournamentParticipant {
            name: "미라",
            class_id: "mage",
            level: 9 + age / 10,
            hp: 61,
            max_hp: 84,
            mp: 46,
            max_mp: 58,
            is_player: false,
        },
        DevTournamentParticipant {
            name: "세라",
            class_id: "healer",
            level: 8 + age / 10,
            hp: 70,
            max_hp: 96,
            mp: 44,
            max_mp: 56,
            is_player: false,
        },
        DevTournamentParticipant {
            name: "록스",
            class_id: "gangster",
            level: 10 + age / 10,
            hp: 77,
            max_hp: 104,
            mp: 18,
            max_mp: 30,
            is_player: false,
        },
        DevTournamentParticipant {
            name: "오르카",
            class_id: "robot",
            level: 10 + age / 10,
            hp: 96,
            max_hp: 124,
            mp: 20,
            max_mp: 32,
            is_player: false,
        },
        DevTournamentParticipant {
            name: "벨",
            class_id: "wanderer",
            level: 9 + age / 10,
            hp: 74,
            max_hp: 100,
            mp: 26,
            max_mp: 36,
            is_player: false,
        },
    ]
}

fn render_dev_tournament_gate(age: u64, inner: usize, lang: UiLanguage) -> Vec<String> {
    let border = format!("+{}+", "-".repeat(inner));
    let title = label(lang, "귀환전 개막", "EARTHBOUND TOURNAMENT");
    let mut lines = vec![
        border.clone(),
        panel_line_center(
            &colorize(&format!("AGE {age} · {title}"), AnsiColor::Yellow),
            inner,
        ),
        panel_line_raw("", inner),
        panel_line_center("╔══════════════════════════════════════╗", inner),
        panel_line_center("║        ORBITAL GATE OPENED          ║", inner),
        panel_line_center("║        8 CHALLENGERS ASSEMBLE       ║", inner),
        panel_line_center("╚══════════════════════════════════════╝", inner),
        panel_line_raw("", inner),
        panel_line_center(
            label(
                lang,
                "토너먼트가 시작됩니다. [진행] 으로 8강 대진표를 확인합니다.",
                "Tournament begins. [Proceed] opens the quarterfinal bracket.",
            ),
            inner,
        ),
        border,
    ];
    normalize_panel_height(&mut lines, inner, 16);
    lines
}

fn render_dev_tournament_bracket(
    age: u64,
    inner: usize,
    progress: usize,
    lang: UiLanguage,
) -> Vec<String> {
    let border = format!("+{}+", "-".repeat(inner));
    let mark = |index: usize, text: &str| {
        if progress == index {
            colorize(text, AnsiColor::Yellow)
        } else if progress > index {
            colorize(text, AnsiColor::Green)
        } else {
            text.to_string()
        }
    };
    let mut lines = vec![
        border.clone(),
        panel_line_raw(
            &header_with_right(
                &format!(
                    "AGE {age} {}",
                    label(lang, "8강 토너먼트", "QUARTERFINAL BRACKET")
                ),
                "8강 > 4강 > 결승",
                inner,
            ),
            inner,
        ),
        border.clone(),
        panel_line_raw(&mark(0, "[YOU 전사] ─┐        ┌─ [YOU] ─┐"), inner),
        panel_line_raw(&mark(0, "[루나 도적] ─┘        │         │"), inner),
        panel_line_raw(&mark(1, "[가론 투사] ─┐        └─ [?] ──┤"), inner),
        panel_line_raw(&mark(1, "[미라 로봇] ─┘                  │"), inner),
        panel_line_raw(&mark(2, "[세라 사제] ─┐        ┌─ [?] ──┘"), inner),
        panel_line_raw(&mark(2, "[록스 깡패] ─┘        │"), inner),
        panel_line_raw(&mark(3, "[오르카 로봇] ─┐      └─ [?]"), inner),
        panel_line_raw(&mark(3, "[벨 자산가] ───┘"), inner),
        border.clone(),
        panel_line_raw(
            label(
                lang,
                "진행: AI 경기 결과가 아래 메시지 라인에 순서대로 표시됩니다. [진행]",
                "Progress: AI results appear in order on the message line. [Proceed]",
            ),
            inner,
        ),
        panel_line_raw(
            match progress {
                0 => "8강 1경기 준비: YOU vs 루나",
                1 => "8강 2경기 결과: 가론 승리",
                2 => "8강 3경기 결과: 세라 승리",
                3 => "8강 4경기 결과: 오르카 승리",
                _ => "4강 진출: YOU · 가론 · 세라 · 오르카",
            },
            inner,
        ),
        border,
    ];
    normalize_panel_height(&mut lines, inner, 18);
    lines
}

fn render_dev_tournament_player_match(
    age: u64,
    inner: usize,
    left: &DevTournamentParticipant,
    right: &DevTournamentParticipant,
    lang: UiLanguage,
) -> Vec<String> {
    let border = format!("+{}+", "-".repeat(inner));
    let arena_width = inner.saturating_sub(2);
    let mut lines = vec![
        border.clone(),
        panel_line_raw(
            &header_with_right(
                &format!(
                    "AGE {age} · {}",
                    label(lang, "8강 1경기", "QUARTERFINAL MATCH 1")
                ),
                "ROUND 03 · PLAYER MATCH",
                inner,
            ),
            inner,
        ),
        panel_line_raw(
            &two_edge_columns(
                &combatant_title(left, lang, AnsiColor::Cyan),
                &combatant_title(right, lang, AnsiColor::Purple),
                arena_width,
            ),
            inner,
        ),
        panel_line_raw(
            &two_edge_columns(&combatant_bars(left), &combatant_bars(right), arena_width),
            inner,
        ),
        panel_line_raw("", inner),
    ];
    for row in render_dev_duel_sprite_rows(left.class_id, right.class_id, arena_width) {
        lines.push(panel_line_raw(&row, inner));
    }
    lines.push(panel_line_raw(
        &colorize(
            "──────────────────────── ARENA FLOOR / ORBITAL RING ────────────────────────",
            AnsiColor::Gray,
        ),
        inner,
    ));
    lines.push(border.clone());
    lines.push(panel_line_raw(
        label(
            lang,
            "루나가 그림자 찌르기를 시전했다. 당신에게 12 피해.",
            "Luna used Shadow Pierce. You took 12 damage.",
        ),
        inner,
    ));
    lines.push(panel_line_raw(
        label(
            lang,
            "당신의 반격이 적중했다. 루나에게 18 피해.",
            "Your counterattack hit. Luna took 18 damage.",
        ),
        inner,
    ));
    lines.push(border.clone());
    lines.push(panel_line_raw(
        label(lang, "[진행] 다음 턴", "[Proceed] Next turn"),
        inner,
    ));
    lines.push(border);
    normalize_panel_height(&mut lines, inner, 22);
    lines
}

fn render_dev_tournament_result(age: u64, inner: usize, lang: UiLanguage) -> Vec<String> {
    let border = format!("+{}+", "-".repeat(inner));
    let mut lines = vec![
        border.clone(),
        panel_line_center(
            &colorize(
                &format!(
                    "AGE {age} {}",
                    label(lang, "토너먼트 테스트 완료", "TOURNAMENT TEST COMPLETE")
                ),
                AnsiColor::Green,
            ),
            inner,
        ),
        panel_line_raw("", inner),
        panel_line_center("YOU ── 가론 ── 오르카", inner),
        panel_line_center("  ╲      ╲      ╲", inner),
        panel_line_center("   ╲      YOU     CHAMPION", inner),
        panel_line_raw("", inner),
        panel_line_raw(
            label(
                lang,
                "개발자 프리뷰입니다. 실제 보상/진행도/DB 상태는 변경하지 않았습니다.",
                "Developer preview only. Rewards, progress, and DB state were not changed.",
            ),
            inner,
        ),
        border,
    ];
    normalize_panel_height(&mut lines, inner, 16);
    lines
}

fn normalize_panel_height(lines: &mut Vec<String>, inner: usize, target: usize) {
    if lines.is_empty() {
        return;
    }
    let Some(border) = lines.pop() else {
        return;
    };
    while lines.len() + 1 < target {
        lines.push(panel_line_raw("", inner));
    }
    lines.push(border);
}

fn combatant_title(
    participant: &DevTournamentParticipant,
    lang: UiLanguage,
    color: AnsiColor,
) -> String {
    let owner = if participant.is_player {
        label(lang, "YOU", "YOU")
    } else {
        participant.name
    };
    colorize(
        &format!(
            "{owner} · {} Lv.{}",
            localized_class_name(participant.class_id, lang),
            participant.level
        ),
        color,
    )
}

fn localized_class_name(class_id: &str, lang: UiLanguage) -> &'static str {
    if lang == UiLanguage::Ko {
        ko_class_label(class_id)
    } else {
        vibemud_core::class_label(class_id)
    }
}

fn combatant_bars(participant: &DevTournamentParticipant) -> String {
    format!(
        "{} {}   {} {}",
        colorize("HP", AnsiColor::Gray),
        colored_hp_bar(participant.hp, participant.max_hp),
        colorize("MP", AnsiColor::Gray),
        colorize(
            &format!("{}/{}", participant.mp, participant.max_mp),
            AnsiColor::Blue,
        )
    )
}

fn colored_hp_bar(current: i32, max: i32) -> String {
    let color = if current * 4 <= max {
        AnsiColor::Red
    } else if current * 2 <= max {
        AnsiColor::Yellow
    } else {
        AnsiColor::Green
    };
    colorize(
        &format!(
            "{} {}",
            hp_bar(current, max, 14),
            hp_label("", current, max).trim()
        ),
        color,
    )
}

fn render_dev_duel_sprite_rows(left_class: &str, right_class: &str, width: usize) -> Vec<String> {
    let left = hero_sprite_for_class(left_class, false, 0);
    let right = hero_sprite_for_class(right_class, false, 1);
    let row_count = left.len().max(right.len()).max(8);
    let mut rows = vec![vec![' '; width]; row_count];
    let left_offset = row_count.saturating_sub(left.len());
    let right_offset = row_count.saturating_sub(right.len());
    let left_pos = 4;
    let right_width = right
        .iter()
        .map(|row| display_width(row))
        .max()
        .unwrap_or(0);
    let right_pos = width.saturating_sub(right_width + 4);
    for (idx, row) in left.iter().enumerate() {
        place_chars(&mut rows[left_offset + idx], left_pos, row);
    }
    for (idx, row) in right.iter().enumerate() {
        place_chars(&mut rows[right_offset + idx], right_pos, row);
    }
    rows.into_iter()
        .map(|row| row.into_iter().collect::<String>())
        .collect()
}

fn render_intro_screen<W: Write>(out: &mut W, art: &[&str], lines: &[&str]) -> Result<()> {
    write!(out, "\x1b[2J\x1b[H")?;
    for row in art {
        writeln!(out, "{row}")?;
    }
    writeln!(
        out,
        "╍────────────────────────────────────────────────────────────╍"
    )?;
    for line in lines {
        writeln!(out, "{line}")?;
    }
    out.flush()?;
    Ok(())
}

fn render_dialogue_screen<W: Write>(
    out: &mut W,
    art: &[&str],
    side: &str,
    speaker: &str,
    text: &str,
) -> Result<()> {
    write!(out, "\x1b[2J\x1b[H")?;
    for row in art {
        writeln!(out, "{row}")?;
    }
    writeln!(
        out,
        "╍────────────────────────────────────────────────────────────╍"
    )?;
    match side {
        "left" => {
            render_intro_text_box(
                out,
                &format!("{speaker} · archive visor ONLINE"),
                &[
                    "   /\\_/\\    planetary archive terminal".to_string(),
                    "  ( •_• )   relay channel: left podium".to_string(),
                    format!("\"{text}\""),
                ],
            )?;
        }
        "right" => {
            render_intro_text_box(
                out,
                &format!("{speaker} · helmet seal READY"),
                &[
                    "                      /\\_/\\   challenger uplink".to_string(),
                    "                     ( •̀_•́ )  relay channel: right podium".to_string(),
                    format!("\"{text}\""),
                ],
            )?;
        }
        _ => {
            render_intro_text_box(out, speaker, &[format!("▶ {text}")])?;
        }
    }
    out.flush()?;
    Ok(())
}

fn render_intro_text_box<W: Write>(out: &mut W, title: &str, lines: &[String]) -> Result<()> {
    const WIDTH: usize = 60;

    let title = clip_display(title, WIDTH);
    let title_width = display_width(&title);
    writeln!(
        out,
        "  ╭─{}{}╮",
        title,
        "─".repeat(WIDTH.saturating_sub(title_width) + 1)
    )?;
    for line in lines {
        writeln!(out, "  │ {} │", pad_chars(line, WIDTH))?;
    }
    writeln!(out, "  ╰{}╯", "─".repeat(WIDTH + 2))?;
    Ok(())
}

fn current_config_for_setup() -> vibemud_db::AppConfig {
    vibemud_db::AppPaths::discover()
        .ok()
        .and_then(|paths| std::fs::read_to_string(paths.config).ok())
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn setup_values_from_args(args: &SetupArgs, current: &vibemud_db::AppConfig) -> SetupValues {
    let language = args
        .language
        .clone()
        .unwrap_or_else(|| normalize_language(&current.ui.language));
    let agent = args.agent.clone().unwrap_or_else(|| {
        if current.integrations.claude_enabled {
            "claude".to_string()
        } else if current.integrations.codex_enabled {
            "codex".to_string()
        } else {
            "auto".to_string()
        }
    });
    let terminal = args
        .terminal
        .clone()
        .unwrap_or_else(|| normalize_terminal(&current.integrations.terminal));
    let storage = args.storage.clone().unwrap_or_else(detect_storage_scope);
    let storage_root = storage_root_for(&storage);
    let hud_mode = default_hud_for_terminal(&terminal);
    let popup_panes = default_popup_for(&terminal, &hud_mode);
    SetupValues {
        language,
        agent,
        terminal,
        storage,
        storage_root,
        hud_mode,
        popup_panes,
    }
}

fn normalize_language(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "en" | "english" => "en".to_string(),
        _ => "ko".to_string(),
    }
}

fn normalize_terminal(value: &str) -> String {
    match value.to_ascii_lowercase().as_str() {
        "tmux" | "ghostty" | "windows-terminal" | "plain" => value.to_ascii_lowercase(),
        _ => "auto".to_string(),
    }
}

fn detect_storage_scope() -> String {
    if std::env::var_os("VIBEMUD_HOME").is_none() && nearest_project_storage_marker().is_some() {
        "project".to_string()
    } else {
        "user".to_string()
    }
}

fn storage_root_for(storage: &str) -> Option<PathBuf> {
    (storage == "project")
        .then(|| std::env::current_dir().ok().map(|dir| dir.join(".vibemud")))
        .flatten()
}

fn nearest_project_storage_marker() -> Option<PathBuf> {
    let current = std::env::current_dir().ok()?;
    current
        .ancestors()
        .map(|dir| {
            dir.join(".vibemud")
                .join(vibemud_db::PROJECT_STORAGE_MARKER_FILE)
        })
        .find(|marker| marker.is_file())
}

fn default_hud_for_terminal(terminal: &str) -> String {
    if terminal == "plain" {
        "statusline".to_string()
    } else {
        "side".to_string()
    }
}

fn default_popup_for(terminal: &str, hud_mode: &str) -> bool {
    terminal != "plain" && hud_mode != "statusline"
}

fn apply_setup_storage(values: &SetupValues) -> Result<()> {
    match values.storage.as_str() {
        "project" => {
            let root = values
                .storage_root
                .clone()
                .context("failed to resolve project storage path")?;
            std::fs::create_dir_all(&root)?;
            let marker = root.join(vibemud_db::PROJECT_STORAGE_MARKER_FILE);
            if !marker.exists() {
                std::fs::write(&marker, "VibeMUD project storage\n")?;
            }
            std::env::set_var("VIBEMUD_HOME", &root);
        }
        _ => {
            std::env::remove_var("VIBEMUD_HOME");
            if let Some(marker) = nearest_project_storage_marker() {
                anyhow::bail!(
                    "project storage marker exists at {}; refusing to disable project storage implicitly. Remove the marker explicitly after backing up project data, or run setup outside that project tree.",
                    marker.display()
                );
            }
        }
    }
    Ok(())
}

fn apply_setup_values(values: &SetupValues) -> Result<()> {
    write_config_value("ui.language", &values.language)?;
    write_config_value("integrations.terminal", &values.terminal)?;
    write_config_value("ui.hud_mode", &values.hud_mode)?;
    write_config_value(
        "ui.popup_pane_enabled",
        if values.popup_panes { "true" } else { "false" },
    )?;
    write_config_value(
        "integrations.tmux_enabled",
        if values.terminal == "plain" || values.terminal == "windows-terminal" {
            "false"
        } else {
            "true"
        },
    )?;
    write_config_value(
        "integrations.claude_enabled",
        if values.agent == "claude" {
            "true"
        } else {
            "false"
        },
    )?;
    write_config_value(
        "integrations.codex_enabled",
        if values.agent == "codex" {
            "true"
        } else {
            "false"
        },
    )?;
    Ok(())
}

fn prompt_choice(prompt: &str, choices: &[(&str, &str)], default: &str) -> Result<String> {
    loop {
        println!(
            "
{prompt}"
        );
        for (idx, (value, label)) in choices.iter().enumerate() {
            let marker = if *value == default { "*" } else { " " };
            println!("  {}. [{marker}] {label} ({value})", idx + 1);
        }
        print!("Select [default: {default}]: ");
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim();
        if input.is_empty() {
            return Ok(default.to_string());
        }
        if let Ok(index) = input.parse::<usize>() {
            if let Some((value, _)) = choices.get(index.saturating_sub(1)) {
                return Ok((*value).to_string());
            }
        }
        if choices.iter().any(|(value, _)| *value == input) {
            return Ok(input.to_string());
        }
        println!("Invalid choice: {input}");
    }
}

fn print_setup_summary(values: &SetupValues) {
    let ko = values.language == "ko";
    if ko {
        println!(
            "
VibeMUD 설정 완료"
        );
        println!("  언어: {}", values.language);
        println!("  환경: {}", values.agent);
        println!("  터미널: {}", values.terminal);
        println!("  저장 위치: {}", storage_summary(values, true));
        println!("  HUD 기본값: {}", values.hud_mode);
        println!(
            "  선택창 기본값: {}",
            if values.popup_panes {
                "사용"
            } else {
                "끄기"
            }
        );
        println!(
            "
다시 바꾸려면: vibemud setup"
        );
    } else {
        println!(
            "
VibeMUD setup complete"
        );
        println!("  language: {}", values.language);
        println!("  agent: {}", values.agent);
        println!("  terminal: {}", values.terminal);
        println!("  storage: {}", storage_summary(values, false));
        println!("  HUD default: {}", values.hud_mode);
        println!(
            "  popup panes default: {}",
            if values.popup_panes { "on" } else { "off" }
        );
        println!(
            "
Run `vibemud setup` to change these choices later."
        );
    }
}

fn storage_summary(values: &SetupValues, ko: bool) -> String {
    match (values.storage.as_str(), values.storage_root.as_ref(), ko) {
        ("project", Some(root), true) => format!("project ({})", root.display()),
        ("project", Some(root), false) => format!("project ({})", root.display()),
        ("project", None, true) => "project (.vibemud)".to_string(),
        ("project", None, false) => "project (.vibemud)".to_string(),
        (_, _, true) => "user 기본값".to_string(),
        _ => "user default".to_string(),
    }
}

fn reset_game(yes: bool) -> Result<()> {
    if !yes {
        anyhow::bail!(
            "game reset deletes current progress; rerun with `vibemud reset --yes` to confirm"
        );
    }
    let _ = vibemud_runtime::stop_runtime();
    let _ = cleanup_hud_processes();
    let (_paths, mut conn) = vibemud_db::open_app()?;
    let snapshot = vibemud_db::reset_game_state(&mut conn)?;
    let lang = ui_language();
    println!(
        "{}",
        integration_hint(
            lang,
            "게임 리셋 완료: 다음 HUD 시작 시 오프닝을 다시 보여준 뒤 새 모험을 시작합니다. mudctl hunt start --area forest-edge --auto-start",
            "Game reset complete: the next HUD start will replay the intro before the new run. mudctl hunt start --area forest-edge --auto-start",
            "게임 리셋 완료: 다음 /vibemud:mud start 에서 오프닝을 다시 보여준 뒤 새 모험을 시작합니다.",
            "Game reset complete: the next /vibemud:mud start will replay the intro before the new run.",
        )
    );
    println!(
        "{} Lv.{} {} HP {}/{} Gold {}",
        label(lang, "초기 상태:", "Initial state:"),
        snapshot.player.level,
        snapshot.player.name,
        snapshot.player.hp,
        snapshot.player.max_hp,
        snapshot.player.gold
    );
    Ok(())
}

fn parse_toml_scalar(value: &str) -> toml::Value {
    if let Ok(v) = value.parse::<bool>() {
        toml::Value::Boolean(v)
    } else if let Ok(v) = value.parse::<i64>() {
        toml::Value::Integer(v)
    } else {
        toml::Value::String(value.to_string())
    }
}

fn set_toml_path(root: &mut toml::Value, key: &str, value: toml::Value) {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = root;
    for part in &parts[..parts.len().saturating_sub(1)] {
        if !current.is_table() {
            *current = toml::Value::Table(toml::map::Map::new());
        }
        let table = current.as_table_mut().expect("table just created");
        current = table
            .entry((*part).to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    }
    if let Some(last) = parts.last() {
        if !current.is_table() {
            *current = toml::Value::Table(toml::map::Map::new());
        }
        current
            .as_table_mut()
            .expect("table just created")
            .insert((*last).to_string(), value);
    }
}

fn print_doctor() -> Result<()> {
    let paths = vibemud_db::init_app()?;
    let redacted = redact_path(&paths.root.display().to_string());
    let (_paths, conn) = vibemud_db::open_app()?;
    println!("VibeMUD doctor");
    println!("home: {redacted}");
    println!("db: ok");
    println!(
        "journal_mode: {}",
        vibemud_db::pragma_value(&conn, "journal_mode")?
    );
    println!(
        "vibe_fever: {}",
        if vibemud_db::vibe_fever_active() {
            "active"
        } else {
            "idle"
        }
    );
    println!("privacy: code/prompt/file reading disabled");
    Ok(())
}

fn redact_path(value: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if value.starts_with(&home) {
            return value.replace(&home, "~");
        }
    }
    if std::env::var("VIBEMUD_HOME").is_ok() {
        return "<vibemud-home>".to_string();
    }
    value.to_string()
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
    #[test]
    fn hud_command_detection_catches_native_and_node_wrappers_only() {
        assert!(hud_command_is_safe_to_stop(
            "/path/to/vibemud hud --panel --refresh 1"
        ));
        assert!(hud_command_is_safe_to_stop(
            "node /prefix/bin/vibemud.js hud --panel --refresh 1"
        ));
        assert!(hud_command_is_safe_to_stop(
            "/path/to/vibemud-hud --panel --refresh 1"
        ));
        assert!(!hud_command_is_safe_to_stop("vibemud session stop"));
        assert!(!hud_command_is_safe_to_stop("rg vibemud"));
        assert!(!hud_command_is_safe_to_stop(
            "bash -c sleep 30 # vibemud hud marker"
        ));
    }

    #[test]
    fn hud_process_guard_registers_and_removes_pid_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let pid_file = hud_pid_dir(root).join(format!("{}.pid", std::process::id()));

        {
            let _guard = HudProcessGuard::register(root).unwrap();
            assert_eq!(
                std::fs::read_to_string(&pid_file).unwrap().trim(),
                std::process::id().to_string()
            );
        }

        assert!(!pid_file.exists());
    }

    #[test]
    fn cleanup_hud_processes_removes_stale_registry_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = hud_pid_dir(tmp.path());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dead.pid"), "999999999\n").unwrap();
        std::fs::write(dir.join("invalid.pid"), "not-a-pid\n").unwrap();

        cleanup_hud_processes_in_root(tmp.path()).unwrap();

        assert!(!dir.join("dead.pid").exists());
        assert!(!dir.join("invalid.pid").exists());
    }

    #[test]
    fn intro_art_frames_have_enough_detail() {
        assert!(PLANET64_INTRO_FRAMES.len() >= 4);
        assert!(PLANET64_INTRO_FRAMES.iter().all(|frame| frame.len() >= 10));
        let final_frame = PLANET64_INTRO_FRAMES[3].join("\n");
        assert!(final_frame.contains("ORBITAL QUALIFIER GATE"));
        assert!(final_frame.contains("ATTEMPTS: 01 / 10"));
    }

    #[test]
    fn intro_dialogue_stays_short_enough_for_one_minute_opening() {
        assert!(INTRO_NARRATION_KO.len() <= 6);
        assert!(INTRO_DIALOGUE_KO.len() <= 6);
        assert_eq!(INTRO_NARRATION_KO.len(), INTRO_NARRATION_EN.len());
        assert_eq!(INTRO_DIALOGUE_KO.len(), INTRO_DIALOGUE_EN.len());
    }

    #[test]
    fn intro_if_needed_flag_parses_for_hud_launchers() {
        let cli = VibeMudCli::try_parse_from(["vibemud", "intro", "--if-needed"]).unwrap();
        match cli.command {
            VibeMudCommand::Intro(args) => {
                assert!(args.if_needed);
                assert!(!args.replay);
            }
            other => panic!("expected intro command, got {other:?}"),
        }
    }

    #[test]
    fn setup_plain_terminal_defaults_to_statusline_without_popups() {
        let current = vibemud_db::AppConfig::default();
        let args = SetupArgs {
            language: Some("en".to_string()),
            agent: Some("codex".to_string()),
            terminal: Some("plain".to_string()),
            storage: Some("project".to_string()),
            yes: true,
        };

        let values = setup_values_from_args(&args, &current);

        assert_eq!(values.language, "en");
        assert_eq!(values.agent, "codex");
        assert_eq!(values.terminal, "plain");
        assert_eq!(values.storage, "project");
        assert_eq!(values.hud_mode, "statusline");
        assert!(!values.popup_panes);
    }

    #[test]
    fn setup_keeps_side_hud_and_popups_for_tmux() {
        let current = vibemud_db::AppConfig::default();
        let args = SetupArgs {
            terminal: Some("tmux".to_string()),
            storage: Some("user".to_string()),
            yes: true,
            ..SetupArgs::default()
        };

        let values = setup_values_from_args(&args, &current);

        assert_eq!(values.language, "ko");
        assert_eq!(values.terminal, "tmux");
        assert_eq!(values.storage, "user");
        assert_eq!(values.hud_mode, "side");
        assert!(values.popup_panes);
    }

    #[test]
    fn setup_user_storage_does_not_delete_project_marker() {
        let _guard = test_lock();
        let old_dir = std::env::current_dir().unwrap();
        let old_home = std::env::var_os("VIBEMUD_HOME");
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        let nested = project.join("nested");
        let project_home = project.join(".vibemud");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&project_home).unwrap();
        let marker = project_home.join(vibemud_db::PROJECT_STORAGE_MARKER_FILE);
        std::fs::write(&marker, "VibeMUD project storage\n").unwrap();
        std::env::set_current_dir(&nested).unwrap();
        std::env::set_var("VIBEMUD_HOME", &project_home);

        let values = SetupValues {
            language: "ko".to_string(),
            agent: "auto".to_string(),
            terminal: "plain".to_string(),
            storage: "user".to_string(),
            storage_root: None,
            hud_mode: "statusline".to_string(),
            popup_panes: false,
        };

        let err = apply_setup_storage(&values).unwrap_err();

        assert!(marker.exists(), "user setup must not remove project marker");
        assert!(
            err.to_string()
                .contains("refusing to disable project storage implicitly"),
            "unexpected error: {err}"
        );
        assert!(std::env::var_os("VIBEMUD_HOME").is_none());
        std::env::set_current_dir(old_dir).unwrap();
        if let Some(value) = old_home {
            std::env::set_var("VIBEMUD_HOME", value);
        }
    }

    #[test]
    fn normalizes_korean_argv() {
        let raw = vec![
            "mudctl".to_string(),
            "던전".to_string(),
            "입장".to_string(),
            "고블린".to_string(),
            "소굴".to_string(),
        ];
        assert_eq!(
            normalize_mudctl_argv(raw),
            vec!["mudctl", "dungeon", "enter", "goblin-den"]
        );
    }

    #[test]
    fn normalizes_korean_stats_alias() {
        let raw = vec!["mudctl".to_string(), "스탯".to_string(), "닫기".to_string()];
        assert_eq!(normalize_mudctl_argv(raw), vec!["mudctl", "stats", "close"]);
    }

    #[test]
    fn normalizes_top_level_shortcuts() {
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "c".into()]),
            vec!["mudctl", "stats", "open"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "x".into()]),
            vec!["mudctl", "stats", "close"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "s".into()]),
            vec!["mudctl", "hunt", "stop"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "m".into()]),
            vec!["mudctl", "map"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "a".into()]),
            vec!["mudctl", "hunt", "start"]
        );
    }

    #[test]
    fn plugin_cmux_panel_avoids_destructive_respawn_and_empty_launcher() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", script_path.display()));

        assert!(
            script.contains("cmux_current_surface()"),
            "cmux launcher must know the current surface before sending commands"
        );
        assert!(
            script.contains("Refusing to run VibeMUD HUD command on the current cmux surface"),
            "cmux launcher must refuse to target the user's active pane"
        );
        assert!(
            script.contains(r#"cmux_send_shell_command "$cli" "$surface" "$command""#),
            "cmux launcher should send into the fresh pane instead of respawning surfaces"
        );
        assert!(
            !script.contains("respawn-pane --surface"),
            "cmux launcher must not use respawn-pane because stale refs can blank the coding pane"
        );
        assert!(
            !script.contains(r#"shell_quote "$(resolve_binary vibemud)""#),
            "binary resolution failures must not become an empty exec target"
        );
        assert!(
            script.contains("close_cmux_surface_if_vibemud()"),
            "cmux cleanup must verify VibeMUD content before closing recorded surfaces"
        );
        let close_start = script
            .find("close_cmux_panel()")
            .expect("cmux close function exists");
        let close_tail = &script[close_start..];
        let close_end = close_tail
            .find("\npane_exists()")
            .expect("cmux close function has pane_exists boundary");
        let close_function = &close_tail[..close_end];
        assert!(
            close_function.contains("close_cmux_surface_if_vibemud"),
            "cmux stop must route through verified VibeMUD surface cleanup"
        );
        assert!(
            !close_function.contains("close-surface --surface"),
            "cmux stop must not blindly close stale recorded surfaces"
        );
        assert!(
            script.contains("kill_tmux_pane_if_vibemud()"),
            "tmux cleanup must verify VibeMUD content before killing recorded panes"
        );
        assert!(
            !script.contains(r#"tmux kill-pane -t "$existing""#),
            "tmux selectors must not blindly kill stale recorded panes"
        );
        assert!(
            script.contains("intro --if-needed || true"),
            "HUD panel launchers must play the one-time intro after a fresh install or reset before entering the live HUD"
        );
    }

    #[test]
    fn plugin_cmux_selectors_reuse_existing_hud_surface_before_creating_new_pane() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", script_path.display()));
        let start = script
            .find("open_cmux_selector_surface()")
            .expect("shared cmux selector function exists");
        let tail = &script[start..];
        let end = tail
            .find("\nclose_cmux_panel()")
            .expect("shared cmux selector function has close_cmux_panel boundary");
        let function = &tail[..end];

        assert!(
            function.contains(r#"cmux_surface_has_hud "$cli" "$existing""#)
                && function
                    .contains(r#"cmux_send_shell_command "$cli" "$existing" "$selector_cmd""#),
            "cmux selectors should run inside the existing VibeMUD HUD surface when available"
        );
        assert!(
            function.contains(r#""$cli" --id-format both new-pane --direction right"#)
                && function.contains("extract_cmux_surface_ref"),
            "cmux selectors should still create and parse a new surface when no HUD surface exists"
        );
        assert!(
            function.contains(r#"focus_cmux_surface "$cli" "$existing""#)
                && function.contains(r#"focus_cmux_surface "$cli" "$surface""#),
            "cmux selectors must focus either the reused HUD surface or the fallback fresh surface"
        );
        assert!(
            function.contains("Do not close it"),
            "stale cmux state pointing at a non-VibeMUD surface must not be closed"
        );
        assert!(
            script.contains("printf 'true; bash %s'"),
            "cmux launchers must run as children of the interactive shell so HUD panes can be reused for menus"
        );
    }

    #[test]
    fn plugin_tmux_selectors_reuse_recorded_hud_pane() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", script_path.display()));
        let start = script
            .find("open_tmux_selector_pane()")
            .expect("shared tmux selector function exists");
        let tail = &script[start..];
        let end = tail
            .find("\nfocus_tmux_pane()")
            .expect("shared tmux selector function has focus_tmux_pane boundary");
        let function = &tail[..end];

        assert!(
            function.contains(r#"pane_exists "$existing" && tmux_pane_has_vibemud "$existing""#),
            "tmux selectors must verify the recorded pane is a VibeMUD HUD before reusing it"
        );
        assert!(
            function.contains(r#"tmux respawn-pane -k -t "$pane_id" bash -lc "$selector_cmd""#),
            "tmux selectors should replace the HUD process in the existing pane instead of splitting another pane"
        );
        assert!(
            function.contains("tmux split-window"),
            "tmux selectors should still create a fallback pane when no reusable HUD pane exists"
        );
    }

    #[test]
    fn plugin_dispatcher_supports_ghostty_side_panel() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", script_path.display()));

        assert!(
            script.contains("is_ghostty_session()"),
            "dispatcher must detect Ghostty sessions"
        );
        assert!(
            script.contains(r#""${TERM_PROGRAM:-}" == "ghostty""#)
                && script.contains(r#""${TERM:-}" == "xterm-ghostty""#)
                && script.contains("GHOSTTY_RESOURCES_DIR"),
            "Ghostty detection should cover TERM_PROGRAM, TERM, and Ghostty resources env"
        );
        assert!(
            script.contains("open_ghostty_panel()"),
            "dispatcher must expose a Ghostty panel opener"
        );
        assert!(
            script.contains("set newTerm to split oldTerm direction right"),
            "Ghostty panel opener must create a right-side split"
        );
        assert!(
            script.contains("input text $quoted_launcher to newTerm"),
            "Ghostty panel opener must launch the HUD inside the new split"
        );
        assert!(
            script.contains(r#"send key "enter" to newTerm"#),
            "Ghostty panel opener must press Enter after pasting the launcher command"
        );
        assert!(
            script.contains(r#"shell_command="$(shell_quote "$launcher")""#),
            "Ghostty HUD launcher must run as a child of the interactive shell so Ctrl-C does not close the split"
        );
        assert!(
            script.contains("ghostty_terminal_has_hud()"),
            "Ghostty panel opener must distinguish a running HUD from a stale recorded split"
        );
        assert!(
            script.contains(r#"close_ghostty_panel"#),
            "Ghostty panel opener must close stale recorded Ghostty splits before reopening"
        );
        assert!(
            script.contains("open_ghostty_panel && return 0"),
            "open_best_panel must fall back to Ghostty after cmux/tmux"
        );
    }

    #[test]
    fn plugin_dispatcher_supports_ghostty_selectors_on_hud_split() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", script_path.display()));

        assert!(
            script.contains("open_ghostty_selector_pane()"),
            "dispatcher must expose a Ghostty selector opener"
        );
        assert!(
            script.contains("ghostty_send_shell_command()"),
            "Ghostty selectors must be launched inside the existing HUD split"
        );
        assert!(
            script.contains(r#"selector_cmd="$(shell_quote "$launcher")""#),
            "Ghostty selectors must run as children of the reusable split shell instead of exec-replacing it"
        );
        assert!(
            script.contains("schedule_ghostty_focus \"$panel_terminal\""),
            "Ghostty selectors must move focus to the HUD/selector split after launch"
        );
        assert!(
            script.contains("export VIBEMUD_RETURN_GHOSTTY_TERMINAL="),
            "Ghostty selector launchers must remember the original Claude terminal"
        );
        assert!(
            script.contains("VIBEMUD_RETURN_GHOSTTY_TERMINAL")
                && script.contains("schedule_ghostty_focus"),
            "selector close must return focus to the original Ghostty terminal"
        );
        for (selector, title) in [
            ("map", "VibeMUD Map"),
            ("stats", "VibeMUD Items"),
            ("settings", "VibeMUD Settings"),
            ("quest", "VibeMUD Quests"),
        ] {
            assert!(
                script.contains(&format!(
                    r#"open_ghostty_selector_pane {selector} "{title}""#
                )),
                "run_*_menu must fall back to the Ghostty {selector} selector"
            );
        }
    }

    #[test]
    fn plugin_tmux_panel_requires_tmux_context_before_ghostty_fallback() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", script_path.display()));

        assert!(
            script.contains("tmux_context_available()"),
            "dispatcher must distinguish a real tmux client from an unrelated tmux server"
        );
        assert!(
            script.contains(r#"[[ -n "${TMUX:-}${TMUX_PANE:-}${VIBEMUD_TMUX_PANE:-}" ]]"#),
            "tmux context detection must be based on tmux client environment"
        );

        let open_tmux = script
            .split("open_tmux_panel()")
            .nth(1)
            .and_then(|tail| tail.split("close_tmux_panel()").next())
            .expect("open_tmux_panel function exists");
        assert!(
            open_tmux.contains("tmux_context_available || return 1"),
            "open_tmux_panel must not create panes in a detached/unrelated tmux server"
        );

        let current_tmux_pane = script
            .split("current_tmux_pane()")
            .nth(1)
            .and_then(|tail| tail.split("current_tmux_client()").next())
            .expect("current_tmux_pane function exists");
        assert!(
            current_tmux_pane.contains("tmux_context_available || return 1"),
            "current_tmux_pane must not infer a pane when Claude is not inside tmux"
        );
    }

    #[test]
    fn plugin_stop_closes_ghostty_panel_state() {
        let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh");
        let script = std::fs::read_to_string(&script_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", script_path.display()));
        let stop_case = script
            .split("s|stop|end|정지|종료|pause|중지)")
            .nth(1)
            .and_then(|tail| tail.split("set|settings|setting|설정|환경설정)").next())
            .expect("stop case exists");

        assert!(
            script.contains("close_ghostty_panel()"),
            "dispatcher must define a Ghostty panel closer"
        );
        assert!(
            stop_case.contains("close_ghostty_panel"),
            "stop/end must close the recorded Ghostty HUD split"
        );
    }

    #[test]
    fn session_claude_uses_windows_terminal_power_shell_layout_on_windows() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"),
        )
        .expect("lib source is readable");

        assert!(
            source.contains("fn start_agent_layout(cli: &str)"),
            "agent session launcher must choose a platform-specific pane backend"
        );
        assert!(
            source.contains("cfg!(windows)")
                && source.contains("VIBEMUD_FORCE_WINDOWS_TERMINAL_LAYOUT"),
            "Windows builds and tests must be able to route session layouts to Windows Terminal"
        );
        assert!(
            source.contains("fn start_windows_terminal_layout(cli: &str)")
                && source.contains("Windows Terminal (wt.exe)"),
            "Windows pane layout must have a native wt.exe implementation"
        );
        assert!(
            source.contains("\"split-pane\"")
                && source.contains("\"-H\"")
                && source.contains("\"VibeMUD HUD\""),
            "Windows Terminal layout must create a right-side HUD pane"
        );
        assert!(
            source.contains("\"powershell.exe\"")
                && source.contains("\"pwsh.exe\"")
                && source.contains("powershell_command_invocation"),
            "Windows layout must run panes through PowerShell-compatible command invocations"
        );
        assert!(
            source.contains("fn inside_windows_terminal()"),
            "Windows layout must not trust inherited WT_SESSION (VS Code/Cursor terminals) when targeting -w 0"
        );
        assert!(
            source.contains("DETACHED_PROCESS") && source.contains("CREATE_NEW_PROCESS_GROUP"),
            "background runtime must detach from the parent console on Windows so closing the hook/terminal console or Ctrl+C cannot kill it"
        );
        assert!(
            source.contains("ENABLE_VIRTUAL_TERMINAL_PROCESSING"),
            "CLI entry points must enable VT processing so ANSI HUD output renders on legacy conhost"
        );
    }

    #[test]
    fn plugin_windows_dispatcher_covers_hook_command_surface() {
        let plugin_scripts = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud/scripts");
        let dispatcher = std::fs::read_to_string(plugin_scripts.join("vibemud-claude.ps1"))
            .expect("Windows dispatcher vibemud-claude.ps1 must exist next to vibemud-claude.sh");

        for token in [
            "'start'",
            "'end'",
            "'help'",
            "'set'",
            "'panel'",
            "'reset'",
            "'inventory'",
            "'quest'",
        ] {
            assert!(
                dispatcher.contains(token),
                "Windows dispatcher must keep the {token} command surface"
            );
        }
        for ack in [
            "사냥 시작 · HUD 열림",
            "정지 완료",
            "장비창: 열림",
            "지도: HUD · 선택은 /mud a <지역/던전>",
            "게임 리셋 완료 · /mud start 시 오프닝 후 HUD",
            "차단: watch (panel 권장)",
        ] {
            assert!(
                dispatcher.contains(ack),
                "Windows dispatcher acknowledgements must stay aligned with vibemud-claude.sh: {ack}"
            );
        }
        assert!(
            dispatcher.contains("split-pane") && dispatcher.contains("VibeMUD HUD"),
            "Windows dispatcher must open the HUD as a Windows Terminal split"
        );
        assert!(
            dispatcher.contains("Test-HudProcess"),
            "Windows dispatcher must reuse a verified existing HUD before creating panes"
        );
        assert!(
            dispatcher.contains("stats-select") && dispatcher.contains("map-select"),
            "Windows selectors must reuse the shared bash selector entrypoints via Git Bash when available"
        );
        assert!(
            dispatcher.contains("Test-InsideWindowsTerminal"),
            "Windows dispatcher must not trust inherited WT_SESSION (VS Code/Cursor terminals) when targeting -w 0"
        );
        assert!(
            dispatcher.contains("'-ExecutionPolicy', 'Bypass'"),
            "HUD pane shell must bypass execution policy so npm .ps1 shims can run"
        );
        assert!(
            dispatcher.contains("\\Windows\\System32\\"),
            "Git Bash resolution must exclude the WSL bash.exe from System32"
        );
        assert!(
            dispatcher.contains("ConvertTo-CommandLineArgument"),
            "wt.exe launches must quote arguments (Start-Process joins arrays without quoting)"
        );
        assert!(
            dispatcher.contains("Close-SelectorPanes"),
            "Windows dispatcher must close verified selector panes on switch/stop/reset instead of stacking wt splits"
        );
    }

    #[test]
    fn plugin_context_hook_routes_dispatchers_per_platform() {
        let plugin_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../claude-marketplace/plugins/vibemud");
        let hook = std::fs::read_to_string(plugin_root.join("scripts/vibemud-context-hook.js"))
            .expect("Node context hook must exist for cross-platform hook execution");
        let hooks_json = std::fs::read_to_string(plugin_root.join("hooks/hooks.json"))
            .expect("hooks.json is readable");

        assert!(
            hooks_json.contains("node \\\"${CLAUDE_PLUGIN_ROOT}/scripts/vibemud-context-hook.js\\\"")
                || hooks_json
                    .contains("node \"${CLAUDE_PLUGIN_ROOT}/scripts/vibemud-context-hook.js\""),
            "hooks.json must invoke the Node hook with an explicit interpreter (shebang-only .py does not run on native Windows)"
        );
        assert!(
            hook.contains("process.platform === 'win32'")
                && hook.contains("vibemud-claude.ps1")
                && hook.contains("vibemud-claude.sh"),
            "context hook must route to the PowerShell dispatcher on Windows and bash dispatcher on POSIX"
        );
        assert!(
            hook.contains("VIBEMUD_CONTEXT_MODE") && hook.contains("'hook'"),
            "context hook must force hook context mode for dispatcher acknowledgements"
        );
        assert!(
            hook.contains("decision: 'block'"),
            "context hook must block prompt expansion so game output stays out of the coding context"
        );
        assert!(
            hook.contains("delete env.NODE_OPTIONS"),
            "context hook must strip editor debugger bootloaders (VS Code/Cursor JS debug terminals) from dispatcher children"
        );
    }

    #[test]
    fn normalizes_korean_top_level_shortcuts() {
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "캐릭터".into()]),
            vec!["mudctl", "stats"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "닫기".into()]),
            vec!["mudctl", "stats", "close"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "정지".into()]),
            vec!["mudctl", "hunt", "stop"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "지도".into()]),
            vec!["mudctl", "map"]
        );
        assert_eq!(
            normalize_mudctl_argv(vec!["mudctl".into(), "사냥".into()]),
            vec!["mudctl", "hunt", "start"]
        );
    }

    #[test]
    fn equipment_sell_common_accepts_optional_rarity_threshold() {
        let parsed =
            MudCtlCli::try_parse_from(["mudctl", "equipment", "sell-common", "epic"]).unwrap();
        let MudCommand::Equipment {
            command: EquipmentCommand::SellCommon { rarity },
        } = parsed.command
        else {
            panic!("expected equipment sell-common command");
        };
        assert_eq!(rarity.as_deref(), Some("epic"));

        let parsed = MudCtlCli::try_parse_from(["mudctl", "equipment", "empty"]).unwrap();
        assert!(matches!(
            parsed.command,
            MudCommand::Equipment {
                command: EquipmentCommand::Empty
            }
        ));

        let parsed = MudCtlCli::try_parse_from(["mudctl", "equipment", "lock", "item-1"]).unwrap();
        assert!(matches!(
            parsed.command,
            MudCommand::Equipment {
                command: EquipmentCommand::Lock { item_id }
            } if item_id == "item-1"
        ));

        let parsed =
            MudCtlCli::try_parse_from(["mudctl", "equipment", "unlock", "item-1"]).unwrap();
        assert!(matches!(
            parsed.command,
            MudCommand::Equipment {
                command: EquipmentCommand::Unlock { item_id }
            } if item_id == "item-1"
        ));
    }

    #[test]
    fn agent_layout_guard_blocks_nested_codex_or_claude_sessions() {
        let _guard = test_lock();
        std::env::set_var("OMX_SESSION_ID", "omx-test-session");
        std::env::remove_var("VIBEMUD_ALLOW_NESTED_AGENT_LAYOUT");

        assert!(agent_layout_guard_reason("codex").is_some());
        assert!(agent_layout_guard_reason("claude").is_some());
        assert!(agent_layout_guard_reason("zsh").is_none());

        std::env::set_var("VIBEMUD_ALLOW_NESTED_AGENT_LAYOUT", "1");
        assert!(agent_layout_guard_reason("codex").is_none());

        std::env::remove_var("OMX_SESSION_ID");
        std::env::remove_var("VIBEMUD_ALLOW_NESTED_AGENT_LAYOUT");
    }

    #[test]
    fn normal_header_uses_title_row_with_age() {
        let _guard = test_lock();
        std::env::set_var("NO_COLOR", "1");
        let snapshot = vibemud_core::GameSnapshot::initial();
        let dto = localized_status_dto(&snapshot, UiLanguage::Ko);
        let inner = 48;
        let border = format!("+{}+", "-".repeat(inner));
        let header = render_normal_header(&snapshot, &dto, inner, &border, UiLanguage::Ko);
        let joined = header.join("\n");
        assert!(joined.contains("VibeMUD 상태"));
        assert!(joined.contains("나이 0세 0일 00시"));
        assert!(joined.contains("캐릭터"));
        assert!(joined.contains("위치"));
        assert!(joined.contains("HP"));
        assert!(joined.contains("MP"));
        assert!(joined.contains("경험치"));
        assert!(joined.contains("공/방/속"));
        assert!(joined.contains("│"));
        assert!(header[1].starts_with("| VibeMUD 상태"));
        assert!(header[2].starts_with("| 캐릭터"));
        assert!(!header[2].starts_with("|   캐릭터"));
        assert_eq!(display_width(&header[0]), inner + 2);
        assert!(header[1..]
            .iter()
            .all(|line| display_width(line) == inner + 2));
        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn normal_header_age_uses_runtime_clock_tick_not_event_state_version() {
        let _guard = test_lock();
        std::env::set_var("NO_COLOR", "1");
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.state_version = 20 * 24 * 3;
        snapshot.clock_tick = 20;
        let dto = localized_status_dto(&snapshot, UiLanguage::Ko);
        let inner = 48;
        let border = format!("+{}+", "-".repeat(inner));
        let header = render_normal_header(&snapshot, &dto, inner, &border, UiLanguage::Ko);
        let joined = header.join("\n");

        assert!(joined.contains("나이 0세 0일 01시"));
        assert!(!joined.contains("나이 0세 3일 00시"));
        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn korean_status_location_uses_dungeon_label_in_dungeon_mode() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "dungeon".to_string();
        snapshot.player.current_dungeon_id = Some("crystal-cave".to_string());
        snapshot.player.current_area_id = Some("old-mine".to_string());

        let dto = localized_status_dto(&snapshot, UiLanguage::Ko);

        assert_eq!(dto.area_label, "수정 동굴");
        assert_eq!(dto.danger_label, "던전");
    }

    #[test]
    fn character_age_advances_one_hour_per_twenty_ticks() {
        assert_eq!(character_age_parts(0), (0, 0, 0));
        assert_eq!(character_age_parts(19), (0, 0, 0));
        assert_eq!(character_age_parts(20), (0, 0, 1));
        assert_eq!(character_age_parts(20 * 24), (0, 1, 0));
        assert_eq!(character_age_parts(20 * 24 * 360), (1, 0, 0));
    }

    #[test]
    fn hidden_dev_tournament_player_match_keeps_fixed_width() {
        let _guard = test_lock();
        std::env::set_var("NO_COLOR", "1");
        let participants = dev_tournament_participants(20);
        let lines = render_dev_tournament_player_match(
            20,
            94,
            &participants[0],
            &participants[1],
            UiLanguage::Ko,
        );

        assert!(lines.join("\n").contains("PLAYER MATCH"));
        assert!(lines.join("\n").contains("[진행] 다음 턴"));
        assert!(lines.iter().all(|line| display_width(line) == 96));
        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn normal_header_dims_labels_when_color_is_enabled() {
        let _guard = test_lock();
        std::env::remove_var("NO_COLOR");
        let snapshot = vibemud_core::GameSnapshot::initial();
        let dto = localized_status_dto(&snapshot, UiLanguage::Ko);
        let inner = 48;
        let border = format!("+{}+", "-".repeat(inner));
        let header = render_normal_header(&snapshot, &dto, inner, &border, UiLanguage::Ko);
        let joined = header.join("\n");
        assert!(joined.contains("\x1b[90m캐릭터\x1b[0m"));
        assert!(joined.contains("\x1b[90m │ \x1b[0m"));
        assert!(header.iter().all(|line| display_width(line) == inner + 2));
    }

    #[test]
    fn equipment_rows_use_single_column_with_enhancement_levels() {
        let _guard = test_lock();
        std::env::set_var("NO_COLOR", "1");
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.inventory.push(vibemud_core::InventoryItem {
            id: "item-1".to_string(),
            item_id: "basic-sword".to_string(),
            item_type: "equipment".to_string(),
            name: "Ares Sword".to_string(),
            rarity: "common".to_string(),
            rarity_color: Some("white".to_string()),
            tier: Some(1),
            quantity: 1,
            durability: None,
            max_durability: None,
            equipped_slot: Some("weapon".to_string()),
            locked: false,
            enhancement_level: 3,
            stat1_type: Some("attack".to_string()),
            stat1_value: Some(7),
            stat2_type: Some("speed".to_string()),
            stat2_value: Some(2),
            stat3_type: None,
            stat3_value: None,
            power_score: Some(19),
        });

        let rows = equipment_rows(&snapshot, 56, UiLanguage::Ko);
        assert_eq!(rows.len(), 8);
        assert!(rows[0].contains("무기"));
        assert!(rows[0].contains("아레스 검 +3 T1"));
        assert!(!rows[0].contains("전투력"));
        assert!(rows[1].contains("부무기"));
        assert!(rows[1].contains("미착용"));
        assert!(rows[2].contains("상의"));
        assert!(rows[3].contains("하의"));
        assert!(rows[5].contains("신발"));
        assert!(rows[6].contains("펫"));
        assert!(!rows[0].contains("상의"));
        assert!(rows.iter().all(|row| display_width(row) <= 56));
        std::env::remove_var("NO_COLOR");
    }

    #[test]
    fn panel_top_section_can_switch_to_detailed_stats() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        set_stats_view(&conn, Some(StatsCommand::Open)).unwrap();
        let panel = render_panel_dashboard(
            &conn,
            &HudArgs {
                side: false,
                statusline: false,
                full: false,
                once: true,
                refresh: 1,
                width: Some(90),
                ascii: false,
                live: false,
                panel: true,
                log_lines: 8,
            },
        )
        .unwrap();
        assert!(panel.contains("캐릭터 상세 능력치") || panel.contains("CHARACTER DETAILS"));
        assert!(panel.contains("레벨: Lv.") || panel.contains("Level: Lv."));
        assert!(panel.contains("장착 장비") || panel.contains("EQUIPMENT"));
        assert!(panel.contains("스탯") || panel.contains("STATS"));
        assert!(panel.contains("미착용") || panel.contains("Unequipped"));
        assert!(!panel.contains("액션 피드백"));
        assert!(!panel.contains("ACTION FEEDBACK"));
        assert!(!panel.contains("상태 바"));
        assert!(panel.contains("mudctl stats close"));
        assert!(!panel.contains("AUTO-HUNT SCENE"));
        assert!(!panel.contains("GAME / AUTO-HUNT LOG"));
        set_stats_view(&conn, Some(StatsCommand::Close)).unwrap();
        let panel = render_panel_dashboard(
            &conn,
            &HudArgs {
                side: false,
                statusline: false,
                full: false,
                once: true,
                refresh: 1,
                width: Some(90),
                ascii: false,
                live: false,
                panel: true,
                log_lines: 8,
            },
        )
        .unwrap();
        assert!(!panel.contains("mudctl stats close"));
        assert!(panel.contains("AUTO-HUNT SCENE") || panel.contains("자동 사냥"));
    }

    #[test]
    fn panel_can_switch_to_map_dashboard() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        set_map_view(&conn).unwrap();
        let panel = render_panel_dashboard(
            &conn,
            &HudArgs {
                side: false,
                statusline: false,
                full: false,
                once: true,
                refresh: 1,
                width: Some(110),
                ascii: false,
                live: false,
                panel: true,
                log_lines: 8,
            },
        )
        .unwrap();
        assert!(panel.contains("지역 / 던전 지도") || panel.contains("AREA / DUNGEON MAP"));
        assert!(panel.contains("[숲길]") || panel.contains("[Forest]"));
        assert!(panel.contains("[고블굴]") || panel.contains("[GobDen]"));
        assert!(panel.contains("임/늑") || panel.contains("Imp/Wolf"));
        assert!(panel.contains("액션 피드백") || panel.contains("ACTION FEEDBACK"));
    }

    #[test]
    fn integration_hint_selects_codex_cli_copy_only_for_codex() {
        assert_eq!(
            integration_hint_for(
                true,
                UiLanguage::Ko,
                "mudctl 상태",
                "mudctl status",
                "/vibemud:mud 상태",
                "/vibemud:mud status",
            ),
            "mudctl 상태"
        );
        assert_eq!(
            integration_hint_for(
                false,
                UiLanguage::En,
                "mudctl 상태",
                "mudctl status",
                "/vibemud:mud 상태",
                "/vibemud:mud status",
            ),
            "/vibemud:mud status"
        );
    }

    #[test]
    fn panel_can_switch_to_quest_dashboard() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        set_quest_view(&conn).unwrap();
        let panel = render_panel_dashboard(
            &conn,
            &HudArgs {
                side: false,
                statusline: false,
                full: false,
                once: true,
                refresh: 1,
                width: Some(110),
                ascii: false,
                live: false,
                panel: true,
                log_lines: 8,
            },
        )
        .unwrap();
        assert!(panel.contains("일일 퀘스트") || panel.contains("DAILY QUESTS"));
        assert!(panel.contains("FEVERTIME"));
        assert!(panel.contains("mudctl quest claim-all"));
        assert!(panel.contains("액션 피드백") || panel.contains("ACTION FEEDBACK"));
        assert!(!panel.contains("AUTO-HUNT SCENE"));
        assert!(!panel.contains("GAME / AUTO-HUNT LOG"));
    }

    #[test]
    fn scene_renders_idle_character_without_monster() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let snapshot = vibemud_core::GameSnapshot::initial();
        let rows = render_auto_hunt_scene(&snapshot, &[], &conn, 42, 12, UiLanguage::En);
        let scene = rows.join("\n");
        assert!(scene.contains("AUTO-HUNT SCENE"));
        assert!(scene.contains("[Train-0/10]"));
        assert!(scene.contains("▒▒◕▒▒◕▒▒"));
        assert!(scene.contains("▒ ▄ ▒"));
        assert!(scene.contains("/▒▒▒▒▒\\"));
        assert!(!scene.contains("( o_o )"));
        assert!(scene.contains("HERO"));
        assert!(!scene.contains("HERO HP"));
        assert!(scene.contains("standing by"));
    }

    #[test]
    fn default_character_has_jump_and_fallen_frames() {
        let mut jump_snapshot = vibemud_core::GameSnapshot::initial();
        jump_snapshot.player.mode = "auto_hunt".to_string();
        jump_snapshot.state_version = 1;
        let jump = hero_sprite(&jump_snapshot, &[]).join("\n");
        assert!(jump.contains("·/▒▒▒▒▒\\›"));
        assert!(jump.contains("╱   ╲"));
        assert!(!jump.contains('<'));

        let mut fallen_snapshot = vibemud_core::GameSnapshot::initial();
        fallen_snapshot.player.mode = "recovering".to_string();
        let fallen = hero_sprite(&fallen_snapshot, &[]).join("\n");
        assert!(fallen.contains("▒▒x▒▒x▒▒"));
        assert!(fallen.contains("▒ - ▒"));
        assert!(fallen.contains("---"));

        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let rows = render_auto_hunt_scene(&fallen_snapshot, &[], &conn, 42, 12, UiLanguage::En);
        let scene = rows.join("\n");
        assert!(scene.contains("Fallen: character is recovering."));
    }

    #[test]
    fn hero_sprite_sets_cover_future_classes() {
        let adventurer = hero_sprite_for_class("adventurer", false, 0).join("\n");
        let fighter = hero_sprite_for_class("fighter", false, 0).join("\n");
        let robot = hero_sprite_for_class("robot", false, 0).join("\n");
        let priest = hero_sprite_for_class("priest", false, 0).join("\n");
        let gangster = hero_sprite_for_class("gangster", false, 0).join("\n");
        let capitalist = hero_sprite_for_class("capitalist", false, 0).join("\n");
        let legacy_warrior = hero_sprite_for_class("warrior", false, 0).join("\n");
        let legacy_rogue = hero_sprite_for_class("rogue", false, 0).join("\n");
        let legacy_mage = hero_sprite_for_class("mage", false, 0).join("\n");
        let legacy_healer = hero_sprite_for_class("healer", false, 0).join("\n");
        let legacy_wanderer = hero_sprite_for_class("wanderer", false, 0).join("\n");

        assert!(adventurer.contains("▒▒◕▒▒◕▒▒"));
        assert!(fighter.contains("╭─────╮"));
        assert!(fighter.contains("◎"));
        assert!(robot.contains("╔════╗"));
        assert!(robot.contains("✧◇✧"));
        assert!(priest.contains("✚"));
        assert!(gangster.contains("▒○▒"));
        assert!(capitalist.contains("◎◎◎◎◎◎◎"));
        assert!(capitalist.contains("╔▒$▒$▒╗"));
        assert_eq!(legacy_warrior, fighter);
        assert_eq!(legacy_rogue, gangster);
        assert_eq!(legacy_mage, robot);
        assert_eq!(legacy_healer, priest);
        assert_eq!(legacy_wanderer, capitalist);
    }

    #[test]
    fn low_level_warrior_uses_basic_adventurer_before_job_change() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.class_id = "warrior".to_string();
        snapshot.player.level = 1;
        let novice = hero_sprite(&snapshot, &[]).join("\n");
        assert!(novice.contains("▒▒◕▒▒◕▒▒"));
        assert!(!novice.contains("╭─────╮"));

        snapshot.player.level = 10;
        let pre_change = hero_sprite(&snapshot, &[]).join("\n");
        assert!(pre_change.contains("▒▒◕▒▒◕▒▒"));
        assert!(!pre_change.contains("╭─────╮"));

        snapshot.player.class_id = "fighter".to_string();
        let fighter = hero_sprite(&snapshot, &[]).join("\n");
        assert!(fighter.contains("╭─────╮"));
        assert!(fighter.contains("◎"));
    }

    #[test]
    fn class_movement_frames_change_for_walk_loop() {
        assert_ne!(adventurer_sprite(true, 1), adventurer_sprite(true, 2));
        assert_ne!(fighter_sprite(true, 1), fighter_sprite(true, 2));
        assert_ne!(robot_sprite(true, 1), robot_sprite(true, 2));
        assert_ne!(priest_sprite(true, 1), priest_sprite(true, 2));
        assert_ne!(gangster_sprite(true, 1), gangster_sprite(true, 2));
        assert_ne!(capitalist_sprite(true, 1), capitalist_sprite(true, 2));
    }

    #[test]
    fn dungeon_mode_uses_walking_frames_and_travel_position() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "dungeon".to_string();
        snapshot.player.class_id = "fighter".to_string();
        snapshot.player.level = 11;

        let frame = hero_sprite_with_phase(&snapshot, &[], 1).join("\n");
        assert!(frame.contains('›'));
        assert!(frame.contains('·'));

        let early = hero_scene_position(&snapshot, MonsterSceneState::None, 46, 12, 0, 60, 1);
        let later = hero_scene_position(&snapshot, MonsterSceneState::None, 46, 12, 0, 60, 9);
        assert!(early > 1);
        assert!(later > early);
    }

    #[test]
    fn live_combat_snapshot_keeps_monster_visible_without_fresh_logs() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "dungeon".to_string();
        snapshot.combat.in_combat = true;

        assert_eq!(
            monster_scene_state(&snapshot, &[]),
            MonsterSceneState::Alive
        );
    }

    #[test]
    fn class_movement_frames_only_signal_rightward_motion() {
        let class_ids = [
            "adventurer",
            "fighter",
            "robot",
            "priest",
            "gangster",
            "capitalist",
        ];
        for class_id in class_ids {
            for phase in 1..=3 {
                let frame = hero_sprite_for_class(class_id, true, phase).join("\n");
                assert!(
                    !frame.contains('<'),
                    "{class_id} phase {phase} should not contain leftward markers:\n{frame}"
                );
                assert!(
                    frame.contains('›') || frame.contains('»') || frame.contains('·'),
                    "{class_id} phase {phase} should show rightward motion/trail:\n{frame}"
                );
            }
        }
    }

    #[test]
    fn scene_renders_defeated_monster_during_auto_hunt() {
        let _guard = test_lock();
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("VIBEMUD_HOME", tmp.path());
        vibemud_db::init_app().unwrap();
        let (_paths, conn) = vibemud_db::open_app().unwrap();
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "auto_hunt".to_string();
        snapshot.state_version = 10;
        let logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Imp defeated.".to_string(),
        }];
        let rows = render_auto_hunt_scene(&snapshot, &logs, &conn, 60, 12, UiLanguage::En);
        let scene = rows.join("\n");
        assert!(scene.contains('×'));
        assert!(scene.contains("MON"));
        assert!(!scene.contains("MON HP"));
        assert!(scene.contains("░"));
        assert!(scene.contains("monster defeated"));
    }

    #[test]
    fn combat_hp_rows_split_labels_and_right_align_monster() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "auto_hunt".to_string();
        snapshot.state_version = 10;
        let logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Warrior hit Wolf for 15. Wolf HP 38/53.".to_string(),
        }];
        let rows = render_combat_hp_rows(&snapshot, &logs, 50, UiLanguage::Ko);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].starts_with("영웅 "));
        assert!(rows[0].ends_with("몬스터 38/53"));
        assert!(rows[1].starts_with("["));
        assert!(rows[1].ends_with("]"));
        assert_eq!(display_width(&rows[0]), 50);
        assert_eq!(display_width(&rows[1]), 50);
    }

    #[test]
    fn normal_monster_sprite_is_bottom_aligned_with_hero() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "auto_hunt".to_string();
        snapshot.state_version = 10;
        let logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Imp appeared.".to_string(),
        }];

        let rows = render_combat_sprite_rows(&snapshot, &logs, 60);
        let monster_head = rows
            .iter()
            .position(|row| row.contains("▒▒▒▒▒▒"))
            .expect("normal monster head should render");
        let monster_feet = rows
            .iter()
            .position(|row| row.contains("▒ ▒ ▒ ▒"))
            .expect("normal monster feet should render");

        assert!(monster_head > 0);
        assert!(monster_feet > monster_head);
        assert_eq!(monster_feet, rows.len().saturating_sub(1));
    }

    #[test]
    fn defeated_monster_sprite_expires_after_short_ttl() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "auto_hunt".to_string();
        snapshot.state_version = 10;
        snapshot.combat.in_combat = false;
        let old = (OffsetDateTime::now_utc()
            - time::Duration::seconds(DEFEATED_SCENE_TTL_SECONDS + 2))
        .format(&Rfc3339)
        .unwrap();
        let logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: old,
            event_type: "tick_advanced".to_string(),
            message: "Imp defeated.".to_string(),
        }];

        assert_eq!(
            monster_scene_state(&snapshot, &logs),
            MonsterSceneState::None
        );
    }

    #[test]
    fn monster_sprite_kind_maps_normal_and_boss_roles() {
        let mut snapshot = vibemud_core::GameSnapshot::initial();
        snapshot.player.mode = "dungeon".to_string();
        snapshot.state_version = 10;
        let normal_logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Brute appeared.".to_string(),
        }];
        assert_eq!(
            monster_scene_kind(&snapshot, &normal_logs),
            MonsterKind::ClaudeGlyphImp
        );

        let boss_logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Boss Goblin appeared.".to_string(),
        }];
        assert_eq!(
            monster_scene_kind(&snapshot, &boss_logs),
            MonsterKind::DookkaBurrower
        );
        assert_eq!(
            monster_scene_state(&snapshot, &boss_logs),
            MonsterSceneState::Alive
        );

        let wanderer_boss_logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Boss Warden appeared.".to_string(),
        }];
        assert_eq!(
            monster_scene_kind(&snapshot, &wanderer_boss_logs),
            MonsterKind::WandererBoss
        );

        let portrait_boss_logs = vec![vibemud_db::LogEntry {
            state_version: 10,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Boss Medusa appeared.".to_string(),
        }];
        assert_eq!(
            monster_scene_kind(&snapshot, &portrait_boss_logs),
            MonsterKind::PortraitKeeperBoss
        );
    }

    #[test]
    fn normal_monster_sprite_uses_previous_boss_block_shape() {
        let sprite = monster_sprite(MonsterSceneState::Alive, MonsterKind::ClaudeGlyphImp);
        assert_eq!(sprite[0], "  ▒▒▒▒▒▒  ");
        assert_eq!(sprite[1], "▒▒▒▄▒▒▄▒▒▒");
        assert_eq!(sprite[2], "  ▒ ▒ ▒ ▒ ");
        assert_eq!(sprite.len(), 3);
    }

    #[test]
    fn warrior_boss_monster_sprite_uses_warrior_shape() {
        let sprite = monster_sprite(MonsterSceneState::Alive, MonsterKind::DookkaBurrower);
        assert_eq!(sprite[0], "  \\|/   ");
        assert_eq!(sprite[1], " .-^-.  ");
        assert_eq!(sprite[2], "/  o o\\ ");
        assert_eq!(sprite[3], "|  '-' |");
        assert_eq!(sprite[4], " /|_|\\ ");
        assert_eq!(sprite[5], "  / \\  ");
    }

    #[test]
    fn wanderer_and_portrait_keeper_boss_sprites_are_available() {
        let wanderer = monster_sprite(MonsterSceneState::Alive, MonsterKind::WandererBoss);
        let portrait = monster_sprite(MonsterSceneState::Alive, MonsterKind::PortraitKeeperBoss);
        assert!(wanderer.join("\n").contains("<|  V  |>"));
        assert!(portrait.join("\n").contains(".-\"\"\"-."));
    }

    #[test]
    fn defeated_monster_sprites_are_simple_markers() {
        let normal = monster_sprite(MonsterSceneState::Defeated, MonsterKind::ClaudeGlyphImp);
        let boss = monster_sprite(MonsterSceneState::Defeated, MonsterKind::DookkaBurrower);
        assert_eq!(normal, ["▒▒▒×▒▒×▒▒▒", "  ------  "]);
        assert!(boss.join("\n").contains("/  x x\\"));
        assert!(boss.join("\n").contains("|  --- |"));
    }

    #[test]
    fn dookka_monster_sprite_styles_body_and_eyes_separately() {
        let _guard = test_lock();
        std::env::remove_var("NO_COLOR");
        let styled = style_dookka_sprite_row("▒▄");
        assert!(styled.contains("\x1b[48;2;224;132;100m \x1b[0m"));
        assert!(styled.contains("\x1b[38;2;224;132;100m▄\x1b[0m"));
        assert!(!styled.contains('e'));
        assert_eq!(display_width(&styled), 2);
    }

    #[test]
    fn korean_message_display_localizes_combat_log() {
        let message = display_message("Burrower defeated.", UiLanguage::Ko);
        assert_eq!(message, "버로어 처치");
        assert_eq!(
            display_message("Reward: +32 XP.", UiLanguage::Ko),
            "보상: +32 경험치"
        );
        assert_eq!(
            display_message("Reward: +10 gold.", UiLanguage::Ko),
            "보상: +10 골드"
        );
        assert_eq!(
            display_message("Wolf remains at 38/53 HP.", UiLanguage::Ko),
            "늑대 잔여 HP 38/53"
        );
        assert_eq!(
            display_message(
                "Warrior hit Boss Golem for 23. Boss Golem HP 12/99.",
                UiLanguage::Ko
            ),
            "투사가 보스 골렘에게 23 피해 (몬스터 HP 12/99)"
        );
        assert_eq!(
            display_message("Warrior missed Wolf.", UiLanguage::Ko),
            "투사의 공격이 늑대에게 빗나감"
        );
        assert_eq!(
            display_message(
                "Boss Golem hit Warrior for 7. Warrior HP 93/100.",
                UiLanguage::Ko
            ),
            "보스 골렘이 투사에게 7 피해 (영웅 HP 93/100)"
        );
        assert_eq!(
            display_message(
                "Crawler hit Warrior for 21. Warrior HP 513/552.",
                UiLanguage::Ko
            ),
            "크롤러가 투사에게 21 피해 (영웅 HP 513/552)"
        );
        assert_eq!(
            display_message("Boss reward: Perseus Blade.", UiLanguage::Ko),
            "보스 보상 획득: 페르세우스 칼날"
        );
        assert_eq!(
            display_message(
                "Dungeon crystal-cave cleared. Restarting dungeon run.",
                UiLanguage::Ko
            ),
            "수정 동굴 클리어. 던전 재개"
        );
    }

    #[test]
    fn boss_reward_log_color_uses_reward_item_rarity() {
        let _guard = test_lock();
        std::env::remove_var("NO_COLOR");
        let rare = color_log_message(
            "Boss reward: Perseus Blade.",
            &display_message("Boss reward: Perseus Blade.", UiLanguage::Ko),
            UiLanguage::Ko,
        );
        assert!(
            rare.contains("\x1b[34m"),
            "rare boss reward should be blue, got {rare:?}"
        );
        assert!(!rare.contains("\x1b[33m"));

        let uncommon = color_log_message(
            "Boss reward: Hector Axe.",
            &display_message("Boss reward: Hector Axe.", UiLanguage::Ko),
            UiLanguage::Ko,
        );
        assert!(
            uncommon.contains("\x1b[32m"),
            "uncommon boss reward should be green, got {uncommon:?}"
        );
        assert!(!uncommon.contains("\x1b[33m"));

        let gold = color_log_message(
            "Boss reward: +100 gold.",
            &display_message("Boss reward: +100 gold.", UiLanguage::Ko),
            UiLanguage::Ko,
        );
        assert!(gold.contains("\x1b[33m"));
    }

    #[test]
    fn ansi_color_does_not_change_display_width() {
        let colored = colorize("HP 111/120", AnsiColor::Green);
        assert_eq!(display_width(&colored), "HP 111/120".chars().count());
        let line = panel_line_raw(&format!("{colored}  MP 30/30"), 40);
        assert_eq!(display_width(&line), 42);
    }

    #[test]
    fn panel_log_uses_fixed_tag_and_colored_rewards() {
        let _guard = test_lock();
        std::env::remove_var("NO_COLOR");
        let entry = vibemud_db::LogEntry {
            state_version: 42,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Reward: +30 XP.".to_string(),
        };
        let line = panel_log_line(&entry, UiLanguage::Ko);
        assert!(line.starts_with("[보 상]"));
        assert!(!line.contains("#42"));
        assert!(line.contains("\x1b[32m+30 경험치\x1b[0m"));
        let entry = vibemud_db::LogEntry {
            state_version: 43,
            created_at: "now".to_string(),
            event_type: "tick_advanced".to_string(),
            message: "Reward: +12 gold.".to_string(),
        };
        let line = panel_log_line(&entry, UiLanguage::Ko);
        assert!(line.starts_with("[보 상]"));
        assert!(line.contains("\x1b[33m+12 골드\x1b[0m"));
    }
}
