#!/usr/bin/env bash
set -euo pipefail

print_guide() {
  cat <<'GUIDE'
VibeMUD 조작 가이드
============================================================
VibeMUD는 코딩 중 옆에서 자동으로 진행되는 로컬 MUD RPG입니다.
조작은 메뉴 중심으로 단순하게 사용합니다.

핵심 단축키
------------------------------------------------------------
  c  캐릭터/능력치       i  장비/소지품
  m  지도/던전 선택      q  일일 퀘스트
  start  게임 시작       end  게임 종료

빠른 흐름
------------------------------------------------------------
1) 캐릭터 상태 보기
   /vibemud:mud c
   - 캐릭터 레벨, HP/MP, 능력치, 전투력을 확인합니다.
   - 화면에서 빠져나올 때는 q 를 누릅니다.

2) 장비/소지품 관리
   /vibemud:mud i
   - 장착 장비, 보유 골드, 능력치, 소지품을 한 화면에서 봅니다.
   - ↑/↓ 로 이동, Enter 로 선택, q 로 뒤로/닫기.
   - 일괄 비우기는 잠금 아이템을 제외하고 소지품을 모두 판매합니다.
   - 보존할 아이템은 선택 메뉴에서 "잠금"을 켜면 이름 앞에 [잠금]이 붙습니다.

3) 지역/던전 변경
   /vibemud:mud m
   - 지도/지역/던전 선택창을 엽니다.
   - 선택창이 열리면 ↑/↓ 이동, Enter 선택, q 취소.

4) 퀘스트
   /vibemud:mud q
   - 일일 퀘스트 5개를 확인합니다.
   - 완료된 퀘스트는 선택해서 보상을 받고, 보상 일괄 수령도 가능합니다.
   - 화면에서 빠져나올 때는 q 를 누릅니다.

5) 설정
   /vibemud:mud set
   - 설정 메뉴를 엽니다.
   - 언어와 팝업 선택창 사용 여부는 한 번 더 2개 옵션 중에서 최종 선택합니다.
   - 오프닝 시나리오 다시보기와 게임 전체 초기화도 설정 메뉴 안에서 선택합니다.

자주 쓰는 명령
------------------------------------------------------------
  /vibemud:mud c                 캐릭터/능력치 상세 화면 열기
  /vibemud:mud i                 장비/소지품 관리창 열기
  /vibemud:mud m                 지도/지역/던전 선택창 열기
  /vibemud:mud q                 일일 퀘스트창 열기
  /vibemud:mud set               설정 메뉴 열기
  /vibemud:mud start             게임 시작/이어하기
  /vibemud:mud end               게임 종료
  /vibemud:mud help              이 조작 가이드 보기

보조 화면 공통 조작
------------------------------------------------------------
  ↑/↓ 또는 j/k        항목 이동
  Enter               선택/실행
  q                   뒤로 가기 또는 닫기

장비 조작
------------------------------------------------------------
  /vibemud:mud i                 장비/소지품 관리창
  장비창 안에서 Enter            선택한 장비 강화/툴팁/소지품 동작
  소지품창 안에서 Enter          장착/강화/판매 동작
  일괄 비우기                   잠금 제외 소지품 전체 판매
  잠금/잠금해제                  일괄 비우기에서 제외할 아이템 보호

현재 장비 슬롯
------------------------------------------------------------
  weapon       무기        subweapon    부무기
  armor_top    상의        armor_bottom 하의
  trinket      장신구      boots        신발
  pet          펫          special      특수장비

한국어 별칭
------------------------------------------------------------
  캐릭터=c, 장비/소지품/가방=i, 지도/던전/지역=m
  퀘스트/일일퀘스트=q, 설정=set, 시작=start, 종료=end, 도움말=help
  보조 화면 안에서는 q 로 뒤로/닫기

추천 루틴
------------------------------------------------------------
  /vibemud:mud start   게임 시작/이어하기
  /vibemud:mud c       캐릭터 상태 확인
  /vibemud:mud i       장비/소지품 정리
  /vibemud:mud q       일일 퀘스트 보상 확인
  /vibemud:mud m       더 좋은 지역/던전으로 이동
  /vibemud:mud set     필요할 때 설정/초기화
  /vibemud:mud end     게임 종료
GUIDE
}


print_live_preview() {
  seconds="${1:-6}"
  echo
  echo "Live preview (${seconds}s). The runtime keeps running in the background after this preview."
  echo "For the continuous live dashboard, run: /vibemud:mud watch"
  echo
  i=1
  while [[ "$i" -le "$seconds" ]]; do
    echo "[$i/$seconds] $(run_vibemud statusline 2>/dev/null || true)"
    latest="$(run_mudctl log --tail 1 2>/dev/null | tail -1 || true)"
    if [[ -n "$latest" ]]; then
      echo "      $latest"
    fi
    sleep 1
    i=$((i + 1))
  done
  echo
  run_mudctl system || true
}

print_next() {
  cat <<'NEXT'
Suggested VibeMUD loop:

1. If stopped or idle:
   /vibemud:mud a

2. If you want equipment/inventory management:
   /vibemud:mud i

3. If you want the dungeon/area map:
   /vibemud:mud m

4. If you want to know what is happening:
   /vibemud:mud now

5. If you want recent events:
   /vibemud:mud log

6. If you want the live game screen:
   /vibemud:mud panel

7. If you are done:
   /vibemud:mud s
NEXT
  echo
  run_mudctl system || true
}

script_dir() {
  cd "$(dirname "${BASH_SOURCE[0]}")" && pwd
}

repo_root() {
  local dir
  dir="$(script_dir)"
  cd "${dir}/../../../.." && pwd
}

resolve_binary() {
  local name="$1" root
  if [[ -n "${VIBEMUD_BIN_DIR:-}" && -x "${VIBEMUD_BIN_DIR%/}/${name}" ]]; then
    printf '%s\n' "${VIBEMUD_BIN_DIR%/}/${name}"
    return 0
  fi
  if command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
    return 0
  fi
  root="$(repo_root)"
  if [[ -x "${root}/target/debug/${name}" ]]; then
    printf '%s\n' "${root}/target/debug/${name}"
    return 0
  fi
  return 1
}

run_vibemud() {
  local bin
  bin="$(resolve_binary vibemud)" || {
    echo "vibemud binary not found in PATH or target/debug; run cargo build -p vibemud-cli" >&2
    return 127
  }
  "$bin" "$@"
}

run_mudctl() {
  local bin
  bin="$(resolve_binary mudctl)" || {
    echo "mudctl binary not found in PATH or target/debug; run cargo build -p vibemud-cli" >&2
    return 127
  }
  "$bin" "$@"
}

run_vibemud_runtime() {
  local bin
  bin="$(resolve_binary vibemud-runtime)" || {
    echo "vibemud-runtime binary not found in VIBEMUD_BIN_DIR, PATH, or target/debug; run cargo build -p vibemud-cli" >&2
    return 127
  }
  "$bin" "$@"
}

print_settings_menu() {
  cat <<'SETTINGS'
VibeMUD 설정 메뉴
============================================================
  r  게임 전체 초기화
     - 캐릭터 레벨/골드/장비/소지품/전투/로그/런타임 상태를 초기값으로 되돌립니다.
     - config.toml의 UI/통합 설정 파일은 유지합니다.
     - 실행: /vibemud:mud reset

  l  한국어/English 설정
     - 선택 후 한국어 / English 중 하나를 한 번 더 골라 최종 적용합니다.

  p  팝업 선택창 사용 여부
     - 선택 후 사용 / 사용 안 함 중 하나를 한 번 더 골라 최종 적용합니다.

  o  오프닝 시나리오 다시보기
     - 플래닛 64 배경 설명과 대화 오프닝을 다시 재생합니다.
     - 실행: /vibemud:mud intro

  q  닫기
SETTINGS
}

reset_game_progress() {
  if is_safe_context; then
    run_quiet run_mudctl hunt stop || true
    run_quiet run_vibemud session stop || true
    run_quiet run_vibemud reset --yes || return 1
    run_quiet run_mudctl stats close || true
    close_tmux_panel
    close_cmux_panel
    close_ghostty_panel
    stop_background_helpers
    stop_terminal_broadcast
    ack "게임 리셋 완료 · /mud start 시 오프닝 후 HUD"
    return 0
  fi
  run_mudctl hunt stop >/dev/null 2>&1 || true
  run_vibemud session stop >/dev/null 2>&1 || true
  run_vibemud reset --yes
  run_mudctl stats close >/dev/null 2>&1 || true
  close_tmux_panel
  close_cmux_panel
  close_ghostty_panel
  stop_background_helpers
  stop_terminal_broadcast
}

run_settings_menu() {
  if open_cmux_settings_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_settings_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_ghostty_selector_pane settings "VibeMUD Settings" >/dev/null 2>&1; then
    return 0
  fi
  if [[ -t 0 && -t 1 ]]; then
    run_settings_selector
    return $?
  fi
  if ! is_unsafe_context; then
    return 2
  fi
  print_settings_menu
  return 2
}

config_value() {
  local key="$1" default="$2" line value
  line="$(run_vibemud config get "$key" 2>/dev/null | tail -1 || true)"
  if [[ "$line" == "$key = "* ]]; then
    value="${line#"$key = "}"
    value="${value%\"}"
    value="${value#\"}"
    printf '%s\n' "$value"
  else
    printf '%s\n' "$default"
  fi
}

popup_pane_enabled() {
  case "$(config_value ui.popup_pane_enabled true)" in
    1|true|TRUE|True|yes|YES|on|ON) return 0 ;;
    *) return 1 ;;
  esac
}

message_printline() {
  local value
  value="$(config_value ui.message_printline 7)"
  if [[ "$value" =~ ^[0-9]+$ && "$value" -ge 1 ]]; then
    printf '%s\n' "$value"
  else
    printf '7\n'
  fi
}

context_mode="${VIBEMUD_CONTEXT_MODE:-safe}"
verbose_context=0
unsafe_context=0
filtered_args=()
for arg in "$@"; do
  case "$arg" in
    --verbose)
      verbose_context=1
      ;;
    --unsafe-context)
      unsafe_context=1
      ;;
    --context-mode=*)
      context_mode="${arg#--context-mode=}"
      ;;
    *)
      filtered_args+=("$arg")
      ;;
  esac
done
set -- "${filtered_args[@]}"

if [[ "$verbose_context" -eq 1 ]]; then
  context_mode="verbose"
fi
if [[ "$unsafe_context" -eq 1 ]]; then
  context_mode="unsafe"
fi

is_safe_context() {
  [[ "$context_mode" == "safe" || "$context_mode" == "hook" || "$context_mode" == "zero" ]]
}

is_unsafe_context() {
  [[ "$context_mode" == "unsafe" ]]
}

is_nested_agent_session_command() {
  [[ "${1:-}" == "session" ]] || return 1
  case "${2:-}" in
    codex|claude) return 0 ;;
    *) return 1 ;;
  esac
}

ack() {
  printf '[VibeMUD] %s\n' "$*"
}

run_quiet() {
  local tmp status
  tmp="$(mktemp "${TMPDIR:-/tmp}/vibemud-command.XXXXXX")"
  if "$@" >"$tmp" 2>&1; then
    rm -f "$tmp"
    return 0
  fi
  status=$?
  sed -n '1,3p' "$tmp" >&2 || true
  rm -f "$tmp"
  return "$status"
}

command_label() {
  printf '%s' "$*" | tr '\n' ' ' | cut -c 1-80
}

