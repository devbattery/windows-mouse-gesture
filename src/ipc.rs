//! 코어(`glide-core.exe`)와 설정 창(`glide-settings.exe`) 사이의 named pipe 프로토콜.
//!
//! - 파이프 이름: `\\.\pipe\glide-core-<사용자 SID>` (사용자마다 다르게, 빠른 사용자 전환 대비)
//! - 메시지: 4바이트 리틀 엔디언 길이 + UTF-8 JSON. 한 건 최대 64KB.
//! - 연결 한 번에 요청 하나, 응답 하나. 연결을 오래 붙잡지 않는다.
//! - 명령: `getStatus`, `getSettings`, `setSettings`. 파일 경로를 받는 명령은 두지 않는다
//!   (백업 파일 읽기·쓰기는 설정 창이 하고, 코어는 검증된 설정 값만 받는다).
//! - 코어는 받은 설정을 다시 검증한다. 설정 창의 검증은 사용자 안내용일 뿐이다.

use std::io::{self, Read, Write};

use serde_json::{Map, Value, json};

pub const PROTOCOL_VERSION: u64 = 1;
/// 메시지 한 건의 최대 크기(바이트). 규칙 64개짜리 설정도 수 KB라 충분하다.
pub const MAX_MESSAGE: usize = 64 * 1024;
pub const CORE_EXE: &str = "glide-core.exe";
pub const SETTINGS_EXE: &str = "glide-settings.exe";

/// 설정 창 → 코어 요청.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// 실행 상태(사용 여부·uiAccess·훅 처리 시간 등)
    GetStatus,
    /// 현재 설정(설정 파일 형식)과 마지막 설정 오류
    GetSettings,
    /// 설정 저장. 값은 설정 파일 형식이며 코어가 다시 검증한다.
    SetSettings(Value),
}

impl Request {
    pub fn to_value(&self) -> Value {
        let (command, settings) = match self {
            Request::GetStatus => ("getStatus", None),
            Request::GetSettings => ("getSettings", None),
            Request::SetSettings(settings) => ("setSettings", Some(settings)),
        };
        let mut value = json!({ "protocolVersion": PROTOCOL_VERSION, "command": command });
        if let Some(settings) = settings {
            value["settings"] = settings.clone();
        }
        value
    }

    pub fn from_value(value: &Value) -> Result<Self, String> {
        if value.get("protocolVersion").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
            return Err(
                "설정 창과 코어의 버전이 맞지 않습니다. 같은 버전으로 다시 설치해 주세요.".into(),
            );
        }
        match value.get("command").and_then(Value::as_str) {
            Some("getStatus") => Ok(Request::GetStatus),
            Some("getSettings") => Ok(Request::GetSettings),
            Some("setSettings") => match value.get("settings") {
                Some(settings @ Value::Object(_)) => Ok(Request::SetSettings(settings.clone())),
                _ => Err("저장할 설정이 없습니다.".into()),
            },
            _ => Err("알 수 없는 요청입니다.".into()),
        }
    }
}

/// 성공 응답. `fields`(객체)에 `ok: true`를 더한다.
pub fn success(fields: Value) -> Value {
    let mut object = match fields {
        Value::Object(object) => object,
        _ => Map::new(),
    };
    object.insert("ok".into(), Value::Bool(true));
    Value::Object(object)
}

/// 실패 응답.
pub fn failure(message: impl Into<String>) -> Value {
    json!({ "ok": false, "error": message.into() })
}

/// 응답을 결과로 바꾼다. 실패면 코어가 보낸 문구를 돌려준다.
pub fn into_result(response: Value) -> Result<Value, String> {
    if response.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(response)
    } else {
        Err(response
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("코어가 요청을 처리하지 못했습니다.")
            .to_string())
    }
}

