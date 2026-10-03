---
description: "VibeMUD 모험 창: start 시작, c/i/m/q 메뉴, close 창 접기, end 게임 정지"
argument-hint: "start|c|i|m|q|set|close|end|help|legacy"
allowed-tools: []
hide-from-slash-command-tool: "true"
---

# VibeMUD

The mod handles `/mud` and `/vibemud:mud` directly through `command.run` and returns `{}`. Game state, controls and errors belong exclusively in its pane. `close` hides the pane while the game continues; `end` stops the shared game runtime. Never fetch game state, read project files, run a dispatcher, or interpret game arguments for this command.

The classic context firewall blocks commands if the mod is unavailable. The explicit `legacy` option keeps the previous external terminal integration during migration.

If this text reaches Claude because hooks are disabled, give only this setup notice: “VibeMUD requires Claude Code 2.1.287+ with the VibeMUD mod enabled, Node.js 18+, and the matching native CLI. Reload the plugin, then run /vibemud:mud start. The external-pane compatibility option is /vibemud:mud legacy start.”
