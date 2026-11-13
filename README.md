# Glide for Windows

크롬 확장 **Glide**의 우클릭 마우스 제스처를 모든 Windows 10/11 앱에서 쓰기 위한 상주 프로그램입니다.

> **현재 단계: M2 코어 v1.** 트레이 아이콘으로 상주하며 동작 23종, 궤적·안내 표시, 앱별 예외,
> 설정 파일을 지원합니다. 설정 화면(Tauri)과 관리자 권한 창 지원(uiAccess)은 M3에서 들어갑니다.

## 동작 방식

- 오른쪽 버튼을 누른 채 그리면 방향(←→↑↓, 최대 12획)을 인식합니다. 인식 규칙은 확장과 같습니다.
- 그리는 동안 화면에 궤적과 하단 안내(실행될 동작·단축키)를 보여 줍니다. 디자인은 확장과 같습니다.
- 그냥 우클릭하면 원래 우클릭(컨텍스트 메뉴)이 그대로 동작합니다.
- **Shift·Ctrl·Alt·Win**을 누른 채 우클릭하면 제스처 없이 원래 우클릭이 됩니다.
- 움직이지 않고 **길게 누르고 있으면**(기본 400ms) 원래 우클릭으로 넘겨줍니다. 탐색기의 우클릭 드래그(복사/이동 메뉴)처럼 우클릭 드래그가 필요할 때 씁니다.
- 제스처 중 **Esc**를 누르면 취소합니다(Esc는 앱에도 전달됩니다).
- 전체 화면 D3D 게임·프레젠테이션 중에는 자동으로 멈춥니다.
- 펜·터치 입력과 다른 프로그램이 보낸 입력은 건드리지 않습니다.

> **크롬 확장 Glide는 꺼 주세요.** 이 프로그램이 브라우저에서도 제스처를 처리합니다(우클릭을 먼저 가로채므로
> 확장은 동작하지 않습니다). 브라우저를 제외하고 확장을 쓰려면 `excludedProcesses`에 `chrome.exe` 등을 넣으세요.

## 요구 사항

- Windows 10 22H2 또는 Windows 11 (x64)
- Rust 1.85 이상 (`stable-x86_64-pc-windows-msvc`)
- Visual Studio 2022 Build Tools의 C++ 빌드 도구와 Windows SDK

## 빌드와 실행

```powershell
cargo build --release
.\target\release\glide-core.exe
```

실행하면 콘솔 창 없이 알림 영역(트레이)에 아이콘이 생깁니다. 아이콘을 누르면 메뉴가 열립니다.

- **제스처 사용**: 잠시 끄고 켜기(다시 실행하면 켜진 상태로 시작)
- **설정 파일 열기** / **설정 폴더 열기**
- 상태 줄: 사용 중·꺼짐·전체 화면 일시 중지, 훅 처리 시간(p99), 훅 재설치 횟수
- **종료**

| 옵션 | 설명 |
| --- | --- |
| `--console` | 진단 로그와 훅 처리 시간 통계(30초마다)를 콘솔에 출력 |
| `--demo-overlay` | 견본 제스처(←)로 궤적·안내 표시를 그려 보고 종료(화면 표시 확인용, 동작은 실행하지 않음) |
| `--help` | 도움말 |

## 설정

`%APPDATA%\Glide\settings.json`. 처음 실행할 때 기본값으로 만들어지고, 저장하면 1초 안에 반영됩니다.
값이 올바르지 않으면 이전 설정을 계속 쓰고 트레이 알림으로 이유를 알려 줍니다(파일은 덮어쓰지 않습니다).

```json
{
  "schemaVersion": 1,
  "enabled": true,
  "threshold": 22,
  "rules": [
    { "gesture": "L", "action": "back" },
    { "gesture": "RD", "action": "keyboard", "shortcut": "Ctrl+Shift+N" }
  ],
  "windows": {
    "holdToPassMs": 400,
    "excludedProcesses": ["mstsc.exe"],
    "autoPauseFullscreen": true
  }
}
```