/// 메시지 한 건을 쓴다(길이 + JSON).
pub fn write_message(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(value).map_err(io::Error::other)?;
    if body.len() > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "메시지가 너무 큽니다",
        ));
    }
    // 길이와 본문을 한 번에 써서 상대가 반쪽 메시지를 읽는 시간을 줄인다.
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    writer.write_all(&frame)?;
    writer.flush()
}

/// 메시지 한 건을 읽는다. 길이가 0이거나 최대 크기를 넘으면 본문을 읽지 않고 거부한다.
pub fn read_message(reader: &mut impl Read) -> io::Result<Value> {
    let mut length = [0u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "메시지 길이가 올바르지 않습니다",
        ));
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// 현재 사용자 SID 문자열(예: `S-1-5-21-...`). 파이프 이름과 접근 제어(DACL)에 쓴다.
#[cfg(windows)]
pub fn user_sid() -> io::Result<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: 자기 프로세스 토큰을 조회만 하고, 받은 핸들·메모리는 이 함수 안에서 해제한다.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        // TOKEN_USER 뒤에 SID가 이어 붙는 가변 길이 구조라 넉넉한 버퍼를 쓴다(u64 정렬).
        let mut buffer = [0u64; 64];
        let mut needed = 0;
        let ok = GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
            &mut needed,
        );
        let error = io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(error);
        }
        let user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut text: *mut u16 = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = (0..).take_while(|&i| *text.add(i) != 0).count();
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, length));
        LocalFree(text as HLOCAL);
        Ok(sid)
    }
}

/// 이 사용자의 코어 파이프 이름.
#[cfg(windows)]
pub fn pipe_name() -> io::Result<String> {
    Ok(format!(r"\\.\pipe\glide-core-{}", user_sid()?))
}

/// 프로세스 실행 파일의 전체 경로. 열 수 없으면(종료됨, 권한 부족) None.
#[cfg(windows)]
pub fn process_path(pid: u32) -> Option<std::path::PathBuf> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };

    if pid == 0 {
        return None;
    }
    // SAFETY: 최소 권한(이름 조회)으로 열고, 사용 후 반드시 닫는다.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut length,
        );
        CloseHandle(process);
        (ok != 0).then(|| String::from_utf16_lossy(&buffer[..length as usize]).into())
    }
}

/// `pid`가 이 실행 파일과 같은 폴더에 있는 `exe_name`인지.
/// 설치 폴더(Program Files)는 관리자만 쓸 수 있으므로, 설치본끼리는 서로를 확인하는 근거가 된다.
#[cfg(windows)]
pub fn is_sibling_process(pid: u32, exe_name: &str) -> bool {
    let expected = std::env::current_exe()
        .ok()
        .and_then(|me| me.parent().map(|folder| folder.join(exe_name)));
    let (Some(actual), Some(expected)) = (process_path(pid), expected) else {
        return false;
    };
    // 짧은 이름(8.3)·대소문자 차이를 없애려고 실제 경로로 바꿔 비교한다.
    match (
        std::fs::canonicalize(actual),
        std::fs::canonicalize(expected),
    ) {
        (Ok(actual), Ok(expected)) => {
            actual.to_string_lossy().to_lowercase() == expected.to_string_lossy().to_lowercase()
        }
        _ => false,
    }
}

/// 파이프 호출 실패 이유.
#[derive(Debug, PartialEq, Eq)]
pub enum CallError {
    /// 코어가 실행 중이 아님(파이프 없음)
    NotRunning,
    /// 코어가 요청을 거절함(검증 실패 등). 코어가 보낸 문구.
    Failed(String),
    /// 연결·읽기·쓰기 오류, 또는 서버가 코어로 확인되지 않음
    Io(String),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CallError::NotRunning => f.write_str("Glide가 실행 중이 아닙니다."),
            CallError::Failed(message) => f.write_str(message),
            CallError::Io(message) => write!(f, "Glide와 통신하지 못했습니다: {message}"),
        }
    }
}

