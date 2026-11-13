//! 워커 스레드. 훅이 넘긴 일(입력 재전송, 동작 실행, 로그)을 처리한다.
//! 여기서는 시간이 걸려도 마우스 입력이 끊기지 않는다.

use std::sync::mpsc::{self, Receiver, SyncSender};
use std::thread;

use glide_core::gesture::Sequence;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{VK_LEFT, VK_MENU};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

use super::input;

/// 다시 보낼 우클릭의 종류.
#[derive(Clone, Copy, Debug)]
pub enum Injection {
    /// 움직이지 않고 뗌 → 누름+뗌
    Click,
    /// 길게 누름 → 누름만(이후 실제 드래그·뗌은 통과)
    Press,
    /// 다시 보낸 누름보다 실제 뗌이 먼저 옴 → 뗌을 뒤에 보냄
    Release,
}

/// 훅 스레드 → 워커 스레드 메시지. 할당 없이 복사되는 값만 담는다.
/// `target`은 우클릭을 시작한 지점의 최상위 창 핸들 값이다.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Inject {
        kind: Injection,
        target: isize,
    },
    Started,
    Changed(Sequence),
    Finished {
        sequence: Sequence,
        invalid: bool,
        target: isize,
    },
    Cancelled,
}

/// 콘솔 출력 전용 스레드로 보내는 로그 한 줄. `true`면 표준 오류로 쓴다.
type Line = (bool, String);

/// 로그를 별도 스레드로 넘긴다. 콘솔에서 텍스트를 선택(빠른 편집)하면 출력이 멈추는데,
/// 워커가 직접 출력하면 그동안 우클릭 재전송까지 멈추기 때문이다. 가득 차면 로그만 버린다.
struct Log(SyncSender<Line>);

impl Log {
    fn info(&self, text: String) {
        let _ = self.0.try_send((false, text));
    }

    fn warn(&self, text: String) {
        let _ = self.0.try_send((true, text));
    }
}

/// 채널이 닫힐 때까지(훅 스레드 종료) 사건을 처리한다.
pub fn run(events: Receiver<Event>) {
    let (line_tx, line_rx) = mpsc::sync_channel::<Line>(256);
    let printer = thread::Builder::new()
        .name("glide-log".into())
        .spawn(move || {
            for (warning, text) in line_rx {
                if warning {
                    eprintln!("{text}");
                } else {
                    println!("{text}");
                }
            }
        });
    let log = Log(line_tx);
    for event in events {
        match event {
            Event::Inject { kind, target } => {
                // 입력을 먼저 보내고 로그는 그 뒤에 남긴다(프로세스 이름 조회가 재전송을 늦추지 않게).
                let sent = match kind {
                    Injection::Click => input::right_click(),
                    Injection::Press => input::right_down(),
                    Injection::Release => input::right_up(),
                };
                let what = match kind {
                    Injection::Click => "단순 우클릭으로 다시 보냄",
                    Injection::Press => "길게 눌러 원래 우클릭으로 통과",
                    Injection::Release => "뗌 순서를 보정해 다시 보냄",
                };
                if sent {
                    log.info(format!("[우클릭] {what} · {}", app_name(target)));
                } else {
                    log.warn(format!(
                        "[경고] 우클릭을 다시 보내지 못했습니다({what}) · {}",
                        app_name(target)
                    ));
                }
            }
            Event::Started => log.info("[제스처] 시작".into()),
            Event::Changed(sequence) => log.info(format!("[제스처] {}", arrows(sequence))),
            Event::Cancelled => log.info("[제스처] 취소".into()),
            Event::Finished {
                sequence,
                invalid,
                target,
            } => execute(&log, sequence, invalid, target),
        }
    }
    // 남은 로그를 모두 출력한 뒤 끝낸다.
    drop(log);
    if let Ok(printer) = printer {
        let _ = printer.join();
    }
}

fn arrows(sequence: Sequence) -> String {
    sequence
        .as_str()
        .chars()
        .map(|direction| match direction {
            'L' => '←',
            'R' => '→',
            'U' => '↑',
            _ => '↓',
        })
        .map(String::from)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 제스처 하나를 실행한다. M1은 L(뒤로 가기)만 실행하고 나머지는 기록만 한다.
fn execute(log: &Log, sequence: Sequence, invalid: bool, target: isize) {
    let app = app_name(target);
    if invalid {
        log.info(format!(
            "[결과] 너무 긴 제스처라 실행하지 않았습니다 · {app}"
        ));
        return;
    }
    if sequence.is_empty() {
        log.info(format!(
            "[결과] 방향이 분명하지 않아 실행하지 않았습니다 · {app}"
        ));
        return;
    }
    if sequence.as_str() != "L" {
        log.info(format!(
            "[결과] {} 은(는) M1에서 실행하지 않는 제스처입니다(L만 실행) · {app}",
            arrows(sequence)
        ));
        return;
    }
    // 삼킨 우클릭은 창을 활성화하지 않는다. 다른 창에 키가 들어가지 않도록 대상 창을 확인한다.
    if !ensure_foreground(target) {
        log.info(format!(
            "[결과] 제스처를 시작한 창을 활성화하지 못해 실행하지 않았습니다 · {app}"
        ));
        return;
    }
    if input::chord(&[VK_MENU], VK_LEFT, true) {
        log.info(format!("[결과] 뒤로 가기(Alt+←)를 보냈습니다 · {app}"));
    } else {
        log.warn(format!("[경고] 뒤로 가기 키를 보내지 못했습니다 · {app}"));
    }
}

/// 창을 소유한 프로세스의 실행 파일 이름(예: notepad.exe). 호환성 기록용이며,
/// 창 제목은 개인 정보가 담길 수 있어 읽지 않는다. 알 수 없으면 "알 수 없는 앱".
fn app_name(window: isize) -> String {
    const UNKNOWN: &str = "알 수 없는 앱";
    if window == 0 {
        return UNKNOWN.into();
    }
    let mut process_id = 0;
    // SAFETY: 창 핸들 값과 출력 포인터를 넘긴다. 창이 사라졌으면 0을 돌려준다.
    unsafe { GetWindowThreadProcessId(window as _, &mut process_id) };
    if process_id == 0 {
        return UNKNOWN.into();
    }
    // SAFETY: 최소 권한(이름 조회)으로 열고, 사용 후 반드시 닫는다.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if process.is_null() {
            // 관리자 권한 프로세스 등은 열 수 없다.
            return format!("PID {process_id}");
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        );
        CloseHandle(process);
        if ok == 0 {
            return format!("PID {process_id}");
        }
        let path = String::from_utf16_lossy(&buffer[..length as usize]);
        path.rsplit('\\').next().unwrap_or(&path).to_owned()
    }
}

fn foreground_root() -> isize {
    // SAFETY: 조회 함수들이다.
    unsafe {
        let window = GetForegroundWindow();
        if window.is_null() {
            0
        } else {
            GetAncestor(window, GA_ROOT) as isize
        }
    }
}

/// 대상 창이 전면 창이면 true. 아니면 활성화를 한 번 시도하고 결과를 돌려준다.
fn ensure_foreground(target: isize) -> bool {
    if target == 0 {
        return false;
    }
    if foreground_root() == target {
        return true;
    }
    // SAFETY: 창 핸들 값을 그대로 넘긴다. 창이 사라졌으면 실패만 한다.
    unsafe { SetForegroundWindow(target as _) };
    foreground_root() == target
}
