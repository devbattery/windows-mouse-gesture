//! Glide for Windows 상주 프로세스(M1 프로토타입, 콘솔 실행).

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
