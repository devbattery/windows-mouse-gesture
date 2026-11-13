//! 최근 우클릭 기록.
//!
//! 일상 사용 중 "어느 앱에서 우클릭·제스처가 어떻게 처리됐는지"를 설정 창에서 보고, 문제가
//! 있는 앱을 예외에 넣을 수 있게 한다. 메모리에만 두고 파일에 남기지 않는다. 앱은 실행 파일
//! 이름만 기록하고, 창 제목과 좌표는 기록하지 않는다(개인정보).

use std::collections::VecDeque;

use serde_json::{Value, json};

use crate::machine::Ending;

/// 보관할 최근 기록 수
pub const CAPACITY: usize = 50;

/// 우클릭 하나가 끝난 방식.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 제스처 동작을 실행함
    Executed,
    /// 제스처 동작을 실행하지 못함(창 활성화 실패, 예외 앱, 바탕 화면 등)
    Failed,
    /// 등록되지 않은 방향
    Unmatched,
    /// 방향이 없거나 최대 획 수를 넘음
    NotGesture,
    /// 움직이지 않고 뗌 → 단순 우클릭
    Click,
    /// 오래 누름 → 원래 우클릭으로 통과
    Hold,
    /// 예외 앱 → 원래 우클릭으로 통과
    Excluded,
    /// 다른 버튼과 함께 눌림 → 원래 우클릭으로 통과
    OtherButton,
    /// 제스처 취소(Esc·다른 버튼·화면 잠금 등)
    Cancelled,
}

const KINDS: [Kind; 9] = [
    Kind::Executed,
    Kind::Failed,
    Kind::Unmatched,
    Kind::NotGesture,
    Kind::Click,
    Kind::Hold,
    Kind::Excluded,
    Kind::OtherButton,
    Kind::Cancelled,
];

impl Kind {
    /// JSON에 쓰는 이름(설정 창이 한국어 문구로 바꾼다).
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Executed => "executed",
            Kind::Failed => "failed",
            Kind::Unmatched => "unmatched",
            Kind::NotGesture => "notGesture",
            Kind::Click => "click",
            Kind::Hold => "hold",
            Kind::Excluded => "excluded",
            Kind::OtherButton => "otherButton",
            Kind::Cancelled => "cancelled",
        }
    }

    fn index(self) -> usize {
        KINDS.iter().position(|kind| *kind == self).unwrap_or(0)
    }
}

impl From<Ending> for Kind {
    fn from(ending: Ending) -> Self {
        match ending {
            Ending::Click => Kind::Click,
            Ending::Hold => Kind::Hold,
            Ending::Excluded => Kind::Excluded,
            Ending::OtherButton => Kind::OtherButton,
            Ending::Cancelled => Kind::Cancelled,
        }
    }
}

/// 기록 한 건.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 유닉스 시각(ms)
    pub at: u64,
    /// 실행 파일 이름(예: notepad.exe). 알 수 없으면 빈 문자열
    pub app: String,
    pub kind: Kind,
    /// 동작 ID, 방향 문자열, 실패 이유 등
    pub detail: Option<String>,
}

/// 최근 기록(최대 [`CAPACITY`]건)과 시작 이후 종류별 횟수.
#[derive(Debug, Default)]
pub struct History {
    entries: VecDeque<Entry>,
    counts: [u64; KINDS.len()],
}

impl History {
    pub fn push(&mut self, entry: Entry) {
        self.counts[entry.kind.index()] += 1;
        if self.entries.len() == CAPACITY {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    pub fn count(&self, kind: Kind) -> u64 {
        self.counts[kind.index()]
    }

    /// 설정 창용 JSON. 최근 기록은 최신순이다.
    pub fn to_value(&self) -> Value {
        let entries: Vec<Value> = self
            .entries
            .iter()
            .rev()
            .map(|entry| {
                json!({
                    "at": entry.at,
                    "app": entry.app,
                    "kind": entry.kind.as_str(),
                    "detail": entry.detail,
                })
            })
            .collect();
        let counts: serde_json::Map<String, Value> = KINDS
            .iter()
            .map(|kind| (kind.as_str().to_string(), json!(self.count(*kind))))
            .collect();
        json!({ "entries": entries, "counts": counts })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(at: u64, kind: Kind) -> Entry {
        Entry {
            at,
            app: "notepad.exe".into(),
            kind,
            detail: None,
        }
    }

    #[test]
    fn keeps_latest_entries_and_total_counts() {
        let mut history = History::default();
        for at in 0..(CAPACITY as u64 + 5) {
            history.push(entry(at, Kind::Click));
        }
        history.push(entry(1_000, Kind::Hold));
        let value = history.to_value();
        let entries = value["entries"].as_array().unwrap();
        assert_eq!(entries.len(), CAPACITY);
        // 최신순
        assert_eq!(entries[0]["at"], 1_000);
        assert_eq!(entries[0]["kind"], "hold");
        assert_eq!(entries[CAPACITY - 1]["at"], 6);
        // 횟수는 버린 기록까지 센다
        assert_eq!(value["counts"]["click"], CAPACITY as u64 + 5);
        assert_eq!(value["counts"]["hold"], 1);
        assert_eq!(value["counts"]["executed"], 0);
    }

    #[test]
    fn endings_map_to_kinds() {
        assert_eq!(Kind::from(Ending::Click), Kind::Click);
        assert_eq!(Kind::from(Ending::Cancelled).as_str(), "cancelled");
        // 모든 종류의 이름이 서로 다르다
        let mut names: Vec<_> = KINDS.iter().map(|kind| kind.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KINDS.len());
    }
}