if [[ $# -eq 0 || "${1:-}" == "help" || "${1:-}" == "--help" || "${1:-}" == "-h" || "${1:-}" == "guide" || "${1:-}" == "가이드" || "${1:-}" == "도움말" ]]; then
  print_guide
  exit 0
fi

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

vibemud_home_dir() {
  if [[ -n "${VIBEMUD_HOME:-}" ]]; then
    printf '%s\n' "$VIBEMUD_HOME"
  else
    printf '%s\n' "${HOME}/.vibemud"
  fi
}

state_safe_id() {
  local value="${1:-global}"
  value="${value//[^A-Za-z0-9_.-]/_}"
  printf '%s\n' "${value:-global}"
}

tmux_state_scope() {
  local pane_id="${TMUX_PANE:-${VIBEMUD_TMUX_PANE:-}}" scope
  command -v tmux >/dev/null 2>&1 || {
    printf 'global\n'
    return 0
  }
  if [[ -n "$pane_id" ]]; then
    scope="$(tmux display-message -p -t "$pane_id" '#{session_name}-#{window_id}' 2>/dev/null || true)"
  else
    scope="$(tmux display-message -p '#{session_name}-#{window_id}' 2>/dev/null || true)"
  fi
  state_safe_id "tmux-${scope:-global}"
}

cmux_state_scope() {
  state_safe_id "cmux-${CMUX_WORKSPACE_ID:-workspace}-${CMUX_TAB_ID:-tab}-${CMUX_BUNDLE_ID:-bundle}"
}

panel_state_key() {
  if is_cmux_session; then
    cmux_state_scope
  elif [[ -n "${TMUX:-}${TMUX_PANE:-}${VIBEMUD_TMUX_PANE:-}" ]]; then
    tmux_state_scope
  else
    printf 'global\n'
  fi
}

panel_state_file() {
  printf '%s/panel-pane-%s' "$(vibemud_home_dir)" "$(panel_state_key)"
}

quest_pane_state_file() {
  printf '%s/quest-pane-%s' "$(vibemud_home_dir)" "$(panel_state_key)"
}

background_helpers_file() {
  printf '%s/background-helper-pids' "$(vibemud_home_dir)"
}

cmux_panel_state_file() {
  printf '%s/cmux-panel-surface-%s' "$(vibemud_home_dir)" "$(cmux_state_scope)"
}

cmux_quest_state_file() {
  printf '%s/cmux-quest-surface-%s' "$(vibemud_home_dir)" "$(cmux_state_scope)"
}

cmux_panel_launcher() {
  printf '%s/cmux-hud-panel.sh' "$(vibemud_home_dir)"
}

ghostty_panel_launcher() {
  printf '%s/ghostty-hud-panel.sh' "$(vibemud_home_dir)"
}

ghostty_selector_launcher() {
  printf '%s/ghostty-%s-selector.sh' "$(vibemud_home_dir)" "$1"
}

cmux_selector_launcher() {
  printf '%s/cmux-map-selector.sh' "$(vibemud_home_dir)"
}

cmux_stats_launcher() {
  printf '%s/cmux-stats-selector.sh' "$(vibemud_home_dir)"
}

cmux_settings_launcher() {
  printf '%s/cmux-settings-selector.sh' "$(vibemud_home_dir)"
}

cmux_quest_launcher() {
  printf '%s/cmux-quest-selector.sh' "$(vibemud_home_dir)"
}

tmux_quest_launcher() {
  printf '%s/tmux-quest-selector.sh' "$(vibemud_home_dir)"
}

broadcast_pid_file() {
  printf '%s/terminal-broadcast.pid' "$(vibemud_home_dir)"
}

is_cmux_session() {
  [[ -n "${CMUX_WORKSPACE_ID:-}" || -n "${CMUX_TAB_ID:-}" || -n "${CMUX_BUNDLE_ID:-}" ]]
}

is_ghostty_session() {
  [[ "${TERM_PROGRAM:-}" == "ghostty" || "${TERM:-}" == "xterm-ghostty" || -n "${GHOSTTY_RESOURCES_DIR:-}" ]]
}

ghostty_applescript_available() {
  [[ "$(uname -s 2>/dev/null || true)" == "Darwin" ]] || return 1
  command -v osascript >/dev/null 2>&1 || return 1
}

ghostty_state_scope() {
  local source="${CLAUDE_SESSION_ID:-${CLAUDECODE_SESSION_ID:-${PWD:-global}}}"
  state_safe_id "ghostty-${source}"
}

ghostty_panel_state_file() {
  printf '%s/ghostty-panel-terminal-%s' "$(vibemud_home_dir)" "$(ghostty_state_scope)"
}

cmux_cli() {
  if [[ -n "${CMUX_BUNDLED_CLI_PATH:-}" && -x "${CMUX_BUNDLED_CLI_PATH}" ]]; then
    printf '%s\n' "${CMUX_BUNDLED_CLI_PATH}"
  else
    command -v cmux 2>/dev/null || {
      local app_cli="/Applications/cmux.app/Contents/Resources/bin/cmux"
      [[ -x "$app_cli" ]] && printf '%s\n' "$app_cli"
    } || true
  fi
}

process_alive() {
  local pid="${1:-}"
  [[ -n "$pid" ]] && kill -0 "$pid" 2>/dev/null
}

track_background_helper() {
  local pid="${1:-}" file
  [[ -n "$pid" ]] || return 0
  file="$(background_helpers_file)"
  mkdir -p "$(dirname "$file")"
  printf '%s\n' "$pid" >> "$file"
}

stop_background_helpers() {
  local file pid
  file="$(background_helpers_file)"
  [[ -f "$file" ]] || return 0
  while IFS= read -r pid; do
    [[ -n "$pid" ]] || continue
    if process_alive "$pid"; then
      kill "$pid" 2>/dev/null || true
    fi
  done < "$file"
  rm -f "$file"
}

detect_visible_tty() {
  if [[ -t 1 ]]; then
    tty
    return 0
  fi

  local pid tty_name parent
  pid="$$"
  while [[ -n "$pid" && "$pid" != "0" ]]; do
    tty_name="$(ps -o tty= -p "$pid" 2>/dev/null | awk '{print $1}')"
    if [[ -n "$tty_name" && "$tty_name" != "??" && "$tty_name" != "?" ]]; then
      printf '/dev/%s\n' "$tty_name"
      return 0
    fi
    parent="$(ps -o ppid= -p "$pid" 2>/dev/null | tr -d ' ')"
    [[ -z "$parent" || "$parent" == "$pid" ]] && break
    pid="$parent"
  done
  return 1
}

terminal_size_for_tty() {
  local tty_path="$1"
  local rows cols
  if size="$(stty size < "$tty_path" 2>/dev/null)"; then
    rows="${size%% *}"
    cols="${size##* }"
  fi
  printf '%s %s\n' "${rows:-32}" "${cols:-80}"
}

stop_terminal_broadcast() {
  local pid_file pid
  pid_file="$(broadcast_pid_file)"
  pid="$(cat "$pid_file" 2>/dev/null || true)"
  if process_alive "$pid"; then
    kill "$pid" 2>/dev/null || true
  fi
  rm -f "$pid_file"
}

start_terminal_broadcast() {
  local tty_path pid_file existing home_dir quoted_path quoted_home vibemud_bin quoted_vibemud rows cols lines panel_cmd
  tty_path="$(detect_visible_tty || true)"
  if [[ -z "$tty_path" || ! -w "$tty_path" ]]; then
    return 1
  fi

  home_dir="$(vibemud_home_dir)"
  mkdir -p "$home_dir"
  pid_file="$(broadcast_pid_file)"
  existing="$(cat "$pid_file" 2>/dev/null || true)"
  if process_alive "$existing"; then
    echo "VibeMUD terminal HUD broadcast is already running on ${tty_path} (pid ${existing})."
    return 0
  fi

  read -r rows cols < <(terminal_size_for_tty "$tty_path")
  lines="$(message_printline)"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  vibemud_bin="$(resolve_binary vibemud)" || return 1
  [[ -n "$vibemud_bin" ]] || return 1
  quoted_vibemud="$(shell_quote "$vibemud_bin")"
  panel_cmd="export PATH=$quoted_path; export VIBEMUD_HOME=$quoted_home; export VIBEMUD_PANEL_HEIGHT=${rows}; export VIBEMUD_PANEL_WIDTH=${cols}; printf '\033]2;VibeMUD HUD\033\\' > '$tty_path'; echo 'Starting VibeMUD HUD...' > '$tty_path'; $quoted_vibemud intro --if-needed > '$tty_path' 2>&1 || true; exec $quoted_vibemud hud --panel --refresh 1 --log-lines ${lines} > '$tty_path' 2>&1"
  nohup bash -lc "$panel_cmd" >/dev/null 2>&1 &
  printf '%s\n' "$!" > "$pid_file"
  echo "VibeMUD terminal HUD broadcast started on ${tty_path} (pid $!)."
  echo "Use Ctrl-C in the terminal to interrupt the view if it is foreground; use /vibemud:mud stop to stop runtime and broadcaster."
}

run_terminal_broadcast() {
  if start_terminal_broadcast; then
    return 0
  fi
  echo
  echo "VibeMUD live broadcast is starting in this command output."
  echo "Press Ctrl-C to close only the HUD view; the background runtime keeps running."
  echo "Run /vibemud:mud stop later to stop auto-hunt/runtime."
  echo
  sleep 1
  run_vibemud intro --if-needed || true
  run_vibemud hud --panel --refresh 1 --log-lines "$(message_printline)"
}

extract_cmux_surface_ref() {
  grep -Eo 'surface:[0-9]+' | tail -1
}

cmux_surface_alive() {
  local cli="$1"
  local surface="$2"
  [[ -n "$surface" ]] && "$cli" read-screen --surface "$surface" --lines 1 >/dev/null 2>&1
}

cmux_surface_has_hud() {
  local cli="$1"
  local surface="$2"
  local screen
  [[ -n "$surface" ]] || return 1
  screen="$("$cli" read-screen --surface "$surface" --lines 16 2>/dev/null || true)"
  printf '%s\n' "$screen" | grep -Eq 'VibeMUD|AUTO-HUNT|GAME / AUTO-HUNT|상세 능력치|자동 사냥|DAILY QUESTS|일일 퀘스트'
}

close_cmux_surface_if_vibemud() {
  local cli="$1"
  local surface="$2"
  local current_surface
  [[ -n "$surface" ]] || return 0
  cmux_surface_alive "$cli" "$surface" || return 0
  current_surface="$(cmux_current_surface || true)"
  if cmux_same_surface "$surface" "$current_surface"; then
    return 0
  fi
  if cmux_surface_has_hud "$cli" "$surface"; then
    "$cli" close-surface --surface "$surface" >/dev/null 2>&1 || true
  fi
}

cmux_current_surface() {
  if [[ -n "${CMUX_SURFACE_ID:-}" ]]; then
    printf '%s\n' "${CMUX_SURFACE_ID}"
    return 0
  fi
  local cli output
  cli="$(cmux_cli)"
  [[ -n "$cli" ]] || return 1
  output="$("$cli" identify 2>/dev/null || true)"
  printf '%s\n' "$output" | grep -Eo 'surface:[0-9]+|[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}' | tail -1
}

cmux_same_surface() {
  local left="${1:-}" right="${2:-}"
  [[ -n "$left" && -n "$right" && "$left" == "$right" ]]
}

cmux_exec_command() {
  local launcher="$1"
  # Prefix with a harmless command. If an interactive shell prompt consumes the
  # first byte, the remaining command still reaches the real launcher instead of
  # becoming a broken partial command. Run the launcher as a child of the
  # interactive shell so Ctrl-C can return the pane to a shell for later menus.
  printf 'true; bash %s' "$(shell_quote "$launcher")"
}

write_cmux_panel_launcher() {
  local launcher="$1"
  local home_dir quoted_path quoted_home vibemud_bin quoted_vibemud lines
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  vibemud_bin="$(resolve_binary vibemud)" || return 1
  [[ -n "$vibemud_bin" ]] || return 1
  quoted_vibemud="$(shell_quote "$vibemud_bin")"
  lines="$(message_printline)"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
printf '\033]2;VibeMUD HUD\033\\'
echo 'Starting VibeMUD HUD...'
$quoted_vibemud intro --if-needed || true
exec $quoted_vibemud hud --panel --refresh 1 --log-lines $lines
LAUNCHER
  chmod +x "$launcher"
}

write_ghostty_panel_launcher() {
  local launcher="$1"
  local home_dir quoted_path quoted_home vibemud_bin quoted_vibemud lines
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  vibemud_bin="$(resolve_binary vibemud)" || return 1
  [[ -n "$vibemud_bin" ]] || return 1
  quoted_vibemud="$(shell_quote "$vibemud_bin")"
  lines="$(message_printline)"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
printf '\033]2;VibeMUD HUD\033\\'
echo 'Starting VibeMUD HUD...'
$quoted_vibemud intro --if-needed || true
exec $quoted_vibemud hud --panel --refresh 1 --log-lines $lines
LAUNCHER
  chmod +x "$launcher"
}

write_ghostty_selector_launcher() {
  local launcher="$1"
  local selector="$2"
  local title="$3"
  local return_terminal="${4:-}"
  local home_dir quoted_path quoted_home quoted_script quoted_return_terminal
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  quoted_script="$(shell_quote "$(script_dir)/vibemud-claude.sh")"
  quoted_return_terminal="$(shell_quote "$return_terminal")"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
export VIBEMUD_CONTEXT_MODE=unsafe
export VIBEMUD_SELECTOR_PANE=1
export VIBEMUD_RETURN_GHOSTTY_TERMINAL=$quoted_return_terminal
printf '\\033]2;$title\\033\\\\'
exec bash $quoted_script ${selector}-select
LAUNCHER
  chmod +x "$launcher"
}

write_cmux_selector_launcher() {
  local launcher="$1"
  local home_dir quoted_path quoted_home quoted_script return_surface quoted_return_surface
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  quoted_script="$(shell_quote "$(script_dir)/vibemud-claude.sh")"
  return_surface="${VIBEMUD_RETURN_CMUX_SURFACE:-${CMUX_SURFACE_ID:-}}"
  quoted_return_surface="$(shell_quote "$return_surface")"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
export VIBEMUD_CONTEXT_MODE=unsafe
export VIBEMUD_SELECTOR_PANE=1
export VIBEMUD_RETURN_CMUX_SURFACE=$quoted_return_surface
printf '\\033]2;VibeMUD Map\\033\\\\'
exec bash $quoted_script map-select
LAUNCHER
  chmod +x "$launcher"
}

write_cmux_stats_launcher() {
  local launcher="$1"
  local home_dir quoted_path quoted_home quoted_script return_surface quoted_return_surface
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  quoted_script="$(shell_quote "$(script_dir)/vibemud-claude.sh")"
  return_surface="${VIBEMUD_RETURN_CMUX_SURFACE:-${CMUX_SURFACE_ID:-}}"
  quoted_return_surface="$(shell_quote "$return_surface")"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
export VIBEMUD_CONTEXT_MODE=unsafe
export VIBEMUD_SELECTOR_PANE=1
export VIBEMUD_RETURN_CMUX_SURFACE=$quoted_return_surface
printf '\\033]2;VibeMUD Items\\033\\\\'
exec bash $quoted_script stats-select
LAUNCHER
  chmod +x "$launcher"
}

write_cmux_settings_launcher() {
  local launcher="$1"
  local home_dir quoted_path quoted_home quoted_script return_surface quoted_return_surface
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  quoted_script="$(shell_quote "$(script_dir)/vibemud-claude.sh")"
  return_surface="${VIBEMUD_RETURN_CMUX_SURFACE:-${CMUX_SURFACE_ID:-}}"
  quoted_return_surface="$(shell_quote "$return_surface")"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
export VIBEMUD_CONTEXT_MODE=unsafe
export VIBEMUD_SELECTOR_PANE=1
export VIBEMUD_RETURN_CMUX_SURFACE=$quoted_return_surface
printf '\\033]2;VibeMUD Settings\\033\\\\'
exec bash $quoted_script settings-select
LAUNCHER
  chmod +x "$launcher"
}

write_cmux_quest_launcher() {
  local launcher="$1"
  local home_dir quoted_path quoted_home quoted_script return_surface quoted_return_surface
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  quoted_script="$(shell_quote "$(script_dir)/vibemud-claude.sh")"
  return_surface="${VIBEMUD_RETURN_CMUX_SURFACE:-${CMUX_SURFACE_ID:-}}"
  quoted_return_surface="$(shell_quote "$return_surface")"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
export VIBEMUD_CONTEXT_MODE=unsafe
export VIBEMUD_SELECTOR_PANE=1
export VIBEMUD_RETURN_CMUX_SURFACE=$quoted_return_surface
printf '\\033]2;VibeMUD Quests\\033\\\\'
exec bash $quoted_script quest-select
LAUNCHER
  chmod +x "$launcher"
}

write_tmux_quest_launcher() {
  local launcher="$1" return_pane="${2:-}"
  local home_dir quoted_path quoted_home quoted_script quoted_return_pane quoted_return_client
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$(dirname "$launcher")"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  quoted_script="$(shell_quote "$(script_dir)/vibemud-claude.sh")"
  quoted_return_pane="$(shell_quote "$return_pane")"
  quoted_return_client="$(shell_quote "$(current_tmux_client)")"
  cat > "$launcher" <<LAUNCHER
#!/usr/bin/env bash
set -euo pipefail
export PATH=$quoted_path
export VIBEMUD_HOME=$quoted_home
export VIBEMUD_CONTEXT_MODE=unsafe
export VIBEMUD_SELECTOR_PANE=1
export VIBEMUD_RETURN_TMUX_PANE=$quoted_return_pane
export VIBEMUD_RETURN_TMUX_CLIENT=$quoted_return_client
printf '\\033]2;VibeMUD Quests\\033\\\\'
exec bash $quoted_script quest-select
LAUNCHER
  chmod +x "$launcher"
}

focus_cmux_surface() {
  local cli="$1"
  local surface="$2"
  [[ -n "$surface" ]] || return 1
  "$cli" focus-panel --panel "$surface" >/dev/null 2>&1 || return 1
  "$cli" set-app-focus active >/dev/null 2>&1 || true
}

cmux_send_shell_command() {
  local cli="$1"
  local surface="$2"
  local command="$3"
  [[ -n "$surface" && -n "$command" ]] || return 1

  # A fresh cmux pane can start the user's interactive zsh first. If oh-my-zsh
  # shows its update prompt, sending "exec ..." immediately lets that prompt
  # consume the first byte ("e"), leaving "xec ..." at the shell prompt. Give
  # shell startup a moment, cancel any interactive prompt, clear the current
  # line, then send the full command.
  sleep 0.45
  "$cli" send-key --surface "$surface" ctrl+c >/dev/null 2>&1 || true
  sleep 0.20
  "$cli" send-key --surface "$surface" ctrl+c >/dev/null 2>&1 || true
  "$cli" send-key --surface "$surface" ctrl+u >/dev/null 2>&1 || true
  "$cli" send --surface "$surface" "${command}\\n" >/dev/null 2>&1 || {
    "$cli" send --surface "$surface" "$command" >/dev/null 2>&1 || return 1
    "$cli" send-key --surface "$surface" enter >/dev/null 2>&1 || return 1
  }
}

cmux_respawn_or_send() {
  local cli="$1"
  local surface="$2"
  local command="$3"
  local current_surface
  current_surface="$(cmux_current_surface || true)"
  if cmux_same_surface "$surface" "$current_surface"; then
    echo "Refusing to run VibeMUD HUD command on the current cmux surface ($surface)." >&2
    return 1
  fi
  # Do not use cmux respawn-pane here. A bad or stale surface ref can respawn
  # the user's active coding pane, blanking their terminal. Sending into the
  # freshly-created pane is slower but non-destructive.
  cmux_send_shell_command "$cli" "$surface" "$command"
}

schedule_cmux_focus() {
  local surface="${1:-}" cli
  [[ -n "$surface" ]] || return 0
  cli="$(cmux_cli)"
  [[ -n "$cli" ]] || return 0
  (
    sleep 0.05
    focus_cmux_surface "$cli" "$surface" >/dev/null 2>&1 || true
    sleep 0.25
    focus_cmux_surface "$cli" "$surface" >/dev/null 2>&1 || true
    sleep 0.65
    focus_cmux_surface "$cli" "$surface" >/dev/null 2>&1 || true
  ) >/dev/null 2>&1 &
  track_background_helper "$!"
}

schedule_cmux_close() {
  local surface="${1:-}" cli
  [[ -n "$surface" ]] || return 0
  cli="$(cmux_cli)"
  [[ -n "$cli" ]] || return 0
  (
    sleep 0.25
    "$cli" close-surface --surface "$surface" >/dev/null 2>&1 || true
  ) >/dev/null 2>&1 &
  track_background_helper "$!"
}

open_cmux_panel() {
  is_cmux_session || return 1

  local cli home_dir state_file existing launcher panel_cmd output surface original_surface
  cli="$(cmux_cli)"
  [[ -n "$cli" ]] || return 1

  home_dir="$(vibemud_home_dir)"
  mkdir -p "$home_dir"
  state_file="$(cmux_panel_state_file)"
  existing="$(cat "$state_file" 2>/dev/null || true)"
  if cmux_surface_alive "$cli" "$existing"; then
    if cmux_surface_has_hud "$cli" "$existing"; then
      echo "VibeMUD HUD is already running in cmux surface ${existing}."
      return 0
    fi
    # The state file may be stale or polluted. Never close a surface unless it
    # is positively identified as a VibeMUD HUD; otherwise this can blank the
    # user's current coding pane.
    rm -f "$state_file"
  fi

  launcher="$(cmux_panel_launcher)"
  write_cmux_panel_launcher "$launcher" || return 1
  panel_cmd="$(cmux_exec_command "$launcher")"
  original_surface="$(cmux_current_surface || true)"

  output="$("$cli" --id-format both new-pane --direction right 2>&1)" || {
    echo "$output" >&2
    return 1
  }
  surface="$(printf '%s\n' "$output" | extract_cmux_surface_ref)"
  if [[ -z "$surface" ]]; then
    surface="$(printf '%s\n' "$output" | grep -Eo '[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}' | tail -1)"
  fi
  if [[ -z "$surface" ]]; then
    echo "Could not identify the new cmux HUD surface from: $output" >&2
    return 1
  fi

  cmux_respawn_or_send "$cli" "$surface" "$panel_cmd" || return 1
  sleep 0.5
  if ! cmux_surface_has_hud "$cli" "$surface"; then
    "$cli" send-key --surface "$surface" enter >/dev/null 2>&1 || true
  fi
  "$cli" rename-tab --surface "$surface" "VibeMUD HUD" >/dev/null 2>&1 || true
  if [[ -n "$original_surface" ]]; then
    "$cli" focus-panel --panel "$original_surface" >/dev/null 2>&1 || true
  fi
  printf '%s\n' "$surface" > "$state_file"
  echo "VibeMUD HUD opened in a right-side cmux pane (${surface})."
  echo "Your Claude Code terminal remains usable for coding."
  return 0
}

open_cmux_selector_surface() {
  is_cmux_session || return 1

  local selector="$1" title="$2"
  local cli home_dir state_file existing launcher selector_cmd output surface
  cli="$(cmux_cli)"
  [[ -n "$cli" ]] || return 1

  home_dir="$(vibemud_home_dir)"
  mkdir -p "$home_dir"
  state_file="$(cmux_panel_state_file)"
  existing="$(cat "$state_file" 2>/dev/null || true)"

  case "$selector" in
    map)
      launcher="$(cmux_selector_launcher)"
      write_cmux_selector_launcher "$launcher"
      ;;
    stats)
      launcher="$(cmux_stats_launcher)"
      write_cmux_stats_launcher "$launcher"
      ;;
    settings)
      launcher="$(cmux_settings_launcher)"
      write_cmux_settings_launcher "$launcher"
      ;;
    quest)
      launcher="$(cmux_quest_launcher)"
      write_cmux_quest_launcher "$launcher"
      ;;
    *) return 1 ;;
  esac
  selector_cmd="$(cmux_exec_command "$launcher")"

  if cmux_surface_alive "$cli" "$existing"; then
    if cmux_surface_has_hud "$cli" "$existing"; then
      cmux_send_shell_command "$cli" "$existing" "$selector_cmd" || return 1
      printf '%s\n' "$existing" > "$state_file"
      sleep 0.2
      focus_cmux_surface "$cli" "$existing" || true
      "$cli" rename-tab --surface "$existing" "$title" >/dev/null 2>&1 || true
      "$cli" trigger-flash --surface "$existing" >/dev/null 2>&1 || true
      return 0
    fi
    # Stale state may point at a non-VibeMUD surface. Do not close it; just
    # ignore the stale ref and create a fresh selector surface below.
    rm -f "$state_file"
  fi

  output="$("$cli" --id-format both new-pane --direction right 2>&1)" || {
    echo "$output" >&2
    return 1
  }
  surface="$(printf '%s\n' "$output" | extract_cmux_surface_ref)"
  if [[ -z "$surface" ]]; then
    surface="$(printf '%s\n' "$output" | grep -Eo '[0-9A-Fa-f]{8}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{4}-[0-9A-Fa-f]{12}' | tail -1)"
  fi
  if [[ -z "$surface" ]]; then
    echo "Could not identify the new cmux ${selector} selector surface from: $output" >&2
    return 1
  fi

  cmux_respawn_or_send "$cli" "$surface" "$selector_cmd" || return 1
  printf '%s\n' "$surface" > "$state_file"
  sleep 0.2
  focus_cmux_surface "$cli" "$surface" || true
  "$cli" rename-tab --surface "$surface" "$title" >/dev/null 2>&1 || true
  "$cli" trigger-flash --surface "$surface" >/dev/null 2>&1 || true
  return 0
}

