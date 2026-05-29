//! The app shell: a frameless window with the design's custom titlebar, the
//! tool rail (Library / Normalize / AI / Settings + Sync), the read-only status
//! bar, and the active tool body. Faithful to spec §3.
//!
//! State is held here; the tool bodies (filled in across waves G2–G5) read it.
//! The engine is called directly — this is a thin client over `niutero-engine`.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};
use niutero_engine::{self as engine, EntryView, Vault};

use crate::theme::{self, Theme};

/// The four tools in the left rail (spec §1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Library,
    Normalize,
    Ai,
    Settings,
}

/// The three Library layouts (spec §4), switched from the titlebar.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LibView {
    Classic,
    Reader,
    Board,
}

/// The open library plus its loaded entries.
struct Library {
    vault: Vault,
    entries: Vec<EntryView>,
}

impl Library {
    fn load(path: &std::path::Path) -> Result<Library, String> {
        let vault = engine::open(path)?;
        engine::record_open(&vault.root);
        let entries = engine::list(&vault, engine::Filter::All)?;
        Ok(Library { vault, entries })
    }
}

pub struct NiuteroApp {
    dark: bool,
    tool: Tool,
    lib_view: LibView,
    library: Option<Library>,
    /// Set when opening a library fails, shown in the empty state.
    open_error: Option<String>,
}

impl NiuteroApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Fonts must be bound before the first frame uses the custom serif/mono
        // families: `set_fonts` only takes effect on the *next* frame, so doing
        // it here (not in `update`) avoids a "family not bound" panic on frame 1.
        theme::install_fonts(&cc.egui_ctx);
        // Boot a library: an explicit path arg wins, else the most-recently
        // opened vault from the machine-local registry.
        let path = std::env::args().nth(1).map(PathBuf::from).or_else(|| {
            engine::recent_vaults()
                .ok()
                .and_then(|v| v.into_iter().next().map(|r| r.path))
        });
        let (library, open_error) = match path {
            Some(p) => match Library::load(&p) {
                Ok(lib) => (Some(lib), None),
                Err(e) => (None, Some(e)),
            },
            None => (None, None),
        };
        NiuteroApp {
            dark: false,
            tool: Tool::Library,
            lib_view: LibView::Classic,
            library,
            open_error,
        }
    }

    fn lib_name(&self) -> String {
        self.library
            .as_ref()
            .map(|l| l.vault.config.name.clone())
            .unwrap_or_else(|| "No library".to_string())
    }

    fn entry_count(&self) -> usize {
        self.library.as_ref().map(|l| l.entries.len()).unwrap_or(0)
    }
}

impl eframe::App for NiuteroApp {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        // Transparent so the rounded frameless window corners read cleanly.
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = Theme::of(self.dark);
        theme.apply(ctx);

        // Frameless: paint our own rounded window background.
        let window = egui::Frame {
            fill: theme.bg,
            corner_radius: egui::CornerRadius::same(12),
            stroke: egui::Stroke::new(1.0, theme.border),
            ..Default::default()
        };
        egui::CentralPanel::default().frame(window).show(ctx, |ui| {
            self.title_bar(ui, &theme);
            self.status_bar_and_body(ctx, ui, &theme);
        });
    }
}

impl NiuteroApp {
    // ---- titlebar (spec §3): logo + lib name, centered view switcher, theme toggle
    fn title_bar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let bar_h = 38.0;
        let rect =
            egui::Rect::from_min_size(ui.max_rect().min, egui::vec2(ui.max_rect().width(), bar_h));
        let resp = ui.interact(
            rect,
            ui.id().with("titlebar"),
            egui::Sense::click_and_drag(),
        );
        if resp.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if resp.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        // bottom hairline
        ui.painter().hline(
            rect.x_range(),
            rect.max.y,
            egui::Stroke::new(1.0, theme.border),
        );

