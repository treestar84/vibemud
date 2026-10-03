# Install the local VibeMUD Claude Code plugin on native Windows.
#
# Recommended easier entry point:
#   scripts/install.ps1 -For claude -Scope local
#
# Advanced direct usage:
#   scripts/install-claude-plugin.ps1 [-Scope user|project|local] [-SkipBuild]
#
# What this does:
#   1. Builds VibeMUD release binaries unless -SkipBuild is set.
#   2. Creates stable .cmd shims in %LOCALAPPDATA%\VibeMUD\bin for the plugin.
#   3. Validates claude-marketplace/ with `claude plugin validate`.
#   4. Adds/updates the local `vibemud-local` marketplace.
#   5. Installs/enables `vibemud@vibemud-local` in the selected scope.
#
# Windows plugin support status: implemented, not yet smoke-verified on real
# Windows hardware. The plugin also requires Node.js (>=18) on PATH because the
# context hook entry point is scripts/vibemud-context-hook.js.

[CmdletBinding()]
param(
  [ValidateSet('user', 'project', 'local')]
  [string]$Scope = 'user',

  [switch]$SkipBuild
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

if (-not $env:LOCALAPPDATA) {
  Write-Error 'LOCALAPPDATA environment variable is not set; cannot locate the VibeMUD shim directory.'
  exit 1
}
$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$MarketplaceDir = Join-Path $RepoRoot 'claude-marketplace'
$MarketplaceName = 'vibemud-local'
$PluginId = "vibemud@$MarketplaceName"
$ShimDir = Join-Path $env:LOCALAPPDATA 'VibeMUD\bin'
$Bins = @('vibemud', 'mudctl', 'vibemud-hud', 'vibemud-runtime')

if (-not (Get-Command claude -ErrorAction SilentlyContinue)) {
  Write-Error 'Claude Code CLI not found: install Claude Code before installing the plugin.'
  exit 127
}
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
  Write-Error 'Node.js not found: the VibeMUD plugin context hook requires Node.js (>=18) on PATH.'
  exit 127
}
if (-not (Test-Path -LiteralPath $MarketplaceDir)) {
  Write-Error "Marketplace directory not found: $MarketplaceDir"
  exit 1
}

if (-not $SkipBuild) {
  Write-Host '==> Building VibeMUD release binaries'
  Push-Location $RepoRoot
  try {
    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
  } finally {
    Pop-Location
  }
}

Write-Host "==> Installing CLI shims: $ShimDir"
New-Item -ItemType Directory -Force -Path $ShimDir | Out-Null
foreach ($Bin in $Bins) {
  $Target = Join-Path $RepoRoot "target\release\$Bin.exe"
  if (-not (Test-Path -LiteralPath $Target)) {
    Write-Error "Missing built binary: $Target. Run without -SkipBuild, or build with: cargo build --workspace --release"
    exit 1
  }
  $Cmd = Join-Path $ShimDir "$Bin.cmd"
  $EscapedTarget = $Target.Replace('%', '%%')
  $CmdContent = "@echo off`r`nchcp 65001 >nul`r`n" + '"' + $EscapedTarget + '" %*' + "`r`n"
  [System.IO.File]::WriteAllText($Cmd, $CmdContent, [System.Text.UTF8Encoding]::new($false))
}
[System.IO.File]::WriteAllText((Join-Path $ShimDir 'native-dir.txt'), (Join-Path $RepoRoot 'target\release'), [System.Text.UTF8Encoding]::new($false))

Write-Host '==> Validating Claude marketplace/plugin'
claude plugin validate $MarketplaceDir
if ($LASTEXITCODE -ne 0) { throw "claude plugin validate failed with exit code $LASTEXITCODE" }

function Test-MarketplaceExists {
  # PS 5.1 turns redirected native stderr into terminating errors under
  # $ErrorActionPreference = 'Stop'; relax it locally for the probe call.
  $ErrorActionPreference = 'Continue'
  try {
    $Raw = claude plugin marketplace list --json 2>$null | Out-String
    $Data = $Raw | ConvertFrom-Json
    foreach ($Item in @($Data)) {
      if ($Item.name -eq $MarketplaceName) { return $true }
    }
  } catch {
  }
  return $false
}

function Test-PluginInstalled {
  $ErrorActionPreference = 'Continue'
  try {
    $Raw = claude plugin list --json 2>$null | Out-String
    $Data = $Raw | ConvertFrom-Json
    $Installed = if ($Data.PSObject.Properties['installed']) { $Data.installed } else { $Data }
    foreach ($Item in @($Installed)) {
      if ($Item.id -ne $PluginId) { continue }
      if ($Item.scope -ne $Scope) { continue }
      if (@('project', 'local') -contains $Scope) {
        $ProjectPath = if ($Item.PSObject.Properties['projectPath']) { $Item.projectPath } else { $null }
        if ($null -ne $ProjectPath -and $ProjectPath -ne $RepoRoot) { continue }
      }
      return $true
    }
  } catch {
  }
  return $false
}

if (Test-MarketplaceExists) {
  Write-Host "==> Updating existing marketplace: $MarketplaceName"
  claude plugin marketplace update $MarketplaceName
  if ($LASTEXITCODE -ne 0) { throw "claude plugin marketplace update failed with exit code $LASTEXITCODE" }
} else {
  Write-Host "==> Adding marketplace: $MarketplaceDir"
  claude plugin marketplace add $MarketplaceDir --scope $Scope
  if ($LASTEXITCODE -ne 0) { throw "claude plugin marketplace add failed with exit code $LASTEXITCODE" }
}

if (Test-PluginInstalled) {
  Write-Host "==> Refreshing installed plugin in $Scope scope"
  # Claude skips cache refresh when a local marketplace keeps the same plugin
  # version. Reinstall with data preserved so local changes are applied.
  claude plugin uninstall $PluginId --scope $Scope --keep-data
  claude plugin install $PluginId --scope $Scope
} else {
  Write-Host "==> Installing plugin: $PluginId ($Scope scope)"
  claude plugin install $PluginId --scope $Scope
}
if ($LASTEXITCODE -ne 0) { throw "claude plugin install failed with exit code $LASTEXITCODE" }

Write-Host @"

VibeMUD Claude Code plugin installed (Windows native path; not yet smoke-verified).

Restart Claude Code, then use the Claude quick start:
  /vibemud:mud start
  /vibemud:mud c
  /vibemud:mud i
  /vibemud:mud m
  /vibemud:mud q
  /vibemud:mud end

Notes for Windows:
  - The HUD pane opens as a Windows Terminal (wt.exe) split where available.
  - With Git Bash installed, c/i/m/q/set open the interactive selector pane.
  - Without Git Bash, selectors switch the HUD view; use direct commands like
    /vibemud:mud a <area|dungeon> and /vibemud:mud equip <item>.

Stable binary shims:
  $ShimDir

To uninstall:
  claude plugin uninstall $PluginId --scope $Scope
"@
