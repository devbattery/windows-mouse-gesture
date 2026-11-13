//! 저수준 마우스 훅 스레드.
//!
//! 원칙: 훅 콜백은 "삼킬지 통과시킬지"만 정하고 바로 반환한다. 입력 주입·동작 실행·
//! 로그는 채널로 워커 스레드에 넘긴다. Windows는 콜백이 시간 제한(LowLevelHooksTimeout)을
//! 여러 번 넘기면 훅을 알림 없이 제거하기 때문이다.
//!
//! 상태 머신·타이머·훅 핸들은 모두 이 스레드 하나가 소유한다. 콜백과 타이머 프로시저는
//! 같은 스레드의 메시지 루프에서 차례로 호출되므로 잠금이 필요 없다.

use std::cell::RefCell;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, SyncSender};

use glide_core::gesture::scale_threshold;
use glide_core::machine::{Command, Input, Machine, Output, Verdict};
use glide_core::stats::LatencyHistogram;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GA_ROOT, GetAncestor, GetMessageW, HHOOK, KillTimer,
    LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx,
    WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_XBUTTONDOWN, WindowFromPoint,
};

use super::Config;
use super::clock;
use super::input::MARK;
use super::worker::{Event, Injection};

/// 펜·터치에서 만들어진 마우스 메시지의 dwExtraInfo 서명(MI_WP_SIGNATURE). 이런 입력은 건드리지 않는다.
const PEN_TOUCH_MASK: usize = 0xFFFF_FF00;
const PEN_TOUCH_SIGNATURE: usize = 0xFF51_5700;
/// 제스처 중 Esc 확인 주기(ms)
const ESC_POLL_MS: u32 = 16;
/// 훅이 조용히 제거됐을 가능성에 대비한 주기적 재설치 간격(ms)
const REINSTALL_MS: u32 = 60_000;

/// 훅 콜백 처리 시간. 메인 스레드가 주기적으로 읽어 출력한다.
pub static HOOK_LATENCY: LatencyHistogram = LatencyHistogram::new();
/// 워커 채널이 가득 차 버린 사건 수(0이어야 정상)
pub static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);
/// 훅 재설치 횟수
pub static REINSTALLS: AtomicU64 = AtomicU64::new(0);

struct Context {
    machine: Machine,
    hook: HHOOK,
    hold_timer: usize,
    esc_timer: usize,
    /// 제스처를 시작한 지점의 최상위 창(동작을 실행할 대상)
    target: isize,
    events: SyncSender<Event>,
    config: Config,
}

thread_local! {
    static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) };
}

/// 이 스레드의 컨텍스트로 `f`를 실행한다. 이미 빌려 쓰는 중이면(재진입) 아무것도 하지 않는다.
fn with_context<R>(f: impl FnOnce(&mut Context) -> R) -> Option<R> {
    CONTEXT.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => guard.as_mut().map(f),
        Err(_) => None,
    })
}

fn install() -> HHOOK {
    // SAFETY: 이 실행 파일의 모듈 핸들과 정적 콜백을 넘긴다. 저수준 훅은 DLL 주입 없이 이 스레드에서 호출된다.
    unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(mouse_proc),
            GetModuleHandleW(null_mut()),
            0,
        )
    }
}

/// 훅 스레드 본체. 준비되면 `ready`로 스레드 ID를(실패하면 0을) 보내고, WM_QUIT를 받을 때까지 돈다.
pub fn run(config: Config, events: SyncSender<Event>, ready: Sender<u32>) {
    clock::init();
    // SAFETY: 현재 스레드 의사 핸들에 대한 우선순위 변경이다.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
    let hook = install();
    if hook.is_null() {
        let _ = ready.send(0);
        return;
    }
    CONTEXT.with(|cell| {
        *cell.borrow_mut() = Some(Context {
            machine: Machine::default(),
            hook,
            hold_timer: 0,
            esc_timer: 0,
            target: 0,
            events,
            config,
        })
    });
    // SAFETY: 창 없는 스레드 타이머. WM_TIMER가 이 스레드 큐로 오고 DispatchMessageW가 프로시저를 부른다.
    let reinstall = unsafe { SetTimer(null_mut(), 0, REINSTALL_MS, Some(reinstall_proc)) };
    // SAFETY: 인자 없는 조회 함수다.
    let _ = ready.send(unsafe { GetCurrentThreadId() });

    // SAFETY: MSG는 0으로 초기화해도 유효한 C 구조체다.
    let mut message: MSG = unsafe { std::mem::zeroed() };
    // GetMessageW는 WM_QUIT면 0, 오류면 -1을 돌려준다.
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        // SAFETY: 방금 받은 메시지를 그대로 넘긴다.
        unsafe { DispatchMessageW(&message) };
    }

    // SAFETY: 이 스레드가 만든 타이머와 훅을 정리한다.
    unsafe { KillTimer(null_mut(), reinstall) };
    CONTEXT.with(|cell| {
        if let Some(mut context) = cell.borrow_mut().take() {
            context.stop_hold_timer();
            context.stop_esc_poll();
            unsafe { UnhookWindowsHookEx(context.hook) };
        }
    });
}

