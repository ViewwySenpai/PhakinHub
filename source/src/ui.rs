use crate::{
    core::{
        config::{kat_interval_ms, ClickMode, KAT_COUNTS},
        keys,
        timers::TimerState,
    },
    model, native, recording,
    runtime::{self, Controller, Mode, Runtime, Shared},
};
use eframe::egui::{
    self, pos2, vec2, Align, Color32 as Color, CornerRadius, FontId, Frame, Layout, Margin,
    RichText, Stroke, Vec2, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Tools,
    Timers,
    AutoTs,
    Katakuri,
    Auto,
    Record,
    Settings,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Tools => "Tools",
            Self::Timers => "Timers",
            Self::AutoTs => "Auto TS",
            Self::Katakuri => "Katakuri",
            Self::Auto => "Auto actions",
            Self::Record => "Record & Playback",
            Self::Settings => "Settings",
        }
    }
}
#[derive(Clone, Copy)]
struct Palette {
    bg: Color,
    panel: Color,
    soft: Color,
    border: Color,
    text: Color,
    muted: Color,
    blue: Color,
    blue_bg: Color,
    green: Color,
    green_bg: Color,
    red: Color,
    red_bg: Color,
    amber: Color,
    amber_bg: Color,
}
fn rgb(hex: u32) -> Color {
    Color::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                bg: rgb(0x101720),
                panel: rgb(0x19222e),
                soft: rgb(0x202c3a),
                border: rgb(0x2d3b4b),
                text: rgb(0xe5edf7),
                muted: rgb(0x98a9be),
                blue: rgb(0x66a8ff),
                blue_bg: rgb(0x213b5d),
                green: rgb(0x63d5ae),
                green_bg: rgb(0x163f36),
                red: rgb(0xff8d96),
                red_bg: rgb(0x452b34),
                amber: rgb(0xf2c27b),
                amber_bg: rgb(0x403727),
            }
        } else {
            Self {
                bg: rgb(0xf1f5fa),
                panel: Color::WHITE,
                soft: rgb(0xf6f8fb),
                border: rgb(0xe4eaf2),
                text: rgb(0x29394f),
                muted: rgb(0x7a8ba3),
                blue: rgb(0x3887f6),
                blue_bg: rgb(0xeaf2ff),
                green: rgb(0x15926d),
                green_bg: rgb(0xe6f7f0),
                red: rgb(0xde5e6b),
                red_bg: rgb(0xffeff1),
                amber: rgb(0xb17b2f),
                amber_bg: rgb(0xfff5e4),
            }
        }
    }
}
fn card(p: Palette) -> Frame {
    Frame::new()
        .fill(p.panel)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(16)
        .inner_margin(20)
}
fn button(ui: &mut egui::Ui, text: &str, p: Palette, kind: u8) -> egui::Response {
    let (bg, fg) = match kind {
        1 => (p.blue, Color::WHITE),
        2 => (p.red_bg, p.red),
        3 => (p.amber_bg, p.amber),
        _ => (p.soft, p.text),
    };
    ui.add(
        egui::Button::new(RichText::new(text).size(13.5).color(fg))
            .fill(bg)
            .stroke(Stroke::new(
                if kind == 1 { 0.0_f32 } else { 1.0_f32 },
                if kind == 2 { p.red_bg } else { p.border },
            ))
            .corner_radius(9)
            .min_size(vec2(74., 34.)),
    )
}
fn badge(ui: &mut egui::Ui, text: &str, fg: Color, bg: Color) {
    Frame::new()
        .fill(bg)
        .corner_radius(6)
        .inner_margin(Margin::symmetric(9, 4))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.).color(fg).strong());
        });
}
fn muted(ui: &mut egui::Ui, text: impl Into<String>, p: Palette) {
    ui.label(RichText::new(text.into()).size(12.5).color(p.muted));
}
fn heading(ui: &mut egui::Ui, text: &str, p: Palette) {
    ui.label(RichText::new(text).size(17.).strong().color(p.text));
}
fn toggle(ui: &mut egui::Ui, value: &mut bool, p: Palette) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(38., 22.), egui::Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    ui.painter()
        .rect_filled(rect, 11., if *value { p.blue } else { p.border });
    ui.painter().circle_filled(
        pos2(
            if *value {
                rect.right() - 11.
            } else {
                rect.left() + 11.
            },
            rect.center().y,
        ),
        8.,
        Color::WHITE,
    );
    response
}
fn draw_icon(painter: &egui::Painter, center: egui::Pos2, kind: usize, color: Color, size: f32) {
    let s = size * 0.37;
    let stroke = Stroke::new(1.6_f32, color);
    match kind {
        0 => {
            for dx in [-s, s] {
                for dy in [-s, s] {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(
                            center + vec2(dx * 0.62, dy * 0.62),
                            vec2(s * 0.82, s * 0.82),
                        ),
                        2.,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        1 => {
            painter.circle_stroke(center, s, stroke);
            painter.line_segment([center, center + vec2(0., -s * 0.6)], stroke);
            painter.line_segment([center, center + vec2(s * 0.5, s * 0.2)], stroke);
        }
        2 => {
            painter.line_segment([center + vec2(-s, -s), center + vec2(-s * 0.5, s)], stroke);
            painter.line_segment([center + vec2(-s, -s), center + vec2(s, s * 0.3)], stroke);
            painter.line_segment(
                [center + vec2(-s * 0.5, s), center + vec2(0., s * 0.2)],
                stroke,
            );
            painter.line_segment(
                [center + vec2(s, s * 0.3), center + vec2(0., s * 0.2)],
                stroke,
            );
        }
        3 => {
            painter.circle_stroke(center, s, stroke);
            painter.circle_filled(center, 2.5, color);
            for d in [vec2(0., -1.), vec2(0., 1.), vec2(-1., 0.), vec2(1., 0.)] {
                painter.line_segment([center + d * (s + 2.), center + d * (s + 5.)], stroke);
            }
        }
        4 => {
            for (y, x) in [(-s, -s / 2.), (0., s / 2.), (s, -s / 3.)] {
                painter.line_segment([center + vec2(-s, y), center + vec2(s, y)], stroke);
                painter.circle_filled(center + vec2(x, y), 3., color);
            }
        }
        5 => {
            painter.circle_stroke(center, s, stroke);
            painter.circle_filled(center, s * 0.48, color);
        }
        _ => {
            painter.circle_stroke(center, s, stroke);
            painter.circle_stroke(center, s * 0.35, stroke);
            for a in 0..8 {
                let angle = a as f32 * std::f32::consts::TAU / 8.;
                let v = vec2(angle.cos(), angle.sin());
                painter.line_segment([center + v * s, center + v * (s + 3.)], stroke);
            }
        }
    }
}
fn icon_tile(ui: &mut egui::Ui, kind: usize, p: Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(56., 56.), egui::Sense::hover());
    ui.painter().rect_filled(r, 12., p.blue_bg);
    draw_icon(ui.painter(), r.center(), kind, p.blue, 23.);
}

struct Artwork {
    logo: egui::TextureHandle,
    katakuri: egui::TextureHandle,
    auto_ts: egui::TextureHandle,
    timer: egui::TextureHandle,
}
impl Artwork {
    fn load(ctx: &egui::Context) -> Self {
        fn texture(ctx: &egui::Context, name: &str, bytes: &[u8]) -> egui::TextureHandle {
            let image = image::load_from_memory(bytes)
                .expect("Bundled artwork must be a valid PNG")
                .into_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            ctx.load_texture(
                name,
                egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                egui::TextureOptions::LINEAR,
            )
        }
        Self {
            logo: texture(ctx, "phakinhub-logo", include_bytes!("../assets/logo.png")),
            katakuri: texture(
                ctx,
                "katakuri-art",
                include_bytes!("../assets/tools/katakuri.png"),
            ),
            auto_ts: texture(
                ctx,
                "auto-ts-art",
                include_bytes!("../assets/tools/auto-ts.png"),
            ),
            timer: texture(
                ctx,
                "timer-art",
                include_bytes!("../assets/tools/timer.png"),
            ),
        }
    }
    fn for_page(&self, page: Page) -> Option<&egui::TextureHandle> {
        match page {
            Page::Katakuri => Some(&self.katakuri),
            Page::AutoTs => Some(&self.auto_ts),
            Page::Timers => Some(&self.timer),
            _ => None,
        }
    }
}
fn paint_artwork(ui: &egui::Ui, texture: &egui::TextureHandle, bounds: egui::Rect) {
    let natural = texture.size_vec2();
    let scale = (bounds.width() / natural.x).min(bounds.height() / natural.y);
    let rect = egui::Rect::from_center_size(bounds.center(), natural * scale);
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(pos2(0., 0.), pos2(1., 1.)),
        Color::WHITE,
    );
}
fn artwork_tile(ui: &mut egui::Ui, texture: &egui::TextureHandle, p: Palette) {
    let (r, _) = ui.allocate_exact_size(vec2(56., 56.), egui::Sense::hover());
    ui.painter().rect_filled(r, 12., p.blue_bg);
    paint_artwork(ui, texture, r.shrink(2.));
}

pub struct Hub {
    artwork: Artwork,
    shared: Shared,
    _controller: Controller,
    page: Page,
    applied_dark: Option<bool>,
    applied_mode: Mode,
    applied_top: bool,
    group: String,
    speed: String,
    normal_position: Option<egui::Pos2>,
    normal_size: Vec2,
}
impl Hub {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        let faces: [(&str, &'static [u8]); 2] = [
            (
                "Fredoka",
                include_bytes!("../assets/fonts/Fredoka-Regular.ttf"),
            ),
            ("Mitr", include_bytes!("../assets/fonts/Mitr-Regular.ttf")),
        ];
        for (name, bytes) in faces {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_static(bytes).into());
        }
        // egui selects the first font containing a glyph: English/numbers use
        // Fredoka, Thai uses Mitr, and the defaults cover remaining symbols.
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .splice(0..0, ["Fredoka".into(), "Mitr".into()]);
        }
        cc.egui_ctx.set_fonts(fonts);
        let shared = Arc::new(Mutex::new(Runtime::new(model::app_dir())));
        let speed = shared.lock().unwrap().engine.cfg.rec_speed.to_string();
        let controller = Controller::start(shared.clone(), cc.egui_ctx.clone());
        Self {
            artwork: Artwork::load(&cc.egui_ctx),
            shared,
            _controller: controller,
            page: Page::Tools,
            applied_dark: None,
            applied_mode: Mode::Full,
            applied_top: false,
            group: "Auto TS".into(),
            speed,
            normal_position: None,
            normal_size: vec2(1120., 760.),
        }
    }
    fn style(&mut self, ctx: &egui::Context, dark: bool) {
        if self.applied_dark == Some(dark) {
            return;
        }
        self.applied_dark = Some(dark);
        let p = Palette::new(dark);
        let mut style = (*ctx.style()).clone();
        style.visuals = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        style.visuals.override_text_color = Some(p.text);
        style.visuals.panel_fill = p.panel;
        style.visuals.window_fill = p.panel;
        style.visuals.extreme_bg_color = p.soft;
        style.visuals.faint_bg_color = p.soft;
        style.visuals.window_stroke = Stroke::new(1.0_f32, p.border);
        style.visuals.window_corner_radius = CornerRadius::same(14);
        style.visuals.selection.bg_fill = p.blue_bg;
        style.visuals.selection.stroke = Stroke::new(1.0_f32, p.blue);
        for v in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.noninteractive,
        ] {
            v.bg_fill = p.soft;
            v.weak_bg_fill = p.soft;
            v.bg_stroke = Stroke::new(1.0_f32, p.border);
            v.fg_stroke = Stroke::new(1.0_f32, p.text);
            v.corner_radius = CornerRadius::same(8);
        }
        style.visuals.widgets.hovered.bg_fill = p.blue_bg;
        style.visuals.widgets.hovered.weak_bg_fill = p.blue_bg;
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.blue);
        style.visuals.widgets.active.bg_fill = p.blue_bg;
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.blue);
        style.spacing.item_spacing = vec2(12., 10.);
        style.spacing.button_padding = vec2(12., 8.);
        style.spacing.interact_size = vec2(80., 32.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(25.));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.));
        ctx.set_style(style);
    }
    fn sidebar(
        &mut self,
        ctx: &egui::Context,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        egui::SidePanel::left("navigation")
            .exact_width(218.)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.panel)
                    .inner_margin(Margin::symmetric(16, 22)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::from_texture(&self.artwork.logo)
                            .fit_to_exact_size(vec2(40., 40.))
                            .corner_radius(10),
                    );
                    ui.label(RichText::new("PHAKIN HUB").size(15.).strong());
                });
                ui.add_space(28.);
                ui.label(RichText::new("WORKSPACE").size(10.).color(p.muted).strong());
                ui.add_space(3.);
                for (i, page) in [
                    Page::Tools,
                    Page::Timers,
                    Page::AutoTs,
                    Page::Katakuri,
                    Page::Auto,
                    Page::Record,
                    Page::Settings,
                ]
                .into_iter()
                .enumerate()
                {
                    let (r, response) = ui
                        .allocate_exact_size(vec2(ui.available_width(), 42.), egui::Sense::click());
                    let selected = self.page == page;
                    let fg = if selected { p.blue } else { p.muted };
                    if selected || response.hovered() {
                        ui.painter()
                            .rect_filled(r, 10., if selected { p.blue_bg } else { p.soft });
                    }
                    if selected {
                        ui.painter().rect_filled(
                            egui::Rect::from_min_size(r.min + vec2(0., 12.), vec2(3., 18.)),
                            2.,
                            p.blue,
                        );
                    }
                    if let Some(texture) = self.artwork.for_page(page) {
                        paint_artwork(
                            ui,
                            texture,
                            egui::Rect::from_center_size(r.min + vec2(23., 21.), vec2(30., 30.)),
                        );
                    } else {
                        draw_icon(ui.painter(), r.min + vec2(23., 21.), i, fg, 19.);
                    }
                    ui.painter().text(
                        r.min + vec2(44., 21.),
                        egui::Align2::LEFT_CENTER,
                        page.title(),
                        FontId::proportional(13.),
                        if selected { p.blue } else { p.text },
                    );
                    if response.clicked() {
                        self.page = page;
                    }
                }
                let show_status = ui.available_height() > 140.;
                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    if show_status {
                        muted(ui, "Native Rust  ·  v6.1.2", p);
                        ui.add_space(6.);
                    }
                    ui.horizontal(|ui| {
                        if button(ui, "Mini view", p, 0).clicked() {
                            actions.push("toggle_ui".into());
                        }
                        if button(ui, "Cockpit", p, 0).clicked() {
                            actions.push("cockpit".into());
                        }
                    });
                    if !show_status {
                        return;
                    }
                    ui.add_space(6.);
                    Frame::new()
                        .fill(if s.hooks_ready { p.green_bg } else { p.red_bg })
                        .corner_radius(10)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(8., 8.), egui::Sense::hover());
                                ui.painter().circle_filled(
                                    r.center(),
                                    3.5,
                                    if s.hooks_ready { p.green } else { p.red },
                                );
                                ui.label(
                                    RichText::new(if s.hooks_ready {
                                        "Input service ready"
                                    } else {
                                        "Input service unavailable"
                                    })
                                    .size(11.)
                                    .color(if s.hooks_ready { p.green } else { p.red }),
                                );
                            });
                        });
                });
            });
    }
    fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.page.title()).size(26.).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, "Stop all", p, 2).clicked() {
                    actions.push("stop_all".into());
                }
                ui.add_space(4.);
                Frame::new()
                    .fill(p.panel)
                    .stroke(Stroke::new(1.0_f32, p.border))
                    .corner_radius(10)
                    .inner_margin(4)
                    .show(ui, |ui| {
                        if ui
                            .add(
                                egui::Button::new("Dark")
                                    .selected(s.pref.dark)
                                    .corner_radius(7),
                            )
                            .clicked()
                        {
                            s.pref.dark = true;
                        }
                        if ui
                            .add(
                                egui::Button::new("Light")
                                    .selected(!s.pref.dark)
                                    .corner_radius(7),
                            )
                            .clicked()
                        {
                            s.pref.dark = false;
                        }
                    });
            });
        });
        ui.add_space(18.);
    }
    fn overview(&mut self, ui: &mut egui::Ui, p: Palette) {
        let items = [
            (Page::Timers, 1, "Independent timers"),
            (Page::AutoTs, 2, "Auto TS"),
            (Page::Katakuri, 3, "Katakuri"),
            (Page::Record, 5, "Record & Playback"),
            (Page::Auto, 4, "Auto actions"),
            (Page::Settings, 6, "Preferences & keybinds"),
        ];
        for row in items.chunks(2) {
            ui.columns(2, |cols| {
                for (col, (page, icon, title)) in cols.iter_mut().zip(row.iter()) {
                    card(p).show(col, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if let Some(texture) = self.artwork.for_page(*page) {
                                artwork_tile(ui, texture, p);
                            } else {
                                icon_tile(ui, *icon, p);
                            }
                            ui.vertical(|ui| {
                                heading(ui, title, p);
                            });
                        });
                        ui.add_space(12.);
                        if button(ui, "Open tool  →", p, 3).clicked() {
                            self.page = *page;
                        }
                    });
                }
            });
            ui.add_space(8.);
        }
    }
    fn timers(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        card(p).show(ui, |ui| {
            heading(ui, "Tournament Countdown", p);
            ui.horizontal_wrapped(|ui| {
                toggle(ui, &mut s.pref.detection.tournament_auto, p);
                ui.label("Automatic start from Wave color");
                badge(ui, if s.pref.detection.tournament_auto { "WATCHING" } else { "OFF" }, p.blue, p.blue_bg);
                if button(ui, "Reset & re-arm", p, 0).clicked() {
                    actions.push("tournament_rearm".into());
                }
            });
            muted(ui, "25:00 in real time. Enable before the tournament starts. Game speed does not affect this timer.", p);
            muted(ui, "Starts once from the orange Tournament header. Re-arms after the normal -Wave- screen, or with Reset & re-arm.", p);
            if s.pref.detection.tournament_auto {
                let label = match s.detected_wave {
                    crate::detection::Wave::Tournament => "Tournament header found",
                    crate::detection::Wave::Idle => "Normal Wave screen",
                    crate::detection::Wave::Unknown => "Waiting for a clear Tournament header",
                };
                ui.label(label);
                muted(ui, &s.detection_message, p);
            }
            detection_region_controls(ui, &mut s.pref.detection.wave_region,
                crate::detection::Settings::default().wave_region, "Wave detection area", p);
        });
        ui.add_space(10.);
        ui.horizontal(|ui| {
            badge(
                ui,
                &format!("{} timers", s.engine.timers.len()),
                p.blue,
                p.blue_bg,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, "+ Add timer", p, 1).clicked() {
                    s.add_timer();
                }
            });
        });
        ui.add_space(10.);
        let mut delete = None;
        let now = s.now;
        let cfg = &s.engine.cfg;
        for row in s.engine.timers.chunks_mut(2) {
            ui.columns(2, |cols| {
                for (ui, t) in cols.iter_mut().zip(row.iter_mut()) {
                    card(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut t.name)
                                    .font(egui::TextStyle::Heading)
                                    .desired_width((ui.available_width() - 85.).max(100.))
                                    .char_limit(48)
                                    .hint_text("Timer name")
                                    .frame(false),
                            );
                            badge(
                                ui,
                                &keys::key_display(cfg.hotkey(&t.hotkey_id)),
                                p.blue,
                                p.blue_bg,
                            );
                        });
                        ui.add_space(5.);
                        ui.label(
                            RichText::new(t.time_text(now))
                                .size(43.)
                                .strong()
                                .color(if t.done { p.green } else { p.text }),
                        );
                        ui.horizontal(|ui| {
                            badge(
                                ui,
                                &t.status_text(),
                                if t.state == TimerState::Running || t.done {
                                    p.green
                                } else {
                                    p.muted
                                },
                                if t.state == TimerState::Running || t.done {
                                    p.green_bg
                                } else {
                                    p.soft
                                },
                            );
                            muted(
                                ui,
                                if t.duration_s == 0 {
                                    "Stopwatch".into()
                                } else {
                                    format!("{}s countdown", t.duration_s)
                                },
                                p,
                            );
                        });
                        ui.add_space(12.);
                        ui.horizontal(|ui| {
                            let label = match t.state {
                                TimerState::Idle => "Start",
                                TimerState::Running => "Pause",
                                TimerState::Paused => "Reset",
                            };
                            if button(ui, label, p, 1).clicked() {
                                actions.push(t.hotkey_id.clone());
                            }
                            if button(ui, if t.detached { "Dock" } else { "Detach" }, p, 0)
                                .clicked()
                            {
                                t.detached = !t.detached;
                            }
                            if t.custom && button(ui, "Delete", p, 2).clicked() {
                                delete = Some(t.id.clone());
                            }
                        });
                    });
                }
            });
            ui.add_space(8.);
        }
        if let Some(id) = delete {
            s.delete_timer(&id);
        }
    }
    fn auto_ts(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        status_bar(
            ui,
            "Auto TS",
            s.engine.running,
            if s.engine.running {
                format!(
                    "Next in {:.1}s",
                    (s.engine
                        .cfg
                        .effective_interval_ms()
                        .saturating_sub(s.now.saturating_sub(s.engine.last_tick)))
                        as f64
                        / 1000.
                )
            } else {
                "OFF".into()
            },
            p,
            "auto_ts",
            actions,
        );
        ui.add_space(12.);
        ui.columns(2, |cols| {
            card(p).show(&mut cols[0], |ui| {
                heading(ui, "Trigger settings", p);
                ui.add_space(10.);
                muted(ui, "Click mode", p);
                egui::ComboBox::from_id_salt("ts_mode")
                    .width(ui.available_width())
                    .selected_text(s.engine.cfg.click_mode.label())
                    .show_ui(ui, |ui| {
                        for mode in ClickMode::all() {
                            ui.selectable_value(&mut s.engine.cfg.click_mode, mode, mode.label());
                        }
                    });
                ui.add_space(8.);
                field_f64(
                    ui,
                    "Interval (seconds)",
                    &mut s.engine.cfg.interval_s,
                    0.05..=36000.,
                    p,
                );
                speed_controls(ui, s, p);
                ui.add_space(8.);
                badge(
                    ui,
                    &format!(
                        "Effective interval  {:.3}s",
                        s.engine.cfg.effective_interval_s()
                    ),
                    p.blue,
                    p.blue_bg,
                );
            });
            card(p).show(&mut cols[1], |ui| {
                heading(ui, "Points & placement", p);
                ui.add_space(10.);
                point_row(
                    ui,
                    "Auto TS point",
                    s.engine.cfg.point,
                    "capture_ts",
                    p,
                    actions,
                );
                ui.add_space(8.);
                field_u32(
                    ui,
                    "Clicks per trigger",
                    &mut s.engine.cfg.point_clicks,
                    1..=80,
                    p,
                );
                if s.engine.cfg.click_mode == ClickMode::Placement {
                    ui.separator();
                    point_row(
                        ui,
                        "Placement point",
                        s.engine.cfg.place_point,
                        "capture_place",
                        p,
                        actions,
                    );
                    field_u32(ui, "Slot", &mut s.engine.cfg.place_slot, 1..=9, p);
                    muted(ui, "Step delay (ms)", p);
                    ui.add(
                        egui::DragValue::new(&mut s.engine.cfg.place_gap_ms)
                            .range(500..=5000)
                            .speed(50.),
                    );
                    ui.add_space(8.);
                    muted(ui, "Slot → place twice → E ×3 → Auto TS → X", p);
                } else {
                    ui.add_space(12.);
                    muted(
                        ui,
                        "Saved-point mode returns the cursor to its previous location.",
                        p,
                    );
                }
            });
        });
    }
    fn katakuri(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        let active = !s.engine.kat_running.is_empty();
        status_bar(
            ui,
            "Katakuri",
            active,
            format!(
                "{} selected  ·  {} running",
                s.engine.cfg.kat_picked.len(),
                s.engine.kat_running.len()
            ),
            p,
            "kat_run",
            actions,
        );
        ui.add_space(12.);
        for row in KAT_COUNTS.chunks(3) {
            ui.columns(3, |cols| {
                for (ui, n) in cols.iter_mut().zip(row) {
                    card(p).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let running = s.engine.kat_running.contains(n);
                        let mut selected = s.engine.cfg.kat_picked.contains(n);
                        ui.horizontal(|ui| {
                            heading(ui, &format!("{n} Katakuris"), p);
                            if toggle(ui, &mut selected, p).clicked() {
                                s.engine.kat_pick(*n);
                            }
                        });
                        ui.add_space(12.);
                        badge(
                            ui,
                            if running { "ON" } else { "OFF" },
                            if running { p.green } else { p.muted },
                            if running { p.green_bg } else { p.soft },
                        );
                        ui.add_space(8.);
                        ui.label(
                            RichText::new(
                                s.engine
                                    .kat_remaining(*n, s.now)
                                    .map(|ms| format!("{:.1}s", ms as f64 / 1000.))
                                    .unwrap_or_else(|| "—".into()),
                            )
                            .size(32.)
                            .strong(),
                        );
                        muted(ui, "Next action", p);
                        ui.separator();
                        muted(
                            ui,
                            format!(
                                "Interval  {:.3} seconds",
                                kat_interval_ms(*n) as f64 / 1000.
                            ),
                            p,
                        );
                        ui.add_space(6.);
                        if button(
                            ui,
                            if running { "Stop" } else { "Start this count" },
                            p,
                            if running { 2 } else { 0 },
                        )
                        .clicked()
                        {
                            actions.push(format!("kat{n}"));
                        }
                    });
                }
            });
            ui.add_space(8.);
        }
    }
    fn auto_actions(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        ui.columns(2, |cols| {
            card(p).show(&mut cols[0], |ui| {
                heading(ui, "Custom auto click", p);
                ui.add_space(8.);
                status_bar(
                    ui,
                    "Auto click",
                    s.engine.cac_on,
                    if s.engine.cac_on {
                        format!("Next {:.1}s", s.engine.cac_remaining(s.now) as f64 / 1000.)
                    } else {
                        "Stopped".into()
                    },
                    p,
                    "cac",
                    actions,
                );
                ui.add_space(8.);
                field_f64(
                    ui,
                    "Interval (seconds)",
                    &mut s.engine.cfg.cac_interval_s,
                    0.01..=3600.,
                    p,
                );
                field_u32(
                    ui,
                    "Clicks per trigger",
                    &mut s.engine.cfg.cac_clicks,
                    1..=100,
                    p,
                );
            });
            card(p).show(&mut cols[1], |ui| {
                heading(ui, "Auto key press", p);
                ui.add_space(8.);
                status_bar(
                    ui,
                    "Key press",
                    s.engine.akp_on,
                    if s.engine.akp_on {
                        format!("Next {:.1}s", s.engine.akp_remaining(s.now) as f64 / 1000.)
                    } else {
                        "Stopped".into()
                    },
                    p,
                    "akp",
                    actions,
                );
                muted(ui, "Keys in order", p);
                ui.add(
                    egui::TextEdit::singleline(&mut s.engine.cfg.akp_keys)
                        .hint_text("e, q, r or space, 1, 2")
                        .desired_width(f32::INFINITY),
                );
                field_f64(
                    ui,
                    "Interval (seconds)",
                    &mut s.engine.cfg.akp_interval_s,
                    0.01..=3600.,
                    p,
                );
                field_f64(
                    ui,
                    "Delay between keys (seconds)",
                    &mut s.engine.cfg.akp_key_gap_s,
                    0.0..=30.,
                    p,
                );
            });
        });
        ui.add_space(12.);
        ui.columns(2, |cols| {
            card(p).show(&mut cols[0], |ui| {
                ui.horizontal(|ui| {
                    heading(ui, "Hold E to repeat", p);
                    toggle(ui, &mut s.pref.hold_on, p);
                });
                ui.add_space(8.);
                muted(ui, "Hold delay (ms)", p);
                ui.add(
                    egui::DragValue::new(&mut s.pref.hold_delay)
                        .range(200..=3000)
                        .speed(50.),
                );
                muted(ui, "Repeat every (ms)", p);
                ui.add(
                    egui::DragValue::new(&mut s.pref.hold_rate)
                        .range(20..=500)
                        .speed(10.),
                );
            });
            card(p).show(&mut cols[1], |ui| {
                heading(ui, "Auto upgrade", p);
                ui.add_space(12.);
                muted(ui, "Homura · E three times", p);
                if button(ui, "Upgrade Homura", p, 3).clicked() {
                    actions.push("homura_upgrade".into());
                }
                ui.add_space(10.);
                muted(ui, "Kurumi · E four times", p);
                if button(ui, "Upgrade Kurumi", p, 3).clicked() {
                    actions.push("kurumi_upgrade".into());
                }
            });
        });
    }
    fn record(
        &mut self,
        ui: &mut egui::Ui,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        let recording = s.recorder.is_some();
        let status = if recording {
            "RECORDING"
        } else if s.playing {
            "PLAYING"
        } else {
            "IDLE"
        };
        Frame::new()
            .fill(if recording {
                p.red_bg
            } else if s.playing {
                p.green_bg
            } else {
                p.panel
            })
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(12)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    badge(
                        ui,
                        status,
                        if recording {
                            p.red
                        } else if s.playing {
                            p.green
                        } else {
                            p.muted
                        },
                        p.panel,
                    );
                    let detail = if let Some(r) = &s.recorder {
                        format!(
                            "{}  ·  {} events",
                            crate::core::timers::fmt_mmss(s.now.saturating_sub(r.started) / 1000),
                            r.count
                        )
                    } else if s.playing {
                        format!(
                            "Pass {} / {}",
                            s.play_pass,
                            if s.play_total == 0 {
                                "∞".into()
                            } else {
                                s.play_total.to_string()
                            }
                        )
                    } else {
                        "Ready to record or open a file".into()
                    };
                    muted(ui, detail, p);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(
                            ui,
                            if s.playing { "Stop playback" } else { "Play" },
                            p,
                            if s.playing { 2 } else { 1 },
                        )
                        .clicked()
                        {
                            actions.push("play_toggle".into());
                        }
                        if button(
                            ui,
                            if recording {
                                "Stop recording"
                            } else {
                                "Record"
                            },
                            p,
                            2,
                        )
                        .clicked()
                        {
                            actions.push("rec_toggle".into());
                        }
                    });
                });
            });
        ui.add_space(12.);
        ui.columns(2,|cols|{card(p).show(&mut cols[0],|ui|{heading(ui,"Recording",p);ui.add_space(10.);muted(ui,"Recording name",p);ui.add_enabled(!recording&&!s.play_busy,egui::TextEdit::singleline(&mut s.engine.cfg.rec_name).desired_width(f32::INFINITY).char_limit(80));
            ui.add_space(8.);ui.horizontal(|ui|{toggle(ui,&mut s.engine.cfg.rec_moves,p);ui.label("Capture mouse movement");});muted(ui,"Turn off to replay clicks at the current cursor location.",p);muted(ui,"Mouse sample interval (ms)",p);ui.add(egui::DragValue::new(&mut s.engine.cfg.rec_sample_ms).range(8..=200).speed(1.));ui.add_space(14.);shortcut_row(ui,s,"rec_toggle","Record key",p);ui.add_space(8.);muted(ui,"Keyboard holds, modifiers, mouse buttons and wheel are captured. App controls are excluded.",p);
        });card(p).show(&mut cols[1],|ui|{heading(ui,"Playback",p);ui.add_space(10.);muted(ui,"Speed multiplier",p);ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut self.speed).desired_width((ui.available_width()-90.).max(80.)));if button(ui,"Apply",p,1).clicked(){match self.speed.trim().parse::<f64>(){Ok(v)if recording::valid_speed(v)=>{s.engine.cfg.rec_speed=v;s.message=format!("Playback speed set to {v}×");},_=>s.message="Enter a finite number greater than zero.".into()}}});muted(ui,"0.5 = half speed  ·  1 = normal  ·  2 = double",p);badge(ui,&format!("Active speed: {}×",s.engine.cfg.rec_speed),p.blue,p.blue_bg);ui.add_space(8.);field_u32(ui,"Loops (0 = repeat until stopped)",&mut s.engine.cfg.rec_loops,0..=9999,p);ui.add_space(10.);shortcut_row(ui,s,"play_toggle","Playback key",p);});});
        ui.add_space(12.);
        card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                heading(ui, "Recording file", p);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if button(ui, "Save as…", p, 0).clicked() {
                        actions.push("rec_save_as".into());
                    }
                    if button(ui, "Open…", p, 0).clicked() {
                        actions.push("rec_open".into());
                    }
                });
            });
            ui.add_space(6.);
            muted(ui, s.record_path().display().to_string(), p);
        });
    }
    fn settings(&mut self, ui: &mut egui::Ui, s: &mut Runtime, p: Palette) {
        card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, "Appearance", p);
            ui.add_space(8.);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut s.pref.dark, false, "Light");
                ui.selectable_value(&mut s.pref.dark, true, "Dark");
                ui.add_space(28.);
                toggle(ui, &mut s.pref.on_top, p);
                ui.label("Always on top");
            });
        });
        ui.add_space(12.);
        card(p).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                heading(ui, "Keybinds", p);
                toggle(ui, &mut s.engine.cfg.hotkeys_enabled, p);
                badge(
                    ui,
                    if s.engine.cfg.hotkeys_enabled {
                        "ENABLED"
                    } else {
                        "DISABLED"
                    },
                    if s.engine.cfg.hotkeys_enabled {
                        p.green
                    } else {
                        p.muted
                    },
                    p.soft,
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("keybind_group")
                        .width(180.)
                        .selected_text(&self.group)
                        .show_ui(ui, |ui| {
                            for group in [
                                "Auto TS",
                                "Katakuri",
                                "Timers",
                                "Record / Playback",
                                "Auto actions",
                                "Panel / System",
                            ] {
                                ui.selectable_value(&mut self.group, group.to_owned(), group);
                            }
                        });
                });
            });
            ui.add_space(8.);
            muted(
                ui,
                "Press Set, then a key; or select one from Choose key. Esc cancels.",
                p,
            );
            ui.add_space(8.);
            for (id, name, group) in model::actions(&s.engine.timers) {
                if group != self.group {
                    continue;
                }
                Frame::new()
                    .fill(p.soft)
                    .corner_radius(10)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).size(14.));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if button(ui, "Clear", p, 2).clicked() {
                                    s.engine.cfg.set_hotkey(&id, "");
                                }
                                if button(ui, "Set", p, 0).clicked() {
                                    s.bind(id.clone());
                                }
                                binding_picker(ui, s, &id);
                            });
                        });
                    });
            }
        });
    }
    fn detached(&self, ctx: &egui::Context) {
        let timers = {
            let s = self.shared.lock().unwrap();
            s.engine
                .timers
                .iter()
                .filter(|t| t.detached)
                .cloned()
                .collect::<Vec<_>>()
        };
        for timer in timers {
            let shared = self.shared.clone();
            let id = timer.id.clone();
            let viewport = ViewportId::from_hash_of(("timer", &id));
            let mut builder = ViewportBuilder::default()
                .with_title(format!("{} · PhakinHub", timer.name))
                .with_inner_size([310., 245.])
                .with_min_inner_size([300., 220.])
                .with_always_on_top();
            if let Some(pos) = timer.position {
                builder = builder.with_position(native::visible_position(
                    pos,
                    [310., 245.],
                    ctx.pixels_per_point(),
                ));
            }
            ctx.show_viewport_deferred(viewport, builder, move |ctx, _| {
                let mut actions = Vec::new();
                let mut remove = false;
                {
                    let mut s = shared.lock().unwrap();
                    let p = Palette::new(s.pref.dark);
                    let now = s.now;
                    let before = model::serialize(&s.engine.cfg, &s.engine.timers, &s.pref);
                    if let Some(t) = s.engine.timers.iter_mut().find(|t| t.id == id) {
                        if ctx.input(|i| i.viewport().close_requested()) {
                            t.detached = false;
                            s.dirty = true;
                            return;
                        }
                        if let Some(rect) = ctx.input(|i| i.viewport().outer_rect) {
                            let position = [rect.left(), rect.top()];
                            if t.position != Some(position) {
                                t.position = Some(position);
                            }
                        }
                        egui::CentralPanel::default()
                            .frame(Frame::new().fill(p.panel).inner_margin(18))
                            .show(ctx, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut t.name)
                                        .frame(false)
                                        .font(egui::TextStyle::Heading)
                                        .char_limit(48)
                                        .desired_width(f32::INFINITY),
                                );
                                ui.add_space(8.);
                                ui.label(
                                    RichText::new(t.time_text(now))
                                        .size(46.)
                                        .strong()
                                        .color(if t.done { p.green } else { p.text }),
                                );
                                muted(ui, t.status_text(), p);
                                ui.add_space(12.);
                                ui.horizontal(|ui| {
                                    if button(
                                        ui,
                                        match t.state {
                                            TimerState::Idle => "Start",
                                            TimerState::Running => "Pause",
                                            TimerState::Paused => "Reset",
                                        },
                                        p,
                                        1,
                                    )
                                    .clicked()
                                    {
                                        actions.push(t.hotkey_id.clone());
                                    }
                                    if button(ui, "Dock", p, 0).clicked() {
                                        t.detached = false;
                                    }
                                    if t.custom && button(ui, "Delete", p, 2).clicked() {
                                        remove = true;
                                    }
                                });
                            });
                        if before != model::serialize(&s.engine.cfg, &s.engine.timers, &s.pref) {
                            s.dirty = true;
                        }
                    } else {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    if remove {
                        s.delete_timer(&id);
                    }
                }
                for action in actions {
                    runtime::dispatch(&shared, &action);
                }
                if ctx.input(|i| i.focused) {
                    native::EDITING.store(
                        ctx.wants_keyboard_input(),
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
                ctx.request_repaint_after(Duration::from_millis(100));
            });
        }
    }
    fn change_mode(&mut self, ctx: &egui::Context, mode: Mode, on_top: bool) {
        if self.applied_mode != mode {
            if self.applied_mode == Mode::Full {
                self.normal_position = ctx.input(|i| i.viewport().outer_rect.map(|r| r.min));
                self.normal_size = ctx.input(|i| i.content_rect().size());
            }
            match mode {
                Mode::Full => {
                    ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
                    ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(vec2(980., 600.)));
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.normal_size));
                    if let Some(p) = self.normal_position {
                        ctx.send_viewport_cmd(ViewportCommand::OuterPosition(p));
                    }
                }
                Mode::Mini => {
                    ctx.send_viewport_cmd(ViewportCommand::Decorations(true));
                    ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(vec2(300., 350.)));
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(360., 620.)));
                }
                Mode::Cockpit => {
                    let [x, y, _, h] = native::screen_work_area();
                    let scale = ctx.pixels_per_point();
                    ctx.send_viewport_cmd(ViewportCommand::Decorations(false));
                    ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(vec2(150., 300.)));
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(
                        210. / scale,
                        (h - 90) as f32 / scale,
                    )));
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(pos2(
                        x as f32 / scale,
                        (y + 44) as f32 / scale,
                    )));
                }
            }
            self.applied_mode = mode;
        }
        let top = on_top || mode != Mode::Full;
        if self.applied_top != top {
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(if top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            }));
            self.applied_top = top;
        }
    }
    fn compact(
        &mut self,
        ctx: &egui::Context,
        s: &mut Runtime,
        p: Palette,
        actions: &mut Vec<String>,
    ) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg).inner_margin(12))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if button(ui, "Expand panel", p, 1).clicked() {
                        s.mode = Mode::Full;
                    }
                    ui.separator();
                    status_bar(
                        ui,
                        "Auto TS",
                        s.engine.running,
                        format!(
                            "Every {:.2}s · Next {}",
                            s.engine.cfg.effective_interval_s(),
                            if s.engine.running {
                                format!(
                                    "{:.1}s",
                                    s.engine
                                        .cfg
                                        .effective_interval_ms()
                                        .saturating_sub(s.now.saturating_sub(s.engine.last_tick))
                                        as f64
                                        / 1000.
                                )
                            } else {
                                "—".into()
                            }
                        ),
                        p,
                        "auto_ts",
                        actions,
                    );
                    for n in KAT_COUNTS {
                        if s.engine.cfg.kat_picked.contains(&n) {
                            ui.horizontal(|ui| {
                                ui.label(format!("{n} KAT"));
                                badge(
                                    ui,
                                    if s.engine.kat_running.contains(&n) {
                                        "ON"
                                    } else {
                                        "OFF"
                                    },
                                    p.blue,
                                    p.blue_bg,
                                );
                            });
                            muted(
                                ui,
                                format!(
                                    "Every {:.3}s  ·  Next {}",
                                    kat_interval_ms(n) as f64 / 1000.,
                                    s.engine
                                        .kat_remaining(n, s.now)
                                        .map(|n| format!("{:.1}s", n as f64 / 1000.))
                                        .unwrap_or_else(|| "—".into())
                                ),
                                p,
                            );
                        }
                    }
                    if s.mode == Mode::Mini {
                        ui.separator();
                        compact_timers(ui, s, p, actions);
                    } else {
                        ui.separator();
                        if button(ui, "Katakuri run / stop", p, 0).clicked() {
                            actions.push("kat_run".into());
                        }
                        for (id, label) in [
                            ("homura_upgrade", "Homura E ×3"),
                            ("kurumi_upgrade", "Kurumi E ×4"),
                        ] {
                            if button(ui, label, p, 3).clicked() {
                                actions.push(id.into());
                            }
                        }
                    }
                    ui.separator();
                    if button(ui, "Stop all", p, 2).clicked() {
                        actions.push("stop_all".into());
                    }
                });
            });
    }
    fn cockpit_rails(&self, ctx: &egui::Context) {
        let [x, y, w, h] = native::screen_work_area();
        let scale = ctx.pixels_per_point();
        for side in ["top", "right", "bottom"] {
            let shared = self.shared.clone();
            let (px, py, pw, ph) = match side {
                "top" => (x, y, w, 44),
                "right" => (x + w - 210, y + 44, 210, h - 90),
                _ => (x, y + h - 46, w, 46),
            };
            let builder = ViewportBuilder::default()
                .with_title(format!("PhakinHub {side}"))
                .with_decorations(false)
                .with_always_on_top()
                .with_resizable(false)
                .with_position([px as f32 / scale, py as f32 / scale])
                .with_inner_size([pw as f32 / scale, ph as f32 / scale]);
            ctx.show_viewport_deferred(
                ViewportId::from_hash_of(("cockpit", side)),
                builder,
                move |ctx, _| {
                    let mut actions = Vec::new();
                    {
                        let mut s = shared.lock().unwrap();
                        let p = Palette::new(s.pref.dark);
                        if ctx.input(|i| i.viewport().close_requested()) {
                            s.mode = Mode::Full;
                            return;
                        }
                        egui::CentralPanel::default()
                            .frame(Frame::new().fill(p.panel).inner_margin(8))
                            .show(ctx, |ui| match side {
                                "top" => {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new("PHAKIN HUB  /  COCKPIT").strong());
                                        if button(ui, "Fit game", p, 0).clicked() {
                                            actions.push("fit_game".into());
                                        }
                                        if button(ui, "Exit cockpit", p, 0).clicked() {
                                            s.mode = Mode::Full;
                                        }
                                    });
                                }
                                "right" => {
                                    egui::ScrollArea::vertical().show(ui, |ui| {
                                        compact_timers(ui, &mut s, p, &mut actions);
                                    });
                                }
                                _ => {
                                    ui.horizontal(|ui| {
                                        ui.label(format!(
                                            "Auto click: {}",
                                            if s.engine.cac_on { "ON" } else { "OFF" }
                                        ));
                                        ui.separator();
                                        ui.label(format!(
                                            "Key press: {}",
                                            if s.engine.akp_on { "ON" } else { "OFF" }
                                        ));
                                        ui.separator();
                                        ui.label(&s.message);
                                    });
                                }
                            });
                    }
                    for action in actions {
                        runtime::dispatch(&shared, &action);
                    }
                    ctx.request_repaint_after(Duration::from_millis(100));
                },
            );
        }
    }
}
impl eframe::App for Hub {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let shared = self.shared.clone();
        let mut actions = Vec::new();
        let (dark, mode, top) = {
            let s = shared.lock().unwrap();
            (s.pref.dark, s.mode, s.pref.on_top)
        };
        self.style(ctx, dark);
        self.change_mode(ctx, mode, top);
        let p = Palette::new(dark);
        {
            let mut s = shared.lock().unwrap();
            let before = model::serialize(&s.engine.cfg, &s.engine.timers, &s.pref);
            // Window input is a second capture path when a driver or another
            // application prevents the global hook from delivering the key.
            if s.rebind.is_some() {
                let key = ctx.input_mut(|i| take_binding_key(&mut i.events));
                if let Some(vk) = key {
                    s.capture_binding(vk);
                }
            }
            if s.exit {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            if mode == Mode::Full {
                self.sidebar(ctx, &mut s, p, &mut actions);
                egui::TopBottomPanel::bottom("status_bar")
                    .frame(
                        Frame::new()
                            .fill(p.panel)
                            .inner_margin(Margin::symmetric(22, 9)),
                    )
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&s.message).size(11.5).color(p.muted));
                        });
                    });
                egui::CentralPanel::default()
                    .frame(Frame::new().fill(p.bg).inner_margin(26))
                    .show(ctx, |ui| {
                        self.toolbar(ui, &mut s, p, &mut actions);
                        if s.rebind.is_some() || s.capture.is_some() {
                            Frame::new()
                                .fill(p.amber_bg)
                                .corner_radius(10)
                                .inner_margin(12)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.spinner();
                                        if let Some((action, end)) = &s.rebind {
                                            let label = model::actions(&s.engine.timers)
                                                .into_iter()
                                                .find(|(id, _, _)| id == action)
                                                .map(|(_, name, _)| name)
                                                .unwrap_or_else(|| action.clone());
                                            ui.label(format!(
                                                "{label}: press a key or mouse side button ({}s)",
                                                end.saturating_sub(s.now).div_ceil(1000)
                                            ));
                                        } else {
                                            ui.label(
                                                "Click the point outside this app. Esc cancels.",
                                            );
                                        }
                                        if button(ui, "Cancel", p, 0).clicked() {
                                            s.cancel_capture();
                                        }
                                    });
                                });
                            ui.add_space(10.);
                        }
                        egui::ScrollArea::vertical()
                            .id_salt(self.page.title())
                            .auto_shrink([false, false])
                            .show(ui, |ui| match self.page {
                                Page::Tools => self.overview(ui, p),
                                Page::Timers => self.timers(ui, &mut s, p, &mut actions),
                                Page::AutoTs => self.auto_ts(ui, &mut s, p, &mut actions),
                                Page::Katakuri => self.katakuri(ui, &mut s, p, &mut actions),
                                Page::Auto => self.auto_actions(ui, &mut s, p, &mut actions),
                                Page::Record => self.record(ui, &mut s, p, &mut actions),
                                Page::Settings => self.settings(ui, &mut s, p),
                            });
                    });
            } else {
                self.compact(ctx, &mut s, p, &mut actions);
            }
            if before != model::serialize(&s.engine.cfg, &s.engine.timers, &s.pref) {
                s.dirty = true;
            }
        }
        if ctx.input(|i| i.focused) {
            native::EDITING.store(
                ctx.wants_keyboard_input(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        for action in actions {
            runtime::dispatch(&shared, &action);
        }
        self.detached(ctx);
        if mode == Mode::Cockpit {
            self.cockpit_rails(ctx);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}
fn speed_controls(ui: &mut egui::Ui, s: &mut Runtime, p: Palette) {
    muted(ui, "Game speed", p);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut s.pref.detection.speed_auto, false, "Manual");
        ui.selectable_value(&mut s.pref.detection.speed_auto, true, "Automatic");
    });
    ui.add_enabled_ui(!s.pref.detection.speed_auto, |ui| {
        ui.horizontal(|ui| {
            for v in [1, 2, 3] {
                ui.selectable_value(&mut s.engine.cfg.ts_mode, v, format!("x{v}"));
            }
        });
    });
    if s.pref.detection.speed_auto {
        let label = match s.detected_speed {
            Some(speed) => format!("HUD: x{speed} · Active: x{}", s.engine.cfg.ts_mode),
            None => format!("HUD unclear · Keeping x{}", s.engine.cfg.ts_mode),
        };
        ui.label(label);
        muted(ui, &s.detection_message, p);
        muted(
            ui,
            "Reads the bottom-left GAMESPEED color while Roblox is in the foreground.",
            p,
        );
        detection_region_controls(
            ui,
            &mut s.pref.detection.speed_region,
            crate::detection::Settings::default().speed_region,
            "Speed detection area",
            p,
        );
    }
}
fn detection_region_controls(
    ui: &mut egui::Ui,
    region: &mut crate::detection::Region,
    defaults: crate::detection::Region,
    title: &str,
    p: Palette,
) {
    ui.collapsing(title, |ui| {
        muted(
            ui,
            "Percent of the Roblox client area. Adjust if your HUD layout differs.",
            p,
        );
        let mut fields = [
            region.x * 100.,
            region.y * 100.,
            region.w * 100.,
            region.h * 100.,
        ];
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            for (label, field) in ["Left", "Top", "Width", "Height"]
                .into_iter()
                .zip(fields.iter_mut())
            {
                ui.label(label);
                changed |= ui
                    .add(
                        egui::DragValue::new(field)
                            .range(0.0..=100.0)
                            .speed(0.1)
                            .suffix("%")
                            .min_decimals(1)
                            .max_decimals(2),
                    )
                    .changed();
            }
        });
        if changed {
            region.x = (fields[0] / 100.).clamp(0., 0.99);
            region.y = (fields[1] / 100.).clamp(0., 0.99);
            region.w = (fields[2] / 100.).clamp(0.01, (1. - region.x).max(0.01));
            region.h = (fields[3] / 100.).clamp(0.01, (1. - region.y).max(0.01));
        }
        if button(ui, "Restore area", p, 0).clicked() {
            *region = defaults;
        }
    });
}
fn field_f64(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    p: Palette,
) {
    muted(ui, label, p);
    ui.add(
        egui::DragValue::new(value)
            .range(range)
            .speed(0.05)
            .max_decimals(3),
    );
    ui.add_space(4.);
}
fn field_u32(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    p: Palette,
) {
    muted(ui, label, p);
    ui.add(egui::DragValue::new(value).range(range).speed(1.));
    ui.add_space(4.);
}
fn point_row(
    ui: &mut egui::Ui,
    label: &str,
    point: Option<(i32, i32)>,
    action: &str,
    p: Palette,
    actions: &mut Vec<String>,
) {
    muted(ui, label, p);
    ui.horizontal(|ui| {
        ui.label(
            point
                .map(|(x, y)| format!("{x}, {y}"))
                .unwrap_or_else(|| "No point selected".into()),
        );
        if button(ui, "Pick point", p, 0).clicked() {
            actions.push(action.into());
        }
    });
}
fn status_bar(
    ui: &mut egui::Ui,
    label: &str,
    on: bool,
    detail: String,
    p: Palette,
    action: &str,
    actions: &mut Vec<String>,
) {
    Frame::new()
        .fill(if on { p.green_bg } else { p.soft })
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                badge(
                    ui,
                    if on { "ON" } else { "OFF" },
                    if on { p.green } else { p.muted },
                    p.panel,
                );
                ui.label(RichText::new(label).strong());
                muted(ui, detail, p);
                if button(
                    ui,
                    if on { "Stop" } else { "Start" },
                    p,
                    if on { 2 } else { 1 },
                )
                .clicked()
                {
                    actions.push(action.into());
                }
            });
        });
}
fn shortcut_row(ui: &mut egui::Ui, s: &mut Runtime, id: &str, label: &str, p: Palette) {
    muted(ui, label, p);
    ui.horizontal(|ui| {
        binding_picker(ui, s, id);
        if button(ui, "Set key", p, 0).clicked() {
            s.bind(id.into());
        }
    });
}

