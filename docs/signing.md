# 코드 서명

## 지금 상태

**공개 릴리스의 실행 파일과 설치 프로그램은 아직 서명돼 있지 않습니다.** 그래서 다음과 같이 동작합니다.

- **관리자 권한 창 지원판:** 설치 프로그램(`GlideSetup-<버전>.exe`)이 설치할 때 **이 PC 전용 인증서**를 만들어 `glide-core.exe` 하나에만 서명합니다. 공개 인증서만 이 PC의 신뢰 루트에 등록하고, 개인키는 바로 지웁니다. 자세한 단계는 README의 설치 절에 있습니다.
- **브라우저 다운로드:** 서명이 없으므로 "흔히 다운로드되지 않는 파일" 경고가 뜰 수 있습니다.
- **설치 프로그램 실행:** "Windows의 PC 보호"(SmartScreen) 창이 뜰 수 있습니다.

## 계획: SignPath Foundation 무료 서명

오픈소스 프로젝트는 [SignPath Foundation](https://signpath.org/)의 무료 코드 서명을 받을 수 있습니다. 승인되면 다음이 달라집니다.

- **서명 대상:** 릴리스 작업이 세 실행 파일 모두에 정식 인증서로 서명합니다.
- **설치 때 인증서:** 설치 프로그램(`installer/glide-setup.ps1`)은 이미 신뢰된 서명이 있으면 이 PC 전용 인증서를 만들지 않습니다(신뢰 루트를 건드리지 않음).
- **스마트 앱 컨트롤 안내:** 릴리스 작업이 `build-installer.ps1 -Signed`로 컴파일하면(세 실행 파일의 서명을 먼저 확인), 설치 프로그램이 서명 전 판을 막는 안내를 하지 않습니다.

### 조건

정확한 조건은 [이용 약관](https://signpath.org/terms)에서 확인합니다.

- OSI 승인 라이선스(MIT, `LICENSE`)이고 공개 저장소여야 합니다.
- 독점 구성 요소가 없어야 합니다.
- 서명 권한이 있는 사람은 모두 SignPath와 GitHub에서 2단계 인증을 써야 합니다.
- 서명은 CI 빌드에서만 합니다. SignPath가 저장소·커밋·빌드 작업 출처를 확인합니다.
- 아래 "코드 서명 정책"을 README나 웹 페이지에 공개해야 합니다.

### 신청 뒤 저장소 설정 (저장소 관리자가 직접)

GitHub 저장소 **Settings → Secrets and variables → Actions**에 다음을 넣습니다. 값은 SignPath 화면에서 확인하고, 이 문서나 커밋에는 적지 않습니다.

| 종류 | 이름 | 내용 |
| --- | --- | --- |
| Variable | `SIGNPATH_ORGANIZATION_ID` | SignPath 조직 ID. 이 값이 있어야 릴리스 작업의 서명 단계가 실행됩니다 |
| Variable | `SIGNPATH_PROJECT_SLUG` | SignPath 프로젝트 슬러그 |
| Variable | `SIGNPATH_SIGNING_POLICY_SLUG` | 서명 정책 슬러그(예. `release-signing`) |
| Secret | `SIGNPATH_API_TOKEN` | SignPath CI 사용자 API 토큰 |

SignPath 프로젝트에도 설정할 것이 있습니다.

- **입력 형식:** 아티팩트 구성은 "zip 안의 PE 파일 3개"로 만듭니다. 대상은 `glide-core.exe`, `glide-core-uiaccess.exe`, `glide-settings.exe`입니다.
- **확인할 정보:** 제품명 `Glide for Windows`(코어)·`Glide 설정`(설정 창)과 버전을 확인하도록 둡니다.
- **빌드 연결:** 신뢰 빌드 시스템으로 GitHub.com을 연결합니다.

### 흐름 (`.github/workflows/release.yml`)

1. `tools/package.ps1`: 빌드하고 uiAccess 매니페스트를 확인합니다.
2. `bin/`을 GitHub artifact로 올립니다.
3. `signpath/github-action-submit-signing-request`가 서명을 요청하고, 결과를 기다려 받습니다.
4. 서명 상태가 `Valid`인지 확인한 뒤 원래 파일과 바꿉니다.
5. `tools/build-installer.ps1 -Signed` → `tools/test-installer.ps1 -Signed` → GitHub Release.

### 설치 프로그램 서명 (아직 구현하지 않음)

위 흐름은 설치 프로그램 안의 실행 파일 3개만 서명합니다. `GlideSetup-<버전>.exe` 자체도 서명해야 스마트 앱 컨트롤이
켜진 PC에서 실행되고 SmartScreen 경고도 줄어듭니다. SignPath는 컴파일하는 러너 밖에서 서명하므로, Inno Setup의
`SignTool`(컴파일 중에 서명 명령을 실행)을 바로 쓸 수 없습니다. 승인된 뒤 다음 순서로 넣습니다.

1. **제거 프로그램:** `SignTool` 없이 `SignedUninstaller=yes`로 컴파일하면, `SignedUninstallerDir`(기본은 출력 폴더)에
   고유한 이름의 제거 프로그램 사본이 생기고 서명하라는 안내가 나옵니다. 이 파일을 SignPath로 서명해 그 자리에 두고
   다시 컴파일하면 서명이 설치 프로그램에 들어갑니다. Inno Setup 판이나 `SetupIconFile`·`VersionInfo` 지시문이 바뀌면
   새 이름의 사본이 생기므로 다시 서명해야 합니다. 이 값을 켜면 설치 중에 쓰는 임시 사본도 서명됩니다.
   끈 채로 설치 프로그램만 서명하면 임시 사본은 서명되지 않아 Defender ASR 같은 보안 프로그램이 막을 수 있습니다(Inno Setup 문서).
2. **설치 프로그램:** 다시 컴파일한 `GlideSetup-<버전>.exe`를 SignPath로 서명하고 `Valid`를 확인한 뒤
   `SHA256SUMS.txt`를 다시 만듭니다.
3. SignPath 프로젝트에 이 두 단계를 위한 아티팩트 구성(PE 파일 1개)과 서명 정책을 추가합니다.

검증할 수 있는 서명 환경이 생기기 전에는 이 단계를 워크플로에 넣지 않습니다.

## 코드 서명 정책 (초안, 승인 전)

> 승인 뒤 README에 옮겨 공개합니다. 승인 전에는 이 문구를 쓰지 않습니다.

- Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).
- 커밋·리뷰·승인 담당: 저장소 소유자 [devbattery](https://github.com/devbattery). 릴리스 서명 요청은 저장소 소유자가 승인합니다.
- 개인정보: 이 프로그램은 새 버전 확인 말고는 어떤 정보도 다른 네트워크 시스템으로 보내지 않습니다.
  새 버전 확인은 하루 한 번 GitHub Releases API(`api.github.com`)에 최신 버전 번호를 묻는 HTTPS 요청이며,
  앱 이름·버전(User-Agent) 외의 정보는 보내지 않습니다. 설정 창에서 끌 수 있습니다(기본 켜짐).
  신청할 때 SignPath에 이 동작을 알리고, 기본값이 정책상 문제 되면 기본 꺼짐으로 바꿉니다.
