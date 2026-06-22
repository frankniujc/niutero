// The page-side extractor. This function is injected into the active tab with
// chrome.scripting.executeScript({ func }), which serializes it via toString()
// and runs it in the page. It MUST stay self-contained: no imports, no
// references to anything outside its own body — only DOM APIs. It returns a
// plain JSON-serializable object.
//
// Strategy: prefer a DOI (publishers, Google Scholar, ACL, PubMed, etc. emit
// `citation_doi`; arXiv gets a synthetic DataCite DOI). When there is no DOI,
// fall back to the Highwire Press `citation_*` / Dublin Core meta tags.
export function extractCitation() {
  const metas = {};
  for (const m of document.querySelectorAll("meta[name], meta[property]")) {
    const key = (m.getAttribute("name") || m.getAttribute("property") || "")
      .trim()
      .toLowerCase();
    const val = (m.getAttribute("content") || "").trim();
    if (!key || !val) continue;
    (metas[key] = metas[key] || []).push(val);
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

  // Allow `<` / `>` so SICI-style DOIs (e.g. 10.1002/(SICI)...<588::...>...)
  // aren't truncated. DOI values here come from meta/href/JSON-LD (not raw page
  // text), so there is no stray markup to over-capture.
  const DOI_RE = /\b10\.\d{4,9}\/[^\s"']+/i;
  const cleanDoi = (s) => {
    if (!s) return "";
    let d = String(s)
      .trim()
      .replace(/^doi:\s*/i, "")
      .replace(/^https?:\/\/(dx\.)?doi\.org\//i, "");
    const m = d.match(DOI_RE);
    if (!m) return "";
    // Strip trailing punctuation that prose / URLs often glue on.
    return m[0].replace(/[.,;)\]]+$/, "");
  };

  // --- DOI ---
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
        // Flatten any @graph wrapper (Yoast/WordPress and many publishers nest
        // the real article node there).
        const nodes = [];
        for (const r of roots) {
          if (!r || typeof r !== "object") continue;
          nodes.push(r);
          if (Array.isArray(r["@graph"])) nodes.push(...r["@graph"]);
        }
        for (const node of nodes) {
          if (!node || typeof node !== "object") continue;
          // sameAs is often an array of identifier URLs; identifier may be a
          // PropertyValue ({ propertyID:"doi", value:"..." }) or a string.
          const sameAs = Array.isArray(node.sameAs) ? node.sameAs : [node.sameAs];
          const idents = Array.isArray(node.identifier)
            ? node.identifier
            : [node.identifier];
          const candidates = [node.doi, node.DOI, ...sameAs];
          for (const id of idents) {
            if (typeof id === "string") candidates.push(id);
            else if (id && typeof id === "object") candidates.push(id.value);
          }
          for (const cand of candidates) {
            const c = cleanDoi(typeof cand === "string" ? cand : "");
            if (c) {
              doi = c;
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

  // --- arXiv ---
  let arxivId = "";
  const am = location.href.match(
    /arxiv\.org\/(?:abs|pdf)\/([0-9]{4}\.[0-9]{4,}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7})(v\d+)?/i,
  );
  if (am) arxivId = am[1] + (am[2] || "");
  if (!arxivId) {
    const a = first("citation_arxiv_id");
    if (a) arxivId = a.trim();
  }
  // arXiv mints a versionless DataCite DOI — prefer resolving by it for
  // canonical BibTeX rather than scraping the abstract page.
  if (!doi && arxivId) doi = "10.48550/arXiv." + arxivId.replace(/v\d+$/i, "");

  // --- authors ---
  // Pick ONE source by precedence rather than merging schemes: many publishers
  // (OJS, DSpace…) emit the same people in both Highwire `citation_author` and
  // Dublin Core `dc.creator`, often in two name formats — concatenating would
  // double every author. citation_author (repeated) beats citation_authors (the
  // legacy semicolon list); dc.creator is the DC author field (not contributor,
  // which is often editors).
  let rawAuthors = all("citation_author");
  if (!rawAuthors.length) rawAuthors = all("citation_authors");
  if (!rawAuthors.length) rawAuthors = all("dc.creator");
  if (!rawAuthors.length) rawAuthors = all("bepress_citation_author");

  const authors = [];
  const seenAuthors = new Set();
  for (const a of rawAuthors) {
    const parts = a.includes(";") ? a.split(";") : [a];
    for (const part of parts) {
      const t = part.trim();
      if (!t) continue;
      const k = t.toLowerCase();
      if (seenAuthors.has(k)) continue; // backstop dedup
      seenAuthors.add(k);
      authors.push(t);
    }
  }

  // --- year ---
  const dateStr = first(
    "citation_publication_date",
    "citation_date",
    "citation_online_date",
    "prism.publicationdate",
    "dc.date",
    "article:published_time",
  );
  const ym = dateStr.match(/(\d{4})/);
  const year = ym ? ym[1] : "";

  const journal = first("citation_journal_title", "prism.publicationname", "dc.source");
  const conference = first("citation_conference_title", "citation_inbook_title");
  const booktitle = first("citation_book_title");
  const firstpage = first("citation_firstpage");
  const lastpage = first("citation_lastpage");
  const pages = firstpage
    ? lastpage && lastpage !== firstpage
      ? firstpage + "--" + lastpage
      : firstpage
    : "";
  const canonical = document.querySelector('link[rel="canonical"]');

  const fields = {
    title: first("citation_title", "dc.title", "og:title", "twitter:title") || document.title || "",
    authors,
    year,
    journal,
    conference,
    booktitle,
    volume: first("citation_volume", "prism.volume"),
    number: first("citation_issue", "prism.number"),
    pages,
    publisher: first("citation_publisher", "dc.publisher"),
    institution: first(
      "citation_dissertation_institution",
      "citation_technical_report_institution",
    ),
    doi,
    arxivId,
    url:
      first("citation_public_url", "citation_abstract_html_url") ||
      (canonical && canonical.href) ||
      location.href,
  };

  let type = "misc";
  if (journal) type = "article";
  else if (conference || booktitle) type = "inproceedings";
  else if (first("citation_dissertation_institution")) type = "phdthesis";
  else if (first("citation_technical_report_institution", "citation_technical_report_number"))
    type = "techreport";
  else if (arxivId) type = "article";

  // Enough to build a usable entry from meta alone? Include conference/booktitle
  // so a proceedings/chapter page the classifier already calls inproceedings
  // (and buildBibtex can render) isn't rejected as "no metadata".
  const hasMeta = !!(
    fields.title &&
    (authors.length || year || journal || conference || booktitle || arxivId)
  );

  return {
    doi,
    arxivId,
    type,
    fields,
    hasMeta,
    pageUrl: location.href,
    pageTitle: document.title,
  };
}
