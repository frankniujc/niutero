//! AI assist: the machine-local AI config, the connectivity test, and the
//! LLM-backed operations (tag suggestion, grounded ask, tag-vocabulary
//! organize). Everything here is suggestion-/read-only except
//! [`apply_tag_merges`], which routes through [`rename_tag`] so the
//! sidecar-only guarantee holds.

use serde::{Deserialize, Serialize};

use niutero_bib::{entries, BibItem};
use niutero_vault::{AiConfig, Registry, Vault};

use crate::{facets_of, list_tags, read_items, rename_tag, show, EntryView};

/// The built-in default model when the user hasn't picked one. A current,
/// undated Anthropic id (dated snapshots age out); the GUI seeds its model
/// field from this same constant so CLI and GUI defaults can't drift.
pub const DEFAULT_MODEL: &str = "claude-haiku-4-5";

/// The machine-local AI config (Settings → AI assistant). Stored in the registry
/// (`vaults.toml`), never in a synced vault.
pub fn ai_config() -> Result<AiConfig, String> {
    Ok(Registry::load()
        .map_err(|e| format!("read AI config: {e}"))?
        .ai)
}

/// Persist the machine-local AI config (a wholesale overwrite — for callers
/// that hold a complete config, like the GUI settings page seeding from
/// [`ai_config`] in the same frame). For read-modify-write field updates use
/// [`update_ai_config`], which runs the whole cycle under the registry lock.
pub fn set_ai_config(cfg: AiConfig) -> Result<(), String> {
    niutero_vault::registry::with_registry_mut(|reg| reg.ai = cfg)
        .map_err(|e| format!("save AI config: {e}"))
}

/// Update the machine-local AI config with the read-modify-write inside the
/// registry's exclusive cross-process lock, so two concurrent updaters (CLI vs
/// GUI, two CLI invocations) can't drop each other's fields. Returns the
/// config as saved.
pub fn update_ai_config(f: impl FnOnce(&mut AiConfig)) -> Result<AiConfig, String> {
    niutero_vault::registry::with_registry_mut(|reg| {
        f(&mut reg.ai);
        reg.ai.clone()
    })
    .map_err(|e| format!("save AI config: {e}"))
}

/// Resolve the (api key, model) to call with, honoring the stored config and
/// falling back to `$ANTHROPIC_API_KEY` / the default model. Errors (with an
/// actionable message) when LLM assist is off, no key is available, or the
/// config asks for a provider/base URL this build can't honor — a request must
/// never silently go to a different service than the one configured.
pub(crate) fn resolve_ai() -> Result<(String, String), String> {
    let cfg = ai_config()?;
    if !cfg.enabled {
        return Err(
            "LLM assist is off — run `niutero-cli ai config --enable true` \
                    or enable it in Settings → AI assistant"
                .into(),
        );
    }
    let provider = cfg.provider.trim();
    if !provider.is_empty() && !provider.eq_ignore_ascii_case("anthropic") {
        return Err(format!(
            "the AI provider \"{provider}\" isn't wired yet (only Anthropic is) — run \
             `niutero-cli ai config --provider anthropic` or change it in Settings → AI assistant"
        ));
    }
    if !cfg.base_url.trim().is_empty() {
        return Err("a custom AI base URL isn't honored yet — clear it with \
                    `niutero-cli ai config --base-url \"\"` so requests can't be misrouted"
            .into());
    }
    let key_from_registry = !cfg.api_key.trim().is_empty();
    let key = if key_from_registry {
        cfg.api_key.trim().to_string()
    } else {
        std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            "no API key — run `niutero-cli ai config --key …` \
             or add one in Settings → AI assistant"
                .to_string()
        })?
    };
    let model = if cfg.model.trim().is_empty() {
        DEFAULT_MODEL.to_string()
    } else {
        cfg.model
    };
    // The SOURCE label only — never log the key (or its prefix or length).
    log::debug!(
        "ai: model={model}, key from {}",
        if key_from_registry {
            "registry config"
        } else {
            "$ANTHROPIC_API_KEY"
        }
    );
    Ok((key, model))
}

/// **Online (LLM).** Verify the configured key/model with a tiny request.
pub fn ai_test() -> Result<String, String> {
    let (key, model) = resolve_ai()?;
    // 64 tokens, not the bare minimum: a thinking-enabled model can spend a
    // tiny budget entirely on thinking and return no text, failing the test
    // for a perfectly valid key.
    let reply = niutero_online::anthropic_text_with(
        &key,
        &model,
        64,
        "Reply with exactly the word: OK",
        "ping",
    )?;
    Ok(format!("Connected to {model} — replied “{}”", reply.trim()))
}

