//! Glide for Windows 핵심 로직.
//!
//! 운영체제와 무관한 순수 로직(방향 인식·우클릭 상태 머신·설정 검증·단축키 파서·
//! 동작 정의·백업 형식·지연·누름 시간 통계·최근 기록·코어와 설정 창 사이 프로토콜)을 담는다.
//! 코어(`glide-core.exe`)와 설정 창(`glide-settings.exe`)이 함께 쓴다.
//! Win32 API는 파이프 이름에 쓰는 사용자 SID 조회(`ipc::user_sid`)만 여기에 있고,
//! 훅·입력 주입·화면 표시는 코어 실행 파일(`src/win/`)에 있다.

pub mod actions;
pub mod backup;
pub mod gesture;
pub mod history;
pub mod ipc;
pub mod machine;
pub mod settings;
pub mod shortcut;
pub mod stats;
