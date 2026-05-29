//! niutero — the desktop GUI (Phase 2).
//!
//! A pure-Rust egui/eframe app (wgpu/GPU backend) that drives `niutero-engine`
//! directly. It is a thin client over the same operations the CLI exposes — it
//! can do nothing the engine can't. Cross-platform (Windows/macOS/Linux).

// Hide the console window on Windows release builds (it's a GUI app).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod theme;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([880.0, 580.0])
            .with_title("Niutero")
            // Frameless: the design has its own titlebar (traffic lights, view
            // switcher, theme toggle). Transparent for clean rounded corners.
            .with_decorations(false)
            .with_transparent(true),
        ..Default::default()
    };
    eframe::run_native(
        "Niutero",
        native_options,
        Box::new(|cc| Ok(Box::new(app::NiuteroApp::new(cc)))),
    )
}
