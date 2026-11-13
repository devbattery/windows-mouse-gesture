//! 저수준 마우스 훅 스레드.
//!
//! 원칙: 훅 콜백은 "삼킬지 통과시킬지"만 정하고 바로 반환한다. 입력 주입·동작 실행은
//! 워커 스레드로, 궤적·안내 표시는 UI 스레드로 채널을 통해 넘긴다. Windows는 콜백이
//! 시간 제한(LowLevelHooksTimeout)을 여러 번 넘기면 훅을 알림 없이 제거하기 때문이다.
//!
//! 상태 머신·타이머·훅 핸들·예외 판정 캐시는 모두 이 스레드 하나가 소유한다. 콜백과 타이머
//! 프로시저, 스레드 메시지는 같은 스레드의 메시지 루프에서 차례로 처리되므로 잠금이 필요 없다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Sender, SyncSender};
use std::time::{Duration, Instant};

use glide_core::gesture::scale_threshold;
use glide_core::machine::{Command, Input, Machine, Output, Verdict};
use glide_core::stats::LatencyHistogram;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONEAREST, MonitorFromPoint};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThread, GetCurrentThreadId, SetThreadPriority,
    THREAD_PRIORITY_HIGHEST,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GA_ROOT, GetAncestor, GetMessageW, GetWindowThreadProcessId,
    HHOOK, KillTimer, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, SetTimer, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_XBUTTONDOWN, WindowFromPoint,
};

use super::clock;
use super::input::MARK;
use super::shared::{self, HOLD_MS, THRESHOLD, WM_HOOK_CLEAR_EXCLUSIONS, WM_HOOK_EXCLUSION};
use super::ui::{self, UiEvent};
use super::worker::{Event, Injection};

/// 펜·터치에서 만들어진 마우스 메시지의 dwExtraInfo 서명(MI_WP_SIGNATURE). 이런 입력은 건드리지 않는다.
const PEN_TOUCH_MASK: usize = 0xFFFF_FF00;
const PEN_TOUCH_SIGNATURE: usize = 0xFF51_5700;
/// 제스처 중 Esc 확인 주기(ms)
const ESC_POLL_MS: u32 = 16;
/// 훅이 조용히 제거됐을 가능성에 대비한 주기적 재설치 간격(ms)
const REINSTALL_MS: u32 = 60_000;
/// 예외 판정 캐시 최대 크기. 넘으면 비운다.
const MAX_CACHED_PROCESSES: usize = 512;
/// 예외 판정을 믿는 시간. 지나면 다시 판정한다(PID 재사용 대비).
const EXCLUSION_TTL: Duration = Duration::from_secs(30);

/// 훅 콜백 처리 시간. 트레이 상태 줄과 콘솔 통계가 읽는다.
pub static HOOK_LATENCY: LatencyHistogram = LatencyHistogram::new();
/// 채널이 가득 차 버린 사건 수(0이어야 정상). 궤적 점은 세지 않는다.
pub static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);
/// 훅 재설치 횟수
pub static REINSTALLS: AtomicU64 = AtomicU64::new(0);

