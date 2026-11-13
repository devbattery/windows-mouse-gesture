//! 설정 창과 통신하는 named pipe 서버(프로토콜은 `glide_core::ipc`).
//!
//! 보안
//! - 접근 제어 목록(DACL)에 현재 사용자 SID만 넣는다. 다른 사용자·원격 클라이언트는 열 수 없다.
//! - `FILE_FLAG_FIRST_PIPE_INSTANCE`로 처음 만든 인스턴스 하나를 끝까지 유지한다(최대 1개).
//!   다른 프로그램이 같은 이름을 먼저 만들었으면 실패하므로 이름을 가로챌 수 없다.
//! - 연결한 프로세스가 같은 폴더의 `glide-settings.exe`인지 확인한다. 설정 파일 자체는 사용자 권한으로
//!   쓸 수 있으므로 이것이 보안 경계는 아니고, 다른 프로그램의 실수·오용을 줄이는 보조 장치다.
//! - 연결 한 번에 요청 하나만 처리한다. 파일 경로를 받는 명령은 없다.

use std::fs::File;
use std::io;
use std::mem::ManuallyDrop;
use std::os::windows::io::FromRawHandle;

use glide_core::ipc::{self, Request};
use serde_json::Value;
use std::time::Duration;

use windows_sys::Win32::Foundation::{
    ERROR_NO_DATA, ERROR_PIPE_CONNECTED, GetLastError, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
    LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, FlushFileBuffers, PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeClientProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT,
};

/// 만들어 둔 파이프 인스턴스. 프로세스가 끝날 때까지 닫지 않는다.
pub struct Server {
    handle: HANDLE,
}

// 핸들 값은 서버 스레드 하나만 쓴다.
unsafe impl Send for Server {}

/// 현재 사용자만 열 수 있는 파이프를 만든다. 이미 같은 이름이 있으면 실패한다.
pub fn create(name: &str) -> io::Result<Server> {
    let sid = ipc::user_sid()?;
    // D:P = 상속 안 받는 DACL, A;;GA;;;<SID> = 이 사용자에게 모든 권한
    let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: 보안 설명자는 LocalAlloc으로 받아 사용 후 LocalFree로 해제한다. 문자열은 널 종료다.
    unsafe {
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let size = ipc::MAX_MESSAGE as u32 + 4;
        let handle = CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            size,
            size,
            0,
            &attributes,
        );
        let error = io::Error::last_os_error();
        LocalFree(descriptor as HLOCAL);
        if handle == INVALID_HANDLE_VALUE {
            return Err(error);
        }
        Ok(Server { handle })
    }
}

