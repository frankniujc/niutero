// Build a BibTeX entry from page-extracted fields. Used only as the fallback
// when the page has no DOI (DOIs resolve server-side to canonical BibTeX). The
// output must be valid — balanced braces, an ASCII cite key — because the
// engine validates every captured entry and rejects malformed ones with a 400.

const NON_KEY = /[^A-Za-z0-9]+/g;
// Built from ASCII-only strings so this source file stays pure ASCII.
const COMBINING = new RegExp("[\\u0300-\\u036f]", "g"); // combining diacritics
const CONTROL = new RegExp("[\\u0000-\\u001f\\u007f]", "g"); // C0 controls + DEL

export function buildBibtex(data) {
  const f = (data && data.fields) || {};
  const type = (data && data.type) || "misc";
  const key = citeKey(f);

  const lines = [];
  const push = (name, value) => {
    const v = bibValue(value);
    if (v) lines.push(`  ${name} = {${v}}`);
  };

  push("title", f.title);
  push("author", formatAuthors(f.authors));
  push("year", f.year);
  if (type === "article") push("journal", f.journal || f.conference);
  else if (type === "inproceedings") push("booktitle", f.booktitle || f.conference);
  push("volume", f.volume);
  push("number", f.number);
  push("pages", f.pages);
  push("publisher", f.publisher);
  push("institution", f.institution);
  if (f.arxivId) {
    push("eprint", f.arxivId);
    push("archivePrefix", "arXiv");
  }
  if (f.doi) push("doi", f.doi);
  push("url", f.url);

  return `@${type}{${key},\n${lines.join(",\n")}\n}\n`;
}

export function citeKey(f) {
  const surname = foldAscii(surnameOf((f.authors && f.authors[0]) || "")).toLowerCase();
  const year = String(f.year || "").replace(/\D/g, "").slice(0, 4);
  const word = foldAscii(firstTitleWord(f.title)).toLowerCase();
  // A key must lead with a letter. With no author/title word, or when the
  // composed key would start with a digit (e.g. a numeric surname, or a title
  // like "2024 in Review"), fall back to a "ref" prefix rather than emitting a
  // digit-leading (or empty) key that classic BibTeX mishandles.
  if (!surname && !word) return "ref" + year;
  const key = surname + year + word;
  return /^[A-Za-z]/.test(key) ? key : "ref" + key;
}

function surnameOf(author) {
  if (!author) return "";
  // "Lastname, Firstnames" -> Lastname; "First Last" -> Last.
  if (author.includes(",")) return author.split(",")[0].trim();
  const parts = author.trim().split(/\s+/);
  return parts[parts.length - 1] || "";
}

function firstTitleWord(title) {
  if (!title) return "";
  const STOP = new Set([
    "a", "an", "the", "on", "of", "in", "for", "and", "to",
    "with", "from", "toward", "towards", "using", "via", "is", "are",
  ]);
  for (const w of title.toLowerCase().split(/[^a-z0-9]+/i)) {
    if (w && !STOP.has(w)) return w;
  }
  return "";
}

function foldAscii(s) {
  return String(s).normalize("NFKD").replace(COMBINING, "").replace(NON_KEY, "");
}

export function formatAuthors(authors) {
  if (!authors || !authors.length) return "";
  return authors
    .map((a) => String(a).trim())
    .filter(Boolean)
    .join(" and ");
}

// Make a value safe inside a `{...}` BibTeX field: drop control chars, collapse
// whitespace, and balance braces (so the engine's serializer never sees an
// unbalanced value). We deliberately do not LaTeX-escape — niutero stores
// values verbatim, exactly as a user would type them.
export function bibValue(value) {
  if (value == null) return "";
  const s = String(value).replace(CONTROL, " ").replace(/\s+/g, " ").trim();
  return balanceBraces(s);
}

function balanceBraces(s) {
  let depth = 0;
  let out = "";
  for (const ch of s) {
    if (ch === "{") {
      depth++;
      out += ch;
    } else if (ch === "}") {
      if (depth > 0) {
        depth--;
        out += ch;
      } // drop an unmatched closing brace
    } else {
      out += ch;
    }
  }
  if (depth > 0) out += "}".repeat(depth);
  return out;
}