        ui.scope_builder(
            egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(14.0, 0.0))),
            |ui| {
                ui.horizontal_centered(|ui| {
                    self.window_controls(ui, theme);
                    ui.add_space(8.0);
                    niu_mark(ui, theme, 20.0);
                    ui.add_space(7.0);
                    ui.label(
                        RichText::new("Niutero")
                            .font(theme::serif(14.0))
                            .color(theme.text),
                    );
                    ui.label(RichText::new("—").color(theme.faint));
                    ui.label(
                        RichText::new(self.lib_name())
                            .color(theme.text_2)
                            .size(12.5),
                    );

                    // centered view switcher (Library only)
                    if matches!(self.tool, Tool::Library) {
                        let avail = ui.available_width();
                        ui.add_space((avail - 230.0).max(0.0) * 0.5);
                        self.view_switcher(ui, theme);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let icon = if self.dark { "☀" } else { "☾" };
                        if ui
                            .add(icbtn(icon, theme))
                            .on_hover_text("Toggle theme")
                            .clicked()
                        {
                            self.dark = !self.dark;
                        }
                    });
                });
            },
        );
        ui.add_space(bar_h - ui.min_rect().height().min(bar_h)); // ensure we advance past the bar
        ui.allocate_space(egui::vec2(0.0, 0.0));
        // Move the cursor below the titlebar for subsequent panels.
        ui.advance_cursor_after_rect(rect);
    }

    /// macOS-style traffic lights — functional in a frameless window.
    fn window_controls(&self, ui: &mut egui::Ui, _theme: &Theme) {
        let dot = |ui: &mut egui::Ui, color: Color32| -> egui::Response {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
            let c = if resp.hovered() {
                color
            } else {
                color.gamma_multiply(0.92)
            };
            ui.painter().circle_filled(rect.center(), 5.5, c);
            resp
        };
        if dot(ui, Color32::from_rgb(0xF0, 0x58, 0x4E))
            .on_hover_text("Close")
            .clicked()
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.add_space(4.0);
        if dot(ui, Color32::from_rgb(0xF5, 0xBC, 0x4F))
            .on_hover_text("Minimize")
            .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        ui.add_space(4.0);
        if dot(ui, Color32::from_rgb(0x5F, 0xC1, 0x59))
            .on_hover_text("Zoom")
            .clicked()
        {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
    }

    fn view_switcher(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let mut seg = |ui: &mut egui::Ui, label: &str, v: LibView| {
            let on = self.lib_view == v;
            let txt =
                RichText::new(label)
                    .size(12.5)
                    .color(if on { theme.accent } else { theme.text_2 });
            let btn = egui::Button::new(txt)
                .fill(if on {
                    theme.surface
                } else {
                    Color32::TRANSPARENT
                })
                .corner_radius(7.0);
            if ui.add(btn).clicked() {
                self.lib_view = v;
            }
        };
        egui::Frame::default()
            .fill(theme.surface_2)
            .corner_radius(9.0)
            .inner_margin(3)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    seg(ui, "Classic", LibView::Classic);
                    seg(ui, "Reader", LibView::Reader);
                    seg(ui, "Board", LibView::Board);
                });
            });
    }

    fn status_bar_and_body(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui, theme: &Theme) {
        // Status bar pinned to the bottom (spec §3).
        egui::TopBottomPanel::bottom("niu-status")
            .exact_height(26.0)
            .frame(
                egui::Frame::default()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.painter(); // ensure layout
                    let dot = ui
                        .allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover())
                        .0;
                    ui.painter().circle_filled(dot.center(), 3.5, theme.accent);
                    ui.label(
                        RichText::new("connector · 127.0.0.1:23510")
                            .font(theme::mono(11.0))
                            .color(theme.muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} entries", self.entry_count()))
                                .color(theme.muted)
                                .size(11.5),
                        );
                        ui.label(RichText::new("·").color(theme.faint));
                        ui.label(RichText::new("modified").color(theme.text_2).size(11.5));
                        ui.label(
                            RichText::new("⎇ main")
                                .font(theme::mono(11.0))
                                .color(theme.muted),
                        );
                    });
                });
            });

        // Tool rail on the left (spec §3).
        egui::SidePanel::left("niu-rail")
            .exact_width(60.0)
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin::symmetric(0, 12)),
            )
            .show_inside(ui, |ui| self.tool_rail(ui, theme));

        // Active tool body.
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(theme.bg))
            .show_inside(ui, |ui| match self.tool {
                Tool::Library => self.body_library(ui, theme),
                Tool::Normalize => placeholder(ui, theme, "Normalize", "The cleanup engine (G4)."),
                Tool::Ai => {
                    placeholder(ui, theme, "AI Assistant", "Chat across your library (G5).")
                }
                Tool::Settings => placeholder(
                    ui,
                    theme,
                    "Settings",
                    "Library, workflow, appearance, sync (G5).",
                ),
            });
    }

    fn tool_rail(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        ui.vertical_centered(|ui| {
            ui.add_space(2.0);
            niu_mark(ui, theme, 30.0);
            ui.add_space(10.0);
            for (tool, label) in [
                (Tool::Library, "Lib"),
                (Tool::Normalize, "Nrm"),
                (Tool::Ai, "AI"),
                (Tool::Settings, "Set"),
            ] {
                if rail_button(ui, theme, label, self.tool == tool).clicked() {
                    self.tool = tool;
                }
                ui.add_space(4.0);
            }
        });
        // Sync pinned to the bottom.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(2.0);
            let _ = rail_button(ui, theme, "↻", false);
        });
    }

    fn body_library(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        if self.library.is_none() {
            empty_state(ui, theme, self.open_error.as_deref());
            return;
        }
        // G1 taste of real data; G2 builds the full Classic 3-pane.
        egui::Frame::default()
            .inner_margin(egui::Margin::same(20))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} — {} entries",
                        self.lib_name(),
                        self.entry_count()
                    ))
                    .color(theme.muted)
                    .size(12.0),
                );
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let entries = &self.library.as_ref().unwrap().entries;
                    for e in entries {
                        let title = e
                            .fields
                            .get("title")
                            .map(String::as_str)
                            .unwrap_or("(untitled)");
                        let creator = e.fields.get("author").map(String::as_str).unwrap_or("");
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(title)
                                .font(theme::serif(16.0))
                                .color(theme.text),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{}  ·  {}",
                                creator,
                                e.fields.get("year").map(String::as_str).unwrap_or("")
                            ))
                            .color(theme.text_2)
                            .size(12.5),
                        );
                        ui.separator();
                    }
                });
            });
    }
}

