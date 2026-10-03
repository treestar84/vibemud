# VibeMUD에 기여하기

VibeMUD는 Claude Code 안에서 실행되는 로컬 게임입니다. 문제를 재현할 때는 Claude Code 버전, 운영체제, 설치 방법, 입력한 `/mud` 명령과 실제 게임 화면의 동작을 적어 주세요. **프롬프트·대화 기록·소스 코드·개인 저장 DB를 이슈에 올리지 마세요.** 취약점은 [보안 제보 안내](SECURITY.md)에 따라 비공개로 알려 주세요.

## 변경할 때 지킬 계약

- 기본 인터페이스는 Claude Code 2.1.287+ mod 창입니다. `/mud`가 이미 사용 중이면 `/vibemud:mud`를 사용합니다. tmux·cmux·macOS Ghostty는 명시적 `legacy` 호환 옵션입니다.
- 소스 파일, 프롬프트, transcript, 에디터 버퍼, 에이전트 대화 내용을 게임 상태로 사용하지 않습니다. 턴 시작·완료 신호만 피버에 사용합니다.
- 게임 상태는 사용자 데이터 폴더의 SQLite에 저장합니다. 게임 변경은 runtime 명령 큐를 거치고, 화면 조회는 읽기 전용으로 유지합니다.
- 창 접기와 게임 정지를 구분하고, 메뉴에서 모험으로 돌아갈 수 있게 유지합니다. 좁은 화면·한글 표시 폭·긴 이름을 함께 확인합니다.
- Windows mod 경로는 Bash·WSL·tmux·Windows Terminal 분할을 요구하지 않아야 합니다. 실제 실행하지 않은 플랫폼은 검증했다고 쓰지 않습니다.
- Codex, iTerm2, `~mud`는 현재 지원 대상이 아닙니다.

## 로컬 검사

변경한 영역과 관련된 검사를 먼저 실행하세요. CLI나 mod를 바꿨다면:

```bash
cargo fmt --check
cargo test -p vibemud-db -p vibemud-runtime -p vibemud-cli
cargo build --workspace
node --test npm/test/bridge.test.js
claude plugin validate ./claude-marketplace/plugins/vibemud --strict
claude plugin test ./claude-marketplace/plugins/vibemud
node --check claude-marketplace/plugins/vibemud/scripts/vibemud-context-hook.js
python3 -m py_compile claude-marketplace/plugins/vibemud/scripts/vibemud-context-hook.py
bash -n scripts/install.sh scripts/install-claude-plugin.sh claude-marketplace/plugins/vibemud/scripts/vibemud-claude.sh
```

npm 패키징이나 저장소 메타데이터를 바꿨다면:

```bash
node npm/scripts/check-release-metadata.js
(cd npm && npm run test:resolve)
(cd npm && npm pack --dry-run --json | node scripts/check-pack-contents.js)
```

PowerShell 파일을 바꿨다면 Windows에서 parser 검사도 실행하세요. 실제 Claude 창의 사용감을 바꾼 경우, 가능하면 **시작 → 장비·지도·상점 → 창 접기·복귀 → 정지 → 프로세스 정리**를 직접 확인하고, 사용한 OS와 Claude Code 버전을 변경 설명에 적어 주세요. mod 테스트 호스트의 요소 검증은 Windows·Desktop 실기 확인과 구분해 주세요.

전체 플랫폼 빌드와 npm 패키지 dry-run은 릴리스 준비 시 [package dry run](https://github.com/treestar84/vibemud/actions/workflows/package-dry-run.yml)을 수동 실행합니다. 일반 푸시·PR은 이 대형 워크플로를 자동으로 실행하지 않습니다.

공개 트리에는 소스, 플러그인, npm 패키징, GitHub 검증 워크플로, 설치 스크립트와 핵심 문서만 넣습니다. 생성물·게임 데이터·로그·개인 참고 자료·미디어 데모는 제외합니다. 문서가 필요하면 새 `docs/` 폴더보다 기존 README나 보안·기여 안내를 먼저 고쳐 주세요.
