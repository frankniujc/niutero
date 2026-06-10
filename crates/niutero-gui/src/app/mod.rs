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
use std::sync::atomic::Ordering;

use eframe::egui::{self, Color32, RichText};
use log::{info, warn};
use niutero_engine::{self as engine, EntryView, Vault};

use crate::ai::AiState;
use crate::dialog::Dialog;
use crate::icons::{self, Glyph};
use crate::library::LibState;
use crate::normalize::{NormCache, NormView, NormalizeState};
use crate::overlays::TaskState;
use crate::settings::{self, SettingsState};
use crate::tags::{self, TagsState};
use crate::theme::{self, Theme};
use crate::widgets;

mod jobs;
mod library_ctl;
mod settings_ctl;
mod tags_ctl;

use jobs::{AiJob, BgHandle};

/// The tools in the left rail (spec §1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Library,
    Normalize,
    Ai,
    Tags,
    Settings,
}

/// The Library layouts (spec §4), switched from the titlebar. The Board view
/// (§4·C kanban) is temporarily removed — restore `library/board.rs` from git
/// history when it returns; the status/stars machinery it used stays live in
/// Reader and the detail panels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LibView {
    Classic,
    Reader,
}

/// The open library plus its loaded entries.
struct Library {
    vault: Vault,
    entries: Vec<EntryView>,
    /// Reload generation — bumped on every successful reload. Views cache
    /// derived models keyed on this (an explicit integer, not pointer
    /// identity, so a future in-place reload can't silently freeze a cache).
    gen: u64,
}

impl Library {
    fn load(path: &std::path::Path) -> Result<Library, String> {
        let vault = engine::open(path)?;
        engine::record_open(&vault.root);
        let entries = engine::list(&vault, engine::Filter::All)?;
        Ok(Library {
            vault,
            entries,
            gen: 0,
        })
    }
    /// Re-list entries after a mutation so the views reflect the new state.
    fn reload(&mut self) {
        match engine::list(&self.vault, engine::Filter::All) {
            Ok(e) => {
                self.entries = e;
                self.gen += 1;
            }
            Err(e) => warn!("reload entries: {e}"),
        }
    }
}

