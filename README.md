# VibeMUD

**Claude Code 옆에서 함께 진행되는 로컬 방치형 RPG.** `/mud start`로 게임 창을 열면 코딩하는 동안 캐릭터가 사냥하고 성장합니다. 장비·던전·퀘스트는 Claude Code 안의 mod 창에서 조작합니다.

*An idle RPG inside a Claude Code mod pane. The game runs locally in Rust and SQLite; see [Quick start](#빠른-시작) for source installation.*

> **배포 상태:** 이 저장소의 `0.2.0`은 개발본입니다. 현재 공개 npm 패키지와 섞어 설치하면 mod용 bridge가 맞지 않을 수 있습니다. 지금은 아래와 같이 **이 저장소의 CLI와 플러그인을 함께** 실행하세요. 새 퍼블릭 저장소를 푸시하는 것과 npm 배포는 별개입니다.

## 빠른 시작

[Claude Code 2.1.287 이상](https://code.claude.com/docs/ko/plugins/mods/overview), Node.js 18 이상, Rust toolchain이 필요합니다. 실제 mod 창은 macOS Claude Code 2.1.288에서 확인했습니다.

macOS / Linux:

```bash
git clone https://github.com/treestar84/vibemud.git
cd vibemud
cargo build --workspace
export VIBEMUD_BIN_DIR="$PWD/target/debug"
claude --plugin-dir ./claude-marketplace/plugins/vibemud
```

Windows PowerShell:

```powershell
git clone https://github.com/treestar84/vibemud.git
Set-Location vibemud
cargo build --workspace
$env:VIBEMUD_BIN_DIR = (Resolve-Path ./target/debug).Path
claude --plugin-dir ./claude-marketplace/plugins/vibemud
```

Claude Code 입력창에서 `/mud start`를 실행하세요. 다른 플러그인이 `/mud`를 이미 사용한다면 `/vibemud:mud start`를 사용합니다. `/mud`는 코딩 중에도 바로 실행되는 명령입니다. 긴 이름의 명령은 Claude Code의 예약 방식에 따라 현재 작업이 끝난 뒤 실행될 수 있습니다.

지속 설치가 필요하면 소스에서 빌드한 뒤 `scripts/install.sh --for claude --scope local` 또는 PowerShell의 `scripts/install.ps1 -For claude -Scope local`을 사용하고 Claude Code를 다시 시작하세요. 설치 방식과 관계없이 새 CLI와 mod를 같은 버전으로 맞춰야 합니다.

## 게임 화면과 조작

```text
/mud start       게임 시작 또는 다시 연결
/mud             모험 화면 열기
/mud c           캐릭터, 파티·스킬·상점
/mud i           장비와 소지품
/mud m           사냥터와 던전
/mud q           퀘스트
/mud set         설정과 게임 정지
/mud close       창 접기 · 게임 계속
/mud end         게임 정지
```

같은 명령을 `/vibemud:mud`로도 실행할 수 있습니다. 창 안에서는 버튼을 누르거나 `hunt start forest-edge`, `dungeon enter goblin-den`, `quest claim-all` 같은 게임 명령을 입력하세요.

**모험으로 돌아가기**는 메뉴만 바꿉니다. **창 접기 · 게임 계속**는 창을 숨기고 코딩 입력 위에 *모험 펼치기* 버튼을 남깁니다. `Esc`는 코딩 입력으로 초점을 돌립니다. 게임을 실제로 멈추려면 설정에서 **게임 정지**를 확인하거나 `/mud end`를 실행하세요. `end`는 같은 저장을 쓰는 다른 Claude 창의 게임도 정지합니다.

상점·장비·던전·퀘스트·파티는 이름과 수치를 같은 열에 맞춰 보여 줍니다. 한글 표시 폭에 따라 긴 이름을 줄여 표시하며, 선택하면 전체 이름과 상세 정보를 볼 수 있습니다. 소지품과 목적지 목록은 페이지로 나뉩니다. 체력·성장 막대, 전투 장면, 등급 색과 선택 표시를 함께 사용합니다.

Claude 작업이 시작되면 작업 **시작·완료 신호만** 피버에 사용합니다. 작업 내용·토큰 수·소스 파일은 읽지 않습니다. 작업이 끝난 뒤 15초 동안 피버 여운이 이어집니다. 모험 화면은 그동안 실제로 얻은 처치·경험치·골드·아이템을 보여 줍니다. 구매나 판매는 원정 수익으로 계산하지 않으며, 코딩을 완료했다는 이유로 추가 게임 보상을 만들지 않습니다. 화면에 남는 지난 원정 요약은 mod 실행 중에만 유지됩니다.

Claude Code가 창 배치를 결정합니다. 넓은 전체 화면 터미널에서는 옆에, 좁은 화면에서는 입력창 위에 표시될 수 있습니다. [공식 mod 창 설명](https://code.claude.com/docs/ko/plugins/mods/interface)

## 저장 데이터와 개인정보

| 환경 | 기본 게임 데이터 폴더 |
| --- | --- |
| macOS / Linux | `~/.vibemud` |
| Windows | `%LOCALAPPDATA%\VibeMUD` (`LOCALAPPDATA`가 없으면 `%USERPROFILE%\.vibemud`) |

`VIBEMUD_HOME`에 절대 경로를 주면 다른 데이터 폴더를 사용할 수 있습니다. 프로젝트 파일에는 새 게임 저장을 만들지 않습니다. 기존 프로젝트의 `.vibemud` 저장이 발견되고 사용자 저장소가 비어 있다면, 창에서 **원본을 남긴 채 복사**할지 선택할 수 있습니다.

게임 상태와 원정 집계는 사용자 데이터 폴더의 SQLite에 저장됩니다. VibeMUD는 소스 파일, 프롬프트, 대화 기록, 에디터 버퍼, 에이전트 대화 내용을 게임 상태로 사용하지 않습니다. 게임 명령과 결과도 Claude의 대화 문맥에 추가하지 않습니다.

초기화는 확인 후 `backups/before-mod-reset-*.db`에 백업을 만들고 진행을 지웁니다. 백업을 복원할 때는 모든 게임과 Claude 세션을 종료하고, 현재 데이터 폴더를 따로 보관한 다음, **새 데이터 폴더**에 백업을 `vibemud.db`로 복사하세요. 기존 `config.toml`도 함께 놓고 `VIBEMUD_HOME`을 새 폴더로 지정합니다. 실행 중인 DB를 덮어쓰지 마세요.

각 Claude 세션의 게임 연결은 별도로 갱신됩니다. mod가 시작한 게임은 마지막 연결이 사라지면 종료되며, 비정상 종료 시에도 연결 만료 후 약 30초 안에 멈춥니다. 창만 접으면 연결이 유지됩니다.

## 호환 옵션과 지원 범위

이전 외부 창 방식은 전환 안정화 기간 동안 명시적으로 사용할 수 있습니다.

```text
/vibemud:mud legacy start
/vibemud:mud legacy i
/vibemud:mud legacy end
```

외부 창 검증 대상은 tmux, cmux, macOS Ghostty입니다. Windows의 기본 mod 경로는 Bash·WSL·tmux·`wt.exe` 없이 Node launcher가 네이티브 `.exe`를 호출하도록 구현했습니다. **Windows 실기, Windows IME, Desktop 앱의 실제 화면은 아직 검증하지 못했습니다.** Windows x64 빌드·Rust·bridge 검사와 ARM64 빌드 검사는 [수동 실행하는 GitHub Actions](https://github.com/treestar84/vibemud/actions/workflows/package-dry-run.yml)에서 확인할 수 있습니다. Codex와 iTerm2는 지원 대상이 아닙니다.

## 개발과 공개

Rust runtime이 전투와 보상을 처리하고, SQLite 명령 큐가 게임 변경을 직렬화합니다. mod는 읽기 전용 snapshot으로 화면을 그립니다. 같은 요청을 재시도해도 중복 구매·보상이 생기지 않도록 요청 ID를 사용합니다.

```text
Claude Code mod → Node launcher → Rust CLI bridge
                                   ↓
                       SQLite 명령 큐 → Rust runtime
                                   ↓
                          SQLite snapshot → mod 창
```

주요 검사:

```bash
cargo fmt --check
cargo test --workspace
cargo build --workspace
node --test npm/test/bridge.test.js
claude plugin validate ./claude-marketplace/plugins/vibemud --strict
claude plugin test ./claude-marketplace/plugins/vibemud
node npm/scripts/check-release-metadata.js
(cd npm && npm run test:resolve)
(cd npm && npm pack --dry-run --json | node scripts/check-pack-contents.js)
```

전체 플랫폼·패키지 검증은 릴리스 준비 시에만 GitHub의 **Actions → package dry run → Run workflow**에서 수동 실행합니다. 일반 푸시와 PR에는 자동 실행되지 않으며, 업로드한 빌드 아티팩트는 3일 뒤 만료됩니다.

기여 방법은 [CONTRIBUTING.md](CONTRIBUTING.md), 취약점 제보는 [SECURITY.md](SECURITY.md), CLI 패키징은 [npm/README.md](npm/README.md)를 참고하세요. MIT 라이선스입니다.
