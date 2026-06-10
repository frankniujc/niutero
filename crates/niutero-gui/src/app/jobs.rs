//! Off-thread work: background jobs (Online enrich / Sync / DOI import) and
//! single-flight LLM jobs (Test / Ask / Auto-tag / Organize) — spawned on
//! worker threads, reported back over channels, polled by the UI each frame.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use eframe::egui;
use niutero_engine as engine;

use crate::ai;
use crate::overlays::TaskState;
use crate::tags;

use super::NiuteroApp;

/// The result of an off-thread LLM job, routed to its consumer when it lands.
enum AiResult {
    /// `ai test` — a one-line message (or error) for a toast.
    Test(Result<String, String>),
    /// `ask` — the assistant's answer (or error) for the chat thread.
    Ask(Result<String, String>),
    /// Auto-tag — per-entry suggested tags `(citekey, tags)` (or error).
    AutoTag(Result<Vec<(String, Vec<String>)>, String>),
    /// Organize — the model's tidy plan (or error).
    Organize(Result<engine::OrganizePlan, String>),
}

/// Which surface started the LLM job — results are only delivered to a
/// matching consumer (a stale Organize result must not land in a freshly
/// opened Import wizard).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AiJobKind {
    Test,
    Ask,
    AutoTag,
    Organize,
}

/// The UI's handle on a running LLM job: the channel, what started it, the
/// vault it runs against (results for another library are dropped), and a
/// cooperative cancel flag multi-call loops (auto-tag) check between entries.
pub(super) struct AiJob {
    rx: mpsc::Receiver<AiResult>,
    pub(super) kind: AiJobKind,
    root: PathBuf,
    cancel: Arc<AtomicBool>,
}

/// A message from a background worker thread to the UI.
enum BgMsg {
    /// Units completed so far (drives the task toast's bar).
    Progress(usize),
    /// The job finished; the string is a one-line result summary.
    Done(String),
    /// The job failed; the string is shown as a toast.
    Failed(String),
}

/// The UI's handle on a running background job: the receiving end of its channel
/// plus a cooperative cancel flag the worker checks (so a long multi-entry job
/// can be stopped, and a library switch can detach the job).
pub(super) struct BgHandle {
    rx: mpsc::Receiver<BgMsg>,
    pub(super) cancel: Arc<AtomicBool>,
    /// Whether this job mutates the LIBRARY on success (entries / sidecar) —
    /// gates the post-completion auto-commit. PDF binary fetches and HF
    /// pushes don't (pdfs/ is git-ignored).
    mutates: bool,
}

