use crate::core::{
    config::{Config, ACTIONS},
    timers::{default_timers, PTimer},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Preferences {
    pub detection: crate::detection::Settings,
    pub dark: bool,
    pub on_top: bool,
    pub hold_on: bool,
    pub hold_delay: u64,
    pub hold_rate: u64,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            detection: crate::detection::Settings::default(),
            dark: false,
            on_top: false,
            hold_on: false,
            hold_delay: 1000,
            hold_rate: 50,
        }
    }
}
pub type Ini = BTreeMap<String, String>;
pub fn ini(text: &str) -> Ini {
    let mut map = Ini::new();
    let mut section = String::new();
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_lowercase();
        } else if let Some((key, value)) = line.split_once('=') {
            map.insert(
                format!("{}.{}", section, key.trim().to_lowercase()),
                value.trim().into(),
            );
        }
    }
    map
}
pub fn decode(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xff, 0xfe]) {
        String::from_utf16_lossy(
            &bytes[2..]
                .chunks_exact(2)
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect::<Vec<_>>(),
        )
    } else {
        String::from_utf8_lossy(bytes)
            .trim_start_matches('\u{feff}')
            .to_owned()
    }
}
pub fn parse(text: &str) -> (Config, Vec<PTimer>, Preferences) {
    let data = ini(text);
    let get = |key: &str| data.get(key).map(String::as_str).unwrap_or("");
    let mut normalized = text.to_owned();
    normalized.push_str("\n[Settings]\n");
    for (old, new) in [
        ("rec.name", "rec_name"),
        ("rec.path", "rec_path"),
        ("rec.moves", "rec_moves"),
        ("rec.sample", "rec_sample"),
        ("rec.loops", "rec_loops"),
        ("rec.speed", "rec_speed"),
        ("settings.kat_on", "kat_picked"),
    ] {
        if data.contains_key(old) {
            normalized.push_str(&format!("{new}={}\n", get(old)));
        }
    }
    let mut cfg = Config::from_ini(&normalized);
    for (key, value) in &data {
        if let Some(action) = key.strip_prefix("hotkeys.") {
            cfg.hotkeys.insert(action.into(), value.clone());
        }
    }
    if cfg.hotkey("play_toggle").is_empty() {
        if let Some(value) = cfg.hotkeys.remove("rec_play") {
            cfg.hotkeys.insert("play_toggle".into(), value);
        }
    }
    let mut timers = default_timers();
    let ids = if data.contains_key("timers.extra_ids") {
        get("timers.extra_ids").to_owned()
    } else {
        let count = get("timers.extra_count")
            .parse::<u32>()
            .unwrap_or(0)
            .min(1000);
        (3..3 + count)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    for number in ids
        .split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .filter(|n| *n >= 3)
        .take(1000)
    {
        let id = format!("custom{number}");
        if timers.iter().any(|t| t.id == id) {
            continue;
        }
        let mut t = PTimer::new(
            &id,
            &format!("TIMER {number}"),
            0,
            &format!("timer_{number}_tmr"),
        );
        t.custom = true;
        timers.push(t);
    }
    for t in &mut timers {
        let name = get(&format!("timers.{}_name", t.id));
        if !name.is_empty() {
            t.name = name.chars().take(48).collect();
        }
        t.detached = get(&format!("timers.{}_detached", t.id)) == "1";
        let xy = (
            get(&format!("timers.{}_x", t.id)).parse::<f32>(),
            get(&format!("timers.{}_y", t.id)).parse::<f32>(),
        );
        if let (Ok(x), Ok(y)) = xy {
            if x.is_finite() && y.is_finite() {
                t.position = Some([x, y]);
            }
        }
        cfg.hotkeys.entry(t.hotkey_id.clone()).or_default();
    }
    cfg.hotkeys.retain(|id, _| {
        ACTIONS.iter().any(|(a, _)| a == id) || timers.iter().any(|t| &t.hotkey_id == id)
    });
    let pref = Preferences {
        detection: {
            let mut d = crate::detection::Settings::default();
            d.speed_auto = get("detection.speed_auto") == "1";
            d.tournament_auto = get("detection.tournament_auto") == "1";
            for (name, region) in [("speed", &mut d.speed_region), ("wave", &mut d.wave_region)] {
                let parts: Vec<f32> = get(&format!("detection.{name}_region"))
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
                if let [x, y, w, h] = parts.as_slice() {
                    let candidate = crate::detection::Region {
                        x: *x,
                        y: *y,
                        w: *w,
                        h: *h,
                    };
                    if candidate.valid() {
                        *region = candidate;
                    }
                }
            }
            d
        },
        dark: get("appearance.theme") == "dark",
        on_top: get("appearance.always_on_top") == "1",
        hold_on: get("settings.hold_on") == "1",
        hold_delay: get("settings.hold_delay")
            .parse()
            .unwrap_or(1000)
            .clamp(200, 3000),
        hold_rate: get("settings.hold_rate")
            .parse()
            .unwrap_or(50)
            .clamp(20, 500),
    };
    (cfg, timers, pref)
}
pub fn serialize(cfg: &Config, timers: &[PTimer], pref: &Preferences) -> String {
    let mut text = cfg.to_ini();
    text.push_str(&format!("hold_on={}\nhold_delay={}\nhold_rate={}\n\n[Appearance]\ntheme={}\nalways_on_top={}\n\n[Timers]\n",pref.hold_on as u8,pref.hold_delay,pref.hold_rate,if pref.dark{"dark"}else{"light"},pref.on_top as u8));
    let ids = timers
        .iter()
        .filter(|t| t.custom)
        .map(|t| t.id.trim_start_matches("custom"))
        .collect::<Vec<_>>();
    text.push_str(&format!(
        "extra_count={}\nextra_ids={}\n",
        ids.len(),
        ids.join(",")
    ));
    for t in timers {
        let name = t.name.replace(['\r', '\n'], " ");
        text.push_str(&format!(
            "{}_name={}\n{}_detached={}\n",
            t.id, name, t.id, t.detached as u8
        ));
        if let Some([x, y]) = t.position {
            text.push_str(&format!("{}_x={}\n{}_y={}\n", t.id, x, t.id, y));
        }
    }
    let d = &pref.detection;
    text.push_str(&format!(
        "\n[Detection]\nspeed_auto={}\ntournament_auto={}\n",
        d.speed_auto as u8, d.tournament_auto as u8
    ));
    for (name, r) in [("speed", d.speed_region), ("wave", d.wave_region)] {
        text.push_str(&format!("{name}_region={},{},{},{}\n", r.x, r.y, r.w, r.h));
    }
    text
}
pub fn app_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("PHAKINHUB_DATA_DIR") {
        return path.into();
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| ".".into())
}
pub fn load(dir: &Path) -> (Config, Vec<PTimer>, Preferences) {
    let path = if dir.join("phakinhub-rust.ini").exists() {
        dir.join("phakinhub-rust.ini")
    } else {
        dir.join("config.ini")
    };
    parse(&std::fs::read(path).map(|b| decode(&b)).unwrap_or_default())
}
pub fn save(
    dir: &Path,
    cfg: &Config,
    timers: &[PTimer],
    pref: &Preferences,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let temp = dir.join("phakinhub-rust.ini.tmp");
    std::fs::write(&temp, serialize(cfg, timers, pref))?;
    std::fs::rename(temp, dir.join("phakinhub-rust.ini"))
}
pub fn add_timer(timers: &mut Vec<PTimer>, cfg: &mut Config) -> String {
    let mut n = 3;
    while timers.iter().any(|t| t.id == format!("custom{n}")) {
        n += 1;
    }
    let mut t = PTimer::new(
        &format!("custom{n}"),
        &format!("TIMER {n}"),
        0,
        &format!("timer_{n}_tmr"),
    );
    t.custom = true;
    cfg.hotkeys.insert(t.hotkey_id.clone(), String::new());
    let id = t.id.clone();
    timers.push(t);
    id
}
pub fn delete_timer(timers: &mut Vec<PTimer>, cfg: &mut Config, id: &str) -> bool {
    if let Some(i) = timers.iter().position(|t| t.id == id && t.custom) {
        let t = timers.remove(i);
        cfg.hotkeys.remove(&t.hotkey_id);
        true
    } else {
        false
    }
}
pub fn actions(timers: &[PTimer]) -> Vec<(String, String, &'static str)> {
    let mut out = Vec::new();
    for (id, label) in ACTIONS {
        if timers.iter().any(|t| t.hotkey_id == *id) {
            continue;
        }
        let group = if id.starts_with("kat") {
            "Katakuri"
        } else if ["auto_ts", "inc_delay", "dec_delay"].contains(id) {
            "Auto TS"
        } else if ["rec_toggle", "play_toggle"].contains(id) {
            "Record / Playback"
        } else if [
            "cac",
            "akp",
            "hold_repeat",
            "homura_upgrade",
            "kurumi_upgrade",
        ]
        .contains(id)
        {
            "Auto actions"
        } else {
            "Panel / System"
        };
        out.push((id.to_string(), label.to_string(), group));
    }
    out.extend(
        timers
            .iter()
            .map(|t| (t.hotkey_id.clone(), t.name.clone(), "Timers")),
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_ahk_settings_and_sparse_timer_ids() {
        let (c,t,_) = parse("[Timers]\nextra_ids=3,5\ncustom5_name=Keep Five\n[Rec]\nspeed=7.5\n[Hotkeys]\ntimer_5_tmr=XButton2\n[Settings]\nkat_on=3, 8\n");
        assert_eq!(t.len(), 8);
        assert_eq!(
            t.iter().find(|t| t.id == "custom5").unwrap().name,
            "Keep Five"
        );
        assert_eq!(c.rec_speed, 7.5);
        assert_eq!(c.hotkey("timer_5_tmr"), "XButton2");
        assert_eq!(c.kat_picked.len(), 2);
    }
    #[test]
    fn delete_does_not_renumber_survivors() {
        let (mut c, mut t, mut p) =
            parse("[Timers]\nextra_count=3\ncustom5_name=Keep\n[Hotkeys]\ntimer_5_tmr=F8");
        delete_timer(&mut t, &mut c, "custom4");
        p.dark = true;
        let survivor = t.iter_mut().find(|t| t.id == "custom5").unwrap();
        survivor.detached = true;
        survivor.position = Some([-200., 130.]);
        let (c, t, p) = parse(&serialize(&c, &t, &p));
        let survivor = t.iter().find(|t| t.id == "custom5").unwrap();
        assert_eq!(survivor.name, "Keep");
        assert!(p.dark && survivor.detached);
        assert_eq!(c.hotkey("timer_5_tmr"), "F8");
        assert!(!c.hotkeys.contains_key("timer_4_tmr"));
    }
    #[test]
    fn empty_deleted_list_stays_empty_and_reuses_clean_number() {
        let (mut c, mut t, p) = parse("");
        let id = add_timer(&mut t, &mut c);
        c.set_hotkey("timer_3_tmr", "F9");
        assert!(delete_timer(&mut t, &mut c, &id));
        let (mut c, mut t, _) = parse(&serialize(&c, &t, &p));
        assert_eq!(t.len(), 6);
        add_timer(&mut t, &mut c);
        assert_eq!(c.hotkey("timer_3_tmr"), "");
        assert!(!delete_timer(&mut t, &mut c, "t3"));
    }
    #[test]
    fn utf16_ahk_ini() {
        let data: Vec<_> = [0xfeffu16]
            .into_iter()
            .chain("[Appearance]\ntheme=dark".encode_utf16())
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(parse(&decode(&data)).2.dark);
    }
    #[test]
    fn detection_preferences_and_tournament_survive_migration() {
        let (mut cfg, mut timers, mut pref) =
            parse("[Hotkeys]\ngyro_tmr=F7\n[Timers]\nextra_ids=3,5\n");
        pref.detection.speed_auto = true;
        pref.detection.tournament_auto = true;
        pref.detection.wave_region.x = 0.4;
        let timer = timers.iter_mut().find(|t| t.id == "tournament").unwrap();
        timer.detached = true;
        cfg.set_hotkey("tournament_tmr", "F8");
        let (cfg, timers, loaded) = parse(&serialize(&cfg, &timers, &pref));
        assert_eq!(loaded.detection, pref.detection);
        assert_eq!(cfg.hotkey("gyro_tmr"), "F7");
        assert_eq!(cfg.hotkey("tournament_tmr"), "F8");
        assert_eq!(timers.iter().filter(|t| t.id == "tournament").count(), 1);
        assert!(
            timers
                .iter()
                .find(|t| t.id == "tournament")
                .unwrap()
                .detached
        );
        let (_, _, invalid) =
            parse("[Detection]\nspeed_region=NaN,0,1,1\nwave_region=0.99,0,0.5,0.2");
        assert_eq!(invalid.detection, crate::detection::Settings::default());
    }
    #[test]
    fn settings_can_be_replaced_and_reloaded_on_windows() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("save-test-{}", std::process::id()));
        let (mut cfg, mut timers, mut pref) = parse("");
        save(&dir, &cfg, &timers, &pref).unwrap();
        pref.dark = true;
        add_timer(&mut timers, &mut cfg);
        timers.last_mut().unwrap().name = "ทดสอบ Timer".into();
        save(&dir, &cfg, &timers, &pref).unwrap();
        let (_, loaded, appearance) = load(&dir);
        assert!(appearance.dark);
        assert_eq!(loaded.last().unwrap().name, "ทดสอบ Timer");
        std::fs::remove_file(dir.join("phakinhub-rust.ini")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
