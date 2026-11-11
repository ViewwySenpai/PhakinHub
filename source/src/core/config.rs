//! Settings and their config.ini round-trip. Pure string handling so the parser
//! can be tested without touching the disk.

use std::collections::{BTreeMap, BTreeSet};

pub const ACTIONS: &[(&str, &str)] = &[
    ("auto_ts", "Auto TS"),
    ("kat3", "Run 3 Katakuris"),
    ("kat4", "Run 4 Katakuris"),
    ("kat5", "Run 5 Katakuris"),
    ("kat6", "Run 6 Katakuris"),
    ("kat7", "Run 7 Katakuris"),
    ("kat8", "Run 8 Katakuris"),
    ("kat_run", "Run / Stop All Picked"),
    ("homura_upgrade", "Upgrade Homura (E x3)"),
    ("kurumi_upgrade", "Upgrade Kurumi (E x4)"),
    ("tsuku_tmr", "Tsuku Timer"),
    ("gojo_tmr", "Gojo/Vampire Timer"),
    ("gyro_tmr", "Timer"),
    ("luffy_tmr", "Luffy Timer"),
    ("timer5_tmr", "Kizuki Timer"),
    ("tournament_tmr", "Tournament Countdown"),
    ("cac", "Custom Auto Click"),
    ("akp", "Auto Key Press"),
    ("inc_delay", "Interval +"),
    ("dec_delay", "Interval -"),
    ("toggle_ui", "Show / Hide Panel"),
    ("cockpit", "Cockpit Mode"),
    ("fit_game", "Fit Game Window"),
    ("exit_app", "Exit"),
    ("rec_toggle", "Record Input Start / Stop"),
    ("play_toggle", "Playback Start / Stop"),
    ("hold_repeat", "Hold E to Repeat"),
    ("hk_toggle", "Keybinds On / Off"),
    ("stop_all", "Stop All Actions"),
];

/// Lowest gap allowed anywhere in the placement cycle.
pub const MIN_STEP_GAP_MS: u64 = 500;

/// Katakuri counts that can be picked, and the rule for their click rate.
pub const KAT_COUNTS: [u32; 6] = [3, 4, 5, 6, 7, 8];
pub const KAT_BUDGET_MS: u64 = 7000;

