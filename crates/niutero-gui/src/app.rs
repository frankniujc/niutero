//! The app shell: a frameless window laid out with top-level egui panels — the
//! design's custom titlebar (top), the tool rail (left), the read-only status
//! bar (bottom), and the active tool body (center). Faithful to spec §3.
//!
//! Top-level `ctx` panels (rather than nested `show_inside`) are used so egui
//! computes the central region correctly — nesting silently dropped the
//! Library's right-hand detail panel. The window is frameless with square
//! corners for now; rounded corners are a later polish.
//!
//! State lives here; tool bodies read it. The engine is called directly — this
//! is a thin client over `niutero-engine`. The Library view's engine-touching
//! requests come back as [`library::LibAction`]s that
//! [`NiuteroApp::apply_lib_action`] applies, so the read borrow and the engine
//! write never overlap.

use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};
use log::{info, warn};
use niutero_engine::{self as engine, EntryView, Vault};

use crate::icons::{self, Glyph};
use crate::library::{self, LibAction, LibState};
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
    /// Re-list entries after a mutation so the views reflect the new state.
    fn reload(&mut self) {
        match engine::list(&self.vault, engine::Filter::All) {
            Ok(e) => self.entries = e,
            Err(e) => warn!("reload entries: {e}"),
        }
    }
}

pub struct NiuteroApp {
    dark: bool,
    tool: Tool,
    lib_view: LibView,
    library: Option<Library>,
    /// Set when opening a library fails, shown in the empty state.
    open_error: Option<String>,
    /// Classic/Reader/Board view-local UI state (selection, filter, lock, …).
    lib: LibState,
    /// Transient one-line confirmation (e.g. "Copied citation"), shown briefly.
    toast: Option<String>,
}

impl NiuteroApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Fonts must be bound before the first frame uses the custom serif/mono
        // families: `set_fonts` only takes effect on the *next* frame, so doing
        // it here (not in `update`) avoids a "family not bound" panic on frame 1.
        theme::install_fonts(&cc.egui_ctx);
        // Enable the SVG image loader (egui_extras + resvg) for the icon set.
        egui_extras::install_image_loaders(&cc.egui_ctx);

        // Boot a library: an explicit path arg wins, else the most-recently
        // opened vault from the machine-local registry.
        let path = std::env::args().nth(1).map(PathBuf::from).or_else(|| {
            engine::recent_vaults()
                .ok()
                .and_then(|v| v.into_iter().next().map(|r| r.path))
        });
        let (library, open_error) = match path {
            Some(p) => match Library::load(&p) {
                Ok(lib) => {
                    info!(
                        "opened library '{}' ({} entries)",
                        lib.vault.config.name,
                        lib.entries.len()
                    );
                    (Some(lib), None)
                }
                Err(e) => {
                    warn!("open library failed: {e}");
                    (None, Some(e))
                }
            },
            None => {
                info!("no library to open (no path arg, no recent vault)");
                (None, None)
            }
        };
        NiuteroApp {
            dark: false,
            tool: Tool::Library,
            lib_view: LibView::Classic,
            library,
            open_error,
            lib: LibState::default(),
            toast: None,
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

    /// Open `path` as the active library, resetting view state. On failure the
    /// error shows in the empty state + a toast (the old library is dropped).
    fn switch_to(&mut self, path: PathBuf) {
        match Library::load(&path) {
            Ok(lib) => {
                info!(
                    "opened library '{}' ({} entries)",
                    lib.vault.config.name,
                    lib.entries.len()
                );
                self.library = Some(lib);
                self.open_error = None;
                self.lib = LibState::default();
            }
            Err(e) => {
                warn!("open library: {e}");
                self.open_error = Some(e.clone());
                self.toast = Some(e);
                self.library = None;
            }
        }
    }

    /// Apply a library pick from the titlebar menu / empty state.
    fn apply_vault_pick(&mut self, pick: VaultPick) {
        match pick {
            VaultPick::Open(p) => self.switch_to(p),
            VaultPick::New(p) => match engine::init(&p) {
                Ok(_) => self.switch_to(p),
                Err(e) => self.toast = Some(e),
            },
        }
    }
}

/// A library chosen from the switcher: open an existing vault, or create one.
enum VaultPick {
    Open(PathBuf),
    New(PathBuf),
}

/// Native folder picker (`rfd`); `None` if the user cancels.
fn pick_folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

