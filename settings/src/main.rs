//! Glide 설정 창(Tauri).
//!
//! - 설정 읽기·저장과 상태 조회는 코어(`glide-core.exe`)에 named pipe로 요청한다. 설정 파일을 쓰는 것은
//!   코어 하나뿐이다. 코어가 꺼져 있으면 파일을 읽어 보여만 주고 저장은 막는다.
//! - 백업 파일 읽기·쓰기는 이 프로세스(Rust 쪽)가 한다. 웹 화면에는 파일 경로를 넘기지 않고,
//!   코어에는 검증된 설정 값만 보낸다.
//! - 웹 화면(ui/)은 정적 파일이며 외부 네트워크를 쓰지 않는다(CSP로 차단). 새 버전 확인은 코어가 한다.

#![cfg_attr(not(test), windows_subsystem = "windows")]

use std::time::{SystemTime, UNIX_EPOCH};

use glide_core::actions::{self, Effect, Rule};
use glide_core::backup;
use glide_core::ipc::{self, CallError, Request};
use glide_core::settings::{self, Settings};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

/// 가져올 백업 파일의 최대 크기(확장과 같음)
const MAX_BACKUP_BYTES: u64 = 1024 * 1024;

/// 코어에 요청 하나를 보낸다. 같은 폴더의 glide-core.exe가 연 파이프만 믿는다.
fn call_core(request: Request) -> Result<Value, CallError> {
    let name = ipc::pipe_name().map_err(|error| CallError::Io(error.to_string()))?;
    ipc::call(&name, &request, |pid| {
        ipc::is_sibling_process(pid, ipc::CORE_EXE)
    })
}

/// 설정 창 동작 목록. 키 입력 동작은 실제로 보내는 조합(`keys`)도 담아, 직접 지정한 단축키가
/// 기본 동작과 같은지 화면에서 알려 줄 수 있게 한다.
fn catalog() -> Value {
    actions::catalog()
        .into_iter()
        .map(|entry| {
            let keys = match actions::effect(entry.id) {
                Effect::Keys(shortcut) => Some(shortcut.text),
                _ => None,
            };
            json!({
                "id": entry.id,
                "label": entry.label,
                "hint": entry.hint,
                "supported": entry.supported,
                "windowsOnly": entry.windows_only,
                "keys": keys,
            })
        })
        .collect()
}

/// 코어가 꺼져 있을 때 보여 줄 설정(파일을 읽기만 한다).
fn read_settings_file() -> (Value, Option<String>) {
    match std::fs::read_to_string(settings::file_path()) {
        Ok(text) => match settings::parse(&text) {
            Ok(settings) => (settings.to_value(), None),
            Err(message) => (Settings::default().to_value(), Some(message)),
        },
        // 아직 한 번도 실행하지 않았으면 파일이 없다. 기본값을 보여 준다.
        Err(_) => (Settings::default().to_value(), None),
    }
}

/// 코어 상태. 실행 중이 아니면 `running: false`와 이유를 돌려준다.
fn core_status() -> Value {
    match call_core(Request::GetStatus) {
        Ok(status) => json!({ "running": true, "status": status }),
        Err(CallError::NotRunning) => json!({ "running": false }),
        Err(error) => json!({ "running": false, "error": error.to_string() }),
    }
}

