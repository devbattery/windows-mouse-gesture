//! 지금 창이 열린 앱 목록(예외 앱 고르기용). 설정 창의 "실행 중인 앱에서 고르기"와 트레이 메뉴가 함께 쓴다.
//!
//! - 코어는 우클릭한 창의 **최상위 창 프로세스**로 예외를 판정한다(`src/win/hook.rs` `window_at`).
//!   그래서 여기서도 최상위 창의 프로세스 이름을 쓰고, 목록에서 고른 이름이 실제 판정과 일치하게 한다.
//! - 순서는 창 겹침 순서(최근에 앞에 있던 창부터)다. 게임에서 빠져나와 목록을 열면 그 게임이 위쪽에 온다.
//! - 창 제목은 고를 때 알아보기 위한 것이다. 저장·진단·전송하지 않는다.

use crate::ipc;

/// 목록 최대 개수
pub const MAX_RUNNING_APPS: usize = 100;
/// 창 제목 최대 글자 수
pub const MAX_TITLE_CHARS: usize = 80;
/// 스토어(UWP) 앱 창을 대신 소유하는 프로세스
const STORE_APP_HOST: &str = "applicationframehost.exe";

/// 창이 열린 앱 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningApp {
    /// 실행 파일 이름(소문자, 예외 목록 형식)
    pub name: String,
    /// 그 앱의 첫 창 제목(잘라낸 값)
    pub title: String,
}

/// (실행 파일 이름, 창 제목) 목록을 정리한다. 받은 순서(창 겹침 순서)를 유지한다.
/// - 이름은 코어 비교 규칙과 같게 소문자로 바꾸고, 같은 실행 파일은 첫 창 하나로 합친다.
/// - Glide 자신은 뺀다(코어는 자기 창에서 제스처를 쓰지 않으므로 넣을 필요가 없다).
pub fn running_app_list(windows: Vec<(String, String)>) -> Vec<RunningApp> {
    let mut apps: Vec<RunningApp> = Vec::new();
    for (name, title) in windows {
        if apps.len() >= MAX_RUNNING_APPS {
            break;
        }
        let name = name.trim().to_lowercase();
        let title = title.trim();
        if name.is_empty()
            || title.is_empty()
            || name == ipc::CORE_EXE
            || name == ipc::SETTINGS_EXE
            || apps.iter().any(|known| known.name == name)
        {
            continue;
        }
        let mut short = truncate(title, MAX_TITLE_CHARS);
        // 스토어 앱 창은 모두 ApplicationFrameHost.exe가 소유한다. 이 이름을 넣으면 스토어 앱 전체가
        // 예외가 되므로, 창 제목 하나만 보고 고르지 않게 알린다.
        if name == STORE_APP_HOST {
            short = format!("스토어 앱 전체(예: {short})");
        }
        apps.push(RunningApp { name, title: short });
    }
    apps
}

/// 글자 수로 자르고, 잘랐으면 말줄임표를 붙인다.
fn truncate(text: &str, max_chars: usize) -> String {
    let mut short: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        short.push('…');
    }
    short
}

/// 트레이 메뉴 항목 문구: `game.exe · 창 제목`. 제목은 더 짧게 자르고, `&`는 메뉴 단축키로 해석되지 않게 `&&`로 바꾼다.
pub fn menu_text(app: &RunningApp, max_title_chars: usize) -> String {
    format!("{} · {}", app.name, truncate(&app.title, max_title_chars)).replace('&', "&&")
}

/// 지금 창이 열린 앱 목록.
#[cfg(windows)]
pub fn running_apps() -> Vec<RunningApp> {
    running_app_list(top_level_windows())
}

