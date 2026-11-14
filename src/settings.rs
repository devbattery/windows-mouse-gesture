//! 설정 형식과 검증.
//!
//! 규칙(`rules`)·사용 여부·감도는 크롬 확장의 `validateSettings`·`validateRules`와 같은
//! 규칙과 오류 문구로 검증한다. Windows 전용 값은 `windows` 객체에 둔다.
//! 모르는 필드는 무시해서 다음 버전의 설정 파일도 읽을 수 있게 한다.

use serde_json::{Map, Value, json};

use crate::actions::{self, Rule};
use crate::gesture::{DEFAULT_THRESHOLD, MAX_STROKES, MAX_THRESHOLD, MIN_THRESHOLD};
use crate::shortcut;

pub const SCHEMA_VERSION: u64 = 1;
pub const MAX_RULES: usize = 64;
pub const MIN_HOLD_MS: u64 = 100;
pub const MAX_HOLD_MS: u64 = 3_000;
pub const DEFAULT_HOLD_MS: u32 = 400;
const MAX_EXCLUDED: usize = 256;

/// 예외에 넣기를 권하는 앱(실행 파일 이름, 이유). 기본으로 넣지는 않고 설정 창에서 버튼으로 제안한다.
/// 원격·가상 머신 화면에서는 제스처가 바깥 창(이 PC)에 실행되므로 원격 쪽 우클릭이 지연되거나
/// 엉뚱한 창이 닫힐 수 있다. 공식 문서·배포 문서에서 이름을 확인한 것만 둔다.
pub const SUGGESTED_EXCLUSIONS: [(&str, &str); 4] = [
    ("mstsc.exe", "원격 데스크톱 연결"),
    (
        "msrdc.exe",
        "원격 데스크톱 클라이언트(Azure Virtual Desktop)",
    ),
    ("vmconnect.exe", "Hyper-V 가상 머신 연결"),
    ("virtualboxvm.exe", "VirtualBox 가상 머신 창"),
];

/// 설정 파일 위치: `%APPDATA%\Glide\settings.json`(사용자별, 로밍 프로필).
/// 코어(읽기·쓰기)와 설정 창(코어가 꺼져 있을 때 읽기만)이 같은 규칙을 쓴다.
pub fn file_path() -> std::path::PathBuf {
    std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Glide")
        .join("settings.json")
}

/// 검증을 통과한 설정.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub enabled: bool,
    /// 방향 판정 임계값(96 DPI 기준 px)
    pub threshold: u32,
    pub rules: Vec<Rule>,
    /// 움직이지 않고 이 시간(ms) 누르면 원래 우클릭으로 통과
    pub hold_ms: u32,
    /// 제스처를 쓰지 않을 실행 파일 이름(소문자, 예: "mstsc.exe")
    pub excluded_processes: Vec<String>,
    /// 전체 화면 게임·프레젠테이션 중 자동 일시 중지
    pub auto_pause_fullscreen: bool,
    /// 다른 프로그램이 주입한 입력(원격 제어 프로그램 등)도 제스처로 해석
    pub allow_injected_input: bool,
    /// 하루 한 번 GitHub Releases에 최신 버전을 물어보고 새 버전이 있으면 알림
    pub check_updates: bool,
}

/// 확장의 기본 제스처 10종(`shared.js` defaults.rules)과 같다.
const DEFAULT_RULES: [(&str, &str); 10] = [
    ("L", "back"),
    ("R", "forward"),
    ("D", "newTab"),
    ("DR", "closeTab"),
    ("UD", "reload"),
    ("DU", "restoreTab"),
    ("UL", "previousTab"),
    ("UR", "nextTab"),
    ("U", "scrollTop"),
    ("DL", "scrollBottom"),
];

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: DEFAULT_THRESHOLD,
            rules: DEFAULT_RULES
                .iter()
                .map(|(gesture, action)| Rule {
                    gesture: (*gesture).into(),
                    action: (*action).into(),
                    shortcut: None,
                })
                .collect(),
            hold_ms: DEFAULT_HOLD_MS,
            excluded_processes: Vec::new(),
            auto_pause_fullscreen: true,
            allow_injected_input: false,
            check_updates: true,
        }
    }
}

