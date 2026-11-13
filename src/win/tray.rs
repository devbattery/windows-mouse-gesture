//! 알림 영역(트레이) 아이콘.
//!
//! 아이콘 클릭은 UI 제어 창으로 `WM_UI_TRAY` 메시지가 온다(NOTIFYICON_VERSION_4 형식).
//! Explorer가 다시 시작되면 `TaskbarCreated` 브로드캐스트를 받아 아이콘을 다시 등록한다.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_WARNING, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, LoadIconW};

use super::shared::WM_UI_TRAY;

const ICON_ID: u32 = 1;

pub struct Tray {
    window: HWND,
    added: bool,
}

/// UTF-16 고정 길이 배열에 문자열을 복사한다(넘치면 자르고 끝에 0을 둔다).
fn copy_into(target: &mut [u16], text: &str) {
    let limit = target.len() - 1;
    let mut length = 0;
    for (slot, unit) in target.iter_mut().zip(text.encode_utf16().take(limit)) {
        *slot = unit;
        length += 1;
    }
    target[length] = 0;
}

impl Tray {
    pub fn new(window: HWND) -> Self {
        Self {
            window,
            added: false,
        }
    }

    fn data(&self) -> NOTIFYICONDATAW {
        // SAFETY: 0으로 초기화해도 유효한 C 구조체다.
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.window;
        data.uID = ICON_ID;
        data
    }

    /// 아이콘을 등록한다(Explorer 재시작 뒤에도 다시 부른다).
    pub fn add(&mut self, tip: &str) {
        let mut data = self.data();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = WM_UI_TRAY;
        // SAFETY: 시스템 기본 아이콘을 불러온다. 사용자 아이콘 리소스는 설치 프로그램 단계(M3)에서 넣는다.
        data.hIcon = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) };
        copy_into(&mut data.szTip, tip);
        // SAFETY: 이 스레드의 창으로 아이콘을 등록한다.
        unsafe {
            self.added = Shell_NotifyIconW(NIM_ADD, &data) != 0;
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }

    pub fn set_tip(&self, tip: &str) {
        if !self.added {
            return;
        }
        let mut data = self.data();
        data.uFlags = NIF_TIP | NIF_SHOWTIP;
        copy_into(&mut data.szTip, tip);
        // SAFETY: 등록한 아이콘의 도움말만 바꾼다.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    /// 경고 알림(설정 파일 오류 등).
    pub fn warn(&self, title: &str, text: &str) {
        if !self.added {
            return;
        }
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = NIIF_WARNING;
        copy_into(&mut data.szInfoTitle, title);
        copy_into(&mut data.szInfo, text);
        // SAFETY: 등록한 아이콘에 알림을 띄운다.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    pub fn remove(&mut self) {
        if self.added {
            let data = self.data();
            // SAFETY: 등록한 아이콘을 제거한다.
            unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
            self.added = false;
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
    }
}