open_cmux_map_selector_pane() {
  open_cmux_selector_surface map "VibeMUD Map"
}

open_cmux_stats_selector_pane() {
  open_cmux_selector_surface stats "VibeMUD Character"
}

open_cmux_settings_selector_pane() {
  open_cmux_selector_surface settings "VibeMUD Settings"
}

open_cmux_quest_selector_pane() {
  open_cmux_selector_surface quest "VibeMUD Quests"
}

close_cmux_panel() {
  local cli state_file surface quest_state_file quest_surface
  is_cmux_session || return 0
  cli="$(cmux_cli)"
  [[ -n "$cli" ]] || return 0
  state_file="$(cmux_panel_state_file)"
  surface="$(cat "$state_file" 2>/dev/null || true)"
  close_cmux_surface_if_vibemud "$cli" "$surface"
  rm -f "$state_file"
  quest_state_file="$(cmux_quest_state_file)"
  quest_surface="$(cat "$quest_state_file" 2>/dev/null || true)"
  close_cmux_surface_if_vibemud "$cli" "$quest_surface"
  rm -f "$quest_state_file"
}

pane_exists() {
  local pane_id="$1"
  [[ -n "$pane_id" ]] && tmux list-panes -a -F '#{pane_id}' 2>/dev/null | grep -Fxq "$pane_id"
}

tmux_context_available() {
  [[ -n "${TMUX:-}${TMUX_PANE:-}${VIBEMUD_TMUX_PANE:-}" ]]
}

current_tmux_pane() {
  local pane_id
  tmux_context_available || return 1

  pane_id="${TMUX_PANE:-${VIBEMUD_TMUX_PANE:-}}"
  if pane_exists "$pane_id"; then
    printf '%s\n' "$pane_id"
    return 0
  fi

  pane_id="$(tmux display-message -p '#{pane_id}' 2>/dev/null || true)"
  if pane_exists "$pane_id"; then
    printf '%s\n' "$pane_id"
    return 0
  fi

  return 1
}

current_tmux_client() {
  tmux display-message -p '#{client_name}' 2>/dev/null || true
}

discover_tmux_hud_pane() {
  local return_pane="${1:-}" return_window pane_id pane_window pane_command pane_title
  return_window="$(tmux display-message -p -t "$return_pane" '#{session_name}:#{window_index}' 2>/dev/null || true)"
  while IFS=$'\t' read -r pane_id pane_window pane_command pane_title; do
    [[ -n "$pane_id" && "$pane_id" != "$return_pane" ]] || continue
    [[ -z "$return_window" || "$pane_window" == "$return_window" ]] || continue
    if [[ "$pane_command" == "vibemud" || "$pane_command" == "vibemud-hud" || "$pane_title" == *"VibeMUD"* ]]; then
      printf '%s\n' "$pane_id"
      return 0
    fi
  done < <(tmux list-panes -a -F '#{pane_id}	#{session_name}:#{window_index}	#{pane_current_command}	#{pane_title}' 2>/dev/null || true)

  while IFS=$'\t' read -r pane_id pane_window pane_command pane_title; do
    [[ -n "$pane_id" && "$pane_id" != "$return_pane" ]] || continue
    [[ -z "$return_window" || "$pane_window" == "$return_window" ]] || continue
    if tmux capture-pane -p -t "$pane_id" -S -12 2>/dev/null | grep -Eq 'VibeMUD|AUTO-HUNT|GAME / AUTO-HUNT|일일 퀘스트|DAILY QUESTS'; then
      printf '%s\n' "$pane_id"
      return 0
    fi
  done < <(tmux list-panes -a -F '#{pane_id}	#{session_name}:#{window_index}	#{pane_current_command}	#{pane_title}' 2>/dev/null || true)

  return 1
}

tmux_pane_has_vibemud() {
  local pane_id="${1:-}" metadata screen
  pane_exists "$pane_id" || return 1
  metadata="$(tmux display-message -p -t "$pane_id" '#{pane_current_command}	#{pane_title}' 2>/dev/null || true)"
  if printf '%s\n' "$metadata" | grep -Eq '(^|	)(vibemud|vibemud-hud|vibemud-runtime)($|	)|VibeMUD|AUTO-HUNT'; then
    return 0
  fi
  screen="$(tmux capture-pane -p -t "$pane_id" -S -16 2>/dev/null || true)"
  printf '%s\n' "$screen" | grep -Eq 'VibeMUD|AUTO-HUNT|GAME / AUTO-HUNT|상세 능력치|자동 사냥|DAILY QUESTS|일일 퀘스트'
}

kill_tmux_pane_if_vibemud() {
  local pane_id="${1:-}" current
  pane_exists "$pane_id" || return 0
  current="$(current_tmux_pane || true)"
  [[ -n "$current" && "$pane_id" == "$current" ]] && return 0
  if tmux_pane_has_vibemud "$pane_id"; then
    tmux kill-pane -t "$pane_id" >/dev/null 2>&1 || true
  fi
}

applescript_quote() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\r'/}"
  value="${value//$'\n'/\\n}"
  printf '"%s"' "$value"
}

ghostty_terminal_exists() {
  local terminal_id="${1:-}" quoted_terminal_id
  [[ -n "$terminal_id" ]] || return 1
  ghostty_applescript_available || return 1
  quoted_terminal_id="$(applescript_quote "$terminal_id")"
  osascript <<APPLESCRIPT >/dev/null 2>&1
set targetId to $quoted_terminal_id
tell application "Ghostty"
  repeat with term in terminals
    if ((id of term) as text) is targetId then
      return "yes"
    end if
  end repeat
end tell
error "not found"
APPLESCRIPT
}

ghostty_terminal_has_hud() {
  local terminal_id="${1:-}" quoted_terminal_id attempt title
  [[ -n "$terminal_id" ]] || return 1
  ghostty_applescript_available || return 1
  quoted_terminal_id="$(applescript_quote "$terminal_id")"
  for attempt in 1 2 3; do
    title="$(osascript <<APPLESCRIPT 2>/dev/null || true
set targetId to $quoted_terminal_id
tell application "Ghostty"
  repeat with term in terminals
    if ((id of term) as text) is targetId then
      return ((name of term) as text)
    end if
  end repeat
end tell
return ""
APPLESCRIPT
)"
    if printf '%s\n' "$title" | grep -Eq 'VibeMUD|AUTO-HUNT'; then
      return 0
    fi
    sleep 0.35
  done
  return 1
}

ghostty_focused_terminal_id() {
  ghostty_applescript_available || return 1
  osascript <<'APPLESCRIPT' 2>/dev/null | tail -1 | tr -d '\r'
tell application "Ghostty"
  return ((id of focused terminal of selected tab of front window) as text)
end tell
APPLESCRIPT
}

focus_ghostty_terminal() {
  local terminal_id="${1:-}" quoted_terminal_id
  [[ -n "$terminal_id" ]] || return 1
  ghostty_applescript_available || return 1
  quoted_terminal_id="$(applescript_quote "$terminal_id")"
  osascript <<APPLESCRIPT >/dev/null 2>&1
set targetId to $quoted_terminal_id
tell application "Ghostty"
  repeat with term in terminals
    if ((id of term) as text) is targetId then
      focus term
      return "ok"
    end if
  end repeat
end tell
error "not found"
APPLESCRIPT
}

schedule_ghostty_focus() {
  local terminal_id="${1:-}"
  [[ -n "$terminal_id" ]] || return 0
  (
    sleep 0.10
    focus_ghostty_terminal "$terminal_id" >/dev/null 2>&1 || true
    sleep 0.35
    focus_ghostty_terminal "$terminal_id" >/dev/null 2>&1 || true
    sleep 0.75
    focus_ghostty_terminal "$terminal_id" >/dev/null 2>&1 || true
  ) >/dev/null 2>&1 &
  track_background_helper "$!"
}

