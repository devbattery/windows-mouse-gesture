//! 우클릭 상태 머신.
//!
//! 오른쪽 버튼을 누른 순간에는 제스처인지 단순 클릭인지 알 수 없다. 그래서 누름을
//! 일단 삼키고(Swallow), 판정이 나면 필요한 입력만 다시 보낸다(Inject 명령).
//! 훅 콜백이 즉시 결정을 내려야 하므로 입출력·시간·할당이 없는 순수 로직으로 두고,
//! 타이머 설치·입력 주입·로그는 호출 측(Win32 연결부)이 [`Command`]를 보고 처리한다.

use crate::gesture::{Gesture, Sequence};

/// 훅이 관찰한 사건.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    /// 사용자가 오른쪽 버튼을 누름. `allowed`가 false면(보조 키 등) 제스처 없이 통과시킨다.
    RightDown {
        x: f64,
        y: f64,
        allowed: bool,
        threshold: f64,
    },
    RightUp {
        x: f64,
        y: f64,
    },
    Move {
        x: f64,
        y: f64,
    },
    /// 왼쪽·가운데·추가 버튼 누름
    OtherDown,
    /// 판정 대기 중 일정 시간 움직이지 않음(길게 누르기 → 원래 우클릭 드래그로 통과)
    HoldTimeout,
    /// 판정 대기 중에 대상 앱이 예외 앱으로 밝혀짐(낙관적 시작 후 비동기 판정 결과)
    Excluded,
    /// 제스처 중 Esc
    Cancel,
    /// 우리가 다시 보낸 오른쪽 누름이 훅을 통과함
    OwnRightDownSeen,
    /// 화면 잠금·보안 데스크톱·절전·디스플레이 변경 등으로 뗌을 받지 못할 수 있음.
    /// 진행 중인 판정·제스처는 실행 없이 취소하고 입력은 다시 보내지 않는다.
    /// - 판정 대기·제스처 중: 앱은 누름을 보지 못했으므로, 뒤늦게 뗌이 오면 삼킨다(취소 대기).
    ///   뗌이 아예 사라졌으면 다음 오른쪽 누름이 이 상태를 정리한다.
    /// - 그 밖의 상태: 앱이 이미 누름을 봤거나 추적할 것이 없으므로 대기 상태로 돌아간다.
    Reset,
}

/// 우클릭 하나가 제스처 실행 말고 다른 방식으로 끝난 이유. 최근 기록과 통계에 쓴다.
/// 제스처 실행 결과(실행·일치 없음)는 동작을 찾는 워커가 기록한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// 움직이지 않고 뗌 → 단순 우클릭으로 다시 보냄
    Click,
    /// 움직이지 않고 오래 누름 → 원래 우클릭으로 통과
    Hold,
    /// 판정 대기 중 예외 앱으로 밝혀짐 → 원래 우클릭으로 통과
    Excluded,
    /// 오른쪽을 누른 채 다른 버튼을 누름 → 원래 우클릭으로 통과
    OtherButton,
    /// 제스처를 취소함(Esc·다른 버튼·초기화·뗌 유실)
    Cancelled,
}

/// 이 입력을 다음 훅·앱에 넘길지 여부.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Swallow,
}

/// 상태 전이에 따라 호출 측이 실행할 일.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    StartHoldTimer,
    StopHoldTimer,
    StartEscPoll,
    StopEscPoll,
    /// 오른쪽 누름+뗌을 다시 보낸다(단순 우클릭)
    InjectRightClick,
    /// 오른쪽 누름만 다시 보낸다(길게 누르기 통과)
    InjectRightDown,
    /// 오른쪽 뗌만 다시 보낸다(다시 보낸 누름보다 실제 뗌이 먼저 온 경우)
    InjectRightUp,
    GestureStarted,
    /// 방향 시퀀스가 늘었거나 무효(최대 획 수 초과)가 됨
    GestureChanged {
        sequence: Sequence,
        invalid: bool,
    },
    GestureFinished {
        sequence: Sequence,
        invalid: bool,
    },
    GestureCancelled,
}