impl NiuteroApp {
    /// Cancel and drop any in-flight LLM job (cooperatively — the worker
    /// checks the flag between entries; a single in-flight curl just finishes
    /// into a dropped channel).
    pub(super) fn cancel_ai_job(&mut self) {
        if let Some(job) = self.ai_job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Cancel the LLM job if a *wizard* started it — used when a wizard closes
    /// or a new one opens, so a stale result can't land in the wrong wizard.
    pub(super) fn cancel_wizard_ai_job(&mut self) {
        if self
            .ai_job
            .as_ref()
            .is_some_and(|j| matches!(j.kind, AiJobKind::AutoTag | AiJobKind::Organize))
        {
            self.cancel_ai_job();
        }
    }

    /// Start the Online-enrich background job: fetch missing metadata from
    /// doi.org for every entry that has a DOI. Runs on a worker thread (network
    /// I/O must never block the UI) that reopens the vault from its path.
    pub(super) fn start_enrich(&mut self, ctx: &egui::Context) {
        if self.bg.is_some() {
            self.toast = Some("A background task is already running".into());
            return;
        }
        let Some(lib) = self.library.as_ref() else {
            return;
        };
        let root = lib.vault.root.clone();
        // Match the engine's `entry_doi` rule (doi field, or a url with the exact
        // doi.org prefix) so the toast total reflects what `enrich` will accept.
        let keys: Vec<String> = lib
            .entries
            .iter()
            .filter(|e| {
                e.fields.get("doi").is_some_and(|d| !d.trim().is_empty())
                    || e.fields.get("url").is_some_and(|u| {
                        let u = u.trim();
                        u.starts_with("https://doi.org/") || u.starts_with("http://doi.org/")
                    })
            })
            .map(|e| e.citekey.clone())
            .collect();
        let total = keys.len();
        if total == 0 {
            self.toast = Some("No entries with a DOI to enrich".into());
            return;
        }
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let v = match engine::open(&root) {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(BgMsg::Failed(e));
                    ctx.request_repaint();
                    return;
                }
            };
            let mut filled = 0usize;
            let mut processed = 0usize;
            for k in keys.iter() {
                if worker_cancel.load(Ordering::Relaxed) {
                    break;
                }
                processed += 1;
                if let Ok(f) = engine::enrich(&v, k) {
                    if !f.is_empty() {
                        filled += 1;
                    }
                }
                let _ = tx.send(BgMsg::Progress(processed));
                ctx.request_repaint();
            }
            let summary = if processed < total {
                format!("Enrich stopped — {filled} filled ({processed}/{total})")
            } else {
                format!("Enriched {filled} of {total} entries")
            };
            let _ = tx.send(BgMsg::Done(summary));
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running(
            "Online enrich…",
            "Enrich finished",
            total,
        ));
        self.bg = Some(BgHandle {
            rx,
            cancel,
            mutates: true,
        });
    }

    /// Start a Sync (commit & push, pulling/merging first) on a worker thread.
    pub(super) fn start_sync(&mut self, ctx: &egui::Context) {
        if self.bg.is_some() {
            self.toast = Some("A background task is already running".into());
            return;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            self.toast = Some("Open a library first".into());
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = match engine::open(&root).and_then(|v| engine::sync(&v, None)) {
                Ok(engine::SyncStatus::Synced { committed, merged }) => {
                    let extra = match (committed, merged) {
                        (true, true) => " · committed · merged",
                        (true, false) => " · committed",
                        (false, true) => " · merged",
                        (false, false) => " · already up to date",
                    };
                    BgMsg::Done(format!("Synced{extra}"))
                }
                Ok(engine::SyncStatus::Conflict) => {
                    BgMsg::Failed("Sync conflict — resolve it manually".into())
                }
                Err(e) => BgMsg::Failed(format!("Sync failed: {e}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running("Syncing…", "Synced", 1));
        self.bg = Some(BgHandle {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            mutates: true,
        });
    }

    /// Fetch a single entry by DOI from doi.org on a worker thread. Returns
    /// whether the job actually started (so the caller can keep the dialog open
    /// with the typed DOI if it didn't).
    pub(super) fn start_doi_import(&mut self, doi: String, ctx: &egui::Context) -> bool {
        if self.bg.is_some() {
            self.toast = Some("A background task is already running".into());
            return false;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            self.toast = Some("Open a library first".into());
            return false;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = match engine::open(&root).and_then(|v| {
                // The library's configured duplicate default applies here too.
                let policy = engine::default_dup_policy(&v, engine::DupPolicy::Skip);
                let rep = engine::import_doi(&v, &doi, policy)?;
                // Opt-in post-import hooks; both are no-ops without their pref.
                let mut extra = String::new();
                let keys = rep.new_keys();
                if !keys.is_empty() {
                    if let Ok((f, a)) = engine::auto_fetch_pdfs(&v, &keys) {
                        if a > 0 {
                            extra.push_str(&format!(" · {f}/{a} PDF(s)"));
                        }
                    }
                    if let Ok((f, a)) = engine::auto_enrich(&v, &keys) {
                        if a > 0 {
                            extra.push_str(&format!(" · enriched {f}/{a}"));
                        }
                    }
                }
                Ok((rep, extra))
            }) {
                Ok((rep, extra)) if rep.added > 0 => {
                    BgMsg::Done(format!("Imported {} from DOI{extra}", rep.added))
                }
                Ok(_) => BgMsg::Done("Already in the library".into()),
                Err(e) => BgMsg::Failed(format!("DOI fetch failed: {e}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running("Fetching DOI…", "DOI imported", 1));
        self.bg = Some(BgHandle {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            mutates: true,
        });
        true
    }

    /// Run the opt-in post-import hooks (PDF auto-fetch, enrich-on-import)
    /// for freshly imported keys on a worker. Silently a no-op when both
    /// prefs are off, the key list is empty, or a task is already running —
    /// the hooks are best-effort by contract.
    pub(super) fn start_post_import(&mut self, keys: Vec<String>, ctx: &egui::Context) {
        if keys.is_empty() || self.bg.is_some() {
            return;
        }
        let Some(lib) = self.library.as_ref() else {
            return;
        };
        if !engine::pdf_auto_fetch_enabled(&lib.vault)
            && !lib.vault.config.workflow.enrich_on_import
        {
            return;
        }
        let root = lib.vault.root.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = match engine::open(&root).and_then(|v| {
                let mut parts = Vec::new();
                let (f, a) = engine::auto_fetch_pdfs(&v, &keys)?;
                if a > 0 {
                    parts.push(format!("{f}/{a} PDF(s)"));
                }
                let (f, a) = engine::auto_enrich(&v, &keys)?;
                if a > 0 {
                    parts.push(format!("enriched {f}/{a}"));
                }
                Ok(if parts.is_empty() {
                    "nothing applicable".to_string()
                } else {
                    parts.join(" · ")
                })
            }) {
                Ok(s) => BgMsg::Done(format!("Post-import: {s}")),
                Err(e) => BgMsg::Failed(format!("Post-import hooks failed: {e}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running("Post-import…", "Post-import done", 1));
        self.bg = Some(BgHandle {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            mutates: true,
        });
    }

    /// True when an HF push/pull can work right now: a repo configured for the
    /// open vault and a machine token set. Two cheap registry reads — used on
    /// clicks, never per frame.
    pub(super) fn hf_ready(&self) -> bool {
        self.library.as_ref().is_some_and(|l| {
            engine::pdf_repo(&l.vault)
                .map(|r| r.is_some())
                .unwrap_or(false)
        }) && engine::hf_token_set().unwrap_or(false)
    }

    /// Download the entry's PDF from its url on a worker (then push it to the
    /// HF repo when configured). Network never blocks the UI thread.
    pub(super) fn start_pdf_fetch(&mut self, key: String, ctx: &egui::Context) {
        if self.bg.is_some() {
            self.set_toast("A background task is already running");
            return;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            return;
        };
        let push = self.hf_ready();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = match engine::open(&root).and_then(|v| {
                engine::fetch_pdf(&v, &key)?;
                let mut note = "PDF downloaded".to_string();
                if push {
                    match engine::pdf_push(&v, &key) {
                        Ok(_) => note.push_str(" · pushed to HF"),
                        Err(e) => note.push_str(&format!(" · HF push failed: {e}")),
                    }
                }
                Ok(note)
            }) {
                Ok(n) => BgMsg::Done(n),
                Err(e) => BgMsg::Failed(format!("PDF fetch failed: {e}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running("Fetching PDF…", "PDF ready", 1));
        self.bg = Some(BgHandle {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            mutates: false,
        });
    }

    /// Upload an (already attached) PDF to the HF dataset repo on a worker.
    pub(super) fn start_pdf_push(&mut self, key: String, ctx: &egui::Context) {
        if self.bg.is_some() {
            // The attach already succeeded — only the push is skipped.
            self.set_toast("PDF attached (HF push skipped — a task is running)");
            return;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = match engine::open(&root).and_then(|v| engine::pdf_push(&v, &key)) {
                Ok(remote) => BgMsg::Done(format!("PDF attached · pushed to {remote}")),
                Err(e) => BgMsg::Failed(format!("PDF attached, but the HF push failed: {e}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running("Pushing PDF to HF…", "PDF pushed", 1));
        self.bg = Some(BgHandle {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            mutates: false,
        });
    }

    /// Download an entry's PDF from the HF dataset repo on a worker.
    pub(super) fn start_pdf_pull(&mut self, key: String, ctx: &egui::Context) {
        if self.bg.is_some() {
            self.set_toast("A background task is already running");
            return;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = match engine::open(&root).and_then(|v| engine::pdf_pull(&v, &key)) {
                Ok(_) => BgMsg::Done("PDF pulled from HF".into()),
                Err(e) => BgMsg::Failed(format!("HF pull failed: {e}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running("Pulling PDF from HF…", "PDF pulled", 1));
        self.bg = Some(BgHandle {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            mutates: false,
        });
    }

    /// Create the vault's HF dataset repo on a worker (Settings → Create).
    pub(super) fn start_create_pdf_repo(&mut self, ctx: &egui::Context) {
        if self.bg.is_some() {
            self.set_toast("A background task is already running");
            return;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            self.set_toast("Open a library first");
            return;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let msg = match engine::open(&root).and_then(|v| engine::create_pdf_repo(&v)) {
                Ok(m) => BgMsg::Done(m),
                Err(e) => BgMsg::Failed(format!("Create repo failed: {e}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
        self.task = Some(TaskState::running(
            "Creating dataset repo…",
            "Repo ready",
            1,
        ));
        self.bg = Some(BgHandle {
            rx,
            cancel: Arc::new(AtomicBool::new(false)),
            mutates: false,
        });
    }

    /// Drain the background worker's channel, updating the task toast and, on
    /// completion, reloading the library so new/enriched entries appear.
    pub(super) fn poll_background(&mut self, _ctx: &egui::Context) {
        let msgs: Vec<BgMsg> = self
            .bg
            .as_ref()
            .map(|b| b.rx.try_iter().collect())
            .unwrap_or_default();
        if msgs.is_empty() {
            return;
        }
        // Read before the terminal arms drop `bg`.
        let mutates = self.bg.as_ref().is_some_and(|b| b.mutates);
        let mut reload = false;
        let mut terminal = false;
        let mut succeeded = false;
        for m in msgs {
            match m {
                BgMsg::Progress(n) => {
                    if let Some(t) = self.task.as_mut() {
                        t.done = n;
                    }
                }
                BgMsg::Done(summary) => {
                    if let Some(t) = self.task.as_mut() {
                        t.done = t.total;
                        t.finished = true;
                        t.summary = Some(summary);
                    }
                    self.bg = None;
                    reload = true;
                    terminal = true;
                    succeeded = true;
                }
                BgMsg::Failed(e) => {
                    self.toast = Some(e);
                    self.task = None;
                    self.bg = None;
                    terminal = true;
                }
            }
        }
        // On success the entries changed → reload the in-memory view.
        if reload {
            if let Some(lib) = self.library.as_mut() {
                lib.reload();
            }
            self.lib.refresh();
            self.norm_cache = None;
        }
        if terminal {
            // Auto-commit only when a LIBRARY-MUTATING job actually succeeded
            // — a failed DOI fetch or a non-mutating HF push must never sweep
            // a user's unrelated pending edits into a commit. The git-status
            // refresh runs on every completion regardless (a Sync that failed
            // its push still advanced the work tree).
            if succeeded && mutates {
                self.after_mutation();
            } else {
                self.refresh_git();
            }
        }
    }

    // --------------------------------------------------------- LLM (off-thread)

    /// True (and toasts) if an AI request is already in flight — they're
    /// single-flight so results route unambiguously.
    fn ai_busy(&mut self) -> bool {
        if self.ai_job.is_some() {
            self.toast = Some("An AI request is already running".into());
            true
        } else {
            false
        }
    }

    /// Spawn-side bookkeeping shared by the `start_*` fns.
    fn track_ai_job(
        &mut self,
        rx: mpsc::Receiver<AiResult>,
        kind: AiJobKind,
        root: PathBuf,
        cancel: Arc<AtomicBool>,
    ) {
        self.ai_job = Some(AiJob {
            rx,
            kind,
            root,
            cancel,
        });
    }

    /// Test the configured model with a tiny request (Settings → Test).
    pub(super) fn start_ai_test(&mut self, ctx: &egui::Context) {
        if self.ai_busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(AiResult::Test(engine::ai_test()));
            ctx.request_repaint();
        });
        // Test is library-independent: track it under whatever root is open
        // (or none) — poll_ai only root-checks library-bound results.
        let root = self
            .library
            .as_ref()
            .map(|l| l.vault.root.clone())
            .unwrap_or_default();
        self.track_ai_job(rx, AiJobKind::Test, root, Arc::new(AtomicBool::new(false)));
        self.set_toast("Testing the connection…");
    }

    /// Ask a question grounded in the library (AI tab / popup). Returns whether
    /// the job actually started (busy / no library refuse it), so callers keep
    /// the typed question when it didn't.
    pub(super) fn start_ai_ask(&mut self, question: String, ctx: &egui::Context) -> bool {
        if self.ai_busy() {
            return false;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            self.set_toast("Open a library first");
            return false;
        };
        self.ai.turns.push(ai::Turn {
            user: true,
            text: question.clone(),
        });
        self.ai.pending = true;
        self.ai.input.clear();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let worker_root = root.clone();
        std::thread::spawn(move || {
            let r = engine::open(&worker_root).and_then(|v| engine::ask(&v, &question));
            let _ = tx.send(AiResult::Ask(r));
            ctx.request_repaint();
        });
        self.track_ai_job(rx, AiJobKind::Ask, root, Arc::new(AtomicBool::new(false)));
        true
    }

    /// Run the model over `keys`, collecting suggested tags per entry (Auto-tag
    /// wizard). Fail-fast: the first error (usually a bad key/network) aborts.
    /// Cooperative cancel between entries — a 20-key run can be stopped by
    /// closing the wizard or switching libraries. Returns whether it started.
    pub(super) fn start_auto_tag(&mut self, keys: Vec<String>, ctx: &egui::Context) -> bool {
        if keys.is_empty() || self.ai_busy() {
            return false;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            return false;
        };
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let ctx = ctx.clone();
        let worker_root = root.clone();
        std::thread::spawn(move || {
            let r = engine::open(&worker_root).and_then(|v| {
                let mut out = Vec::new();
                for k in &keys {
                    if worker_cancel.load(Ordering::Relaxed) {
                        return Ok(out); // receiver is gone anyway; stop burning calls
                    }
                    let tags = engine::suggest_tags(&v, k)?;
                    if !tags.is_empty() {
                        out.push((k.clone(), tags));
                    }
                }
                Ok(out)
            });
            let _ = tx.send(AiResult::AutoTag(r));
            ctx.request_repaint();
        });
        self.track_ai_job(rx, AiJobKind::AutoTag, root, cancel);
        true
    }

    /// Ask the model to tidy the vocabulary (Organize wizard). Returns whether
    /// the job actually started.
    pub(super) fn start_organize(&mut self, instructions: String, ctx: &egui::Context) -> bool {
        if self.ai_busy() {
            return false;
        }
        let Some(root) = self.library.as_ref().map(|l| l.vault.root.clone()) else {
            return false;
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let worker_root = root.clone();
        std::thread::spawn(move || {
            let r =
                engine::open(&worker_root).and_then(|v| engine::organize_tags(&v, &instructions));
            let _ = tx.send(AiResult::Organize(r));
            ctx.request_repaint();
        });
        self.track_ai_job(
            rx,
            AiJobKind::Organize,
            root,
            Arc::new(AtomicBool::new(false)),
        );
        true
    }

    /// Route a finished AI job to its consumer (chat / wizard / toast).
    pub(super) fn poll_ai(&mut self) {
        let (result, kind, root) = match self.ai_job.as_ref() {
            None => return,
            Some(job) => match job.rx.try_recv() {
                Ok(r) => (r, job.kind, job.root.clone()),
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // The worker died without a result (a panic inside the
                    // engine call). Unstick the consumer that started it —
                    // otherwise the chat spinner / wizard "Working…" persist
                    // forever.
                    let kind = job.kind;
                    self.ai_job = None;
                    if kind == AiJobKind::Ask {
                        self.ai.pending = false;
                    }
                    if matches!(kind, AiJobKind::AutoTag | AiJobKind::Organize) {
                        if let Some(w) = self.tag_wizard.as_mut() {
                            w.fail();
                        }
                    }
                    self.set_toast("The AI request failed unexpectedly");
                    return;
                }
            },
        };
        self.ai_job = None;
        // A result for a different library than the one now open is stale —
        // drop it (switch_to also cancels jobs; this is the backstop).
        let library_bound = !matches!(kind, AiJobKind::Test);
        let current_root = self.library.as_ref().map(|l| l.vault.root.clone());
        if library_bound && current_root.as_deref() != Some(root.as_path()) {
            return;
        }
        match result {
            AiResult::Test(r) => self.set_toast(r.unwrap_or_else(|e| e)),
            AiResult::Ask(r) => {
                self.ai.pending = false;
                self.ai.turns.push(ai::Turn {
                    user: false,
                    text: r.unwrap_or_else(|e| format!("⚠ {e}")),
                });
            }
            AiResult::AutoTag(r) => {
                // Deliver only into the wizard kind that started the job.
                let w = self
                    .tag_wizard
                    .as_mut()
                    .filter(|w| w.kind() == tags::WizardKind::Autotag);
                match (r, w) {
                    (Ok(results), Some(w)) => w.set_autotag(results),
                    (Err(e), w) => {
                        if let Some(w) = w {
                            w.fail();
                        }
                        self.set_toast(e);
                    }
                    (Ok(_), None) => {} // wizard gone — drop the stale result
                }
            }
            AiResult::Organize(r) => {
                let w = self
                    .tag_wizard
                    .as_mut()
                    .filter(|w| w.kind() == tags::WizardKind::Organize);
                match (r, w) {
                    (Ok(plan), Some(w)) => w.set_organize(plan.merges, plan.new_tags),
                    (Err(e), w) => {
                        if let Some(w) = w {
                            w.fail();
                        }
                        self.set_toast(e);
                    }
                    (Ok(_), None) => {}
                }
            }
        }
    }
}
