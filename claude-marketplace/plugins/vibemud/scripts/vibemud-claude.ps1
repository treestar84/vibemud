# VibeMUD Claude Code dispatcher for native Windows (PowerShell 5.1+ / pwsh).
#
# This is the Windows counterpart of vibemud-claude.sh. It implements the
# safe/hook command surface used by the plugin context hook, plus verbose
# passthrough. The explicit legacy pane option uses Windows Terminal (wt.exe) splits:
#   - HUD panel: right-side split running `vibemud hud --panel`.
#   - Selectors (c/i/m/q/set): reuse the interactive bash selector inside a
#     wt split when Git Bash is available; otherwise fall back to switching
#     the HUD view via mudctl, exactly like the POSIX plain fallback.
# Windows support status: implemented, not yet smoke-verified on real
# Windows hardware. Keep acknowledgement strings aligned with
# vibemud-claude.sh so hook summaries stay consistent across platforms.

[CmdletBinding()]
param(
  [Parameter(ValueFromRemainingArguments = $true)]
  [string[]]$Rest = @()
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'
try {
  [Console]::OutputEncoding = [System.Text.Encoding]::UTF8
  $OutputEncoding = [System.Text.Encoding]::UTF8
} catch {
}

$script:ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$script:PluginRoot = Split-Path -Parent $script:ScriptDir
$script:RepoRoot = Resolve-Path -LiteralPath (Join-Path $script:ScriptDir '..\..\..\..') -ErrorAction SilentlyContinue
$script:ContextMode = if ($env:VIBEMUD_CONTEXT_MODE) { $env:VIBEMUD_CONTEXT_MODE } else { 'safe' }

# --- argument context flags (parity with vibemud-claude.sh) -----------------
$FilteredArgs = @()
foreach ($Arg in $Rest) {
  switch -Regex ($Arg) {
    '^--verbose$' { $script:ContextMode = 'verbose' }
    '^--unsafe-context$' { $script:ContextMode = 'unsafe' }
    '^--context-mode=(.+)$' { $script:ContextMode = $Matches[1] }
    default { $FilteredArgs += $Arg }
  }
}

function Test-SafeContext {
  return @('safe', 'hook', 'zero') -contains $script:ContextMode
}

function Test-UnsafeContext {
  return $script:ContextMode -eq 'unsafe'
}

function Write-Ack {
  param([string]$Message)
  Write-Output "[VibeMUD] $Message"
}

# --- binary resolution -------------------------------------------------------
function Resolve-VibemudBinary {
  param([string]$Name)
  $ExeName = "$Name.exe"
  if ($env:VIBEMUD_BIN_DIR) {
    foreach ($Candidate in @((Join-Path $env:VIBEMUD_BIN_DIR $ExeName), (Join-Path $env:VIBEMUD_BIN_DIR "$Name.cmd"), (Join-Path $env:VIBEMUD_BIN_DIR $Name))) {
      if (Test-Path -LiteralPath $Candidate) { return $Candidate }
    }
  }
  $FromPath = Get-Command $Name -ErrorAction SilentlyContinue
  if ($FromPath) { return $FromPath.Source }
  if ($env:LOCALAPPDATA) {
    foreach ($Candidate in @((Join-Path $env:LOCALAPPDATA "VibeMUD\bin\$Name.cmd"), (Join-Path $env:LOCALAPPDATA "VibeMUD\bin\$ExeName"))) {
      if (Test-Path -LiteralPath $Candidate) { return $Candidate }
    }
  }
  if ($script:RepoRoot) {
    foreach ($Sub in @('target\release', 'target\debug')) {
      $Candidate = Join-Path $script:RepoRoot (Join-Path $Sub $ExeName)
      if (Test-Path -LiteralPath $Candidate) { return $Candidate }
    }
  }
  return $null
}

function Invoke-Vibemud {
  param([string[]]$CommandArgs)
  $Bin = Resolve-VibemudBinary 'vibemud'
  if (-not $Bin) {
    throw 'vibemud binary not found (VIBEMUD_BIN_DIR, PATH, %LOCALAPPDATA%\VibeMUD\bin, target\release). Install with: npm install -g vibemud'
  }
  & $Bin @CommandArgs
}

function Invoke-Mudctl {
  param([string[]]$CommandArgs)
  $Bin = Resolve-VibemudBinary 'mudctl'
  if (-not $Bin) {
    throw 'mudctl binary not found (VIBEMUD_BIN_DIR, PATH, %LOCALAPPDATA%\VibeMUD\bin, target\release). Install with: npm install -g vibemud'
  }
  & $Bin @CommandArgs
}

function Invoke-Quiet {
  param(
    [ValidateSet('vibemud', 'mudctl')]
    [string]$Tool,
    [string[]]$CommandArgs
  )
  $Output = @()
  $Code = 1
  # Windows PowerShell 5.1 turns redirected native stderr into terminating
  # NativeCommandError under $ErrorActionPreference = 'Stop'; relax it locally
  # so warnings on stderr do not abort the captured call.
  $ErrorActionPreference = 'Continue'
  try {
    if ($Tool -eq 'vibemud') {
      $Output = @(Invoke-Vibemud -CommandArgs $CommandArgs 2>&1)
    } else {
      $Output = @(Invoke-Mudctl -CommandArgs $CommandArgs 2>&1)
    }
    $Code = $LASTEXITCODE
    if ($null -eq $Code) { $Code = 0 }
  } catch {
    $Output = @("$_")
    $Code = 127
  }
  if ($Code -ne 0 -and $Output) {
    $Output | Select-Object -First 3 | ForEach-Object { [Console]::Error.WriteLine("$_") }
  }
  return $Code
}

function Get-CommandLabel {
  param([string[]]$CommandArgs)
  $Label = ($CommandArgs -join ' ') -replace "`n", ' '
  if ($Label.Length -gt 80) { $Label = $Label.Substring(0, 80) }
  return $Label
}

function ConvertTo-CommandLineArgument {
  # Start-Process joins array ArgumentList items with spaces WITHOUT quoting,
  # so any item containing spaces (working directory, pane titles, -Command
  # payloads) would be split apart. Quote each item per Windows argv rules.
  param([string]$Value)
  if ($Value -match '^[^\s"]+$') { return $Value }
  $Escaped = $Value -replace '(\\*)"', '$1$1\"'
  $Escaped = $Escaped -replace '(\\+)$', '$1$1'
  return '"' + $Escaped + '"'
}

function ConvertTo-CommandLine {
  param([string[]]$Items)
  return (@($Items | ForEach-Object { ConvertTo-CommandLineArgument $_ }) -join ' ')
}

# --- Windows Terminal HUD panel ----------------------------------------------
function Get-WindowsTerminal {
  foreach ($Name in @('wt', 'wt.exe')) {
    $Found = Get-Command $Name -ErrorAction SilentlyContinue
    if ($Found) { return $Found.Source }
  }
  return $null
}

function Get-PowerShellHost {
  foreach ($Name in @('pwsh', 'pwsh.exe', 'powershell', 'powershell.exe')) {
    $Found = Get-Command $Name -ErrorAction SilentlyContinue
    if ($Found) { return $Found.Source }
  }
  return 'powershell.exe'
}

function Get-GitBash {
  # VS Code/Cursor terminals commonly expose C:\Windows\System32\bash.exe (WSL)
  # on PATH. WSL bash cannot run the selector with Windows C:/ paths, so only
  # accept the bash.exe that ships with Git for Windows.
  $Git = Get-Command git.exe -ErrorAction SilentlyContinue
  if ($Git -and $Git.Source) {
    $GitRoot = Split-Path -Parent (Split-Path -Parent $Git.Source)
    foreach ($Candidate in @((Join-Path $GitRoot 'bin\bash.exe'), (Join-Path $GitRoot 'usr\bin\bash.exe'))) {
      if (Test-Path -LiteralPath $Candidate) { return $Candidate }
    }
  }
  foreach ($Name in @('bash.exe', 'bash')) {
    $Found = Get-Command $Name -ErrorAction SilentlyContinue
    if ($Found -and $Found.Source -and $Found.Source -notmatch '\\Windows\\System32\\') { return $Found.Source }
  }
  foreach ($Candidate in @('C:\Program Files\Git\bin\bash.exe', 'C:\Program Files (x86)\Git\bin\bash.exe')) {
    if (Test-Path -LiteralPath $Candidate) { return $Candidate }
  }
  return $null
}

function Test-InsideWindowsTerminal {
  # WT_SESSION is inherited by nested apps (e.g. VS Code/Cursor launched from a
  # WT tab), so it alone does not prove this process renders inside WT. VS Code
  # style integrated terminals set TERM_PROGRAM (e.g. 'vscode'); real Windows
  # Terminal leaves it unset or sets 'WindowsTerminal'. Targeting `-w 0` from a
  # VS Code terminal would inject panes into an unrelated WT window.
  if (-not $env:WT_SESSION) { return $false }
  if ($env:TERM_PROGRAM -and $env:TERM_PROGRAM -ne 'WindowsTerminal') { return $false }
  return $true
}

function Test-HudProcess {
  try {
    $Procs = Get-CimInstance Win32_Process -Filter "Name = 'vibemud.exe'" -ErrorAction SilentlyContinue
    foreach ($Proc in @($Procs)) {
      if ($Proc.CommandLine -and $Proc.CommandLine -match 'hud' -and $Proc.CommandLine -match '--panel') {
        return $true
      }
    }
  } catch {
  }
  return $false
}

function Open-BestPanel {
  # Reuse a verified existing VibeMUD HUD before creating anything new.
  if (Test-HudProcess) { return $true }
  $Wt = Get-WindowsTerminal
  if (-not $Wt) { return $false }
  $VibemudBin = Resolve-VibemudBinary 'vibemud'
  if (-not $VibemudBin) { return $false }
  $Shell = Get-PowerShellHost
  $HudCommand = "& '$($VibemudBin -replace "'", "''")' hud --panel --refresh 1 --log-lines 999"
  $WtArgs = @()
  if (Test-InsideWindowsTerminal) { $WtArgs += @('-w', '0') }
  # -ExecutionPolicy Bypass: the resolved binary may be an npm-generated .ps1
  # shim, which the default Restricted policy would refuse to run in the pane.
  $WtArgs += @(
    'split-pane', '-H', '--size', '0.4', '--title', 'VibeMUD HUD',
    '-d', (Get-Location).Path,
    $Shell, '-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $HudCommand
  )
  try {
    $Proc = Start-Process -FilePath $Wt -ArgumentList (ConvertTo-CommandLine $WtArgs) -PassThru -WindowStyle Hidden
    if ($Proc) { $Proc.WaitForExit() }
    return ($Proc -and $Proc.ExitCode -eq 0)
  } catch {
    return $false
  }
}

function Close-HudPanel {
  # Closing the wt pane externally is not supported by wt.exe; ending the HUD
  # process closes the pane because it was launched without -NoExit.
  try {
    $Procs = Get-CimInstance Win32_Process -Filter "Name = 'vibemud.exe'" -ErrorAction SilentlyContinue
    foreach ($Proc in @($Procs)) {
      if ($Proc.CommandLine -and $Proc.CommandLine -match 'hud' -and $Proc.CommandLine -match '--panel') {
        Stop-Process -Id $Proc.ProcessId -Force -ErrorAction SilentlyContinue
      }
    }
  } catch {
  }
}

function Get-SelectorProcesses {
  # Selector panes are Git Bash processes running our dispatcher's *-select
  # entrypoints; the command-line match keeps user bash sessions untouched.
  $Selectors = @()
  try {
    $Procs = Get-CimInstance Win32_Process -Filter "Name = 'bash.exe'" -ErrorAction SilentlyContinue
    foreach ($Proc in @($Procs)) {
      if ($Proc.CommandLine -and $Proc.CommandLine -match 'vibemud-claude\.sh' -and $Proc.CommandLine -match '-select') {
        $Selectors += $Proc
      }
    }
  } catch {
  }
  return $Selectors
}

function Close-SelectorPanes {
  # wt.exe cannot retarget an existing pane, so switching selectors means
  # closing the verified previous selector pane (its bash root exits, the wt
  # pane closes) before opening the next one — the respawn-pane equivalent.
  foreach ($Proc in @(Get-SelectorProcesses)) {
    Stop-Process -Id $Proc.ProcessId -Force -ErrorAction SilentlyContinue
  }
}

function Open-SelectorPane {
  param(
    [ValidateSet('stats-select', 'map-select', 'quest-select', 'settings-select')]
    [string]$Selector,
    [string]$Title
  )
  # The interactive selector UI lives in vibemud-claude.sh; reuse it through
  # Git Bash inside a Windows Terminal split when both are available.
  $Wt = Get-WindowsTerminal
  $Bash = Get-GitBash
  $Dispatcher = Join-Path $script:ScriptDir 'vibemud-claude.sh'
  if (-not ($Wt -and $Bash -and (Test-Path -LiteralPath $Dispatcher))) { return $false }
  Close-SelectorPanes
  $DispatcherPosix = ($Dispatcher -replace '\\', '/')
  $BashCommand = "VIBEMUD_CONTEXT_MODE=unsafe '$DispatcherPosix' $Selector"
  $WtArgs = @()
  if (Test-InsideWindowsTerminal) { $WtArgs += @('-w', '0') }
  $WtArgs += @(
    'split-pane', '-H', '--size', '0.4', '--title', $Title,
    '-d', (Get-Location).Path,
    $Bash, '-lc', $BashCommand
  )
  try {
    $Proc = Start-Process -FilePath $Wt -ArgumentList (ConvertTo-CommandLine $WtArgs) -PassThru -WindowStyle Hidden
    if ($Proc) { $Proc.WaitForExit() }
    return ($Proc -and $Proc.ExitCode -eq 0)
  } catch {
    return $false
  }
}

function Test-DungeonTarget {
  param([string]$Target)
  $Dungeons = @(
    'goblin-den', 'crystal-cave', 'lich-tomb', 'cyclops-forge', 'medusa-temple', 'titan-vault',
    '고블린소굴', '고블린', '고블굴', '고블', '수정동굴', '수정굴', '수정',
    '리치무덤', '리치묘', '리치', '키클롭스대장간', '키클롭스', '대장간',
    '메두사신전', '메두사', '신전', '티탄금고', '티탄', '금고', '던전'
  )
  return $Dungeons -contains $Target
}

function Test-MapAliasArg {
  param([string]$Value)
  return @('m', 'map', 'menu', '지도', '메뉴', '던전', '지역') -contains $Value
}

function Test-NestedAgentSessionCommand {
  param([string[]]$CommandArgs)
  if ($CommandArgs.Count -lt 2) { return $false }
  if ($CommandArgs[0] -ne 'session') { return $false }
  return @('codex', 'claude') -contains $CommandArgs[1]
}

function Write-Guide {
  # Keep in sync with print_guide in vibemud-claude.sh.
  $Bash = Get-GitBash
  $Dispatcher = Join-Path $script:ScriptDir 'vibemud-claude.sh'
  if ($Bash -and (Test-Path -LiteralPath $Dispatcher)) {
    & $Bash ($Dispatcher -replace '\\', '/') 'help'
    if ($LASTEXITCODE -eq 0) { return }
  }
  Write-Output @'
VibeMUD 조작 가이드 (Windows)
============================================================
VibeMUD는 코딩 중 옆에서 자동으로 진행되는 로컬 MUD RPG입니다.

핵심 단축키
------------------------------------------------------------
  c  캐릭터/능력치       i  장비/소지품
  m  지도/던전 선택      q  일일 퀘스트
  start  게임 시작       end  게임 종료

자주 쓰는 명령
------------------------------------------------------------
  /vibemud:mud start             게임 시작/이어하기 + HUD pane
  /vibemud:mud c                 캐릭터/능력치 (HUD 뷰 또는 선택창)
  /vibemud:mud i                 장비/소지품 (HUD 뷰 또는 선택창)
  /vibemud:mud m                 지도/던전 (선택은 /mud a <지역/던전>)
  /vibemud:mud q                 일일 퀘스트
  /vibemud:mud set               설정 메뉴
  /vibemud:mud end               게임 종료
  /vibemud:mud help              이 조작 가이드 보기

Windows 안내
------------------------------------------------------------
  HUD pane은 Windows Terminal(wt.exe)에서 오른쪽 split로 열립니다.
  VS Code/Cursor 통합 터미널에서는 HUD가 별도 Windows Terminal 창으로 열립니다.
  wt.exe가 없으면 새 통합 터미널에서 직접 실행하세요: vibemud hud --panel
  Git Bash가 설치되어 있으면 선택창(↑/↓/Enter/q)도 pane으로 열립니다.
  Git Bash가 없으면 HUD 뷰 전환 + 직접 명령으로 조작합니다:
    /vibemud:mud a <지역/던전>   지역/던전 이동
    /vibemud:mud equip <아이템>  장비 장착

한국어 별칭
------------------------------------------------------------
  캐릭터=c, 장비/소지품/가방=i, 지도/던전/지역=m
  퀘스트/일일퀘스트=q, 설정=set, 시작=start, 종료=end, 도움말=help
'@
}

function Invoke-ResetGameProgress {
  if (Test-SafeContext) {
    Invoke-Quiet mudctl @('hunt', 'stop') | Out-Null
    Invoke-Quiet vibemud @('session', 'stop') | Out-Null
    $Code = Invoke-Quiet vibemud @('reset', '--yes')
    if ($Code -ne 0) { return $false }
    Invoke-Quiet mudctl @('stats', 'close') | Out-Null
    Close-HudPanel
    Close-SelectorPanes
    Write-Ack '게임 리셋 완료 · /mud start 시 오프닝 후 HUD'
    return $true
  }
  Invoke-Mudctl @('hunt', 'stop') | Out-Null
  Invoke-Vibemud @('session', 'stop') | Out-Null
  Invoke-Vibemud @('reset', '--yes') | Out-Null
  if ($LASTEXITCODE -ne 0) { return $false }
  Invoke-Mudctl @('stats', 'close') | Out-Null
  Close-HudPanel
  Close-SelectorPanes
  return $true
}

function Invoke-StartCommand {
  param([string[]]$Targets)
  if (Test-SafeContext) {
    if ($Targets.Count -eq 0) {
      if ((Invoke-Quiet mudctl @('hunt', 'start', '--auto-start')) -ne 0) {
        Write-Ack '실패: 이어하기'
        exit 1
      }
    } elseif (Test-DungeonTarget $Targets[0]) {
      if ((Invoke-Quiet mudctl (@('dungeon', 'enter') + $Targets + @('--auto-start'))) -ne 0) {
        Write-Ack '실패: 던전 사냥'
        exit 1
      }
    } else {
      if ((Invoke-Quiet mudctl @('hunt', 'start', '--area', $Targets[0], '--auto-start')) -ne 0) {
        Write-Ack '실패: 자동 사냥'
        exit 1
      }
    }
    Invoke-Quiet mudctl @('stats', 'close') | Out-Null
    if (Open-BestPanel) {
      Write-Ack '사냥 시작 · HUD 열림'
      exit 0
    }
    Write-Ack '사냥 시작 · HUD 실패'
    exit 3
  }
  if ($Targets.Count -eq 0) {
    Invoke-Mudctl @('hunt', 'start', '--auto-start') | Out-Null
  } elseif (Test-DungeonTarget $Targets[0]) {
    Invoke-Mudctl (@('dungeon', 'enter') + $Targets + @('--auto-start')) | Out-Null
  } else {
    Invoke-Mudctl @('hunt', 'start', '--area', $Targets[0], '--auto-start') | Out-Null
  }
  Invoke-Quiet mudctl @('stats', 'close') | Out-Null
  if (Open-BestPanel) {
    Write-Output 'VibeMUD is running. Right-side HUD panel opened.'
    Write-Output 'Use /vibemud:mud now, /vibemud:mud log, or /vibemud:mud stop from Claude Code.'
  } else {
    Write-Output ''
    Write-Output 'VibeMUD is running, but no Windows Terminal side-panel was available.'
    Write-Output 'In VS Code/Cursor, open a second integrated terminal and run: vibemud hud --panel'
    Write-Output 'Use /vibemud:mud now or /vibemud:mud log while coding.'
    Write-Output ''
  }
}

function Invoke-SelectorCommand {
  param(
    [ValidateSet('stats-select', 'map-select', 'quest-select', 'settings-select')]
    [string]$Selector,
    [string]$Title,
    [string]$OpenAck,
    [string]$FallbackAck,
    [string[]]$FallbackView
  )
  if (Open-SelectorPane -Selector $Selector -Title $Title) {
    if (Test-SafeContext) {
      Write-Ack $OpenAck
      exit 0
    }
    return
  }
  if ($FallbackView.Count -gt 0) {
    Invoke-Quiet mudctl $FallbackView | Out-Null
  }
  if (Test-SafeContext) {
    Write-Ack $FallbackAck
    exit 0
  }
}

# --- main dispatch ------------------------------------------------------------
if ($FilteredArgs.Count -eq 0 -or @('help', '--help', '-h', 'guide', '가이드', '도움말') -contains $FilteredArgs[0]) {
  Write-Guide
  exit 0
}

$Command = $FilteredArgs[0]
$CommandRest = @()
if ($FilteredArgs.Count -gt 1) { $CommandRest = $FilteredArgs[1..($FilteredArgs.Count - 1)] }

switch ($Command) {
  { @('a', '사냥', 'hunt', 'start', '시작', '플레이', 'play') -contains $_ } {
    Invoke-StartCommand -Targets $CommandRest
    break
  }
  { @('s', 'stop', 'end', '정지', '종료', 'pause', '중지') -contains $_ } {
    if (Test-SafeContext) {
      Invoke-Quiet mudctl @('hunt', 'stop') | Out-Null
      Invoke-Quiet vibemud @('session', 'stop') | Out-Null
    } else {
      Invoke-Mudctl @('hunt', 'stop') | Out-Null
      Invoke-Vibemud @('session', 'stop') | Out-Null
    }
    Close-HudPanel
    Close-SelectorPanes
    if (Test-SafeContext) { Write-Ack '정지 완료' }
    break
  }
  { @('intro', 'opening', 'scenario', '오프닝', '시나리오', '스토리') -contains $_ } {
    if (Test-SafeContext) {
      Write-Ack '오프닝 다시보기: 터미널에서 vibemud intro --replay 또는 설정 메뉴의 다시보기를 사용하세요.'
      exit 0
    }
    Invoke-Vibemud (@('intro', '--replay') + $CommandRest) | Out-Null
    break
  }
  { @('set', 'settings', 'setting', '설정', '환경설정') -contains $_ } {
    if ($CommandRest.Count -gt 0 -and (@('reset', '리셋', '초기화') -contains $CommandRest[0])) {
      if (Invoke-ResetGameProgress) { exit 0 }
      Write-Ack '게임 리셋 실패'
      exit 1
    }
    Invoke-SelectorCommand -Selector 'settings-select' -Title 'VibeMUD Settings' `
      -OpenAck '설정창 열림' -FallbackAck '설정: 열림' -FallbackView @()
    break
  }
  { @('reset', '리셋', '초기화', '새게임', 'newgame') -contains $_ } {
    if (Invoke-ResetGameProgress) { exit 0 }
    Write-Ack '게임 리셋 실패'
    exit 1
  }
  { @('now', '상태', 'system', '시스템') -contains $_ } {
    if (Test-SafeContext) {
      Write-Ack '상태: HUD (--verbose)'
      exit 0
    }
    Invoke-Mudctl @('system') | Out-Null
    break
  }
  { @('i', 'inventory', 'item', 'items', '장비', '소지품', '가방', 'c', 'character', '캐릭터') -contains $_ } {
    Invoke-SelectorCommand -Selector 'stats-select' -Title 'VibeMUD Items' `
      -OpenAck '장비/소지품창 열림' -FallbackAck '장비창: 열림' -FallbackView @('stats')
    break
  }
  { @('q', 'quest', 'quests', '퀘스트', '일일퀘스트') -contains $_ } {
    Invoke-SelectorCommand -Selector 'quest-select' -Title 'VibeMUD Quests' `
      -OpenAck '퀘스트창 열림' -FallbackAck '퀘스트: 열림' -FallbackView @('quest')
    break
  }
  { @('x', 'close', '닫기') -contains $_ } {
    if (Test-SafeContext) {
      Invoke-Quiet mudctl @('stats', 'close') | Out-Null
      Write-Ack '스탯: 닫힘'
      exit 0
    }
    Invoke-Mudctl @('stats', 'close') | Out-Null
    break
  }
  { @('m', 'map', 'menu', '지도', '메뉴', '던전', '지역') -contains $_ } {
    $NonAlias = @($CommandRest | Where-Object { -not (Test-MapAliasArg $_) })
    if ($NonAlias.Count -gt 0) {
      # Re-dispatch explicit map targets through the normal hunt/dungeon path.
      Invoke-StartCommand -Targets $NonAlias
      break
    }
    if (Open-SelectorPane -Selector 'map-select' -Title 'VibeMUD Map') {
      if (Test-SafeContext) {
        Write-Ack '지도 선택창 열림'
        exit 0
      }
      break
    }
    Invoke-Quiet mudctl @('map') | Out-Null
    if (Test-SafeContext) {
      Write-Ack '지도: HUD · 선택은 /mud a <지역/던전>'
      exit 0
    }
    Write-Output '지도 HUD를 열었습니다. 선택: /vibemud:mud a <지역/던전>'
    break
  }
  { @('log', '로그', 'tail') -contains $_ } {
    if (Test-SafeContext) {
      Write-Ack '로그: HUD (--verbose)'
      exit 0
    }
    if ($CommandRest.Count -gt 0 -and $CommandRest[0] -like '--*') {
      Invoke-Mudctl (@('log') + $CommandRest) | Out-Null
    } else {
      $Tail = if ($CommandRest.Count -gt 0) { $CommandRest[0] } else { '5' }
      Invoke-Mudctl @('log', '--tail', $Tail) | Out-Null
    }
    break
  }
  { @('queue', '큐') -contains $_ } {
    if (Test-SafeContext) {
      Write-Ack '큐: HUD (--verbose)'
      exit 0
    }
    if ($CommandRest.Count -gt 0 -and $CommandRest[0] -like '--*') {
      Invoke-Mudctl (@('queue') + $CommandRest) | Out-Null
    } else {
      $Tail = if ($CommandRest.Count -gt 0) { $CommandRest[0] } else { '5' }
      Invoke-Mudctl @('queue', '--tail', $Tail) | Out-Null
    }
    break
  }
  { @('panel', '패널', '오른쪽') -contains $_ } {
    if (Open-BestPanel) {
      if (Test-SafeContext) {
        Write-Ack 'HUD 열림'
        exit 0
      }
      Write-Output 'VibeMUD right-side HUD panel is open.'
    } else {
      if (Test-SafeContext) {
        Write-Ack 'HUD 실패 · 새 터미널에서 vibemud hud --panel'
        exit 3
      }
      Write-Output 'Windows Terminal (wt.exe) is required for the persistent side HUD panel.'
      Write-Output 'In VS Code/Cursor, open a second integrated terminal and run: vibemud hud --panel'
    }
    break
  }
  { @('broadcast', '브로드캐스트', '방송', 'live', '라이브') -contains $_ } {
    if (-not (Test-UnsafeContext)) {
      Write-Ack '차단: broadcast (--unsafe-context)'
      exit 0
    }
    Write-Output 'broadcast is not supported by the Windows dispatcher yet; use /vibemud:mud panel.'
    exit 1
  }
  { @('watch', '보기', 'hud', '화면') -contains $_ } {
    if (-not (Test-UnsafeContext)) {
      Write-Ack '차단: watch (panel 권장)'
      exit 0
    }
    $Refresh = if ($CommandRest.Count -gt 0) { $CommandRest[0] } else { '1' }
    Invoke-Vibemud @('hud', '--live', '--side', '--refresh', $Refresh, '--log-lines', '7') | Out-Null
    break
  }
  { @('preview', '프리뷰') -contains $_ } {
    if (Test-SafeContext) {
      Write-Ack '프리뷰: HUD'
      exit 0
    }
    Invoke-Mudctl @('system') | Out-Null
    break
  }
  { @('next', '다음', '추천') -contains $_ } {
    if (Test-SafeContext) {
      Write-Ack '추천: a → i/m → s'
      exit 0
    }
    Invoke-Mudctl @('system') | Out-Null
    break
  }
  { @('init', 'session', 'statusline', 'config', 'doctor', 'simulate', 'vibe') -contains $_ } {
    if (Test-SafeContext) {
      if (Test-NestedAgentSessionCommand $FilteredArgs) {
        Write-Ack '차단: 중첩 agent session (현재 Codex/Claude/OMX 안에서는 새 codex/claude 세션을 만들지 않음)'
        exit 0
      }
      $Label = Get-CommandLabel $FilteredArgs
      Invoke-Quiet vibemud $FilteredArgs | Out-Null
      Write-Ack "완료: vibemud $Label"
      exit 0
    }
    Invoke-Vibemud $FilteredArgs | Out-Null
    break
  }
  'runtime' {
    if (Test-SafeContext) {
      Write-Ack '차단: runtime (start/stop)'
      exit 0
    }
    $Bin = Resolve-VibemudBinary 'vibemud-runtime'
    if (-not $Bin) {
      Write-Error 'vibemud-runtime binary not found.'
      exit 127
    }
    & $Bin @CommandRest
    break
  }
  { @('status', 'full-status', 'stats', 'area', 'hunt', 'dungeon', 'party', 'inventory', 'equipment', 'quest', 'equip', 'unequip', 'enhance', '강화', 'skill', 'shop', 'rest', 'town', 'alias') -contains $_ } {
    if (Test-SafeContext) {
      $Label = Get-CommandLabel $FilteredArgs
      Invoke-Quiet mudctl $FilteredArgs | Out-Null
      Write-Ack "완료: mudctl $Label"
      exit 0
    }
    Invoke-Mudctl $FilteredArgs | Out-Null
    break
  }
  default {
    [Console]::Error.WriteLine("Unknown VibeMUD command: $Command")
    [Console]::Error.WriteLine('')
    Write-Guide
    exit 2
  }
}
