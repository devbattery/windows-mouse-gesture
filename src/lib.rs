//! Glide for Windows 핵심 로직.
//!
//! 운영체제와 무관한 순수 로직(방향 인식·우클릭 상태 머신·지연 통계)만 담는다.
//! Win32 훅·입력 주입은 실행 파일(`src/main.rs`, `src/win/`)에 있다.

pub mod gesture;
pub mod machine;
pub mod stats;