/// **Online (LLM).** Ask Claude to suggest tags for an entry, drawing on the
/// library's existing tag vocabulary so suggestions reuse the user's namespaces
/// (`topics:`/`wf:`/…). Suggestion-only — it returns tags to review, it does NOT
/// apply them (use `tag --add`). Needs LLM assist enabled with a key.
pub fn suggest_tags(v: &Vault, citekey: &str) -> Result<Vec<String>, String> {
    let view = show(v, citekey)?; // errors if the entry is absent
    let (key, model) = resolve_ai()?;
    let vocab = tag_vocabulary(v);
    // Bounded sizes only — never the prompt or response text.
    log::debug!(
        "ai suggest-tags: {citekey}, vocabulary {} tag(s)",
        vocab.len()
    );
    let (system, user) = tag_prompt(&view, &vocab);
    let text = niutero_online::anthropic_text_with(&key, &model, 256, &system, &user)?;
    Ok(parse_tag_list(&text))
}

/// **Online (LLM).** Answer a question grounded in the library. The model is sent
/// a compact summary of each entry (cite key · title · authors · year · venue ·
/// tags) and asked to cite the cite keys it draws on in `[brackets]`. Read-only —
/// it never edits the library.
pub fn ask(v: &Vault, question: &str) -> Result<String, String> {
    let q = question.trim();
    if q.is_empty() {
        return Err("ask what?".into());
    }
    let (key, model) = resolve_ai()?;
    let items = read_items(v)?;
    let context = grounding_context(v, &items);
    // Bounded sizes only — never the question, prompt, or response text.
    log::debug!("ai ask: grounding context {}B", context.len());
    let system = "You are a research-librarian assistant for a personal citation library. Answer \
        ONLY from the entries provided — if the library doesn't cover it, say so. Be concise. When \
        you refer to a paper, cite its cite key in square brackets, e.g. [vaswani2017attention]."
        .to_string();
    let user = format!("Library entries:\n{context}\n\nQuestion: {q}");
    niutero_online::anthropic_text_with(&key, &model, 1024, &system, &user)
}

/// A compact, token-bounded grounding summary of the library for [`ask`].
pub(crate) fn grounding_context(v: &Vault, items: &[BibItem]) -> String {
    /// Hard byte budget: the check runs *before* each append, so one oversized
    /// entry can't blow past it.
    const BUDGET: usize = 12_000;
    let mut out = String::new();
    for e in entries(items) {
        let f = |n: &str| e.get(n).map(str::trim).unwrap_or("");
        let venue = if f("journal").is_empty() {
            f("booktitle")
        } else {
            f("journal")
        };
        let tags = facets_of(v, &e.citekey).tags.join(", ");
        let line = format!(
            "- [{}] {} | {} | {} | {} | tags: {}\n",
            e.citekey,
            f("title"),
            f("author"),
            f("year"),
            venue,
            if tags.is_empty() { "—" } else { &tags },
        );
        if out.len() + line.len() > BUDGET {
            out.push_str("…(more entries omitted)\n");
            break;
        }
        out.push_str(&line);
    }
    out
}

/// One proposed change from [`organize_tags`]. `Deserialize` too, so the JSON
/// the CLI emits (`ai organize --json`) round-trips as `--plan` input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizePlan {
    /// Equivalent tags to fold together: `from` → `into`.
    pub merges: Vec<TagMerge>,
    /// Brand-new tags the model suggests for recurring, untagged topics.
    pub new_tags: Vec<TagSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagMerge {
    pub from: String,
    pub into: String,
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSuggestion {
    pub name: String,
    #[serde(default)]
    pub reason: String,
}

/// **Online (LLM).** Ask Claude to tidy the tag vocabulary: which existing tags
/// are duplicates/variants to merge, and which new tags to add. Returns a plan to
/// review — nothing is applied. `instructions` is optional extra steering.
pub fn organize_tags(v: &Vault, instructions: &str) -> Result<OrganizePlan, String> {
    let (key, model) = resolve_ai()?;
    let vocab = list_tags(v);
    if vocab.is_empty() {
        return Ok(OrganizePlan {
            merges: Vec::new(),
            new_tags: Vec::new(),
        });
    }
    // Bounded sizes only — never the prompt or response text.
    log::debug!("ai organize: vocabulary {} tag(s)", vocab.len());
    let listing = vocab
        .iter()
        .map(|(t, n)| format!("{t} ({n})"))
        .collect::<Vec<_>>()
        .join(", ");
    let system = "You tidy a researcher's tag vocabulary. Return STRICT JSON only (no prose, no \
        code fences) of the form {\"merges\":[{\"from\":\"ns:a\",\"into\":\"ns:b\",\"reason\":\"…\"}],\
        \"new_tags\":[{\"name\":\"ns:c\",\"reason\":\"…\"}]}. Only merge tags that already exist; \
        keep namespaces (topics:/wf:/…); propose new tags only for clearly recurring topics."
        .to_string();
    let extra = if instructions.trim().is_empty() {
        String::new()
    } else {
        format!("\n\nExtra instructions: {}", instructions.trim())
    };
    let user = format!("Existing tags (with entry counts): {listing}{extra}");
    let text = niutero_online::anthropic_text_with(&key, &model, 1024, &system, &user)?;
    parse_organize_plan(&text)
}

/// Parse the model's JSON plan, tolerating ```json fences. Drops merges/new tags
/// that don't reference real existing tags would be ideal, but we keep the
/// model's output and let the GUI review filter — here we just parse leniently.
pub(crate) fn parse_organize_plan(text: &str) -> Result<OrganizePlan, String> {
    // Strip a leading ```json / ``` fence if present.
    let t = text.trim();
    let t = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        merges: Vec<TagMerge>,
        #[serde(default)]
        new_tags: Vec<TagSuggestion>,
    }
    let raw: Raw = serde_json::from_str(t)
        .map_err(|e| format!("the model didn't return a tidy plan ({e})"))?;
    Ok(OrganizePlan {
        merges: raw.merges,
        new_tags: raw.new_tags,
    })
}

