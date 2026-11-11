use crate::{
    core::{
        config::Config,
        engine::{Action, Engine, Toggle},
        keys,
    },
    detection::{Reading, Tracker, Wave},
    model::{self, Preferences},
    native::{self, Held, NativeEvent},
    recording::{self, KeyCode, Op, Recorder},
};
use std::{
    collections::{BTreeSet, VecDeque},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
pub type Shared = Arc<Mutex<Runtime>>;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Full,
    Mini,
    Cockpit,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capture {
    Ts,
    Placement,
}
pub struct PlayControl {
    pub stop: AtomicBool,
    pub speed: AtomicU64,
}
impl Default for PlayControl {
    fn default() -> Self {
        Self {
            stop: AtomicBool::new(false),
            speed: AtomicU64::new(1f64.to_bits()),
        }
    }
}
pub struct Runtime {
    pub detector: Tracker,
    pub detection_message: String,
    pub detection_at: Option<u64>,
    pub detected_speed: Option<u32>,
    pub detected_wave: Wave,
    pub engine: Engine,
    pub pref: Preferences,
    pub dir: PathBuf,
    pub message: String,
    pub hooks_ready: bool,
    pub now: u64,
    pub mode: Mode,
    pub exit: bool,
    pub rebind: Option<(String, u64)>,
    pub rebind_started: u64,
    pub capture: Option<Capture>,
    pub dirty: bool,
    pub recorder: Option<Recorder>,
    pub recording_path: PathBuf,
    pub playing: bool,
    pub play_busy: bool,
    pub play_pass: u32,
    pub play_total: u32,
    pub play: Arc<PlayControl>,
    physical: BTreeSet<u16>,
    hold_start: Option<u64>,
    hold_next: u64,
    pub plan_busy: bool,
    pub generation: Arc<AtomicU64>,
    pub shutting: Arc<AtomicBool>,
    pending: VecDeque<(Vec<Action>, bool, u64)>,
}
impl Runtime {
    pub fn new(dir: PathBuf) -> Self {
        let (cfg, timers, pref) = model::load(&dir);
        let mut engine = Engine::new(cfg);
        engine.timers = timers;
        Self {
            engine,
            detector: Tracker::default(),
            detection_message: "Detection is off".into(),
            detection_at: None,
            detected_speed: None,
            detected_wave: Wave::Unknown,
            pref,
            dir,
            message: "Ready".into(),
            hooks_ready: false,
            now: 0,
            mode: Mode::Full,
            exit: false,
            rebind: None,
            rebind_started: 0,
            capture: None,
            dirty: false,
            recorder: None,
            recording_path: PathBuf::new(),
            playing: false,
            play_busy: false,
            play_pass: 0,
            play_total: 1,
            play: Arc::default(),
            physical: BTreeSet::new(),
            hold_start: None,
            hold_next: 0,
            plan_busy: false,
            pending: VecDeque::new(),
            generation: Arc::new(AtomicU64::new(0)),
            shutting: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn record_path(&self) -> PathBuf {
        if !self.engine.cfg.rec_path.is_empty() {
            return self.engine.cfg.rec_path.clone().into();
        }
        self.dir
            .join("recordings")
            .join(format!("{}.rec", safe_name(&self.engine.cfg.rec_name)))
    }
    pub fn apply_detection(&mut self, reading: Reading, now: u64) {
        let reading = Reading {
            speed: if self.pref.detection.speed_auto {
                reading.speed
            } else {
                None
            },
            wave: if self.pref.detection.tournament_auto {
                reading.wave
            } else {
                Wave::Unknown
            },
        };
        self.detection_at = Some(now);
        self.detected_speed = reading.speed;
        self.detected_wave = reading.wave;
        self.detection_message = "Reading Roblox HUD".into();
        let (speed, start) = self.detector.observe(&reading, now);
        if self.pref.detection.speed_auto {
            if let Some(speed) = speed {
                if self.engine.cfg.ts_mode != speed {
                    self.engine.cfg.ts_mode = speed;
                    self.dirty = true;
                }
            }
        }
        if start && self.pref.detection.tournament_auto {
            if let Some(t) = self
                .engine
                .timers
                .iter_mut()
                .find(|t| t.id == crate::core::timers::TOURNAMENT_ID)
            {
                t.restart(now);
                self.message = "Tournament detected: 25:00 countdown started (real time).".into();
            }
        }
    }
    pub fn add_timer(&mut self) {
        model::add_timer(&mut self.engine.timers, &mut self.engine.cfg);
        self.dirty = true;
    }
    pub fn delete_timer(&mut self, id: &str) {
        let cancel = self.engine.timers.iter().any(|t| {
            t.id == id
                && t.custom
                && self
                    .rebind
                    .as_ref()
                    .is_some_and(|(action, _)| action == &t.hotkey_id)
        });
        if model::delete_timer(&mut self.engine.timers, &mut self.engine.cfg, id) {
            if cancel {
                self.cancel_capture();
            }
            self.dirty = true;
            self.message = "Timer deleted. Other timers are unchanged.".into();
        }
    }
    pub fn stop_automation(&mut self) {
        self.engine.stop_ts();
        let counts: Vec<_> = self.engine.kat_running.iter().copied().collect();
        for n in counts {
            self.engine.kat_stop(n);
        }
        self.engine.cac_on = false;
        self.engine.akp_on = false;
        self.hold_start = None;
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
    pub fn bind(&mut self, action: String) {
        if self.recorder.is_some() || self.play_busy {
            self.message = "Stop Record / Playback before changing a keybind.".into();
            return;
        }
        self.cancel_capture();
        self.rebind_started = native::now();
        self.rebind = Some((action, self.rebind_started + 30_000));
        self.message = "Press a key or mouse side button, or choose a key from the list.".into();
        native::REBIND.store(true, Ordering::Relaxed);
    }
    pub fn set_binding(&mut self, action: &str, vk: u16) -> bool {
        if self.recorder.is_some()
            || self.play_busy
            || !self.engine.cfg.hotkeys.contains_key(action)
        {
            return false;
        }
        let Some(name) = keys::name_from_vk(vk) else {
            return false;
        };
        self.cancel_capture();
        self.engine.cfg.set_hotkey(action, &name);
        self.message = format!("Keybind set to {}", keys::key_display(&name));
        self.dirty = true;
        true
    }
    pub fn capture_binding(&mut self, vk: u16) -> bool {
        let Some((action, _)) = self.rebind.clone() else {
            return false;
        };
        if vk == 0x1b {
            self.cancel_capture();
            self.message = "Keybind capture cancelled.".into();
            return true;
        }
        if vk == 1 || vk == 2 {
            return false;
        }
        if self.set_binding(&action, vk) {
            // A held binding key must not immediately run the newly bound action.
            self.physical.insert(vk);
            return true;
        }
        false
    }
    pub fn cancel_capture(&mut self) {
        self.rebind = None;
        self.capture = None;
        native::REBIND.store(false, Ordering::Relaxed);
        native::CAPTURE.store(false, Ordering::Relaxed);
    }
}
fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| !c.is_control() && !"<>:\"/\\|?*".contains(*c))
        .take(80)
        .collect();
    let s = s.trim().trim_end_matches('.');
    if s.is_empty() {
        "macro1".into()
    } else {
        s.into()
    }
}
pub struct Controller {
    shared: Shared,
    join: Option<std::thread::JoinHandle<()>>,
    detection_join: Option<std::thread::JoinHandle<()>>,
    _hooks: Option<native::Hooks>,
}
impl Controller {
    pub fn start(shared: Shared, ctx: eframe::egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let hooks = native::Hooks::start(tx);
        {
            let mut s = shared.lock().unwrap();
            s.hooks_ready = hooks.is_ok();
            if let Err(e) = &hooks {
                s.message = e.clone();
            }
        }
        let state = shared.clone();
        let detection_state = shared.clone();
        let detection_join = std::thread::spawn(move || {
            let mut previous = None;
            loop {
                let settings = {
                    let s = detection_state.lock().unwrap();
                    if s.shutting.load(Ordering::Relaxed) {
                        break;
                    }
                    s.pref.detection.clone()
                };
                if previous.as_ref() != Some(&settings) {
                    let mut s = detection_state.lock().unwrap();
                    s.detector.reset_candidates();
                    s.detected_speed = None;
                    s.detected_wave = Wave::Unknown;
                    s.detection_at = None;
                    previous = Some(settings.clone());
                }
                if settings.speed_auto || settings.tournament_auto {
                    // Screen reads and recognition stay outside the input/UI mutex.
                    let sample = crate::screen::read(&settings);
                    let mut s = detection_state.lock().unwrap();
                    if settings == s.pref.detection {
                        match sample {
                            Ok(reading) => s.apply_detection(reading, native::now()),
                            Err(message) => {
                                s.detection_message = message;
                                s.detected_speed = None;
                                s.detected_wave = Wave::Unknown;
                                s.detection_at = None;
                                s.detector.reset_candidates();
                            }
                        }
                    }
                } else {
                    detection_state.lock().unwrap().detection_message = "Detection is off".into();
                }
                for _ in 0..8 {
                    if detection_state
                        .lock()
                        .unwrap()
                        .shutting
                        .load(Ordering::Relaxed)
                    {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        });
        let join = std::thread::spawn(move || {
            let mut last_save = Instant::now();
            let mut last_repaint = Instant::now();
            loop {
                if state.lock().unwrap().shutting.load(Ordering::Relaxed) {
                    break;
                }
                let mut actions = Vec::new();
                for ev in rx.try_iter().take(2000) {
                    if let Some(action) = process_event(&mut state.lock().unwrap(), ev) {
                        actions.push(action);
                    }
                }
                for action in actions {
                    dispatch(&state, &action);
                }
                let work = {
                    let mut s = state.lock().unwrap();
                    s.now = native::now();
                    let now = s.now;
                    for t in &mut s.engine.timers {
                        t.poll(now);
                    }
                    if s.rebind.as_ref().is_some_and(|(_, end)| now >= *end) {
                        s.cancel_capture();
                        s.message = "Keybind capture timed out.".into();
                    }
                    s.play
                        .speed
                        .store(s.engine.cfg.rec_speed.to_bits(), Ordering::Relaxed);
                    if !s.plan_busy && !s.play_busy {
                        let generation = s.generation.load(Ordering::SeqCst);
                        s.pending.retain(|(_, _, g)| *g == generation);
                        let (mut plan, cycle) = if let Some((p, c, _)) = s.pending.pop_front() {
                            (p, c)
                        } else {
                            s.engine.tick(now)
                        };
                        if s.pref.hold_on
                            && s.hold_start
                                .is_some_and(|start| now >= start + s.pref.hold_delay)
                            && now >= s.hold_next
                        {
                            plan.push(Action::Key("e".into()));
                            s.hold_next = now + s.pref.hold_rate;
                        }
                        Some((plan, cycle))
                    } else {
                        None
                    }
                };
                if let Some((plan, cycle)) = work {
                    spawn_plan(&state, plan, cycle);
                }
                if last_save.elapsed() > Duration::from_millis(700) {
                    save(&state);
                    last_save = Instant::now();
                }
                if last_repaint.elapsed() > Duration::from_millis(50) {
                    ctx.request_repaint();
                    last_repaint = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(4));
            }
        });
        Self {
            shared,
            join: Some(join),
            detection_join: Some(detection_join),
            _hooks: hooks.ok(),
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        {
            let mut s = self.shared.lock().unwrap();
            s.shutting.store(true, Ordering::SeqCst);
            s.play.stop.store(true, Ordering::SeqCst);
            s.stop_automation();
            if let Some(rec) = s.recorder.take() {
                let _ = rec.finish(native::now());
            }
            s.cancel_capture();
            s.dirty = true;
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        if let Some(j) = self.detection_join.take() {
            let _ = j.join();
        }
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) {
            let busy = {
                let s = self.shared.lock().unwrap();
                s.plan_busy || s.play_busy
            };
            if !busy {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        save(&self.shared);
    }
}
pub fn save(shared: &Shared) {
    let snapshot = {
        let mut s = shared.lock().unwrap();
        if !s.dirty {
            return;
        }
        s.dirty = false;
        (
            s.dir.clone(),
            s.engine.cfg.clone(),
            s.engine.timers.clone(),
            s.pref.clone(),
        )
    };
    if let Err(e) = model::save(&snapshot.0, &snapshot.1, &snapshot.2, &snapshot.3) {
        let mut s = shared.lock().unwrap();
        s.message = format!("Settings could not be saved: {e}");
        s.dirty = true;
    }
}
fn is_control(cfg: &Config, vk: u16) -> bool {
    ["rec_toggle", "play_toggle"]
        .iter()
        .any(|act| keys::vk_from_name(cfg.hotkey(act)) == Some(vk))
}
pub fn process_event(s: &mut Runtime, event: NativeEvent) -> Option<String> {
    let down = matches!(event.op, Op::Key(_, true) | Op::Button(_, true));
    let up = matches!(event.op, Op::Key(_, false) | Op::Button(_, false));
    let repeated = if let Some(vk) = event.vk {
        if down {
            !s.physical.insert(vk)
        } else {
            if up {
                s.physical.remove(&vk);
            }
            false
        }
    } else {
        false
    };
    if s.rebind.is_some() {
        if down && !repeated && event.at >= s.rebind_started {
            if let Some(vk) = event.vk {
                s.capture_binding(vk);
            }
        }
        return None;
    }
    if let Some(target) = s.capture {
        if down && event.vk == Some(0x1b) {
            s.cancel_capture();
            s.message = "Point capture cancelled.".into();
            return None;
        }
        if event.op == Op::Button(1, true) && !event.own {
            let point = event.position.unwrap_or_else(native::cursor);
            match target {
                Capture::Ts => s.engine.cfg.point = Some(point),
                Capture::Placement => s.engine.cfg.place_point = Some(point),
            }
            s.cancel_capture();
            s.message = format!("Point saved at {}, {}", point.0, point.1);
            s.dirty = true;
            return None;
        }
    }
    let action = if down && !repeated && (!event.own || !native::EDITING.load(Ordering::Relaxed)) {
        event.vk.and_then(|vk| {
            s.engine
                .cfg
                .hotkeys
                .iter()
                .find(|(act, key)| {
                    (s.engine.cfg.hotkeys_enabled || act.as_str() == "hk_toggle")
                        && keys::vk_from_name(key) == Some(vk)
                })
                .map(|(act, _)| act.clone())
        })
    } else {
        None
    };
    if event.vk == Some(0x45) {
        if down && !event.own && !repeated {
            s.hold_start = Some(event.at);
            s.hold_next = event.at + s.pref.hold_delay;
        } else if up {
            s.hold_start = None;
        }
    }
    let control =
        s.engine.cfg.hotkeys_enabled && event.vk.is_some_and(|vk| is_control(&s.engine.cfg, vk));
    if let Some(rec) = s.recorder.as_mut() {
        let balanced_release = rec.has_release(&event.op);
        if !event.injected && ((!event.own && !control) || balanced_release) {
            let result = (|| {
                if !event.own
                    && s.engine.cfg.rec_moves
                    && matches!(event.op, Op::Button(..) | Op::Wheel(..))
                {
                    if let Some((x, y)) = event.position {
                        rec.capture(event.at, &Op::Move(x, y), true, 0)?;
                    }
                }
                rec.capture(
                    event.at,
                    &event.op,
                    s.engine.cfg.rec_moves,
                    s.engine.cfg.rec_sample_ms,
                )
            })();
            if let Err(e) = result {
                s.recorder = None;
                s.message = format!("Recording stopped: {e}");
            }
        }
    }
    action
}
pub fn dispatch(shared: &Shared, id: &str) {
    if id == "rec_toggle" {
        toggle_record(shared);
        return;
    }
    if id == "play_toggle" {
        toggle_play(shared);
        return;
    }
    if id == "rec_open" || id == "rec_save_as" {
        record_dialog(shared, id == "rec_save_as");
        return;
    }
    if id == "fit_game" {
        let result = native::fit_game();
        shared.lock().unwrap().message = result
            .map(|_| "Game fitted between cockpit panels.".into())
            .unwrap_or_else(|e| e);
        return;
    }
    let mut plan = Vec::new();
    let mut cycle = false;
    {
        let mut s = shared.lock().unwrap();
        let now = native::now();
        if s.play_busy
            && [
                "auto_ts",
                "kat_run",
                "cac",
                "akp",
                "homura_upgrade",
                "kurumi_upgrade",
            ]
            .contains(&id)
        {
            s.message = "Stop Playback before starting an automatic action.".into();
            return;
        }
        match id {
            "tournament_rearm" => {
                s.detector.rearm();
                if let Some(t) = s
                    .engine
                    .timers
                    .iter_mut()
                    .find(|t| t.id == crate::core::timers::TOURNAMENT_ID)
                {
                    t.reset();
                }
                s.message = "Tournament reset. Waiting for a stable Tournament Wave header.".into();
            }
            "stop_all" => {
                s.stop_automation();
                s.play.stop.store(true, Ordering::SeqCst);
                s.playing = false;
                s.pref.hold_on = false;
                if let Some(r) = s.recorder.take() {
                    let _ = r.finish(now);
                }
                s.message = "All actions stopped.".into();
            }
            "auto_ts" => {
                if s.engine.running {
                    s.engine.stop_ts();
                    s.generation.fetch_add(1, Ordering::SeqCst);
                } else {
                    match s.engine.start_ts(now) {
                        Ok(p) => {
                            plan = p;
                            cycle = s.engine.cfg.is_placement();
                        }
                        Err(e) => {
                            s.message = match e {
                                crate::core::engine::StartError::NoPlacePoint => {
                                    "Set the placement point first."
                                }
                                _ => "Set the Auto TS point first.",
                            }
                            .into()
                        }
                    }
                }
            }
            "kat_run" => {
                let stopping = !s.engine.kat_running.is_empty();
                plan = s.engine.kat_run_all(now);
                if stopping {
                    s.generation.fetch_add(1, Ordering::SeqCst);
                } else if s.engine.cfg.kat_picked.is_empty() {
                    s.message = "Select at least one Katakuri count.".into();
                }
            }
            "kat3" | "kat4" | "kat5" | "kat6" | "kat7" | "kat8" => {
                if !s.play_busy {
                    plan = s.engine.kat_toggle_run(id[3..].parse().unwrap(), now);
                }
            }
            "cac" | "akp" => {
                s.engine.toggle(
                    if id == "cac" {
                        Toggle::Cac
                    } else {
                        Toggle::Akp
                    },
                    now,
                );
                if (id == "cac" && !s.engine.cac_on) || (id == "akp" && !s.engine.akp_on) {
                    s.generation.fetch_add(1, Ordering::SeqCst);
                }
            }
            "homura_upgrade" | "kurumi_upgrade" => {
                plan = s.engine.upgrade_plan(if id == "homura_upgrade" {
                    crate::core::engine::HOMURA_PRESSES
                } else {
                    crate::core::engine::KURUMI_PRESSES
                });
            }
            "inc_delay" => s.engine.cfg.interval_s += 0.05,
            "dec_delay" => s.engine.cfg.interval_s = (s.engine.cfg.interval_s - 0.05).max(0.05),
            "hold_repeat" => {
                s.pref.hold_on = !s.pref.hold_on;
                s.hold_start = None;
            }
            "hk_toggle" => s.engine.cfg.hotkeys_enabled = !s.engine.cfg.hotkeys_enabled,
            "toggle_ui" => {
                s.mode = if s.mode == Mode::Mini {
                    Mode::Full
                } else {
                    Mode::Mini
                }
            }
            "cockpit" => {
                s.mode = if s.mode == Mode::Cockpit {
                    Mode::Full
                } else {
                    Mode::Cockpit
                }
            }
            "exit_app" => s.exit = true,
            "capture_ts" | "capture_place" => {
                s.capture = Some(if id == "capture_ts" {
                    Capture::Ts
                } else {
                    Capture::Placement
                });
                native::CAPTURE.store(true, Ordering::Relaxed);
            }
            "cancel_capture" => s.cancel_capture(),
            _ => {
                if id == "tournament_tmr" {
                    s.detector.suppress_current();
                }
                if let Some(t) = s.engine.timer_by_hotkey(id) {
                    t.cycle(now);
                }
            }
        }
        s.dirty = true;
    }
    spawn_plan(shared, plan, cycle);
}
fn wait_while(ms: u64, keep: &impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed().as_millis() < (ms as u128) {
        if !keep() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    keep()
}
fn spawn_plan(shared: &Shared, plan: Vec<Action>, cycle: bool) {
    if plan.is_empty() {
        return;
    }
    let (generation, expected, shutting) = {
        let mut s = shared.lock().unwrap();
        let expected = s.generation.load(Ordering::SeqCst);
        if s.plan_busy {
            s.pending.push_back((plan, cycle, expected));
            return;
        }
        s.plan_busy = true;
        (s.generation.clone(), expected, s.shutting.clone())
    };
    let state = shared.clone();
    std::thread::spawn(move || {
        let keep =
            || generation.load(Ordering::SeqCst) == expected && !shutting.load(Ordering::Relaxed);
        let cursor = native::cursor();
        let mut held = Held::default();
        let ts_at = if cycle {
            Engine::ts_index_in_cycle(&plan)
        } else {
            None
        };
        let result = (|| -> Result<(), String> {
            for (index, action) in plan.iter().enumerate() {
                if !keep() {
                    break;
                }
                match action {
                    Action::Sleep(ms) => {
                        if !wait_while(*ms, &keep) {
                            break;
                        }
                    }
                    Action::Key(name) => {
                        while keep() && native::foreground_is_app() {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        if !keep() {
                            break;
                        }
                        let vk = keys::vk_from_name(name)
                            .ok_or_else(|| format!("Unknown key: {name}"))?;
                        let k = KeyCode::vk(vk);
                        held.apply(&Op::Key(k, true))?;
                        wait_while(15, &keep);
                        held.apply(&Op::Key(k, false))?;
                    }
                    Action::Click => click(&mut held, &keep)?,
                    Action::WarpClickStay { x, y, clicks }
                    | Action::WarpClickReturn { x, y, clicks } => {
                        let back = native::cursor();
                        native::set_cursor(*x, *y)?;
                        for _ in 0..*clicks {
                            if !keep() {
                                break;
                            }
                            click(&mut held, &keep)?;
                        }
                        if matches!(action, Action::WarpClickReturn { .. }) {
                            native::set_cursor(back.0, back.1)?;
                        }
                    }
                    Action::RestoreCursor => native::set_cursor(cursor.0, cursor.1)?,
                }
                if Some(index) == ts_at {
                    state.lock().unwrap().engine.cycle_ts_done(native::now());
                }
            }
            Ok(())
        })();
        drop(held);
        if cycle {
            let _ = native::set_cursor(cursor.0, cursor.1);
        }
        let mut s = state.lock().unwrap();
        s.plan_busy = false;
        if cycle {
            s.engine.cycle_finished();
        }
        if let Err(e) = result {
            s.stop_automation();
            s.message = e;
        }
    });
}
fn click(held: &mut Held, keep: &impl Fn() -> bool) -> Result<(), String> {
    if native::cursor_is_app() {
        return Ok(());
    }
    held.apply(&Op::Button(1, true))?;
    wait_while(15, keep);
    held.apply(&Op::Button(1, false))
}
fn toggle_record(shared: &Shared) {
    let mut s = shared.lock().unwrap();
    let now = native::now();
    if let Some(rec) = s.recorder.take() {
        s.message = match rec.finish(now) {
            Ok(n) => format!("Saved {n} events."),
            Err(e) => format!("Could not finish recording: {e}"),
        };
        return;
    }
    if s.play_busy {
        s.message = "Stop Playback before recording.".into();
        return;
    }
    s.engine.cfg.rec_path.clear();
    let path = s.record_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            s.message = format!("Cannot create recordings folder: {e}");
            return;
        }
    }
    match Recorder::new(&path, now) {
        Ok(rec) => {
            s.recorder = Some(rec);
            s.recording_path = path;
            s.message = "Recording keyboard and mouse input. Use Record again to stop.".into();
        }
        Err(e) => s.message = format!("Cannot start recording: {e}"),
    };
    s.dirty = true;
}
fn toggle_play(shared: &Shared) {
    let (path, loops, control, shutting, controls) = {
        let mut s = shared.lock().unwrap();
        if s.playing {
            s.play.stop.store(true, Ordering::SeqCst);
            s.playing = false;
            return;
        }
        if s.play_busy || s.recorder.is_some() {
            s.message = "Stop recording before Playback.".into();
            return;
        }
        let controls = ["rec_toggle", "play_toggle"]
            .iter()
            .filter_map(|a| {
                if s.engine.cfg.hotkeys_enabled {
                    keys::vk_from_name(s.engine.cfg.hotkey(a))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        (
            s.record_path(),
            s.engine.cfg.rec_loops,
            s.play.clone(),
            s.shutting.clone(),
            controls,
        )
    };
    let events = match std::fs::read(&path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| recording::parse(&model::decode(&bytes)))
    {
        Ok(v) => v,
        Err(e) => {
            shared.lock().unwrap().message = format!("Cannot play recording: {e}");
            return;
        }
    };
    if events.is_empty() {
        shared.lock().unwrap().message = "The recording has no events.".into();
        return;
    }
    {
        let mut s = shared.lock().unwrap();
        s.stop_automation();
        s.playing = true;
        s.play_busy = true;
        s.play_pass = 1;
        s.play_total = loops;
        s.message = "Playback ready. Switch to the target window to begin.".into();
        control.stop.store(false, Ordering::SeqCst);
        control
            .speed
            .store(s.engine.cfg.rec_speed.to_bits(), Ordering::Relaxed);
    }
    let state = shared.clone();
    std::thread::spawn(move || {
        let keep = || !control.stop.load(Ordering::SeqCst) && !shutting.load(Ordering::Relaxed);
        let mut pass = 0;
        let result = (|| -> Result<(), String> {
            while keep() && (state.lock().unwrap().plan_busy || native::foreground_is_app()) {
                std::thread::sleep(Duration::from_millis(2));
            }
            while keep() {
                let mut held = Held::default();
                let mut clock = Instant::now();
                let mut virtual_ms = 0.;
                for event in &events {
                    while keep() {
                        let now = Instant::now();
                        let speed = f64::from_bits(control.speed.load(Ordering::Relaxed));
                        virtual_ms += now.duration_since(clock).as_secs_f64() * 1000. * speed;
                        clock = now;
                        if virtual_ms >= event.at as f64 {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    if !keep() {
                        break;
                    }
                    let is_control = match event.op {
                        Op::Key(code, _) => controls.contains(&native::vk_of(code)),
                        Op::Button(b, _) => controls.contains(&match b {
                            1 => 1,
                            2 => 2,
                            3 => 4,
                            4 => 5,
                            _ => 6,
                        }),
                        _ => false,
                    };
                    if !is_control {
                        held.apply(&event.op)?;
                    }
                }
                drop(held);
                pass += 1;
                if loops > 0 && pass >= loops {
                    break;
                }
                state.lock().unwrap().play_pass = pass + 1;
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(())
        })();
        let mut s = state.lock().unwrap();
        s.play_busy = false;
        s.playing = false;
        s.message = match result {
            Ok(()) => "Playback stopped. Held inputs released.".into(),
            Err(e) => format!("Playback error: {e}"),
        };
    });
}
fn record_dialog(shared: &Shared, save_as: bool) {
    let (dir, current) = {
        let s = shared.lock().unwrap();
        if s.recorder.is_some() || s.play_busy {
            return;
        }
        (s.dir.join("recordings"), s.record_path())
    };
    if save_as && !current.exists() {
        shared.lock().unwrap().message = "Record something before using Save as.".into();
        return;
    }
    if let Some(path) = native::file_dialog(save_as, &dir) {
        if save_as && path != current {
            if let Err(e) = std::fs::copy(current, &path) {
                shared.lock().unwrap().message = format!("Save failed: {e}");
                return;
            }
        }
        let mut s = shared.lock().unwrap();
        s.engine.cfg.rec_path = path.to_string_lossy().into();
        if let Some(stem) = path.file_stem() {
            s.engine.cfg.rec_name = stem.to_string_lossy().into();
        }
        s.dirty = true;
        s.message = "Recording file selected.".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_speed_changes_only_after_stable_reading_and_manual_stays_manual() {
        let mut s = Runtime::new("no-test-config".into());
        let reading = Reading {
            speed: Some(3),
            wave: Wave::Unknown,
        };
        for now in [0, 400, 800] {
            s.apply_detection(reading.clone(), now);
        }
        assert_eq!(s.engine.cfg.ts_mode, 1);
        s.pref.detection.speed_auto = true;
        for now in [1000, 1400] {
            s.apply_detection(reading.clone(), now);
            assert_eq!(s.engine.cfg.ts_mode, 1);
        }
        s.apply_detection(reading, 1800);
        assert_eq!(s.engine.cfg.ts_mode, 3);
        assert_eq!(s.engine.cfg.effective_interval_ms(), 3330);
        for now in [2200, 2600, 3000] {
            s.apply_detection(Reading::default(), now);
        }
        assert_eq!(s.engine.cfg.ts_mode, 3); // occlusion does not guess x1
        s.pref.detection.speed_auto = false;
        s.engine.cfg.ts_mode = 2;
        for now in [4000, 4400, 4800] {
            s.apply_detection(
                Reading {
                    speed: Some(1),
                    wave: Wave::Unknown,
                },
                now,
            );
        }
        assert_eq!(s.engine.cfg.ts_mode, 2);
    }
    #[test]
    fn tournament_is_real_time_and_does_not_restart_on_repeated_or_missing_headers() {
        let mut s = Runtime::new("no-test-config".into());
        s.pref.detection.tournament_auto = true;
        s.pref.detection.speed_auto = true;
        let reading = Reading {
            speed: Some(3),
            wave: Wave::Tournament,
        };
        for now in [0, 400, 800] {
            s.apply_detection(reading.clone(), now);
        }
        let get = |s: &Runtime| {
            s.engine
                .timers
                .iter()
                .find(|t| t.id == "tournament")
                .unwrap()
                .seconds(10800)
        };
        assert_eq!(get(&s), 1490);
        s.engine.cfg.ts_mode = 1;
        s.apply_detection(Reading::default(), 1200);
        for now in [1600, 2000, 2400] {
            s.apply_detection(reading.clone(), now);
        }
        assert_eq!(get(&s), 1490);
        let t = s
            .engine
            .timers
            .iter_mut()
            .find(|t| t.id == "tournament")
            .unwrap();
        t.poll(1_500_800);
        assert!(t.done);
        s.apply_detection(reading.clone(), 1_501_000);
        assert!(
            s.engine
                .timers
                .iter()
                .find(|t| t.id == "tournament")
                .unwrap()
                .done
        );
        s.detector.rearm();
        for now in [1_502_000, 1_502_400, 1_502_800] {
            s.apply_detection(reading.clone(), now);
        }
        let t = s
            .engine
            .timers
            .iter()
            .find(|t| t.id == "tournament")
            .unwrap();
        assert!(!t.done);
        assert_eq!(t.seconds(1_502_800), 1500);
    }
    fn input(at: u64, vk: u16, down: bool) -> NativeEvent {
        NativeEvent {
            at,
            op: match vk {
                4 => Op::Button(3, down),
                5 => Op::Button(4, down),
                6 => Op::Button(5, down),
                _ => Op::Key(KeyCode::vk(vk), down),
            },
            vk: Some(vk),
            own: true,
            injected: true,
            position: None,
        }
    }
    #[test]
    fn remapped_key_can_bind_and_only_runs_after_releasing_it() {
        let mut s = Runtime::new("no-test-config".into());
        s.bind("inc_delay".into());
        let at = s.rebind_started + 1;
        assert!(process_event(&mut s, input(at, 0x77, true)).is_none());
        assert_eq!(s.engine.cfg.hotkey("inc_delay"), "f8");
        assert!(s.rebind.is_none() && s.dirty);
        assert!(process_event(&mut s, input(at + 1, 0x77, true)).is_none());
        process_event(&mut s, input(at + 2, 0x77, false));
        let mut pressed = input(at + 3, 0x77, true);
        pressed.own = false;
        assert_eq!(process_event(&mut s, pressed).as_deref(), Some("inc_delay"));
    }
    #[test]
    fn both_side_buttons_can_be_captured() {
        for (vk, name) in [(5, "XButton1"), (6, "XButton2")] {
            let mut s = Runtime::new("no-test-config".into());
            s.bind("gyro_tmr".into());
            let at = s.rebind_started + 1;
            assert!(process_event(&mut s, input(at, vk, true)).is_none());
            assert_eq!(s.engine.cfg.hotkey("gyro_tmr"), name);
        }
    }
    #[test]
    fn escape_cancels_without_erasing_the_old_key() {
        let mut s = Runtime::new("no-test-config".into());
        s.engine.cfg.set_hotkey("inc_delay", "f7");
        s.bind("inc_delay".into());
        assert!(s.capture_binding(0x1b));
        assert!(s.rebind.is_none());
        assert_eq!(s.engine.cfg.hotkey("inc_delay"), "f7");
    }
    #[test]
    fn queued_input_before_set_does_not_choose_a_key() {
        let mut s = Runtime::new("no-test-config".into());
        s.bind("inc_delay".into());
        s.rebind_started = 100;
        process_event(&mut s, input(99, 0x77, true));
        assert!(s.rebind.is_some());
        assert!(s.engine.cfg.hotkey("inc_delay").is_empty());
        process_event(&mut s, input(100, 0x77, false));
        process_event(&mut s, input(101, 0x77, true));
        assert_eq!(s.engine.cfg.hotkey("inc_delay"), "f8");
    }
    #[test]
    fn choosing_a_key_without_hooks_clears_alias_conflicts_and_persists() {
        let mut s = Runtime::new("no-test-config".into());
        assert!(!s.hooks_ready);
        s.engine.cfg.set_hotkey("gyro_tmr", "vk77");
        assert!(s.set_binding("inc_delay", 0x77));
        let (cfg, _, _) = model::parse(&model::serialize(&s.engine.cfg, &s.engine.timers, &s.pref));
        assert!(cfg.hotkey("gyro_tmr").is_empty());
        assert_eq!(cfg.hotkey("inc_delay"), "f8");
    }
    #[test]
    fn hotkey_hold_does_not_repeat_or_capture_control() {
        let mut s = Runtime::new(PathBuf::from("nonexistent-test-directory"));
        s.engine.cfg.set_hotkey("rec_toggle", "F9");
        let ev = NativeEvent {
            at: 1,
            op: Op::Key(KeyCode::vk(0x78), true),
            vk: Some(0x78),
            own: false,
            injected: false,
            position: None,
        };
        assert_eq!(
            process_event(&mut s, ev.clone()).as_deref(),
            Some("rec_toggle")
        );
        assert!(process_event(&mut s, ev).is_none());
    }
    #[test]
    fn side_button_can_trigger_a_timer() {
        let mut s = Runtime::new("no-test-config".into());
        s.engine.cfg.set_hotkey("gyro_tmr", "XButton2");
        assert_eq!(
            process_event(
                &mut s,
                NativeEvent {
                    at: 2,
                    op: Op::Button(5, true),
                    vk: Some(6),
                    own: false,
                    injected: false,
                    position: Some((10, 20))
                }
            )
            .as_deref(),
            Some("gyro_tmr")
        );
    }
    #[test]
    fn names_cannot_escape_recordings_folder() {
        assert_eq!(safe_name("../foo\\bar:*?"), "..foobar");
        assert_eq!(safe_name("  "), "macro1");
    }
    #[test]
    fn recording_filters_controls_and_keeps_exact_clicks_and_releases() {
        let path =
            std::env::temp_dir().join(format!("phakinhub-input-test-{}.rec", std::process::id()));
        let mut s = Runtime::new("no-test-config".into());
        s.engine.cfg.rec_moves = true;
        s.engine.cfg.rec_sample_ms = 100;
        s.engine.cfg.set_hotkey("rec_toggle", "F9");
        s.recorder = Some(Recorder::new(&path, 0).unwrap());
        let events = [
            (1, Op::Key(KeyCode::vk(0x78), true), Some(0x78), false, None),
            (2, Op::Key(KeyCode::vk(65), true), Some(65), true, None),
            (3, Op::Key(KeyCode::vk(66), true), Some(66), false, None),
            (4, Op::Move(90, 90), None, false, Some((90, 90))),
            (5, Op::Button(4, true), Some(5), false, Some((101, 202))),
            (6, Op::Button(4, false), Some(5), true, Some((500, 500))),
        ];
        for (at, op, vk, own, position) in events {
            process_event(
                &mut s,
                NativeEvent {
                    at,
                    op,
                    vk,
                    own,
                    injected: false,
                    position,
                },
            );
        }
        let mut simulated = input(8, 66, false);
        simulated.own = false;
        process_event(&mut s, simulated);
        let mut simulated = input(9, 67, true);
        simulated.own = false;
        process_event(&mut s, simulated);
        s.recorder.take().unwrap().finish(10).unwrap();
        let recording = recording::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(!recording
            .iter()
            .any(|e| matches!(e.op,Op::Key(k,_) if k.value==0x78||k.value==65)));
        assert_eq!(recording[1].op, Op::Move(101, 202));
        assert_eq!(recording[2].op, Op::Button(4, true));
        assert_eq!(recording[3].op, Op::Button(4, false));
        assert_eq!(recording[4].op, Op::Key(KeyCode::vk(66), false));
        assert_eq!(recording[4].at, 10);
        assert!(!recording
            .iter()
            .any(|e| matches!(e.op,Op::Key(k,_) if k.value==67)));
        assert_eq!(recording.last().unwrap().at, 10);
    }
    #[test]
    fn deleting_a_timer_cancels_its_pending_keybind() {
        let mut s = Runtime::new("no-test-config".into());
        s.add_timer();
        s.bind("timer_3_tmr".into());
        s.delete_timer("custom3");
        assert!(s.rebind.is_none());
        assert!(!s.engine.cfg.hotkeys.contains_key("timer_3_tmr"));
    }
    #[test]
    fn busy_automation_preserves_a_requested_plan_and_stop_invalidates_it() {
        let mut s = Runtime::new("no-test-config".into());
        s.plan_busy = true;
        let shared = Arc::new(Mutex::new(s));
        spawn_plan(&shared, vec![Action::Sleep(50)], true);
        let mut s = shared.lock().unwrap();
        assert_eq!(s.pending.len(), 1);
        let queued = s.pending[0].2;
        s.stop_automation();
        assert_ne!(queued, s.generation.load(Ordering::SeqCst));
    }
}
