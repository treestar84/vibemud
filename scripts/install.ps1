param(
  [ValidateSet('cli', 'claude', 'all')]
  [string]$For = 'cli',

  [ValidateSet('user', 'project', 'local')]
  [string]$Scope = 'local',

  [switch]$SkipBuild,

  [switch]$Setup,

  [ValidateSet('ko', 'en')]
  [string]$Language,

  [ValidateSet('auto', 'claude', 'cli')]
  [string]$Agent,

  [ValidateSet('auto', 'tmux', 'ghostty', 'windows-terminal', 'plain')]
  [string]$Terminal,

  [ValidateSet('user', 'project')]
  [string]$Storage,

  [switch]$Yes
)

$ErrorActionPreference = 'Stop'
if (-not $env:LOCALAPPDATA) {
  throw 'LOCALAPPDATA environment variable is not set; cannot locate the VibeMUD shim directory.'
}
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$ShimDir = Join-Path $env:LOCALAPPDATA 'VibeMUD\bin'
$Bins = @('vibemud', 'mudctl', 'vibemud-hud', 'vibemud-runtime')

function Build-Release {
  if ($SkipBuild) { return }
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo not found: install Rust (https://rustup.rs) or rerun with -SkipBuild against prebuilt binaries.'
  }
  Write-Host '==> Building VibeMUD release binaries'
  Push-Location $RepoRoot
  try {
    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
  } finally {
    Pop-Location
  }
}

function Install-Shims {
  Write-Host "==> Installing CLI shims: $ShimDir"
  New-Item -ItemType Directory -Force -Path $ShimDir | Out-Null
  foreach ($Bin in $Bins) {
    $Target = Join-Path $RepoRoot "target\release\$Bin.exe"
    if (-not (Test-Path $Target)) {
      throw "Missing built binary: $Target. Run without -SkipBuild, or build with: cargo build --workspace --release"
    }
    $Cmd = Join-Path $ShimDir "$Bin.cmd"
    $EscapedTarget = $Target.Replace('%', '%%')
    $CmdContent = "@echo off`r`nchcp 65001 >nul`r`n" + '"' + $EscapedTarget + '" %*' + "`r`n"
    [System.IO.File]::WriteAllText($Cmd, $CmdContent, [System.Text.UTF8Encoding]::new($false))
  }
  # The mod launches .exe directly, without cmd.exe parsing game arguments.
  [System.IO.File]::WriteAllText((Join-Path $ShimDir 'native-dir.txt'), (Join-Path $RepoRoot 'target\release'), [System.Text.UTF8Encoding]::new($false))
}

function Invoke-FirstTimeSetup {
  if (-not ($Setup -or $Language -or $Agent -or $Terminal -or $Storage -or $Yes)) { return }
  $SetupArgs = @('setup')
  if ($Language) { $SetupArgs += @('--language', $Language) }
  if ($Agent) { $SetupArgs += @('--agent', $Agent) }
  if ($Terminal) { $SetupArgs += @('--terminal', $Terminal) }
  if ($Storage) { $SetupArgs += @('--storage', $Storage) }
  if ($Yes) { $SetupArgs += '--yes' }
  $VibeMudCmd = Join-Path $ShimDir 'vibemud.cmd'
  Write-Host '==> Running first-time setup'
  & $VibeMudCmd @SetupArgs
  if ($LASTEXITCODE -ne 0) { throw "vibemud setup failed with exit code $LASTEXITCODE" }
}


function Write-Path-Hint {
  $PathItems = ($env:PATH -split ';') | Where-Object { $_ }
  if ($PathItems -notcontains $ShimDir) {
    Write-Host ''
    Write-Host 'PATH note:'
    Write-Host "  Add this directory to PATH if 'vibemud' is not found:"
    Write-Host "  $ShimDir"
  }
}

function Write-Cli-QuickStart {
  Write-Host ''
  Write-Host 'CLI quick start:'
  Write-Host '  vibemud init'
  Write-Host '  vibemud session start --background'
  Write-Host '  mudctl hunt start --area forest-edge --auto-start'
  Write-Host '  vibemud hud --panel --refresh 1 --log-lines 12'
}

function Write-Claude-QuickStart {
  Write-Host ''
  Write-Host 'Claude Code quick start:'
  Write-Host '  /vibemud:mud start'
  Write-Host '  /vibemud:mud c'
  Write-Host '  /vibemud:mud i'
  Write-Host '  /vibemud:mud m'
  Write-Host '  /vibemud:mud q'
  Write-Host '  /vibemud:mud end'
}

Build-Release
Install-Shims
Invoke-FirstTimeSetup

if ($For -eq 'claude' -or $For -eq 'all') {
  Write-Host ''
  Write-Host '==> Installing Claude Code plugin (Windows native path; not yet smoke-verified)'
  & (Join-Path $PSScriptRoot 'install-claude-plugin.ps1') -Scope $Scope -SkipBuild
  if ($LASTEXITCODE -ne 0) { throw "Claude plugin install failed with exit code $LASTEXITCODE" }
  Write-Claude-QuickStart
}


if ($For -eq 'cli') {
  Write-Cli-QuickStart
}

Write-Path-Hint
if (-not ($Setup -or $Language -or $Agent -or $Terminal -or $Storage -or $Yes)) {
  Write-Host ''
  Write-Host 'First-time setup:'
  Write-Host '  Run `vibemud setup` to choose language, Claude/CLI mode, terminal, and storage location.'
  Write-Host '  HUD/popup behavior uses safe defaults and remains configurable later.'
}
Write-Host ''
Write-Host 'VibeMUD install complete.'
Write-Host "Installed shims: $ShimDir"