/// 한 사건의 처리 결과. 명령은 최대 4개라 고정 배열에 담는다(훅 스레드 무할당).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Output {
    pub verdict: Verdict,
    /// 이 사건으로 우클릭 하나가 끝났으면 그 이유
    pub ending: Option<Ending>,
    commands: [Option<Command>; 4],
    len: usize,
}

impl Output {
    fn new(verdict: Verdict) -> Self {
        Self {
            verdict,
            ending: None,
            commands: [None; 4],
            len: 0,
        }
    }

    fn ended(mut self, ending: Ending) -> Self {
        self.ending = Some(ending);
        self
    }

    fn with(mut self, command: Command) -> Self {
        debug_assert!(self.len < self.commands.len(), "명령 슬롯이 부족합니다");
        if self.len < self.commands.len() {
            self.commands[self.len] = Some(command);
            self.len += 1;
        }
        self
    }

    pub fn commands(&self) -> impl Iterator<Item = Command> + '_ {
        self.commands[..self.len].iter().flatten().copied()
    }
}

#[derive(Clone, Copy, Debug)]
enum State {
    /// 아무 버튼도 추적하지 않음
    Idle,
    /// 오른쪽 누름을 삼키고 제스처인지 판정 대기
    Pending(Gesture),
    /// 임계값 이상 움직여 제스처로 확정
    Gesturing(Gesture),
    /// 제스처를 취소함. 누름을 앱이 보지 못했으므로 뗌도 삼킨다
    CancelledWaitUp,
    /// 길게 누르기로 누름을 다시 보냄. `down_seen`은 그 누름이 훅을 통과했는지
    Passthrough { down_seen: bool },
    /// 처음부터 통과시킨 우클릭(보조 키 등). 뗌까지 그대로 통과
    Bypass,
}

/// 우클릭 하나의 생애를 추적한다. 훅 스레드 하나에서만 사용한다.
#[derive(Debug)]
pub struct Machine {
    state: State,
}

impl Default for Machine {
    fn default() -> Self {
        Self { state: State::Idle }
    }
}

impl Machine {
    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    pub fn is_gesturing(&self) -> bool {
        matches!(self.state, State::Gesturing(_))
    }