ghostty_send_shell_command() {
  local terminal_id="$1"
  local command="$2"
  local quoted_terminal_id quoted_command
  [[ -n "$terminal_id" && -n "$command" ]] || return 1
  ghostty_applescript_available || return 1
  quoted_terminal_id="$(applescript_quote "$terminal_id")"
  quoted_command="$(applescript_quote "$command")"
  osascript <<APPLESCRIPT >/dev/null 2>&1
set targetId to $quoted_terminal_id
tell application "Ghostty"
  repeat with term in terminals
    if ((id of term) as text) is targetId then
      focus term
      delay 0.05
      send key "c" modifiers "control" to term
      delay 0.20
      send key "u" modifiers "control" to term
      input text $quoted_command to term
      send key "enter" to term
      return "ok"
    end if
  end repeat
end tell
error "not found"
APPLESCRIPT
}

ensure_ghostty_panel_terminal() {
  is_ghostty_session || return 1
  ghostty_applescript_available || return 1
  local state_file terminal_id
  state_file="$(ghostty_panel_state_file)"
  terminal_id="$(cat "$state_file" 2>/dev/null || true)"
  if ghostty_terminal_exists "$terminal_id"; then
    printf '%s\n' "$terminal_id"
    return 0
  fi
  open_ghostty_panel >/dev/null 2>&1 || return 1
  terminal_id="$(cat "$state_file" 2>/dev/null || true)"
  ghostty_terminal_exists "$terminal_id" || return 1
  printf '%s\n' "$terminal_id"
}

open_ghostty_selector_pane() {
  popup_pane_enabled || return 1
  is_ghostty_session || return 1
  ghostty_applescript_available || return 1

  local selector="$1"
  local title="$2"
  local panel_terminal return_terminal launcher selector_cmd
  return_terminal="$(ghostty_focused_terminal_id || true)"
  panel_terminal="$(ensure_ghostty_panel_terminal)" || return 1
  [[ -n "$return_terminal" && "$return_terminal" != "$panel_terminal" ]] || {
    return_terminal="$(ghostty_focused_terminal_id || true)"
  }
  launcher="$(ghostty_selector_launcher "$selector")"
  write_ghostty_selector_launcher "$launcher" "$selector" "$title" "$return_terminal"
  selector_cmd="$(shell_quote "$launcher")"
  ghostty_send_shell_command "$panel_terminal" "$selector_cmd" || return 1
  schedule_ghostty_focus "$panel_terminal"
  printf '%s\n' "$panel_terminal" > "$(ghostty_panel_state_file)"
  return 0
}

open_ghostty_panel() {
  is_ghostty_session || return 1
  ghostty_applescript_available || return 1

  local home_dir state_file existing launcher shell_command quoted_launcher terminal_id output
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$home_dir"
  state_file="$(ghostty_panel_state_file)"
  existing="$(cat "$state_file" 2>/dev/null || true)"
  if ghostty_terminal_exists "$existing"; then
    if ghostty_terminal_has_hud "$existing"; then
      echo "VibeMUD HUD is already running in Ghostty terminal ${existing}."
      return 0
    fi
  fi
  rm -f "$state_file"

  launcher="$(ghostty_panel_launcher)"
  write_ghostty_panel_launcher "$launcher"
  shell_command="$(shell_quote "$launcher")"
  quoted_launcher="$(applescript_quote "$shell_command")"

  output="$(osascript <<APPLESCRIPT 2>&1
tell application "Ghostty"
  activate
  set oldTerm to focused terminal of selected tab of front window
  set newTerm to split oldTerm direction right
  delay 0.35
  input text $quoted_launcher to newTerm
  send key "enter" to newTerm
  delay 0.05
  focus oldTerm
  return ((id of newTerm) as text)
end tell
APPLESCRIPT
)" || {
    echo "$output" >&2
    return 1
  }
  terminal_id="$(printf '%s\n' "$output" | tail -1 | tr -d '\r')"
  [[ -n "$terminal_id" ]] || return 1
  printf '%s\n' "$terminal_id" > "$state_file"
  echo "VibeMUD HUD opened in a right-side Ghostty split (${terminal_id})."
  echo "Your Claude Code terminal remains usable for coding."
  return 0
}

close_ghostty_panel() {
  is_ghostty_session || return 0
  ghostty_applescript_available || return 0
  local state_file terminal_id quoted_terminal_id
  state_file="$(ghostty_panel_state_file)"
  terminal_id="$(cat "$state_file" 2>/dev/null || true)"
  if [[ -n "$terminal_id" ]] && ghostty_terminal_has_hud "$terminal_id"; then
    quoted_terminal_id="$(applescript_quote "$terminal_id")"
    osascript <<APPLESCRIPT >/dev/null 2>&1 || true
set targetId to $quoted_terminal_id
tell application "Ghostty"
  repeat with term in terminals
    if ((id of term) as text) is targetId then
      close term
      exit repeat
    end if
  end repeat
end tell
APPLESCRIPT
  fi
  rm -f "$state_file"
}

open_tmux_panel() {
  if ! command -v tmux >/dev/null 2>&1; then
    return 1
  fi
  tmux_context_available || return 1
  local home_dir state_file existing panel_cmd pane_id quoted_path quoted_home vibemud_bin quoted_vibemud lines
  local target_pane split_target=()
  target_pane="$(current_tmux_pane || true)"
  if [[ -n "$target_pane" ]]; then
    split_target=(-t "$target_pane")
  elif [[ -z "${TMUX:-}" ]]; then
    return 1
  fi
  home_dir="$(vibemud_home_dir)"
  mkdir -p "$home_dir"
  state_file="$(panel_state_file)"
  existing="$(cat "$state_file" 2>/dev/null || true)"
  if pane_exists "$existing"; then
    if tmux_pane_has_vibemud "$existing"; then
      tmux display-message "VibeMUD HUD panel already running in $existing"
      return 0
    fi
    rm -f "$state_file"
  fi
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  vibemud_bin="$(resolve_binary vibemud)" || return 1
  [[ -n "$vibemud_bin" ]] || return 1
  quoted_vibemud="$(shell_quote "$vibemud_bin")"
  lines="$(message_printline)"
  panel_cmd="export PATH=$quoted_path; export VIBEMUD_HOME=$quoted_home; printf '\033]2;VibeMUD HUD\033\\'; echo 'Starting VibeMUD HUD...'; $quoted_vibemud intro --if-needed || true; exec $quoted_vibemud hud --panel --refresh 1 --log-lines ${lines}"
  pane_id="$(tmux split-window "${split_target[@]}" -h -p 40 -d -P -F '#{pane_id}' "$panel_cmd")"
  printf '%s\n' "$pane_id" > "$state_file"
  tmux display-message "VibeMUD HUD panel opened in $pane_id (right 40%)"
  return 0
}

close_tmux_panel() {
  if ! command -v tmux >/dev/null 2>&1; then
    return 0
  fi
  local state_file pane_id quest_state_file quest_pane_id
  state_file="$(panel_state_file)"
  pane_id="$(cat "$state_file" 2>/dev/null || true)"
  kill_tmux_pane_if_vibemud "$pane_id"
  rm -f "$state_file"
  quest_state_file="$(quest_pane_state_file)"
  quest_pane_id="$(cat "$quest_state_file" 2>/dev/null || true)"
  kill_tmux_pane_if_vibemud "$quest_pane_id"
  rm -f "$quest_state_file"
}

open_best_panel() {
  if is_cmux_session && [[ -n "$(cmux_cli)" ]]; then
    open_cmux_panel && return 0
  fi
  open_tmux_panel && return 0
  if is_cmux_session; then
    open_cmux_panel && return 0
  fi
  if is_ghostty_session; then
    open_ghostty_panel && return 0
  fi
  return 1
}

start_runtime_if_needed() {
  local status
  status="$(run_vibemud session status 2>/dev/null || true)"
  if [[ "$status" != "running" ]]; then
    run_vibemud session start --background
  fi
}

target_is_dungeon() {
  case "${1:-}" in
    goblin-den|crystal-cave|lich-tomb|cyclops-forge|medusa-temple|titan-vault|고블린소굴|고블린|고블굴|고블|수정동굴|수정굴|수정|리치무덤|리치묘|리치|키클롭스대장간|키클롭스|대장간|메두사신전|메두사|신전|티탄금고|티탄|금고|던전)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

map_alias_arg() {
  case "${1:-}" in
    m|map|menu|지도|메뉴|던전|지역) return 0 ;;
    *) return 1 ;;
  esac
}

only_map_alias_args() {
  [[ "$#" -gt 0 ]] || return 0
  local arg
  for arg in "$@"; do
    map_alias_arg "$arg" || return 1
  done
  return 0
}

map_continent=arcadia
map_labels=()
map_ids=()
map_kinds=()

map_load_continent() {
  map_continent="${1:-arcadia}"
  if [[ "$map_continent" == "atlas" ]]; then
    map_labels=(
      "흑요 해안       Lv20 위험 치명"
      "티탄 초원       Lv24 위험 치명"
      "예언자 유적     Lv28 위험 치명"
      "스틱스 늪       Lv32 위험 치명"
      "올림포스 관문   Lv36 위험 치명"
      "키클롭스 대장간 Lv22 던전"
      "메두사 신전     Lv30 던전"
      "티탄 금고       Lv38 던전"
      "아르카디아 대륙으로 이동"
    )
    map_ids=(obsidian-coast titan-steppe oracle-ruins styx-marsh olympus-gate cyclops-forge medusa-temple titan-vault arcadia)
    map_kinds=(area area area area area dungeon dungeon dungeon continent)
  else
    map_labels=(
  "훈련장        Lv1  안전"
  "숲 가장자리   Lv3  위험 낮음"
  "낡은 광산     Lv6  위험 보통"
  "안개 늪       Lv10 위험 높음"
  "무너진 요새   Lv15 위험 치명"
  "고블린 소굴   Lv5  던전"
  "수정 동굴     Lv10 던전"
  "리치 무덤     Lv16 던전"
  "아틀라스 대륙으로 이동"
    )
    map_ids=(training-field forest-edge old-mine misty-swamp fallen-fortress goblin-den crystal-cave lich-tomb atlas)
    map_kinds=(area area area area area dungeon dungeon dungeon continent)
  fi
}

map_load_continent arcadia

map_choice_count() {
  printf '%s\n' "${#map_ids[@]}"
}

render_selector_world_map() {
  if [[ "$map_continent" == "atlas" ]]; then
    cat <<'MAP'
아틀라스 대륙
        [대장간]--[흑요]--[초원]
                   |       |
                [유적]--[메두사]
                   |
              [스틱스]--[관문]--[금고]

        ↓ 아르카디아 대륙으로 이동
MAP
  else
    cat <<'MAP'
아르카디아 대륙
                 [마을]
                   |
                [훈련]
                   |
        [고블굴]--[숲길]--[광산]--[수정굴]
                   |
                [늪지]
                   |
                [요새]--[리치묘]

        ↓ 아틀라스 대륙으로 이동
MAP
  fi
}

render_map_selector() {
  local selected="$1" i marker
  printf '\033[2J\033[H'
  echo "VibeMUD 지도 선택"
  echo "↑/↓ 또는 j/k 이동 · Enter 선택 · 1-$(map_choice_count) 직접 선택 · q 취소"
  echo "----------------------------------------"
  render_selector_world_map
  echo "----------------------------------------"
  echo
  for i in "${!map_ids[@]}"; do
    marker="  "
    [[ "$i" -eq "$selected" ]] && marker="▶ "
    printf '%s%d. %-30s [%s]\n' "$marker" "$((i + 1))" "${map_labels[$i]}" "${map_ids[$i]}"
  done
}

run_map_target() {
  local index="$1" id kind
  id="${map_ids[$index]}"
  kind="${map_kinds[$index]}"
  if [[ "$kind" == "continent" ]]; then
    map_load_continent "$id"
    return 2
  fi
  if [[ "$kind" == "dungeon" ]]; then
    run_mudctl dungeon enter "$id" --auto-start >/dev/null
  else
    run_mudctl hunt start --area "$id" --auto-start >/dev/null
  fi
  run_mudctl stats close >/dev/null 2>&1 || true
  if [[ "${VIBEMUD_SELECTOR_PANE:-}" == "1" ]]; then
    printf '\n선택 완료: %s\n' "${map_labels[$index]}"
    sleep 0.4
    exec_selector_hud
  fi
  open_best_panel >/dev/null 2>&1 || true
  printf '\n선택 완료: %s\n' "${map_labels[$index]}"
  sleep 0.6
}

stats_slot_ids=(weapon subweapon armor_top armor_bottom trinket boots pet special inventory close)
stats_menu_items=(unequip tooltip close)
inventory_menu_items=(equip enhance sell lock close)
equip_slot_menu_items=(weapon subweapon close)

stats_slot_count() {
  printf '%s\n' "${#stats_slot_ids[@]}"
}

stats_slots_cache=""
inventory_items_cache=""
equipment_stats_cache=""
stats_cache_valid=0

stats_refresh_cache() {
  stats_slots_cache="$(run_mudctl equipment slots --raw 2>/dev/null || true)"
  inventory_items_cache="$(run_mudctl equipment inventory 2>/dev/null || true)"
  equipment_stats_cache="$(run_mudctl equipment stats --raw 2>/dev/null || true)"
  stats_cache_valid=1
}

stats_slots_output() {
  [[ "$stats_cache_valid" == "1" ]] || stats_refresh_cache
  printf '%s\n' "$stats_slots_cache"
}

inventory_items_output() {
  [[ "$stats_cache_valid" == "1" ]] || stats_refresh_cache
  printf '%s\n' "$inventory_items_cache"
}

equipment_stats_output() {
  [[ "$stats_cache_valid" == "1" ]] || stats_refresh_cache
  printf '%s\n' "$equipment_stats_cache"
}

stats_slots_output_uncached() {
  run_mudctl equipment slots --raw 2>/dev/null || true
}

inventory_items_output_uncached() {
  run_mudctl equipment inventory 2>/dev/null || true
}

equipment_stats_output_uncached() {
  run_mudctl equipment stats --raw 2>/dev/null || true
}

stats_slot_id_at() {
  local index="$1"
  printf '%s\n' "${stats_slot_ids[$index]}"
}

stats_slot_label_at() {
  local wanted="$1" line slot label status summary
  while IFS='|' read -r slot label status summary; do
    [[ "$slot" == "$wanted" ]] || continue
    printf '%s\n' "${label:-$wanted}"
    return 0
  done < <(stats_slots_output)
  printf '%s\n' "$wanted"
}

stats_slot_status_at() {
  local wanted="$1" line slot label status summary
  while IFS='|' read -r slot label status summary; do
    [[ "$slot" == "$wanted" ]] || continue
    printf '%s\n' "${status:-empty}"
    return 0
  done < <(stats_slots_output)
  printf 'empty\n'
}