/// The library switcher menu: recent libraries + open/new.
fn library_menu(ui: &mut egui::Ui, theme: &Theme, pick: &mut Option<VaultPick>) {
    ui.set_min_width(280.0);
    ui.label(
        RichText::new("RECENT LIBRARIES")
            .size(10.5)
            .strong()
            .color(theme.muted),
    );
    ui.add_space(2.0);
    let recents = engine::recent_vaults().unwrap_or_default();
    if recents.is_empty() {
        ui.label(RichText::new("(none yet)").color(theme.faint).size(12.0));
    }
    for rv in recents.iter().take(8) {
        let name = rv
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("(library)");
        let resp = ui
            .add(egui::Button::new(RichText::new(name).size(13.0).color(theme.text)).frame(false))
            .on_hover_text(rv.path.display().to_string());
        if resp.clicked() {
            *pick = Some(VaultPick::Open(rv.path.clone()));
        }
    }
    ui.separator();
    if ui
        .add(
            egui::Button::new(
                RichText::new("Open library…")
                    .size(13.0)
                    .color(theme.accent),
            )
            .frame(false),
        )
        .clicked()
    {
        if let Some(p) = pick_folder("Open a library folder") {
            *pick = Some(VaultPick::Open(p));
        }
    }
    if ui
        .add(
            egui::Button::new(RichText::new("New library…").size(13.0).color(theme.accent))
                .frame(false),
        )
        .clicked()
    {
        if let Some(p) = pick_folder("Choose a folder for the new library") {
            *pick = Some(VaultPick::New(p));
        }
    }
}

impl eframe::App for NiuteroApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = Theme::of(self.dark);
        theme.apply(ctx);

        // Titlebar (top), status (bottom), rail (left), tool body (center).
        egui::TopBottomPanel::top("niu-titlebar")
            .exact_height(38.0)
            .frame(egui::Frame::default().fill(theme.surface))
            .show(ctx, |ui| self.title_bar(ui, &theme));

        egui::TopBottomPanel::bottom("niu-status")
            .exact_height(26.0)
            .frame(
                egui::Frame::default()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin::symmetric(14, 0)),
            )
            .show(ctx, |ui| self.status_bar(ui, &theme));

        egui::SidePanel::left("niu-rail")
            .exact_width(60.0)
            .resizable(false)
            .frame(
                egui::Frame::default()
                    .fill(theme.surface)
                    .inner_margin(egui::Margin::symmetric(0, 12)),
            )
            .show(ctx, |ui| self.tool_rail(ui, &theme));

        match self.tool {
            Tool::Library => self.body_library(ctx, &theme),
            Tool::Normalize => {
                tool_placeholder(ctx, &theme, "Normalize", "The cleanup engine (G4).")
            }
            Tool::Ai => tool_placeholder(
                ctx,
                &theme,
                "AI Assistant",
                "Chat across your library (G5).",
            ),
            Tool::Settings => tool_placeholder(
                ctx,
                &theme,
                "Settings",
                "Library, workflow, appearance, sync (G5).",
            ),
        }

        // Transient toast (bottom-center).
        if let Some(msg) = self.toast.clone() {
            egui::Area::new("niu-toast".into())
                .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -40.0))
                .show(ctx, |ui| {
                    egui::Frame::default()
                        .fill(theme.text)
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(12, 7))
                        .show(ui, |ui| {
                            ui.label(RichText::new(msg).color(theme.bg).size(12.5));
                        });
                });
        }
    }
}

