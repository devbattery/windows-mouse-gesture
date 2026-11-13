//! 빌드 스크립트: `uiaccess` 기능으로 빌드하면 코어 실행 파일에 uiAccess 매니페스트를 넣는다.
//!
//! uiAccess="true"인 실행 파일은 (1) 로컬 컴퓨터가 신뢰하는 인증서로 서명되고 (2) Program Files 같은
//! 보안 위치에 있을 때만 시작된다. 조건을 못 갖추면 실행 자체가 실패하므로, 개발용 기본 빌드에는 넣지 않고
//! 설치 패키지(`tools/package.ps1`)만 이 기능으로 따로 빌드한다.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let ui_access = std::env::var_os("CARGO_FEATURE_UIACCESS").is_some();
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if ui_access && msvc {
        // MSVC 링커가 매니페스트를 만들어 실행 파일 안에 넣는다. 실행 권한은 그대로(asInvoker)이고
        // uiAccess만 켠다. 라이브러리·테스트가 아닌 glide-core 실행 파일에만 적용한다.
        println!("cargo:rustc-link-arg-bin=glide-core=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=glide-core=/MANIFESTUAC:level='asInvoker' uiAccess='true'"
        );
    }
}