// ---------------------------------------------------------------- helpers

/// The solid-tile logo: a white serif N on an accent squircle, nudged down ~7%
/// to optically center (caps reserve descender space) — spec §2 / `NiuMark`.
fn niu_mark(ui: &mut egui::Ui, theme: &Theme, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        egui::CornerRadius::same((size * 0.28) as u8),
        theme.accent,
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, size * 0.07),
        egui::Align2::CENTER_CENTER,
        "N",
        theme::serif(size * 0.62),
        Color32::WHITE,
    );
}

fn rail_button(ui: &mut egui::Ui, theme: &Theme, label: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(42.0, 42.0), egui::Sense::click());
    let fill = if on {
        theme.accent_tint
    } else if resp.hovered() {
        theme.surface_2
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(11), fill);
    if on {
        // 3px inset accent marker on the left.
        let m = egui::Rect::from_min_max(
            egui::pos2(rect.left() - 12.0, rect.top() + 11.0),
            egui::pos2(rect.left() - 9.0, rect.bottom() - 11.0),
        );
        ui.painter()
            .rect_filled(m, egui::CornerRadius::same(2), theme.accent);
    }
    let color = if on { theme.accent } else { theme.muted };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        egui::FontId::proportional(12.0),
        color,
    );
    resp
}

fn icbtn(glyph: &str, theme: &Theme) -> egui::Button<'static> {
    egui::Button::new(RichText::new(glyph).size(15.0).color(theme.muted))
        .fill(Color32::TRANSPARENT)
        .corner_radius(8.0)
        .min_size(egui::vec2(32.0, 32.0))
}

fn placeholder(ui: &mut egui::Ui, theme: &Theme, title: &str, sub: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.4);
        ui.label(
            RichText::new(title)
                .font(theme::serif(24.0))
                .color(theme.text),
        );
        ui.add_space(6.0);
        ui.label(RichText::new(sub).color(theme.muted));
    });
}

fn empty_state(ui: &mut egui::Ui, theme: &Theme, err: Option<&str>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.label(
            RichText::new("No library open")
                .font(theme::serif(22.0))
                .color(theme.text),
        );
        ui.add_space(6.0);
        match err {
            Some(e) => ui.label(RichText::new(e).color(theme.rose)),
            None => ui.label(
                RichText::new(
                    "Pass a vault folder:  niutero <path>   (or open one — coming in G2)",
                )
                .font(theme::mono(12.0))
                .color(theme.muted),
            ),
        };
    });
}