- `enabled`, `threshold`(12~48), `rules`(최대 64개)는 확장 설정과 형식·검증 규칙이 같습니다. 확장에서 내보낸 백업의 `gestures` 배열을 `rules`에 그대로 붙여 넣을 수 있습니다.
- `threshold`는 96 DPI 기준 px이며 모니터 배율에 맞춰 자동 환산됩니다.
- `excludedProcesses`: 제스처를 쓰지 않을 앱의 실행 파일 이름(경로 없이). 원격 데스크톱·가상 머신·그림 도구·테두리 없는 전체 화면 게임에 권장합니다.
- `keyboard` 동작의 `shortcut`은 확장과 같은 표기(`Ctrl+T`, `Alt+Left`, `Shift+F5`, `Cmd+D` 등)이며, `Cmd`는 Win 키로 보냅니다.

### 동작 목록

| ID | 동작 | Windows에서 보내는 입력 |
| --- | --- | --- |
| `back` / `forward` | 뒤로 / 앞으로 가기 | Alt + ← / Alt + → |
| `reload` / `hardReload` | 새로고침 / 강력 새로고침 | F5 / Ctrl + F5 |
| `newTab` / `newWindow` / `closeTab` | 새 탭 / 새 창 / 현재 탭 닫기 | Ctrl + T / Ctrl + N / Ctrl + W |
| `restoreTab` | 닫은 탭 다시 열기 | Ctrl + Shift + T |
| `previousTab` / `nextTab` | 이전 탭 / 다음 탭 | Ctrl + Shift + Tab / Ctrl + Tab |
| `scrollTop` / `scrollBottom` | 맨 위로 / 맨 아래로 | Ctrl + Home / Ctrl + End |
| `zoomIn` / `zoomOut` / `zoomReset` | 확대 / 축소 / 원래 크기 | Ctrl + = / Ctrl + - / Ctrl + 0 |
| `fullscreen` | 전체 화면 전환 | F11 |
| `minimizeWindow` | 창 최소화 | 창 명령 |
| `maximizeWindow` | 창 최대화 / 복원 | 창 명령 |
| `closeWindow` | 창 닫기 | 닫기 요청(WM_CLOSE) |
| `showDesktop` | 바탕 화면 보기 | Win + D |
| `mediaPlayPause` | 미디어 재생 / 일시정지 | 미디어 키 |
| `duplicateTab` / `muteTab` | 탭 복제 / 탭 음소거 | 공통 단축키가 없어 Windows에서는 실행하지 않음 |

키 입력 동작은 제스처를 시작한 창이 전면 창일 때만 보냅니다(다른 창에 키가 들어가지 않게).
창 동작은 바탕 화면·작업 표시줄에는 실행하지 않습니다.

## 검사

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`tests/gesture_fixtures.rs`·`tests/shortcut_fixtures.rs`는 확장의 `shared.js`로 만든 픽스처와 결과를 비교해,
두 구현이 같은 입력에 같은 제스처·단축키 해석을 내는지 확인합니다. 확장 규칙이 바뀌면 다시 생성합니다.

```powershell
node tools/gen-gesture-fixtures.cjs <확장 저장소>/shared.js
```

## 수동 검증 절차

훅과 오버레이는 실제 마우스 입력이 필요해 자동 테스트로 확인할 수 없습니다. 릴리스 전에 아래를 확인합니다.

