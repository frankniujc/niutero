//! niutero — the desktop GUI (Phase 2).
//!
//! A pure-Rust egui/eframe app (wgpu/GPU backend) that drives `niutero-engine`
//! directly. It is a thin client over the same operations the CLI exposes — it
//! can do nothing the engine can't. Cross-platform (Windows/macOS/Linux).

// Hide the console window on Windows release builds (it's a GUI app).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ai;
mod app;
mod icons;
mod library;
mod normalize;
mod overlays;
mod settings;
mod theme;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result<()> {
    // Logging to stderr via the `log` facade (eframe/egui/winit log here too).
    // Default: warnings from deps, info from this app; tune with RUST_LOG.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,niutero=info"),
    )
    .init();
    log::info!("niutero {} starting", env!("CARGO_PKG_VERSION"));

    let native_options = eframe::NativeOptions {
        // OpenGL backend — the native graphics path that works on this
        // Windows-on-ARM hardware (wgpu's DX12/Vulkan device creation fails here).
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 820.0])
            .with_min_inner_size([880.0, 580.0])
            .with_title("Niutero")
            // Frameless: the design has its own titlebar (traffic lights, view
            // switcher, theme toggle). Opaque, square corners for now (rounded
            // frameless corners are a later polish).
            .with_decorations(false),
        ..Default::default()
    };
    eframe::run_native(
        "Niutero",
        native_options,
        Box::new(|cc| Ok(Box::new(app::NiuteroApp::new(cc)))),
    )
}