inventory_item_count() {
  local count=0 id label kind summary locked
  while IFS='|' read -r id label kind summary locked; do
    [[ "$id" == "empty" || "$id" == "close" || "$kind" == "action" || -z "$id" ]] && continue
    count=$((count + 1))
  done < <(inventory_items_output)
  printf '%s\n' "$count"
}

inventory_row_id_at() {
  local index="$1" current=0 id label kind summary locked
  while IFS='|' read -r id label kind summary locked; do
    [[ -z "$id" ]] && continue
    if [[ "$current" -eq "$index" ]]; then
      printf '%s\n' "$id"
      return 0
    fi
    current=$((current + 1))
  done < <(inventory_items_output)
  printf 'close\n'
}

inventory_row_kind_at() {
  local wanted="$1" id label kind summary locked
  while IFS='|' read -r id label kind summary locked; do
    [[ "$id" == "$wanted" ]] || continue
    printf '%s\n' "${kind:-empty}"
    return 0
  done < <(inventory_items_output)
  printf 'empty\n'
}

inventory_row_locked_at() {
  local wanted="$1" id label kind summary locked
  while IFS='|' read -r id label kind summary locked; do
    [[ "$id" == "$wanted" ]] || continue
    printf '%s\n' "${locked:-unlocked}"
    return 0
  done < <(inventory_items_output)
  printf 'unlocked\n'
}

inventory_row_label_at() {
  local index="$1" current=0 id label kind summary locked
  while IFS='|' read -r id label kind summary locked; do
    [[ -z "$id" ]] && continue
    if [[ "$current" -eq "$index" ]]; then
      printf '%s\n' "${label:-$id}"
      return 0
    fi
    current=$((current + 1))
  done < <(inventory_items_output)
  printf '소지품\n'
}

inventory_row_count() {
  local count=0 id label kind summary locked
  while IFS='|' read -r id label kind summary locked; do
    [[ -n "$id" ]] && count=$((count + 1))
  done < <(inventory_items_output)
  [[ "$count" -gt 0 ]] || count=1
  printf '%s\n' "$count"
}

inventory_tooltip_at() {
  local index="$1" item_id kind
  item_id="$(inventory_row_id_at "$index")"
  kind="$(inventory_row_kind_at "$item_id")"
  if [[ "$kind" == "empty" || "$kind" == "action" || "$item_id" == "close" ]]; then
    printf ''
    return 0
  fi
  run_mudctl equipment item-tooltip "$item_id" 2>/dev/null || true
}

pad_display_right() {
  local text="$1" target="$2"
  local width=0 i ch
  for ((i = 0; i < ${#text}; i++)); do
    ch="${text:i:1}"
    case "$ch" in
      [가-힣]|[ㄱ-ㅎ]|[ㅏ-ㅣ]|[一-龯]|[ぁ-ゟ]|[゠-ヿ]) width=$((width + 2)) ;;
      *) width=$((width + 1)) ;;
    esac
  done
  printf '%s%*s' "$text" "$((target - width > 0 ? target - width : 0))" ''
}

print_two_column_row() {
  local marker="$1" label="$2" summary="$3" width="${4:-12}"
  printf '%s' "$marker"
  pad_display_right "$label" "$width"
  printf ' │ %s\n' "$summary"
}

print_stat_pair_row() {
  local left_label="$1" left_value="$2" right_label="$3" right_value="$4"
  printf '  '
  pad_display_right "${left_label}: ${left_value}" 22
  printf ' │ '
  pad_display_right "${right_label}: ${right_value}" 22
  printf '\n'
}

render_stats_selector() {
  local selected="$1" mode="${2:-slots}" menu_selected="${3:-0}" message="${4:-}" tooltip="${5:-}" inventory_selected="${6:-0}"
  local i slot label status summary marker menu_marker item_count id kind locked left_id left_label left_value right_id right_label right_value selected_inventory_id selected_inventory_locked
  printf '\033[2J\033[H'
  echo "VibeMUD 장비창"
  echo "↑/↓ 이동 · Enter 선택 · q 뒤로/닫기"
  echo "----------------------------------------"
  item_count="$(inventory_item_count)"
  printf '요약 정보\n'
  while IFS='|' read -r left_id left_label left_value right_id right_label right_value; do
    [[ -z "$left_id" ]] && continue
    print_stat_pair_row "$left_label" "$left_value" "$right_label" "$right_value"
  done < <(equipment_stats_output)
  echo "----------------------------------------"
  printf '장착 장비\n'
  while IFS='|' read -r slot label status summary; do
    [[ "$slot" == "close" ]] && continue
    marker="  "
    [[ "$mode" != inventory* ]] && {
      for i in "${!stats_slot_ids[@]}"; do
        [[ "${stats_slot_ids[$i]}" == "$slot" && "$i" -eq "$selected" ]] && marker="▶ "
      done
    }
    print_two_column_row "$marker" "$label" "$summary" 12
  done < <(stats_slots_output)
  marker="  "
  [[ "$mode" != inventory* && "${stats_slot_ids[$selected]}" == "inventory" ]] && marker="▶ "
  print_two_column_row "$marker" "소지품창" "획득한 아이템 관리" 12
  marker="  "
  [[ "$mode" != inventory* && "${stats_slot_ids[$selected]}" == "close" ]] && marker="▶ "
  print_two_column_row "$marker" "닫기" "장비창 닫기" 12
  echo "----------------------------------------"
  printf '소지품 %s/20\n' "$item_count"
  local row_index=0
  while IFS='|' read -r id label kind summary locked; do
    marker="  "
    [[ "$mode" == inventory* && "$row_index" -eq "$inventory_selected" ]] && marker="▶ "
    if [[ "$id" == "close" || "$kind" == "action" ]]; then
      print_two_column_row "$marker" "$label" "$summary" 18
    elif [[ "$id" == "empty" ]]; then
      printf '%s%s\n' "$marker" "$summary"
    else
      printf '%s%s\n' "$marker" "$label"
    fi
    row_index=$((row_index + 1))
  done < <(inventory_items_output)
  echo "----------------------------------------"
  if [[ -n "$message" ]]; then
    printf '%s\n' "$message"
  fi
  if [[ -n "$tooltip" ]]; then
    printf '%s\n' "$tooltip"
  fi
  if [[ "$mode" == "menu" ]]; then
    echo
    echo "선택 메뉴"
    for i in "${!stats_menu_items[@]}"; do
      menu_marker="  "
      [[ "$i" -eq "$menu_selected" ]] && menu_marker="▶ "
      case "${stats_menu_items[$i]}" in
        unequip) printf '%s%s\n' "$menu_marker" "해제" ;;
        tooltip) printf '%s%s\n' "$menu_marker" "툴팁보기" ;;
        close) printf '%s%s\n' "$menu_marker" "닫기" ;;
      esac
    done
  elif [[ "$mode" == "inventory-menu" ]]; then
    echo
    echo "선택 메뉴"
    selected_inventory_id="$(inventory_row_id_at "$inventory_selected")"
    selected_inventory_locked="$(inventory_row_locked_at "$selected_inventory_id")"
    for i in "${!inventory_menu_items[@]}"; do
      menu_marker="  "
      [[ "$i" -eq "$menu_selected" ]] && menu_marker="▶ "
      case "${inventory_menu_items[$i]}" in
        equip) printf '%s%s\n' "$menu_marker" "장착" ;;
        enhance) printf '%s%s\n' "$menu_marker" "강화" ;;
        sell) printf '%s%s\n' "$menu_marker" "판매" ;;
        lock)
          if [[ "$selected_inventory_locked" == "locked" ]]; then
            printf '%s%s\n' "$menu_marker" "잠금해제"
          else
            printf '%s%s\n' "$menu_marker" "잠금"
          fi
          ;;
        close) printf '%s%s\n' "$menu_marker" "닫기" ;;
      esac
    done
  elif [[ "$mode" == "equip-slot-menu" ]]; then
    echo
    echo "장착 위치 선택"
    for i in "${!equip_slot_menu_items[@]}"; do
      menu_marker="  "
      [[ "$i" -eq "$menu_selected" ]] && menu_marker="▶ "
      case "${equip_slot_menu_items[$i]}" in
        weapon) printf '%s%s\n' "$menu_marker" "주무기 장착" ;;
        subweapon) printf '%s%s\n' "$menu_marker" "부무기 장착" ;;
        close) printf '%s%s\n' "$menu_marker" "닫기" ;;
      esac
    done
  fi
}

run_stats_command_action() {
  local command="$1" output pattern
  shift || true
  output="$(run_mudctl "$command" "$@" 2>&1 || true)"
  run_vibemud session start --ticks 1 >/dev/null 2>&1 || true
  case "$command" in
    enhance) pattern='강화성공|강화실패|강화 성공|강화 실패|not enough gold|already at max|enhance target|equipped items cannot|cannot be enhanced' ;;
    equip) pattern='Equipped|already equipped|cannot be equipped|not found' ;;
    unequip) pattern='Unequipped|not equipped|not found' ;;
    *) pattern='Sold|판매|Sale skipped|판매 건너뜀|Emptied inventory|No items|No unlocked|Locked|Unlocked|not found|unknown bulk sell rarity' ;;
  esac
  output="$(run_mudctl log --tail 8 2>/dev/null | grep -E "$pattern" | tail -1 || true)"
  if [[ -z "$output" ]]; then
    output="$(run_mudctl queue --tail 1 2>/dev/null | tail -1 || true)"
  fi
  output="$(clean_action_result "$output")"
  if [[ "$command" == "enhance" && -n "$output" ]]; then
    format_enhance_result "$output"
    return 0
  fi
  printf '%s\n' "${output:-처리 결과를 확인하지 못했습니다.}"
}

clean_action_result() {
  local output="$1"
  output="$(printf '%s\n' "$output" | tail -1)"
  output="$(printf '%s\n' "$output" | sed -E 's/^#[0-9]+[[:space:]]+[^[:space:]]+[[:space:]]+명령처리[[:space:]]+//')"
  if [[ "$output" == *" enhance target "* ]]; then
    output="enhance target ${output##* enhance target }"
  elif [[ "$output" == *" equipped items cannot"* ]]; then
    output="equipped items cannot${output##* equipped items cannot}"
  elif [[ "$output" == *" cannot be enhanced"* ]]; then
    output="${output##* processed=* }"
  elif [[ "$output" == *" not enough gold"* ]]; then
    output="not enough gold${output##* not enough gold}"
  elif [[ "$output" == *" already at max"* ]]; then
    output="${output##* processed=* }"
  fi
  printf '%s\n' "$output"
}

format_enhance_result() {
  local output="$1"
  if [[ "$output" == *"강화성공"* || "$output" == *"강화 성공"* ]]; then
    printf '%s\n%s\n' "✨ 강화 성공!" "$output"
  elif [[ "$output" == *"강화실패"* || "$output" == *"강화 실패"* ]]; then
    printf '%s\n%s\n' "💥 강화 실패" "$output"
  elif [[ "$output" == *"not enough gold"* ]]; then
    printf '%s\n%s\n' "💰 골드 부족" "$output"
  elif [[ "$output" == *"already at max"* ]]; then
    printf '%s\n%s\n' "⭐ 최대 강화" "$output"
  elif [[ "$output" == *"equipped items cannot be enhanced"* ]]; then
    printf '%s\n%s\n' "⚠️ 장착 중 강화 불가" "장착 해제 후 소지품창에서 강화해 주세요."
  elif [[ "$output" == *"cannot be enhanced"* ]]; then
    printf '%s\n%s\n' "⚠️ 강화 불가" "선택한 아이템은 강화 가능한 장비가 아닙니다."
  elif [[ "$output" == *"is not equipped equipment"* ]]; then
    printf '%s\n%s\n' "⚠️ 강화 대상 없음" "선택한 슬롯에 장착된 장비를 찾지 못했습니다. 장비창을 새로고침한 뒤 다시 선택해 주세요."
  else
    printf '%s\n%s\n' "🔧 강화 결과" "$output"
  fi
}

render_enhance_animation() {
  local selected="$1" mode="$2" menu_selected="$3" inventory_selected="$4" tooltip="$5" target="$6"
  local frames=(
    "◇ 강화 준비 중"
    "◆ 마력 주입 중"
    "✦ 룬 공명 중"
    "✧ 강화 판정 중"
  )
  local frame
  for frame in "${frames[@]}"; do
    render_stats_selector \
      "$selected" \
      "$mode" \
      "$menu_selected" \
      "$frame"$'\n'"대상: $target" \
      "$tooltip" \
      "$inventory_selected"
    sleep 0.12
  done
}

enhance_animation_target() {
  local fallback="$1" tooltip="${2:-}" first_line
  first_line="$(printf '%s\n' "$tooltip" | sed -n '/[^[:space:]]/{p;q;}')"
  printf '%s\n' "${first_line:-$fallback}"
}

run_stats_enhance() {
  run_stats_command_action enhance "$1"
}

run_inventory_equip() {
  local item_id="$1" slot="${2:-}"
  if [[ -n "$slot" ]]; then
    run_stats_command_action equip "$item_id" --slot "$slot"
  else
    run_stats_command_action equip "$item_id"
  fi
}

run_slot_unequip() {
  run_stats_command_action unequip "$1"
}

run_inventory_sell() {
  run_stats_command_action shop sell "$1"
}

run_inventory_empty() {
  run_stats_command_action equipment empty
}

run_inventory_lock_toggle() {
  local item_id="$1"
  if [[ "$(inventory_row_locked_at "$item_id")" == "locked" ]]; then
    run_stats_command_action equipment unlock "$item_id"
  else
    run_stats_command_action equipment lock "$item_id"
  fi
}

selector_tty_state=""
selector_tty_configured=0

setup_selector_tty() {
  [[ "$selector_tty_configured" == "1" ]] && return 0
  [[ -t 0 || -t 1 ]] || return 0
  { selector_tty_state="$(stty -g < /dev/tty)"; } 2>/dev/null || true
  [[ -n "$selector_tty_state" ]] || return 0
  stty -echo -icanon min 1 time 0 < /dev/tty 2>/dev/null || return 0
  selector_tty_configured=1
  trap restore_selector_tty EXIT INT TERM
}

restore_selector_tty() {
  if [[ "$selector_tty_configured" == "1" && -n "$selector_tty_state" ]]; then
    stty "$selector_tty_state" < /dev/tty 2>/dev/null || true
  fi
  selector_tty_configured=0
}

SELECTOR_KEY=""

