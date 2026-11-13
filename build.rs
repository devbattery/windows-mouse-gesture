//! 빌드 스크립트
//!
//! 1. 코어 실행 파일에 버전 정보·아이콘 자원을 넣는다. 파일 속성에 제품명과 버전이 보이고, 코드 서명
//!    정책(SignPath)이 실행 파일의 제품명·버전을 확인할 때도 쓴다.
//! 2. `uiaccess` 기능으로 빌드하면 uiAccess 매니페스트를 넣는다.
//!
//! uiAccess="true"인 실행 파일은 (1) 로컬 컴퓨터가 신뢰하는 인증서로 서명되고 (2) Program Files 같은
//! 보안 위치에 있을 때만 시작된다. 조건을 못 갖추면 실행 자체가 실패하므로, 개발용 기본 빌드에는 넣지 않고
//! 설치 패키지(`tools/package.ps1`)만 이 기능으로 따로 빌드한다.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=settings/icons/icon.ico");
    let ui_access = std::env::var_os("CARGO_FEATURE_UIACCESS").is_some();
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !msvc {
        return;
    }

    // 버전 정보·아이콘. 매니페스트는 넣지 않는다(uiAccess 매니페스트는 아래에서 링커가 만든다).
    // 라이브러리·테스트가 아닌 glide-core 실행 파일에만 링크한다.
    let mut resource = tauri_winres::WindowsResource::new();
    resource
        .set_icon("settings/icons/icon.ico")
        .set("ProductName", "Glide for Windows")
        .set("FileDescription", "Glide mouse gestures")
        .set("CompanyName", "devbattery")
        .set(
            "LegalCopyright",
            "Copyright (c) 2026 devbattery. MIT License.",
        )
        .set("OriginalFilename", "glide-core.exe");
    if let Err(error) = resource.compile_for(&["glide-core"]) {
        panic!("코어 실행 파일 자원을 만들지 못했습니다(Windows SDK의 rc.exe 필요): {error}");
    }

    if ui_access {
        // MSVC 링커가 매니페스트를 만들어 실행 파일 안에 넣는다. 실행 권한은 그대로(asInvoker)이고
        // uiAccess만 켠다.
        println!("cargo:rustc-link-arg-bin=glide-core=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=glide-core=/MANIFESTUAC:level='asInvoker' uiAccess='true'"
        );
    }
}
