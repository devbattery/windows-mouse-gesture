//! 제스처 백업 파일 가져오기·내보내기.
//!
//! 크롬 확장의 `createGestureBackup`·`parseGestureBackup`(shared.js)과 같은 형식을 쓴다.
//! 그래서 확장에서 내보낸 파일을 앱이 읽고, 앱이 "확장 호환"으로 내보낸 파일을 확장이 읽는다.
//!
//! - 형식: `{ app, format: "glide-gestures", formatVersion: 1, exportedAt, gestures }`
//! - 앱에서 내보낼 때는 확장의 `extensionVersion` 대신 `appVersion`과 `platform: "windows"`를 넣는다.
//!   확장은 모르는 필드를 무시한다.
//! - 사용 여부·감도·Windows 설정은 담지 않는다(기기마다 다를 수 있는 값).

use serde_json::{Value, json};

use crate::actions::{self, Effect, Rule};
use crate::settings;

pub const APP: &str = "glide-mouse-gestures";
pub const FORMAT: &str = "glide-gestures";
pub const FORMAT_VERSION: u64 = 1;

/// 가져오기 결과.
#[derive(Debug, PartialEq, Eq)]
pub struct Imported {
    /// 적용할 규칙
    pub rules: Vec<Rule>,
    /// Windows에서 실행할 수 없어 건너뛴 규칙(예: 탭 음소거). 사용자에게 목록으로 보여 준다.
    pub skipped: Vec<Rule>,
}

/// 백업 파일 내용을 읽는다. 오류 문구는 확장 `parseGestureBackup`과 같다.
pub fn parse(text: &str) -> Result<Imported, String> {
    // Windows 편집기에서 붙은 UTF-8 BOM 하나를 허용한다(확장과 같음).
    let source = text.strip_prefix('\u{feff}').unwrap_or(text);
    if source.trim().is_empty() {
        return Err("빈 파일입니다.".into());
    }
    let data: Value =
        serde_json::from_str(source).map_err(|_| "JSON 파일을 읽지 못했습니다.".to_string())?;
    let gestures = match &data {
        // 이전에 손으로 만든 규칙 배열도 같은 검증을 거쳐 받는다.
        Value::Array(_) => &data,
        Value::Object(object) if object.contains_key("format") => {
            if object.get("format").and_then(Value::as_str) != Some(FORMAT) {
                return Err("Glide 제스처 파일이 아닙니다.".into());
            }
            // 확장은 `formatVersion !== 1`로 비교하므로 1.0도 같은 값으로 본다.
            if object.get("formatVersion").and_then(Value::as_f64) != Some(FORMAT_VERSION as f64) {
                return Err("지원하지 않는 제스처 파일 버전입니다.".into());
            }
            // 빈 값이나 누락된 목록을 기본값으로 대체하지 않는다.
            match object.get("gestures") {
                Some(list @ Value::Array(_)) => list,
                _ => return Err("제스처 목록이 없는 파일입니다.".into()),
            }
        }
        _ => return Err("Glide 제스처 파일이 아닙니다.".into()),
    };
    // 앱의 검증은 확장 동작 + Windows 전용 동작을 받는다(앱에서 내보낸 파일도 읽기 위해).
    let rules = settings::validate_rules(gestures)?;
    let (skipped, rules) = rules
        .into_iter()
        .partition(|rule| rule.effect() == Effect::Unsupported);
    Ok(Imported { rules, skipped })
}

/// 내보내기 결과.
#[derive(Debug)]
pub struct Exported {
    /// 파일에 쓸 JSON 문자열(들여쓰기, 끝에 줄바꿈)
    pub text: String,
    /// 확장 호환 형식이라 뺀 규칙(Windows 전용 동작)
    pub skipped: Vec<Rule>,
}

