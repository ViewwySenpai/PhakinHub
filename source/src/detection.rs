//! Read-only HUD recognition. All image classification and debounce logic are
//! pure Rust; tests use cropped user-supplied screenshots, never the desktop.
use image::RgbImage;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Region {
    pub fn valid(self) -> bool {
        [self.x, self.y, self.w, self.h]
            .iter()
            .all(|v| v.is_finite())
            && self.x >= 0.0
            && self.y >= 0.0
            && self.w >= 0.01
            && self.h >= 0.01
            && self.x + self.w <= 1.001
            && self.y + self.h <= 1.001
    }
    pub fn pixels(self, width: i32, height: i32) -> Option<[i32; 4]> {
        if !self.valid() || width < 320 || height < 240 {
            return None;
        }
        let x = (self.x * width as f32).round() as i32;
        let y = (self.y * height as f32).round() as i32;
        let w = ((self.w * width as f32).round() as i32).min(width - x);
        let h = ((self.h * height as f32).round() as i32).min(height - y);
        (w >= 8 && h >= 8).then_some([x, y, w, h])
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub speed_auto: bool,
    pub tournament_auto: bool,
    pub speed_region: Region,
    pub wave_region: Region,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            speed_auto: false,
            tournament_auto: false,
            speed_region: Region {
                x: 312. / 1919.,
                y: 935. / 1079.,
                w: 90. / 1919.,
                h: 60. / 1079.,
            },
            wave_region: Region {
                x: 795. / 1919.,
                y: 0.,
                w: 335. / 1919.,
                h: 73. / 1079.,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Wave {
    Tournament,
    Idle,
    #[default]
    Unknown,
}
#[derive(Clone, Debug, Default)]
pub struct Reading {
    pub speed: Option<u32>,
    pub wave: Wave,
}

fn matches_color([r, g, b]: [u8; 3], color: u32) -> bool {
    match color {
        1 => r >= 228 && g >= 228 && b >= 228 && r.max(g).max(b) - r.min(g).min(b) < 22,
        2 => g >= 190 && r < 110 && (85..=200).contains(&b) && g > b.saturating_add(35),
        3 => g >= 190 && (115..=225).contains(&r) && b < 100 && g > r.saturating_add(25),
        _ => r >= 155 && (70..=195).contains(&g) && b < 80 && r as f32 > g as f32 * 1.25,
    }
}
struct Mask {
    width: usize,
    height: usize,
    fill: Vec<(usize, usize)>,
    edge: Vec<(usize, usize)>,
}
impl Mask {
    fn parse(text: &str, scale: f32) -> Self {
        let lines: Vec<_> = text.lines().map(str::as_bytes).collect();
        let width = (lines[0].len() as f32 * scale).round() as usize;
        let height = (lines.len() as f32 * scale).round() as usize;
        let mut m = Self {
            width,
            height,
            fill: Vec::new(),
            edge: Vec::new(),
        };
        for y in 0..height {
            for x in 0..width {
                match lines[(y as f32 / scale) as usize][(x as f32 / scale) as usize] {
                    b'+' => m.fill.push((x, y)),
                    b'#' => m.edge.push((x, y)),
                    _ => (),
                }
            }
        }
        m
    }
    fn score(&self, classes: &[u8], width: usize, height: usize) -> f32 {
        if self.width > width || self.height > height {
            return 0.;
        }
        let fraction = |points: &[(usize, usize)], x: usize, y: usize, kind: u8, step: usize| {
            let mut good = 0;
            let mut total = 0;
            for &(px, py) in points.iter().step_by(step.max(1)) {
                total += 1;
                good += (classes[(y + py) * width + x + px] == kind) as u32;
            }
            good as f32 / total.max(1) as f32
        };
        let mut best: f32 = 0.;
        for y in 0..=height - self.height {
            for x in 0..=width - self.width {
                if fraction(&self.fill, x, y, 1, self.fill.len() / 24) < 0.70
                    || fraction(&self.edge, x, y, 2, self.edge.len() / 24) < 0.45
                {
                    continue;
                }
                let fill = fraction(&self.fill, x, y, 1, 1);
                let edge = fraction(&self.edge, x, y, 2, 1);
                // A solid patch of the same color cannot pass the dark-outline test.
                if fill >= 0.78 && edge >= 0.55 {
                    best = best.max(fill * 0.7 + edge * 0.3);
                }
            }
        }
        best
    }
}
struct Templates {
    masks: Vec<Vec<Mask>>,
}
fn templates() -> &'static Templates {
    static T: OnceLock<Templates> = OnceLock::new();
    T.get_or_init(|| Templates {
        masks: [
            include_str!("../assets/detection/speed_1.mask"),
            include_str!("../assets/detection/speed_2.mask"),
            include_str!("../assets/detection/speed_3.mask"),
            include_str!("../assets/detection/wave_idle.mask"),
            include_str!("../assets/detection/wave_left.mask"),
        ]
        .iter()
        .map(|s| {
            [0.8, 0.9, 1., 1.1, 1.2]
                .into_iter()
                .map(|scale| Mask::parse(s, scale))
                .collect()
        })
        .collect(),
    })
}
fn confidence(image: &RgbImage, index: usize, color: u32) -> f32 {
    let classes: Vec<_> = image
        .pixels()
        .map(|p| {
            // Small HUD text has blended outlines after window/DPI scaling.
            if matches_color(p.0, color) {
                1
            } else if p.0.iter().all(|v| *v < 160) {
                2
            } else {
                0
            }
        })
        .collect();
    templates().masks[index]
        .iter()
        .map(|m| m.score(&classes, image.width() as usize, image.height() as usize))
        .fold(0., f32::max)
}
pub fn speed(image: &RgbImage) -> Option<u32> {
    let mut scores = [
        (1, confidence(image, 0, 1)),
        (2, confidence(image, 1, 2)),
        (3, confidence(image, 2, 3)),
    ];
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    (scores[0].1 >= 0.75 && scores[0].1 - scores[1].1 >= 0.12).then_some(scores[0].0)
}
pub fn wave(image: &RgbImage) -> Wave {
    if confidence(image, 4, 4) >= 0.75 {
        Wave::Tournament
    } else if confidence(image, 3, 4) >= 0.75 {
        Wave::Idle
    } else {
        Wave::Unknown
    }
}

/// Stable observations are required before changing speed or starting a timer.
/// Missing/occluded frames never re-arm a tournament or change the speed.
#[derive(Default)]
pub struct Tracker {
    speed_candidate: Option<u32>,
    speed_since: u64,
    speed_samples: u32,
    wave_candidate: Wave,
    wave_since: u64,
    wave_samples: u32,
    latched: bool,
}
impl Tracker {
    pub fn reset_candidates(&mut self) {
        self.speed_candidate = None;
        self.speed_samples = 0;
        self.wave_candidate = Wave::Unknown;
        self.wave_samples = 0;
    }
    pub fn rearm(&mut self) {
        self.latched = false;
        self.reset_candidates();
    }
    pub fn suppress_current(&mut self) {
        self.latched = true;
        self.reset_candidates();
    }
    pub fn observe(&mut self, reading: &Reading, now: u64) -> (Option<u32>, bool) {
        if reading.speed != self.speed_candidate {
            self.speed_candidate = reading.speed;
            self.speed_since = now;
            self.speed_samples = 0;
        }
        self.speed_samples = self.speed_samples.saturating_add(1);
        let speed = if self.speed_samples >= 3 && now.saturating_sub(self.speed_since) >= 700 {
            reading.speed
        } else {
            None
        };
        if reading.wave != self.wave_candidate {
            self.wave_candidate = reading.wave;
            self.wave_since = now;
            self.wave_samples = 0;
        }
        self.wave_samples = self.wave_samples.saturating_add(1);
        if reading.wave == Wave::Idle
            && self.wave_samples >= 3
            && now.saturating_sub(self.wave_since) >= 2000
        {
            self.latched = false;
        }
        let start = reading.wave == Wave::Tournament
            && self.wave_samples >= 3
            && now.saturating_sub(self.wave_since) >= 700
            && !self.latched;
        if start {
            self.latched = true;
        }
        (speed, start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> RgbImage {
        image::open(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(name),
        )
        .unwrap()
        .to_rgb8()
    }
    #[test]
    fn recognizes_supplied_speed_colors_and_rejects_missing_hud() {
        for n in 1..=3 {
            assert_eq!(speed(&fixture(&format!("speed_{n}.png"))), Some(n));
        }
        assert_eq!(speed(&fixture("speed_4.png")), None);
        for color in [[255, 255, 255], [47, 255, 151], [178, 255, 35], [0, 0, 0]] {
            assert_eq!(
                speed(&RgbImage::from_pixel(90, 60, image::Rgb(color))),
                None
            );
        }
    }
    #[test]
    fn tournament_header_is_distinct_from_regular_wave() {
        assert_eq!(wave(&fixture("wave_4.png")), Wave::Tournament);
        for n in 1..=2 {
            assert_eq!(wave(&fixture(&format!("wave_{n}.png"))), Wave::Idle);
        }
        assert_ne!(wave(&fixture("wave_3.png")), Wave::Tournament);
        assert_eq!(
            wave(&RgbImage::from_pixel(335, 73, image::Rgb([230, 155, 30]))),
            Wave::Unknown
        );
    }
    #[test]
    fn scaled_hud_still_matches_after_normalization() {
        use image::imageops::{resize, FilterType::Triangle};
        for n in 1..=3 {
            let im = fixture(&format!("speed_{n}.png"));
            let small = resize(&im, 60, 40, Triangle);
            assert_eq!(speed(&resize(&small, 90, 60, Triangle)), Some(n));
        }
        let small = resize(&fixture("wave_4.png"), 223, 49, Triangle);
        assert_eq!(wave(&resize(&small, 335, 73, Triangle)), Wave::Tournament);
    }
    #[test]
    fn debounce_and_missing_frames_do_not_reset_a_running_tournament() {
        let mut t = Tracker::default();
        let found = Reading {
            speed: Some(3),
            wave: Wave::Tournament,
        };
        assert_eq!(t.observe(&found, 0), (None, false));
        assert_eq!(t.observe(&found, 400), (None, false));
        assert_eq!(t.observe(&found, 800), (Some(3), true));
        assert_eq!(t.observe(&found, 5000), (Some(3), false));
        t.reset_candidates(); // Alt-tab / capture failure
        for now in [6000, 6400, 6800] {
            assert!(!t.observe(&found, now).1);
        }
        for now in [7000, 8000, 9000] {
            assert!(
                !t.observe(
                    &Reading {
                        speed: None,
                        wave: Wave::Idle
                    },
                    now
                )
                .1
            );
        }
        assert!(!t.observe(&found, 10000).1);
        assert!(!t.observe(&found, 10400).1);
        assert!(t.observe(&found, 10800).1);
    }
    #[test]
    fn region_rejects_invalid_and_outside_coordinates() {
        let mut r = Settings::default().speed_region;
        assert_eq!(r.pixels(1919, 1079), Some([312, 935, 90, 60]));
        r.x = f32::NAN;
        assert_eq!(r.pixels(1919, 1079), None);
        r.x = 0.99;
        assert_eq!(r.pixels(1919, 1079), None);
    }
}
