// The one capture flow, shared by the popup button, the toolbar/keyboard
// command, and the context menu. Extracts the active tab's citation and sends
// it to the local connector, preferring a DOI (resolved to canonical BibTeX
// server-side) and falling back to BibTeX built from the page's meta tags.
//
// Returns a plain result object; it does not throw for the expected failure
// modes (callers render `result.error`).

import { extractCitation } from "./extract.js";
import { buildBibtex } from "./bibtex.js";
import { sendBibtex, sendDoi } from "./connector.js";
import { getConfig } from "./config.js";

export async function captureTab(tab) {
  if (!tab || tab.id == null) return fail("No active tab.");
  if (!/^https?:/i.test(tab.url || "")) {
    return fail("This isn't a normal web page, so there's nothing to capture.");
  }

  const cfg = await getConfig();
  if (!cfg.token) {
    return fail(
      "No session token set. Open options and paste the token from `niutero-cli connector`.",
    );
  }

  let data;
  try {
    const [inj] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: extractCitation,
    });
    data = inj && inj.result;
  } catch {
    return fail("The browser blocked reading this page (try a different tab).");
  }
  if (!data) return fail("Couldn't read this page.");

  // 1) DOI path — canonical BibTeX, resolved on the server.
  if (data.doi) {
    try {
      return ok(await sendDoi(cfg, data.doi), "doi", data);
    } catch (e) {
      // If the DOI didn't resolve but the page has usable meta tags, fall
      // through and try those rather than failing outright.
      if (!data.hasMeta) return fail(e.message, data);
    }
  }

  // 2) Meta-tag BibTeX path.
  if (data.hasMeta) {
    try {
      return ok(await sendBibtex(cfg, buildBibtex(data)), "meta", data);
    } catch (e) {
      return fail(e.message, data);
    }
  }

  return fail("No citation metadata found on this page.", data);
}

function ok(r, via, data) {
  return {
    ok: true,
    added: r.added,
    skipped: r.skipped,
    via,
    title: titleOf(data),
    doi: data && data.doi,
  };
}

function fail(error, data) {
  return { ok: false, error, title: titleOf(data), doi: data && data.doi };
}

function titleOf(data) {
  return (data && data.fields && data.fields.title) || (data && data.pageTitle) || "";
}
