//! SendInput으로 마우스·키 입력을 다시 보낸다.
//!
//! 우리가 보낸 입력은 `dwExtraInfo`에 [`MARK`]를 달아 보낸다. 훅이 이 표식을 보고
//! 자기 입력을 다시 가로채지 않는다(무한 루프 방지).

use std::mem::size_of;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT,
    MapVirtualKeyW, SendInput, VIRTUAL_KEY,
};

/// 우리가 주입한 입력의 표식("GLID").
pub const MARK: usize = 0x474C_4944;

fn mouse(flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MARK,
            },
        },
    }
}

fn key(vk: VIRTUAL_KEY, up: bool, extended: bool) -> INPUT {
    let mut flags = 0;
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    // SAFETY: 단순 값 변환 함수다.
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MARK,
            },
        },
    }
}

/// 입력을 한 번에 보낸다. 모두 들어갔으면 true.
/// 관리자 권한 창이 앞에 있으면 UIPI가 조용히 막을 수 있고, 반환값으로는 알 수 없다.
fn send(inputs: &[INPUT]) -> bool {
    // SAFETY: 슬라이스 길이와 구조체 크기를 정확히 넘긴다.
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    sent as usize == inputs.len()
}

pub fn right_click() -> bool {
    send(&[mouse(MOUSEEVENTF_RIGHTDOWN), mouse(MOUSEEVENTF_RIGHTUP)])
}

pub fn right_down() -> bool {
    send(&[mouse(MOUSEEVENTF_RIGHTDOWN)])
}

pub fn right_up() -> bool {
    send(&[mouse(MOUSEEVENTF_RIGHTUP)])
}

/// 보조 키 + 키 하나를 누르고 뗀다. 예: Alt + ←
/// `extended`는 방향키·Home·End처럼 확장 키보드 영역의 키일 때 true.
pub fn chord(modifiers: &[VIRTUAL_KEY], vk: VIRTUAL_KEY, extended: bool) -> bool {
    let mut inputs = Vec::with_capacity(modifiers.len() * 2 + 2);
    inputs.extend(
        modifiers
            .iter()
            .map(|modifier| key(*modifier, false, false)),
    );
    inputs.push(key(vk, false, extended));
    inputs.push(key(vk, true, extended));
    inputs.extend(
        modifiers
            .iter()
            .rev()
            .map(|modifier| key(*modifier, true, false)),
    );
    send(&inputs)
}