/// 규칙을 백업 파일 형식으로 만든다.
///
/// - `exported_at`: ISO 8601 UTC 시각(예: [`timestamp`]로 만든 값)
/// - `extension_compatible`: true면 확장이 모르는 Windows 전용 동작을 빼서, 확장의 가져오기가 거절하지 않게 한다.
pub fn create(
    rules: &[Rule],
    app_version: &str,
    exported_at: &str,
    extension_compatible: bool,
) -> Exported {
    let (kept, skipped): (Vec<&Rule>, Vec<&Rule>) = rules
        .iter()
        .partition(|rule| !extension_compatible || actions::is_extension_compatible(&rule.action));
    let gestures: Vec<Value> = kept.iter().map(|rule| rule.to_value()).collect();
    let value = json!({
        "app": APP,
        "format": FORMAT,
        "formatVersion": FORMAT_VERSION,
        "exportedAt": exported_at,
        "gestures": gestures,
        "appVersion": app_version,
        "platform": "windows",
    });
    Exported {
        // serde_json 값 직렬화는 실패하지 않는다.
        text: serde_json::to_string_pretty(&value).unwrap_or_default() + "\n",
        skipped: skipped.into_iter().cloned().collect(),
    }
}

/// 유닉스 시각(밀리초)을 JavaScript `Date#toISOString`과 같은 형식으로 바꾼다.
/// 예: `2026-11-13T09:01:02.699Z`. 날짜 계산은 Howard Hinnant의 civil_from_days 알고리즘이다.
pub fn timestamp(unix_millis: u64) -> String {
    let millis = unix_millis % 1_000;
    let seconds = unix_millis / 1_000;
    let (days, day_seconds) = (seconds / 86_400, seconds % 86_400);
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        day_seconds / 3_600,
        day_seconds % 3_600 / 60,
        day_seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(pairs: &[(&str, &str)]) -> Vec<Rule> {
        pairs
            .iter()
            .map(|(gesture, action)| Rule {
                gesture: (*gesture).into(),
                action: (*action).into(),
                shortcut: None,
            })
            .collect()
    }

    #[test]
    fn imports_windows_actions_and_skips_unsupported() {
        let text = r#"{"format":"glide-gestures","formatVersion":1,"gestures":[
            {"gesture":"L","action":"back"},
            {"gesture":"D","action":"minimizeWindow"},
            {"gesture":"U","action":"muteTab"}]}"#;
        let imported = parse(text).unwrap();
        assert_eq!(
            imported.rules,
            rules(&[("L", "back"), ("D", "minimizeWindow")])
        );
        assert_eq!(imported.skipped, rules(&[("U", "muteTab")]));
    }

    #[test]
    fn round_trips_own_export() {
        let original = rules(&[("L", "back"), ("RD", "closeWindow")]);
        let exported = create(&original, "0.3.0", "2026-11-13T00:00:00.000Z", false);
        assert!(exported.skipped.is_empty());
        let value: Value = serde_json::from_str(&exported.text).unwrap();
        assert_eq!(value["platform"], "windows");
        assert_eq!(value["appVersion"], "0.3.0");
        assert_eq!(parse(&exported.text).unwrap().rules, original);
    }

    #[test]
    fn extension_export_drops_windows_only_actions() {
        let original = rules(&[("L", "back"), ("RD", "closeWindow"), ("U", "muteTab")]);
        let exported = create(&original, "0.3.0", "2026-11-13T00:00:00.000Z", true);
        assert_eq!(exported.skipped, rules(&[("RD", "closeWindow")]));
        let value: Value = serde_json::from_str(&exported.text).unwrap();
        // 확장은 muteTab을 실행할 수 있으므로 남긴다.
        assert_eq!(value["gestures"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn accepts_float_version_like_javascript() {
        let text = r#"{"format":"glide-gestures","formatVersion":1.0,"gestures":[]}"#;
        assert!(parse(text).unwrap().rules.is_empty());
    }

    #[test]
    fn formats_timestamp_like_javascript() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        // new Date(Date.UTC(2026, 10, 13, 9, 1, 2, 699)).getTime()
        assert_eq!(timestamp(1_794_560_462_699), "2026-11-13T09:01:02.699Z");
        // 윤년 2월 29일
        assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }
}
