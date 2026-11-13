//! 제스처 동작 정의와 HUD 문구.
//!
//! 동작 ID와 라벨은 크롬 확장 `shared.js`의 `actions`와 같다(설정·백업 호환).
//! 확장은 브라우저 API로 실행하지만 Windows 앱은 실제 키 입력이나 창 명령으로 실행하므로,
//! 각 동작의 실행 방식([`Effect`])과 안내용 단축키 표기는 Windows 기준이다.

use crate::gesture::Sequence;
use crate::shortcut::{self, Shortcut};

/// 동작 실행 방식.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// 대상 창을 전면으로 확인한 뒤 키 입력을 보낸다
    Keys(Shortcut),
    /// 대상 창 최소화
    MinimizeWindow,
    /// 대상 창 최대화 ↔ 복원
    ToggleMaximize,
    /// 대상 창에 닫기 요청(WM_CLOSE)
    CloseWindow,
    /// 미디어 재생/일시정지 키
    MediaPlayPause,
    /// Windows에 공통 단축키가 없어 실행할 수 없음
    Unsupported,
}

/// 동작 메타데이터: (ID, 라벨, Windows 단축키 안내, 키 입력 표기)
/// 키 입력 표기가 빈 문자열이면 키 입력이 아닌 동작이다.
const ACTIONS: [(&str, &str, &str, &str); 23] = [
    ("back", "뒤로 가기", "Alt + ←", "Alt+Left"),
    ("forward", "앞으로 가기", "Alt + →", "Alt+Right"),
    ("reload", "새로고침", "F5", "F5"),
    ("hardReload", "강력 새로고침", "Ctrl + F5", "Ctrl+F5"),
    ("newTab", "새 탭", "Ctrl + T", "Ctrl+T"),
    ("newWindow", "새 창", "Ctrl + N", "Ctrl+N"),
    ("closeTab", "현재 탭 닫기", "Ctrl + W", "Ctrl+W"),
    (
        "restoreTab",
        "닫은 탭 다시 열기",
        "Ctrl + Shift + T",
        "Ctrl+Shift+T",
    ),
    (
        "previousTab",
        "이전 탭",
        "Ctrl + Shift + Tab",
        "Ctrl+Shift+Tab",
    ),
    ("nextTab", "다음 탭", "Ctrl + Tab", "Ctrl+Tab"),
    ("duplicateTab", "탭 복제", "", ""),
    ("muteTab", "탭 음소거 켜기 / 끄기", "", ""),
    ("scrollTop", "맨 위로", "Ctrl + Home", "Ctrl+Home"),
    ("scrollBottom", "맨 아래로", "Ctrl + End", "Ctrl+End"),
    ("zoomIn", "확대", "Ctrl + +", "Ctrl+="),
    ("zoomOut", "축소", "Ctrl + -", "Ctrl+-"),
    ("zoomReset", "원래 크기로", "Ctrl + 0", "Ctrl+0"),
    ("fullscreen", "전체 화면 전환", "F11", "F11"),
    // Windows 전용 동작
    ("minimizeWindow", "창 최소화", "", ""),
    ("maximizeWindow", "창 최대화 / 복원", "", ""),
    ("closeWindow", "창 닫기", "", ""),
    ("showDesktop", "바탕 화면 보기", "Win + D", "Cmd+D"),
    ("mediaPlayPause", "미디어 재생 / 일시정지", "", ""),
];

/// 설정에 쓸 수 있는 동작 ID인지(직접 지정한 단축키 "keyboard" 제외).
pub fn is_known(id: &str) -> bool {
    ACTIONS.iter().any(|(action, ..)| *action == id)
}

/// 모든 동작 ID(설정 화면·문서용).
pub fn ids() -> impl Iterator<Item = &'static str> {
    ACTIONS.iter().map(|(id, ..)| *id)
}

fn meta(id: &str) -> Option<&'static (&'static str, &'static str, &'static str, &'static str)> {
    ACTIONS.iter().find(|(action, ..)| *action == id)
}

/// 동작의 실행 방식. 알 수 없는 ID면 `Unsupported`.
pub fn effect(id: &str) -> Effect {
    match id {
        "minimizeWindow" => Effect::MinimizeWindow,
        "maximizeWindow" => Effect::ToggleMaximize,
        "closeWindow" => Effect::CloseWindow,
        "mediaPlayPause" => Effect::MediaPlayPause,
        _ => match meta(id) {
            Some((_, _, _, keys)) if !keys.is_empty() => {
                // 표의 키 입력 표기는 테스트로 모두 파싱되는지 확인한다.
                shortcut::parse(keys).map_or(Effect::Unsupported, Effect::Keys)
            }
            _ => Effect::Unsupported,
        },
    }
}

/// 설정에 저장되는 제스처 규칙 하나. `action`이 "keyboard"면 `shortcut`을 보낸다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub gesture: String,
    pub action: String,
    pub shortcut: Option<Shortcut>,
}

impl Rule {
    pub fn effect(&self) -> Effect {
        match &self.shortcut {
            Some(shortcut) if self.action == "keyboard" => Effect::Keys(shortcut.clone()),
            _ => effect(&self.action),
        }
    }
}

/// HUD 안내 톤. 확장 오버레이의 data-tone과 같다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// 실행될 동작
    Ok,
    /// 진행 중·미등록
    Muted,
    /// 실패·경고
    Error,
}

