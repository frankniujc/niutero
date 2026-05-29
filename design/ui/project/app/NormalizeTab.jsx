// Normalize tab — the heart of Niutero: keep a 1,292-entry .bib clean.
// Sub-views: Overview (analysis + plan + health), Review changes (diff),
// Ruleset (the rules), Re-key (apply citation-key pattern).
//
// ============================================================================
// BUTTON REFERENCE — Normalize tool
// ----------------------------------------------------------------------------
// SubNav: Overview / Review changes / Ruleset / Re-key (switch sub-view).
// Overview:
//   • Recommended plan "Run" (Offline cleanup) — runs the rule passes, jumps to Review.
//   • Recommended plan "Run" (Online enrich)   — starts the background enrich task (startTask).
//   • Health row "View"  — select that issue class to inspect.
//   • Health row "Fix"   — jump to Review changes pre-filtered to that issue.
// Review changes:
//   • "Back to overview" — return to the overview.
//   • "Apply all"        — accept every staged change.
//   • "Reject all"       — discard every staged change.
//   • "Copy as patch"    — copy the diff as a patch (mock).
//   • per-entry Accept / Reject — accept or reject that single entry's changes.
// Ruleset: per-rule toggle switches enable/disable each normalization rule.
// Re-key:
//   • "Preview all 1,292" — preview keys for the whole library (mock).
//   • "Apply re-key"      — rewrite citation keys using the pattern.
// ============================================================================

const NZ_HEALTH = [
  { id: 'offline', label: 'Offline-changeable', count: 6, hint: 'Entries a local cleanup pass would rewrite' },
  { id: 'titles', label: 'Odd titles', count: 0, hint: 'ALL-CAPS, missing, or truncated titles' },
  { id: 'arxiv', label: 'arXiv mislabeled', count: 0, hint: 'Published papers still typed as preprints' },
  { id: 'venues', label: 'Inconsistent venues', count: 12, hint: 'Same venue written several ways' },
  { id: 'dupes', label: 'Likely duplicates', count: 0, hint: 'Near-identical title + author + year' },
  { id: 'url', label: 'Missing URL', count: 350, hint: 'No url / doi to resolve the entry' },
  { id: 'year', label: 'Missing year', count: 21, hint: 'No publication year set' },
];

const NZ_DIFFS = [
  { key: 'braunIdentifyingFunctionallyImportant2024', title: 'Identifying Functionally Important Features…', rule: 'Venue canonicalization',
    changes: [{ field: 'journal', from: 'Arxiv preprint arXiv:2405.12241', to: 'arXiv preprint' }, { field: 'archivePrefix', from: '—', to: 'arXiv' }] },
  { key: 'gaoScalingEvaluatingSparse2025', title: 'Scaling and Evaluating Sparse Autoencoders', rule: 'arXiv → published',
    changes: [{ field: 'entrytype', from: 'misc', to: 'inproceedings' }, { field: 'booktitle', from: '—', to: 'International Conference on Learning Representations' }, { field: 'year', from: '2024', to: '2025' }] },
  { key: 'brickenTowardMonosemanticity2023', title: 'Toward Monosemanticity: Decomposing…', rule: 'Title casing',
    changes: [{ field: 'title', from: 'Toward monosemanticity: decomposing language models', to: 'Toward Monosemanticity: Decomposing Language Models' }] },
];

const NZ_RULES = [
  { id: 'venue', name: 'Venue canonicalization', on: true, desc: 'Map venue aliases to one canonical name (e.g. "Proc. of ICLR" → "International Conference on Learning Representations").', meta: '146 aliases' },
  { id: 'casing', name: 'Title casing', on: true, desc: 'Normalize titles to Title Case, preserving protected words and {LaTeX} braces.', meta: 'Title Case' },
  { id: 'arxiv', name: 'Promote arXiv → published', on: true, desc: 'When an arXiv entry has a matching published DOI, switch the entry type and fill the venue.', meta: 'online' },
  { id: 'fields', name: 'Required fields', on: true, desc: 'Flag entries missing url, year, or author. Never auto-fills — surfaces them in Health.', meta: 'url · year · author' },
  { id: 'dupes', name: 'Duplicate detection', on: false, desc: 'Group near-identical entries by normalized title + first author + year for manual merge.', meta: 'off' },
  { id: 'names', name: 'Author name format', on: true, desc: 'Normalize to "Last, First" and collapse repeated whitespace in author lists.', meta: 'Last, First' },
];

