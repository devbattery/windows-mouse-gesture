//! 새 버전 확인의 순수 로직(통신은 코어의 `win/update.rs`가 한다).
//!
//! GitHub Releases API(`/releases/latest`) 응답에서 `tag_name`만 읽어 버전을 비교한다.
//! 응답 속 주소(`html_url` 등)는 쓰지 않고, 검증한 버전 숫자로 Release 페이지 주소를 직접 만든다.
//! 네트워크에서 받은 값이 임의의 주소를 열게 하지 않기 위해서다.

use serde_json::Value;

/// 최신 Release 정보를 묻는 주소(초안·시험판은 GitHub이 빼고 돌려준다).
pub const API_HOST: &str = "api.github.com";
pub const API_PATH: &str = "/repos/devbattery/windows-mouse-gesture/releases/latest";
/// 응답 본문 최대 크기. 실제 응답은 수 KB이며, 이보다 크면 비정상으로 본다.
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// `주.부.수` 형식 버전. 시험판 꼬리(`-rc.1` 등)는 받지 않는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// `0.5.4` 또는 `v0.5.4`를 읽는다. 각 자리는 숫자 1~5자만 허용한다.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.strip_prefix('v').unwrap_or(text);
        let mut parts = text.split('.');
        let mut next = || {
            let part = parts.next()?;
            if part.is_empty() || part.len() > 5 || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        };
        let version = Self {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        // 네 번째 자리가 있으면 이 프로젝트의 버전 형식이 아니다.
        parts.next().is_none().then_some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// `/releases/latest` 응답에서 최신 버전을 읽는다.
pub fn parse_latest(body: &[u8]) -> Result<Version, String> {
    if body.len() > MAX_RESPONSE_BYTES {
        return Err("응답이 너무 큽니다.".into());
    }
    let value: Value =
        serde_json::from_slice(body).map_err(|_| "응답 형식이 올바르지 않습니다.".to_string())?;
    let tag = value
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or("응답에 버전(tag_name)이 없습니다.")?;
    Version::parse(tag).ok_or_else(|| format!("알 수 없는 버전 형식입니다: {tag}"))
}

/// 지금 버전보다 새 버전인지.
pub fn is_newer(current: Version, latest: Version) -> bool {
    latest > current
}

/// 그 버전의 Release 페이지 주소(검증한 숫자로만 만든다).
pub fn release_url(version: Version) -> String {
    format!("https://github.com/devbattery/windows-mouse-gesture/releases/tag/v{version}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn parses_project_versions() {
        assert_eq!(
            v("v0.5.4"),
            Version {
                major: 0,
                minor: 5,
                patch: 4
            }
        );
        assert_eq!(v("1.10.0").to_string(), "1.10.0");
        for bad in [
            "",
            "v",
            "0.5",
            "0.5.4.1",
            "0.5.x",
            "0.5.4-rc.1",
            " 0.5.4",
            "0..4",
            "+1.0.0",
            "123456.0.0",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn compares_numerically() {
        assert!(is_newer(v("0.5.3"), v("0.5.4")));
        assert!(is_newer(v("0.5.9"), v("0.10.0")));
        assert!(is_newer(v("0.9.9"), v("1.0.0")));
        assert!(!is_newer(v("0.5.4"), v("0.5.4")));
        assert!(!is_newer(v("0.6.0"), v("0.5.9")));
    }

    #[test]
    fn reads_tag_from_github_response() {
        // 실제 응답처럼 다른 필드가 많고, 주소 필드는 무시한다.
        let body = br#"{"url":"https://api.github.com/x","html_url":"https://evil.example/","tag_name":"v0.6.0","name":"Glide","draft":false,"prerelease":false,"assets":[{"name":"glide.zip","size":1}]}"#;
        let latest = parse_latest(body).unwrap();
        assert_eq!(latest, v("0.6.0"));
        assert_eq!(
            release_url(latest),
            "https://github.com/devbattery/windows-mouse-gesture/releases/tag/v0.6.0"
        );
    }

    #[test]
    fn rejects_bad_responses() {
        assert!(parse_latest(b"not json").is_err());
        assert!(parse_latest(br#"{"message":"Not Found"}"#).is_err());
        assert!(parse_latest(br#"{"tag_name":"v1.0.0/../../x"}"#).is_err());
        assert!(parse_latest(br#"{"tag_name":5}"#).is_err());
        let huge = vec![b' '; MAX_RESPONSE_BYTES + 1];
        assert_eq!(parse_latest(&huge).unwrap_err(), "응답이 너무 큽니다.");
    }
}
