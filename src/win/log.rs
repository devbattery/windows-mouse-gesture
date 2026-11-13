//! 진단 로그. `--console`로 실행했을 때만 출력한다.
//!
//! 콘솔에서 텍스트를 선택(빠른 편집)하면 출력이 멈추므로, 출력은 전용 스레드가 맡고
//! 다른 스레드는 기다리지 않는 `try_send`로 넘긴다. 가득 차면 로그만 버린다.
//! 파일에는 남기지 않는다(개인정보).

use std::sync::OnceLock;
use std::sync::mpsc::{self, SyncSender};
use std::thread;

static SENDER: OnceLock<SyncSender<(bool, String)>> = OnceLock::new();

/// 출력 스레드를 시작한다. 한 번만 호출한다. 호출하지 않으면 로그는 모두 버려진다.
pub fn start() {
    let (sender, receiver) = mpsc::sync_channel::<(bool, String)>(256);
    let spawned = thread::Builder::new()
        .name("glide-log".into())
        .spawn(move || {
            for (warning, text) in receiver {
                if warning {
                    eprintln!("{text}");
                } else {
                    println!("{text}");
                }
            }
        });
    if spawned.is_ok() {
        let _ = SENDER.set(sender);
    }
}

pub fn info(text: impl FnOnce() -> String) {
    if let Some(sender) = SENDER.get() {
        let _ = sender.try_send((false, text()));
    }
}

pub fn warn(text: impl FnOnce() -> String) {
    if let Some(sender) = SENDER.get() {
        let _ = sender.try_send((true, text()));
    }
}