/// 처음 화면: 코어 상태, 설정, 동작 목록, 기본 설정.
#[tauri::command(async)]
fn load() -> Value {
    let core = core_status();
    let (settings, settings_error) = if core["running"] == true {
        match call_core(Request::GetSettings) {
            Ok(value) => (
                value["settings"].clone(),
                value["error"].as_str().map(String::from),
            ),
            Err(error) => {
                let (settings, _) = read_settings_file();
                (settings, Some(error.to_string()))
            }
        }
    } else {
        read_settings_file()
    };
    json!({
        "core": core,
        "settings": settings,
        "settingsError": settings_error,
        "catalog": catalog(),
        "defaults": Settings::default().to_value(),
        "suggestedExclusions": settings::SUGGESTED_EXCLUSIONS
            .iter()
            .map(|(name, reason)| json!({ "name": name, "reason": reason }))
            .collect::<Vec<_>>(),
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// 상태 칸 갱신용.
#[tauri::command(async)]
fn status() -> Value {
    core_status()
}

/// 일상 사용 점검 칸: 최근 우클릭 기록, 누름 시간 통계, 진단 정보.
#[tauri::command(async)]
fn diagnostics() -> Result<Value, String> {
    call_core(Request::GetDiagnostics).map_err(|error| match error {
        CallError::NotRunning => "Glide가 실행 중이 아니라 기록을 볼 수 없습니다.".into(),
        other => other.to_string(),
    })
}

/// 설정 저장. 코어가 다시 검증하고 파일에 쓴 뒤 바로 반영한다.
#[tauri::command(async)]
fn save(settings: Value) -> Result<(), String> {
    call_core(Request::SetSettings(settings))
        .map(|_| ())
        .map_err(|error| match error {
            CallError::NotRunning => {
                "Glide가 실행 중이 아니라 저장할 수 없습니다. 먼저 Glide를 시작해 주세요.".into()
            }
            other => other.to_string(),
        })
}

/// 같은 폴더의 glide-core.exe를 시작한다. 셸(ShellExecuteW)로 실행해야 uiAccess 판이
/// 서명·설치 위치 확인을 거쳐 관리자 창 지원 권한으로 시작된다.
#[tauri::command(async)]
fn start_core() -> Result<(), String> {
    let core = std::env::current_exe()
        .ok()
        .and_then(|me| me.parent().map(|folder| folder.join(ipc::CORE_EXE)))
        .filter(|path| path.is_file())
        .ok_or("설정 창과 같은 폴더에 glide-core.exe가 없습니다.")?;
    if shell_open(&core.display().to_string()) {
        Ok(())
    } else {
        Err("Glide를 시작하지 못했습니다.".into())
    }
}

/// 새 버전을 받는 곳. 설정 창은 인터넷에 접속하지 않고(새 버전 확인은 코어가 함), 사용자가 누르면 브라우저로 연다.
const RELEASES_URL: &str = "https://github.com/devbattery/windows-mouse-gesture/releases/latest";

/// "Glide 정보" 칸의 버튼. 웹 화면은 종류 이름만 보내고, 여는 주소·파일은 여기서 정한 것만 쓴다.
/// - `releases`: GitHub Releases 페이지(기본 브라우저)
/// - `license`, `notices`: 설치 폴더의 LICENSE.txt, THIRD-PARTY-NOTICES.txt(기본 텍스트 편집기)
#[tauri::command(async)]
fn open_info(kind: String) -> Result<(), String> {
    let target = match kind.as_str() {
        "releases" => RELEASES_URL.to_string(),
        "license" | "notices" => {
            let name = if kind == "license" {
                "LICENSE.txt"
            } else {
                "THIRD-PARTY-NOTICES.txt"
            };
            std::env::current_exe()
                .ok()
                .and_then(|me| me.parent().map(|folder| folder.join(name)))
                .filter(|path| path.is_file())
                .ok_or(format!(
                    "{name} 파일이 설정 창과 같은 폴더에 없습니다. 설치 스크립트로 설치하면 함께 복사됩니다."
                ))?
                .display()
                .to_string()
        }
        _ => return Err("알 수 없는 항목입니다.".into()),
    };
    if shell_open(&target) {
        Ok(())
    } else {
        Err("열지 못했습니다.".into())
    }
}

#[cfg(windows)]
fn shell_open(file: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let file = wide(file);
    // SAFETY: 널 종료 문자열로 셸 실행을 요청한다.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            wide("open").as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // 32 이하는 실패 코드다.
    (result as isize) > 32
}

#[cfg(not(windows))]
fn shell_open(_file: &str) -> bool {
    false
}

/// 진단 정보를 클립보드에 넣는다(유니코드 텍스트).
#[tauri::command(async)]
fn copy_text(text: String) -> Result<(), String> {
    // 화면이 만든 진단 문구만 받는다. 지나치게 큰 값은 거절한다.
    if text.len() > 64 * 1024 {
        return Err("복사할 내용이 너무 깁니다.".into());
    }
    if clipboard_set(&text) {
        Ok(())
    } else {
        Err("클립보드를 쓰지 못했습니다. 다른 프로그램이 클립보드를 쓰고 있으면 잠시 뒤 다시 시도해 주세요.".into())
    }
}

#[cfg(windows)]
fn clipboard_set(text: &str) -> bool {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let bytes = wide.len() * size_of::<u16>();
    // SAFETY: 이동 가능한 전역 메모리에 널 종료 UTF-16을 복사해 클립보드에 넘긴다.
    // SetClipboardData가 성공하면 메모리 소유권은 시스템으로 넘어가고, 실패하면 직접 해제한다.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        let mut ok = false;
        if EmptyClipboard() != 0 {
            let memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if !memory.is_null() {
                let target = GlobalLock(memory) as *mut u16;
                if !target.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), target, wide.len());
                    GlobalUnlock(memory);
                    ok = !SetClipboardData(CF_UNICODETEXT as u32, memory).is_null();
                }
                if !ok {
                    GlobalFree(memory);
                }
            }
        }
        CloseClipboard();
        ok
    }
}

