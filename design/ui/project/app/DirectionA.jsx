// Direction A — "Refined Classic": four-column 3-pane, clean & comfortable.
// LibraryViewA returns just the panes (Side/List/Detail) so it can be reused
// inside the full-app shell as well as the standalone exploration artboard.
//
// ============================================================================
// BUTTON REFERENCE — Classic library view
// ----------------------------------------------------------------------------
// Left sidebar (Side):
//   • "All Entries"          — clear the tag filter (show every entry).
//   • Tags ✦ (sparkle)       — open the AI auto-tag popover (TagAIPopover).
//   • Tags + (plus)          — create a new tag (mock).
//   • tag rows (TagTree)     — click to filter the list by that tag; click again clears.
//   • filter chip "✕"        — clear the active tag filter (footer, only when filtering).
// List toolbar:
//   • panelLeft icon         — collapse / show the left tags sidebar (hideLeft).
//   • + (plus)               — create a new bibliography item (mock).
//   • link icon              — add an entry by identifier (DOI / arXiv).
//   • search box             — full-text search across fields & tags.
//   • panelRight icon        — collapse / show the right details panel (hideRight).
//   • "Creator ▾" header     — sort the list by creator (mock).
// List rows: click a row to select it and load it into the details panel.
// Details panel (Detail):
//   • LockBtn                — toggle the panel between locked (read-only) and editing.
//   • Cite (primary)         — copy a formatted citation (mock).
//   • link icon              — open the entry's source URL.
//   • BibTeX                 — copy the raw BibTeX entry to the clipboard (mock).
// ============================================================================
function LibraryViewA({ theme, setTheme }) {
  const items = window.NIU.items;
  const [sel, setSel] = React.useState(2);
  const [tag, setTag] = React.useState(null);
  const [tagAI, setTagAI] = React.useState(false);
  const [locked, setLocked] = React.useState(true);
  const [hideLeft, setHideLeft] = React.useState(false);
  const [hideRight, setHideRight] = React.useState(false);
  const cur = items.find((x) => x.id === sel) || items[0];

  const Side = React.createElement('aside', {
    style: { width: 248, flex: '0 0 248px', background: 'var(--surface)', borderRight: '1px solid var(--border)', display: 'flex', flexDirection: 'column', minHeight: 0 },
  },
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, padding: '14px 10px', minHeight: 0 } },
      // tag-first, Obsidian style: All entries, then the tag tree
      React.createElement('button', { onClick: () => setTag(null),
        style: { ...rowItem, height: 36, marginBottom: 4, background: !tag ? 'var(--sel)' : 'transparent', color: !tag ? 'var(--text)' : 'var(--text-2)' } },
        Icon.library({ s: 17, style: { color: !tag ? 'var(--accent)' : 'var(--muted)', flex: '0 0 auto' } }),
        React.createElement('span', { style: { flex: 1, textAlign: 'left' } }, 'All Entries'),
        React.createElement('span', { style: { fontSize: 11, color: 'var(--muted)', fontVariantNumeric: 'tabular-nums' } }, items.length),
      ),
      React.createElement('div', { style: { position: 'relative', display: 'flex', alignItems: 'center', gap: 4, margin: '14px 0 6px', padding: '0 10px' } },
        React.createElement('span', { style: { fontSize: 11, fontWeight: 700, letterSpacing: '.06em', textTransform: 'uppercase', color: 'var(--muted)', flex: 1 } }, 'Tags'),
        React.createElement('button', { className: 'niu-icbtn' + (tagAI ? ' on' : ''), style: { width: 22, height: 22, color: tagAI ? 'var(--accent)' : 'var(--muted)', background: tagAI ? 'var(--accent-tint)' : 'transparent' }, title: 'Auto-tag with AI', onClick: () => setTagAI((v) => !v) }, Icon.sparkle({ s: 14 })),
        React.createElement('button', { className: 'niu-icbtn', style: { width: 22, height: 22 }, title: 'New tag' }, Icon.plus({ s: 14 })),
        tagAI ? React.createElement(TagAIPopover, { onClose: () => setTagAI(false) }) : null,
      ),
      React.createElement(TagTree, { active: tag, onToggle: (t) => setTag(tag === t ? null : t) }),
    ),
    tag ? React.createElement('div', { style: { borderTop: '1px solid var(--border)', padding: '10px 14px', display: 'flex', alignItems: 'center', gap: 8 } },
      Icon.filter({ s: 14, style: { color: 'var(--accent)' } }),
      React.createElement('span', { style: { flex: 1, fontSize: 12.5, color: 'var(--text-2)' } }, 'Filtered by ', React.createElement('span', { style: { color: 'var(--accent)', fontWeight: 600 } }, tagValue(tag))),
      React.createElement('button', { className: 'niu-icbtn', style: { width: 24, height: 24 }, onClick: () => setTag(null) }, Icon.close({ s: 14 })),
    ) : null,
  );

  const List = React.createElement('section', { style: { flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0, borderRight: '1px solid var(--border)', background: 'var(--bg)' } },
    React.createElement('div', { style: { height: 52, flex: '0 0 52px', display: 'flex', alignItems: 'center', gap: 8, padding: '0 14px', borderBottom: '1px solid var(--border)' } },
      // Collapse the left tags sidebar.
      React.createElement('button', { className: 'niu-icbtn', title: hideLeft ? 'Show tags panel' : 'Hide tags panel', style: { color: hideLeft ? 'var(--accent)' : 'var(--muted)' }, onClick: () => setHideLeft((v) => !v) }, Icon.panelLeft({ s: 17 })),
      React.createElement('span', { style: { width: 1, height: 20, background: 'var(--border)', margin: '0 2px' } }),
      // New blank item / add by identifier (DOI or arXiv).
      React.createElement('button', { className: 'niu-icbtn', title: 'New item' }, Icon.plus({ s: 18 })),
      React.createElement('button', { className: 'niu-icbtn', title: 'Add by identifier (DOI / arXiv)' }, Icon.link({ s: 18 })),
      React.createElement('div', { className: 'niu-search' }, Icon.search({ s: 16 }), React.createElement('input', { placeholder: 'Search all fields & tags', defaultValue: '' })),
      // Collapse the right details panel.
      React.createElement('button', { className: 'niu-icbtn', title: hideRight ? 'Show details panel' : 'Hide details panel', style: { color: hideRight ? 'var(--accent)' : 'var(--muted)' }, onClick: () => setHideRight((v) => !v) }, Icon.panelRight({ s: 17 })),
    ),
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', padding: '0 16px', height: 34, flex: '0 0 34px', borderBottom: '1px solid var(--border-2)', fontSize: 11, fontWeight: 700, letterSpacing: '.04em', textTransform: 'uppercase', color: 'var(--muted)' } },
      React.createElement('span', { style: { flex: 1 } }, 'Title'),
      React.createElement('span', { style: { width: 110, display: 'flex', alignItems: 'center', gap: 4, color: 'var(--accent)' } }, 'Creator', Icon.chevDown({ s: 13 })),
      React.createElement('span', { style: { width: 48 } }, 'Year'),
      React.createElement('span', { style: { width: 22 } }),
    ),
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0 } },
      items.filter((it) => !tag || it.tags.includes(tag)).map((it) => {
        const tm = typeMeta(it.type); const on = it.id === sel;
        return React.createElement('div', {
          key: it.id, onClick: () => setSel(it.id),
          style: { display: 'flex', alignItems: 'center', gap: 11, padding: '0 16px', height: 56, cursor: 'pointer', borderBottom: '1px solid var(--border-2)', background: on ? 'var(--sel)' : 'transparent', boxShadow: on ? 'inset 3px 0 0 var(--sel-line)' : 'none' },
        },
          tm.icon({ s: 18, style: { color: tm.color, flex: '0 0 auto' } }),
          React.createElement('div', { style: { flex: 1, minWidth: 0 } },
            React.createElement('div', { className: 'niu-serif', style: { fontSize: 15.5, lineHeight: 1.25, color: 'var(--text)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontWeight: on ? 500 : 400 } }, it.title),
            React.createElement('div', { style: { display: 'flex', gap: 8, marginTop: 3 } }, it.tags.slice(0, 3).map((t) => React.createElement('span', { key: t, style: { fontSize: 10.5, fontWeight: 600, color: tagColor(t) } }, '#' + tagValue(t)))),
          ),
          React.createElement('span', { style: { width: 110, fontSize: 13, color: 'var(--text-2)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, it.creator),
          React.createElement('span', { style: { width: 48, fontSize: 13, color: 'var(--text-2)', fontVariantNumeric: 'tabular-nums' } }, it.year),
          React.createElement('span', { style: { width: 22, color: it.pdf ? tm.color : 'var(--faint)' } }, it.pdf ? Icon.attach({ s: 16 }) : null),
        );
      }),
    ),
  );

  const tm = typeMeta(cur.type);
  const Detail = React.createElement('aside', { style: { width: 384, flex: '0 0 384px', background: 'var(--surface)', display: 'flex', flexDirection: 'column', minHeight: 0 } },
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, padding: '16px 22px 20px' } },
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 } },
        React.createElement('div', { style: { display: 'inline-flex', alignItems: 'center', gap: 6, padding: '3px 9px', borderRadius: 6, background: 'var(--accent-tint)', color: 'var(--accent)', fontSize: 11, fontWeight: 700, letterSpacing: '.03em', textTransform: 'uppercase' } }, tm.icon({ s: 13 }), tm.label),
        React.createElement('div', { style: { flex: 1 } }),
        React.createElement('span', { style: { fontSize: 11, color: 'var(--muted)', fontWeight: 600 } }, locked ? 'Locked' : 'Editing'),
        React.createElement(LockBtn, { locked, onToggle: () => setLocked((v) => !v) }),
      ),
      React.createElement(EditField, { tag: 'h1', locked, value: cur.title, className: 'niu-serif', style: { display: 'block', fontSize: 24, lineHeight: 1.24, margin: '4px 0 8px', fontWeight: 500, letterSpacing: '-.01em', textWrap: 'pretty' } }),
      React.createElement(AuthorsField, { authors: cur.authors, locked: locked }),
      metaRow('Publication', cur.fullVenue, false, locked),
      metaRow('Year', String(cur.year), false, locked),
      metaRow('DOI', cur.doi || '—', true, locked),
      React.createElement('div', { style: { display: 'flex', alignItems: 'baseline', gap: 14, padding: '9px 0', borderTop: '1px solid var(--border-2)' } },
        React.createElement('span', { style: metaKey }, 'Citation Key'),
        React.createElement('span', { className: 'niu-mono', style: { fontSize: 12, color: 'var(--accent)', wordBreak: 'break-all' } }, cur.cite),
      ),
      React.createElement('div', { style: { marginTop: 18, marginBottom: 8, fontSize: 11, fontWeight: 700, letterSpacing: '.05em', textTransform: 'uppercase', color: 'var(--muted)' } }, 'Abstract'),
      React.createElement(EditField, { tag: 'p', locked, value: cur.abstract, style: { fontSize: 13.5, lineHeight: 1.62, color: 'var(--text-2)', margin: 0, textWrap: 'pretty' } }),
      React.createElement('div', { style: { marginTop: 18, marginBottom: 9, fontSize: 11, fontWeight: 700, letterSpacing: '.05em', textTransform: 'uppercase', color: 'var(--muted)' } }, 'Tags'),
      React.createElement('div', { style: { display: 'flex', flexWrap: 'wrap', gap: 7 } },
        cur.tags.map((t) => TagChip(t, { full: true }))),
    ),
    React.createElement('div', { style: { padding: '4px 16px 14px', display: 'flex', flexDirection: 'column', gap: 8, maxWidth: 248 } },
      // Footer actions: Cite + open-source-link on row 1, BibTeX on row 2.
      React.createElement('div', { style: { display: 'flex', gap: 8 } },
        React.createElement('button', { className: 'niu-btn pri', style: { flex: 1, justifyContent: 'center' } }, Icon.quote({ s: 16 }), 'Cite'),
        React.createElement('button', { className: 'niu-icbtn', style: { border: '1px solid var(--border)', flex: '0 0 auto' }, title: 'Open link' }, Icon.link({ s: 16 })),
      ),
      React.createElement('button', { className: 'niu-btn', style: { width: '100%', justifyContent: 'center' }, title: 'Copy BibTeX' }, Icon.book({ s: 16 }), 'BibTeX'),
    ),
  );

  return React.createElement(React.Fragment, null, hideLeft ? null : Side, List, hideRight ? null : Detail);
}