pub fn kat_interval_ms(count: u32) -> u64 {
    (KAT_BUDGET_MS as f64 / count as f64).round() as u64
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMode {
    /// Click wherever the mouse already is.
    Cursor,
    /// Warp to the saved TS point, click, put the cursor back.
    Point,
    /// Run the full placement cycle each interval.
    Placement,
}

impl ClickMode {
    pub fn label(self) -> &'static str {
        match self {
            ClickMode::Cursor => "Auto At Cursor",
            ClickMode::Point => "Auto At Point",
            ClickMode::Placement => "Auto On Placement",
        }
    }
    pub fn all() -> [ClickMode; 3] {
        [ClickMode::Cursor, ClickMode::Point, ClickMode::Placement]
    }
    fn as_str(self) -> &'static str {
        match self {
            ClickMode::Cursor => "cursor",
            ClickMode::Point => "point",
            ClickMode::Placement => "placement",
        }
    }
    fn from_str(s: &str) -> ClickMode {
        match s.to_ascii_lowercase().as_str() {
            "point" => ClickMode::Point,
            "placement" => ClickMode::Placement,
            _ => ClickMode::Cursor,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub hotkeys: BTreeMap<String, String>,
    pub hotkeys_enabled: bool,

    pub interval_s: f64,
    pub ts_mode: u32,
    pub click_mode: ClickMode,
    pub point: Option<(i32, i32)>,
    pub point_clicks: u32,

    /// Counts the user has picked. Running state is deliberately not saved, so
    /// launching the app never starts clicking on its own.
    pub kat_picked: BTreeSet<u32>,

    pub place_slot: u32,
    pub place_point: Option<(i32, i32)>,
    pub place_gap_ms: u64,

    pub rec_name: String,
    pub rec_path: String,
    pub rec_moves: bool,
    pub rec_sample_ms: u64,
    pub rec_loops: u32,
    pub rec_speed: f64,

    pub cac_interval_s: f64,
    pub cac_clicks: u32,
    pub akp_keys: String,
    pub akp_interval_s: f64,
    pub akp_key_gap_s: f64,
}

impl Default for Config {
    fn default() -> Self {
        let mut hotkeys = BTreeMap::new();
        for (id, _) in ACTIONS {
            // No defaults on purpose: the user assigns every key themselves.
            hotkeys.insert((*id).to_string(), String::new());
        }
        Self {
            hotkeys,
            hotkeys_enabled: true,
            interval_s: 10.0,
            ts_mode: 1,
            click_mode: ClickMode::Cursor,
            point: None,
            point_clicks: 4,
            kat_picked: BTreeSet::new(),
            place_slot: 1,
            place_point: None,
            place_gap_ms: MIN_STEP_GAP_MS,
            rec_name: "macro1".into(),
            rec_path: String::new(),
            rec_moves: true,
            rec_sample_ms: 33,
            rec_loops: 1,
            rec_speed: 1.0,
            cac_interval_s: 1.0,
            cac_clicks: 1,
            akp_keys: String::new(),
            akp_interval_s: 1.0,
            akp_key_gap_s: 0.05,
        }
    }
}

impl Config {
    pub fn effective_interval_s(&self) -> f64 {
        let m = self.ts_mode.max(1) as f64;
        (self.interval_s / m * 100.0).round() / 100.0
    }

    pub fn effective_interval_ms(&self) -> u64 {
        (self.effective_interval_s() * 1000.0).round() as u64
    }

    /// The gap actually used between steps, never below the floor.
    pub fn step_gap_ms(&self) -> u64 {
        self.place_gap_ms.max(MIN_STEP_GAP_MS)
    }

    pub fn is_placement(&self) -> bool {
        self.click_mode == ClickMode::Placement
    }

    pub fn hotkey(&self, action: &str) -> &str {
        self.hotkeys.get(action).map(|s| s.as_str()).unwrap_or("")
    }

    pub fn set_hotkey(&mut self, action: &str, key: &str) {
        // One key drives one action: release whoever held it before.
        if !key.is_empty() {
            let clashes: Vec<String> = self
                .hotkeys
                .iter()
                .filter(|(a, k)| {
                    a.as_str() != action
                        && (k.eq_ignore_ascii_case(key)
                            || (super::keys::vk_from_name(key).is_some()
                                && super::keys::vk_from_name(k) == super::keys::vk_from_name(key)))
                })
                .map(|(a, _)| a.clone())
                .collect();
            for a in clashes {
                self.hotkeys.insert(a, String::new());
            }
        }
        self.hotkeys.insert(action.to_string(), key.to_string());
    }

    pub fn to_ini(&self) -> String {
        let mut out = String::from("[Hotkeys]\n");
        for (id, key) in &self.hotkeys {
            out.push_str(&format!("{}={}\n", id, key));
        }
        out.push_str("\n[Settings]\n");
        let pt = |p: Option<(i32, i32)>| match p {
            Some((x, y)) => (x.to_string(), y.to_string()),
            None => (String::new(), String::new()),
        };
        let (px, py) = pt(self.point);
        let (lx, ly) = pt(self.place_point);
        let picked: Vec<String> = self.kat_picked.iter().map(|n| n.to_string()).collect();
        out.push_str(&format!(
            "hk_enabled={}\n",
            if self.hotkeys_enabled { 1 } else { 0 }
        ));
        out.push_str(&format!("interval={}\n", self.interval_s));
        out.push_str(&format!("ts_mode={}\n", self.ts_mode));
        out.push_str(&format!("click_mode={}\n", self.click_mode.as_str()));
        out.push_str(&format!("point_x={}\npoint_y={}\n", px, py));
        out.push_str(&format!("point_clicks={}\n", self.point_clicks));
        out.push_str(&format!("kat_picked={}\n", picked.join(",")));
        out.push_str(&format!("place_slot={}\n", self.place_slot));
        out.push_str(&format!("place_x={}\nplace_y={}\n", lx, ly));
        out.push_str(&format!("place_delay={}\n", self.place_gap_ms));
        out.push_str(&format!("rec_name={}\n", self.rec_name));
        out.push_str(&format!("rec_path={}\n", self.rec_path));
        out.push_str(&format!(
            "rec_moves={}\n",
            if self.rec_moves { 1 } else { 0 }
        ));
        out.push_str(&format!("rec_sample={}\n", self.rec_sample_ms));
        out.push_str(&format!("rec_loops={}\n", self.rec_loops));
        out.push_str(&format!("rec_speed={}\n", self.rec_speed));
        out.push_str(&format!("cac_interval={}\n", self.cac_interval_s));
        out.push_str(&format!("cac_clicks={}\n", self.cac_clicks));
        out.push_str(&format!("akp_keys={}\n", self.akp_keys));
        out.push_str(&format!("akp_interval={}\n", self.akp_interval_s));
        out.push_str(&format!("akp_delay={}\n", self.akp_key_gap_s));
        out
    }

    /// Anything missing or malformed falls back to the default, so a damaged
    /// config.ini can never stop the app from starting.
    pub fn from_ini(text: &str) -> Self {
        let mut cfg = Config::default();
        let mut section = String::new();
        let mut kv: BTreeMap<String, String> = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].to_ascii_lowercase();
                continue;
            }
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim().to_ascii_lowercase();
                let val = line[eq + 1..].trim().to_string();
                kv.insert(format!("{}.{}", section, key), val);
            }
        }
        let get = |k: &str| kv.get(k).map(|s| s.as_str()).unwrap_or("");
        for (id, _) in ACTIONS {
            let v = get(&format!("hotkeys.{}", id)).to_string();
            cfg.hotkeys.insert((*id).to_string(), v);
        }
        let num = |k: &str, d: f64| {
            get(k)
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .unwrap_or(d)
        };
        let flag = |k: &str, d: bool| match get(k) {
            "" => d,
            "0" => false,
            _ => true,
        };
        cfg.hotkeys_enabled = flag("settings.hk_enabled", true);
        cfg.interval_s = num("settings.interval", 10.0).max(0.05);
        cfg.ts_mode = (num("settings.ts_mode", 1.0) as u32).clamp(1, 3);
        cfg.click_mode = ClickMode::from_str(get("settings.click_mode"));
        cfg.point = parse_point(get("settings.point_x"), get("settings.point_y"));
        cfg.point_clicks = (num("settings.point_clicks", 4.0) as u32).clamp(1, 80);
        cfg.kat_picked = get("settings.kat_picked")
            .split([',', ' '])
            .filter_map(|p| p.trim().parse::<u32>().ok())
            .filter(|n| KAT_COUNTS.contains(n))
            .collect();
        cfg.place_slot = (num("settings.place_slot", 1.0) as u32).clamp(1, 9);
        cfg.place_point = parse_point(get("settings.place_x"), get("settings.place_y"));
        cfg.place_gap_ms = (num("settings.place_delay", MIN_STEP_GAP_MS as f64) as u64)
            .clamp(MIN_STEP_GAP_MS, 5000);
        let name = get("settings.rec_name");
        cfg.rec_name = if name.is_empty() {
            "macro1".into()
        } else {
            name.to_string()
        };
        cfg.rec_path = get("settings.rec_path").to_string();
        cfg.rec_moves = flag("settings.rec_moves", true);
        cfg.rec_sample_ms = (num("settings.rec_sample", 33.0) as u64).clamp(8, 200);
        cfg.rec_loops = (num("settings.rec_loops", 1.0) as u32).min(99);
        cfg.rec_speed = num("settings.rec_speed", 1.0);
        if cfg.rec_speed <= 0.0 {
            cfg.rec_speed = 1.0;
        }
        cfg.cac_interval_s = num("settings.cac_interval", 1.0).max(0.01);
        cfg.cac_clicks = (num("settings.cac_clicks", 1.0) as u32).max(1);
        cfg.akp_keys = get("settings.akp_keys").to_string();
        cfg.akp_interval_s = num("settings.akp_interval", 1.0).max(0.01);
        cfg.akp_key_gap_s = num("settings.akp_delay", 0.05).max(0.0);
        cfg
    }
}