/// HUD에 표시할 문구.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hud {
    pub title: String,
    pub detail: String,
    pub tone: Tone,
}

/// 규칙의 큰 글자(동작 이름)와 보조 글자(단축키). 확장 `ruleParts`와 같은 방식이다.
pub fn rule_parts(rule: &Rule) -> (String, String) {
    if rule.action == "keyboard" {
        let text = rule.shortcut.as_ref().map_or("", |s| s.text.as_str());
        // 직접 지정한 단축키가 기본 동작과 같으면 그 동작 이름을 보여 준다.
        let title = ACTIONS
            .iter()
            .find(|(_, _, _, keys)| {
                !keys.is_empty() && shortcut::parse(keys).is_ok_and(|parsed| parsed.text == text)
            })
            .map_or("키 입력", |(_, label, ..)| *label);
        return (title.into(), text.into());
    }
    match meta(&rule.action) {
        Some((_, label, hint, _)) => {
            let detail = if effect(&rule.action) == Effect::Unsupported {
                "Windows에서 지원하지 않는 동작"
            } else if hint.is_empty() {
                "기본 단축키 없음"
            } else {
                hint
            };
            ((*label).into(), detail.into())
        }
        None => (rule.action.clone(), String::new()),
    }
}

/// 그리는 중인 제스처의 HUD 문구. 확장 `showGestureLabel`과 같은 규칙이다.
pub fn gesture_hud(sequence: Sequence, invalid: bool, rules: &[Rule]) -> Hud {
    if invalid {
        return Hud {
            title: "너무 긴 제스처".into(),
            detail: "Esc로 취소".into(),
            tone: Tone::Error,
        };
    }
    if let Some(rule) = rules.iter().find(|rule| rule.gesture == sequence.as_str()) {
        let (title, detail) = rule_parts(rule);
        return Hud {
            title,
            detail,
            tone: Tone::Ok,
        };
    }
    // 더 긴 규칙으로 이어질 수 있으면 진행 중으로 안내한다.
    let continuing = rules.iter().any(|rule| {
        rule.gesture.len() > sequence.len() && rule.gesture.starts_with(sequence.as_str())
    });
    let title = if continuing {
        "계속 그리세요"
    } else {
        "등록되지 않은 제스처"
    };
    Hud {
        title: title.into(),
        detail: String::new(),
        tone: Tone::Muted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gesture::Gesture;

    fn sequence(points: &[(f64, f64)]) -> Sequence {
        let mut gesture = Gesture::new(0.0, 0.0, 22.0);
        for (x, y) in points {
            gesture.feed(*x, *y);
        }
        gesture.sequence()
    }

    fn rule(gesture: &str, action: &str) -> Rule {
        Rule {
            gesture: gesture.into(),
            action: action.into(),
            shortcut: None,
        }
    }

    #[test]
    fn every_key_action_parses() {
        for (id, _, _, keys) in ACTIONS {
            if !keys.is_empty() {
                assert!(shortcut::parse(keys).is_ok(), "{id}: {keys}");
                assert!(matches!(effect(id), Effect::Keys(_)), "{id}");
            }
        }
    }

    #[test]
    fn maps_windows_effects() {
        assert_eq!(effect("duplicateTab"), Effect::Unsupported);
        assert_eq!(effect("muteTab"), Effect::Unsupported);
        assert_eq!(effect("minimizeWindow"), Effect::MinimizeWindow);
        let Effect::Keys(back) = effect("back") else {
            panic!("back은 키 입력이어야 합니다")
        };
        assert!(back.alt && back.vk == 37);
        let Effect::Keys(desktop) = effect("showDesktop") else {
            panic!()
        };
        assert!(desktop.meta && desktop.vk == b'D' as u16);
    }

    #[test]
    fn hud_follows_extension_rules() {
        let rules = vec![
            rule("L", "back"),
            rule("DR", "closeTab"),
            rule("DU", "muteTab"),
        ];
        let left = gesture_hud(sequence(&[(-50.0, 0.0)]), false, &rules);
        assert_eq!(
            (left.title.as_str(), left.detail.as_str(), left.tone),
            ("뒤로 가기", "Alt + ←", Tone::Ok)
        );
        let down = gesture_hud(sequence(&[(0.0, 50.0)]), false, &rules);
        assert_eq!(
            (down.title.as_str(), down.tone),
            ("계속 그리세요", Tone::Muted)
        );
        let up = gesture_hud(sequence(&[(0.0, -50.0)]), false, &rules);
        assert_eq!(up.title, "등록되지 않은 제스처");
        let unsupported = gesture_hud(sequence(&[(0.0, 50.0), (0.0, 0.0)]), false, &rules);
        assert_eq!(unsupported.detail, "Windows에서 지원하지 않는 동작");
        assert_eq!(
            gesture_hud(Sequence::default(), true, &rules).tone,
            Tone::Error
        );
    }

    #[test]
    fn keyboard_rule_uses_matching_action_name() {
        let mut custom = rule("R", "keyboard");
        custom.shortcut = Some(shortcut::parse("ctrl+t").unwrap());
        assert_eq!(rule_parts(&custom), ("새 탭".into(), "Ctrl+T".into()));
        custom.shortcut = Some(shortcut::parse("ctrl+k").unwrap());
        assert_eq!(rule_parts(&custom), ("키 입력".into(), "Ctrl+K".into()));
    }
}