const NZ_REKEY = [
  { old: 'Arad2025', neu: 'arad2025SAEsAreGood', t: 'SAEs Are Good for Steering…' },
  { old: 'braun_e2e_24', neu: 'braun2024IdentifyingFunctionally', t: 'Identifying Functionally Important…' },
  { old: 'farrell2024', neu: 'farrell2024ApplyingSparseAutoencoders', t: 'Applying Sparse Autoencoders to Unlearn…' },
  { old: 'gao2024scaling', neu: 'gao2025ScalingEvaluatingSparse', t: 'Scaling and Evaluating Sparse Autoencoders', clash: true },
  { old: 'bricken23', neu: 'bricken2023TowardMonosemanticity', t: 'Toward Monosemanticity…' },
];

function nzHeader(title, sub) {
  return React.createElement('div', { style: { display: 'flex', alignItems: 'flex-end', justifyContent: 'space-between', marginBottom: 22, gap: 16 } },
    React.createElement('div', null,
      React.createElement('h1', { style: { fontSize: 26, fontWeight: 700, letterSpacing: '-.02em', margin: 0, color: 'var(--text)' } }, title),
      sub ? React.createElement('div', { style: { fontSize: 13.5, color: 'var(--muted)', marginTop: 5 } }, sub) : null,
    ),
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 9, fontSize: 13, color: 'var(--text-2)' } },
      React.createElement('span', { style: { fontFamily: 'var(--serif)', fontWeight: 500 } }, 'BibVault'),
      React.createElement('span', { style: { width: 1, height: 14, background: 'var(--border)' } }),
      React.createElement('span', { style: { fontVariantNumeric: 'tabular-nums' } }, '1,292 entries'),
    ),
  );
}
const nzCard = { background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 14, boxShadow: 'var(--shadow)' };
const nzCardHead = { padding: '15px 20px', borderBottom: '1px solid var(--border-2)', display: 'flex', alignItems: 'center', gap: 9, fontSize: 13.5, fontWeight: 700, color: 'var(--text)' };