fn parse_point(x: &str, y: &str) -> Option<(i32, i32)> {
    match (x.parse::<i32>(), y.parse::<i32>()) {
        (Ok(a), Ok(b)) => Some((a, b)),
        _ => None,
    }
}

pub fn parse_key_list(s: &str) -> Vec<String> {
    s.split([',', ' '])
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_with_no_keys_bound() {
        let c = Config::default();
        assert!(ACTIONS.iter().all(|(id, _)| c.hotkey(id).is_empty()));
    }

    #[test]
    fn katakuri_rate_is_seven_over_the_count() {
        // the two values the AHK build shipped with, which pin the formula
        assert_eq!(kat_interval_ms(4), 1750);
        assert_eq!(kat_interval_ms(8), 875);
        assert_eq!(kat_interval_ms(3), 2333);
        assert_eq!(kat_interval_ms(7), 1000);
    }

    #[test]
    fn round_trips_through_ini() {
        let mut c = Config::default();
        c.set_hotkey("auto_ts", "F6");
        c.set_hotkey("kat4", "F4");
        c.interval_s = 7.5;
        c.ts_mode = 3;
        c.click_mode = ClickMode::Placement;
        c.point = Some((666, 606));
        c.point_clicks = 6;
        c.kat_picked = [4, 8].into_iter().collect();
        c.place_slot = 3;
        c.place_point = Some((900, 500));
        c.place_gap_ms = 750;

        let back = Config::from_ini(&c.to_ini());
        assert_eq!(back.hotkey("auto_ts"), "F6");
        assert_eq!(back.hotkey("kat4"), "F4");
        assert_eq!(back.click_mode, ClickMode::Placement);
        assert!(back.is_placement());
        assert_eq!(back.point, Some((666, 606)));
        assert_eq!(back.point_clicks, 6);
        assert_eq!(back.kat_picked, [4, 8].into_iter().collect());
        assert_eq!(back.place_slot, 3);
        assert_eq!(back.place_point, Some((900, 500)));
        assert_eq!(back.place_gap_ms, 750);
    }

    #[test]
    fn junk_values_fall_back_instead_of_crashing() {
        let ini = "[Settings]\ninterval=abc\nts_mode=99\npoint_clicks=999\nplace_delay=10\nkat_picked=9,abc,4\n";
        let c = Config::from_ini(ini);
        assert_eq!(c.interval_s, 10.0);
        assert_eq!(c.ts_mode, 3);
        assert_eq!(c.point_clicks, 80);
        assert_eq!(c.place_gap_ms, MIN_STEP_GAP_MS);
        // 9 is out of range and "abc" is not a number; only 4 survives
        assert_eq!(c.kat_picked, [4].into_iter().collect());
    }

    #[test]
    fn an_old_config_below_the_floor_is_raised() {
        let c = Config::from_ini("[Settings]\nplace_delay=120\n");
        assert_eq!(c.step_gap_ms(), MIN_STEP_GAP_MS);
    }

    #[test]
    fn binding_a_key_releases_its_previous_owner() {
        let mut c = Config::default();
        c.set_hotkey("tsuku_tmr", "F1");
        c.set_hotkey("cockpit", "F1");
        assert_eq!(c.hotkey("tsuku_tmr"), "");
        assert_eq!(c.hotkey("cockpit"), "F1");
    }

    #[test]
    fn effective_interval_follows_speed() {
        let mut c = Config::default();
        c.interval_s = 10.0;
        assert_eq!(c.effective_interval_ms(), 10_000);
        c.ts_mode = 2;
        assert_eq!(c.effective_interval_ms(), 5_000);
    }

    #[test]
    fn recorder_settings_survive_a_round_trip() {
        let mut c = Config::default();
        c.rec_name = "fishing".into();
        c.rec_moves = false;
        c.rec_sample_ms = 50;
        c.rec_loops = 0;
        c.rec_speed = 2.0;
        let back = Config::from_ini(&c.to_ini());
        assert_eq!(back.rec_name, "fishing");
        assert!(!back.rec_moves);
        assert_eq!(back.rec_sample_ms, 50);
        assert_eq!(back.rec_loops, 0);
        assert_eq!(back.rec_speed, 2.0);
    }

    #[test]
    fn silly_recorder_values_are_clamped() {
        let c =
            Config::from_ini("[Settings]\nrec_sample=1\nrec_loops=999\nrec_speed=0\nrec_name=\n");
        assert_eq!(c.rec_sample_ms, 8);
        assert_eq!(c.rec_loops, 99);
        assert_eq!(c.rec_speed, 1.0);
        assert_eq!(c.rec_name, "macro1");
    }

    #[test]
    fn key_list_splits_on_commas_and_spaces() {
        assert_eq!(
            parse_key_list("e, q,r  space"),
            vec!["e", "q", "r", "space"]
        );
        assert!(parse_key_list("  ").is_empty());
    }
}
