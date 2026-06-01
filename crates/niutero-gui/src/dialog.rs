//! Modal dialogs over the Library: create a new entry from fields, or add an
//! entry by DOI / import a `.bib` file. The dialog only collects input and
//! reports an [`DialogOutcome`]; the app performs the engine call (a synchronous
//! `add`/`import`, or an off-thread DOI fetch) so this stays UI-only.

use eframe::egui::{self, RichText};
use niutero_engine::Status;

use crate::theme::Theme;
use crate::widgets;

/// Which modal is open.
pub enum Dialog {
    NewEntry(NewEntryForm),
    AddByDoi(AddByDoiForm),
}

impl Dialog {
    pub fn new_entry(status: Option<Status>) -> Self {
        Dialog::NewEntry(NewEntryForm::new(status))
    }
    pub fn add_by_doi() -> Self {
        Dialog::AddByDoi(AddByDoiForm::default())
    }
}

/// The new-entry form fields. `status` pre-files the entry into a Board column.
pub struct NewEntryForm {
    pub type_: String,
    pub key: String,
    pub title: String,
    pub author: String,
    pub year: String,
    pub venue: String,
    pub doi: String,
    pub status: Option<Status>,
}

impl NewEntryForm {
    fn new(status: Option<Status>) -> Self {
        NewEntryForm {
            type_: "article".into(),
            key: String::new(),
            title: String::new(),
            author: String::new(),
            year: String::new(),
            venue: String::new(),
            doi: String::new(),
            status,
        }
    }
}

#[derive(Default)]
pub struct AddByDoiForm {
    pub doi: String,
}

/// What the user asked of the dialog this frame.
pub enum DialogOutcome {
    /// No terminal action — keep the dialog open.
    Keep,
    /// Dismiss without doing anything.
    Cancel,
    /// Create the entry from [`NewEntryForm`].
    CreateEntry,
    /// Fetch [`AddByDoiForm::doi`] from the network.
    FetchDoi,
    /// Pick and import a local `.bib` file.
    ImportFile,
}

/// The common BibTeX entry types offered in the new-entry type picker.
const TYPES: [&str; 6] = [
    "article",
    "inproceedings",
    "book",
    "incollection",
    "techreport",
    "misc",
];

/// Render the open dialog as a modal; returns the user's intent for this frame.
pub fn dialog_ui(ctx: &egui::Context, theme: &Theme, dialog: &mut Dialog) -> DialogOutcome {
    let modal = egui::Modal::new(egui::Id::new("niu-dialog")).show(ctx, |ui| {
        ui.set_width(420.0);
        match dialog {
            Dialog::NewEntry(f) => new_entry_form(ui, theme, f),
            Dialog::AddByDoi(f) => add_by_doi_form(ui, theme, f),
        }
    });
    // Click-outside / Esc cancels, unless a button already decided the outcome.
    if modal.should_close() && matches!(modal.inner, DialogOutcome::Keep) {
        DialogOutcome::Cancel
    } else {
        modal.inner
    }
}

fn new_entry_form(ui: &mut egui::Ui, theme: &Theme, f: &mut NewEntryForm) -> DialogOutcome {
    let mut outcome = DialogOutcome::Keep;
    ui.label(
        RichText::new("New entry")
            .size(17.0)
            .strong()
            .color(theme.text),
    );
    ui.add_space(2.0);
    ui.label(
        RichText::new("Fields go straight into references.bib; a cite key is generated if you leave it blank.")
            .size(12.0)
            .color(theme.muted),
    );
    ui.add_space(12.0);

    egui::Grid::new("niu-new-entry-grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Type").size(12.5).color(theme.text_2));
            egui::ComboBox::from_id_salt("niu-new-type")
                .selected_text(f.type_.clone())
                .show_ui(ui, |ui| {
                    for t in TYPES {
                        ui.selectable_value(&mut f.type_, t.to_string(), t);
                    }
                });
            ui.end_row();

            field_row(ui, theme, "Cite key", &mut f.key, "(auto)");
            field_row(ui, theme, "Title", &mut f.title, "");
            field_row(ui, theme, "Author", &mut f.author, "Last, First and …");
            field_row(ui, theme, "Year", &mut f.year, "");
            field_row(ui, theme, "Venue", &mut f.venue, "journal / booktitle");
            field_row(ui, theme, "DOI", &mut f.doi, "");
        });

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if widgets::button(ui, theme, None, "Create", true, 32.0).clicked() {
                outcome = DialogOutcome::CreateEntry;
            }
            if widgets::button(ui, theme, None, "Cancel", false, 32.0).clicked() {
                outcome = DialogOutcome::Cancel;
            }
        });
    });
    outcome
}

fn add_by_doi_form(ui: &mut egui::Ui, theme: &Theme, f: &mut AddByDoiForm) -> DialogOutcome {
    let mut outcome = DialogOutcome::Keep;
    ui.label(
        RichText::new("Add by DOI / import")
            .size(17.0)
            .strong()
            .color(theme.text),
    );
    ui.add_space(2.0);
    ui.label(
        RichText::new(
            "Fetch one entry from doi.org, or import every entry from a local .bib file.",
        )
        .size(12.0)
        .color(theme.muted),
    );
    ui.add_space(14.0);

    ui.label(RichText::new("DOI").size(12.5).color(theme.text_2));
    ui.add_space(3.0);
    let doi_resp = ui.add(
        egui::TextEdit::singleline(&mut f.doi)
            .hint_text("10.1145/3292500.3330701")
            .desired_width(f32::INFINITY),
    );
    let submit_doi = doi_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.add_space(14.0);

    ui.horizontal(|ui| {
        if widgets::button(
            ui,
            theme,
            Some(crate::icons::Glyph::Download),
            "Import .bib file…",
            false,
            32.0,
        )
        .clicked()
        {
            outcome = DialogOutcome::ImportFile;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let fetch = widgets::button(ui, theme, None, "Fetch", true, 32.0).clicked();
            if widgets::button(ui, theme, None, "Cancel", false, 32.0).clicked() {
                outcome = DialogOutcome::Cancel;
            }
            if (fetch || submit_doi) && !f.doi.trim().is_empty() {
                outcome = DialogOutcome::FetchDoi;
            }
        });
    });
    outcome
}

/// One labeled single-line field row inside the new-entry grid.
fn field_row(ui: &mut egui::Ui, theme: &Theme, label: &str, value: &mut String, hint: &str) {
    ui.label(RichText::new(label).size(12.5).color(theme.text_2));
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(280.0),
    );
    ui.end_row();
}
