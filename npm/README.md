# VibeMUD npm 패키징

이 폴더는 VibeMUD의 Rust CLI를 npm에서 실행하기 위한 패키지 소스입니다. 루트 `vibemud` 패키지는 `vibemud`, `mudctl`, `vibemud-runtime`, `vibemud-hud` 명령을 제공하며, 플랫폼별 네이티브 패키지를 선택합니다.

**저장소의 0.2.0은 개발본이며 아직 npm에 공개되지 않았습니다.** Claude Code mod를 지금 사용하려면 [저장소 README의 소스 설치](../README.md#빠른-시작)를 따르세요. 현재 npm `latest`를 설치해도 이 개발본의 mod bridge가 설치되는 것은 아닙니다.

## 패키지 구성

| 패키지 | 역할 |
| --- | --- |
| `vibemud` | CLI 진입점, 네이티브 바이너리 탐색 |
| `@vibemud/native-darwin-*` | macOS 바이너리 |
| `@vibemud/native-linux-*-gnu` | glibc Linux 바이너리 |
| `@vibemud/native-win32-*` | Windows `.exe` 바이너리 |

루트 패키지는 현재 OS/CPU의 optional dependency를 찾고, 지정된 네이티브 경로·패키지 안의 바이너리·로컬 Cargo 빌드 결과를 순서대로 확인합니다. 바이너리를 찾지 못한 설치는 실패하게 해, 실행할 수 없는 CLI가 설치된 상태를 피합니다. Windows mod 실행은 `.cmd`에 게임 명령을 전달하지 않습니다.

`VIBEMUD_BIN` 또는 `VIBEMUD_BIN_DIR`로 mod의 네이티브 CLI를 명시할 수 있습니다. 각 npm 명령에는 `MUDCTL_BIN`, `VIBEMUD_RUNTIME_BIN`, `VIBEMUD_HUD_BIN`도 사용할 수 있습니다. 별도 npm prefix를 쓴다면 PATH에 추가하거나 절대 경로를 지정하세요.

게임 데이터는 npm cache가 아닌 사용자 데이터 폴더에 보관합니다. 기본값은 macOS/Linux `~/.vibemud`, Windows `%LOCALAPPDATA%\VibeMUD`입니다. `VIBEMUD_HOME`에 절대 경로를 주면 변경할 수 있습니다. 코딩 파일·프롬프트·대화 내용은 게임 상태로 사용하지 않습니다.

## 개발 검사와 배포 순서

소스 루트에서 빌드한 뒤 확인합니다.

```bash
cargo build --workspace
node npm/scripts/check-release-metadata.js
(cd npm && npm run test:resolve)
node --test npm/test/bridge.test.js
(cd npm && npm pack --dry-run --json | node scripts/check-pack-contents.js)
```

[GitHub Actions](https://github.com/treestar84/vibemud/actions/workflows/package-dry-run.yml)는 플랫폼별 바이너리를 빌드하고 mod 테스트, 패키지 조립, 설치 smoke를 수행합니다. 현재 통과 여부는 해당 워크플로에서 확인하세요. 배포 시에는 플랫폼별 `@vibemud/native-*`를 먼저 공개하고, 모두 확인한 뒤 루트 `vibemud`를 공개합니다. `publish-npm` 워크플로는 수동 실행과 릴리스 태그·빌드 아티팩트 확인을 요구합니다. **저장소 공개만으로 npm 패키지가 배포되지는 않습니다.**

소스와 문제 제보: [treestar84/vibemud](https://github.com/treestar84/vibemud). 사용자 설치·게임 조작은 [루트 README](../README.md)를 참고하세요.
