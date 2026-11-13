//! Glide 설정 창(Tauri).
//!
//! - 설정 읽기·저장과 상태 조회는 코어(`glide-core.exe`)에 named pipe로 요청한다. 설정 파일을 쓰는 것은
//!   코어 하나뿐이다. 코어가 꺼져 있으면 파일을 읽어 보여만 주고 저장은 막는다.
//! - 백업 파일 읽기·쓰기는 이 프로세스(Rust 쪽)가 한다. 웹 화면에는 파일 경로를 넘기지 않고,
//!   코어에는 검증된 설정 값만 보낸다.
//! - 웹 화면(ui/)은 정적 파일이며 외부 네트워크를 쓰지 않는다(CSP로 차단).

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
        "version": env!("CARGO_PKG_VERSION"),
    })
}

/// 상태 칸 갱신용.
#[tauri::command(async)]
fn status() -> Value {
    core_status()
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
            save,
            start_core,
            import_backup,
            export_backup
        ])
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Glide 설정 창을 열지 못했습니다: {error}");
        std::process::exit(1);
    }
}