function NormalizeOverview({ go, startTask }) {
  const [picked, setPicked] = React.useState('offline');
  const enrich = () => startTask && startTask({ label: 'Enriching metadata', sub: 'Fetching published versions · arXiv, OpenReview', doneLabel: 'Enrichment complete', doneSub: '58 entries upgraded · 4 unchanged', total: 62 });
  return React.createElement('div', { style: { maxWidth: 880, margin: '0 auto' } },
    nzHeader('Library analysis', 'Last scanned just now · 2 actions recommended'),
    // recommended plan
    React.createElement('div', { style: { ...nzCard, marginBottom: 22 } },
      React.createElement('div', { style: nzCardHead }, Icon.sparkle({ s: 17, style: { color: 'var(--accent)' } }), 'Recommended plan'),
      nzPlanRow(1, Icon.refresh, 'Offline cleanup', 'Rewrites local formatting — casing, venues, whitespace. Safe and reversible.', '6 changes', false, () => go('review')),
      nzPlanRow(2, Icon.download, 'Online enrich', 'Up to 62 entries (60 arXiv · 2 OpenReview) may upgrade to a published version.', 'network · rate-limited', true, enrich),
    ),
    // health
    React.createElement('div', { style: nzCard },
      React.createElement('div', { style: nzCardHead }, Icon.checkCircle({ s: 17, style: { color: 'var(--accent)' } }), 'Health',
        React.createElement('span', { style: { flex: 1 } }),
        React.createElement('span', { style: { fontSize: 12, fontWeight: 600, color: 'var(--muted)' } }, '4 of 7 checks need attention')),
      React.createElement('div', { style: { padding: '6px 8px' } },
        NZ_HEALTH.map((h) => {
          const issue = h.count > 0; const on = picked === h.id;
          return React.createElement('div', { key: h.id, onClick: () => issue && setPicked(h.id),
            style: { display: 'flex', alignItems: 'center', gap: 14, padding: '11px 12px', borderRadius: 10, cursor: issue ? 'pointer' : 'default', background: on && issue ? 'var(--sel)' : 'transparent' } },
            React.createElement('span', { style: { display: 'flex', color: issue ? '#B6792B' : 'var(--accent)' } }, issue ? Icon.warn({ s: 17 }) : Icon.check({ s: 17 })),
            React.createElement('div', { style: { flex: 1, minWidth: 0 } },
              React.createElement('div', { style: { fontSize: 14, fontWeight: 600, color: 'var(--text)' } }, h.label),
              React.createElement('div', { style: { fontSize: 12, color: 'var(--muted)' } }, h.hint)),
            React.createElement('span', { style: { fontSize: 16, fontWeight: 700, fontVariantNumeric: 'tabular-nums', minWidth: 44, textAlign: 'right', color: issue ? '#B6792B' : 'var(--faint)' } }, h.count),
            React.createElement('div', { style: { display: 'flex', gap: 7, visibility: issue ? 'visible' : 'hidden' } },
              React.createElement('button', { className: 'niu-btn', style: { height: 28, padding: '0 12px' }, onClick: (e) => { e.stopPropagation(); setPicked(h.id); } }, 'View'),
              React.createElement('button', { className: 'niu-btn pri', style: { height: 28, padding: '0 12px' }, onClick: (e) => { e.stopPropagation(); go('review'); } }, 'Fix')),
          );
        }),
      ),
    ),
  );
}
function nzPlanRow(n, icon, title, desc, tag, primary, onRun) {
  return React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 16, padding: '16px 20px', borderBottom: n === 1 ? '1px solid var(--border-2)' : 'none' } },
    React.createElement('div', { style: { width: 36, height: 36, borderRadius: 10, background: 'var(--surface-2)', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--text-2)', flex: '0 0 auto' } }, icon({ s: 18 })),
    React.createElement('div', { style: { flex: 1, minWidth: 0 } },
      React.createElement('div', { style: { fontSize: 15, fontWeight: 700, color: 'var(--text)', marginBottom: 2 } }, title),
      React.createElement('div', { style: { fontSize: 13, color: 'var(--text-2)', textWrap: 'pretty' } }, desc)),
    React.createElement('span', { style: { fontSize: 11.5, fontWeight: 600, color: 'var(--muted)', whiteSpace: 'nowrap' } }, tag),
    React.createElement('button', { className: 'niu-btn' + (primary ? ' pri' : ''), onClick: onRun, style: { minWidth: 76, justifyContent: 'center' } }, 'Run'),
  );
}