/// 저수준 마우스 훅 콜백.
unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        // SAFETY: 받은 인자를 그대로 다음 훅에 넘긴다.
        return unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) };
    }
    let started = clock::now();
    // SAFETY: code >= 0이면 lparam은 이 호출 동안 유효한 MSLLHOOKSTRUCT를 가리킨다(Win32 문서).
    let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
    let swallow = with_context(|context| context.on_mouse(wparam as u32, info)).unwrap_or(false);
    HOOK_LATENCY.record(clock::micros_since(started));
    if swallow {
        // 0이 아닌 값을 돌려주면 이 입력은 다음 훅과 앱에 전달되지 않는다.
        1
    } else {
        // SAFETY: 받은 인자를 그대로 다음 훅에 넘긴다.
        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }
}

impl Context {
    /// 마우스 사건 하나를 판정한다. true면 삼킨다.
    fn on_mouse(&mut self, message: u32, info: &MSLLHOOKSTRUCT) -> bool {
        if info.flags & LLMHF_INJECTED != 0 {
            // 우리가 다시 보낸 누름이 훅을 통과했는지 상태 머신에 알린다.
            if info.dwExtraInfo == MARK && message == WM_RBUTTONDOWN {
                let output = self.machine.handle(Input::OwnRightDownSeen);
                self.apply(output);
            }
            // 다른 프로그램이 주입한 입력도 제스처로 해석하지 않는다.
            return false;
        }
        if info.dwExtraInfo & PEN_TOUCH_MASK == PEN_TOUCH_SIGNATURE {
            return false;
        }
        // 가장 흔한 경우(대기 중 커서 이동)는 상태 머신까지 가지 않고 바로 통과시킨다.
        if message == WM_MOUSEMOVE && self.machine.is_idle() {
            return false;
        }
        let x = info.pt.x as f64;
        let y = info.pt.y as f64;
        let input = match message {
            WM_RBUTTONDOWN => {
                let allowed = !modifiers_down();
                let threshold = if allowed {
                    self.target = root_window_at(info.pt);
                    scale_threshold(self.config.threshold, dpi_at(info.pt))
                } else {
                    0.0
                };
                Input::RightDown {
                    x,
                    y,
                    allowed,
                    threshold,
                }
            }
            WM_RBUTTONUP => Input::RightUp { x, y },
            WM_MOUSEMOVE => Input::Move { x, y },
            WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => Input::OtherDown,
            _ => return false,
        };
        let output = self.machine.handle(input);
        self.apply(output);
        output.verdict == Verdict::Swallow
    }

    /// 상태 머신이 요청한 일을 처리한다. 타이머는 여기서, 나머지는 워커로 넘긴다.
    fn apply(&mut self, output: Output) {
        for command in output.commands() {
            match command {
                Command::StartHoldTimer => {
                    self.stop_hold_timer();
                    // SAFETY: 창 없는 스레드 타이머를 만든다.
                    self.hold_timer =
                        unsafe { SetTimer(null_mut(), 0, self.config.hold_ms, Some(hold_proc)) };
                }
                Command::StopHoldTimer => self.stop_hold_timer(),
                Command::StartEscPoll => {
                    self.stop_esc_poll();
                    // 제스처 전에 눌렸던 기록(최하위 비트)을 비워 둔다.
                    escape_pressed();
                    // SAFETY: 창 없는 스레드 타이머를 만든다.
                    self.esc_timer =
                        unsafe { SetTimer(null_mut(), 0, ESC_POLL_MS, Some(esc_proc)) };
                }
                Command::StopEscPoll => self.stop_esc_poll(),
                Command::InjectRightClick => self.inject(Injection::Click),
                Command::InjectRightDown => self.inject(Injection::Press),
                Command::InjectRightUp => self.inject(Injection::Release),
                Command::GestureStarted => self.send(Event::Started),
                Command::GestureChanged(sequence) => self.send(Event::Changed(sequence)),
                Command::GestureFinished { sequence, invalid } => self.send(Event::Finished {
                    sequence,
                    invalid,
                    target: self.target,
                }),
                Command::GestureCancelled => self.send(Event::Cancelled),
            }
        }
    }

    fn inject(&self, kind: Injection) {
        self.send(Event::Inject {
            kind,
            target: self.target,
        });
    }