    pub fn handle(&mut self, input: Input) -> Output {
        use Verdict::{Pass, Swallow};
        match input {
            Input::RightDown {
                x,
                y,
                allowed,
                threshold,
            } => {
                // 뗌을 놓친 채(훅 재설치·보안 데스크톱 등) 다시 눌린 경우 이전 추적을 정리하고 새로 시작한다.
                let mut output = Output::new(if allowed { Swallow } else { Pass });
                match self.state {
                    State::Pending(_) => output = output.with(Command::StopHoldTimer),
                    State::Gesturing(_) => {
                        output = output
                            .with(Command::StopEscPoll)
                            .with(Command::GestureCancelled)
                            .ended(Ending::Cancelled)
                    }
                    _ => {}
                }
                if allowed {
                    self.state = State::Pending(Gesture::new(x, y, threshold));
                    output.with(Command::StartHoldTimer)
                } else {
                    self.state = State::Bypass;
                    output
                }
            }
            Input::RightUp { x, y } => match self.state {
                State::Idle => Output::new(Pass),
                State::Pending(_) => {
                    self.state = State::Idle;
                    Output::new(Swallow)
                        .with(Command::StopHoldTimer)
                        .with(Command::InjectRightClick)
                        .ended(Ending::Click)
                }
                State::Gesturing(mut gesture) => {
                    // 마지막 이동이 합쳐져 오지 않았어도 뗀 지점까지 반영한다(확장과 같음).
                    gesture.feed(x, y);
                    self.state = State::Idle;
                    Output::new(Swallow)
                        .with(Command::StopEscPoll)
                        .with(Command::GestureFinished {
                            sequence: gesture.sequence(),
                            invalid: gesture.invalid(),
                        })
                }
                State::CancelledWaitUp => {
                    self.state = State::Idle;
                    Output::new(Swallow)
                }
                State::Passthrough { down_seen } => {
                    self.state = State::Idle;
                    if down_seen {
                        Output::new(Pass)
                    } else {
                        // 다시 보낸 누름보다 실제 뗌이 먼저 왔다. 그대로 통과시키면 앱이 뗌→누름 순서로 받아
                        // 버튼이 눌린 채 남으므로, 뗌을 삼키고 누름 뒤에 다시 보낸다.
                        Output::new(Swallow).with(Command::InjectRightUp)
                    }
                }
                State::Bypass => {
                    self.state = State::Idle;
                    Output::new(Pass)
                }
            },
            // 이동은 커서가 움직여야 하므로 항상 통과시킨다.
            Input::Move { x, y } => match self.state {
                State::Pending(mut gesture) => {
                    gesture.feed(x, y);
                    if gesture.moved() {
                        self.state = State::Gesturing(gesture);
                        let output = Output::new(Pass)
                            .with(Command::StopHoldTimer)
                            .with(Command::StartEscPoll)
                            .with(Command::GestureStarted);
                        if gesture.sequence().is_empty() {
                            output
                        } else {
                            output.with(Command::GestureChanged {
                                sequence: gesture.sequence(),
                                invalid: gesture.invalid(),
                            })
                        }
                    } else {
                        self.state = State::Pending(gesture);
                        Output::new(Pass)
                    }
                }
                State::Gesturing(mut gesture) => {
                    let before = (gesture.sequence(), gesture.invalid());
                    gesture.feed(x, y);
                    self.state = State::Gesturing(gesture);
                    // 방향이 늘었거나 최대 획 수를 넘어 무효가 됐을 때만 알린다.
                    if (gesture.sequence(), gesture.invalid()) == before {
                        Output::new(Pass)
                    } else {
                        Output::new(Pass).with(Command::GestureChanged {
                            sequence: gesture.sequence(),
                            invalid: gesture.invalid(),
                        })
                    }
                }
                _ => Output::new(Pass),
            },
            Input::OtherDown => match self.state {
                State::Pending(_) => {
                    // 오른쪽을 누른 채 다른 버튼을 누르면 제스처가 아니다. 삼켰던 누름을 돌려준다.
                    self.state = State::Passthrough { down_seen: false };
                    Output::new(Pass)
                        .with(Command::StopHoldTimer)
                        .with(Command::InjectRightDown)
                        .ended(Ending::OtherButton)
                }
                State::Gesturing(_) => {
                    self.state = State::CancelledWaitUp;
                    Output::new(Pass)
                        .with(Command::StopEscPoll)
                        .with(Command::GestureCancelled)
                        .ended(Ending::Cancelled)
                }
                _ => Output::new(Pass),
            },
            Input::HoldTimeout => match self.state {
                State::Pending(_) => {
                    self.state = State::Passthrough { down_seen: false };
                    Output::new(Pass)
                        .with(Command::InjectRightDown)
                        .ended(Ending::Hold)
                }
                // 이미 지난 타이머
                _ => Output::new(Pass),
            },
            Input::Excluded => match self.state {
                // 아직 제스처로 확정되지 않았으면 삼켰던 누름을 돌려주고 원래 우클릭으로 넘긴다.
                State::Pending(_) => {
                    self.state = State::Passthrough { down_seen: false };
                    Output::new(Pass)
                        .with(Command::StopHoldTimer)
                        .with(Command::InjectRightDown)
                        .ended(Ending::Excluded)
                }
                // 이미 제스처로 확정됐으면 그대로 둔다(실행 단계에서 예외 앱이면 실행하지 않음).
                _ => Output::new(Pass),
            },
            Input::Cancel => match self.state {
                State::Gesturing(_) => {
                    self.state = State::CancelledWaitUp;
                    Output::new(Pass)
                        .with(Command::StopEscPoll)
                        .with(Command::GestureCancelled)
                        .ended(Ending::Cancelled)
                }
                _ => Output::new(Pass),
            },
            Input::Reset => match self.state {
                // 삼킨 누름은 앱이 보지 못했으므로 이 우클릭은 사라진다(잠금 직전의 누름).
                State::Pending(_) => {
                    self.state = State::CancelledWaitUp;
                    Output::new(Pass)
                        .with(Command::StopHoldTimer)
                        .ended(Ending::Cancelled)
                }
                State::Gesturing(_) => {
                    self.state = State::CancelledWaitUp;
                    Output::new(Pass)
                        .with(Command::StopEscPoll)
                        .with(Command::GestureCancelled)
                        .ended(Ending::Cancelled)
                }
                // 다시 보낸 누름이 아직 훅을 지나지 않았다. 대기 상태로 가면 그 사이 온 실제 뗌이 먼저 통과해
                // 앱이 뗌→누름 순서로 받으므로, 순서 보정을 위해 이 상태를 유지한다(다음 누름이 정리).
                State::Passthrough { down_seen: false } => Output::new(Pass),
                _ => {
                    self.state = State::Idle;
                    Output::new(Pass)
                }
            },
            Input::OwnRightDownSeen => {
                if let State::Passthrough { .. } = self.state {
                    self.state = State::Passthrough { down_seen: true };
                }
                Output::new(Pass)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Command::*;
    use Verdict::*;

    fn down(machine: &mut Machine, allowed: bool) -> Output {
        machine.handle(Input::RightDown {
            x: 100.0,
            y: 100.0,
            allowed,
            threshold: 22.0,
        })
    }

    fn commands(output: &Output) -> Vec<Command> {
        output.commands().collect()
    }

    #[test]
    fn plain_right_click_is_replayed() {
        let mut machine = Machine::default();
        let pressed = down(&mut machine, true);
        assert_eq!(pressed.verdict, Swallow);
        assert_eq!(commands(&pressed), [StartHoldTimer]);
        // 임계값보다 짧은 이동은 클릭으로 본다
        assert_eq!(
            machine.handle(Input::Move { x: 105.0, y: 103.0 }).verdict,
            Pass
        );
        let released = machine.handle(Input::RightUp { x: 105.0, y: 103.0 });
        assert_eq!(released.verdict, Swallow);
        assert_eq!(commands(&released), [StopHoldTimer, InjectRightClick]);
        assert!(machine.is_idle());
    }

    #[test]
    fn gesture_runs_and_swallows_release() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        let started = machine.handle(Input::Move { x: 40.0, y: 100.0 });
        assert_eq!(started.verdict, Pass);
        let mut probe = Gesture::new(100.0, 100.0, 22.0);
        probe.feed(40.0, 100.0);
        let left = probe.sequence();
        assert_eq!(
            commands(&started),
            [
                StopHoldTimer,
                StartEscPoll,
                GestureStarted,
                GestureChanged {
                    sequence: left,
                    invalid: false
                }
            ]
        );
        assert!(machine.is_gesturing());
        let released = machine.handle(Input::RightUp { x: 30.0, y: 100.0 });
        assert_eq!(released.verdict, Swallow);
        assert_eq!(
            commands(&released),
            [
                StopEscPoll,
                GestureFinished {
                    sequence: left,
                    invalid: false
                }
            ]
        );
        assert_eq!(left.as_str(), "L");
    }

    #[test]
    fn modifier_click_bypasses_until_release() {
        let mut machine = Machine::default();
        let pressed = down(&mut machine, false);
        assert_eq!(pressed.verdict, Pass);
        assert!(commands(&pressed).is_empty());
        assert_eq!(
            machine.handle(Input::Move { x: 300.0, y: 100.0 }).verdict,
            Pass
        );
        assert_eq!(
            machine
                .handle(Input::RightUp { x: 300.0, y: 100.0 })
                .verdict,
            Pass
        );
        assert!(machine.is_idle());
    }

    #[test]
    fn long_press_hands_the_button_back() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        let timeout = machine.handle(Input::HoldTimeout);
        assert_eq!(commands(&timeout), [InjectRightDown]);
        // 다시 보낸 누름이 훅을 통과한 뒤에는 실제 드래그·뗌을 모두 통과시킨다
        machine.handle(Input::OwnRightDownSeen);
        assert_eq!(
            machine.handle(Input::Move { x: 400.0, y: 100.0 }).verdict,
            Pass
        );
        let released = machine.handle(Input::RightUp { x: 400.0, y: 100.0 });
        assert_eq!(released.verdict, Pass);
        assert!(commands(&released).is_empty());
    }

    #[test]
    fn early_release_after_long_press_keeps_order() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        machine.handle(Input::HoldTimeout);
        // 다시 보낸 누름이 아직 훅을 지나지 않았는데 실제 뗌이 먼저 옴
        let released = machine.handle(Input::RightUp { x: 100.0, y: 100.0 });
        assert_eq!(released.verdict, Swallow);
        assert_eq!(commands(&released), [InjectRightUp]);
    }

