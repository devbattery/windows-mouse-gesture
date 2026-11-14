//! 알림 영역(트레이) 아이콘.
//!
//! 아이콘 클릭은 UI 제어 창으로 `WM_UI_TRAY` 메시지가 온다(NOTIFYICON_VERSION_4 형식).
//! Explorer가 다시 시작되면 `TaskbarCreated` 브로드캐스트를 받아 아이콘을 다시 등록한다.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, GetSystemMetrics, HICON, IDI_APPLICATION,
    LR_DEFAULTCOLOR, LoadIconW, SM_CXSMICON,
};

use super::shared::WM_UI_TRAY;

const ICON_ID: u32 = 1;

/// Glide 아이콘(크롬 확장과 같은 그림). 실행 파일에 넣어 별도 파일 없이 쓴다.
const ICON_16: &[u8] = include_bytes!("../../assets/16.png");
const ICON_32: &[u8] = include_bytes!("../../assets/32.png");

pub struct Tray {
    window: HWND,
    added: bool,
    /// 만든 아이콘(없으면 Windows 기본 아이콘을 쓴다). 끝날 때 해제한다.
    icon: HICON,
}

/// PNG 바이트로 알림 영역 크기의 아이콘을 만든다(Windows Vista 이후 PNG 아이콘 자원 지원).
fn load_icon() -> HICON {
    // SAFETY: 시스템 지표 조회와, 정적 PNG 바이트로 아이콘을 만드는 호출이다.
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let png = if size > 16 { ICON_32 } else { ICON_16 };
        // 0x0003_0000: 아이콘 자원 형식 버전 3.0
        CreateIconFromResourceEx(
            png.as_ptr(),
            png.len() as u32,
            1,
            0x0003_0000,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
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
            icon: load_icon(),
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
        data.hIcon = if self.icon.is_null() {
            // SAFETY: Glide 아이콘을 만들지 못했으면 시스템 기본 아이콘을 불러온다.
            unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) }
        } else {
            self.icon
        };
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
        self.notify(title, text, NIIF_WARNING);
    }

    /// 정보 알림(새 버전 등). 사용자가 알림을 누르면 `NIN_BALLOONUSERCLICK`이 온다.
    pub fn info(&self, title: &str, text: &str) {
        self.notify(title, text, NIIF_INFO);
    }

    fn notify(&self, title: &str, text: &str, icon: u32) {
        if !self.added {
            return;
        }
        let mut data = self.data();
        data.uFlags = NIF_INFO;
        data.dwInfoFlags = icon;
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
        if !self.icon.is_null() {
            // SAFETY: load_icon이 만든 아이콘을 트레이에서 뺀 뒤 해제한다.
            unsafe { DestroyIcon(self.icon) };
        }
    }
}
