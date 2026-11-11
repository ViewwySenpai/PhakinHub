//! The whole macro's behaviour, expressed as plans of `Action`s.
//!
//! Nothing here touches Windows. `tick` is handed the current time and returns
//! what should happen; the platform layer just carries the actions out. That
//! split is what lets the timing rules below be tested for real.

use std::collections::BTreeSet;

use super::config::{kat_interval_ms, ClickMode, Config};
use super::timers::PTimer;

pub const UPGRADE_GAP_MS: u64 = 225;
pub const HOMURA_PRESSES: u32 = 3;
pub const KURUMI_PRESSES: u32 = 4;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Click wherever the cursor already is.
    Click,
    /// Move to (x, y) and click, leaving the cursor there.
    WarpClickStay {
        x: i32,
        y: i32,
        clicks: u32,
    },
    /// Move to (x, y), click, then put the cursor back where it was.
    WarpClickReturn {
        x: i32,
        y: i32,
        clicks: u32,
    },
    /// Return the cursor to where it was when the plan started.
    RestoreCursor,
    /// Tap a key (short hold so games register it).
    Key(String),
    Sleep(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum StartError {
    NoClickPoint,
    NoPlacePoint,
    NoTsPointForPlacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    Cac,
    Akp,
}

pub struct Engine {
    pub cfg: Config,
    pub timers: Vec<PTimer>,
    pub running: bool,
    /// True while a placement cycle is being carried out; the interval timer is
    /// held back so the next cycle cannot overlap the one in flight.
    pub placing: bool,
    pub last_tick: u64,
    /// Katakuri counts that are actually clicking right now.
    pub kat_running: BTreeSet<u32>,
    kat_next: Vec<(u32, u64)>,
    pub cac_on: bool,
    pub akp_on: bool,
    cac_next: u64,
    akp_next: u64,
}

impl Engine {
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg,
            timers: super::timers::default_timers(),
            running: false,
            placing: false,
            last_tick: 0,
            kat_running: BTreeSet::new(),
            kat_next: Vec::new(),
            cac_on: false,
            akp_on: false,
            cac_next: 0,
            akp_next: 0,
        }
    }

    // ----- Auto TS -------------------------------------------------------

    /// The plain Auto TS click, outside a placement cycle.
    pub fn ts_actions(&self) -> Vec<Action> {
        match (self.cfg.click_mode, self.cfg.point) {
            (ClickMode::Point, Some((x, y))) => vec![Action::WarpClickReturn {
                x,
                y,
                clicks: self.cfg.point_clicks.max(1),
            }],
            _ => vec![Action::Click],
        }
    }

    /// Slot key, place, place again, E x3, TS at the saved point, then X.
    ///
    /// Inside the cycle the cursor stays where it clicked; it is put back once,
    /// after the X. Every gap is at least `MIN_STEP_GAP_MS`.
    pub fn cycle_plan(&self) -> Vec<Action> {
        let gap = self.cfg.step_gap_ms();
        let mut plan = vec![
            Action::Key(self.cfg.place_slot.to_string()),
            Action::Sleep(gap),
        ];
        if let Some((x, y)) = self.cfg.place_point {
            plan.push(Action::WarpClickStay { x, y, clicks: 1 });
            plan.push(Action::Sleep(gap));
            plan.push(Action::WarpClickStay { x, y, clicks: 1 });
            plan.push(Action::Sleep(gap));
        }
        for _ in 0..HOMURA_PRESSES {
            plan.push(Action::Key("e".into()));
            plan.push(Action::Sleep(gap));
        }
        // the TS step always uses the saved point and the per-trigger count
        match self.cfg.point {
            Some((x, y)) => plan.push(Action::WarpClickStay {
                x,
                y,
                clicks: self.cfg.point_clicks.max(1),
            }),
            None => {
                for _ in 0..self.cfg.point_clicks.max(1) {
                    plan.push(Action::Click);
                }
            }
        }
        plan.push(Action::Sleep(gap));
        plan.push(Action::Key("x".into()));
        plan.push(Action::RestoreCursor);
        plan
    }

    /// Where in a cycle plan the TS click happens. The countdown to the next
    /// cycle is measured from there, not from the start of the cycle.
    pub fn ts_index_in_cycle(plan: &[Action]) -> Option<usize> {
        plan.iter()
            .rposition(|a| matches!(a, Action::WarpClickStay { .. }) || matches!(a, Action::Click))
    }

    /// Refuses to start rather than clicking somewhere that was never chosen.
    pub fn start_ts(&mut self, now: u64) -> Result<Vec<Action>, StartError> {
        if self.cfg.click_mode == ClickMode::Point && self.cfg.point.is_none() {
            return Err(StartError::NoClickPoint);
        }
        if self.cfg.is_placement() {
            if self.cfg.place_point.is_none() {
                return Err(StartError::NoPlacePoint);
            }
            if self.cfg.point.is_none() {
                return Err(StartError::NoTsPointForPlacement);
            }
        }
        self.running = true;
        self.last_tick = now;
        if self.cfg.is_placement() {
            self.placing = true;
            Ok(self.cycle_plan())
        } else {
            Ok(self.ts_actions())
        }
    }

    pub fn stop_ts(&mut self) {
        self.running = false;
        self.placing = false;
    }

    /// Call when the TS step of a cycle has just been performed.
    pub fn cycle_ts_done(&mut self, now: u64) {
        self.last_tick = now;
    }

    /// Call when a placement cycle has finished running.
    pub fn cycle_finished(&mut self) {
        self.placing = false;
    }

    pub fn upgrade_plan(&self, presses: u32) -> Vec<Action> {
        let mut p = Vec::new();
        for i in 0..presses {
            p.push(Action::Key("e".into()));
            if i + 1 < presses {
                p.push(Action::Sleep(UPGRADE_GAP_MS));
            }
        }
        p
    }

    // ----- Katakuri ------------------------------------------------------

    pub fn kat_pick(&mut self, count: u32) {
        if self.cfg.kat_picked.contains(&count) {
            self.cfg.kat_picked.remove(&count);
            self.kat_stop(count);
        } else {
            self.cfg.kat_picked.insert(count);
        }
    }

    /// Start or stop one count on its own. Used by that count's hotkey, which
    /// runs it directly rather than only picking it.
    pub fn kat_toggle_run(&mut self, count: u32, now: u64) -> Vec<Action> {
        self.cfg.kat_picked.insert(count);
        if self.kat_running.contains(&count) {
            self.kat_stop(count);
            Vec::new()
        } else {
            self.kat_start(count, now);
            vec![Action::Click]
        }
    }

    fn kat_start(&mut self, count: u32, now: u64) {
        self.kat_running.insert(count);
        self.kat_next.retain(|(c, _)| *c != count);
        self.kat_next.push((count, now + kat_interval_ms(count)));
    }

    pub fn kat_stop(&mut self, count: u32) {
        self.kat_running.remove(&count);
        self.kat_next.retain(|(c, _)| *c != count);
    }

    pub fn kat_any_running(&self) -> bool {
        !self.kat_running.is_empty()
    }

    /// The RUN / STOP button: starts every picked count, or stops everything.
    pub fn kat_run_all(&mut self, now: u64) -> Vec<Action> {
        if self.kat_any_running() {
            let running: Vec<u32> = self.kat_running.iter().copied().collect();
            for c in running {
                self.kat_stop(c);
            }
            return Vec::new();
        }
        if self.cfg.kat_picked.is_empty() {
            return Vec::new();
        }
        let picked: Vec<u32> = self.cfg.kat_picked.iter().copied().collect();
        for c in picked {
            self.kat_start(c, now);
        }
        vec![Action::Click]
    }

    // ----- other toggles -------------------------------------------------

    pub fn toggle(&mut self, what: Toggle, now: u64) {
        match what {
            Toggle::Cac => {
                self.cac_on = !self.cac_on;
                if self.cac_on {
                    self.cac_next = now;
                }
            }
            Toggle::Akp => {
                self.akp_on = !self.akp_on;
                if self.akp_on {
                    self.akp_next = now;
                }
            }
        }
    }

    // ----- the heartbeat -------------------------------------------------

    /// Everything that is due at `now`. The bool says the returned plan is a
    /// placement cycle, which the caller must follow with `cycle_ts_done` and
    /// `cycle_finished`.
    pub fn tick(&mut self, now: u64) -> (Vec<Action>, bool) {
        let mut out = Vec::new();

        if self.running && !self.placing {
            let due = self.last_tick + self.cfg.effective_interval_ms();
            if now >= due {
                self.last_tick = now;
                if self.cfg.is_placement() {
                    self.placing = true;
                    return (self.cycle_plan(), true);
                }
                out.extend(self.ts_actions());
            }
        }

        // Each Katakuri count keeps its own cadence, independent of the others.
        let mut due_counts = Vec::new();
        for (count, next) in self.kat_next.iter_mut() {
            if now >= *next {
                *next = now + kat_interval_ms(*count);
                due_counts.push(*count);
            }
        }
        for _ in due_counts {
            out.push(Action::Click);
        }

        if self.cac_on && now >= self.cac_next {
            self.cac_next = now + (self.cfg.cac_interval_s * 1000.0).round().max(10.0) as u64;
            for _ in 0..self.cfg.cac_clicks.max(1) {
                out.push(Action::Click);
            }
        }
        if self.akp_on && now >= self.akp_next {
            self.akp_next = now + (self.cfg.akp_interval_s * 1000.0).round().max(10.0) as u64;
            let gap = (self.cfg.akp_key_gap_s * 1000.0).round() as u64;
            for k in super::config::parse_key_list(&self.cfg.akp_keys) {
                out.push(Action::Key(k));
                out.push(Action::Sleep(gap));
            }
        }

        (out, false)
    }

    pub fn timer_by_hotkey(&mut self, action: &str) -> Option<&mut PTimer> {
        self.timers.iter_mut().find(|t| t.hotkey_id == action)
    }

    #[cfg(test)]
    pub fn kat_state(&self, count: u32) -> KatState {
        if self.kat_running.contains(&count) {
            KatState::Running
        } else if self.cfg.kat_picked.contains(&count) {
            KatState::Picked
        } else {
            KatState::Off
        }
    }

    pub fn kat_remaining(&self, count: u32, now: u64) -> Option<u64> {
        self.kat_next
            .iter()
            .find(|(n, _)| *n == count)
            .map(|(_, next)| next.saturating_sub(now))
    }
    pub fn cac_remaining(&self, now: u64) -> u64 {
        self.cac_next.saturating_sub(now)
    }
    pub fn akp_remaining(&self, now: u64) -> u64 {
        self.akp_next.saturating_sub(now)
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KatState {
    Off,
    Picked,
    Running,
}

/// How long a plan takes, so tests can reason about overlap.
#[cfg(test)]
pub fn plan_duration_ms(plan: &[Action]) -> u64 {
    plan.iter()
        .map(|a| match a {
            Action::Sleep(ms) => *ms,
            Action::Key(_) => 30,
            Action::Click => 15,
            Action::WarpClickStay { clicks, .. } | Action::WarpClickReturn { clicks, .. } => {
                (*clicks as u64) * 17
            }
            Action::RestoreCursor => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::MIN_STEP_GAP_MS;

    fn placement_engine() -> Engine {
        let mut cfg = Config::default();
        cfg.click_mode = ClickMode::Placement;
        cfg.point = Some((634, 585));
        cfg.point_clicks = 4;
        cfg.place_slot = 3;
        cfg.place_point = Some((849, 528));
        Engine::new(cfg)
    }

    fn shape(plan: &[Action]) -> Vec<String> {
        plan.iter()
            .filter(|a| !matches!(a, Action::Sleep(_)))
            .map(|a| match a {
                Action::Key(k) => format!("key {}", k),
                Action::Click => "click".into(),
                Action::WarpClickStay { x, y, clicks } => format!("stay {},{} x{}", x, y, clicks),
                Action::WarpClickReturn { x, y, clicks } => format!("back {},{} x{}", x, y, clicks),
                Action::RestoreCursor => "cursor home".into(),
                Action::Sleep(_) => unreachable!(),
            })
            .collect()
    }

    #[test]
    fn cursor_mode_clicks_where_the_mouse_is() {
        let e = Engine::new(Config::default());
        assert_eq!(e.ts_actions(), vec![Action::Click]);
    }

    #[test]
    fn point_mode_returns_the_cursor_after_clicking() {
        let mut cfg = Config::default();
        cfg.click_mode = ClickMode::Point;
        cfg.point = Some((666, 606));
        cfg.point_clicks = 4;
        let e = Engine::new(cfg);
        assert_eq!(
            e.ts_actions(),
            vec![Action::WarpClickReturn {
                x: 666,
                y: 606,
                clicks: 4
            }]
        );
    }

    #[test]
    fn cycle_order_matches_the_spec() {
        let e = placement_engine();
        assert_eq!(
            shape(&e.cycle_plan()),
            vec![
                "key 3",
                "stay 849,528 x1",
                "stay 849,528 x1",
                "key e",
                "key e",
                "key e",
                "stay 634,585 x4",
                "key x",
                "cursor home",
            ]
        );
    }

    #[test]
    fn the_cursor_stays_put_until_after_the_x() {
        let plan = placement_engine().cycle_plan();
        // nothing inside the cycle hands the cursor back
        assert!(!plan
            .iter()
            .any(|a| matches!(a, Action::WarpClickReturn { .. })));
        // and the very last thing it does is restore it
        assert_eq!(plan.last(), Some(&Action::RestoreCursor));
        let x_at = plan
            .iter()
            .position(|a| *a == Action::Key("x".into()))
            .unwrap();
        let home_at = plan
            .iter()
            .position(|a| *a == Action::RestoreCursor)
            .unwrap();
        assert!(
            x_at < home_at,
            "the cursor must go home after X, not before"
        );
    }

    #[test]
    fn every_gap_in_the_cycle_is_at_least_the_floor() {
        let mut e = placement_engine();
        e.cfg.place_gap_ms = 100; // below the floor on purpose
        let gaps: Vec<u64> = e
            .cycle_plan()
            .iter()
            .filter_map(|a| match a {
                Action::Sleep(ms) => Some(*ms),
                _ => None,
            })
            .collect();
        assert!(gaps.iter().all(|g| *g >= MIN_STEP_GAP_MS), "{:?}", gaps);
    }

    #[test]
    fn placement_refuses_to_start_without_both_points() {
        let mut cfg = Config::default();
        cfg.click_mode = ClickMode::Placement;
        let mut e = Engine::new(cfg);
        assert_eq!(e.start_ts(0), Err(StartError::NoPlacePoint));

        e.cfg.place_point = Some((1, 2));
        assert_eq!(e.start_ts(0), Err(StartError::NoTsPointForPlacement));
        assert!(!e.running);

        e.cfg.point = Some((3, 4));
        assert!(e.start_ts(0).is_ok());
    }

    #[test]
    fn the_countdown_starts_at_the_ts_click_not_the_cycle_start() {
        let mut e = placement_engine();
        e.cfg.interval_s = 10.0;
        e.start_ts(0).unwrap();

        let plan = e.cycle_plan();
        let ts_at = Engine::ts_index_in_cycle(&plan).unwrap();
        let ts_time = plan_duration_ms(&plan[..ts_at]);
        assert!(
            ts_time > 3000,
            "the TS click lands late in the cycle: {}",
            ts_time
        );

        e.cycle_ts_done(ts_time);
        e.cycle_finished();
        // next cycle is due one interval after the TS click
        let (_, is_cycle) = e.tick(ts_time + 9_999);
        assert!(!is_cycle);
        let (_, is_cycle) = e.tick(ts_time + 10_000);
        assert!(is_cycle);
    }

    #[test]
    fn a_cycle_never_starts_on_top_of_one_still_running() {
        let mut e = placement_engine();
        e.cfg.interval_s = 1.0; // shorter than a cycle takes
        e.start_ts(0).unwrap();
        let (plan, is_cycle) = e.tick(5_000);
        assert!(!is_cycle);
        assert!(plan.is_empty());
    }

    #[test]
    fn plain_auto_ts_fires_on_the_interval() {
        let mut cfg = Config::default();
        cfg.interval_s = 10.0;
        let mut e = Engine::new(cfg);
        e.start_ts(0).unwrap();
        let mut fired = Vec::new();
        let mut now = 0;
        while now < 35_000 {
            now += 25;
            let (plan, _) = e.tick(now);
            if plan.contains(&Action::Click) {
                fired.push(now);
            }
        }
        assert_eq!(fired, vec![10_000, 20_000, 30_000]);
    }

    #[test]
    fn picking_a_katakuri_count_does_not_start_it() {
        let mut e = Engine::new(Config::default());
        e.kat_pick(4);
        assert_eq!(e.kat_state(4), KatState::Picked);
        assert!(!e.kat_any_running());
        let mut now = 0;
        while now < 5_000 {
            now += 25;
            let (plan, _) = e.tick(now);
            assert!(plan.is_empty(), "a picked count must stay silent until RUN");
        }
    }

    #[test]
    fn a_count_hotkey_runs_that_count_directly() {
        let mut e = Engine::new(Config::default());
        e.kat_toggle_run(4, 0);
        assert_eq!(e.kat_state(4), KatState::Running);
        assert_eq!(e.kat_state(8), KatState::Off);
    }

    #[test]
    fn two_counts_run_at_their_own_rates_side_by_side() {
        let mut e = Engine::new(Config::default());
        e.kat_toggle_run(4, 0);
        e.kat_toggle_run(8, 0);
        let mut hits = Vec::new();
        let mut now = 0;
        while now < 7_000 {
            now += 25;
            let (plan, _) = e.tick(now);
            for _ in plan.iter().filter(|a| **a == Action::Click) {
                hits.push(now);
            }
        }
        // 7 s of 1.750 s plus 7 s of 0.875 s
        let expected = 7_000 / 1750 + 7_000 / 875;
        assert_eq!(hits.len() as u64, expected, "got {:?}", hits);
    }

    #[test]
    fn run_all_starts_the_picked_set_and_stops_everything() {
        let mut e = Engine::new(Config::default());
        e.kat_pick(4);
        e.kat_pick(8);
        e.kat_run_all(0);
        assert_eq!(e.kat_running, [4, 8].into_iter().collect());
        e.kat_run_all(0);
        assert!(!e.kat_any_running());
        // the picks survive a stop
        assert_eq!(e.cfg.kat_picked, [4, 8].into_iter().collect());
    }

    #[test]
    fn run_all_with_nothing_picked_does_nothing() {
        let mut e = Engine::new(Config::default());
        assert!(e.kat_run_all(0).is_empty());
        assert!(!e.kat_any_running());
    }

    #[test]
    fn unpicking_a_running_count_stops_it() {
        let mut e = Engine::new(Config::default());
        e.kat_toggle_run(5, 0);
        assert_eq!(e.kat_state(5), KatState::Running);
        e.kat_pick(5); // toggles the pick off
        assert_eq!(e.kat_state(5), KatState::Off);
        assert!(!e.kat_any_running());
    }

    #[test]
    fn the_two_upgrades_press_e_the_right_number_of_times() {
        let e = Engine::new(Config::default());
        let count = |p: Vec<Action>| p.iter().filter(|a| **a == Action::Key("e".into())).count();
        assert_eq!(count(e.upgrade_plan(HOMURA_PRESSES)), 3);
        assert_eq!(count(e.upgrade_plan(KURUMI_PRESSES)), 4);
    }

    #[test]
    fn stopping_mid_cycle_prevents_the_next_one() {
        let mut e = placement_engine();
        e.start_ts(0).unwrap();
        e.stop_ts();
        let (plan, is_cycle) = e.tick(60_000);
        assert!(!is_cycle);
        assert!(plan.is_empty());
    }
}
