//! 단축키 문자열 파서.
//!
//! 크롬 확장 `shared.js`의 `parseShortcut`을 옮겼다. 같은 입력에 같은 표기(`text`)와
//! 키 코드를 내는지는 `fixtures/shortcuts.json`(확장 코드로 생성)으로 검증한다.
//! 확장의 키 코드(`KeyboardEvent.keyCode`)는 이 범위에서 Windows 가상 키 코드와 같다.

/// 파싱된 단축키 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// 확장의 Cmd(meta). Windows에서는 Win 키로 보낸다.
    pub meta: bool,
    /// Windows 가상 키 코드
    pub vk: u16,
    /// 방향키·Home·End처럼 확장 키보드 영역의 키
    pub extended: bool,
    /// 정규화한 표기(예: "Ctrl+Shift+T"). 확장과 같다.
    pub text: String,
}

const EMPTY_MESSAGE: &str = "Ctrl+T처럼 단축키를 입력해 주세요.";
const MODIFIER_MESSAGE: &str = "보조 키는 Ctrl, Alt, Shift, Cmd를 한 번씩만 입력해 주세요.";
const UNSUPPORTED_MESSAGE: &str = "지원하지 않는 키입니다. 예: Ctrl+T, Alt+Left, Shift+F, Space";

/// 이름 키: (소문자 이름, 표기, 가상 키, 확장 키 여부)
const NAMED_KEYS: [(&str, &str, u16, bool); 15] = [
    ("space", "Space", 32, false),
    ("enter", "Enter", 13, false),
    ("tab", "Tab", 9, false),
    ("escape", "Escape", 27, false),
    ("backspace", "Backspace", 8, false),
    ("delete", "Delete", 46, true),
    ("insert", "Insert", 45, true),
    ("home", "Home", 36, true),
    ("end", "End", 35, true),
    ("pageup", "PageUp", 33, true),
    ("pagedown", "PageDown", 34, true),
    ("arrowleft", "ArrowLeft", 37, true),
    ("arrowup", "ArrowUp", 38, true),
    ("arrowright", "ArrowRight", 39, true),
    ("arrowdown", "ArrowDown", 40, true),
];

/// 구두점(US 배열): (기본 글자, 가상 키 VK_OEM_*, Shift 글자)
const PUNCTUATION: [(&str, u16, &str); 11] = [
    ("-", 189, "_"),
    ("=", 187, "+"),
    ("[", 219, "{"),
    ("]", 221, "}"),
    ("\\", 220, "|"),
    (";", 186, ":"),
    ("'", 222, "\""),
    (",", 188, "<"),
    (".", 190, ">"),
    ("/", 191, "?"),
    ("`", 192, "~"),
];

const ALIASES: [(&str, &str); 10] = [
    ("left", "arrowleft"),
    ("right", "arrowright"),
    ("up", "arrowup"),
    ("down", "arrowdown"),
    ("esc", "escape"),
    ("return", "enter"),
    ("spacebar", "space"),
    ("del", "delete"),
    ("pgup", "pageup"),
    ("pgdn", "pagedown"),
];

/// "Ctrl+T"·"Alt+Left"·"Shift+F5"·"Ctrl++" 같은 문자열을 파싱한다. 오류 문구는 확장과 같다.
pub fn parse(text: &str) -> Result<Shortcut, String> {
    // 확장은 JS 문자열 길이(UTF-16 단위)로 80자를 제한한다.
    if text.trim().is_empty() || text.encode_utf16().count() > 80 {
        return Err(EMPTY_MESSAGE.into());
    }
    // 끝의 "+"(예: "Ctrl++")는 Plus 키로 읽는다. 확장의 `replace(/\+\s*$/, "Plus")`와 같다.
    let trimmed = text.trim();
    let normalized = match trimmed.strip_suffix('+') {
        Some(rest) => format!("{rest}Plus"),
        None => trimmed.to_owned(),
    };
    let mut parts: Vec<&str> = normalized.split('+').map(str::trim).collect();
    let mut name = parts.pop().unwrap_or_default().to_owned();

    let (mut ctrl, mut alt, mut shift, mut meta) = (false, false, false, false);
    for part in parts {
        let slot = match part.to_lowercase().as_str() {
            "ctrl" | "control" => &mut ctrl,
            "alt" | "option" => &mut alt,
            "shift" => &mut shift,
            "meta" | "cmd" | "command" => &mut meta,
            _ => return Err(MODIFIER_MESSAGE.into()),
        };
        if *slot {
            return Err(MODIFIER_MESSAGE.into());
        }
        *slot = true;
    }

    let lower = name.to_lowercase();
    let lookup = ALIASES
        .iter()
        .find(|(alias, _)| *alias == lower)
        .map_or(lower.as_str(), |(_, target)| *target)
        .to_owned();
    let (vk, extended);
    if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic()) {
        name = name.to_ascii_uppercase();
        vk = name.as_bytes()[0] as u16;
        extended = false;
    } else if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) {
        vk = name.as_bytes()[0] as u16;
        extended = false;
    } else if let Some(number) = function_key(&name) {
        name = format!("F{number}");
        vk = 111 + number;
        extended = false;
    } else if let Some((_, label, code, ext)) = NAMED_KEYS.iter().find(|(key, ..)| *key == lookup) {
        name = (*label).to_owned();
        vk = *code;
        extended = *ext;
    } else {
        if lookup == "plus" {
            name = "+".into();
        }
        let Some((key, code, shifted)) = PUNCTUATION
            .iter()
            .find(|(key, _, shifted)| *key == name || *shifted == name)
        else {
            return Err(UNSUPPORTED_MESSAGE.into());
        };
        if name == *shifted {
            shift = true;
        }
        name = (*key).to_owned();
        vk = *code;
        extended = false;
    }

    let text = [
        ctrl.then_some("Ctrl"),
        alt.then_some("Alt"),
        shift.then_some("Shift"),
        meta.then_some("Cmd"),
        Some(name.as_str()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("+");
    Ok(Shortcut {
        ctrl,
        alt,
        shift,
        meta,
        vk,
        extended,
        text,
    })
}

/// "F1"~"F24"(대소문자 무관)이면 번호를 돌려준다.
fn function_key(name: &str) -> Option<u16> {
    let digits = name.strip_prefix(['f', 'F'])?;
    if digits.is_empty() || digits.len() > 2 || digits.starts_with('0') {
        return None;
    }
    let number: u16 = digits.parse().ok()?;
    (1..=24).contains(&number).then_some(number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_shortcuts() {
        let shortcut = parse("ctrl + shift + t").unwrap();
        assert_eq!(shortcut.text, "Ctrl+Shift+T");
        assert_eq!(shortcut.vk, b'T' as u16);
        assert!(shortcut.ctrl && shortcut.shift && !shortcut.alt);

        let back = parse("Alt+Left").unwrap();
        assert_eq!(back.text, "Alt+ArrowLeft");
        assert_eq!(back.vk, 37);
        assert!(back.extended);

        assert_eq!(parse("Ctrl++").unwrap().text, "Ctrl+Shift+=");
        assert_eq!(parse("shift+f5").unwrap().text, "Shift+F5");
        assert_eq!(parse("Cmd+D").unwrap().text, "Cmd+D");
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse("").unwrap_err(), EMPTY_MESSAGE);
        assert_eq!(parse("Ctrl+Ctrl+T").unwrap_err(), MODIFIER_MESSAGE);
        assert_eq!(parse("Hyper+T").unwrap_err(), MODIFIER_MESSAGE);
        assert_eq!(parse("Ctrl+F25").unwrap_err(), UNSUPPORTED_MESSAGE);
    }
}
