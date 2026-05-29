// Direction B — "Focus Reader": list rail + dominant reading pane with PDF preview.
//
// ============================================================================
// BUTTON REFERENCE — Reader library view
// ----------------------------------------------------------------------------
// Left sidebar: "All Entries", "+" new tag, TagTree rows (filter), reading-status rows.
// List rail: search box, filter icon, cards (click to open in the reader).
// Reader pane header:
//   • panelLeft icon  — collapse / show the left tags sidebar (hideSide).
//   • rows icon       — collapse / show the middle card list (hideList).
//   • LockBtn         — toggle edit/lock for the reading pane fields.
//   • star icon       — favourite / add to "To Read" (mock).
//   • more (⋯) icon   — overflow menu (mock).
// Reader actions: Open PDF, Cite, Copy BibTeX, Source (open URL).
// Tags row: "+ add" attaches a tag to the entry (mock).
// ============================================================================
function LibraryViewB({ theme, setTheme }) {
  const items = window.NIU.items;
  const [sel, setSel] = React.useState(5);
  const [locked, setLocked] = React.useState(true);
  const [hideSide, setHideSide] = React.useState(false);
  const [hideList, setHideList] = React.useState(false);
  const cur = items.find((x) => x.id === sel) || items[0];
  const tm = typeMeta(cur.type);
  const sm = statusMeta(cur.status);

  // compact collections sidebar
  const Side = React.createElement('aside', { style: { width: 210, flex: '0 0 210px', background: 'var(--surface)', borderRight: '1px solid var(--border)', display: 'flex', flexDirection: 'column', padding: '14px 10px', minHeight: 0 } },
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0 } },
      React.createElement('button', { style: { ...rowItem, height: 34, marginBottom: 4, background: 'var(--sel)', color: 'var(--text)' } },
        Icon.library({ s: 16, style: { color: 'var(--accent)', flex: '0 0 auto' } }),
        React.createElement('span', { style: { flex: 1, textAlign: 'left', fontSize: 13 } }, 'All Entries'),
        React.createElement('span', { style: { fontSize: 11, color: 'var(--muted)' } }, window.NIU.items.length)),
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 6, margin: '12px 0 6px', padding: '0 10px' } },
        React.createElement('span', { style: { fontSize: 11, fontWeight: 700, letterSpacing: '.06em', textTransform: 'uppercase', color: 'var(--muted)', flex: 1 } }, 'Tags'),
        React.createElement('button', { className: 'niu-icbtn', style: { width: 22, height: 22 }, title: 'New tag' }, Icon.plus({ s: 14 }))),
      React.createElement(TagTree, { active: null, onToggle: () => {} }),
      React.createElement('div', { style: { ...secLabel, marginTop: 16 } }, 'Reading status'),
      [['Unread', '#888B82', 41], ['Reading', '#B6792B', 8], ['Read', '#1F8A5B', 135]].map(([n, c, v]) => React.createElement('div', { key: n, style: { display: 'flex', alignItems: 'center', gap: 9, padding: '6px 10px', fontSize: 13, color: 'var(--text-2)' } },
        React.createElement('span', { style: { width: 8, height: 8, borderRadius: '50%', background: c } }), React.createElement('span', { style: { flex: 1 } }, n), React.createElement('span', { style: { fontSize: 11, color: 'var(--muted)' } }, v))),
    ),
  );

  // item card list
  const List = React.createElement('section', { style: { width: 340, flex: '0 0 340px', display: 'flex', flexDirection: 'column', minWidth: 0, borderRight: '1px solid var(--border)', background: 'var(--bg)' } },
    React.createElement('div', { style: { padding: '14px 14px 10px' } },
      React.createElement('div', { className: 'niu-search', style: { height: 36 } }, Icon.search({ s: 16 }), React.createElement('input', { placeholder: 'Search Sparse Autoencoders' })),
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginTop: 11 } },
        React.createElement('span', { style: { fontSize: 12.5, color: 'var(--muted)' } }, React.createElement('strong', { style: { color: 'var(--text-2)' } }, '18'), ' items · sorted by date added'),
        React.createElement('button', { className: 'niu-icbtn' }, Icon.filter({ s: 16 })),
      ),
    ),
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, padding: '0 12px 12px' } },
      items.map((it) => {
        const itm = typeMeta(it.type); const on = it.id === sel; const ism = statusMeta(it.status);
        return React.createElement('div', {
          key: it.id, onClick: () => setSel(it.id),
          style: { padding: '12px 13px', marginBottom: 8, borderRadius: 11, cursor: 'pointer', border: '1px solid ' + (on ? 'var(--accent)' : 'var(--border)'), background: on ? 'var(--accent-tint)' : 'var(--surface)', transition: 'border-color .12s,background .12s' },
        },
          React.createElement('div', { style: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 7 } },
            React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 10.5, fontWeight: 700, letterSpacing: '.03em', textTransform: 'uppercase', color: itm.color } }, itm.icon({ s: 13 }), it.venue + ' ' + it.year),
            React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 10.5, fontWeight: 600, color: ism.color } }, React.createElement('span', { style: { width: 6, height: 6, borderRadius: '50%', background: ism.color } }), ism.label),
          ),
          React.createElement('div', { className: 'niu-serif', style: { fontSize: 16, lineHeight: 1.28, color: 'var(--text)', fontWeight: 500, marginBottom: 6, display: '-webkit-box', WebkitLineClamp: 2, WebkitBoxOrient: 'vertical', overflow: 'hidden' } }, it.title),
          React.createElement('div', { style: { fontSize: 12.5, color: 'var(--text-2)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, it.creator),
        );
      }),
    ),
  );

  // dominant reading pane
  const Reader = React.createElement('section', { className: 'niu-scroll', style: { flex: 1, minWidth: 0, background: 'var(--bg)', minHeight: 0 } },
    React.createElement('div', { style: { maxWidth: 720, margin: '0 auto', padding: '34px 44px 48px' } },
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 10, marginBottom: 16 } },
        React.createElement('button', { className: 'niu-icbtn', title: hideSide ? 'Show tags panel' : 'Hide tags panel', style: { color: hideSide ? 'var(--accent)' : 'var(--muted)', marginLeft: -6 }, onClick: () => setHideSide((v) => !v) }, Icon.panelLeft({ s: 17 })),
        React.createElement('button', { className: 'niu-icbtn', title: hideList ? 'Show list' : 'Hide list', style: { color: hideList ? 'var(--accent)' : 'var(--muted)' }, onClick: () => setHideList((v) => !v) }, Icon.rows({ s: 17 })),
        React.createElement('span', { style: { width: 1, height: 20, background: 'var(--border)', margin: '0 2px' } }),
        React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 6, padding: '4px 10px', borderRadius: 7, background: 'var(--accent-tint)', color: 'var(--accent)', fontSize: 11.5, fontWeight: 700, letterSpacing: '.03em', textTransform: 'uppercase' } }, tm.icon({ s: 14 }), tm.label),
        React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 6, fontSize: 12, fontWeight: 600, color: sm.color } }, React.createElement('span', { style: { width: 7, height: 7, borderRadius: '50%', background: sm.color } }), sm.label),
        React.createElement('div', { style: { flex: 1 } }),
        React.createElement('span', { style: { fontSize: 11, color: 'var(--muted)', fontWeight: 600, marginRight: 2 } }, locked ? 'Locked' : 'Editing'),
        React.createElement(LockBtn, { locked, onToggle: () => setLocked((v) => !v) }),
        React.createElement('button', { className: 'niu-icbtn', style: { border: '1px solid var(--border)' } }, Icon.star({ s: 16 })),
        React.createElement('button', { className: 'niu-icbtn', style: { border: '1px solid var(--border)' } }, Icon.more({ s: 16 })),
      ),
      React.createElement(EditField, { tag: 'h1', locked, value: cur.title, className: 'niu-serif', style: { display: 'block', fontSize: 33, lineHeight: 1.16, margin: '0 0 12px', fontWeight: 500, letterSpacing: '-.015em', textWrap: 'balance' } }),
      React.createElement(AuthorsField, { authors: cur.authors, locked: locked, lockedStyle: { fontSize: 15, color: 'var(--text-2)', marginBottom: 6 } }),
      React.createElement(EditField, { tag: 'div', locked, value: cur.fullVenue + ' · ' + cur.year, style: { display: 'block', fontSize: 13.5, color: 'var(--muted)' } }),
      React.createElement('div', { style: { display: 'flex', gap: 9, margin: '22px 0 28px' } },
        React.createElement('button', { className: 'niu-btn pri' }, Icon.book({ s: 16 }), 'Open PDF'),
        React.createElement('button', { className: 'niu-btn' }, Icon.quote({ s: 16 }), 'Cite'),
        React.createElement('button', { className: 'niu-btn' }, 'Copy BibTeX'),
        React.createElement('button', { className: 'niu-btn' }, Icon.link({ s: 16 }), 'Source'),
      ),
      // PDF preview placeholder
      React.createElement('div', { style: { display: 'flex', gap: 22, marginBottom: 30 } },
        React.createElement('div', { style: { flex: '0 0 168px', height: 218, borderRadius: 8, border: '1px solid var(--border)', background: 'repeating-linear-gradient(135deg, var(--surface-2) 0 9px, transparent 9px 18px), var(--surface)', display: 'flex', alignItems: 'flex-end', justifyContent: 'center', padding: 10, position: 'relative', overflow: 'hidden' } },
          React.createElement('span', { className: 'niu-mono', style: { fontSize: 10, color: 'var(--muted)', background: 'var(--bg)', padding: '3px 7px', borderRadius: 5 } }, 'pdf-page-1.png'),
        ),
        React.createElement('div', { style: { flex: 1 } },
          React.createElement('div', { style: { fontSize: 11, fontWeight: 700, letterSpacing: '.05em', textTransform: 'uppercase', color: 'var(--muted)', marginBottom: 10 } }, 'Abstract'),
          React.createElement(EditField, { tag: 'p', locked, value: cur.abstract, style: { fontSize: 15, lineHeight: 1.68, color: 'var(--text)', margin: 0, textWrap: 'pretty', fontFamily: 'var(--serif)' } }),
        ),
      ),
      React.createElement('div', { style: { height: 1, background: 'var(--border)', marginBottom: 22 } }),
      React.createElement('div', { style: { display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '14px 40px' } },
        bMeta('Citation key', cur.cite, true, locked), bMeta('DOI', cur.doi || '—', false, locked),
        bMeta('Added', cur.added, false, locked), bMeta('Type', tm.label, false, true),
      ),
      React.createElement('div', { style: { marginTop: 24, display: 'flex', flexWrap: 'wrap', gap: 8, alignItems: 'center' } },
        React.createElement('span', { style: { fontSize: 11, fontWeight: 700, letterSpacing: '.05em', textTransform: 'uppercase', color: 'var(--muted)', marginRight: 4 } }, 'Tags'),
        cur.tags.map((t) => React.createElement('span', { key: t, className: 'niu-tag', style: { background: 'var(--surface)', border: '1px solid var(--border)', color: 'var(--text)', height: 24 } }, React.createElement('span', { className: 'niu-tagdot', style: { background: tagColor(t) } }), React.createElement('span', { style: { color: 'var(--muted)', fontWeight: 600 } }, tagNs(t) + ':'), tagValue(t))),
        React.createElement('button', { className: 'niu-tag', style: { background: 'transparent', border: '1px dashed var(--border)', color: 'var(--muted)', height: 24, cursor: 'pointer' } }, '+ add'),
      ),
    ),
  );

  return React.createElement(React.Fragment, null, hideSide ? null : Side, hideList ? null : List, Reader);
}

function DirectionB({ theme, setTheme }) {
  return React.createElement(WindowChrome, { theme, setTheme, libName: 'BibVault', active: 'library', entries: 184 },
    React.createElement(LibraryViewB, { theme, setTheme }));
}

function bMeta(k, v, mono, locked) {
  return React.createElement('div', null,
    React.createElement('div', { style: { fontSize: 11, fontWeight: 700, letterSpacing: '.04em', textTransform: 'uppercase', color: 'var(--muted)', marginBottom: 4 } }, k),
    React.createElement(EditField, { tag: 'div', locked: locked == null ? true : locked, value: v, className: mono ? 'niu-mono' : '', style: { fontSize: mono ? 12.5 : 14, color: mono ? 'var(--accent)' : 'var(--text)', wordBreak: 'break-all' } }),
  );
}

window.DirectionB = DirectionB;
window.LibraryViewB = LibraryViewB;