/// 작업 표시줄에 보이는 최상위 창의 (실행 파일 이름, 제목) 목록. 창 겹침 순서(위에 있는 창부터)다.
#[cfg(windows)]
fn top_level_windows() -> Vec<(String, String)> {
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW,
    };

    /// 창 하나를 살펴 조건에 맞으면 목록에 넣는다. 열거는 항상 계속한다.
    unsafe extern "system" fn visit(window: HWND, param: LPARAM) -> windows_sys::core::BOOL {
        // SAFETY: param은 아래 EnumWindows 호출 동안만 살아 있는 Vec을 가리킨다(같은 스레드, 동기 호출).
        let found = unsafe { &mut *(param as *mut Vec<(u32, String)>) };
        // SAFETY: 열거가 넘겨준 창 핸들로 조회만 한다. 창이 그사이 사라지면 조회가 실패할 뿐이다.
        unsafe {
            // 보이는 창, 소유 창이 없는 창(대화상자·팝업 제외), 도구 창이 아닌 창
            if IsWindowVisible(window) == 0
                || !GetWindow(window, GW_OWNER).is_null()
                || (GetWindowLongW(window, GWL_EXSTYLE) as u32) & WS_EX_TOOLWINDOW != 0
            {
                return 1;
            }
            // 가려진 창(일시 중단된 스토어 앱, 다른 가상 데스크톱의 창)은 지금 보이는 앱이 아니다.
            let mut cloaked: u32 = 0;
            let result = DwmGetWindowAttribute(
                window,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                size_of::<u32>() as u32,
            );
            if result >= 0 && cloaked != 0 {
                return 1;
            }
            // 다른 프로세스의 창 제목은 메시지 없이 읽으므로, 응답 없는 앱이 있어도 멈추지 않는다.
            let mut buffer = [0u16; 256];
            let length = GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32);
            if length <= 0 {
                return 1;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(window, &mut pid);
            if pid != 0 {
                found.push((pid, String::from_utf16_lossy(&buffer[..length as usize])));
            }
        }
        1
    }

    let mut found: Vec<(u32, String)> = Vec::new();
    // SAFETY: 콜백은 이 호출 안에서만 불리고, found는 호출이 끝날 때까지 살아 있다.
    unsafe { EnumWindows(Some(visit), &mut found as *mut _ as LPARAM) };
    found
        .into_iter()
        .filter_map(|(pid, title)| {
            // 이름 조회는 최소 권한(제한 조회)이다. 읽지 못하는 프로세스는 예외에 넣을 수도 없으니 뺀다.
            let path = ipc::process_path(pid)?;
            Some((path.file_name()?.to_string_lossy().into_owned(), title))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(name: &str, title: &str) -> (String, String) {
        (name.to_string(), title.to_string())
    }

    fn app(name: &str, title: &str) -> RunningApp {
        RunningApp {
            name: name.into(),
            title: title.into(),
        }
    }

    #[test]
    fn running_apps_keep_window_order_merge_and_skip_glide() {
        let list = running_app_list(vec![
            pair("Game.EXE", "My Game"),
            pair("explorer.exe", "문서"),
            pair("game.exe", "My Game - 두 번째 창"),
            pair("glide-settings.exe", "Glide 설정"),
            pair("glide-core.exe", "Glide"),
            pair("notitle.exe", "   "),
            pair("", "이름 없음"),
        ]);
        assert_eq!(
            list,
            [app("game.exe", "My Game"), app("explorer.exe", "문서")]
        );
    }

    #[test]
    fn running_app_titles_and_count_are_limited() {
        let long = "가".repeat(MAX_TITLE_CHARS + 5);
        let list = running_app_list(vec![pair("a.exe", &long)]);
        assert_eq!(list[0].title.chars().count(), MAX_TITLE_CHARS + 1);
        assert!(list[0].title.ends_with('…'));

        let many = (0..MAX_RUNNING_APPS + 20)
            .map(|index| pair(&format!("app{index:03}.exe"), "창"))
            .collect();
        let list = running_app_list(many);
        assert_eq!(list.len(), MAX_RUNNING_APPS);
        // 앞쪽(최근 창)이 남는다.
        assert_eq!(list[0].name, "app000.exe");
    }

    #[test]
    fn store_app_host_is_labeled_as_all_store_apps() {
        let list = running_app_list(vec![pair("ApplicationFrameHost.exe", "계산기")]);
        assert_eq!(list[0].title, "스토어 앱 전체(예: 계산기)");
    }

    #[test]
    fn menu_text_escapes_ampersand_and_shortens_title() {
        assert_eq!(
            menu_text(&app("game.exe", "Tom & Jerry"), 40),
            "game.exe · Tom && Jerry"
        );
        assert_eq!(menu_text(&app("a.exe", "가나다라"), 2), "a.exe · 가나…");
    }
}