pub struct NiuteroApp {
    dark: bool,
    /// Display authors as `First Last` (default: as stored, `Last, First`).
    /// Machine-local appearance pref, like the theme.
    author_first_last: bool,
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
    /// Tags tool state (selection, sort, session-local colors).
    tags: TagsState,
    /// The open tag wizard (Organize / Auto-tag / Import), if any.
    tag_wizard: Option<tags::Wizard>,
    /// Accent swatch index (0 = the theme's own green; see `settings::ACCENTS`).
    accent_idx: usize,
    /// Whether the floating AI popup is open, and its composer buffer.
    ai_popup_open: bool,
    ai_popup_input: String,
    /// A running/finished background task shown as the bottom-left toast.
    task: Option<TaskState>,
    /// Transient one-line confirmation (e.g. "Copied citation"), shown briefly.
    toast: Option<String>,
    /// Auto-dismiss bookkeeping for `toast`: the message currently being timed
    /// and the `ctx` time at which it should vanish. A change in `toast` (re)arms
    /// the deadline, so the many `self.toast = Some(..)` sites stay timer-free.
    toast_shown: Option<String>,
    toast_deadline: Option<f64>,
    /// Cached git branch/dirty state of the open vault, for the status bar.
    /// `None` when no library is open or the vault isn't a git repo. Refreshed
    /// on library (re)load and after each edit — never recomputed per frame.
    git: Option<engine::GitStatus>,
    /// The open modal dialog (new entry / add-by-DOI), if any.
    dialog: Option<Dialog>,
    /// A running off-thread job (Online enrich / Sync / DOI import). Network and
    /// LLM calls must never block the UI thread, so they run on a worker that
    /// reports back over this channel; the UI polls it each frame.
    bg: Option<BgHandle>,
    /// A running off-thread LLM job (Test / Ask / Auto-tag / Organize). Single
    /// in-flight at a time; the result is routed to its consumer on the next poll.
    ai_job: Option<AiJob>,
    /// Toast re-arm generation: bumped by [`NiuteroApp::set_toast`] so an
    /// identical message shown twice in quick succession still gets a fresh
    /// auto-dismiss window (string comparison alone can't see the repeat).
    toast_gen: u64,
    toast_gen_armed: u64,
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
            Ok("tags") => Tool::Tags,
            Ok("settings") => Tool::Settings,
            _ => Tool::Library,
        };
        let lib_view = match std::env::var("NIU_VIEW").as_deref() {
            Ok("reader") => LibView::Reader,
            _ => LibView::Classic,
        };
        let task = std::env::var("NIU_TASK").ok().map(|_| {
            let mut t = TaskState::running("Online enrich…", "Enrich finished", 184);
            t.done = 92; // a static mid-progress demo for screenshots
            t
        });
        let norm_view = match std::env::var("NIU_NORMVIEW").as_deref() {
            Ok("review") => NormView::Review,
            Ok("ruleset") => NormView::Ruleset,
            Ok("rekey") => NormView::Rekey,
            _ => NormView::Overview,
        };
        let git = library
            .as_ref()
            .and_then(|l| engine::git_status(&l.vault.root));
        // Dev/QA: open a dialog on boot for screenshots/smoke (NIU_DIALOG=new|doi).
        let dialog = match std::env::var("NIU_DIALOG").as_deref() {
            Ok("new") => Some(Dialog::new_entry(None)),
            Ok("doi") => Some(Dialog::add_by_doi()),
            Ok("delete") => Some(Dialog::confirm_delete(
                "demo2024".into(),
                "A Demonstration Entry Title".into(),
            )),
            _ => None,
        };
        // Dev/QA: open a tag wizard on boot (NIU_WIZARD=organize|autotag|textag;
        // "import" stays as an alias for the TexTag wizard's old name).
        let tag_wizard = match std::env::var("NIU_WIZARD").as_deref() {
            Ok("organize") => Some(tags::Wizard::new(tags::WizardKind::Organize)),
            Ok("autotag") => Some(tags::Wizard::new(tags::WizardKind::Autotag)),
            Ok("textag") | Ok("import") => Some(tags::Wizard::new(tags::WizardKind::TexTag)),
            _ => None,
        };
        // Appearance is a machine-local pref: the app reopens looking the way
        // it was left (vaults.toml [ui] — personal, never synced).
        let ui = engine::ui_prefs().unwrap_or_default();
        NiuteroApp {
            dark: ui.dark,
            author_first_last: ui.author_first_last,
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
            tags: TagsState::default(),
            tag_wizard,
            accent_idx: ui.accent.min(settings::ACCENTS.len() - 1),
            ai_popup_open: std::env::var("NIU_POPUP").is_ok(),
            ai_popup_input: String::new(),
            task,
            toast: None,
            toast_shown: None,
            toast_deadline: None,
            git,
            dialog,
            bg: None,
            ai_job: None,
            toast_gen: 0,
            toast_gen_armed: 0,
        }
    }

    /// Show a transient toast. Always use this over assigning `self.toast`
    /// directly when the same message can repeat (copy actions, tag ops): the
    /// generation bump re-arms the auto-dismiss timer even for identical text.
    fn set_toast(&mut self, msg: impl Into<String>) {
        self.toast = Some(msg.into());
        self.toast_gen += 1;
    }

    /// Persist the appearance (theme + accent + author-name style) so the app
    /// reopens as left.
    pub(super) fn persist_ui_prefs(&mut self) {
        let prefs = engine::UiPrefs {
            dark: self.dark,
            accent: self.accent_idx,
            author_first_last: self.author_first_last,
        };
        if let Err(e) = engine::set_ui_prefs(prefs) {
            self.set_toast(format!("Couldn't save appearance: {e}"));
        }
    }

    /// Post-mutation bookkeeping: the opt-in auto-commit (the library's
    /// `workflow.auto_commit`), then a git-status refresh (the commit changes
    /// it). Call after every successful library mutation — never after a
    /// plain open/load, which must not commit a user's pending edits.
    pub(super) fn after_mutation(&mut self) {
        if let Some(lib) = self.library.as_ref() {
            if let Err(e) = engine::auto_commit_if_enabled(&lib.vault) {
                self.set_toast(format!("Auto-commit failed: {e}"));
            }
        }
        self.refresh_git();
    }

    /// Recompute the cached git branch/dirty state from the open vault. Cheap
    /// enough for load/edit boundaries (two `git` calls), but must not run per
    /// frame. `None` when no library is open or the vault isn't a git repo.
    fn refresh_git(&mut self) {
        self.git = self
            .library
            .as_ref()
            .and_then(|l| engine::git_status(&l.vault.root));
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
        // Detach any in-flight background job from the *previous* library: signal
        // it to stop and drop our handle so its completion can't refresh/lock the
        // new library (its disk write, if any, still finishes harmlessly).
        if let Some(bg) = self.bg.take() {
            bg.cancel.store(true, Ordering::Relaxed);
        }
        // Same for LLM jobs — and close any wizard: a job started against
        // library A must never deliver results (or apply tags) into library B.
        self.cancel_ai_job();
        self.tag_wizard = None;
        // Persist an unsaved AI-settings edit before the state is reset.
        self.flush_settings();
        self.task = None;
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
                // Reset the Tags tool too, or the previous library's session
                // colors / collapse / search / selection / model cache bleed in.
                self.tags = TagsState::default();
                // And the chat: the old conversation was grounded in the old
                // library — its citation chips would resolve against the new one.
                self.ai = AiState::default();
                self.ai_popup_input.clear();
            }
            Err(e) => {
                warn!("open library: {e}");
                self.open_error = Some(e.clone());
                self.toast = Some(e);
                self.library = None;
            }
        }
        self.refresh_git();
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