/// 방향 문자열 검사: L·R·U·D 1~12자, 같은 방향 연속 금지(확장 `validGesture`와 같음).
pub fn valid_gesture(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=MAX_STROKES).contains(&bytes.len())
        && bytes.iter().all(|b| matches!(b, b'L' | b'R' | b'U' | b'D'))
        && bytes.windows(2).all(|pair| pair[0] != pair[1])
}

/// 규칙 배열을 검증한다. 오류 문구는 확장과 같다.
pub fn validate_rules(value: &Value) -> Result<Vec<Rule>, String> {
    let Some(items) = value.as_array() else {
        return Err("제스처 목록 형식이 올바르지 않습니다.".into());
    };
    if items.len() > MAX_RULES {
        return Err("제스처는 최대 64개까지 저장할 수 있습니다.".into());
    }
    let mut rules: Vec<Rule> = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let number = index + 1;
        let Some(object) = item.as_object() else {
            return Err(format!("{number}번째 제스처 형식이 올바르지 않습니다."));
        };
        let gesture = object
            .get("gesture")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !valid_gesture(gesture) {
            return Err(format!("{number}번째 제스처의 방향이 올바르지 않습니다."));
        }
        let action = object
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !(actions::is_known(action) || action == "keyboard") {
            return Err(format!("{number}번째 제스처의 동작이 올바르지 않습니다."));
        }
        if rules.iter().any(|rule| rule.gesture == gesture) {
            return Err(format!("{number}번째 제스처가 중복되었습니다."));
        }
        let shortcut = if action == "keyboard" {
            let text = object
                .get("shortcut")
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some(shortcut::parse(text)?)
        } else {
            None
        };
        rules.push(Rule {
            gesture: gesture.into(),
            action: action.into(),
            shortcut,
        });
    }
    Ok(rules)
}

/// 설정 JSON 전체를 검증한다.
pub fn parse(text: &str) -> Result<Settings, String> {
    // Windows 메모장 등이 붙이는 UTF-8 BOM을 허용한다(확장 백업 파서와 같음).
    let source = text.trim_start_matches('\u{feff}');
    let value: Value =
        serde_json::from_str(source).map_err(|error| format!("JSON을 읽지 못했습니다: {error}"))?;
    validate(&value)
}

/// 0 이상의 정수 값. 확장(`Number.isInteger`)처럼 `22.0` 같은 정수 값 실수도 받는다.
fn integer(value: &Value) -> Option<u64> {
    value.as_u64().or_else(|| {
        value
            .as_f64()
            .filter(|number| {
                number.is_finite()
                    && *number >= 0.0
                    && number.fract() == 0.0
                    && *number <= u32::MAX as f64
            })
            .map(|number| number as u64)
    })
}

pub fn validate(value: &Value) -> Result<Settings, String> {
    const FORMAT: &str = "설정 형식이 올바르지 않습니다.";
    let Some(object) = value.as_object() else {
        return Err(FORMAT.into());
    };
    if let Some(version) = object.get("schemaVersion")
        && integer(version) != Some(SCHEMA_VERSION)
    {
        return Err("지원하지 않는 설정 파일 버전입니다.".into());
    }
    let enabled = object.get("enabled").and_then(Value::as_bool);
    let threshold = object.get("threshold").and_then(integer);
    let rules = object.get("rules").and_then(Value::as_array);
    let (Some(enabled), Some(threshold), Some(_)) = (enabled, threshold, rules) else {
        return Err(FORMAT.into());
    };
    if !(MIN_THRESHOLD as u64..=MAX_THRESHOLD as u64).contains(&threshold) {
        return Err(FORMAT.into());
    }
    let rules = validate_rules(&object["rules"])?;
    let defaults = Settings::default();
    let windows = match object.get("windows") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map.clone(),
        Some(_) => return Err("windows 설정 형식이 올바르지 않습니다.".into()),
    };
    let hold_ms = match windows.get("holdToPassMs") {
        None | Some(Value::Null) => defaults.hold_ms,
        Some(value) => match integer(value) {
            Some(ms) if (MIN_HOLD_MS..=MAX_HOLD_MS).contains(&ms) => ms as u32,
            _ => {
                return Err(format!(
                    "holdToPassMs는 {MIN_HOLD_MS}~{MAX_HOLD_MS} 사이의 정수여야 합니다."
                ));
            }
        },
    };
    let excluded_processes = match windows.get("excludedProcesses") {
        None | Some(Value::Null) => defaults.excluded_processes,
        Some(Value::Array(items)) if items.len() <= MAX_EXCLUDED => {
            let mut names = Vec::with_capacity(items.len());
            for item in items {
                let name = item
                    .as_str()
                    .map(|name| name.trim().to_lowercase())
                    .unwrap_or_default();
                // 경로가 아닌 실행 파일 이름만 받는다(예: mstsc.exe).
                if name.is_empty() || name.contains(['\\', '/']) {
                    return Err(
                        "excludedProcesses에는 실행 파일 이름만 적어 주세요. 예: mstsc.exe".into(),
                    );
                }
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            names
        }
        Some(_) => {
            return Err(format!(
                "excludedProcesses는 최대 {MAX_EXCLUDED}개의 문자열 목록이어야 합니다."
            ));
        }
    };
    let auto_pause_fullscreen = match windows.get("autoPauseFullscreen") {
        None | Some(Value::Null) => defaults.auto_pause_fullscreen,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("autoPauseFullscreen은 true 또는 false여야 합니다.".into()),
    };
    let allow_injected_input = match windows.get("allowInjectedInput") {
        None | Some(Value::Null) => defaults.allow_injected_input,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("allowInjectedInput은 true 또는 false여야 합니다.".into()),
    };
    // 이 항목이 없던 이전 설정 파일은 기본값(켜짐)으로 읽는다.
    let check_updates = match windows.get("checkUpdates") {
        None | Some(Value::Null) => defaults.check_updates,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("checkUpdates는 true 또는 false여야 합니다.".into()),
    };
    Ok(Settings {
        enabled,
        threshold: threshold as u32,
        rules,
        hold_ms,
        excluded_processes,
        auto_pause_fullscreen,
        allow_injected_input,
        check_updates,
    })
}

