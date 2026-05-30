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

use crate::ai::{self, AiAction, AiState};
use crate::icons::{self, Glyph};
use crate::library::{self, LibAction, LibState};
use crate::normalize::{self, NormAction, NormCache, NormView, NormalizeState};
use crate::overlays::{self, OverlayMsg, TaskState};
use crate::settings::{self, SettingsAction, SettingsState};
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
    /// Normalize tool view-local UI state (sub-view, accept/reject, ruleset).
    norm: NormalizeState,
    /// Cached engine analysis for the Normalize tool; recomputed lazily and
    /// invalidated after any apply or a library switch.
    norm_cache: Option<NormCache>,
    /// AI Assistant tool state (composer + session turns).
    ai: AiState,
    /// Settings tool state (sub-view + edited values).
    settings: SettingsState,
    /// Accent swatch index (0 = the theme's own green; see `settings::ACCENTS`).
    accent_idx: usize,
    /// Whether the floating AI popup is open, and its composer buffer.
    ai_popup_open: bool,
    ai_popup_input: String,
    /// A running/finished background task shown as the bottom-left toast.
    task: Option<TaskState>,
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
        // Dev/QA affordance: start on a chosen tool/view (and optionally show the
        // AI popup or a demo task toast) so each surface can be opened directly
        // for screenshots and smoke tests. No effect when these vars are unset.
        let tool = match std::env::var("NIU_TAB").as_deref() {
            Ok("normalize") => Tool::Normalize,
            Ok("ai") => Tool::Ai,
            Ok("settings") => Tool::Settings,
            _ => Tool::Library,
        };
        let lib_view = match std::env::var("NIU_VIEW").as_deref() {
            Ok("reader") => LibView::Reader,
            Ok("board") => LibView::Board,
            _ => LibView::Classic,
        };
        let task = std::env::var("NIU_TASK").ok().map(|_| TaskState {
            label: "Online enrich…".into(),
            done_label: "Enrich finished".into(),
            total: 184,
            start: 0.0,
            duration: 8.0,
        });
        let norm_view = match std::env::var("NIU_NORMVIEW").as_deref() {
            Ok("review") => NormView::Review,
            Ok("ruleset") => NormView::Ruleset,
            Ok("rekey") => NormView::Rekey,
            _ => NormView::Overview,
        };
        NiuteroApp {
            dark: false,
            tool,
            lib_view,
            library,
            open_error,
            lib: LibState::default(),
            norm: NormalizeState {
                view: norm_view,
                ..NormalizeState::default()
            },
            norm_cache: None,
            ai: AiState::default(),
            settings: SettingsState::default(),
            accent_idx: 0,
            ai_popup_open: std::env::var("NIU_POPUP").is_ok(),
            ai_popup_input: String::new(),
            task,
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
                self.norm = NormalizeState::default();
                self.norm_cache = None;
                self.settings = SettingsState::default();
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
        let mut theme = Theme::of(self.dark);
        // Live accent override (Settings → Appearance); index 0 keeps the
        // theme's own green (nicer in dark than the raw swatch).
        if self.accent_idx != 0 {
            if let Some((r, g, b)) = settings::ACCENTS.get(self.accent_idx) {
                theme.set_accent(Color32::from_rgb(*r, *g, *b));
            }
        }
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
            Tool::Normalize => self.body_normalize(ctx, &theme),
            Tool::Ai => self.body_ai(ctx, &theme),
            Tool::Settings => self.body_settings(ctx, &theme),
        }

        // Floating overlays: AI FAB + popup (bottom-right), task toast (bottom-left).
        self.overlays(ctx, &theme);

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
        let entries = &self.library.as_ref().unwrap().entries;
        match self.lib_view {
            LibView::Classic => library::classic(ctx, theme, entries, &mut self.lib, &mut actions),
            LibView::Reader => library::reader(ctx, theme, entries, &mut self.lib, &mut actions),
            LibView::Board => library::board(ctx, theme, entries, &mut self.lib, &mut actions),
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
            LibAction::OpenPdf => {
                let p = engine::pdf_path(&lib.vault, &key);
                if p.exists() {
                    let s = p.to_string_lossy().replace('\\', "/");
                    let url = if s.starts_with('/') {
                        format!("file://{s}")
                    } else {
                        format!("file:///{s}")
                    };
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                } else {
                    self.toast = Some("No PDF attached to this entry".into());
                }
            }
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

    // ------------------------------------------------------------- Normalize

    fn body_normalize(&mut self, ctx: &egui::Context, theme: &Theme) {
        if self.library.is_none() {
            tool_placeholder(
                ctx,
                theme,
                "Normalize",
                "Open a library to analyze and clean it.",
            );
            return;
        }
        self.ensure_norm();
        let Some(cache) = self.norm_cache.as_ref() else {
            tool_placeholder(ctx, theme, "Normalize", "Could not analyze the library.");
            return;
        };
        let entries = &self.library.as_ref().unwrap().entries;
        let mut actions = Vec::new();
        normalize::normalize(ctx, theme, entries, cache, &mut self.norm, &mut actions);
        for a in actions {
            self.apply_norm_action(a, ctx);
        }
    }

    /// Compute (once) the offline analysis the Normalize tool renders. Cheap
    /// for small libraries; cached until invalidated by an apply or a switch.
    fn ensure_norm(&mut self) {
        if self.norm_cache.is_some() {
            return;
        }
        let Some(lib) = self.library.as_ref() else {
            return;
        };
        let report = match engine::analyze(&lib.vault) {
            Ok(r) => r,
            Err(e) => {
                warn!("analyze: {e}");
                self.toast = Some(format!("Analyze failed: {e}"));
                return;
            }
        };
        let diffs = engine::normalize_preview(&lib.vault, None).unwrap_or_default();
        let rekey = engine::rekey_preview(&lib.vault, None).unwrap_or_default();
        let pattern = lib
            .vault
            .config
            .citekey_pattern
            .clone()
            .unwrap_or_else(|| "{auth}{year}{title.1}{Title.2}".into());
        let total = report.total;
        self.norm_cache = Some(NormCache {
            report,
            diffs,
            rekey,
            pattern,
            total,
        });
    }

    fn apply_norm_action(&mut self, action: NormAction, ctx: &egui::Context) {
        match action {
            NormAction::RunOffline => self.norm.view = NormView::Review,
            NormAction::StartEnrich => {
                // Online enrich is a network feature (off the base path); here it
                // surfaces as the background-task toast. The progress is a timed
                // simulation — wiring the real online fetch is a later wave.
                let total = self.norm_cache.as_ref().map(|c| c.total).unwrap_or(0);
                let now = ctx.input(|i| i.time);
                self.task = Some(TaskState {
                    label: "Online enrich…".into(),
                    done_label: "Enrich finished".into(),
                    total,
                    start: now,
                    duration: 6.0,
                });
            }
            NormAction::CopyPatch => {
                let patch = self
                    .norm_cache
                    .as_ref()
                    .map(|c| build_patch(&c.diffs))
                    .unwrap_or_default();
                ctx.copy_text(patch);
                self.toast = Some("Copied patch".into());
            }
            NormAction::ApplyEntry(key) => {
                let args = self
                    .norm_cache
                    .as_ref()
                    .and_then(|c| c.diffs.iter().find(|d| d.citekey == key))
                    .map(norm_edit_args);
                if let Some((set, unset)) = args {
                    let r = self
                        .library
                        .as_ref()
                        .map(|lib| engine::edit(&lib.vault, &key, &set, &unset, None));
                    match r {
                        Some(Ok(())) => {
                            if let Some(lib) = self.library.as_mut() {
                                lib.reload();
                            }
                            self.lib.refresh();
                            info!("normalize: applied {key}");
                        }
                        Some(Err(e)) => self.toast = Some(format!("Apply failed: {e}")),
                        None => {}
                    }
                }
            }
            NormAction::ApplyAll => self.apply_all_norm(),
            NormAction::ApplyRekey => {
                let res = self
                    .library
                    .as_mut()
                    .map(|lib| engine::rekey_apply(&mut lib.vault, None));
                match res {
                    Some(Ok(changes)) => {
                        if let Some(lib) = self.library.as_mut() {
                            lib.reload();
                        }
                        // Cite keys changed → the Library selection is stale.
                        self.lib = LibState::default();
                        self.norm_cache = None;
                        let n = changes.len();
                        self.toast = Some(format!(
                            "Re-keyed {n} entr{}",
                            if n == 1 { "y" } else { "ies" }
                        ));
                    }
                    Some(Err(e)) => self.toast = Some(format!("Re-key failed: {e}")),
                    None => {}
                }
            }
        }
    }

    /// Apply every staged change not rejected: a single atomic `normalize_apply`
    /// when nothing is rejected, else `edit` per accepted entry.
    fn apply_all_norm(&mut self) {
        let none_rejected = !self.norm.done.values().any(|v| !v);
        let mut applied = 0usize;
        let mut error: Option<String> = None;

        if none_rejected {
            if let Some(lib) = self.library.as_ref() {
                match engine::normalize_apply(&lib.vault, None) {
                    Ok(ch) => applied = ch.len(),
                    Err(e) => error = Some(e),
                }
            }
        } else {
            let to_apply: Vec<(String, Vec<String>, Vec<String>)> = self
                .norm_cache
                .as_ref()
                .map(|c| {
                    c.diffs
                        .iter()
                        .filter(|d| self.norm.done.get(&d.citekey) != Some(&false))
                        .map(|d| {
                            let (s, u) = norm_edit_args(d);
                            (d.citekey.clone(), s, u)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let Some(lib) = self.library.as_ref() {
                for (k, s, u) in &to_apply {
                    match engine::edit(&lib.vault, k, s, u, None) {
                        Ok(()) => applied += 1,
                        Err(e) => {
                            error = Some(e);
                            break;
                        }
                    }
                }
            }
        }

        match error {
            Some(e) => self.toast = Some(format!("Apply failed: {e}")),
            None => {
                if let Some(lib) = self.library.as_mut() {
                    lib.reload();
                }
                self.lib.refresh();
                self.norm_cache = None;
                self.norm.done.clear();
                self.norm.view = NormView::Overview;
                self.toast = Some(format!(
                    "Applied {applied} change{}",
                    if applied == 1 { "" } else { "s" }
                ));
            }
        }
    }

    // ------------------------------------------------------------- AI tool

    fn body_ai(&mut self, ctx: &egui::Context, theme: &Theme) {
        let entries: &[EntryView] = self
            .library
            .as_ref()
            .map(|l| l.entries.as_slice())
            .unwrap_or(&[]);
        let mut actions = Vec::new();
        ai::ai_tab(ctx, theme, entries, &mut self.ai, &mut actions);
        for a in actions {
            self.apply_ai_action(a, ctx);
        }
    }

    fn apply_ai_action(&mut self, action: AiAction, ctx: &egui::Context) {
        match action {
            AiAction::OpenEntry(key) => self.jump_to_entry(key),
            AiAction::CopyCitations(keys) => {
                let Some(lib) = self.library.as_ref() else {
                    return;
                };
                let mut out = String::new();
                for k in &keys {
                    if let Ok(c) = engine::cite(&lib.vault, k) {
                        out.push_str(&c);
                        out.push('\n');
                    }
                }
                let n = keys.len();
                ctx.copy_text(out);
                self.toast = Some(format!(
                    "Copied {n} citation{}",
                    if n == 1 { "" } else { "s" }
                ));
            }
            AiAction::Toast(m) => self.toast = Some(m),
        }
    }

    /// Switch to the Library (Classic) and select `key`.
    fn jump_to_entry(&mut self, key: String) {
        self.tool = Tool::Library;
        self.lib_view = LibView::Classic;
        self.lib.selected = Some(key);
        self.lib.refresh();
    }

    // -------------------------------------------------------- Settings tool

    fn body_settings(&mut self, ctx: &egui::Context, theme: &Theme) {
        self.ensure_settings_seed();
        let schema = self
            .library
            .as_ref()
            .map(|l| l.vault.config.schema)
            .unwrap_or(0);
        let dark = self.dark;
        let accent = self.accent_idx;
        let mut actions = Vec::new();
        settings::settings(
            ctx,
            theme,
            &mut self.settings,
            schema,
            dark,
            accent,
            &mut actions,
        );
        for a in actions {
            self.apply_settings_action(a);
        }
    }

    /// Seed the editable Settings fields from the open vault, once.
    fn ensure_settings_seed(&mut self) {
        if self.settings.seeded {
            return;
        }
        if let Some(lib) = self.library.as_ref() {
            self.settings.name = lib.vault.config.name.clone();
            self.settings.pattern = lib
                .vault
                .config
                .citekey_pattern
                .clone()
                .unwrap_or_else(|| "{auth}{year}{title.1}{Title.2}".into());
            if let Ok(p) = engine::sync_prefs(&lib.vault) {
                self.settings.push = p.push;
            }
            self.settings.seeded = true;
        }
    }

    fn apply_settings_action(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::SetTheme(dark) => self.dark = dark,
            SettingsAction::SetAccent(i) => self.accent_idx = i,
            SettingsAction::SetGitRemote(url) => {
                let r = self
                    .library
                    .as_ref()
                    .map(|lib| engine::connect(&lib.vault, &url));
                match r {
                    Some(Ok(())) => {
                        info!("set git remote → {url}");
                        self.toast = Some("Set git remote".into());
                    }
                    Some(Err(e)) => self.toast = Some(format!("Remote failed: {e}")),
                    None => {}
                }
            }
            SettingsAction::SetPush(push) => {
                if let Some(lib) = self.library.as_ref() {
                    match engine::set_sync_prefs(&lib.vault, None, Some(push)) {
                        Ok(_) => info!("push-after-commit → {push}"),
                        Err(e) => self.toast = Some(format!("Sync prefs failed: {e}")),
                    }
                }
            }
            SettingsAction::Toast(m) => self.toast = Some(m),
        }
    }

    // --------------------------------------------------------- overlays

    fn overlays(&mut self, ctx: &egui::Context, theme: &Theme) {
        let entries: &[EntryView] = self
            .library
            .as_ref()
            .map(|l| l.entries.as_slice())
            .unwrap_or(&[]);
        let mut msgs = Vec::new();
        overlays::overlays(
            ctx,
            theme,
            entries,
            self.ai_popup_open,
            self.task.as_ref(),
            &mut self.ai_popup_input,
            &mut msgs,
        );
        for m in msgs {
            self.apply_overlay_msg(m);
        }
    }

    fn apply_overlay_msg(&mut self, msg: OverlayMsg) {
        match msg {
            OverlayMsg::ToggleAi => self.ai_popup_open = !self.ai_popup_open,
            OverlayMsg::CloseAi => self.ai_popup_open = false,
            OverlayMsg::OpenAiTab => {
                self.ai_popup_open = false;
                self.tool = Tool::Ai;
            }
            OverlayMsg::OpenEntry(key) => {
                self.ai_popup_open = false;
                self.jump_to_entry(key);
            }
            OverlayMsg::Review => {
                self.task = None;
                self.tool = Tool::Normalize;
                self.norm.view = NormView::Review;
            }
            OverlayMsg::DismissTask => self.task = None,
            OverlayMsg::Toast(m) => self.toast = Some(m),
        }
    }
}

// ---------------------------------------------------------------- helpers

/// Translate a normalize change into `engine::edit` arguments: `FIELD=VALUE`
/// for each set, and field names to unset (a change to nothing).
fn norm_edit_args(d: &niutero_engine::NormChange) -> (Vec<String>, Vec<String>) {
    let mut set = Vec::new();
    let mut unset = Vec::new();
    for c in &d.diffs {
        match &c.to {
            Some(v) => set.push(format!("{}={}", c.field, v)),
            None => unset.push(c.field.clone()),
        }
    }
    (set, unset)
}

/// Render the staged normalization changes as a human-readable text patch.
fn build_patch(diffs: &[niutero_engine::NormChange]) -> String {
    let mut out = String::new();
    for d in diffs {
        out.push_str(&format!("@ {}\n", d.citekey));
        for c in &d.diffs {
            match (&c.from, &c.to) {
                (Some(f), Some(t)) => {
                    out.push_str(&format!("  {}: - {f}\n  {}: + {t}\n", c.field, c.field))
                }
                (None, Some(t)) => out.push_str(&format!("  {}: + {t}\n", c.field)),
                (Some(f), None) => out.push_str(&format!("  {}: - {f}\n", c.field)),
                (None, None) => {}
            }
        }
        out.push('\n');
    }
    out
}

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
