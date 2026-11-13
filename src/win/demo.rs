//! `--demo-overlay`: 오버레이가 이 PC에서 제대로 그려지는지 확인하는 진단 기능.
//!
//! 실제 마우스 입력 없이, 주 모니터 가운데에서 왼쪽으로 긋는 견본 제스처(←)를 UI 스레드에
//! 그대로 보내 궤적과 HUD를 그린 뒤 스스로 종료한다. 동작은 실행하지 않는다.

use std::sync::mpsc::SyncSender;
use std::thread;
use std::time::Duration;

use glide_core::gesture::Gesture;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

use super::ui::{self, UiEvent};

fn send(ui: &SyncSender<UiEvent>, event: UiEvent) {
    if ui.send(event).is_ok() {
        ui::wake();
    }
}

pub fn run(ui: SyncSender<UiEvent>) {
    // SAFETY: 화면 크기 조회다(Per-Monitor v2라 물리 픽셀).
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let (x, y) = (width / 2 + 150, height / 2);
    let mut gesture = Gesture::new(x as f64, y as f64, 22.0);
    send(&ui, UiEvent::Begin { x, y });
    for step in 1..=30 {
        // 손떨림처럼 위아래로 조금씩 흔들리며 왼쪽으로 300px 이동
        let point = (x - step * 10, y + (step % 3) * 2);
        gesture.feed(point.0 as f64, point.1 as f64);
        send(
            &ui,
            UiEvent::Point {
                x: point.0,
                y: point.1,
            },
        );
        if step == 3 {
            send(
                &ui,
                UiEvent::Changed {
                    sequence: gesture.sequence(),
                    invalid: false,
                },
            );
        }
        thread::sleep(Duration::from_millis(12));
    }
    // 그리는 중 화면(궤적 + "뒤로 가기" 안내)을 잠시 유지
    thread::sleep(Duration::from_millis(1500));
    send(
        &ui,
        UiEvent::Finished {
            sequence: gesture.sequence(),
            invalid: false,
        },
    );
    // 손을 뗀 뒤 결과 안내(750ms)가 사라질 때까지 기다린 뒤 종료
    thread::sleep(Duration::from_millis(1500));
    super::request_exit();
}