fn binding_event(event: &egui::Event) -> Option<u16> {
    match event {
        egui::Event::Key {
            key,
            physical_key,
            pressed: true,
            repeat: false,
            ..
        } => keys::vk_from_name(physical_key.unwrap_or(*key).name()),
        egui::Event::PointerButton {
            button,
            pressed: true,
            ..
        } => match button {
            egui::PointerButton::Middle => Some(4),
            egui::PointerButton::Extra1 => Some(5),
            egui::PointerButton::Extra2 => Some(6),
            _ => None,
        },
        _ => None,
    }
}
fn take_binding_key(events: &mut Vec<egui::Event>) -> Option<u16> {
    let key = events.iter().find_map(binding_event);
    if key.is_some() {
        events.retain(|e| {
            !matches!(
                e,
                egui::Event::Key { .. }
                    | egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Copy
                    | egui::Event::Cut
            ) && !matches!(
                e,
                egui::Event::PointerButton {
                    button: egui::PointerButton::Middle
                        | egui::PointerButton::Extra1
                        | egui::PointerButton::Extra2,
                    ..
                }
            )
        });
    }
    key
}
fn binding_picker(ui: &mut egui::Ui, s: &mut Runtime, id: &str) {
    let current = s.engine.cfg.hotkey(id);
    let title = if current.is_empty() {
        "Choose key…".into()
    } else {
        keys::key_display(current)
    };
    ui.add_enabled_ui(s.recorder.is_none() && !s.play_busy, |ui| {
        egui::ComboBox::from_id_salt(("bind_picker", id))
            .width(130.)
            .height(250.)
            .selected_text(title)
            .show_ui(ui, |ui| {
                let mut group = "";
                for (category, vk) in keys::choices() {
                    if category != group {
                        if !group.is_empty() {
                            ui.separator();
                        }
                        ui.weak(category);
                        group = category;
                    }
                    let name = keys::name_from_vk(vk).unwrap();
                    let selected = keys::vk_from_name(s.engine.cfg.hotkey(id)) == Some(vk);
                    if ui
                        .selectable_label(selected, keys::key_display(&name))
                        .clicked()
                    {
                        s.set_binding(id, vk);
                    }
                }
            });
    });
}
fn compact_timers(ui: &mut egui::Ui, s: &mut Runtime, p: Palette, actions: &mut Vec<String>) {
    let now = s.now;
    for t in s.engine.timers.iter_mut().filter(|t| !t.detached) {
        Frame::new()
            .fill(p.panel)
            .corner_radius(10)
            .stroke(Stroke::new(1.0_f32, p.border))
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&t.name).strong());
                ui.label(
                    RichText::new(t.time_text(now))
                        .size(25.)
                        .strong()
                        .color(if t.done { p.green } else { p.text }),
                );
                ui.horizontal_wrapped(|ui| {
                    if button(
                        ui,
                        match t.state {
                            TimerState::Idle => "Start",
                            TimerState::Running => "Pause",
                            TimerState::Paused => "Reset",
                        },
                        p,
                        0,
                    )
                    .clicked()
                    {
                        actions.push(t.hotkey_id.clone());
                    }
                    if button(ui, "Detach", p, 0).clicked() {
                        t.detached = true;
                        s.dirty = true;
                    }
                });
            });
    }
}

#[cfg(test)]
mod keybind_tests {
    use super::*;
    fn key(k: egui::Key) -> egui::Event {
        egui::Event::Key {
            key: k,
            physical_key: Some(k),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn window_capture_accepts_keyboard_and_both_side_buttons() {
        assert_eq!(binding_event(&key(egui::Key::F9)), Some(0x78));
        for (button, vk) in [
            (egui::PointerButton::Extra1, 5),
            (egui::PointerButton::Extra2, 6),
        ] {
            assert_eq!(
                binding_event(&egui::Event::PointerButton {
                    pos: pos2(0., 0.),
                    button,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE
                }),
                Some(vk)
            );
        }
    }
    #[test]
    fn captured_enter_and_space_cannot_activate_set_again() {
        for k in [egui::Key::Enter, egui::Key::Space] {
            let mut events = vec![
                key(k),
                egui::Event::Text(" ".into()),
                egui::Event::PointerMoved(pos2(10., 10.)),
            ];
            assert!(take_binding_key(&mut events).is_some());
            assert_eq!(events.len(), 1);
            assert!(matches!(events[0], egui::Event::PointerMoved(_)));
        }
    }
}
