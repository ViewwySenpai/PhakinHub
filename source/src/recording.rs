use crate::core::keys;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyCode {
    pub value: u16,
    pub scan: bool,
}
impl KeyCode {
    pub fn vk(value: u16) -> Self {
        Self { value, scan: false }
    }
    pub fn token(self) -> String {
        format!("{}{:03X}", if self.scan { "sc" } else { "vk" }, self.value)
    }
    pub fn parse(s: &str) -> Option<Self> {
        let lower = s.to_ascii_lowercase();
        if let Some(hex) = lower.strip_prefix("sc") {
            return u16::from_str_radix(hex, 16)
                .ok()
                .filter(|v| *v > 0 && *v <= 0x1ff)
                .map(|value| Self { value, scan: true });
        }
        keys::vk_from_name(s).map(Self::vk)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Key(KeyCode, bool),
    Button(u8, bool),
    Move(i32, i32),
    Wheel(i32, bool),
    Wait,
}
#[derive(Debug, Clone)]
pub struct Event {
    pub at: u64,
    pub op: Op,
}
pub fn button_name(button: u8) -> &'static str {
    match button {
        1 => "LButton",
        2 => "RButton",
        3 => "MButton",
        4 => "XButton1",
        5 => "XButton2",
        _ => "",
    }
}
pub fn button_from_name(name: &str) -> Option<u8> {
    (1..=5).find(|&b| button_name(b).eq_ignore_ascii_case(name))
}
pub fn parse(text: &str) -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    let mut at = 0u64;
    for (idx, line) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let parts: Vec<_> = line.split_whitespace().collect();
        let error = || format!("Invalid recording at line {}", idx + 1);
        if parts.len() < 2 {
            return Err(error());
        }
        at = at
            .checked_add(parts[1].parse::<u64>().map_err(|_| error())?)
            .ok_or_else(error)?;
        let value = parts.get(2).copied().unwrap_or("");
        let op = match parts[0] {
            "k" | "K" => Op::Key(KeyCode::parse(value).ok_or_else(error)?, parts[0] == "k"),
            "d" | "u" => Op::Button(button_from_name(value).ok_or_else(error)?, parts[0] == "d"),
            "m" => Op::Move(
                value.parse().map_err(|_| error())?,
                parts
                    .get(3)
                    .ok_or_else(error)?
                    .parse()
                    .map_err(|_| error())?,
            ),
            "w" => match value.to_ascii_lowercase().as_str() {
                "wheelup" => Op::Wheel(120, false),
                "wheeldown" => Op::Wheel(-120, false),
                "wheelright" => Op::Wheel(120, true),
                "wheelleft" => Op::Wheel(-120, true),
                _ => return Err(error()),
            },
            "t" => Op::Wait,
            _ => return Err(error()),
        };
        events.push(Event { at, op });
    }
    Ok(events)
}
pub struct Recorder {
    writer: BufWriter<File>,
    pub started: u64,
    last: u64,
    pub count: u64,
    held: BTreeSet<KeyCode>,
    mouse: BTreeSet<u8>,
    last_move: u64,
    last_xy: Option<(i32, i32)>,
}
impl Recorder {
    pub fn new(path: &Path, now: u64) -> io::Result<Self> {
        let mut writer = BufWriter::new(File::create(path)?);
        writeln!(writer, "; PhakinHub Rust recording v2")?;
        Ok(Self {
            writer,
            started: now,
            last: now,
            count: 0,
            held: BTreeSet::new(),
            mouse: BTreeSet::new(),
            last_move: 0,
            last_xy: None,
        })
    }
    pub fn capture(&mut self, now: u64, op: &Op, moves: bool, sample: u64) -> io::Result<()> {
        match op {
            Op::Move(x, y) => {
                if !moves
                    || now.saturating_sub(self.last_move) < sample
                    || self.last_xy == Some((*x, *y))
                {
                    return Ok(());
                }
                self.last_move = now;
                self.last_xy = Some((*x, *y));
            }
            Op::Key(code, down) => {
                if *down {
                    self.held.insert(*code);
                } else if !self.held.remove(code) {
                    return Ok(());
                }
            }
            Op::Button(b, down) => {
                if *down {
                    self.mouse.insert(*b);
                } else if !self.mouse.remove(b) {
                    return Ok(());
                }
            }
            _ => {}
        }
        self.write(now, op)
    }
    pub fn has_release(&self, op: &Op) -> bool {
        match op {
            Op::Key(k, false) => self.held.contains(k),
            Op::Button(b, false) => self.mouse.contains(b),
            _ => false,
        }
    }
    fn write(&mut self, now: u64, op: &Op) -> io::Result<()> {
        let dt = now.saturating_sub(self.last);
        let line = match op {
            Op::Key(k, d) => format!("{} {dt} {}", if *d { "k" } else { "K" }, k.token()),
            Op::Button(b, d) => format!("{} {dt} {}", if *d { "d" } else { "u" }, button_name(*b)),
            Op::Move(x, y) => format!("m {dt} {x} {y}"),
            Op::Wheel(delta, horizontal) => {
                let name = match (*delta > 0, *horizontal) {
                    (true, false) => "WheelUp",
                    (false, false) => "WheelDown",
                    (true, true) => "WheelRight",
                    (false, true) => "WheelLeft",
                };
                let count = (delta.unsigned_abs() / 120).max(1);
                for n in 0..count {
                    writeln!(self.writer, "w {} {name}", if n == 0 { dt } else { 0 })?;
                }
                self.last = now;
                self.count += count as u64;
                return Ok(());
            }
            Op::Wait => format!("t {dt}"),
        };
        writeln!(self.writer, "{line}")?;
        self.last = now;
        self.count += 1;
        Ok(())
    }
    pub fn finish(mut self, now: u64) -> io::Result<u64> {
        for key in self.held.clone() {
            self.write(now, &Op::Key(key, false))?;
        }
        for b in self.mouse.clone() {
            self.write(now, &Op::Button(b, false))?;
        }
        self.write(now, &Op::Wait)?;
        self.writer.flush()?;
        Ok(self.count)
    }
}
pub fn valid_speed(v: f64) -> bool {
    v.is_finite() && v > 0.0
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_and_new_keyup_remain_distinct() {
        let r = parse(
            "k 0 a\nK 50 a\nk 0 sc11D\nK 10 sc11D\nd 0 XButton1\nu 2 XButton1\nw 0 WheelLeft\nt 30",
        )
        .unwrap();
        assert_eq!(r[1].op, Op::Key(KeyCode::vk(65), false));
        assert_eq!(
            r[2].op,
            Op::Key(
                KeyCode {
                    value: 0x11d,
                    scan: true
                },
                true
            )
        );
        assert_eq!(r.last().unwrap().at, 92);
    }
    #[test]
    fn rejects_unsafe_or_broken_records() {
        for text in [
            "k -1 a",
            "k 0 {a}",
            "k 1 scFFFF",
            "u 0 wrong",
            "m 2 x y",
            "unknown 0",
        ] {
            assert!(parse(text).is_err(), "{text}");
        }
    }
    #[test]
    fn arbitrary_positive_speeds() {
        assert!(valid_speed(0.025));
        assert!(valid_speed(37.5));
        for v in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(!valid_speed(v));
        }
    }
    #[test]
    fn empty_and_comments_are_valid() {
        assert!(parse("; header\n\n").unwrap().is_empty());
    }
}