/// Display a path with the user's home directory collapsed to `~` (keeping
/// native separators) — the status bar shows `~\papers\bibvault` rather than a
/// long absolute path. Falls back to the full path when it isn't under home.
fn compact_path(p: &std::path::Path) -> String {
    if let Some(home) = home_dir() {
        if let Ok(rest) = p.strip_prefix(&home) {
            if rest.as_os_str().is_empty() {
                return "~".to_string();
            }
            return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
        }
    }
    p.display().to_string()
}

/// The user's home directory from the environment (`USERPROFILE` on Windows,
/// `HOME` elsewhere). `None` if neither is set.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The library switcher menu: recent libraries + open/new.
///
/// Each recent row carries a small "×" that *unbinds* the vault from this
/// machine's registry ([`engine::forget_vault`]) — it removes the entry from the
/// recent list only and never touches the vault folder on disk. Forgetting is
/// done inline (the menu stays open) so several stale test vaults can be cleared
/// in one pass; `toast` carries any error back to the caller.
fn library_menu(
    ui: &mut egui::Ui,
    theme: &Theme,
    pick: &mut Option<VaultPick>,
    toast: &mut Option<String>,
) {
    ui.set_min_width(300.0);
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
        ui.horizontal(|ui| {
            let resp = ui
                .add(
                    egui::Button::new(RichText::new(name).size(13.0).color(theme.text))
                        .frame(false),
                )
                .on_hover_text(rv.path.display().to_string());
            if resp.clicked() {
                *pick = Some(VaultPick::Open(rv.path.clone()));
            }
            // Unbind "×" pinned to the right edge — registry-only, no file delete.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let x = ui
                    .add(
                        icons::image(Glyph::Close, theme.faint)
                            .fit_to_exact_size(egui::Vec2::splat(13.0))
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Remove from recent — unbinds it here, keeps all files");
                if x.clicked() {
                    if let Err(e) = engine::forget_vault(&rv.path) {
                        *toast = Some(e);
                    } else {
                        *toast = Some(format!("Removed “{name}” from recent"));
                    }
                }
            });
        });
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
            .show(ctx, |ui| self.tool_rail(ui, &theme, ctx));

        // Drain background-worker messages (Online enrich / Sync / DOI import).
        self.poll_background(ctx);
        // Route a finished LLM job (Test / Ask / Auto-tag / Organize).
        self.poll_ai();

        match self.tool {
            Tool::Library => self.body_library(ctx, &theme),
            Tool::Normalize => self.body_normalize(ctx, &theme),
            Tool::Ai => self.body_ai(ctx, &theme),
            Tool::Tags => self.body_tags(ctx, &theme),
            Tool::Settings => self.body_settings(ctx, &theme),
        }

        // Floating overlays: AI FAB + popup (bottom-right), task toast (bottom-left).
        self.overlays(ctx, &theme);

        // Auto-dismiss the transient toast ~2.5s after it appears. A new message
        // is detected by comparing against the one last shown, which (re)arms the
        // deadline; we request a repaint so it clears on its own without needing
        // further interaction (egui otherwise idles between input events).
        let now = ctx.input(|i| i.time);
        // Re-arm on a new message OR a generation bump (`set_toast`): clicking
        // "Copy citation" twice in 2.5s repeats the exact text, and the second
        // toast still deserves its own dismissal window.
        if self.toast != self.toast_shown || self.toast_gen != self.toast_gen_armed {
            self.toast_shown = self.toast.clone();
            self.toast_gen_armed = self.toast_gen;
            self.toast_deadline = self.toast.as_ref().map(|_| now + 2.5);
        }
        if let Some(deadline) = self.toast_deadline {
            let remaining = deadline - now;
            if remaining <= 0.0 {
                self.toast = None;
                self.toast_shown = None;
                self.toast_deadline = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(remaining));
            }
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

        // Modal dialog (new entry / add-by-DOI), drawn above everything.
        self.dialog_step(ctx, &theme);
        // Tag wizard modal (Organize / Auto-tag / Import).
        self.tag_wizard_step(ctx, &theme);

        // While a background job runs the worker calls `request_repaint` on each
        // message; this is just a low-frequency safety net (not a per-frame spin)
        // in case a wake-up is missed.
        if self.bg.is_some() || self.ai_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Last chance to persist an AI-settings edit typed just before close.
        self.flush_settings();
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
        let mut menu_toast: Option<String> = None;
        ui.horizontal_centered(|ui| {
            ui.add_space(14.0);
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
                |ui| library_menu(ui, theme, &mut pick, &mut menu_toast),
            );

            // centered view switcher (Library only)
            if matches!(self.tool, Tool::Library) {
                let avail = ui.available_width();
                ui.add_space((avail - 230.0).max(0.0) * 0.5);
                self.view_switcher(ui, theme);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.window_controls(ui, theme);
                ui.add_space(4.0);
                let g = if self.dark { Glyph::Sun } else { Glyph::Moon };
                if widgets::icbtn(ui, theme, g, 32.0, 8.0)
                    .on_hover_text("Toggle theme")
                    .clicked()
                {
                    self.dark = !self.dark;
                    info!("theme → {}", if self.dark { "dark" } else { "light" });
                    self.persist_ui_prefs();
                }
            });
        });
        if let Some(p) = pick {
            self.apply_vault_pick(p);
        }
        if menu_toast.is_some() {
            self.toast = menu_toast;
        }
    }

    /// Windows-style window controls — minimize / maximize / close, flush to the
    /// top-right of the titlebar. Functional in this frameless window. Called
    /// inside a right-to-left layout, so the first button added sits rightmost:
    /// Close (rightmost) → Maximize → Minimize.
    fn window_controls(&self, ui: &mut egui::Ui, theme: &Theme) {
        // Buttons abut with no gap, like native Windows controls.
        ui.spacing_mut().item_spacing.x = 0.0;
        if win_control(ui, theme, Glyph::Close, true)
            .on_hover_text("Close")
            .clicked()
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if win_control(ui, theme, Glyph::WinMaximize, false)
            .on_hover_text("Maximize")
            .clicked()
        {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        if win_control(ui, theme, Glyph::WinMinimize, false)
            .on_hover_text("Minimize")
            .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    fn view_switcher(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        egui::Frame::default()
            .fill(theme.surface_2)
            .corner_radius(9.0)
            .inner_margin(3)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (label, glyph, v) in [
                        ("Classic", Glyph::Rows, LibView::Classic),
                        ("Reader", Glyph::Book, LibView::Reader),
                    ] {
                        let on = self.lib_view == v;
                        let w = label.len() as f32 * 7.0 + 30.0;
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
                        if on {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(7),
                                theme.surface,
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(7),
                                theme.surface.gamma_multiply(0.5),
                            );
                        }
                        let fg = if on { theme.accent } else { theme.text_2 };
                        icons::paint_at(
                            ui,
                            egui::Rect::from_center_size(
                                egui::pos2(rect.left() + 13.0, rect.center().y),
                                egui::vec2(15.0, 15.0),
                            ),
                            glyph,
                            fg,
                        );
                        ui.painter().text(
                            egui::pos2(rect.left() + 24.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(12.5),
                            fg,
                        );
                        if resp.clicked() {
                            self.lib_view = v;
                        }
                    }
                });
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        ui.horizontal_centered(|ui| {
            // Left: the open library's real folder path. Nothing when none open.
            if let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) {
                icons::show(ui, Glyph::Folder, 13.0, theme.muted);
                ui.add_space(3.0);
                ui.label(
                    RichText::new(compact_path(&root))
                        .font(theme::mono(11.0))
                        .color(theme.muted),
                )
                .on_hover_text(root.display().to_string());
            }
            // Right: git branch + dirty + entry count — all real. The git half is
            // omitted entirely when the vault isn't a git repository.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sep = |ui: &mut egui::Ui| {
                    ui.label(RichText::new("·").color(theme.faint));
                };
                ui.label(
                    RichText::new(format!("{} entries", self.entry_count()))
                        .color(theme.muted)
                        .size(11.5),
                );
                if let Some(git) = self.git.as_ref() {
                    if git.dirty {
                        sep(ui);
                        ui.label(RichText::new("modified").color(theme.text_2).size(11.5));
                    }
                    if let Some(branch) = git.branch.as_deref() {
                        sep(ui);
                        ui.label(
                            RichText::new(branch.to_owned())
                                .font(theme::mono(11.0))
                                .color(theme.muted),
                        );
                        icons::show(ui, Glyph::Branch, 13.0, theme.muted);
                    }
                }
            });
        });
    }

    fn tool_rail(&mut self, ui: &mut egui::Ui, theme: &Theme, ctx: &egui::Context) {
        ui.vertical_centered(|ui| {
            ui.add_space(2.0);
            niu_mark(ui, theme, 30.0);
            ui.add_space(10.0);
            for (tool, glyph, name) in [
                (Tool::Library, Glyph::Library, "Library"),
                (Tool::Normalize, Glyph::Normalize, "Normalize"),
                (Tool::Ai, Glyph::Ai, "AI Assistant"),
                (Tool::Tags, Glyph::Tag, "Tags"),
                (Tool::Settings, Glyph::Settings, "Settings"),
            ] {
                if rail_button(ui, theme, glyph, self.tool == tool)
                    .on_hover_text(name)
                    .clicked()
                {
                    // Leaving Settings ends its per-frame dirty-flush, so any
                    // unsaved AI edit (a key still in a focused field when the
                    // rail was clicked) must be persisted now.
                    if self.tool == Tool::Settings && tool != Tool::Settings {
                        self.flush_settings();
                    }
                    self.tool = tool;
                }
                ui.add_space(4.0);
            }
        });
        // Sync pinned to the bottom: commit & push (pull/merge first) over git.
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(2.0);
            let running = self.bg.is_some();
            if rail_button(ui, theme, Glyph::Sync, running)
                .on_hover_text("Sync (commit & push)")
                .clicked()
            {
                self.start_sync(ctx);
            }
        });
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

/// A Windows-style window control: full titlebar height, square corners, flush
/// to its neighbours. `danger` (the close button) hovers Windows-red with a
/// white glyph; the others hover with the neutral surface tint.
fn win_control(ui: &mut egui::Ui, theme: &Theme, glyph: Glyph, danger: bool) -> egui::Response {
    let h = ui.max_rect().height();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(44.0, h), egui::Sense::click());
    let glyph_color = if resp.hovered() {
        if danger {
            Color32::WHITE
        } else {
            theme.text
        }
    } else {
        theme.text_2
    };
    if resp.hovered() {
        let bg = if danger {
            Color32::from_rgb(0xE8, 0x11, 0x23)
        } else {
            theme.surface_2
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(0), bg);
    }
    let g = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(16.0));
    icons::paint_at(ui, g, glyph, glyph_color);
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