function NormalizeReview({ go }) {
  const [done, setDone] = React.useState({});
  const total = NZ_DIFFS.reduce((a, d) => a + d.changes.length, 0);
  return React.createElement('div', { style: { maxWidth: 880, margin: '0 auto' } },
    nzHeader('Review changes', total + ' proposed changes across ' + NZ_DIFFS.length + ' entries'),
    // post-cleanup banner — these are staged, not yet written to disk
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 12, padding: '13px 18px', borderRadius: 12, background: 'var(--accent-tint)', border: '1px solid var(--accent)', marginBottom: 18 } },
      Icon.checkCircle({ s: 19, style: { color: 'var(--accent)', flex: '0 0 auto' } }),
      React.createElement('div', { style: { flex: 1, fontSize: 13.5, color: 'var(--text)' } },
        React.createElement('strong', null, 'Offline cleanup finished.'), ' ', total + ' changes are staged — nothing is written to disk until you apply.'),
      React.createElement('button', { className: 'niu-btn', style: { height: 30 }, onClick: () => go && go('overview') }, 'Back to overview')),
    React.createElement('div', { style: { display: 'flex', gap: 9, marginBottom: 18 } },
      React.createElement('button', { className: 'niu-btn pri', onClick: () => { const m = {}; NZ_DIFFS.forEach((d) => m[d.key] = 'acc'); setDone(m); } }, Icon.check({ s: 16 }), 'Apply all'),
      React.createElement('button', { className: 'niu-btn', onClick: () => { const m = {}; NZ_DIFFS.forEach((d) => m[d.key] = 'rej'); setDone(m); } }, 'Reject all'),
      React.createElement('div', { style: { flex: 1 } }),
      React.createElement('button', { className: 'niu-btn' }, Icon.copy({ s: 15 }), 'Copy as patch'),
    ),
    NZ_DIFFS.map((d) => React.createElement('div', { key: d.key, style: { ...nzCard, marginBottom: 16, overflow: 'hidden', opacity: done[d.key] === 'rej' ? 0.5 : 1 } },
      React.createElement('div', { style: { padding: '13px 18px', borderBottom: '1px solid var(--border-2)', display: 'flex', alignItems: 'center', gap: 12 } },
        React.createElement('div', { style: { flex: 1, minWidth: 0 } },
          React.createElement('div', { className: 'niu-serif', style: { fontSize: 15.5, fontWeight: 500, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, d.title),
          React.createElement('div', { className: 'niu-mono', style: { fontSize: 11.5, color: 'var(--accent)', marginTop: 2 } }, d.key)),
        React.createElement('span', { className: 'niu-tag', style: { background: 'var(--accent-tint)', color: 'var(--accent)' } }, d.rule),
      ),
      React.createElement('div', { style: { padding: '6px 0' } },
        d.changes.map((c, i) => React.createElement('div', { key: i, style: { display: 'grid', gridTemplateColumns: '120px 1fr', gap: 14, padding: '8px 18px', alignItems: 'baseline' } },
          React.createElement('span', { className: 'niu-mono', style: { fontSize: 12, color: 'var(--muted)' } }, c.field),
          React.createElement('div', { style: { display: 'flex', flexWrap: 'wrap', alignItems: 'baseline', gap: 8, fontSize: 13 } },
            React.createElement('span', { style: { color: '#C2536B', background: 'rgba(194,83,107,.1)', padding: '2px 7px', borderRadius: 5, textDecoration: c.from === '—' ? 'none' : 'line-through', textDecorationColor: 'rgba(194,83,107,.5)' } }, c.from),
            Icon.arrowRight({ s: 14, style: { color: 'var(--faint)' } }),
            React.createElement('span', { style: { color: 'var(--accent)', background: 'var(--accent-tint)', padding: '2px 7px', borderRadius: 5, fontWeight: 500 } }, c.to)),
        )),
      ),
      React.createElement('div', { style: { display: 'flex', gap: 8, padding: '11px 18px', borderTop: '1px solid var(--border-2)', justifyContent: 'flex-end' } },
        React.createElement('button', { className: 'niu-btn', style: { height: 30 }, onClick: () => setDone((s) => ({ ...s, [d.key]: 'rej' })) }, 'Reject'),
        React.createElement('button', { className: 'niu-btn pri', style: { height: 30 }, onClick: () => setDone((s) => ({ ...s, [d.key]: 'acc' })) },
          done[d.key] === 'acc' ? 'Accepted ✓' : 'Accept'),
      ),
    )),
  );
}

function NormalizeRuleset() {
  const [rules, setRules] = React.useState(NZ_RULES);
  return React.createElement('div', { style: { maxWidth: 880, margin: '0 auto' } },
    nzHeader('Ruleset', 'Rules run top-to-bottom during Offline cleanup. Stored in .niutero/rules.toml'),
    React.createElement('div', { style: nzCard },
      rules.map((r, i) => React.createElement('div', { key: r.id, style: { display: 'flex', alignItems: 'center', gap: 16, padding: '16px 20px', borderBottom: i < rules.length - 1 ? '1px solid var(--border-2)' : 'none' } },
        React.createElement('div', { style: { flex: 1, minWidth: 0 } },
          React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 9, marginBottom: 3 } },
            React.createElement('span', { style: { fontSize: 14.5, fontWeight: 700, color: 'var(--text)' } }, r.name),
            React.createElement('span', { className: 'niu-mono', style: { fontSize: 10.5, color: 'var(--muted)', background: 'var(--surface-2)', padding: '2px 7px', borderRadius: 5 } }, r.meta)),
          React.createElement('div', { style: { fontSize: 13, color: 'var(--text-2)', textWrap: 'pretty' } }, r.desc)),
        nzToggle(r.on, () => setRules((s) => s.map((x) => x.id === r.id ? { ...x, on: !x.on } : x))),
      )),
    ),
  );
}
function nzToggle(on, onClick) {
  return React.createElement('button', { onClick, style: { width: 42, height: 25, borderRadius: 20, border: 'none', cursor: 'pointer', flex: '0 0 auto', background: on ? 'var(--accent)' : 'var(--faint)', position: 'relative', transition: 'background .15s' } },
    React.createElement('span', { style: { position: 'absolute', top: 3, left: on ? 20 : 3, width: 19, height: 19, borderRadius: '50%', background: '#fff', transition: 'left .15s', boxShadow: '0 1px 3px rgba(0,0,0,.3)' } }));
}

