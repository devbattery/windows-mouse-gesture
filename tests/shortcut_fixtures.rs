//! 확장(shared.js parseShortcut)과 같은 입력에 같은 결과를 내는지 픽스처로 검증한다.
//! 픽스처는 `node tools/gen-gesture-fixtures.cjs <shared.js>`로 확장 코드에서 생성한다.

use glide_core::shortcut;
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    input: String,
    ok: Option<Expected>,
    error: Option<String>,
}

#[derive(Deserialize, Debug, PartialEq)]
struct Expected {
    text: String,
    #[serde(rename = "keyCode")]
    key_code: u16,
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
}

#[test]
fn matches_extension_parser() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/shortcuts.json"
    ))
    .expect("fixtures/shortcuts.json을 읽지 못했습니다");
    let file: File = serde_json::from_str(&text).expect("픽스처 JSON 형식이 올바르지 않습니다");
    assert!(
        file.cases.len() >= 40,
        "픽스처가 너무 적습니다: {}개",
        file.cases.len()
    );

    let mut failures = Vec::new();
    for case in &file.cases {
        let actual = shortcut::parse(&case.input).map(|parsed| Expected {
            text: parsed.text,
            key_code: parsed.vk,
            ctrl: parsed.ctrl,
            alt: parsed.alt,
            shift: parsed.shift,
            meta: parsed.meta,
        });
        let matches = match (&actual, &case.ok, &case.error) {
            (Ok(actual), Some(expected), _) => actual == expected,
            (Err(actual), None, Some(expected)) => actual == expected,
            _ => false,
        };
        if !matches {
            failures.push(format!(
                "{:?}: 기대 {:?} / {:?}, 실제 {:?}",
                case.input, case.ok, case.error, actual
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "확장과 결과가 다릅니다:\n{}",
        failures.join("\n")
    );
}
