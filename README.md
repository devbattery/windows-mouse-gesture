# Glide for Windows

크롬 확장 **Glide**의 우클릭 마우스 제스처를 모든 Windows 10/11 앱에서 쓰기 위한 상주 프로그램입니다.

> **현재 단계: M5 공개 배포.** 트레이에 상주하는 코어(`glide-core.exe`)와 설정 창(`glide-settings.exe`, Tauri)으로
> 이루어집니다. [Releases](https://github.com/devbattery/windows-mouse-gesture/releases)의 설치 프로그램으로 설치하면
> 관리자 권한 창(작업 관리자 등) 위에서도 제스처가 동작합니다. 실제 마우스로 할 확인 항목은
> [최종 시험 순서](#최종-시험-순서)에 모았습니다. 변경 내용은 [CHANGELOG](CHANGELOG.md)에 있습니다.

## 동작 방식

- 오른쪽 버튼을 누른 채 그리면 방향(←→↑↓, 최대 12획)을 인식합니다. 인식 규칙은 확장과 같습니다.
- 그리는 동안 화면에 궤적과 하단 안내(실행될 동작·단축키)를 보여 줍니다. 디자인은 확장과 같습니다.
- 그냥 우클릭하면 원래 우클릭(컨텍스트 메뉴)이 그대로 동작합니다.
- **Shift·Ctrl·Alt·Win**을 누른 채 우클릭하면 제스처 없이 원래 우클릭이 됩니다.
- 움직이지 않고 **길게 누르고 있으면**(기본 400ms) 원래 우클릭으로 넘겨줍니다. 탐색기의 우클릭 드래그(복사/이동 메뉴)처럼 우클릭 드래그가 필요할 때 씁니다.
- 제스처 중 **Esc**를 누르면 취소합니다(Esc는 앱에도 전달됩니다).
- 전체 화면 D3D 게임·프레젠테이션 중에는 자동으로 멈춥니다.
- 펜·터치 입력과 다른 프로그램이 보낸 입력은 건드리지 않습니다. 크롬 원격 데스크톱처럼 원격 제어로 쓸 때는
  설정 창에서 **원격 제어로 들어온 입력에서도 제스처 사용**을 켭니다.
- 화면 잠금·UAC 확인 창·절전·원격 연결 전환·모니터 변경이 일어나면 진행 중인 제스처를 실행 없이 취소하고
  훅을 다시 설치합니다(손을 뗀 신호를 놓쳐 궤적이 남거나 우클릭이 이상해지는 것을 막음).

> **크롬 확장 Glide는 꺼 주세요.** 이 프로그램이 브라우저에서도 제스처를 처리합니다(우클릭을 먼저 가로채므로
> 확장은 동작하지 않습니다). 브라우저를 제외하고 확장을 쓰려면 예외 앱에 `chrome.exe` 등을 넣으세요.

## 설치

요구 사항: Windows 10 22H2 또는 Windows 11(x64). 설정 창에 WebView2 런타임이 필요합니다(Windows 11과 최신 Windows 10에는 기본 포함).

1. [Releases](https://github.com/devbattery/windows-mouse-gesture/releases)에서 `GlideSetup-<버전>.exe`를 받습니다.
   같은 곳의 `SHA256SUMS.txt`와 비교하려면 PowerShell에서 `(Get-FileHash GlideSetup-<버전>.exe).Hash.ToLower()`를 실행합니다
   (`Get-FileHash`는 대문자로 출력하고 `SHA256SUMS.txt`는 소문자입니다).
2. 받은 파일을 실행합니다. UAC 확인 창이 한 번 뜹니다.
3. "추가 설정" 두 가지(기본은 둘 다 켬)를 확인하고 설치합니다. 마지막 화면의 **Glide 시작**을 켜 두면 바로 트레이에 나타납니다.

**경고 창이 뜰 때.** 아직 코드 서명 전이라 다음 창이 뜰 수 있습니다([코드 서명 계획](docs/signing.md)).

- 브라우저: "흔히 다운로드되지 않는 파일" → **유지**
- "Windows의 PC 보호" → **추가 정보 → 실행**

설치 프로그램이 풀어 놓은 파일에는 다운로드 표시가 붙지 않으므로, 설치한 뒤에는 실행할 때 경고가 뜨지 않습니다.

설치 프로그램이 하는 일(스크립트 [installer/glide.iss](installer/glide.iss), [installer/glide-setup.ps1](installer/glide-setup.ps1)에서 직접 확인할 수 있습니다):

1. [스마트 앱 컨트롤](#스마트-앱-컨트롤)이 막을 판이면 아무것도 바꾸지 않고 멈춥니다.
2. 설치 폴더에서 실행 중인 Glide를 종료합니다. 다른 위치에서 실행 중이면 멈추고 안내합니다.
3. `C:\Program Files\Glide\`에 실행 파일을 복사합니다. 관리자 권한 창 지원 조건 때문에 폴더는 바꿀 수 없습니다.
4. **관리자 권한 창에서도 제스처 사용**(기본 켬)
   - **이 PC 전용 코드 서명 인증서**를 만들어 `glide-core.exe`에만 서명합니다. 공개 인증서만 로컬 컴퓨터의 신뢰 루트에 등록하고 **개인키는 바로 지웁니다**. 그래서 이후에는 누구도 이 인증서로 다른 프로그램에 서명할 수 없습니다.
   - 실행 파일이 이미 SignPath Foundation 인증서로 서명돼 있으면(정식 서명 릴리스) 이 단계를 건너뜁니다.
   - 다시 설치하면 새 인증서로 바꾸고, 이전 인증서는 신뢰 루트에서 지웁니다.
   - 서명하지 못하면 관리자 창 미지원판으로 바꿔 설치를 마치고 알려 줍니다.
   - 끄면 인증서를 만들지 않고 관리자 창 미지원판을 설치합니다(이전 인증서는 지웁니다).
5. **로그인할 때 자동으로 시작**(기본 켬): `HKLM\...\Run`에 등록합니다. 이 PC의 **모든 사용자**에게 적용됩니다.
6. 모든 사용자 시작 메뉴에 "Glide 설정" 바로 가기를 만들고, "설정 → 앱 → 설치된 앱"에 **Glide for Windows**로 등록합니다.
7. 0.5.x를 `install.cmd`로 설치했던 PC라면 그 등록·인증서·자동 시작(`HKCU\...\Run`)·바로 가기를 정리합니다. 따로 제거하지 않아도 됩니다.
   그때 `-NoUiAccess`·`-NoAutoStart`로 설치했다면, 처음 넘어올 때 그 선택을 "추가 설정"의 기본값으로 이어받습니다.
8. 마지막 화면의 **Glide 시작**은 관리자 권한이 아닌 원래 사용자 권한으로 Glide를 시작합니다.

설치 기록은 `%TEMP%\Setup Log <날짜> #<번호>.txt`에 남습니다. 문제를 알려 줄 때 첨부해 주세요.

**조용한 설치**(명령 프롬프트·PowerShell):

```
GlideSetup-<버전>.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART
```

| 추가 인자 | 설명 |
| --- | --- |
| `/MERGETASKS="!uiaccess"` | 인증서를 만들지 않고 관리자 창 미지원판을 설치 |
| `/MERGETASKS="!autostart"` | 로그인 시 자동 시작하지 않음(둘 다 끄려면 `"!uiaccess,!autostart"`) |
| `/NOLAUNCH` | 설치한 뒤 Glide를 시작하지 않음 |
| `/SKIPSACCHECK` | 스마트 앱 컨트롤 확인을 건너뜀(판단이 틀렸다고 확신할 때만) |
| `/LOG="경로"` | 설치 기록을 지정한 파일에 씀 |

다시 설치할 때는 지난번에 고른 추가 설정을 그대로 씁니다.

**업데이트:** 새 버전 설치 프로그램을 받아 실행하면 덮어씁니다. 설정(`%APPDATA%\Glide`)은 그대로입니다.
0.5.4부터 Glide가 하루 한 번 GitHub에 최신 버전 번호를 물어보고, 새 버전이 있으면 트레이 알림을 띄웁니다
(버전마다 한 번). 알림이나 트레이 메뉴의 **새 버전 받기**를 누르면 Release 페이지가 열립니다. 받기와 설치는
직접 합니다. 서명 전이라 앱이 스스로 내려받아 설치하지는 않습니다.
설정 창 **Glide 정보 → 새 버전 자동 확인**에서 끌 수 있고, 끄면 Glide는 인터넷에 접속하지 않습니다.
`glide-core.exe --check-update`로 지금 바로 확인할 수도 있습니다.

**제거:** "설정 → 앱 → 설치된 앱 → Glide for Windows → 제거"를 누릅니다(UAC 확인 1회). 다음을 지웁니다.

- 인증서(지문과 주체 이름을 확인한 것만)
- 설치 폴더, "앱 및 기능" 등록
- 자동 시작, 바로 가기

설정(`%APPDATA%\Glide`)은 남깁니다. 조용히 제거하려면 `"C:\Program Files\Glide\unins000.exe" /VERYSILENT`를 실행합니다.

> 자체 서명 uiAccess가 최신 Windows(특히 Windows 11 24H2)에서 거부된다는 보고가 있습니다. 설치 뒤 Glide가
> 시작되지 않거나 트레이 상태 줄이 "관리자 권한 창: 지원 안 함"이면, 설치 프로그램을 다시 실행해
> **관리자 권한 창에서도 제스처 사용**을 끄고 설치해 주세요.
> 단, 아래의 스마트 앱 컨트롤이 켜져 있으면 그렇게 설치해도 실행되지 않습니다.

### 스마트 앱 컨트롤

Windows 11의 **스마트 앱 컨트롤**이 켜져 있으면(주로 Windows를 새로 설치한 PC) 지금 배포판은 실행되지 않습니다.

- 스마트 앱 컨트롤은 Microsoft 신뢰 루트 프로그램에 속한 인증 기관의 인증서로 서명했거나 Microsoft 평판이 있는 앱만
  실행합니다. 설치 스크립트가 만드는 이 PC 전용 인증서는 인정하지 않고, 설정 창(`glide-settings.exe`)은 서명이 없습니다.
- 설치 프로그램은 시작할 때 이를 확인하고, 켜져 있으면 **아무것도 바꾸지 않고 멈춥니다**.
  설치 프로그램 자체도 아직 서명이 없으므로, 실행하는 것부터 막힐 수도 있습니다.
- 지금 쓰려면 "Windows 보안 → 앱 및 브라우저 컨트롤 → 스마트 앱 컨트롤 설정"에서 **끔**으로 바꾼 뒤 설치합니다.
  Windows 보호 기능 하나를 끄는 것이니 판단해서 선택해 주세요. 예전에는 한 번 끄면 Windows를 다시 설치해야 켤 수
  있었지만, 2026년 4월 업데이트 이후 빌드(24H2·25H2의 .8116 이상)는 다시 켤 수 있다고 알려져 있습니다.
- 평가 모드(설정 화면의 "평가")에서는 막지 않으므로 그대로 설치하면 됩니다.
- 근본 해결은 공식 코드 서명(SignPath Foundation)입니다([docs/signing.md](docs/signing.md)). 서명판이 나오면 이 절을 고칩니다.
- 판단이 틀렸다고 확신할 때만 `GlideSetup-<버전>.exe /SKIPSACCHECK`로 확인을 건너뛸 수 있습니다.

### 소스에서 빌드해 설치

```powershell
winget install --id Rustlang.Rustup
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
winget install --id JRSoftware.InnoSetup.7 -e   # 설치 프로그램 컴파일러(7.1.1 이상, 현재 사용자에게 설치)
# 새 터미널에서
git clone https://github.com/devbattery/windows-mouse-gesture.git
cd windows-mouse-gesture
powershell -NoProfile -ExecutionPolicy Bypass -File tools\package.ps1           # dist\glide-windows-<버전>\ 생성
powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-installer.ps1   # dist\GlideSetup-<버전>.exe 생성
dist\GlideSetup-<버전>.exe                                                        # UAC 확인 1회
```

- Build Tools의 "C++ 빌드 도구" 워크로드에는 MSVC 컴파일러와 Windows SDK가 들어 있습니다. SDK의 `rc.exe`·`mt.exe`가 필요합니다.
- `package.ps1`은 `Cargo.lock`의 의존성 버전 그대로(`--locked`) 빌드합니다. Rust가 없으면 위 설치 방법을 안내하고 멈춥니다.
- `build-installer.ps1`은 [Inno Setup 7](https://jrsoftware.org/isinfo.php)의 `ISCC.exe`로 설치 프로그램과 `dist\SHA256SUMS.txt`를 만듭니다.
  `ISCC.exe`는 `-IsccPath` → 환경 변수 `INNO_SETUP_ISCC` → `%LOCALAPPDATA%\Programs\Inno Setup 7` → Program Files 순서로 찾고,
  없으면 설치 방법을 안내하고 멈춥니다. 7.1.1보다 오래된 판이면 컴파일이 멈춥니다.
- 배포용 설치 프로그램에는 의존 라이브러리 라이선스 고지(`THIRD-PARTY-NOTICES.txt`)가 꼭 들어가야 해서 [cargo-about](https://github.com/EmbarkStudios/cargo-about)이 필요합니다
  (`cargo install cargo-about --version 0.9.3 --locked --features cli`). 이 PC에 설치만 할 때는 없어도 경고만 하고 넘어갑니다.
- `tools\test-installer.ps1`은 설치·다시 설치·제거를 실제로 해 보는 CI 전용 확인 스크립트입니다. 신뢰 루트와 Program Files를
  바꾸므로 GitHub Actions 밖에서는 실행되지 않습니다.

## 설정 창

트레이 메뉴 **설정 열기** 또는 시작 메뉴 **Glide 설정**으로 엽니다. 크롬 확장의 설정 화면과 같은 디자인입니다.

- 상태: Glide 실행 여부, 관리자 권한 창 지원 여부, 훅 처리 시간(p99). 꺼져 있으면 **Glide 시작** 버튼
- 제스처 목록·녹화(설정 창 안에서는 코어가 제스처를 가로채지 않습니다), 인식 감도
- Windows 동작: 길게 누르기 시간, 전체 화면 자동 일시 중지, 원격 제어 입력 허용, 제스처를 쓰지 않을 앱과
  **실행 중인 앱에서 고르기**(지금 창이 열린 앱을 "실행 파일 이름 · 창 제목"으로 보여 주고 눌러서 추가. 창 제목은 저장하지 않음),
  **많이 넣는 앱**(원격 데스크톱·가상 머신 창) 버튼
- 일상 사용 점검: Glide가 켜진 뒤의 **최근 우클릭 50건**(시각·앱 실행 파일 이름·결과)과 각 줄의 **예외에 추가**,
  **길게 누르기 기준 추천**(제스처를 시작하기까지 걸린 시간의 95%에 여유를 더한 값, 제스처 20회 이상부터),
  **진단 정보 복사**(문제를 알릴 때 붙여 넣는 요약. 앱 이름은 고를 때만 넣음). 기록은 메모리에만 있습니다.
- 백업: **내보내기**(앱 형식), **확장용 내보내기**(창 최소화처럼 Windows에서만 되는 동작을 빼서 확장이 읽을 수 있게),
  **불러오기**(확장 백업도 가능, Windows에서 실행할 수 없는 탭 복제·음소거는 건너뛰고 알려 줌)

설정 파일을 쓰는 것은 코어 하나뿐입니다. 설정 창은 코어에 저장을 요청하고(named pipe), 코어가 다시 검증한 뒤
저장합니다. 그래서 Glide가 꺼져 있으면 설정 창은 보기만 할 수 있습니다.

## 개발 빌드와 실행

```powershell
cargo build --release --workspace
.\target\release\glide-core.exe            # 트레이에 상주
.\target\release\glide-settings.exe        # 설정 창(같은 폴더의 코어와만 연결)
```

개발 빌드는 uiAccess가 없어 관리자 권한 창에서는 동작하지 않습니다(서명·Program Files 설치가 필요).

| 코어 옵션 | 설명 |
| --- | --- |
| `--console` | 진단 로그와 훅 처리 시간 통계(30초마다)를 콘솔에 출력 |
| `--demo-overlay` | 견본 제스처(←)로 궤적·안내 표시를 그려 보고 종료(화면 표시 확인용, 동작은 실행하지 않음) |
| `--help` | 도움말 |

트레이 메뉴: 제스처 사용(잠시 끄기), **제스처를 쓰지 않을 앱**(지금 창이 열린 앱을 최근에 앞에 있던 창부터 보여 주고, 체크된 앱이 예외 앱입니다. 누르면 바로 넣거나 빼고 알림으로 알려 줍니다), 설정 열기, 설정 폴더 열기, 상태 줄(사용 여부·훅 p99·재설치 횟수·관리자 권한 창 지원), 종료.

### 요구 사항

- Windows 10 22H2 또는 Windows 11 (x64), WebView2 런타임(Windows 11 기본 포함)
- Rust 1.85 이상 (`stable-x86_64-pc-windows-msvc`)
- Visual Studio 2022 Build Tools의 C++ 빌드 도구와 Windows SDK
- Node.js(픽스처·아이콘 생성 스크립트만)

## 설정 파일

`%APPDATA%\Glide\settings.json`. 처음 실행할 때 기본값으로 만들어집니다. 설정 창 대신 직접 고쳐도 1초 안에 반영되고,
값이 올바르지 않으면 이전 설정을 계속 쓰며 트레이 알림과 설정 창으로 이유를 알려 줍니다(파일은 덮어쓰지 않습니다).

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
    "autoPauseFullscreen": true,
    "allowInjectedInput": false,
    "checkUpdates": true
  }
}
```

- `enabled`, `threshold`(12~48), `rules`(최대 64개)는 확장 설정과 형식·검증 규칙이 같습니다.
- `threshold`는 96 DPI 기준 px이며 모니터 배율에 맞춰 자동 환산됩니다.
- `excludedProcesses`: 제스처를 쓰지 않을 앱의 실행 파일 이름(경로 없이).
- `allowInjectedInput`: 다른 프로그램이 주입한 마우스 입력(원격 제어 프로그램 등)도 제스처로 해석합니다. 기본은 꺼짐입니다.
  켜면 자동 클릭 도구가 보내는 우클릭도 지연·변환될 수 있습니다.
- `checkUpdates`: 하루 한 번 GitHub에 새 버전을 확인합니다. 기본은 켜짐이고, 값이 없는 이전 설정 파일도 켜짐으로 읽습니다([개인정보](#개인정보)).
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
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`tests/`는 확장의 `shared.js`로 만든 픽스처와 결과를 비교해, 두 구현이 같은 입력에 같은 제스처·단축키·백업 해석을
내는지 확인합니다. 확장 규칙이 바뀌면 다시 생성합니다. 파이프 왕복(요청 4종, 거부, 서버 확인)과 진단 응답 크기는 코어의 단위 테스트입니다.

```powershell
node tools/gen-gesture-fixtures.cjs <확장 저장소>/shared.js   # fixtures/*.json
node tools/make-icon.cjs                                      # settings/icons/icon.ico
```

설정 창의 `settings/ui/shared.js`는 확장 원본의 사본입니다. 확장이 바뀌면 다시 복사합니다.

## 최종 시험 순서

훅·오버레이·uiAccess는 실제 마우스와 설치가 필요해 자동 테스트로 다 확인할 수 없습니다. M1~M4의 수동 게이트를
한곳에 모았습니다. PC 앞에서 실제 마우스로, 위에서부터 차례로 확인합니다. 문제가 생기면 설정 창의
**진단 정보 복사**(필요하면 "최근 앱 이름도 넣기")로 상황을 붙여 주세요.

**0. 설치(M5 게이트: GitHub에서 받아 막힘없이 설치)**
- [ ] 다른 PC에서 [Releases](https://github.com/devbattery/windows-mouse-gesture/releases)의 `GlideSetup-<버전>.exe`를 받고, `Get-FileHash`가 `SHA256SUMS.txt`와 같음
- [ ] 설치 프로그램 실행 → UAC 1회 → 마지막 화면 **Glide 시작**으로 Glide가 트레이에 나타남(이미 설치된 PC는 덮어 설치, 이전 인증서는 자동 정리)
- [ ] 0.5.x를 `install.cmd`로 설치했던 PC: 설치된 앱 목록에 Glide for Windows가 한 줄만 있고, `certlm.msc` 신뢰 루트의 "Glide Local UIAccess Signing"이 1개, 다시 로그인하면 Glide가 한 번만 시작됨
- [ ] 스마트 앱 컨트롤이 켜진 PC(있으면): 설치가 안내와 함께 멈추고, 인증서·설치 폴더·자동 시작이 생기지 않음
- [ ] 트레이 상태 줄 "관리자 권한 창: 지원", 설정 창 상태 칸 "관리자 권한 창 지원: 켜짐"
- [ ] 시작 메뉴 "Glide 설정"으로 설정 창을 열 때 SmartScreen 경고가 뜨지 않음
- [ ] `C:\Program Files\Glide\glide-core.exe` 속성 → 자세히에 "Glide for Windows"와 버전이 보임
- [ ] 설정 → 앱 → 설치된 앱에 "Glide for Windows"(버전·게시자 devbattery)가 보임
- [ ] 설정 창 **Glide 정보**: 버전이 맞고, "새 버전 확인"은 브라우저로 Releases를, "라이선스"·"오픈소스 고지"는 메모장으로 파일을 엶
- [ ] 새 버전 알림(0.5.4 이후 버전이 나왔을 때): Glide 시작 2분쯤 뒤 트레이 알림이 한 번 뜨고, 알림·트레이 메뉴 "새 버전 받기"가
      Release 페이지를 엶. 설정 창 정보 칸에 "새 버전 x.y.z가 있습니다"가 보임. "새 버전 자동 확인"을 끄고 저장하면 다시 묻지 않음
- [ ] (선택) 소스에서 빌드: `git clone` → `tools\package.ps1` → `tools\build-installer.ps1` → `dist\GlideSetup-<버전>.exe`

**1. 우클릭 호환(M1 게이트)**
- [ ] 단순 우클릭의 컨텍스트 메뉴가 정상: 탐색기, 바탕화면, 메모장, VS Code, Office, 브라우저
- [ ] 탐색기 우클릭 드래그가 길게 누르기로 동작(복사·이동 메뉴)
- [ ] 설정 창 상태 칸의 훅 처리 시간 p99 1ms 미만, 10분쯤 평소처럼 쓴 뒤에도 동작

**2. 기능(M2 게이트)**
- [ ] 기본 제스처 10종이 브라우저에서 동작하고, 궤적·안내가 표시됐다가 사라짐
- [ ] Windows 전용 동작(최소화·최대화·창 닫기·바탕 화면 보기·미디어 키)
- [ ] 등록되지 않은 제스처는 손을 떼면 안내가 바로 닫힘, 13획 이상은 "너무 긴 제스처"
- [ ] 예외 앱에서는 우클릭이 원래대로 동작
- [ ] 트레이 "제스처 사용" 끄기·켜기, "종료" 뒤 트레이 아이콘이 남지 않음

**3. 설정 창과 관리자 창(M3 게이트)**
- [ ] 작업 관리자(관리자 권한 창) 위에서 제스처 동작
- [ ] 설정 창: 저장 즉시 반영, 제스처 녹화(코어가 가로채지 않음), 확장 백업 불러오기, 확장용 내보내기를 확장이 불러옴
- [ ] 설정 창을 닫은 뒤 `msedgewebview2.exe`가 남지 않음

**4. 일상 사용(M4 게이트: 입력 유실·오실행·훅 끊김 0건)**
- [ ] 제스처 중 `Win+L`(잠금) → 다시 로그인: 궤적이 남지 않고, 다음 우클릭이 정상
- [ ] 제스처 중 UAC 확인 창이 뜨는 상황(예: 관리자 권한 프로그램 실행) → 궤적이 사라지고 다음 우클릭이 정상
- [ ] 절전 → 복귀 뒤 제스처 동작(설정 창 진단의 "초기화" 횟수가 늘어남)
- [ ] 배율이 다른 모니터(100%·150% 등)에서 같은 체감으로 인식하고, 안내가 그 모니터 하단에 표시
- [ ] 모니터를 연결·해제하거나 해상도를 바꾼 뒤 제스처 동작
- [ ] 원격 데스크톱 연결(`mstsc.exe`) 창: 예외에 넣지 않으면 제스처가 이 PC 쪽에서 실행됨을 확인하고, 불편하면 **많이 넣는 앱**에서 추가
- [ ] 크롬 원격 데스크톱으로 이 PC에 접속: **원격 제어 입력** 끔 → 제스처 없음, 켬 → 제스처 동작
- [ ] 평소 쓰는 게임: 전체 화면이면 자동 일시 중지, 창 모드·테두리 없는 창이면 게임을 켠 채 **실행 중인 앱에서 고르기**로 예외에 넣어 우클릭이 원래대로
- [ ] 게임을 켠 채 트레이 아이콘 우클릭 → **제스처를 쓰지 않을 앱**에서 게임을 누르면 알림이 뜨고 바로 우클릭이 원래대로, 한 번 더 누르면 다시 제스처 동작
- [ ] 설정 창을 열어 둔 채 트레이에서 예외를 바꾸면 몇 초 안에 설정 창 목록에도 반영됨
- [ ] 예외에 넣은 게임에서 1분 넘게 쉬었다가 처음 누른 우클릭도 바로 동작(늦거나 사라지지 않음)
- [ ] 며칠 쓰고 나서 **일상 사용 점검**의 최근 기록에서 "취소됨"·"실행하지 못함"이 반복되는 앱이 있는지 확인
- [ ] 제스처를 20회 이상 쓴 뒤 **길게 누르기 기준 추천**을 확인하고, 다르면 적용

**5. 제거**
- [ ] 설정 → 앱 → 설치된 앱 → Glide for Windows → 제거: UAC 1회 뒤 제거 완료 창이 뜸(실행 중이던 Glide도 종료됨)
- [ ] `C:\Program Files\Glide` 폴더와 설치된 앱 목록의 항목이 사라짐
- [ ] 신뢰 루트(`certlm.msc`)에 "Glide Local UIAccess Signing" 인증서가 남지 않음, 트레이 아이콘·자동 시작·시작 메뉴 바로 가기도 없음
- [ ] 다시 설치해 평소처럼 사용

## 문제 앱 목록

일상 사용에서 문제가 생길 수 있는 경우와 대응입니다. 새로 찾은 앱은 여기에 더합니다.

| 경우 | 증상 | 대응 |
| --- | --- | --- |
| 원격 데스크톱·가상 머신 창(`mstsc.exe`, `msrdc.exe`, `vmconnect.exe`, `virtualboxvm.exe`) | 제스처가 원격 쪽이 아니라 이 PC의 그 창에 실행됨(창 닫기 제스처면 원격 창이 닫힘) | 설정 창 **많이 넣는 앱**에서 예외 추가 |
| 원격 제어로 이 PC에 접속(크롬 원격 데스크톱 등) | 원격 제어 프로그램이 입력을 주입하므로 기본 설정에서는 제스처가 없음 | **원격 제어 입력에서도 제스처 사용** 켜기 |
| 테두리 없는 창 모드 게임 | 전체 화면으로 알아차리지 못해 우클릭이 늦거나 제스처로 바뀜 | 게임을 켠 채 트레이 메뉴 **제스처를 쓰지 않을 앱**에서 게임 선택(또는 설정 창 **실행 중인 앱에서 고르기**) |
| 오른쪽 드래그를 쓰는 앱(3D·지도·그림 도구 등) | 바로 드래그하면 제스처로 인식 | 잠시 누르고 있다가 드래그(길게 누르기)하거나 예외 앱에 추가 |
| 자동 클릭 도구 | 원격 제어 입력을 켜면 그 도구의 우클릭도 제스처 판정을 거침 | 원격 제어 입력을 끄기 |
| 관리자 권한 창 | 미지원판(**관리자 권한 창에서도 제스처 사용**을 끄고 설치)에서는 제스처가 없음 | 그 항목을 켜고 다시 설치 |

## 구조

```
src/
  gesture.rs    방향 인식기 (확장 shared.js의 Gesture 이식, 무할당)
  machine.rs    우클릭 상태 머신 (삼킬지·통과시킬지 결정, 순수 로직)
  settings.rs   설정 형식·검증 (확장 validateSettings 이식 + Windows 항목)
  shortcut.rs   단축키 파서 (확장 parseShortcut 이식)
  actions.rs    동작 23종의 실행 방식·라벨·안내 문구, 설정 창 동작 목록
  backup.rs     백업 파일 가져오기·내보내기 (확장 형식 호환)
  ipc.rs        코어↔설정 창 파이프 프로토콜과 클라이언트
  stats.rs      훅 처리 시간·누름 시간 히스토그램(원자적 카운터), 길게 누르기 추천값
  history.rs    최근 우클릭 기록 50건 (메모리에만)
  apps.rs       창이 열린 앱 목록 (예외 앱 고르기: 설정 창·트레이 메뉴 공용)
  win/
    hook.rs     WH_MOUSE_LL 훅 스레드 (판단만 하고 바로 반환, 예외 판정 캐시, 보안 데스크톱 감지)
    worker.rs   입력 재전송·동작 실행·예외 판정·설정 감시·저장·전체 화면 감시·최근 기록
    diagnostics.rs  일상 사용 점검 응답(기록·누름 통계·OS·모니터·훅 통계)
    pipe.rs     설정 창 파이프 서버 (현재 사용자만, 같은 폴더의 설정 창만)
    launch.rs   탐색기를 통한 프로그램 실행 (uiAccess 권한을 물려주지 않음)
    ui.rs       UI 스레드: 오버레이와 트레이를 돌리는 숨긴 제어 창, 잠금·절전·디스플레이 알림
    overlay.rs  레이어드 창 + Direct2D 궤적·안내 (바뀐 영역만 갱신)
    tray.rs     알림 영역 아이콘·메뉴·알림
    settings_store.rs  settings.json 읽기·원자적 쓰기·변경 감시
    shared.rs   스레드 간 공유 상태(원자 값·설정 스냅샷)
    input.rs    SendInput (주입 입력에 표식을 달아 훅이 다시 잡지 않게 함)
    log.rs / demo.rs / clock.rs  진단 로그, --demo-overlay, 고해상도 시계
build.rs        코어 버전 정보·아이콘 자원, uiaccess 기능일 때 uiAccess 매니페스트
settings/       설정 창(Tauri 2): src/main.rs(명령), ui/(확장 설정 화면 기반 정적 파일)
installer/      glide.iss(Inno Setup 7 설치 프로그램), glide-setup.ps1(인증서·프로세스 종료·이전 설치 정리 보조 스크립트)
tools/          package.ps1(빌드·패키지), build-installer.ps1(설치 프로그램·SHA256SUMS), install-inno-setup.ps1·test-installer.ps1(CI 컴파일러 설치·설치 확인), about.hbs(라이선스 고지 템플릿), 픽스처·아이콘 생성 스크립트(Node)
about.toml      라이선스 고지 설정(대상·허용 라이선스, cargo-about)
assets/         아이콘 PNG(확장과 같은 그림)
.github/        CI(검사·패키지), 릴리스(태그 → GitHub Release, SignPath 서명 준비), 문제 앱 보고 양식
docs/signing.md 코드 서명 현황과 SignPath 계획
```

### 릴리스 내는 법

1. `Cargo.toml`·`settings/Cargo.toml`·`settings/tauri.conf.json`의 버전을 올리고, `CHANGELOG.md`에 그 버전 절을 씁니다.
2. main에 push하고 CI가 통과하는지 확인합니다.
3. `git tag v<버전>` → `git push origin v<버전>`을 실행하면 릴리스 작업이 검사·빌드·(설정돼 있으면) 서명을 하고 Release를 만듭니다.
   태그와 `Cargo.toml` 버전이 다르면 멈춥니다.

## 알려진 제한

- uiAccess는 이 PC 전용 자체 서명에 기댑니다. SignPath 서명이 승인되면 설치 때 인증서를 만들지 않게 됩니다([docs/signing.md](docs/signing.md)).
- 설정 파일은 사용자 권한으로 쓸 수 있으므로, 같은 사용자 권한의 다른 프로그램이 규칙을 바꿀 수 있습니다.
  파이프의 "같은 폴더의 설정 창만 허용"은 보안 경계가 아니라 보조 장치입니다. 단축키 동작은 키 조합 하나만 보냅니다.
- 삼킨 우클릭은 창을 활성화하지 않습니다. 비활성 창 위에서 시작한 키 입력 제스처는 그 창을 전면으로 가져오지 못하면 실행하지 않고 안내합니다.
- 테두리 없는 전체 화면 게임은 브라우저 전체 화면(F11)과 구분되지 않아 자동으로 멈추지 않습니다. 게임을 켠 채 **실행 중인 앱에서 고르기**로 예외 앱에 넣어 주세요.
- 오른쪽 버튼을 누른 채 판정 전에 다른 버튼을 누르면(로커 제스처), 앱은 다른 버튼 누름을 먼저 받고 오른쪽 누름을 나중에 받습니다.
- 궤적은 제스처를 시작한 모니터에만 그립니다. 다른 모니터로 넘어간 부분은 보이지 않지만 방향 인식은 그대로 됩니다.
- 화면 잠금·UAC 확인 창 직전에 누른 우클릭은 실행 없이 취소됩니다(Glide가 판정 중이던 누름은 앱에 전달되지 않음).
- 서명 전이라 다운로드·첫 실행 때 SmartScreen 경고가 뜰 수 있습니다([설치](#설치) 참고).
- 서명 전이라 스마트 앱 컨트롤이 켜진 Windows 11에서는 실행되지 않습니다([스마트 앱 컨트롤](#스마트-앱-컨트롤)).
  설치 프로그램(`GlideSetup-<버전>.exe`)도 아직 서명이 없습니다. 서명 계획은 [docs/signing.md](docs/signing.md)에 있습니다.
- 자동 시작은 이 PC의 모든 사용자에게 적용됩니다(`HKLM\...\Run`). 한 사용자만 끄려면 설정 → 앱 → 시작 프로그램에서
  끄는데, 모든 사용자 항목이라 관리자 권한이 필요할 수 있습니다. 모두 끄려면 설치 프로그램을 다시 실행해
  **로그인할 때 자동으로 시작**을 끄고 설치합니다.
- 설치 프로그램을 "관리자 권한으로 실행"으로 시작하면, 마지막 화면의 **Glide 시작**도 관리자 권한으로 Glide를 시작합니다.
  보통처럼 더블클릭으로 실행해 주세요. 그렇게 시작했다면 트레이에서 종료한 뒤 `C:\Program Files\Glide\glide-core.exe`를
  실행하거나 다시 로그인하면 됩니다(다음 로그인부터는 정상).

## 개인정보

- 네트워크 통신은 **새 버전 확인** 하나뿐입니다(기본 켜짐, 설정 창에서 끌 수 있음).
  - 하루 한 번 `https://api.github.com/repos/devbattery/windows-mouse-gesture/releases/latest`에 GET 요청을 보냅니다.
  - 보내는 것은 일반 HTTPS 요청과 User-Agent(`Glide-for-Windows/<버전>`)뿐이고, 설정·사용 기록·개인정보는 보내지 않습니다.
    GitHub에는 요청한 PC의 IP 주소가 보입니다.
  - 응답에서는 버전 번호만 읽고, 결과는 메모리에만 둡니다. 앱이 파일을 내려받거나 설치하지 않습니다.
- 설정 창(WebView)은 CSP로 외부 연결을 막습니다. 버튼으로 여는 Release 페이지는 기본 브라우저가 엽니다.
- 키보드 훅을 설치하지 않습니다. Esc는 제스처 중에만 키 상태를 조회합니다.
- 로그는 `--console`일 때 콘솔에만 출력하고 파일로 남기지 않습니다. 창 제목은 읽지 않고, 실행 파일 이름만 다룹니다.
- 일상 사용 점검의 최근 기록·누름 시간 통계는 메모리에만 있고 Glide를 끄면 사라집니다. 좌표와 창 제목은 기록하지 않습니다.

## 라이선스

[MIT](LICENSE). 보안 문제는 [SECURITY.md](SECURITY.md)의 방법으로 알려 주세요.