function NormalizeRekey() {
  return React.createElement('div', { style: { maxWidth: 880, margin: '0 auto' } },
    nzHeader('Re-key', 'Regenerate citation keys for existing entries using the library pattern'),
    React.createElement('div', { style: { ...nzCard, padding: '16px 20px', marginBottom: 18, display: 'flex', alignItems: 'center', gap: 14 } },
      Icon.key({ s: 18, style: { color: 'var(--accent)' } }),
      React.createElement('span', { className: 'niu-mono', style: { fontSize: 13.5, color: 'var(--text)' } }, '{auth}{year}{title.1}{Title.2}'),
      React.createElement('div', { style: { flex: 1 } }),
      React.createElement('span', { style: { fontSize: 12.5, color: '#B6792B', display: 'inline-flex', alignItems: 'center', gap: 6 } }, Icon.warn({ s: 14 }), '1 collision — a/b suffix will be added'),
    ),
    React.createElement('div', { style: { ...nzCard, overflow: 'hidden' } },
      React.createElement('div', { style: { display: 'grid', gridTemplateColumns: '1fr 18px 1fr', gap: 14, padding: '11px 20px', borderBottom: '1px solid var(--border-2)', fontSize: 11, fontWeight: 700, letterSpacing: '.04em', textTransform: 'uppercase', color: 'var(--muted)' } },
        React.createElement('span', null, 'Current key'), React.createElement('span', null), React.createElement('span', null, 'New key')),
      NZ_REKEY.map((r, i) => React.createElement('div', { key: i, style: { display: 'grid', gridTemplateColumns: '1fr 18px 1fr', gap: 14, padding: '12px 20px', borderBottom: i < NZ_REKEY.length - 1 ? '1px solid var(--border-2)' : 'none', alignItems: 'center' } },
        React.createElement('div', { style: { minWidth: 0 } },
          React.createElement('div', { className: 'niu-mono', style: { fontSize: 12.5, color: 'var(--muted)', textDecoration: 'line-through' } }, r.old),
          React.createElement('div', { style: { fontSize: 11.5, color: 'var(--faint)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, r.t)),
        Icon.arrowRight({ s: 15, style: { color: 'var(--faint)' } }),
        React.createElement('div', { className: 'niu-mono', style: { fontSize: 12.5, color: 'var(--accent)', display: 'flex', alignItems: 'center', gap: 7 } }, r.neu,
          r.clash ? React.createElement('span', { style: { fontSize: 10, color: '#B6792B', background: 'rgba(182,121,43,.12)', padding: '1px 6px', borderRadius: 4, textDecoration: 'none' } }, '+a') : null),
      )),
    ),
    React.createElement('div', { style: { display: 'flex', justifyContent: 'flex-end', gap: 9, marginTop: 18 } },
      React.createElement('button', { className: 'niu-btn' }, 'Preview all 1,292'),
      React.createElement('button', { className: 'niu-btn pri' }, Icon.key({ s: 16 }), 'Apply re-key'),
    ),
  );
}

function NormalizeTab({ startTask }) {
  const [view, setView] = React.useState('overview');
  const nav = [
    { id: 'overview', label: 'Overview', icon: Icon.checkCircle },
    { id: 'review', label: 'Review changes', icon: Icon.copy, badge: 6 },
    { id: 'ruleset', label: 'Ruleset', icon: Icon.filter },
    { id: 'rekey', label: 'Re-key', icon: Icon.key },
  ];
  const body = view === 'review' ? React.createElement(NormalizeReview, { go: setView })
    : view === 'ruleset' ? React.createElement(NormalizeRuleset)
    : view === 'rekey' ? React.createElement(NormalizeRekey)
    : React.createElement(NormalizeOverview, { go: setView, startTask });
  return React.createElement('section', { style: { flex: 1, display: 'flex', minWidth: 0, background: 'var(--bg)' } },
    React.createElement(SubNav, { items: nav, active: view, onChange: setView }),
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, padding: '30px 38px 48px' } }, body),
  );
}

window.NormalizeTab = NormalizeTab;