read_selector_key() {
  local input_fd="${1:-0}" key more
  if ! IFS= read -rsn1 -u "$input_fd" key; then
    return 1
  fi
  if [[ "$key" == $'\x1b' ]]; then
    while IFS= read -rsn1 -t 1 -u "$input_fd" more; do
      key+="$more"
      case "$more" in
        [A-Za-z~]) break ;;
      esac
      [[ "${#key}" -ge 16 ]] && break
    done
  elif [[ "$key" == "[" || "$key" == "O" ]]; then
    key=$'\x1b'"$key"
    while IFS= read -rsn1 -t 1 -u "$input_fd" more; do
      key+="$more"
      case "$more" in
        [A-Za-z~]) break ;;
      esac
      [[ "${#key}" -ge 16 ]] && break
    done
  fi
  SELECTOR_KEY="$key"
  if [[ -n "${VIBEMUD_SELECTOR_KEY_LOG:-}" ]]; then
    printf '%q\n' "$SELECTOR_KEY" >> "$VIBEMUD_SELECTOR_KEY_LOG" 2>/dev/null || true
  fi
  return 0
}

is_key_up() {
  local key="$1"
  [[ "$key" == "k" || "$key" == "K" || "$key" == $'\x1b[A' || "$key" == $'\x1bOA' || "$key" == $'\x1b['*A || "$key" == $'\x1bO'*A ]]
}

is_key_down() {
  local key="$1"
  [[ "$key" == "j" || "$key" == "J" || "$key" == $'\x1b[B' || "$key" == $'\x1bOB' || "$key" == $'\x1b['*B || "$key" == $'\x1bO'*B ]]
}

close_stats_selector() {
  restore_selector_tty
  run_mudctl stats close >/dev/null 2>&1 || true
  if [[ "${VIBEMUD_SELECTOR_PANE:-}" == "1" ]]; then
    exec_selector_hud
  fi
  return 0
}

run_stats_selector() {
  local selected=0 menu_selected=0 inventory_selected=0 mode="slots" key more count slot status action message="" tooltip="" inv_count item_id kind
  count="$(stats_slot_count)"
  run_mudctl stats close >/dev/null 2>&1 || true
  setup_selector_tty
  stats_refresh_cache
  while true; do
    render_stats_selector "$selected" "$mode" "$menu_selected" "$message" "$tooltip" "$inventory_selected"
    IFS= read -rsn1 key || return 1
    if [[ "$key" == $'\x1b' ]]; then
      IFS= read -rsn1 -t 1 more || more=""
      key+="$more"
      IFS= read -rsn1 -t 1 more || more=""
      key+="$more"
    fi
    if [[ "$mode" == "slots" ]]; then
      case "$key" in
        q|Q)
          close_stats_selector
          return 0
          ;;
        "")
          slot="$(stats_slot_id_at "$selected")"
          if [[ "$slot" == "close" ]]; then
            close_stats_selector
            return 0
          elif [[ "$slot" == "inventory" ]]; then
            mode="inventory"
            inventory_selected=0
            menu_selected=0
            message=""
            tooltip=""
            continue
          fi
          status="$(stats_slot_status_at "$slot")"
          if [[ "$status" == "empty" ]]; then
            message="$(stats_slot_label_at "$slot"): 미착용"
            tooltip=""
          else
            mode="menu"
            menu_selected=0
            tooltip="$(run_mudctl equipment tooltip "$slot" 2>/dev/null || true)"
            message=""
          fi
          ;;
        $'\x1b[A'|$'\x1bOA'|k|K) selected=$(( (selected + count - 1) % count )); message=""; tooltip="" ;;
        $'\x1b[B'|$'\x1bOB'|j|J) selected=$(( (selected + 1) % count )); message=""; tooltip="" ;;
      esac
    elif [[ "$mode" == "menu" ]]; then
      case "$key" in
        q|Q)
          mode="slots"
          message=""
          tooltip=""
          ;;
        "")
          slot="$(stats_slot_id_at "$selected")"
          action="${stats_menu_items[$menu_selected]}"
          case "$action" in
            unequip)
              message="$(run_slot_unequip "$slot")"
              stats_refresh_cache
              tooltip=""
              mode="slots"
              ;;
            tooltip)
              tooltip="$(run_mudctl equipment tooltip "$slot" 2>/dev/null || true)"
              message=""
              ;;
            close)
              mode="slots"
              message=""
              tooltip=""
              ;;
          esac
          ;;
        $'\x1b[A'|$'\x1bOA'|k|K) menu_selected=$(( (menu_selected + 2) % 3 )) ;;
        $'\x1b[B'|$'\x1bOB'|j|J) menu_selected=$(( (menu_selected + 1) % 3 )) ;;
      esac
    elif [[ "$mode" == "inventory" ]]; then
      inv_count="$(inventory_row_count)"
      case "$key" in
        q|Q)
          mode="slots"
          message=""
          tooltip=""
          ;;
        "")
          item_id="$(inventory_row_id_at "$inventory_selected")"
          kind="$(inventory_row_kind_at "$item_id")"
          if [[ "$item_id" == "close" ]]; then
            mode="slots"
            message=""
            tooltip=""
          elif [[ "$item_id" == "empty_inventory" ]]; then
            message="$(run_inventory_empty)"
            stats_refresh_cache
            tooltip=""
            inventory_selected=0
          elif [[ "$kind" == "empty" ]]; then
            message="소지품창이 비어 있습니다."
            tooltip=""
          else
            mode="inventory-menu"
            menu_selected=0
            tooltip="$(run_mudctl equipment item-tooltip "$item_id" 2>/dev/null || true)"
            message=""
          fi
          ;;
        $'\x1b[A'|$'\x1bOA'|k|K) inventory_selected=$(( (inventory_selected + inv_count - 1) % inv_count )); message=""; tooltip="" ;;
        $'\x1b[B'|$'\x1bOB'|j|J) inventory_selected=$(( (inventory_selected + 1) % inv_count )); message=""; tooltip="" ;;
      esac
    elif [[ "$mode" == "inventory-menu" ]]; then
      case "$key" in
        q|Q)
          mode="inventory"
          message=""
          tooltip=""
          ;;
        "")
          item_id="$(inventory_row_id_at "$inventory_selected")"
          action="${inventory_menu_items[$menu_selected]}"
          case "$action" in
            equip)
              kind="$(inventory_row_kind_at "$item_id")"
              if [[ "$kind" == "weapon" || "$kind" == "subweapon" ]]; then
                mode="equip-slot-menu"
                menu_selected=0
                message=""
              else
                message="$(run_inventory_equip "$item_id")"
                stats_refresh_cache
                tooltip=""
                mode="inventory"
              fi
              ;;
            enhance)
              render_enhance_animation \
                "$selected" \
                "$mode" \
                "$menu_selected" \
                "$inventory_selected" \
                "$tooltip" \
                "$(inventory_row_label_at "$inventory_selected")"
              message="$(run_stats_enhance "$item_id")"
              stats_refresh_cache
              tooltip="$(run_mudctl equipment item-tooltip "$item_id" 2>/dev/null || true)"
              ;;
            sell)
              message="$(run_inventory_sell "$item_id")"
              stats_refresh_cache
              tooltip=""
              mode="inventory"
              inventory_selected=0
              ;;
            lock)
              message="$(run_inventory_lock_toggle "$item_id")"
              stats_refresh_cache
              tooltip="$(run_mudctl equipment item-tooltip "$item_id" 2>/dev/null || true)"
              ;;
            close)
              mode="inventory"
              message=""
              tooltip=""
              ;;
          esac
          ;;
        $'\x1b[A'|$'\x1bOA'|k|K) menu_selected=$(( (menu_selected + 4) % 5 )) ;;
        $'\x1b[B'|$'\x1bOB'|j|J) menu_selected=$(( (menu_selected + 1) % 5 )) ;;
      esac
    else
      case "$key" in
        q|Q)
          mode="inventory-menu"
          message=""
          ;;
        "")
          item_id="$(inventory_row_id_at "$inventory_selected")"
          action="${equip_slot_menu_items[$menu_selected]}"
          case "$action" in
            weapon|subweapon)
              message="$(run_inventory_equip "$item_id" "$action")"
              stats_refresh_cache
              tooltip=""
              mode="inventory"
              ;;
            close)
              mode="inventory-menu"
              message=""
              ;;
          esac
          ;;
        $'\x1b[A'|$'\x1bOA'|k|K) menu_selected=$(( (menu_selected + 2) % 3 )) ;;
        $'\x1b[B'|$'\x1bOB'|j|J) menu_selected=$(( (menu_selected + 1) % 3 )) ;;
      esac
    fi
  done
}

quest_rows_output() {
  run_mudctl quest list --raw 2>/dev/null || true
}

quest_row_count() {
  local count=0 id status title progress target reward_kind reward_amount fever
  while IFS='|' read -r id status title progress target reward_kind reward_amount fever; do
    [[ -n "$id" ]] && count=$((count + 1))
  done < <(quest_rows_output)
  [[ "$count" -gt 0 ]] || count=1
  printf '%s\n' "$count"
}

quest_row_id_at() {
  local index="$1" current=0 id status title progress target reward_kind reward_amount fever
  while IFS='|' read -r id status title progress target reward_kind reward_amount fever; do
    [[ -z "$id" ]] && continue
    if [[ "$current" -eq "$index" ]]; then
      printf '%s\n' "$id"
      return 0
    fi
    current=$((current + 1))
  done < <(quest_rows_output)
  printf 'close\n'
}

quest_row_status_at() {
  local wanted="$1" id status title progress target reward_kind reward_amount fever
  while IFS='|' read -r id status title progress target reward_kind reward_amount fever; do
    [[ "$id" == "$wanted" ]] || continue
    printf '%s\n' "${status:-active}"
    return 0
  done < <(quest_rows_output)
  printf 'active\n'
}

quest_status_marker() {
  case "$1" in
    completed) printf '완료' ;;
    claimed) printf '수령' ;;
    action) printf '메뉴' ;;
    *) printf '진행' ;;
  esac
}

quest_reward_text() {
  local kind="$1" amount="$2"
  if [[ "$kind" == "xp" ]]; then
    printf '경험치 +%s' "$amount"
  elif [[ "$kind" == "action" ]]; then
    printf ''
  else
    printf '머니 +%s' "$amount"
  fi
}

render_quest_selector() {
  local selected="$1" message="${2:-}" mode="${3:-list}" menu_selected="${4:-0}" i=0 id status title progress target reward_kind reward_amount fever marker state reward menu_marker
  printf '\033[2J\033[H'
  echo "VibeMUD 일일 퀘스트"
  echo "↑/↓ 이동 · Enter 선택 · q 뒤로/닫기"
  echo "매일 24:00에 5개 자동 갱신 · 보상에는 FEVERTIME이 항상 포함"
  echo "------------------------------------------------------------"
  while IFS='|' read -r id status title progress target reward_kind reward_amount fever; do
    [[ -z "$id" ]] && continue
    marker="  "
    [[ "$i" -eq "$selected" ]] && marker="▶ "
    if [[ "$status" == "action" ]]; then
      printf '%s%s\n' "$marker" "$title"
    else
      state="$(quest_status_marker "$status")"
      reward="$(quest_reward_text "$reward_kind" "$reward_amount")"
      printf '%s[%s] %s\n' "$marker" "$state" "$title"
      printf '    진행 %s/%s · %s · FEVERTIME +%s분\n' "$progress" "$target" "$reward" "$fever"
    fi
    i=$((i + 1))
  done < <(quest_rows_output)
  echo "------------------------------------------------------------"
  if [[ -n "$message" ]]; then
    printf '%s\n' "$message"
  fi
  if [[ "$mode" == "menu" ]]; then
    echo
    echo "선택 메뉴"
    for i in "${!quest_menu_items[@]}"; do
      menu_marker="  "
      [[ "$i" -eq "$menu_selected" ]] && menu_marker="▶ "
      case "${quest_menu_items[$i]}" in
        claim) printf '%s%s\n' "$menu_marker" "보상 받기" ;;
        close) printf '%s%s\n' "$menu_marker" "닫기" ;;
      esac
    done
  fi
  return 0
}

quest_menu_items=(claim close)

run_quest_selector() {
  local selected=0 menu_selected=0 key more count id status action message="" mode="list" input_fd=0 tty_status=1 missed_reads=0
  setup_selector_tty
  if [[ -t 0 ]]; then
    input_fd=0
  elif [[ -t 1 ]]; then
    set +e
    { exec 7</dev/tty; } 2>/dev/null
    tty_status=$?
    set -e
    if [[ "$tty_status" -eq 0 ]]; then
      input_fd=7
    fi
  fi
  count="$(quest_row_count)"
  while true; do
    render_quest_selector "$selected" "$message" "$mode" "$menu_selected"
    if ! read_selector_key "$input_fd"; then
      if [[ "${VIBEMUD_SELECTOR_PANE:-}" == "1" ]]; then
        missed_reads=$((missed_reads + 1))
        if [[ "$missed_reads" -ge 5 ]]; then
          close_quest_selector
          return 1
        fi
        message="입력 연결 대기 중입니다. 연결이 복구되지 않으면 HUD로 돌아갑니다."
        sleep 0.5
        continue
      fi
      return 1
    fi
    key="$SELECTOR_KEY"
    missed_reads=0
    if [[ "$mode" == "menu" ]]; then
      if is_key_up "$key" || is_key_down "$key"; then
        menu_selected=$(( (menu_selected + 1) % 2 ))
        continue
      fi
      case "$key" in
        q|Q)
          mode="list"
          message=""
          ;;
        "")
          id="$(quest_row_id_at "$selected")"
          action="${quest_menu_items[$menu_selected]}"
          case "$action" in
            claim)
              message="$(run_mudctl quest claim "$id" 2>&1 || true)"
              count="$(quest_row_count)"
              mode="list"
              menu_selected=0
              ;;
            close)
              mode="list"
              message=""
              menu_selected=0
              ;;
          esac
          ;;
      esac
    else
      if is_key_up "$key"; then
        selected=$(( (selected + count - 1) % count ))
        message=""
        continue
      elif is_key_down "$key"; then
        selected=$(( (selected + 1) % count ))
        message=""
        continue
      fi
      case "$key" in
        q|Q)
          close_quest_selector
          return 0
          ;;
        "")
          id="$(quest_row_id_at "$selected")"
          status="$(quest_row_status_at "$id")"
          case "$id" in
            close)
              close_quest_selector
              return 0
              ;;
            claim_all)
              message="$(run_mudctl quest claim-all 2>&1 || true)"
              count="$(quest_row_count)"
              selected=0
              ;;
            *)
              if [[ "$status" == "completed" ]]; then
                mode="menu"
                menu_selected=0
                message=""
              elif [[ "$status" == "claimed" ]]; then
                message="이미 보상을 수령했습니다."
              else
                message="아직 완료되지 않았습니다."
              fi
              ;;
          esac
          ;;
      esac
    fi
  done
}

close_quest_selector() {
  restore_selector_tty
  if [[ "${VIBEMUD_SELECTOR_TRANSIENT:-}" == "1" ]]; then
    local quest_surface
    quest_surface="$(cat "$(cmux_quest_state_file)" 2>/dev/null || true)"
    restore_selector_return_focus
    schedule_cmux_close "$quest_surface"
    rm -f "$(quest_pane_state_file)" "$(cmux_quest_state_file)" 2>/dev/null || true
    return 0
  fi
  if [[ "${VIBEMUD_SELECTOR_PANE:-}" == "1" ]]; then
    exec_selector_hud
  fi
  return 0
}