#[cfg(not(windows))]
fn clipboard_set(_text: &str) -> bool {
    false
}

/// 실행 중인 앱 목록에 보여 줄 최대 개수
const MAX_RUNNING_APPS: usize = 100;
/// 목록에 보여 줄 창 제목 최대 글자 수
const MAX_TITLE_CHARS: usize = 80;
/// 스토어(UWP) 앱 창을 대신 소유하는 프로세스
const STORE_APP_HOST: &str = "applicationframehost.exe";

/// 지금 창이 열린 앱 목록(예외 앱 고르기용). `[{ name, title }]`, 이름순.
/// 창 제목은 화면에 보여 주기만 하고 저장하지 않는다. 예외 목록에는 실행 파일 이름만 들어간다.
#[tauri::command(async)]
fn running_apps() -> Vec<Value> {
    running_app_list(top_level_windows())
}

/// (실행 파일 이름, 창 제목) 목록을 화면용으로 정리한다.
/// - 이름은 코어 비교 규칙과 같게 소문자로 바꾸고, 같은 실행 파일은 첫 창 하나로 합친다.
/// - Glide 자신은 뺀다(코어는 자기 창에서 제스처를 쓰지 않으므로 넣을 필요가 없다).
fn running_app_list(windows: Vec<(String, String)>) -> Vec<Value> {
    let mut apps: Vec<(String, String)> = Vec::new();
    for (name, title) in windows {
        let name = name.trim().to_lowercase();
        let title = title.trim();
        if name.is_empty()
            || title.is_empty()
            || name == ipc::CORE_EXE
            || name == ipc::SETTINGS_EXE
            || apps.iter().any(|(known, _)| *known == name)
        {
            continue;
        }
        let mut short: String = title.chars().take(MAX_TITLE_CHARS).collect();
        if title.chars().count() > MAX_TITLE_CHARS {
            short.push('…');
        }
        // 스토어 앱 창은 모두 ApplicationFrameHost.exe가 소유한다. 이 이름을 넣으면 스토어 앱 전체가
        // 예외가 되므로, 창 제목 하나만 보고 고르지 않게 알린다.
        if name == STORE_APP_HOST {
            short = format!("스토어 앱 전체(예: {short})");
        }
        apps.push((name, short));
    }
    apps.sort_by(|a, b| a.0.cmp(&b.0));
    apps.truncate(MAX_RUNNING_APPS);
    apps.into_iter()
        .map(|(name, title)| json!({ "name": name, "title": title }))
        .collect()
}

/// 작업 표시줄에 보이는 최상위 창의 (실행 파일 이름, 제목) 목록.
/// 코어는 우클릭한 창의 최상위 창 프로세스로 예외를 판정하므로(hook.rs `window_at`), 같은 기준으로 고른다.
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

#[cfg(not(windows))]
fn top_level_windows() -> Vec<(String, String)> {
    Vec::new()
}

fn rule_values(rules: &[Rule]) -> Vec<Value> {
    rules.iter().map(Rule::to_value).collect()
}