/// Outcome of one merge from [`apply_tag_merges`].
#[derive(Debug, Clone, Serialize)]
pub struct MergeApplied {
    pub from: String,
    pub into: String,
    /// Entries changed. `0` with no error means `from` didn't exist (a stale
    /// or hallucinated plan line) — benign, but report it as skipped.
    pub changed: usize,
    /// The error, if this merge failed; the loop continues past failures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Apply a reviewed set of tag merges, each via [`rename_tag`] — so vault
/// locking, merge-on-collision semantics, and the sidecar-only guarantee all
/// hold. Captures per-merge errors and continues. `references.bib` is never
/// touched. (The GUI Organize wizard and `ai organize --apply` both run this.)
pub fn apply_tag_merges(v: &mut Vault, merges: &[TagMerge]) -> Vec<MergeApplied> {
    let results: Vec<MergeApplied> = merges
        .iter()
        .map(|m| match rename_tag(v, &m.from, &m.into) {
            Ok(changed) => MergeApplied {
                from: m.from.clone(),
                into: m.into.clone(),
                changed,
                error: None,
            },
            Err(e) => MergeApplied {
                from: m.from.clone(),
                into: m.into.clone(),
                changed: 0,
                error: Some(e),
            },
        })
        .collect();
    let applied = results.iter().filter(|r| r.error.is_none()).count();
    log::info!("tag merges: {applied}/{} applied", results.len());
    results
}

/// Every distinct tag in use across the library, sorted.
fn tag_vocabulary(v: &Vault) -> Vec<String> {
    let mut tags: Vec<String> = v.meta.values().flat_map(|m| m.tags.clone()).collect();
    tags.sort();
    tags.dedup();
    tags
}

/// Build the (system, user) prompts for tag suggestion. Pure.
pub(crate) fn tag_prompt(view: &EntryView, vocab: &[String]) -> (String, String) {
    let system = "You tag bibliography entries for a researcher's library. Reply with ONLY a \
        comma-separated list of tags, reusing the existing vocabulary and its namespaces (e.g. \
        topics:foo, wf:bar) where they fit; propose a new namespaced tag only when nothing fits. \
        No prose."
        .to_string();
    let field = |name: &str| view.fields.get(name).map(String::as_str).unwrap_or("");
    let venue = if field("booktitle").is_empty() {
        field("journal")
    } else {
        field("booktitle")
    };
    let abstract_ = field("abstract");
    let abstract_ = if abstract_.chars().count() > 600 {
        let s: String = abstract_.chars().take(600).collect();
        format!("{s}…")
    } else {
        abstract_.to_string()
    };
    let user = format!(
        "Existing tags: {}\n\nEntry:\n  title: {}\n  author: {}\n  venue: {}\n  abstract: {}\n\n\
         Suggest up to 5 tags.",
        if vocab.is_empty() {
            "(none yet)".to_string()
        } else {
            vocab.join(", ")
        },
        field("title"),
        field("author"),
        venue,
        abstract_,
    );
    (system, user)
}

/// Parse an LLM tag list (comma- or newline-separated, possibly bulleted or
/// quoted) into clean, deduplicated tags. Pure.
pub(crate) fn parse_tag_list(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split([',', '\n']) {
        let t = raw
            .trim()
            .trim_start_matches(['-', '*', '•'])
            .trim()
            .trim_matches(|c| c == '"' || c == '\'' || c == '`')
            .trim();
        if !t.is_empty() && t.chars().count() <= 60 && !out.iter().any(|x| x == t) {
            out.push(t.to_string());
        }
    }
    out
}