impl Settings {
    /// 설정 파일 형식의 JSON 값(설정 창과 주고받을 때도 같은 형식을 쓴다).
    pub fn to_value(&self) -> Value {
        let rules: Vec<Value> = self.rules.iter().map(Rule::to_value).collect();
        json!({
            "schemaVersion": SCHEMA_VERSION,
            "enabled": self.enabled,
            "threshold": self.threshold,
            "rules": rules,
            "windows": {
                "holdToPassMs": self.hold_ms,
                "excludedProcesses": self.excluded_processes,
                "autoPauseFullscreen": self.auto_pause_fullscreen,
                "allowInjectedInput": self.allow_injected_input,
                "checkUpdates": self.check_updates,
            }
        })
    }

    /// 저장용 JSON. 사람이 고치기 쉽게 들여쓰기한다.
    pub fn to_json(&self) -> String {
        // serde_json 값 직렬화는 실패하지 않는다.
        serde_json::to_string_pretty(&self.to_value()).unwrap_or_default() + "\n"
    }

    pub fn is_excluded(&self, process_name: &str) -> bool {
        let name = process_name.to_lowercase();
        self.excluded_processes.contains(&name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let defaults = Settings::default();
        assert_eq!(parse(&defaults.to_json()).unwrap(), defaults);
        assert_eq!(defaults.rules.len(), 10);
    }

    #[test]
    fn accepts_extension_style_settings_without_windows_block() {
        let settings =
            parse(r#"{"enabled":true,"threshold":30,"rules":[{"gesture":"L","action":"back"}]}"#)
                .unwrap();
        assert_eq!(settings.threshold, 30);
        assert_eq!(settings.hold_ms, DEFAULT_HOLD_MS);
        assert!(settings.excluded_processes.is_empty());
    }

    #[test]
    fn rejects_like_extension() {
        // 확장 tests/core.test.cjs의 검증 사례
        assert!(parse(r#"{"enabled":true,"threshold":11,"rules":[]}"#).is_err());
        assert!(parse(r#"{"enabled":true,"threshold":49,"rules":[]}"#).is_err());
        assert!(parse(r#"{"enabled":"yes","threshold":22,"rules":[]}"#).is_err());
        let duplicate = r#"{"enabled":true,"threshold":22,"rules":[{"gesture":"L","action":"back"},{"gesture":"L","action":"reload"}]}"#;
        assert_eq!(
            parse(duplicate).unwrap_err(),
            "2번째 제스처가 중복되었습니다."
        );
        let repeated =
            r#"{"enabled":true,"threshold":22,"rules":[{"gesture":"LL","action":"back"}]}"#;
        assert_eq!(
            parse(repeated).unwrap_err(),
            "1번째 제스처의 방향이 올바르지 않습니다."
        );
        let unknown =
            r#"{"enabled":true,"threshold":22,"rules":[{"gesture":"L","action":"launchRocket"}]}"#;
        assert_eq!(
            parse(unknown).unwrap_err(),
            "1번째 제스처의 동작이 올바르지 않습니다."
        );
        let too_many: Vec<String> = (0..65)
            .map(|_| r#"{"gesture":"L","action":"back"}"#.to_string())
            .collect();
        let many = format!(
            r#"{{"enabled":true,"threshold":22,"rules":[{}]}}"#,
            too_many.join(",")
        );
        assert_eq!(
            parse(&many).unwrap_err(),
            "제스처는 최대 64개까지 저장할 수 있습니다."
        );
    }

    #[test]
    fn keyboard_rules_are_normalized() {
        let text = r#"{"enabled":true,"threshold":22,"rules":[{"gesture":"R","action":"keyboard","shortcut":"ctrl + k"}]}"#;
        let settings = parse(text).unwrap();
        assert_eq!(settings.rules[0].shortcut.as_ref().unwrap().text, "Ctrl+K");
        assert!(settings.to_json().contains("\"shortcut\": \"Ctrl+K\""));
        let bad = r#"{"enabled":true,"threshold":22,"rules":[{"gesture":"R","action":"keyboard","shortcut":""}]}"#;
        assert_eq!(
            parse(bad).unwrap_err(),
            "Ctrl+T처럼 단축키를 입력해 주세요."
        );
    }

    #[test]
    fn windows_block_is_validated() {
        let base = r#""enabled":true,"threshold":22,"rules":[]"#;
        let ok = parse(&format!(
            r#"{{{base},"windows":{{"holdToPassMs":250,"excludedProcesses":[" MSTSC.exe ","mstsc.exe"],"autoPauseFullscreen":false}}}}"#
        ))
        .unwrap();
        assert_eq!(ok.hold_ms, 250);
        assert_eq!(ok.excluded_processes, ["mstsc.exe"]);
        assert!(ok.is_excluded("Mstsc.EXE"));
        assert!(!ok.auto_pause_fullscreen);
        // 주입 입력 허용은 기본 꺼짐이고, 켜면 저장·읽기에서 유지된다
        assert!(!ok.allow_injected_input);
        let injected = parse(&format!(
            r#"{{{base},"windows":{{"allowInjectedInput":true}}}}"#
        ))
        .unwrap();
        assert!(injected.allow_injected_input);
        assert_eq!(parse(&injected.to_json()).unwrap(), injected);
        assert!(
            parse(&format!(
                r#"{{{base},"windows":{{"allowInjectedInput":1}}}}"#
            ))
            .is_err()
        );
        // 새 버전 확인은 기본 켜짐(값이 없는 이전 파일 포함)이고, 끄면 저장·읽기에서 유지된다
        assert!(ok.check_updates);
        let no_updates =
            parse(&format!(r#"{{{base},"windows":{{"checkUpdates":false}}}}"#)).unwrap();
        assert!(!no_updates.check_updates);
        assert_eq!(parse(&no_updates.to_json()).unwrap(), no_updates);
        assert!(parse(&format!(r#"{{{base},"windows":{{"checkUpdates":"no"}}}}"#)).is_err());
        assert!(parse(&format!(r#"{{{base},"windows":{{"holdToPassMs":50}}}}"#)).is_err());
        assert!(
            parse(&format!(
                r#"{{{base},"windows":{{"excludedProcesses":["C:\\app.exe"]}}}}"#
            ))
            .is_err()
        );
        assert!(parse(&format!(r#"{{{base},"schemaVersion":2}}"#)).is_err());
    }

    #[test]
    fn accepts_integer_valued_floats_like_extension() {
        let settings = parse(
            r#"{"enabled":true,"threshold":22.0,"rules":[],"windows":{"holdToPassMs":300.0}}"#,
        )
        .unwrap();
        assert_eq!((settings.threshold, settings.hold_ms), (22, 300));
        assert!(parse(r#"{"enabled":true,"threshold":22.5,"rules":[]}"#).is_err());
    }

    #[test]
    fn tolerates_bom_and_unknown_fields() {
        let text = "\u{feff}{\"enabled\":false,\"threshold\":22,\"rules\":[],\"future\":1}";
        assert!(!parse(text).unwrap().enabled);
    }
}
