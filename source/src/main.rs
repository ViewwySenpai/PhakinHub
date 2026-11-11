#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod core;
mod detection;
mod model;
mod native;
mod recording;
mod runtime;
mod screen;
mod ui;
fn main() -> eframe::Result<()> {
    let mut options = eframe::NativeOptions::default();
    let icon = image::load_from_memory(include_bytes!("../assets/logo.png"))
        .ok()
        .map(|i| {
            let i = i.to_rgba8();
            eframe::egui::IconData {
                width: i.width(),
                height: i.height(),
                rgba: i.into_raw(),
            }
        });
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("PhakinHub — Rust")
        .with_inner_size([1120., 760.])
        .with_min_inner_size([980., 600.]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    options.viewport = viewport;
    options.centered = true;
    eframe::run_native(
        "PhakinHub",
        options,
        Box::new(|cc| Ok(Box::new(ui::Hub::new(cc)))),
    )
}
