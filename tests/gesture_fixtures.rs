//! 확장(shared.js)과 같은 입력에 같은 결과를 내는지 픽스처로 검증한다.
//! 픽스처는 `node tools/gen-gesture-fixtures.cjs <shared.js>`로 확장 코드에서 생성한다.

use glide_core::gesture::Gesture;
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    threshold: f64,
    start: [f64; 2],
    points: Vec<[f64; 2]>,
    expected: Expected,
}

#[derive(Deserialize)]
struct Expected {
    sequence: String,
    moved: bool,
    invalid: bool,
}

#[test]
fn matches_extension_recognizer() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/fixtures/gestures.json"
    ))
    .expect("fixtures/gestures.json을 읽지 못했습니다");
    let file: File = serde_json::from_str(&text).expect("픽스처 JSON 형식이 올바르지 않습니다");
    assert!(
        file.cases.len() >= 20,
        "픽스처가 너무 적습니다: {}개",
        file.cases.len()
    );

    let mut failures = Vec::new();
    for case in &file.cases {
        let mut gesture = Gesture::new(case.start[0], case.start[1], case.threshold);
        for [x, y] in &case.points {
            gesture.feed(*x, *y);
        }
        let actual = (
            gesture.sequence().as_str().to_owned(),
            gesture.moved(),
            gesture.invalid(),
        );
        let expected = (
            case.expected.sequence.clone(),
            case.expected.moved,
            case.expected.invalid,
        );
        if actual != expected {
            failures.push(format!(
                "{}: 기대 {:?}, 실제 {:?}",
                case.name, expected, actual
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "확장과 결과가 다릅니다:\n{}",
        failures.join("\n")
    );
}