- [ ] 단순 우클릭의 컨텍스트 메뉴가 정상: 탐색기, 바탕화면, 메모장, VS Code, Office, 브라우저
- [ ] 탐색기 우클릭 드래그가 길게 누르기로 동작
- [ ] 기본 제스처 10종이 브라우저에서 동작하고, 궤적·안내가 표시됐다가 사라짐
- [ ] Windows 전용 동작(최소화·최대화·창 닫기·바탕 화면 보기·미디어 키)
- [ ] 등록되지 않은 제스처는 손을 떼면 안내가 바로 닫힘, 13획 이상은 "너무 긴 제스처"
- [ ] `excludedProcesses`에 넣은 앱에서는 우클릭이 원래대로 동작
- [ ] 전체 화면 게임에서 자동 일시 중지, 트레이 상태 줄에 표시
- [ ] 트레이 "제스처 사용" 끄기·켜기, "종료" 뒤 트레이 아이콘이 남지 않음
- [ ] settings.json 수정이 1초 안에 반영, 잘못된 값이면 트레이 알림
- [ ] 배율이 다른 모니터(100%·150% 등)에서 같은 체감으로 인식하고, 안내가 그 모니터 하단에 표시
- [ ] 비활성 창 위에서 시작한 제스처가 그 창에서 실행되거나, 실행하지 않고 안내됨
- [ ] 훅 처리 시간 p99 1ms 미만(`--console`의 `[통계]` 또는 트레이 상태 줄), 부하 10분 뒤에도 동작

## 구조

```
src/
  gesture.rs    방향 인식기 (확장 shared.js의 Gesture 이식, 무할당)
  machine.rs    우클릭 상태 머신 (삼킬지·통과시킬지 결정, 순수 로직)
  settings.rs   설정 형식·검증 (확장 validateSettings 이식 + Windows 항목)
  shortcut.rs   단축키 파서 (확장 parseShortcut 이식)
  actions.rs    동작 23종의 실행 방식·라벨·안내 문구
  stats.rs      훅 처리 시간 히스토그램 (원자적 카운터)
  win/
    hook.rs     WH_MOUSE_LL 훅 스레드 (판단만 하고 바로 반환, 예외 판정 캐시)
    worker.rs   입력 재전송·동작 실행·예외 판정·설정 감시·전체 화면 감시
    ui.rs       UI 스레드: 오버레이와 트레이를 돌리는 숨긴 제어 창
    overlay.rs  레이어드 창 + Direct2D 궤적·안내 (바뀐 영역만 갱신)
    tray.rs     알림 영역 아이콘·메뉴·알림
    settings_store.rs  settings.json 읽기·원자적 쓰기·변경 감시
    shared.rs   스레드 간 공유 상태(원자 값·설정 스냅샷)
    input.rs    SendInput (주입 입력에 표식을 달아 훅이 다시 잡지 않게 함)
    log.rs      --console 진단 로그
    demo.rs     --demo-overlay 진단
tests/          확장과의 인식·단축키 결과 비교
fixtures/       확장 코드로 생성한 픽스처
tools/          픽스처 생성 스크립트 (Node)
```

## 알려진 제한

- 관리자 권한으로 실행된 창(작업 관리자 등) 위에서는 동작하지 않습니다. M3에서 uiAccess 서명으로 해결할 예정입니다.
- 삼킨 우클릭은 창을 활성화하지 않습니다. 비활성 창 위에서 시작한 키 입력 제스처는 그 창을 전면으로 가져오지 못하면 실행하지 않고 안내합니다.
- 테두리 없는 전체 화면 게임은 브라우저 전체 화면(F11)과 구분되지 않아 자동으로 멈추지 않습니다. `excludedProcesses`에 넣어 주세요.
- 오른쪽 버튼을 누른 채 판정 전에 다른 버튼을 누르면(로커 제스처), 앱은 다른 버튼 누름을 먼저 받고 오른쪽 누름을 나중에 받습니다.
- 트레이 아이콘은 아직 Windows 기본 아이콘입니다(설치 프로그램 단계에서 교체).
- 서명되지 않은 실행 파일이라 SmartScreen 경고가 뜰 수 있습니다.

## 개인정보

- 네트워크 통신을 하지 않습니다.
- 키보드 훅을 설치하지 않습니다. Esc는 제스처 중에만 키 상태를 조회합니다.
- 로그는 `--console`일 때 콘솔에만 출력하고 파일로 남기지 않습니다. 창 제목은 읽지 않고, 실행 파일 이름만 다룹니다.

## 라이선스

정하지 않았습니다(공개 배포 단계에서 결정).