/// 클라이언트를 하나씩 받아 처리한다(돌아오지 않음). 프로세스가 끝나면 스레드도 함께 끝난다.
///
/// - `verify`: 클라이언트 프로세스 ID를 받아 허용할지 판단
/// - `handle`: 검증된 요청 하나를 처리해 응답(JSON)을 돌려줌
pub fn serve(
    server: Server,
    verify: impl Fn(u32) -> bool,
    mut handle: impl FnMut(Request) -> Value,
) {
    // File로 감싸 Read/Write를 쓰되, 핸들은 닫지 않는다(인스턴스를 끝까지 유지).
    // SAFETY: create가 만든 유효한 파이프 핸들이다.
    let mut pipe = ManuallyDrop::new(unsafe { File::from_raw_handle(server.handle) });
    loop {
        // SAFETY: 동기 파이프 핸들로 연결을 기다린다. 이미 연결된 경우는 ERROR_PIPE_CONNECTED,
        // 클라이언트가 연결했다가 바로 끊은 경우는 ERROR_NO_DATA다.
        let (connected, error) = unsafe {
            let ok = ConnectNamedPipe(server.handle, std::ptr::null_mut()) != 0;
            (ok, if ok { 0 } else { GetLastError() })
        };
        if !connected && error != ERROR_PIPE_CONNECTED && error != ERROR_NO_DATA {
            // 예상하지 못한 실패가 계속되면 CPU를 쓰며 돌지 않도록 잠시 쉰다.
            std::thread::sleep(Duration::from_millis(200));
        }
        if connected || error == ERROR_PIPE_CONNECTED {
            let mut client = 0;
            // SAFETY: 연결된 파이프의 클라이언트 PID를 조회한다.
            let known = unsafe { GetNamedPipeClientProcessId(server.handle, &mut client) } != 0;
            let response = if !known || !verify(client) {
                // 확인되지 않은 클라이언트에게는 응답하지 않고 바로 끊는다. 응답을 쓰면 상대가 읽을 때까지
                // FlushFileBuffers가 기다려야 해서, 읽지 않는 프로그램 하나가 설정 창 통신을 막을 수 있다.
                // (설정 창은 연결 전에 서버가 같은 폴더의 코어인지 먼저 확인하므로 안내는 그쪽에서 한다.)
                None
            } else {
                match ipc::read_message(&mut *pipe) {
                    Ok(value) => Some(match Request::from_value(&value) {
                        Ok(request) => handle(request),
                        Err(message) => ipc::failure(message),
                    }),
                    // 형식이 틀리거나 클라이언트가 끊음: 응답 없이 끊는다.
                    Err(_) => None,
                }
            };
            if let Some(response) = response
                && ipc::write_message(&mut *pipe, &response).is_ok()
            {
                // 연결을 끊으면 읽지 않은 데이터가 버려지므로, 클라이언트가 다 읽을 때까지 기다린다.
                // SAFETY: 연결된 파이프 핸들이다.
                unsafe { FlushFileBuffers(server.handle) };
            }
        }
        // SAFETY: 연결을 끊고 같은 인스턴스로 다음 클라이언트를 기다린다.
        unsafe { DisconnectNamedPipe(server.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glide_core::ipc::CallError;
    use serde_json::json;

    #[test]
    fn serves_requests_one_per_connection() {
        let name = format!(r"\\.\pipe\glide-test-{}", std::process::id());
        let server = create(&name).unwrap();
        // 같은 이름은 다시 만들 수 없다(이름 선점 방지).
        assert!(create(&name).is_err());
        std::thread::spawn(move || {
            serve(
                server,
                |_| true,
                |request| match request {
                    Request::GetStatus => ipc::success(json!({ "version": "test" })),
                    Request::GetSettings => ipc::success(json!({ "settings": {} })),
                    Request::SetSettings(_) => ipc::failure("검증 실패"),
                },
            )
        });
        let status = ipc::call(&name, &Request::GetStatus, |_| true).unwrap();
        assert_eq!(status["version"], "test");
        assert!(ipc::call(&name, &Request::GetSettings, |_| true).is_ok());
        assert_eq!(
            ipc::call(&name, &Request::SetSettings(json!({})), |_| true),
            Err(CallError::Failed("검증 실패".into()))
        );
        // 서버를 믿을 수 없으면 요청을 보내지 않는다.
        assert!(matches!(
            ipc::call(&name, &Request::GetStatus, |_| false),
            Err(CallError::Io(_))
        ));
        assert_eq!(
            ipc::call(r"\\.\pipe\glide-test-none", &Request::GetStatus, |_| true),
            Err(CallError::NotRunning)
        );
    }

    #[test]
    fn rejects_unverified_clients() {
        let name = format!(r"\\.\pipe\glide-test-reject-{}", std::process::id());
        let server = create(&name).unwrap();
        std::thread::spawn(move || serve(server, |_| false, |_| ipc::success(json!({}))));
        // 확인되지 않은 클라이언트는 응답 없이 끊긴다(요청이 처리되지 않음).
        let result = ipc::call(&name, &Request::GetStatus, |_| true);
        assert!(matches!(result, Err(CallError::Io(_))), "{result:?}");
        // 응답을 읽지 않는 클라이언트가 있어도 서버는 다음 연결을 받는다.
        let idle = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&name)
            .unwrap();
        drop(idle);
        assert!(matches!(
            ipc::call(&name, &Request::GetStatus, |_| true),
            Err(CallError::Io(_))
        ));
    }
}
