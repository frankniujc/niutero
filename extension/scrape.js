// Injected into the active tab by popup.js (scripting.executeScript). Returns a
// JSON-cloneable { identifier, metadata }: an identifier the niutero server can
// resolve over the network (a DOI or arXiv id), plus scraped Highwire / Dublin
// Core metadata as the offline fallback. The server decides which to use and
// builds the BibTeX — this only reads the page. Last expression is the result,
// so the whole file is one IIFE.
(() => {
  const metas = {};
  for (const m of document.querySelectorAll("meta[name], meta[property]")) {
    const k = (m.getAttribute("name") || m.getAttribute("property") || "")
      .trim()
      .toLowerCase();
    const v = (m.getAttribute("content") || "").trim();
    if (!k || !v) continue;
    (metas[k] = metas[k] || []).push(v);
  }
  const first = (...keys) => {
    for (const k of keys) {
      const a = metas[k.toLowerCase()];
      if (a && a.length) return a[0];
    }
    return "";
  };
  const all = (...keys) => {
    const out = [];
    for (const k of keys) {
      const a = metas[k.toLowerCase()];
      if (a) out.push(...a);
    }
    return out;
  };

  // Allow `<` / `>` so SICI-style DOIs aren't truncated; values here come from
  // meta / href / JSON-LD, not raw page text, so there's no markup to overrun.
  const DOI_RE = /\b10\.\d{4,9}\/[^\s"']+/i;
  const cleanDoi = (s) => {
    if (!s) return "";
    const d = String(s)
      .trim()
      .replace(/^doi:\s*/i, "")
      .replace(/^https?:\/\/(dx\.)?doi\.org\//i, "");
    const m = d.match(DOI_RE);
    return m ? m[0].replace(/[.,;)\]]+$/, "") : "";
  };

  // --- identifier: arXiv (URL or meta), else a DOI from meta / anchor / JSON-LD ---
  let identifier;
  const ax = location.href.match(
    /arxiv\.org\/(?:abs|pdf)\/([0-9]{4}\.[0-9]{4,}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7})(v\d+)?/i,
  );
  if (ax) identifier = "arXiv:" + ax[1] + (ax[2] || "");
  if (!identifier) {
    const a = first("citation_arxiv_id");
    if (a) identifier = "arXiv:" + a.trim();
  }
  if (!identifier) {
    let doi = cleanDoi(
      first("citation_doi", "doi", "dc.identifier.doi", "prism.doi", "bepress_citation_doi"),
    );
    if (!doi) {
      for (const v of all("dc.identifier")) {
        doi = cleanDoi(v);
        if (doi) break;
      }
    }
    if (!doi) {
      const link = document.querySelector('a[href*="doi.org/10."]');
      if (link) doi = cleanDoi(link.getAttribute("href"));
    }
    if (!doi) {
      for (const s of document.querySelectorAll('script[type="application/ld+json"]')) {
        try {
          const parsed = JSON.parse(s.textContent);
          const roots = Array.isArray(parsed) ? parsed : [parsed];
          const nodes = [];
          for (const r of roots) {
            if (!r || typeof r !== "object") continue;
            nodes.push(r);
            if (Array.isArray(r["@graph"])) nodes.push(...r["@graph"]);
          }
          for (const node of nodes) {
            if (!node || typeof node !== "object") continue;
            const sameAs = Array.isArray(node.sameAs) ? node.sameAs : [node.sameAs];
            const idents = Array.isArray(node.identifier) ? node.identifier : [node.identifier];
            const cands = [node.doi, node.DOI, ...sameAs];
            for (const id of idents) {
              if (typeof id === "string") cands.push(id);
              else if (id && typeof id === "object") cands.push(id.value);
            }
            for (const c of cands) {
              const cd = cleanDoi(typeof c === "string" ? c : "");
              if (cd) {
                doi = cd;
                break;
              }
            }
            if (doi) break;
          }
        } catch {
          // ignore malformed JSON-LD
        }
        if (doi) break;
      }
    }
    if (doi) identifier = doi;
  }

  // --- authors: pick ONE scheme by precedence, deduped — many publishers emit
  //     the same people in both Highwire citation_author and Dublin Core
  //     dc.creator, which would otherwise double every author. ---
  let rawAuthors = all("citation_author");
  if (!rawAuthors.length) rawAuthors = all("citation_authors");
  if (!rawAuthors.length) rawAuthors = all("dc.creator");
  if (!rawAuthors.length) rawAuthors = all("bepress_citation_author");
  const authors = [];
  const seen = new Set();
  for (const a of rawAuthors) {
    for (const part of a.includes(";") ? a.split(";") : [a]) {
      const t = part.trim();
      if (!t) continue;
      const key = t.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      authors.push(t);
    }
  }

  const dateStr = first(
    "citation_publication_date",
    "citation_date",
    "citation_online_date",
    "prism.publicationdate",
    "dc.date",
    "article:published_time",
  );
  const ym = dateStr.match(/(\d{4})/);
  const fp = first("citation_firstpage");
  const lp = first("citation_lastpage");
  const pages = fp ? (lp && lp !== fp ? fp + "--" + lp : fp) : "";
  const journal = first("citation_journal_title", "prism.publicationname", "dc.source");
  const conf = first("citation_conference_title", "citation_inbook_title", "citation_book_title");
  const canonical = document.querySelector('link[rel="canonical"]');

  const metadata = {
    title:
      first("citation_title", "dc.title", "og:title", "twitter:title") || document.title || "",
    authors,
    year: ym ? ym[1] : "",
    journal,
    booktitle: conf,
    publisher: first("citation_publisher", "dc.publisher"),
    volume: first("citation_volume", "prism.volume"),
    issue: first("citation_issue", "prism.number"),
    pages,
    doi: cleanDoi(first("citation_doi", "prism.doi")),
    url:
      (canonical && canonical.href) ||
      first("citation_public_url", "citation_abstract_html_url") ||
      location.href,
    item_type: journal ? "article" : conf ? "conference" : "",
  };

  return { identifier, metadata };
})();