impl NiuteroApp {
    // ---- titlebar (spec §3): logo + lib name, centered view switcher, theme toggle
    fn title_bar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let rect = ui.max_rect();
        // Drag-to-move / double-click-to-maximize on the bar background. Buttons
        // added afterward take pointer priority, so they still click normally.
        let drag = ui.interact(rect, ui.id().with("drag"), egui::Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme.border),
        );

        let mut pick: Option<VaultPick> = None;
        ui.horizontal_centered(|ui| {
            ui.add_space(2.0);
            self.window_controls(ui);
            ui.add_space(8.0);
            niu_mark(ui, theme, 20.0);
            ui.add_space(7.0);
            ui.label(
                RichText::new("Niutero")
                    .font(theme::serif(14.0))
                    .color(theme.text),
            );
            ui.label(RichText::new("—").color(theme.faint));
            // Library name → menu: switch to a recent library, open a folder, or
            // create a new one.
            ui.menu_button(
                RichText::new(self.lib_name())
                    .color(theme.text_2)
                    .size(12.5),
                |ui| library_menu(ui, theme, &mut pick),
            );

            // centered view switcher (Library only)
            if matches!(self.tool, Tool::Library) {
                let avail = ui.available_width();
                ui.add_space((avail - 230.0).max(0.0) * 0.5);
                self.view_switcher(ui, theme);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let g = if self.dark { Glyph::Sun } else { Glyph::Moon };
                if icbtn(ui, theme, g).on_hover_text("Toggle theme").clicked() {
                    self.dark = !self.dark;
                    info!("theme → {}", if self.dark { "dark" } else { "light" });
                }
            });
        });
        if let Some(p) = pick {
            self.apply_vault_pick(p);
        }
    }

    /// macOS-style traffic lights — functional in a frameless window.
    fn window_controls(&self, ui: &mut egui::Ui) {
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
        egui::Frame::default()
            .fill(theme.surface_2)
            .corner_radius(9.0)
            .inner_margin(3)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, v) in [
                        ("Classic", LibView::Classic),
                        ("Reader", LibView::Reader),
                        ("Board", LibView::Board),
                    ] {
                        let on = self.lib_view == v;
                        let txt = RichText::new(label).size(12.5).color(if on {
                            theme.accent
                        } else {
                            theme.text_2
                        });
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
                    }
                });
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        ui.horizontal_centered(|ui| {
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
                    RichText::new("main")
                        .font(theme::mono(11.0))
                        .color(theme.muted),
                );
                icons::show(ui, Glyph::Branch, 13.0, theme.muted);
            });
        });
    }

    fn tool_rail(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        ui.vertical_centered(|ui| {
            ui.add_space(2.0);
            niu_mark(ui, theme, 30.0);
            ui.add_space(10.0);
            for (tool, glyph, name) in [
                (Tool::Library, Glyph::Library, "Library"),
                (Tool::Normalize, Glyph::Normalize, "Normalize"),
                (Tool::Ai, Glyph::Ai, "AI Assistant"),
                (Tool::Settings, Glyph::Settings, "Settings"),
            ] {
                if rail_button(ui, theme, glyph, self.tool == tool)
                    .on_hover_text(name)
                    .clicked()
                {
                    self.tool = tool;
                }
                ui.add_space(4.0);
            }
        });
        // Sync pinned to the bottom (commit & push — wired in a later wave).
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(2.0);
            let _ = rail_button(ui, theme, Glyph::Sync, false).on_hover_text("Sync");
        });
    }

    fn body_library(&mut self, ctx: &egui::Context, theme: &Theme) {
        if self.library.is_none() {
            let err = self.open_error.clone();
            let mut pick = None;
            egui::CentralPanel::default()
                .frame(egui::Frame::default().fill(theme.bg))
                .show(ctx, |ui| empty_state(ui, theme, err.as_deref(), &mut pick));
            if let Some(p) = pick {
                self.apply_vault_pick(p);
            }
            return;
        }
        let mut actions = Vec::new();
        match self.lib_view {
            LibView::Classic => {
                let entries = &self.library.as_ref().unwrap().entries;
                library::classic(ctx, theme, entries, &mut self.lib, &mut actions);
            }
            LibView::Reader => tool_placeholder(ctx, theme, "Reader", "Reading-first layout (G3)."),
            LibView::Board => {
                tool_placeholder(ctx, theme, "Board", "Kanban by reading status (G3).")
            }
        }
        for a in actions {
            self.apply_lib_action(a, ctx);
        }
    }

    /// Apply an engine-touching action from the Library view, then reload.
    fn apply_lib_action(&mut self, action: LibAction, ctx: &egui::Context) {
        let Some(lib) = self.library.as_mut() else {
            return;
        };
        let Some(key) = self.lib.selected.clone() else {
            return;
        };
        match action {
            LibAction::Edit(field, value) => {
                let (set, unset): (Vec<String>, Vec<String>) = if value.trim().is_empty() {
                    (vec![], vec![field.clone()])
                } else {
                    (vec![format!("{field}={value}")], vec![])
                };
                match engine::edit(&lib.vault, &key, &set, &unset, None) {
                    Ok(()) => {
                        info!("edit {key}.{field}");
                        lib.reload();
                        self.lib.refresh();
                    }
                    Err(e) => self.toast = Some(format!("Edit failed: {e}")),
                }
            }
            LibAction::SetStatus(s) => {
                if let Err(e) = engine::set_status(&mut lib.vault, &key, s) {
                    self.toast = Some(format!("Status failed: {e}"));
                } else {
                    lib.reload();
                }
            }
            LibAction::SetStars(n) => {
                if let Err(e) = engine::set_stars(&mut lib.vault, &key, n) {
                    self.toast = Some(format!("Stars failed: {e}"));
                } else {
                    lib.reload();
                }
            }
            LibAction::AddTag(t) => {
                if let Err(e) = engine::set_tags(&mut lib.vault, &key, &[t], &[]) {
                    self.toast = Some(format!("Tag failed: {e}"));
                } else {
                    lib.reload();
                }
            }
            LibAction::RemoveTag(t) => {
                if let Err(e) = engine::set_tags(&mut lib.vault, &key, &[], &[t]) {
                    self.toast = Some(format!("Untag failed: {e}"));
                } else {
                    lib.reload();
                }
            }
            LibAction::OpenUrl(u) => ctx.open_url(egui::OpenUrl::new_tab(u)),
            LibAction::Cite => match engine::cite(&lib.vault, &key) {
                Ok(s) => {
                    ctx.copy_text(s);
                    self.toast = Some("Copied citation".into());
                }
                Err(e) => self.toast = Some(e),
            },
            LibAction::Bibtex => match engine::entry_bibtex(&lib.vault, &key) {
                Ok(s) => {
                    ctx.copy_text(s);
                    self.toast = Some("Copied BibTeX".into());
                }
                Err(e) => self.toast = Some(e),
            },
        }
    }
}