/// 백업 파일을 골라 읽는다. 취소하면 null.
/// 돌려주는 값: `{ rules, skipped }` (skipped = Windows에서 실행할 수 없어 뺀 규칙)
#[tauri::command(async)]
fn import_backup(app: AppHandle) -> Result<Option<Value>, String> {
    let Some(picked) = app
        .dialog()
        .file()
        .set_title("불러올 Glide 제스처 파일")
        .add_filter("Glide 제스처 파일", &["json"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| "이 위치의 파일은 열 수 없습니다.".to_string())?;
    let size = std::fs::metadata(&path)
        .map_err(|error| format!("파일을 읽지 못했습니다: {error}"))?
        .len();
    if size > MAX_BACKUP_BYTES {
        return Err("파일이 너무 큽니다. 1MB 이하의 파일을 선택해 주세요.".into());
    }
    let text =
        std::fs::read_to_string(&path).map_err(|_| "JSON 파일을 읽지 못했습니다.".to_string())?;
    let imported = backup::parse(&text)?;
    Ok(Some(json!({
        "rules": rule_values(&imported.rules),
        "skipped": rule_values(&imported.skipped),
    })))
}

/// 화면의 제스처 목록을 백업 파일로 저장한다. 취소하면 null.
/// - `extension_compatible`: 크롬 확장이 읽을 수 있게 Windows 전용 동작을 뺀다
/// - `file_name`: 저장 대화상자의 기본 이름(화면이 현지 날짜로 만든다)
#[tauri::command(async)]
fn export_backup(
    app: AppHandle,
    rules: Value,
    extension_compatible: bool,
    file_name: String,
) -> Result<Option<Value>, String> {
    // 파일을 만들기 전에 검증해 잘못된 단축키 등은 바로 알려 준다.
    let rules = settings::validate_rules(&rules)?;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis() as u64);
    let exported = backup::create(
        &rules,
        env!("CARGO_PKG_VERSION"),
        &backup::timestamp(millis),
        extension_compatible,
    );
    // 기본 이름에 경로가 섞여 들어오지 않게 파일 이름 부분만 쓴다.
    let file_name = std::path::Path::new(&file_name).file_name().map_or_else(
        || "glide-gestures.json".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    let Some(picked) = app
        .dialog()
        .file()
        .set_title("제스처 내보내기")
        .set_file_name(&file_name)
        .add_filter("Glide 제스처 파일", &["json"])
        .blocking_save_file()
    else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|_| "이 위치에는 저장할 수 없습니다.".to_string())?;
    std::fs::write(&path, exported.text)
        .map_err(|error| format!("파일을 쓰지 못했습니다: {error}"))?;
    Ok(Some(json!({
        "count": rules.len() - exported.skipped.len(),
        "fileName": path.file_name().map(|name| name.to_string_lossy().into_owned()),
        "skipped": rule_values(&exported.skipped),
    })))
}

fn main() {
    let result = tauri::Builder::default()
        // 이미 열려 있으면 새 창을 만들지 않고 기존 창을 앞으로 가져온다.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            load,
            status,
            diagnostics,
            copy_text,
            save,
            start_core,
            open_info,
            running_apps,
            import_backup,
            export_backup
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Glide 설정 창을 열지 못했습니다: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(name: &str, title: &str) -> (String, String) {
        (name.to_string(), title.to_string())
    }

    #[test]
    fn running_apps_are_merged_sorted_and_without_glide() {
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
            [
                json!({ "name": "explorer.exe", "title": "문서" }),
                json!({ "name": "game.exe", "title": "My Game" }),
            ]
        );
    }

    #[test]
    fn running_app_titles_and_count_are_limited() {
        let long = "가".repeat(MAX_TITLE_CHARS + 5);
        let list = running_app_list(vec![pair("a.exe", &long)]);
        let title = list[0]["title"].as_str().unwrap();
        assert_eq!(title.chars().count(), MAX_TITLE_CHARS + 1);
        assert!(title.ends_with('…'));

        let many = (0..MAX_RUNNING_APPS + 20)
            .map(|index| pair(&format!("app{index:03}.exe"), "창"))
            .collect();
        assert_eq!(running_app_list(many).len(), MAX_RUNNING_APPS);
    }

    #[test]
    fn store_app_host_is_labeled_as_all_store_apps() {
        let list = running_app_list(vec![pair("ApplicationFrameHost.exe", "계산기")]);
        assert_eq!(list[0]["title"], "스토어 앱 전체(예: 계산기)");
    }
}
