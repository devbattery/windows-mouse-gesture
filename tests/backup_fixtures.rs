//! 확장(shared.js parseGestureBackup)과 같은 백업 파일에 같은 결과를 내는지 픽스처로 검증한다.
//! 픽스처는 `node tools/gen-gesture-fixtures.cjs <shared.js>`로 확장 코드에서 생성한다.
//!
//! 앱은 Windows에서 실행할 수 없는 동작(탭 복제·음소거)을 `skipped`로 따로 돌려주므로,
//! 확장이 받아들인 목록 = 앱의 `rules` + `skipped`(각각 원래 순서 유지)인지 확인한다.

use glide_core::actions::{self, Effect};
use glide_core::backup;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct File {
    #[serde(rename = "extensionActions")]
    extension_actions: Vec<String>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    input: String,
    gestures: Option<Vec<Value>>,
    error: Option<String>,
}

fn load() -> File {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/backups.json"
    ))
    .expect("fixtures/backups.json을 읽지 못했습니다");
    serde_json::from_str(&text).expect("픽스처 JSON 형식이 올바르지 않습니다")
}

#[test]
fn matches_extension_backup_parser() {
    let file = load();
    assert!(file.cases.len() >= 20, "픽스처가 너무 적습니다");
    let mut failures = Vec::new();
    for case in &file.cases {
        let actual = backup::parse(&case.input);
        let matches = match (&actual, &case.gestures, &case.error) {
            (Ok(imported), Some(expected), _) => {
                let (unsupported, supported): (Vec<&Value>, Vec<&Value>) =
                    expected.iter().partition(|rule| {
                        // 직접 지정한 단축키(keyboard)는 동작 표에 없지만 항상 실행할 수 있다.
                        let action = rule["action"].as_str().unwrap_or_default();
                        action != "keyboard" && actions::effect(action) == Effect::Unsupported
                    });
                let values = |rules: &[actions::Rule]| -> Vec<Value> {
                    rules.iter().map(actions::Rule::to_value).collect()
                };
                values(&imported.rules) == supported.into_iter().cloned().collect::<Vec<_>>()
                    && values(&imported.skipped)
                        == unsupported.into_iter().cloned().collect::<Vec<_>>()
            }
            (Err(actual), None, Some(expected)) => actual == expected,
            _ => false,
        };
        if !matches {
            failures.push(format!(
                "{:?}: 기대 {:?} / {:?}, 실제 {:?}",
                case.input.chars().take(60).collect::<String>(),
                case.gestures,
                case.error,
                actual
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "확장과 결과가 다릅니다:\n{}",
        failures.join("\n")
    );
}

#[test]
fn extension_compatible_export_only_uses_extension_actions() {
    let file = load();
    // 앱의 "확장 호환" 판정이 확장의 실제 동작 목록과 같아야 한다.
    for entry in actions::catalog() {
        assert_eq!(
            actions::is_extension_compatible(entry.id),
            file.extension_actions.iter().any(|id| id == entry.id),
            "{}",
            entry.id
        );
    }
    // 앱의 모든 동작을 하나씩 담아 확장 호환으로 내보내면 확장이 아는 동작만 남는다.
    let rules: Vec<actions::Rule> = actions::catalog()
        .iter()
        .zip([
            "L", "R", "U", "D", "LR", "LU", "LD", "RL", "RU", "RD", "UL", "UR", "UD", "DL", "DR",
            "DU", "LRL", "LRU", "LRD", "LUL", "LUR", "LUD", "LDL",
        ])
        .map(|(entry, gesture)| actions::Rule {
            gesture: gesture.into(),
            action: entry.id.into(),
            shortcut: None,
        })
        .collect();
    let exported = backup::create(&rules, "0.3.0", "2026-11-13T00:00:00.000Z", true);
    let value: Value = serde_json::from_str(&exported.text).unwrap();
    for rule in value["gestures"].as_array().unwrap() {
        let action = rule["action"].as_str().unwrap();
        assert!(
            file.extension_actions.iter().any(|id| id == action),
            "확장이 모르는 동작: {action}"
        );
    }
    assert_eq!(exported.skipped.len(), 5);
}