    #[test]
    fn escape_cancels_and_swallows_release() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        machine.handle(Input::Move { x: 100.0, y: 200.0 });
        let cancelled = machine.handle(Input::Cancel);
        assert_eq!(commands(&cancelled), [StopEscPoll, GestureCancelled]);
        let released = machine.handle(Input::RightUp { x: 100.0, y: 220.0 });
        assert_eq!(released.verdict, Swallow);
        assert!(commands(&released).is_empty());
    }

    #[test]
    fn other_button_while_pending_returns_right_press() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        let other = machine.handle(Input::OtherDown);
        assert_eq!(other.verdict, Pass);
        assert_eq!(commands(&other), [StopHoldTimer, InjectRightDown]);
    }

    #[test]
    fn other_button_cancels_gesture() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        machine.handle(Input::Move { x: 200.0, y: 100.0 });
        let other = machine.handle(Input::OtherDown);
        assert_eq!(commands(&other), [StopEscPoll, GestureCancelled]);
        assert_eq!(
            machine
                .handle(Input::RightUp { x: 200.0, y: 100.0 })
                .verdict,
            Swallow
        );
    }

    #[test]
    fn missed_release_restarts_cleanly() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        machine.handle(Input::Move { x: 200.0, y: 100.0 });
        // 뗌을 놓치고 다시 누름
        let again = down(&mut machine, true);
        assert_eq!(again.verdict, Swallow);
        assert_eq!(
            commands(&again),
            [StopEscPoll, GestureCancelled, StartHoldTimer]
        );
    }

    #[test]
    fn excluded_app_gets_its_press_back() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        let excluded = machine.handle(Input::Excluded);
        assert_eq!(commands(&excluded), [StopHoldTimer, InjectRightDown]);
        machine.handle(Input::OwnRightDownSeen);
        assert_eq!(
            machine
                .handle(Input::RightUp { x: 100.0, y: 100.0 })
                .verdict,
            Pass
        );
        // 대기 상태나 제스처 중에 늦게 온 판정은 무시한다
        assert!(commands(&machine.handle(Input::Excluded)).is_empty());
        down(&mut machine, true);
        machine.handle(Input::Move { x: 200.0, y: 100.0 });
        assert!(commands(&machine.handle(Input::Excluded)).is_empty());
        assert!(machine.is_gesturing());
    }

    #[test]
    fn stale_timers_are_ignored() {
        let mut machine = Machine::default();
        assert_eq!(machine.handle(Input::HoldTimeout).verdict, Pass);
        assert_eq!(machine.handle(Input::Cancel).verdict, Pass);
        assert!(machine.is_idle());
        assert_eq!(
            machine.handle(Input::RightUp { x: 0.0, y: 0.0 }).verdict,
            Pass
        );
    }

    #[test]
    fn diagonal_move_starts_gesture_without_direction() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        let started = machine.handle(Input::Move { x: 130.0, y: 130.0 });
        assert_eq!(
            commands(&started),
            [StopHoldTimer, StartEscPoll, GestureStarted]
        );
        let released = machine.handle(Input::RightUp { x: 131.0, y: 131.0 });
        assert_eq!(
            commands(&released),
            [
                StopEscPoll,
                GestureFinished {
                    sequence: Sequence::default(),
                    invalid: false
                }
            ]
        );
    }

    #[test]
    fn endings_explain_how_each_press_ended() {
        let mut machine = Machine::default();
        down(&mut machine, true);
        let click = machine.handle(Input::RightUp { x: 100.0, y: 100.0 });
        assert_eq!(click.ending, Some(Ending::Click));

        down(&mut machine, true);
        assert_eq!(
            machine.handle(Input::HoldTimeout).ending,
            Some(Ending::Hold)
        );
        // 통과 중 뗌은 이미 끝난 우클릭이라 다시 기록하지 않는다
        assert_eq!(
            machine.handle(Input::RightUp { x: 0.0, y: 0.0 }).ending,
            None
        );

        down(&mut machine, true);
        assert_eq!(
            machine.handle(Input::Excluded).ending,
            Some(Ending::Excluded)
        );
        machine.handle(Input::RightUp { x: 0.0, y: 0.0 });

        down(&mut machine, true);
        assert_eq!(
            machine.handle(Input::OtherDown).ending,
            Some(Ending::OtherButton)
        );
        machine.handle(Input::RightUp { x: 0.0, y: 0.0 });

        down(&mut machine, true);
        machine.handle(Input::Move { x: 200.0, y: 100.0 });
        assert_eq!(
            machine.handle(Input::Cancel).ending,
            Some(Ending::Cancelled)
        );
        // 취소 뒤 뗌과 이동은 끝난 우클릭의 일부다
        assert_eq!(
            machine.handle(Input::RightUp { x: 0.0, y: 0.0 }).ending,
            None
        );

        // 제스처 실행은 워커가 기록하므로 상태 머신은 이유를 남기지 않는다
        down(&mut machine, true);
        machine.handle(Input::Move { x: 200.0, y: 100.0 });
        assert_eq!(
            machine.handle(Input::RightUp { x: 200.0, y: 100.0 }).ending,
            None
        );
    }

    #[test]
    fn reset_cancels_tracking_from_every_state() {
        // 판정 대기: 타이머만 멈추고 아무것도 다시 보내지 않는다
        let mut machine = Machine::default();
        down(&mut machine, true);
        let reset = machine.handle(Input::Reset);
        assert_eq!(commands(&reset), [StopHoldTimer]);
        assert_eq!(reset.ending, Some(Ending::Cancelled));
        // 앱은 누름을 보지 못했으므로 뒤늦게 온 뗌을 삼킨다(짝 없는 뗌이 메뉴를 띄우지 않게)
        let late = machine.handle(Input::RightUp { x: 0.0, y: 0.0 });
        assert_eq!((late.verdict, late.ending), (Swallow, None));
        assert!(machine.is_idle());

        // 제스처 중: 표시를 지우고 동작은 실행하지 않는다
        down(&mut machine, true);
        machine.handle(Input::Move { x: 200.0, y: 100.0 });
        let reset = machine.handle(Input::Reset);
        assert_eq!(commands(&reset), [StopEscPoll, GestureCancelled]);
        assert!(!machine.is_gesturing());
        // 초기화 뒤 이동은 제스처로 그리지 않는다
        assert!(commands(&machine.handle(Input::Move { x: 300.0, y: 100.0 })).is_empty());
        // 뗌이 사라진 채 다시 누르면 새 우클릭으로 시작한다
        let again = down(&mut machine, true);
        assert_eq!(again.verdict, Swallow);
        assert_eq!(commands(&again), [StartHoldTimer]);
        machine.handle(Input::Reset);
        assert_eq!(
            machine.handle(Input::RightUp { x: 0.0, y: 0.0 }).verdict,
            Swallow
        );

        // 다시 보낸 누름이 훅을 지난 통과 상태: 명령 없이 대기로
        for setup in [Input::HoldTimeout, Input::OtherDown] {
            down(&mut machine, true);
            machine.handle(setup);
            machine.handle(Input::OwnRightDownSeen);
            let reset = machine.handle(Input::Reset);
            assert!(commands(&reset).is_empty());
            assert_eq!(reset.ending, None);
            assert!(machine.is_idle());
        }

        // 다시 보낸 누름이 아직 도착하지 않음: 순서 보정을 위해 통과 상태를 유지한다
        down(&mut machine, true);
        machine.handle(Input::HoldTimeout);
        assert!(commands(&machine.handle(Input::Reset)).is_empty());
        assert!(!machine.is_idle());
        let early_up = machine.handle(Input::RightUp { x: 0.0, y: 0.0 });
        assert_eq!(early_up.verdict, Swallow);
        assert_eq!(commands(&early_up), [InjectRightUp]);
        assert!(machine.is_idle());
        down(&mut machine, false);
        assert!(commands(&machine.handle(Input::Reset)).is_empty());
        assert!(machine.is_idle());
        assert!(commands(&machine.handle(Input::Reset)).is_empty());
    }
}
