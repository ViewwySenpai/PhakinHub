//! Timer state machine. No OS calls: the caller supplies `now_ms`, which is what
//! makes every timing rule here testable.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerState {
    Idle,
    Running,
    Paused,
}

pub const TOURNAMENT_ID: &str = "tournament";
pub const TOURNAMENT_SECONDS: u64 = 1500;

#[derive(Debug, Clone)]
pub struct PTimer {
    pub id: String,
    pub name: String,
    /// 0 = stopwatch (counts up), otherwise a countdown of this many seconds.
    pub duration_s: u64,
    pub hotkey_id: String,
    pub state: TimerState,
    acc_ms: u64,
    run_start: u64,
    pub done: bool,
    pub custom: bool,
    pub detached: bool,
    pub position: Option<[f32; 2]>,
}

impl PTimer {
    pub fn new(id: &str, name: &str, duration_s: u64, hotkey_id: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            duration_s,
            hotkey_id: hotkey_id.into(),
            state: TimerState::Idle,
            acc_ms: 0,
            run_start: 0,
            done: false,
            custom: false,
            detached: false,
            position: None,
        }
    }

    pub fn is_stopwatch(&self) -> bool {
        self.duration_s == 0
    }

    fn elapsed_ms(&self, now: u64) -> u64 {
        match self.state {
            TimerState::Running => self.acc_ms + now.saturating_sub(self.run_start),
            _ => self.acc_ms,
        }
    }

    /// Seconds to show: counted up for a stopwatch, remaining for a countdown.
    pub fn seconds(&self, now: u64) -> u64 {
        let ms = self.elapsed_ms(now);
        if self.is_stopwatch() {
            ms / 1000
        } else {
            let total = self.duration_s * 1000;
            if ms >= total {
                0
            } else {
                // round up so a fresh countdown shows its full duration
                (total - ms + 999) / 1000
            }
        }
    }

    /// One key does everything: start, then stop, then reset.
    pub fn cycle(&mut self, now: u64) {
        match self.state {
            TimerState::Idle => {
                self.acc_ms = 0;
                self.run_start = now;
                self.done = false;
                self.state = TimerState::Running;
            }
            TimerState::Running => {
                self.acc_ms += now.saturating_sub(self.run_start);
                self.state = TimerState::Paused;
            }
            TimerState::Paused => self.reset(),
        }
    }

    pub fn reset(&mut self) {
        self.state = TimerState::Idle;
        self.acc_ms = 0;
        self.done = false;
    }

    pub fn restart(&mut self, now: u64) {
        self.reset();
        self.cycle(now);
    }

    /// Call every UI tick; finishes a countdown that has run out.
    pub fn poll(&mut self, now: u64) {
        if !self.is_stopwatch() && self.state == TimerState::Running && self.seconds(now) == 0 {
            self.acc_ms = self.duration_s * 1000;
            self.state = TimerState::Idle;
            self.done = true;
        }
    }

    pub fn time_text(&self, now: u64) -> String {
        if self.done {
            if self.id == TOURNAMENT_ID {
                return "FINISHED".into();
            }
            return "READY!".into();
        }
        if self.state == TimerState::Idle {
            if self.id == TOURNAMENT_ID {
                return fmt_mmss(self.duration_s);
            }
            return "00:00".into();
        }
        fmt_mmss(self.seconds(now))
    }

    pub fn status_text(&self) -> String {
        if self.done {
            if self.id == TOURNAMENT_ID {
                return "Tournament finished".into();
            }
            return format!("{} COMEBACK!", self.name);
        }
        match self.state {
            TimerState::Running => {
                if self.is_stopwatch() {
                    "Running".into()
                } else {
                    "Counting down".into()
                }
            }
            TimerState::Paused => "Paused".into(),
            TimerState::Idle => "Ready".into(),
        }
    }
}

pub fn fmt_mmss(total_s: u64) -> String {
    format!("{:02}:{:02}", total_s / 60, total_s % 60)
}

pub fn default_timers() -> Vec<PTimer> {
    vec![
        PTimer::new("t1", "TSUKU", 400, "tsuku_tmr"),
        PTimer::new("t2", "GOJO/VAMPIRE", 480, "gojo_tmr"),
        PTimer::new("t3", "TIMER 1", 0, "gyro_tmr"),
        PTimer::new("t4", "LUFFY", 1080, "luffy_tmr"),
        PTimer::new("t5", "TIMER 2", 0, "timer5_tmr"),
        PTimer::new(
            TOURNAMENT_ID,
            "Tournament Countdown",
            TOURNAMENT_SECONDS,
            "tournament_tmr",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdown_start_stop_reset() {
        let mut t = PTimer::new("t", "GOJO", 480, "hk");
        t.cycle(0);
        assert_eq!(t.state, TimerState::Running);
        assert_eq!(t.time_text(7_000), "07:53");

        t.cycle(7_000); // stop
        assert_eq!(t.state, TimerState::Paused);
        // frozen: time does not move while paused
        assert_eq!(t.time_text(99_000), "07:53");

        t.cycle(99_000); // reset
        assert_eq!(t.state, TimerState::Idle);
        // this is the bug the AHK version had: reset must clear the value every
        // view reads, not just the label on one screen
        assert_eq!(t.time_text(99_000), "00:00");
        assert_eq!(t.seconds(99_000), 480);
    }

    #[test]
    fn countdown_finishes_then_restarts_clean() {
        let mut k = PTimer::new("t5", "KIZUKI", 80, "hk");
        k.cycle(0);
        k.poll(80_000);
        assert!(k.done);
        assert_eq!(k.state, TimerState::Idle);
        assert_eq!(k.time_text(80_000), "READY!");

        k.cycle(90_000);
        assert_eq!(k.state, TimerState::Running);
        assert!(!k.done);
        assert_eq!(k.seconds(90_000), 80);
    }

    #[test]
    fn stopwatch_counts_up() {
        let mut s = PTimer::new("t3", "TIMER 1", 0, "hk");
        s.cycle(0);
        assert_eq!(s.time_text(65_000), "01:05");
        s.cycle(65_000);
        assert_eq!(s.time_text(200_000), "01:05");
        s.cycle(200_000);
        assert_eq!(s.time_text(200_000), "00:00");
    }

    #[test]
    fn timing_is_wall_clock_so_a_late_tick_cannot_drift() {
        let mut t = PTimer::new("t", "TSUKU", 400, "hk");
        t.cycle(0);
        assert_eq!(t.seconds(123_456), 400 - 124 + 1);
        assert_eq!(t.time_text(123_456), fmt_mmss(277));
    }
}
