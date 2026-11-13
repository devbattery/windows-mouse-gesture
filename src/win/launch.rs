//! 다른 프로그램(설정 창·메모장·탐색기) 실행.
//!
//! uiAccess로 실행된 코어가 `ShellExecuteW`·`CreateProcess`로 자식 프로세스를 만들면, 자식이
//! 코어의 토큰(uiAccess 표시 포함)을 물려받을 수 있다. 그러면 웹 콘텐츠를 띄우는 설정 창(WebView2)이나
//! 메모장까지 관리자 권한 창을 조작할 수 있게 된다. 이를 막으려고, 이미 떠 있는 바탕 화면 셸(explorer)에게
//! 대신 실행을 부탁한다. 그러면 자식은 explorer의 일반 사용자 토큰으로 시작한다.
//!
//! 방법: ShellWindows에서 바탕 화면 창을 찾아 그 셸 뷰의 `IShellDispatch2::ShellExecute`를 부른다
//! (Raymond Chen, "How can I launch an unelevated process from my elevated process").
//! COM을 단일 스레드 아파트로 초기화한 UI 스레드에서만 부른다.

use std::sync::atomic::Ordering;

use windows::Win32::System::Com::{
    CLSCTX_LOCAL_SERVER, CoCreateInstance, IDispatch, IServiceProvider,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Shell::{
    CSIDL_DESKTOP, IShellBrowser, IShellDispatch2, IShellFolderViewDual, IShellView, IShellWindows,
    SID_STopLevelBrowser, SVGIO_BACKGROUND, SWC_DESKTOP, SWFO_NEEDDISPATCH, ShellWindows,
};
use windows::core::{BSTR, Interface};

use super::log;
use super::shared::UI_ACCESS;

/// 창을 보통 크기로 보여 준다(SW_SHOWNORMAL).
const SHOW_NORMAL: i32 = 1;

/// 파일·프로그램을 연다. `parameters`는 명령줄 인수 문자열이다.
/// 셸을 통한 실행이 실패하면, uiAccess가 아닐 때만 직접 실행한다(권한을 물려줄 일이 없음).
pub fn open(file: &str, parameters: Option<&str>) -> bool {
    match open_through_shell(file, parameters.unwrap_or("")) {
        Ok(()) => true,
        Err(error) => {
            log::warn(|| format!("[실행] 탐색기를 통해 열지 못했습니다: {error}"));
            if UI_ACCESS.load(Ordering::Relaxed) {
                false
            } else {
                open_directly(file, parameters)
            }
        }
    }
}

fn open_through_shell(file: &str, parameters: &str) -> windows::core::Result<()> {
    // SAFETY: COM 인터페이스 호출이다. 각 결과는 windows 크레이트가 참조 계수로 관리한다.
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_LOCAL_SERVER)?;
        let location = VARIANT::from(CSIDL_DESKTOP as i32);
        let root = VARIANT::default();
        let mut window = 0;
        let desktop = windows.FindWindowSW(
            &location,
            &root,
            SWC_DESKTOP,
            &mut window,
            SWFO_NEEDDISPATCH,
        )?;
        let provider: IServiceProvider = desktop.cast()?;
        let browser: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let view: IShellView = browser.QueryActiveShellView()?;
        let background: IDispatch = view.GetItemObject(SVGIO_BACKGROUND)?;
        let folder: IShellFolderViewDual = background.cast()?;
        let shell: IShellDispatch2 = folder.Application()?.cast()?;
        shell.ShellExecute(
            &BSTR::from(file),
            &VARIANT::from(BSTR::from(parameters)),
            &VARIANT::from(BSTR::new()),
            &VARIANT::from(BSTR::from("open")),
            &VARIANT::from(SHOW_NORMAL),
        )
    }
}

fn open_directly(file: &str, parameters: Option<&str>) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let file = wide(file);
    let parameters = parameters.map(wide);
    // SAFETY: 널 종료 문자열로 셸 실행을 요청한다.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            file.as_ptr(),
            parameters.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // 32 이하는 실패 코드다.
    (result as isize) > 32
}