exec_selector_hud() {
  local lines vibemud_bin
  restore_selector_tty
  lines="$(message_printline)"
  vibemud_bin="$(resolve_binary vibemud)" || {
    echo "vibemud binary not found; cannot return selector pane to HUD" >&2
    return 127
  }
  restore_selector_return_focus
  printf '\033]2;VibeMUD HUD\033\\'
  exec "$vibemud_bin" hud --panel --refresh 1 --log-lines "$lines"
}

restore_selector_return_focus() {
  if [[ -n "${VIBEMUD_RETURN_CMUX_SURFACE:-}" ]]; then
    schedule_cmux_focus "$VIBEMUD_RETURN_CMUX_SURFACE"
  fi
  if [[ -n "${VIBEMUD_RETURN_TMUX_PANE:-}" ]]; then
    focus_tmux_pane "$VIBEMUD_RETURN_TMUX_PANE" "${VIBEMUD_RETURN_TMUX_CLIENT:-}" >/dev/null 2>&1 || true
    schedule_tmux_focus "$VIBEMUD_RETURN_TMUX_PANE" "${VIBEMUD_RETURN_TMUX_CLIENT:-}"
  fi
  if [[ -n "${VIBEMUD_RETURN_GHOSTTY_TERMINAL:-}" ]]; then
    focus_ghostty_terminal "$VIBEMUD_RETURN_GHOSTTY_TERMINAL" >/dev/null 2>&1 || true
    schedule_ghostty_focus "$VIBEMUD_RETURN_GHOSTTY_TERMINAL"
  fi
}

run_map_selector() {
  local selected=0 key more count
  setup_selector_tty
  map_load_continent "$map_continent"
  count="$(map_choice_count)"
  run_mudctl map >/dev/null 2>&1 || true
  while true; do
    render_map_selector "$selected"
    IFS= read -rsn1 key || return 1
    if [[ "$key" == $'\x1b' ]]; then
      IFS= read -rsn1 -t 1 more || more=""
      key+="$more"
      IFS= read -rsn1 -t 1 more || more=""
      key+="$more"
    fi
    case "$key" in
      "")
        if run_map_target "$selected"; then
          return 0
        elif [[ "$?" -eq 2 ]]; then
          selected=0
          count="$(map_choice_count)"
          continue
        fi
        return 0
        ;;
      q|Q)
        run_mudctl stats close >/dev/null 2>&1 || true
        printf '\n취소됨\n'
        sleep 0.4
        if [[ "${VIBEMUD_SELECTOR_PANE:-}" == "1" ]]; then
          exec_selector_hud
        fi
        restore_selector_tty
        return 1
        ;;
      $'\x1b[A'|$'\x1bOA'|k|K) selected=$(( (selected + count - 1) % count )) ;;
      $'\x1b[B'|$'\x1bOB'|j|J) selected=$(( (selected + 1) % count )) ;;
      [1-9])
        if [[ "$key" -ge 1 && "$key" -le "$count" ]]; then
          selected=$((key - 1))
          if run_map_target "$selected"; then
            return 0
          elif [[ "$?" -eq 2 ]]; then
            selected=0
            count="$(map_choice_count)"
            continue
          fi
          return 0
        fi
        ;;
    esac
  done
}

open_tmux_selector_pane() {
  popup_pane_enabled || return 1
  command -v tmux >/dev/null 2>&1 || return 1
  local selector="$1" label="$2"
  local script_path home_dir state_file existing quoted_path quoted_home quoted_script quoted_return_pane quoted_return_client selector_cmd target_pane return_client pane_id
  local split_target=()
  target_pane="$(current_tmux_pane || true)"
  if [[ -n "$target_pane" ]]; then
    split_target=(-t "$target_pane")
  elif [[ -z "${TMUX:-}" ]]; then
    return 1
  fi
  return_client="$(current_tmux_client)"
  script_path="$(script_dir)/vibemud-claude.sh"
  home_dir="$(vibemud_home_dir)"
  state_file="$(panel_state_file)"
  existing="$(cat "$state_file" 2>/dev/null || true)"
  quoted_path="$(shell_quote "$PATH")"
  quoted_home="$(shell_quote "$home_dir")"
  quoted_script="$(shell_quote "$script_path")"
  quoted_return_pane="$(shell_quote "$target_pane")"
  quoted_return_client="$(shell_quote "$return_client")"
  selector_cmd="export PATH=$quoted_path; export VIBEMUD_HOME=$quoted_home; export VIBEMUD_CONTEXT_MODE=unsafe; export VIBEMUD_SELECTOR_PANE=1; export VIBEMUD_RETURN_TMUX_PANE=$quoted_return_pane; export VIBEMUD_RETURN_TMUX_CLIENT=$quoted_return_client; bash $quoted_script ${selector}-select"

  if pane_exists "$existing" && tmux_pane_has_vibemud "$existing"; then
    pane_id="$existing"
    tmux respawn-pane -k -t "$pane_id" bash -lc "$selector_cmd" || return 1
  else
    kill_tmux_pane_if_vibemud "$existing"
    rm -f "$state_file"
    pane_id="$(tmux split-window "${split_target[@]}" -h -p 40 -P -F '#{pane_id}' bash -lc "$selector_cmd")" || return 1
  fi

  printf '%s\n' "$pane_id" > "$state_file"
  tmux select-pane -t "$pane_id" >/dev/null 2>&1 || true
  focus_tmux_pane "$pane_id" "$return_client"
  schedule_tmux_focus "$pane_id" "$return_client"
  tmux display-message "VibeMUD ${label} selector focused in $pane_id"
}

open_map_selector_pane() {
  open_tmux_selector_pane map map
}

focus_tmux_pane() {
  local pane_id="$1" client="${2:-}" window_target session_target client_name client_session client_window
  pane_exists "$pane_id" || return 1
  window_target="$(tmux display-message -p -t "$pane_id" '#{session_name}:#{window_index}' 2>/dev/null || true)"
  session_target="$(tmux display-message -p -t "$pane_id" '#{session_name}' 2>/dev/null || true)"
  if [[ -n "$client" && -n "$window_target" ]]; then
    tmux switch-client -c "$client" -t "$window_target" >/dev/null 2>&1 || true
    tmux select-pane -t "$pane_id" >/dev/null 2>&1 || true
    return 0
  fi
  if [[ -n "$window_target" ]]; then
    tmux select-window -t "$window_target" >/dev/null 2>&1 || true
  fi
  tmux select-pane -t "$pane_id" >/dev/null 2>&1 || true

  [[ -n "$window_target" ]] || return 0
  while IFS=$'\t' read -r client_name client_session client_window; do
    [[ -n "$client_name" ]] || continue
    [[ "$client_session:$client_window" == "$window_target" || "$client_session" == "$session_target" ]] || continue
    tmux switch-client -c "$client_name" -t "$window_target" >/dev/null 2>&1 || true
    tmux select-pane -t "$pane_id" >/dev/null 2>&1 || true
  done < <(tmux list-clients -F '#{client_name}	#{session_name}	#{window_index}' 2>/dev/null || true)
}

schedule_tmux_focus() {
  local pane_id="$1" client="${2:-}" quoted_pane quoted_client quoted_script
  quoted_pane="$(shell_quote "$pane_id")"
  quoted_client="$(shell_quote "$client")"
  quoted_script="$(shell_quote "focus_tmux_pane() {
  local pane_id=\"\$1\" client=\"\${2:-}\" window_target session_target client_name client_session client_window
  tmux list-panes -a -F '#{pane_id}' 2>/dev/null | grep -Fxq \"\$pane_id\" || return 1
  window_target=\"\$(tmux display-message -p -t \"\$pane_id\" '#{session_name}:#{window_index}' 2>/dev/null || true)\"
  session_target=\"\$(tmux display-message -p -t \"\$pane_id\" '#{session_name}' 2>/dev/null || true)\"
  if [[ -n \"\$client\" && -n \"\$window_target\" ]]; then
    tmux switch-client -c \"\$client\" -t \"\$window_target\" >/dev/null 2>&1 || true
    tmux select-pane -t \"\$pane_id\" >/dev/null 2>&1 || true
    return 0
  fi
  if [[ -n \"\$window_target\" ]]; then
    tmux select-window -t \"\$window_target\" >/dev/null 2>&1 || true
  fi
  tmux select-pane -t \"\$pane_id\" >/dev/null 2>&1 || true
  [[ -n \"\$window_target\" ]] || return 0
  while IFS=\$'\\t' read -r client_name client_session client_window; do
    [[ -n \"\$client_name\" ]] || continue
    [[ \"\$client_session:\$client_window\" == \"\$window_target\" || \"\$client_session\" == \"\$session_target\" ]] || continue
    tmux switch-client -c \"\$client_name\" -t \"\$window_target\" >/dev/null 2>&1 || true
    tmux select-pane -t \"\$pane_id\" >/dev/null 2>&1 || true
  done < <(tmux list-clients -F '#{client_name}	#{session_name}	#{window_index}' 2>/dev/null || true)
}
sleep 0.15
focus_tmux_pane $quoted_pane $quoted_client
sleep 0.45
focus_tmux_pane $quoted_pane $quoted_client
sleep 0.90
focus_tmux_pane $quoted_pane $quoted_client")"
  tmux run-shell -b "bash -lc $quoted_script" >/dev/null 2>&1 || true
}

run_map_menu() {
  if open_cmux_map_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_map_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_ghostty_selector_pane map "VibeMUD Map" >/dev/null 2>&1; then
    return 0
  fi
  if [[ -t 0 && -t 1 ]]; then
    run_map_selector
    return $?
  fi
  run_quiet run_mudctl map || true
  return 2
}

open_stats_selector_pane() {
  open_tmux_selector_pane stats equipment
}

run_stats_menu() {
  if open_cmux_stats_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_stats_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_ghostty_selector_pane stats "VibeMUD Items" >/dev/null 2>&1; then
    return 0
  fi
  if [[ -t 0 && -t 1 ]]; then
    run_stats_selector
    return $?
  fi
  run_quiet run_mudctl stats || true
  return 2
}

settings_menu_items=(language popup intro reset close)
settings_language_items=(language_ko language_en)
settings_popup_items=(popup_on popup_off)

settings_item_count() {
  case "${1:-main}" in
    language) printf '%s\n' "${#settings_language_items[@]}" ;;
    popup) printf '%s\n' "${#settings_popup_items[@]}" ;;
    *) printf '%s\n' "${#settings_menu_items[@]}" ;;
  esac
}

settings_item_at() {
  local menu="${1:-main}" index="${2:-0}"
  case "$menu" in
    language) printf '%s\n' "${settings_language_items[$index]}" ;;
    popup) printf '%s\n' "${settings_popup_items[$index]}" ;;
    *) printf '%s\n' "${settings_menu_items[$index]}" ;;
  esac
}

settings_item_label() {
  case "$1" in
    language) printf '한국어/English 설정' ;;
    language_ko) printf '한국어 UI' ;;
    language_en) printf 'English UI' ;;
    popup) printf '팝업 선택창 사용 여부' ;;
    popup_on) printf '팝업 선택창 사용' ;;
    popup_off) printf '팝업 선택창 사용 안 함' ;;
    intro) printf '오프닝 시나리오 다시보기' ;;
    reset) printf '게임 전체 초기화' ;;
    close) printf '닫기' ;;
    *) printf '%s' "$1" ;;
  esac
}

settings_item_hint() {
  case "$1" in
    language) printf '다음 화면에서 한국어 또는 English를 최종 선택합니다.' ;;
    language_ko) printf 'HUD/로그/메뉴 라벨을 한국어로 표시합니다.' ;;
    language_en) printf 'HUD/log/menu labels are displayed in English.' ;;
    popup) printf '다음 화면에서 오른쪽 선택창 사용 여부를 최종 선택합니다.' ;;
    popup_on) printf '장비/지도/퀘스트/설정을 오른쪽 선택창으로 엽니다.' ;;
    popup_off) printf '선택창 대신 일반 HUD/출력 fallback을 사용합니다.' ;;
    intro) printf '플래닛 64 오프닝을 다시 재생합니다. 테스트용으로 언제든 실행할 수 있습니다.' ;;
    reset) printf '캐릭터 진행, 골드, 장비, 소지품, 전투, 로그를 초기화합니다.' ;;
    close) printf '설정창을 닫고 HUD/클로드 창으로 돌아갑니다.' ;;
    *) printf '' ;;
  esac
}

render_settings_selector() {
  local selected="$1" message="${2:-}" confirm_reset="${3:-0}" submenu="${4:-main}"
  local i item marker language popup lines count title guide
  language="$(config_value ui.language ko)"
  popup="$(config_value ui.popup_pane_enabled true)"
  lines="$(message_printline)"
  count="$(settings_item_count "$submenu")"
  title="VibeMUD 설정"
  guide="↑/↓ 이동 · Enter 선택 · q 닫기"
  if [[ "$submenu" == "language" ]]; then
    title="VibeMUD 언어 설정"
    guide="한국어 / English 중 하나를 선택하면 최종 적용됩니다 · q 뒤로"
  elif [[ "$submenu" == "popup" ]]; then
    title="VibeMUD 팝업 선택창 설정"
    guide="사용 / 사용 안 함 중 하나를 선택하면 최종 적용됩니다 · q 뒤로"
  fi
  printf '\033[2J\033[H'
  echo "$title"
  echo "$guide"
  echo "------------------------------------------------------------"
  printf '현재 언어: %s · 팝업 선택창: %s · 메시지 줄수: %s\n' "$language" "$popup" "$lines"
  echo "------------------------------------------------------------"
  for ((i=0; i<count; i++)); do
    item="$(settings_item_at "$submenu" "$i")"
    marker="  "
    [[ "$i" -eq "$selected" ]] && marker="▶ "
    printf '%s%s\n' "$marker" "$(settings_item_label "$item")"
    printf '    %s\n' "$(settings_item_hint "$item")"
  done
  echo "------------------------------------------------------------"
  if [[ "$confirm_reset" == "1" ]]; then
    echo "초기화 확인: 정말 삭제하려면 Enter를 한 번 더 누르세요. 취소는 ↑/↓ 또는 q."
  fi
  if [[ -n "$message" ]]; then
    printf '%s\n' "$message"
  fi
}

run_settings_config_set() {
  run_vibemud config set "$1" "$2" 2>&1 || true
}

run_settings_intro_replay() {
  printf '\033[2J\033[H'
  run_vibemud intro --replay
  printf '\nEnter 또는 q를 누르면 설정으로 돌아갑니다. '
  IFS= read -rsn1 _key </dev/tty 2>/dev/null || true
  printf '\n'
}

close_settings_selector() {
  restore_selector_tty
  if [[ "${VIBEMUD_SELECTOR_TRANSIENT:-}" == "1" ]]; then
    restore_selector_return_focus
    return 0
  fi
  if [[ "${VIBEMUD_SELECTOR_PANE:-}" == "1" ]]; then
    exec_selector_hud
  fi
  return 0
}