/// 파이프 이름 `name`의 서버에 요청 하나를 보내고 응답을 받는다.
/// `verify_server`는 서버 프로세스 ID를 받아 믿을 수 있는 코어인지 판단한다(가짜 서버 방지).
#[cfg(windows)]
pub fn call(
    name: &str,
    request: &Request,
    verify_server: impl Fn(u32) -> bool,
) -> Result<Value, CallError> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::{GetNamedPipeServerProcessId, WaitNamedPipeW};

    const NOT_FOUND: i32 = 2;
    const PIPE_BUSY: i32 = 231;
    let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    let mut attempts = 0;
    let mut pipe = loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
        {
            Ok(pipe) => break pipe,
            Err(error) if error.raw_os_error() == Some(NOT_FOUND) => {
                return Err(CallError::NotRunning);
            }
            // 코어가 다른 요청을 처리 중이면 잠시 기다린다(인스턴스가 하나뿐이다).
            Err(error) if error.raw_os_error() == Some(PIPE_BUSY) && attempts < 5 => {
                attempts += 1;
                // SAFETY: 널 종료 이름으로 최대 2초 기다린다.
                unsafe { WaitNamedPipeW(wide.as_ptr(), 2_000) };
            }
            Err(error) => return Err(CallError::Io(error.to_string())),
        }
    };
    let mut server = 0;
    // SAFETY: 방금 연 파이프 핸들로 서버 PID를 조회한다.
    let known = unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut server) } != 0;
    if !known || !verify_server(server) {
        return Err(CallError::Io(
            "파이프를 연 프로그램이 Glide 코어로 확인되지 않습니다.".into(),
        ));
    }
    write_message(&mut pipe, &request.to_value())
        .map_err(|error| CallError::Io(error.to_string()))?;
    let response = read_message(&mut pipe).map_err(|error| CallError::Io(error.to_string()))?;
    into_result(response).map_err(CallError::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        for request in [
            Request::GetStatus,
            Request::GetSettings,
            Request::SetSettings(json!({ "enabled": true })),
        ] {
            assert_eq!(Request::from_value(&request.to_value()).unwrap(), request);
        }
    }

    #[test]
    fn rejects_other_versions_and_commands() {
        let old = json!({ "protocolVersion": 0, "command": "getStatus" });
        assert!(Request::from_value(&old).unwrap_err().contains("버전"));
        let unknown = json!({ "protocolVersion": 1, "command": "runFile" });
        assert!(Request::from_value(&unknown).is_err());
        let empty = json!({ "protocolVersion": 1, "command": "setSettings" });
        assert!(Request::from_value(&empty).is_err());
    }

    #[test]
    fn frames_round_trip() {
        let mut buffer = Vec::new();
        write_message(&mut buffer, &json!({ "a": 1 })).unwrap();
        write_message(&mut buffer, &json!([1, 2])).unwrap();
        let mut reader = buffer.as_slice();
        assert_eq!(read_message(&mut reader).unwrap(), json!({ "a": 1 }));
        assert_eq!(read_message(&mut reader).unwrap(), json!([1, 2]));
    }

    #[test]
    fn rejects_bad_lengths() {
        let zero = 0u32.to_le_bytes();
        assert!(read_message(&mut zero.as_slice()).is_err());
        let huge = ((MAX_MESSAGE + 1) as u32).to_le_bytes();
        assert!(read_message(&mut huge.as_slice()).is_err());
        let too_big = json!("x".repeat(MAX_MESSAGE));
        assert!(write_message(&mut Vec::new(), &too_big).is_err());
    }

    #[test]
    fn responses_map_to_results() {
        let ok = success(json!({ "version": "0.3.0" }));
        assert_eq!(into_result(ok).unwrap()["version"], "0.3.0");
        assert_eq!(into_result(failure("저장 실패")).unwrap_err(), "저장 실패");
    }

    #[cfg(windows)]
    #[test]
    fn pipe_name_contains_user_sid() {
        let name = pipe_name().unwrap();
        assert!(name.starts_with(r"\\.\pipe\glide-core-S-1-"), "{name}");
    }
}