function DirectionA({ theme, setTheme }) {
  return React.createElement(WindowChrome, { theme, setTheme, libName: 'BibVault', active: 'library', entries: 184 },
    React.createElement(LibraryViewA, { theme, setTheme }));
}

function metaRow(k, v, accent, locked) {
  return React.createElement('div', { style: { display: 'flex', alignItems: 'baseline', gap: 14, padding: '9px 0', borderTop: '1px solid var(--border-2)' } },
    React.createElement('span', { style: metaKey }, k),
    React.createElement(EditField, { tag: 'span', locked: locked == null ? true : locked, value: v, style: { fontSize: 13, color: accent ? 'var(--accent)' : 'var(--text)', flex: 1 } }),
  );
}

// AI auto-tag popover anchored under the Tags header sparkle button
function TagAIPopover({ onClose }) {
  const [phase, setPhase] = React.useState('ask'); // ask -> scanning -> result
  React.useEffect(() => {
    if (phase !== 'scanning') return;
    const t = setTimeout(() => setPhase('result'), 1500);
    return () => clearTimeout(t);
  }, [phase]);

  const head = React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 8, marginBottom: 10 } },
    React.createElement('span', { style: { width: 24, height: 24, borderRadius: 7, background: 'var(--accent-tint)', color: 'var(--accent)', display: 'flex', alignItems: 'center', justifyContent: 'center', flex: '0 0 auto' } }, Icon.sparkle({ s: 15 })),
    React.createElement('span', { style: { fontSize: 13.5, fontWeight: 700, color: 'var(--text)', flex: 1 } }, 'AI tagging'),
    React.createElement('button', { className: 'niu-icbtn', style: { width: 22, height: 22 }, onClick: onClose }, Icon.close({ s: 14 })),
  );

  let body;
  if (phase === 'ask') {
    body = React.createElement(React.Fragment, null,
      React.createElement('p', { style: { fontSize: 12.5, lineHeight: 1.55, color: 'var(--text-2)', margin: '0 0 12px' } },
        'Scan untagged entries and suggest tags from your existing ', React.createElement('span', { className: 'niu-mono', style: { color: 'var(--accent)' } }, 'topics:'), ' / ', React.createElement('span', { className: 'niu-mono', style: { color: 'var(--accent)' } }, 'wf:'), ' vocabulary?'),
      React.createElement('label', { style: { display: 'flex', alignItems: 'center', gap: 8, fontSize: 12.5, color: 'var(--text-2)', marginBottom: 13, cursor: 'pointer' } },
        React.createElement('input', { type: 'checkbox', style: { accentColor: 'var(--accent)', width: 14, height: 14 } }),
        'Auto-apply on new imports'),
      React.createElement('div', { style: { display: 'flex', gap: 8 } },
        React.createElement('button', { className: 'niu-btn', style: { flex: 1, justifyContent: 'center', height: 30 }, onClick: onClose }, 'Not now'),
        React.createElement('button', { className: 'niu-btn pri', style: { flex: 1, justifyContent: 'center', height: 30 }, onClick: () => setPhase('scanning') }, 'Suggest tags')),
    );
  } else if (phase === 'scanning') {
    body = React.createElement('div', { style: { padding: '6px 0 4px' } },
      React.createElement('div', { style: { fontSize: 12.5, color: 'var(--text-2)', marginBottom: 9, display: 'flex', alignItems: 'center', gap: 7 } },
        React.createElement('span', { className: 'niu-spin', style: { width: 13, height: 13, border: '2px solid var(--border)', borderTopColor: 'var(--accent)', borderRadius: '50%', display: 'inline-block' } }),
        'Analyzing 14 untagged entries…'),
      React.createElement('div', { style: { height: 5, borderRadius: 3, background: 'var(--surface-2)', overflow: 'hidden' } },
        React.createElement('div', { className: 'niu-prog', style: { height: '100%', background: 'var(--accent)', borderRadius: 3 } })),
    );
  } else {
    const sugg = [
      { t: 'topics:sae', n: 6 }, { t: 'topics:unlearning', n: 4 }, { t: 'wf:to-cite', n: 3 },
    ];
    body = React.createElement(React.Fragment, null,
      React.createElement('p', { style: { fontSize: 12.5, lineHeight: 1.5, color: 'var(--text-2)', margin: '0 0 10px' } },
        React.createElement('strong', { style: { color: 'var(--text)' } }, '13 suggestions'), ' across 14 entries:'),
      React.createElement('div', { style: { display: 'flex', flexWrap: 'wrap', gap: 6, marginBottom: 13 } },
        sugg.map((s) => React.createElement('span', { key: s.t, className: 'niu-tag', style: { background: 'var(--surface-2)', color: 'var(--text)' } },
          React.createElement('span', { className: 'niu-tagdot', style: { background: tagColor(s.t) } }), tagValue(s.t),
          React.createElement('span', { style: { color: 'var(--muted)', fontWeight: 700, marginLeft: 2 } }, '×' + s.n)))),
      React.createElement('div', { style: { display: 'flex', gap: 8 } },
        React.createElement('button', { className: 'niu-btn', style: { flex: 1, justifyContent: 'center', height: 30 }, onClick: onClose }, 'Review each'),
        React.createElement('button', { className: 'niu-btn pri', style: { flex: 1, justifyContent: 'center', height: 30 }, onClick: onClose }, 'Apply all')),
    );
  }

  return React.createElement('div', { style: { position: 'absolute', top: 30, right: 6, left: 6, zIndex: 40, background: 'var(--raise)', border: '1px solid var(--border)', borderRadius: 12, boxShadow: 'var(--shadow-lg)', padding: '13px 14px' } },
    head, body);
}

window.DirectionA = DirectionA;
window.LibraryViewA = LibraryViewA;