run_settings_selector() {
  local selected=0 key more count item message="" confirm_reset=0 input_fd=0 tty_status=1 missed_reads=0 submenu="main"
  setup_selector_tty
  if [[ -t 0 || -t 1 ]]; then
    set +e
    exec 7</dev/tty 2>/dev/null
    tty_status=$?
    set -e
    if [[ "$tty_status" -eq 0 ]]; then
      input_fd=7
    fi
  fi
  count="$(settings_item_count "$submenu")"
  while true; do
    render_settings_selector "$selected" "$message" "$confirm_reset" "$submenu"
    if ! IFS= read -rsn1 -u "$input_fd" key; then
      if [[ "${VIBEMUD_SELECTOR_PANE:-}" == "1" ]]; then
        missed_reads=$((missed_reads + 1))
        if [[ "$missed_reads" -ge 5 ]]; then
          close_settings_selector
          return 1
        fi
        message="입력 연결 대기 중입니다. 연결이 복구되지 않으면 HUD로 돌아갑니다."
        sleep 0.5
        continue
      fi
      return 1
    fi
    missed_reads=0
    if [[ "$key" == $'\x1b' ]]; then
      IFS= read -rsn1 -t 1 -u "$input_fd" more || more=""
      key+="$more"
      IFS= read -rsn1 -t 1 -u "$input_fd" more || more=""
      key+="$more"
    fi
    case "$key" in
      q|Q)
        if [[ "$submenu" != "main" ]]; then
          submenu="main"
          selected=0
          count="$(settings_item_count "$submenu")"
          message=""
          confirm_reset=0
          continue
        fi
        close_settings_selector
        return 0
        ;;
      "")
        item="$(settings_item_at "$submenu" "$selected")"
        case "$item" in
          language)
            submenu="language"
            selected=0
            count="$(settings_item_count "$submenu")"
            message="언어를 최종 선택하세요."
            confirm_reset=0
            ;;
          language_ko)
            message="$(run_settings_config_set ui.language ko)"
            submenu="main"
            selected=0
            count="$(settings_item_count "$submenu")"
            confirm_reset=0
            ;;
          language_en)
            message="$(run_settings_config_set ui.language en)"
            submenu="main"
            selected=0
            count="$(settings_item_count "$submenu")"
            confirm_reset=0
            ;;
          popup)
            submenu="popup"
            selected=0
            count="$(settings_item_count "$submenu")"
            message="팝업 선택창 사용 여부를 최종 선택하세요."
            confirm_reset=0
            ;;
          popup_on)
            message="$(run_settings_config_set ui.popup_pane_enabled true)"
            submenu="main"
            selected=0
            count="$(settings_item_count "$submenu")"
            confirm_reset=0
            ;;
          popup_off)
            message="$(run_settings_config_set ui.popup_pane_enabled false)"
            submenu="main"
            selected=0
            count="$(settings_item_count "$submenu")"
            confirm_reset=0
            ;;
          intro)
            run_settings_intro_replay
            message="오프닝 시나리오 다시보기 완료"
            submenu="main"
            selected=0
            count="$(settings_item_count "$submenu")"
            confirm_reset=0
            ;;
          reset)
            if [[ "$confirm_reset" == "1" ]]; then
              printf '\n게임을 초기화합니다...\n'
              reset_game_progress
              return $?
            fi
            message="초기화는 되돌릴 수 없습니다."
            confirm_reset=1
            ;;
          close)
            close_settings_selector
            return 0
            ;;
        esac
        ;;
      $'\x1b[A'|$'\x1bOA'|k|K)
        selected=$(( (selected + count - 1) % count ))
        message=""
        confirm_reset=0
        ;;
      $'\x1b[B'|$'\x1bOB'|j|J)
        selected=$(( (selected + 1) % count ))
        message=""
        confirm_reset=0
        ;;
    esac
  done
}

open_settings_selector_pane() {
  open_tmux_selector_pane settings settings
}

open_quest_selector_pane() {
  open_tmux_selector_pane quest quest
}

run_quest_menu() {
  if open_cmux_quest_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_quest_selector_pane >/dev/null 2>&1; then
    return 0
  fi
  if open_ghostty_selector_pane quest "VibeMUD Quests" >/dev/null 2>&1; then
    return 0
  fi
  if [[ -t 0 && -t 1 ]]; then
    run_quest_selector
    return $?
  fi
  run_quiet run_mudctl quest || true
  return 2
}

case "${1:-}" in
  map-select)
    shift || true
    run_map_selector
    ;;
  stats-select)
    shift || true
    run_stats_selector
    ;;
  settings-select)
    shift || true
    run_settings_selector
    ;;
  quest-select)
    shift || true
    run_quest_selector
    ;;
  a|사냥|hunt)
    shift
    if is_safe_context; then
      if [[ "$#" -eq 0 ]]; then
        run_quiet run_mudctl hunt start --auto-start || {
          ack "실패: 이어하기"
          exit 1
        }
      elif target_is_dungeon "${1:-}"; then
        run_quiet run_mudctl dungeon enter "$@" --auto-start || {
          ack "실패: 던전 사냥"
          exit 1
        }
      else
        run_quiet run_mudctl hunt start --area "${1:-}" --auto-start || {
          ack "실패: 지역 사냥"
          exit 1
        }
      fi
      run_quiet run_mudctl stats close || true
      if open_best_panel >/dev/null 2>&1; then
        ack "사냥 시작 · HUD 열림"
        exit 0
      fi
      ack "사냥 시작 · HUD 실패"
      exit 3
    fi
    if [[ "$#" -eq 0 ]]; then
      run_mudctl hunt start --auto-start
    elif target_is_dungeon "${1:-}"; then
      run_mudctl dungeon enter "$@" --auto-start
    else
      run_mudctl hunt start --area "${1:-}" --auto-start
    fi
    run_mudctl stats close >/dev/null 2>&1 || true
    if open_best_panel; then
      echo "VibeMUD auto-hunt is running. Right-side HUD panel opened."
    else
      echo
      echo "VibeMUD auto-hunt is running, but no side-panel integration was available."
      echo "Use /vibemud:mud now or /vibemud:mud log while coding."
      echo "Emergency fallback, if you accept occupying this terminal: /vibemud:mud broadcast"
      echo
    fi
    ;;
  start|시작|플레이|play)
    shift
    if is_safe_context; then
      if [[ "$#" -eq 0 ]]; then
        if ! run_quiet run_mudctl hunt start --auto-start; then
          ack "실패: 이어하기"
          exit 1
        fi
      elif target_is_dungeon "${1:-}"; then
        if ! run_quiet run_mudctl dungeon enter "$@" --auto-start; then
          ack "실패: 던전 사냥"
          exit 1
        fi
      else
        if ! run_quiet run_mudctl hunt start --area "${1:-}" --auto-start; then
          ack "실패: 자동 사냥"
          exit 1
        fi
      fi
      run_quiet run_mudctl stats close || true
      if open_best_panel >/dev/null 2>&1; then
        ack "사냥 시작 · HUD 열림"
        exit 0
      fi
      ack "사냥 시작 · HUD 실패"
      exit 3
    fi
    if [[ "$#" -eq 0 ]]; then
      run_mudctl hunt start --auto-start
    elif target_is_dungeon "${1:-}"; then
      run_mudctl dungeon enter "$@" --auto-start
    else
      run_mudctl hunt start --area "${1:-}" --auto-start
    fi
    run_mudctl stats close >/dev/null 2>&1 || true
    if open_best_panel; then
      echo "VibeMUD is running. Right-side HUD panel opened."
      echo "Use /vibemud:mud now, /vibemud:mud log, or /vibemud:mud stop from Claude Code."
    else
      echo
      echo "VibeMUD is running, but no side-panel integration was available."
      echo "Use /vibemud:mud now or /vibemud:mud log while coding."
      echo "If you explicitly want the HUD to occupy this terminal, run /vibemud:mud broadcast."
      echo
    fi
    ;;
  s|stop|end|정지|종료|pause|중지)
    status="$(run_vibemud session status 2>/dev/null || true)"
    if is_safe_context; then
      run_quiet run_mudctl hunt stop || true
    else
      run_mudctl hunt stop || true
    fi
    if [[ "$status" == "running" ]]; then
      sleep 2
      if is_safe_context; then
        run_quiet run_vibemud session stop || true
      else
        run_vibemud session stop
      fi
    else
      if is_safe_context; then
        run_quiet run_vibemud session start --ticks 1 || true
        run_quiet run_vibemud session stop || true
      else
        run_vibemud session start --ticks 1 || true
        run_vibemud session stop
      fi
    fi
    close_tmux_panel
    close_cmux_panel
    close_ghostty_panel
    stop_background_helpers
    stop_terminal_broadcast
    is_safe_context && ack "정지 완료"
    ;;
  intro|opening|scenario|오프닝|시나리오|스토리)
    shift || true
    if is_safe_context; then
      ack "오프닝 다시보기: 터미널에서 `vibemud intro --replay` 또는 설정 메뉴의 다시보기를 사용하세요."
      exit 0
    fi
    run_vibemud intro --replay "$@"
    ;;
  set|settings|setting|설정|환경설정)
    shift || true
    case "${1:-}" in
      reset|리셋|초기화)
        if reset_game_progress; then
          exit 0
        fi
        ack "게임 리셋 실패"
        exit 1
        ;;
      *)
        if is_safe_context; then
          if run_settings_menu; then
            ack "설정창 열림"
          else
            ack "설정: 열림"
          fi
          exit 0
        fi
        run_settings_menu
        ;;
    esac
    ;;
  reset|리셋|초기화|새게임|newgame)
    shift || true
    if reset_game_progress; then
      exit 0
    fi
    ack "게임 리셋 실패"
    exit 1
    ;;
  now|상태|system|시스템)
    if is_safe_context; then
      ack "상태: HUD (--verbose)"
      exit 0
    fi
    run_mudctl system
    ;;
  i|inventory|item|items|장비|소지품|가방)
    shift || true
    if is_safe_context; then
      if run_stats_menu; then
        ack "장비/소지품창 열림"
      else
        ack "장비창: 열림"
      fi
      exit 0
    fi
    if run_stats_menu; then
      exit 0
    fi
    run_mudctl inventory
    ;;
  c|character|캐릭터)
    shift || true
    exec bash "$0" i "$@"
    ;;
  q|quest|quests|퀘스트|일일퀘스트)
    shift || true
    if is_safe_context; then
      if run_quest_menu; then
        ack "퀘스트창 열림"
      else
        ack "퀘스트: 열림"
      fi
      exit 0
    fi
    if run_quest_menu; then
      exit 0
    fi
    run_mudctl quest
    ;;
  x|close|닫기)
    shift || true
    if is_safe_context; then
      run_quiet run_mudctl stats close
      ack "스탯: 닫힘"
      exit 0
    fi
    run_mudctl stats close
    ;;
  m|map|menu|지도|메뉴|던전|지역)
    if ! only_map_alias_args "$@"; then
      shift || true
      if [[ "$#" -gt 0 ]]; then
        set -- a "$@"
        # Re-dispatch explicit map targets through the normal hunt/dungeon path.
        exec bash "$0" "$@"
      fi
    fi
    if is_safe_context; then
      if run_map_menu; then
        ack "지도 선택창 열림"
      else
        status=$?
        case "$status" in
          2) ack "지도: HUD · 선택은 /mud a <지역/던전>" ;;
          *) ack "지도: 선택 취소" ;;
        esac
      fi
      exit 0
    fi
    if run_map_menu; then
      exit 0
    fi
    echo "지도 HUD를 열었습니다. 선택: /vibemud:mud a <지역/던전>"
    ;;
  log|로그)
    shift
    if is_safe_context; then
      ack "로그: HUD (--verbose)"
      exit 0
    fi
    if [[ "${1:-}" == --* ]]; then
      run_mudctl log "$@"
    else
      tail="${1:-5}"
      run_mudctl log --tail "$tail"
    fi
    ;;
  panel|패널|오른쪽)
    if is_safe_context; then
      if open_best_panel >/dev/null 2>&1; then
        ack "HUD 열림"
        exit 0
      fi
      ack "HUD 실패"
      exit 3
    fi
    if open_best_panel; then
      echo "VibeMUD right-side HUD panel is open."
    else
      echo "cmux, tmux, or Ghostty AppleScript is required for the persistent side HUD panel."
      echo "Use /vibemud:mud broadcast only if you accept occupying this terminal."
    fi
    ;;
  broadcast|브로드캐스트|방송|live|라이브)
    if ! is_unsafe_context; then
      ack "차단: broadcast (--unsafe-context)"
      exit 0
    fi
    run_terminal_broadcast
    ;;
  watch|보기|hud|화면)
    shift
    if ! is_unsafe_context; then
      ack "차단: watch (panel 권장)"
      exit 0
    fi
    refresh="${1:-1}"
    run_vibemud hud --live --side --refresh "$refresh" --log-lines "$(message_printline)"
    ;;
  preview|프리뷰)
    shift || true
    if is_safe_context; then
      ack "프리뷰: HUD"
      exit 0
    fi
    print_live_preview "${1:-6}"
    ;;
  next|다음|추천)
    shift || true
    if is_safe_context; then
      ack "추천: a → i/m → s"
      exit 0
    fi
    print_next
    ;;
  tail)
    shift
    if is_safe_context; then
      ack "tail: HUD (--verbose)"
      exit 0
    fi
    if [[ "${1:-}" == --* ]]; then
      run_mudctl log "$@"
    else
      tail="${1:-5}"
      run_mudctl log --tail "$tail"
    fi
    ;;
  queue|큐)
    shift
    if is_safe_context; then
      ack "큐: HUD (--verbose)"
      exit 0
    fi
    if [[ "${1:-}" == --* ]]; then
      run_mudctl queue "$@"
    else
      tail="${1:-5}"
      run_mudctl queue --tail "$tail"
    fi
    ;;
  init|session|statusline|config|doctor|simulate|vibe)
    if is_safe_context; then
      if is_nested_agent_session_command "$@"; then
        ack "차단: 중첩 agent session (현재 Codex/Claude/OMX 안에서는 새 codex/claude 세션을 만들지 않음)"
        exit 0
      fi
      label="$(command_label "$@")"
      run_quiet run_vibemud "$@"
      ack "완료: vibemud ${label}"
      exit 0
    fi
    run_vibemud "$@"
    ;;
  runtime)
    shift
    if is_safe_context; then
      ack "차단: runtime (start/stop)"
      exit 0
    fi
    run_vibemud_runtime "$@"
    ;;
  status|full-status|stats|area|hunt|dungeon|party|inventory|equipment|quest|equip|unequip|enhance|강화|skill|shop|rest|town|alias)
    if is_safe_context; then
      label="$(command_label "$@")"
      run_quiet run_mudctl "$@"
      ack "완료: mudctl ${label}"
      exit 0
    fi
    run_mudctl "$@"
    ;;
  *)
    echo "Unknown VibeMUD command: $1" >&2
    echo >&2
    print_guide >&2
    exit 2
    ;;
esac
