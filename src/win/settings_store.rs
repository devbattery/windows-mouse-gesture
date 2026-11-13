//! settings.json 읽기·쓰기·변경 감시.
//!
//! - 위치: `%APPDATA%\Glide\settings.json`
//! - 파일이 없으면 기본값을 쓴다. 임시 파일에 쓴 뒤 이름을 바꿔(같은 볼륨에서 원자적 교체)
//!   저장 중 전원이 꺼져도 깨진 파일이 남지 않게 한다.
//! - 검증에 실패한 파일은 덮어쓰지 않는다. 이전 설정을 유지하고 사용자에게 알린다.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use glide_core::settings::{self, Settings};

pub struct Store {
    path: PathBuf,
    /// 마지막으로 읽은(또는 읽기를 시도한) 파일의 수정 시각과 크기
    seen: Option<(SystemTime, u64)>,
}

/// `poll` 결과.
pub enum Change {
    /// 바뀐 것 없음
    None,
    /// 새 설정을 읽음
    Loaded(Settings),
    /// 파일이 있지만 올바르지 않음(이전 설정 유지)
    Invalid(String),
}

impl Store {
    pub fn new() -> Self {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            path: base.join("Glide").join("settings.json"),
            seen: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn folder(&self) -> PathBuf {
        self.path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    }

    fn stamp(&self) -> Option<(SystemTime, u64)> {
        let metadata = fs::metadata(&self.path).ok()?;
        Some((metadata.modified().ok()?, metadata.len()))
    }

    /// 시작할 때 한 번: 읽거나, 없으면 기본값을 만든다.
    pub fn load_or_create(&mut self) -> Change {
        if !self.path.exists() {
            let defaults = Settings::default();
            if let Err(error) = self.write(&defaults) {
                return Change::Invalid(format!("기본 설정 파일을 만들지 못했습니다: {error}"));
            }
            self.seen = self.stamp();
            return Change::Loaded(defaults);
        }
        self.read()
    }

    /// 파일이 바뀌었으면 다시 읽는다. 워커가 주기적으로 부른다.
    pub fn poll(&mut self) -> Change {
        let stamp = self.stamp();
        if stamp.is_none() {
            // 파일이 없으면 지금 설정을 그대로 쓴다. 편집기가 "임시 파일 저장 → 원본 삭제 → 이름 바꾸기"로
            // 저장하는 순간일 수 있어서, 여기서 기본값을 쓰면 사용자의 저장을 덮어쓸 수 있다.
            // 기본 파일은 다음 실행 때 다시 만든다.
            self.seen = None;
            return Change::None;
        }
        if stamp == self.seen {
            return Change::None;
        }
        self.read()
    }

    fn read(&mut self) -> Change {
        self.seen = self.stamp();
        match fs::read_to_string(&self.path) {
            Ok(text) => match settings::parse(&text) {
                Ok(settings) => Change::Loaded(settings),
                Err(message) => Change::Invalid(message),
            },
            // 편집기가 저장하는 순간 등 일시적인 실패는 다음 확인에서 다시 시도한다.
            Err(error) => {
                self.seen = None;
                Change::Invalid(format!("설정 파일을 읽지 못했습니다: {error}"))
            }
        }
    }

    fn write(&self, settings: &Settings) -> std::io::Result<()> {
        fs::create_dir_all(self.folder())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, settings.to_json())?;
        // Windows에서 std::fs::rename은 MoveFileExW(MOVEFILE_REPLACE_EXISTING)로 교체한다.
        fs::rename(&temporary, &self.path)
    }
}