// ---------------------------------------------------------------- helpers

/// A tool body that fills the central area with a centered placeholder.
fn tool_placeholder(ctx: &egui::Context, theme: &Theme, title: &str, sub: &str) {
    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(theme.bg))
        .show(ctx, |ui| placeholder(ui, theme, title, sub));
}

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

/// A 42×42 rail button painting `glyph`; accent tint + inset marker when active.
fn rail_button(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph, on: bool) -> egui::Response {
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
        let m = egui::Rect::from_min_max(
            egui::pos2(rect.left() - 9.0, rect.top() + 11.0),
            egui::pos2(rect.left() - 6.0, rect.bottom() - 11.0),
        );
        ui.painter()
            .rect_filled(m, egui::CornerRadius::same(2), theme.accent);
    }
    let color = if on { theme.accent } else { theme.muted };
    icons::paint_at(ui, rect.shrink(10.0), glyph, color);
    resp
}

/// A 32×32 transparent icon button (titlebar use).
fn icbtn(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(8), theme.surface_2);
    }
    icons::paint_at(ui, rect.shrink(8.0), glyph, theme.muted);
    resp
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

fn empty_state(ui: &mut egui::Ui, theme: &Theme, err: Option<&str>, pick: &mut Option<VaultPick>) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.32);
        ui.label(
            RichText::new("No library open")
                .font(theme::serif(22.0))
                .color(theme.text),
        );
        ui.add_space(4.0);
        if let Some(e) = err {
            ui.label(RichText::new(e).color(theme.rose));
        } else {
            ui.label(
                RichText::new("Open a folder as a library, or create a new one.")
                    .color(theme.muted),
            );
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            // center the two buttons
            let pad = (ui.available_width() - 300.0).max(0.0) * 0.5;
            ui.add_space(pad);
            let open = ui.add(
                egui::Button::new(
                    RichText::new("Open library…")
                        .size(13.0)
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(theme.accent)
                .corner_radius(8.0)
                .min_size(egui::vec2(140.0, 34.0)),
            );
            if open.clicked() {
                if let Some(p) = pick_folder("Open a library folder") {
                    *pick = Some(VaultPick::Open(p));
                }
            }
            ui.add_space(10.0);
            let new = ui.add(
                egui::Button::new(
                    RichText::new("New library…")
                        .size(13.0)
                        .strong()
                        .color(theme.text),
                )
                .fill(theme.surface)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .corner_radius(8.0)
                .min_size(egui::vec2(140.0, 34.0)),
            );
            if new.clicked() {
                if let Some(p) = pick_folder("Choose a folder for the new library") {
                    *pick = Some(VaultPick::New(p));
                }
            }
        });
        ui.add_space(10.0);
        ui.label(
            RichText::new("(or launch with a path:  niutero <folder>)")
                .font(theme::mono(11.0))
                .color(theme.faint),
        );
    });
}
