//! Glide for Windows 상주 프로세스.
//!
//! 콘솔 창 없이 트레이 아이콘으로 실행된다(`--console`이면 진단 로그를 콘솔에 출력).
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    std::process::exit(win::run());

    #[cfg(not(windows))]
    {
        eprintln!("Glide for Windows는 Windows 10/11 전용입니다.");
        std::process::exit(1);
    }
}
