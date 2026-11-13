//! 제스처 방향 인식기.
//!
//! 크롬 확장 Glide의 `shared.js` `Gesture` 클래스를 그대로 옮겼다. 두 구현이 같은
//! 결과를 내는지는 `fixtures/gestures.json`(확장 코드로 생성)으로 함께 검증한다.
//! 훅 스레드에서 호출되므로 힙 할당을 하지 않는다.

/// 한 제스처에 담을 수 있는 최대 획 수. 확장과 같다.
pub const MAX_STROKES: usize = 12;
/// 방향 판정 임계값(확장 기준 CSS px = 96 DPI 기준 px)의 허용 범위와 기본값.
pub const MIN_THRESHOLD: u32 = 12;
pub const MAX_THRESHOLD: u32 = 48;
pub const DEFAULT_THRESHOLD: u32 = 22;
/// 대각선 떨림 판정 비율. 짧은 축이 긴 축의 80%를 넘으면 방향이 분명하지 않다고 본다.
const DIAGONAL_RATIO: f64 = 0.8;

/// 한 획의 방향. 확장과 같은 글자(L·R·U·D)로 표기한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn as_byte(self) -> u8 {
        match self {
            Direction::Left => b'L',
            Direction::Right => b'R',
            Direction::Up => b'U',
            Direction::Down => b'D',
        }
    }

    fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            b'L' => Some(Direction::Left),
            b'R' => Some(Direction::Right),
            b'U' => Some(Direction::Up),
            b'D' => Some(Direction::Down),
            _ => None,
        }
    }
}

/// 고정 길이 방향 시퀀스. 훅 스레드와 채널에서 할당 없이 복사해 쓰도록 배열에 담는다.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct Sequence {
    bytes: [u8; MAX_STROKES],
    len: u8,
}

impl Sequence {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn last(&self) -> Option<Direction> {
        self.len
            .checked_sub(1)
            .and_then(|index| Direction::from_byte(self.bytes[index as usize]))
    }

    /// "DR"처럼 확장과 같은 표기로 돌려준다. 저장 바이트는 항상 ASCII L·R·U·D다.
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len()]).unwrap_or("")
    }

    /// "← ↓"처럼 화살표로 표기한다(확장 `arrows`와 같음).
    pub fn arrows(&self) -> String {
        let mut text = String::with_capacity(self.len() * 4);
        for (index, byte) in self.bytes[..self.len()].iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            text.push(match byte {
                b'L' => '←',
                b'R' => '→',
                b'U' => '↑',
                _ => '↓',
            });
        }
        text
    }

    fn push(&mut self, direction: Direction) {
        // 호출 측(Gesture::feed)이 MAX_STROKES를 넘지 않게 보장한다.
        if self.len() < MAX_STROKES {
            self.bytes[self.len()] = direction.as_byte();
            self.len += 1;
        }
    }
}

impl std::fmt::Debug for Sequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

impl std::fmt::Display for Sequence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 진행 중인 제스처 하나의 인식 상태.
#[derive(Clone, Copy, Debug)]
pub struct Gesture {
    /// 마지막으로 방향을 확정한 기준점
    x: f64,
    y: f64,
    /// 이 거리(px) 이상 움직여야 방향을 판정한다
    threshold: f64,
    sequence: Sequence,
    /// 임계값 이상 한 번이라도 움직였는지(= 단순 우클릭이 아님)
    moved: bool,
    /// 최대 획 수를 넘겨 실행하지 않을 제스처인지
    invalid: bool,
}

impl Gesture {
    pub fn new(x: f64, y: f64, threshold: f64) -> Self {
        Self {
            x,
            y,
            threshold,
            sequence: Sequence::default(),
            moved: false,
            invalid: false,
        }
    }

    pub fn sequence(&self) -> Sequence {
        self.sequence
    }

    pub fn moved(&self) -> bool {
        self.moved
    }

    pub fn invalid(&self) -> bool {
        self.invalid
    }

    /// 포인터 좌표 하나를 반영한다. 확장의 `Gesture.move`와 같은 순서로 판정한다.
    pub fn feed(&mut self, x: f64, y: f64) {
        let dx = x - self.x;
        let dy = y - self.y;
        let ax = dx.abs();
        let ay = dy.abs();
        if dx.hypot(dy) < self.threshold {
            return;
        }
        self.moved = true;
        let direction = if ax > ay {
            if dx < 0.0 {
                Direction::Left
            } else {
                Direction::Right
            }
        } else if dy < 0.0 {
            Direction::Up
        } else {
            Direction::Down
        };
        let last = self.sequence.last();
        // 방향이 바뀌는 순간 대각선 손떨림을 추가 획으로 오해하지 않도록 분명한 꺾임을 기다린다.
        if Some(direction) != last && ax.min(ay) > ax.max(ay) * DIAGONAL_RATIO {
            return;
        }
        self.x = x;
        self.y = y;
        if self.invalid || Some(direction) == last {
            return;
        }
        if self.sequence.len() == MAX_STROKES {
            self.invalid = true;
        } else {
            self.sequence.push(direction);
        }
    }
}

/// 96 DPI 기준 임계값을 실제 모니터 DPI의 물리 픽셀로 바꾼다.
/// 확장의 임계값은 CSS px라서, 150% 배율 모니터에서는 1.5배 거리를 움직여야 같은 체감이 된다.
pub fn scale_threshold(threshold: u32, dpi: u32) -> f64 {
    let dpi = if dpi == 0 { 96 } else { dpi };
    threshold as f64 * dpi as f64 / 96.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_formats_like_extension() {
        let mut gesture = Gesture::new(0.0, 0.0, 22.0);
        gesture.feed(0.0, 100.0);
        gesture.feed(100.0, 100.0);
        assert_eq!(gesture.sequence().as_str(), "DR");
        assert_eq!(gesture.sequence().last(), Some(Direction::Right));
    }

    #[test]
    fn threshold_scales_with_dpi() {
        assert_eq!(scale_threshold(22, 96), 22.0);
        assert_eq!(scale_threshold(22, 144), 33.0);
        assert_eq!(scale_threshold(22, 0), 22.0);
    }
}