    fn send(&self, event: Event) {
        // 훅 안에서는 기다리지 않는다. 채널이 가득 차면 버리고 횟수만 센다.
        if self.events.try_send(event).is_err() {
            DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn stop_hold_timer(&mut self) {
        if self.hold_timer != 0 {
            // SAFETY: 이 스레드가 만든 타이머 ID다.
            unsafe { KillTimer(null_mut(), self.hold_timer) };
            self.hold_timer = 0;
        }
    }

    fn stop_esc_poll(&mut self) {
        if self.esc_timer != 0 {
            // SAFETY: 이 스레드가 만든 타이머 ID다.
            unsafe { KillTimer(null_mut(), self.esc_timer) };
            self.esc_timer = 0;
        }
    }
}

/// 판정 대기 시간이 지남 → 길게 누르기로 보고 원래 우클릭을 돌려준다.
unsafe extern "system" fn hold_proc(_: HWND, _: u32, id: usize, _: u32) {
    with_context(|context| {
        if context.hold_timer == id {
            context.stop_hold_timer();
            let output = context.machine.handle(Input::HoldTimeout);
            context.apply(output);
        } else {
            // 이미 교체된 타이머
            // SAFETY: 이 스레드의 타이머 ID다.
            unsafe { KillTimer(null_mut(), id) };
        }
    });
}

/// 제스처 중 Esc가 눌렸는지 확인한다. 키보드 훅을 설치하지 않기 위해 상태를 조회하는 방식을 쓴다.
/// Esc 자체는 앱에도 전달된다.
unsafe extern "system" fn esc_proc(_: HWND, _: u32, _: usize, _: u32) {
    if !escape_pressed() {
        return;
    }
    with_context(|context| {
        let output = context.machine.handle(Input::Cancel);
        context.apply(output);
    });
}

/// 훅을 주기적으로 다시 설치한다. 새 훅을 먼저 설치하고 옛 훅을 제거해 공백을 없앤다.
/// 두 호출 사이에 메시지를 처리하지 않으므로 같은 입력이 두 번 처리되지 않고, 그래서 상태와
/// 관계없이 실행해도 안전하다. 제스처 도중 훅이 제거돼 상태 머신이 대기 상태로 돌아오지
/// 못한 경우에도 재설치되어야 하므로 대기 상태를 조건으로 두지 않는다. 남은 상태는 다음
/// 오른쪽 누름에서 상태 머신이 정리한다.
unsafe extern "system" fn reinstall_proc(_: HWND, _: u32, _: usize, _: u32) {
    with_context(|context| {
        let fresh = install();
        if !fresh.is_null() {
            // SAFETY: 이 스레드가 설치한 이전 훅 핸들이다.
            unsafe { UnhookWindowsHookEx(context.hook) };
            context.hook = fresh;
            REINSTALLS.fetch_add(1, Ordering::Relaxed);
        }
    });
}

/// Esc가 지금 눌려 있거나, 마지막 조회 이후 눌렸다 떼어졌으면 true.
/// 최하위 비트("마지막 조회 이후 눌림")로 폴링 간격(16ms)보다 짧은 탭도 잡는다. 이 비트는 다른
/// 프로세스의 조회와 공유돼 놓칠 수 있으므로 최선의 노력이다. 제스처 시작 때 한 번 불러 비워 둔다.
fn escape_pressed() -> bool {
    // SAFETY: 키 상태 조회 함수다. 최상위 비트(음수) = 눌린 상태, 최하위 비트 = 조회 이후 눌림.
    let state = unsafe { GetAsyncKeyState(VK_ESCAPE as i32) };
    state < 0 || state & 1 != 0
}

/// Shift·Ctrl·Alt·Win 중 하나라도 눌려 있으면 제스처 없이 원래 우클릭을 쓴다(확장과 같은 우회 규칙).
fn modifiers_down() -> bool {
    [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN]
        .iter()
        // SAFETY: 키 상태 조회 함수다. 최상위 비트가 1이면(음수) 눌린 상태다.
        .any(|key| unsafe { GetAsyncKeyState(*key as i32) } < 0)
}

fn root_window_at(point: POINT) -> isize {
    // SAFETY: 좌표로 창을 찾는 조회 함수다. 다른 창에 메시지를 보내지 않는다.
    unsafe {
        let window = WindowFromPoint(point);
        if window.is_null() {
            0
        } else {
            GetAncestor(window, GA_ROOT) as isize
        }
    }
}

/// 좌표가 있는 모니터의 유효 DPI. 실패하면 96(100%)으로 본다.
fn dpi_at(point: POINT) -> u32 {
    let mut dpi_x = 96;
    let mut dpi_y = 96;
    // SAFETY: 모니터 핸들과 출력 포인터를 넘긴다.
    let result = unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y)
    };
    if result < 0 { 96 } else { dpi_x }
}