struct Context {
    machine: Machine,
    hook: HHOOK,
    hold_timer: usize,
    esc_timer: usize,
    /// 제스처를 시작한 지점의 최상위 창과 그 프로세스(동작 실행 대상)
    target: isize,
    target_pid: u32,
    /// 오른쪽 버튼을 누른 화면 좌표(궤적 시작점)
    origin: (i32, i32),
    /// 프로세스 ID → (예외 앱 여부, 판정 시각). 워커가 판정해 스레드 메시지로 채운다.
    exclusions: HashMap<u32, (bool, Instant)>,
    own_pid: u32,
    events: SyncSender<Event>,
    ui: SyncSender<UiEvent>,
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
pub fn run(events: SyncSender<Event>, ui: SyncSender<UiEvent>, ready: Sender<u32>) {
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
            target_pid: 0,
            origin: (0, 0),
            exclusions: HashMap::new(),
            // SAFETY: 인자 없는 조회 함수다.
            own_pid: unsafe { GetCurrentProcessId() },
            events,
            ui,
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
        // 창이 없는 스레드 메시지 중 우리 메시지는 직접 처리한다(타이머는 DispatchMessageW로).
        if message.hwnd.is_null() {
            match message.message {
                WM_HOOK_EXCLUSION => {
                    let (pid, excluded) = (message.wParam as u32, message.lParam != 0);
                    with_context(|context| context.on_exclusion(pid, excluded));
                    continue;
                }
                WM_HOOK_CLEAR_EXCLUSIONS => {
                    with_context(|context| context.exclusions.clear());
                    continue;
                }
                _ => {}
            }
        }
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
            WM_RBUTTONDOWN => self.right_down(info.pt),
            WM_RBUTTONUP => Input::RightUp { x, y },
            WM_MOUSEMOVE => Input::Move { x, y },
            WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN => Input::OtherDown,
            _ => return false,
        };
        let output = self.machine.handle(input);
        self.apply(output);
        // 제스처 중 이동은 궤적으로 그린다(시작 알림 뒤에 보내 순서를 지킨다).
        if message == WM_MOUSEMOVE && self.machine.is_gesturing() {
            self.show(
                UiEvent::Point {
                    x: info.pt.x,
                    y: info.pt.y,
                },
                false,
            );
        }
        output.verdict == Verdict::Swallow
    }

    /// 오른쪽 누름: 제스처를 시작할 수 있는지 정한다.
    fn right_down(&mut self, point: POINT) -> Input {
        let (x, y) = (point.x as f64, point.y as f64);
        let mut allowed = shared::gestures_active() && !modifiers_down();
        if allowed {
            let (root, pid) = window_at(point);
            self.target = root;
            self.target_pid = pid;
            self.origin = (point.x, point.y);
            // 우리 창(트레이 등) 위에서는 제스처를 쓰지 않는다.
            if pid == self.own_pid {
                allowed = false;
            } else if pid != 0 {
                // 판정은 잠깐만 믿는다. PID는 프로세스가 끝나면 다른 프로세스에 다시 쓰이기 때문이다.
                match self.exclusions.get(&pid) {
                    Some((excluded, at)) if at.elapsed() < EXCLUSION_TTL => allowed = !excluded,
                    // 처음 보거나 오래된 판정: 일단 제스처로 시작하고 워커에 판정을 맡긴다(낙관적 시작).
                    _ => self.send(Event::Resolve { pid }),
                }
            }
        }
        let threshold = if allowed {
            scale_threshold(THRESHOLD.load(Ordering::Relaxed), dpi_at(point))
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

    /// 워커가 보낸 예외 판정 결과.
    fn on_exclusion(&mut self, pid: u32, excluded: bool) {
        if self.exclusions.len() >= MAX_CACHED_PROCESSES {
            self.exclusions.clear();
        }
        self.exclusions.insert(pid, (excluded, Instant::now()));
        if excluded && pid == self.target_pid {
            // 판정 대기 중이면 삼켰던 누름을 돌려준다(그 외 상태에서는 상태 머신이 무시한다).
            let output = self.machine.handle(Input::Excluded);
            self.apply(output);
        }
    }

    /// 상태 머신이 요청한 일을 처리한다. 타이머는 여기서, 나머지는 워커·UI로 넘긴다.
    fn apply(&mut self, output: Output) {
        for command in output.commands() {
            match command {
                Command::StartHoldTimer => {
                    self.stop_hold_timer();
                    let hold_ms = HOLD_MS.load(Ordering::Relaxed);
                    // SAFETY: 창 없는 스레드 타이머를 만든다.
                    self.hold_timer = unsafe { SetTimer(null_mut(), 0, hold_ms, Some(hold_proc)) };
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
                Command::GestureStarted => {
                    let (x, y) = self.origin;
                    self.show(UiEvent::Begin { x, y }, true);
                }
                Command::GestureChanged { sequence, invalid } => {
                    self.show(UiEvent::Changed { sequence, invalid }, true)
                }
                Command::GestureFinished { sequence, invalid } => {
                    self.show(UiEvent::Finished { sequence, invalid }, true);
                    self.send(Event::Finished {
                        sequence,
                        invalid,
                        target: self.target,
                        pid: self.target_pid,
                    });
                }
                Command::GestureCancelled => self.show(UiEvent::Cancelled, true),
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

    /// UI 스레드에 표시할 사건을 넘기고 깨운다. 궤적 점(`counted` = false)은 버려져도 세지 않는다.
    fn show(&self, event: UiEvent, counted: bool) {
        if self.ui.try_send(event).is_ok() {
            ui::wake();
        } else if counted {
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

/// 좌표 아래의 최상위 창과 그 프로세스 ID.
/// WindowFromPoint는 다른 스레드의 창에 WM_NCHITTEST를 보내지 않으므로(이 스레드는 창이 없음)
/// 응답 없는 앱 때문에 막히지 않는다. GetWindowThreadProcessId도 메시지를 보내지 않는다.
fn window_at(point: POINT) -> (isize, u32) {
    // SAFETY: 좌표·창 핸들로 조회만 한다.
    unsafe {
        let window = WindowFromPoint(point);
        if window.is_null() {
            return (0, 0);
        }
        let root = GetAncestor(window, GA_ROOT);
        let mut pid = 0;
        GetWindowThreadProcessId(root, &mut pid);
        (root as isize, pid)
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
