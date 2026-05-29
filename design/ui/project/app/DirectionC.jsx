// Direction C — "Pipeline Board": kanban by reading status + slide-in detail drawer.
//
// ============================================================================
// BUTTON REFERENCE — Board library view
// ----------------------------------------------------------------------------
// Board columns (To Read / Reading / Read):
//   • column "+" icon     — add a paper to that status column (mock).
//   • "+ Add paper" row   — add a paper to the column (dashed button, mock).
//   • cards               — click to open the detail drawer for that entry.
// Content header:
//   • search box          — search & filter the board.
//   • grid / rows toggle  — board (grid) vs list layout (grid active).
//   • Add (primary)       — add a new entry.
// Detail drawer (opens on card click):
//   • LockBtn             — toggle edit/lock for the drawer fields.
//   • close (✕)           — close the drawer (setOpen(null)).
//   • Cite / BibTeX / link— cite, copy BibTeX, open source URL.
// ============================================================================
function LibraryViewC({ theme, setTheme }) {
  const items = window.NIU.items;
  const [open, setOpen] = React.useState(null);
  const [locked, setLocked] = React.useState(true);
  const cur = items.find((x) => x.id === open);

  const cols = [
    { key: 'unread', label: 'To Read', color: '#888B82' },
    { key: 'reading', label: 'Reading', color: '#B6792B' },
    { key: 'done', label: 'Read', color: '#1F8A5B' },
  ];

  const Card = (it) => {
    const itm = typeMeta(it.type);
    return React.createElement('div', {
      key: it.id, onClick: () => setOpen(it.id),
      style: { background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 13, padding: '13px 14px', marginBottom: 11, cursor: 'pointer', boxShadow: open === it.id ? '0 0 0 2px var(--accent)' : 'var(--shadow)', transition: 'box-shadow .12s,transform .12s' },
    },
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 9 } },
        React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 5, fontSize: 10.5, fontWeight: 700, letterSpacing: '.04em', textTransform: 'uppercase', color: itm.color } }, itm.icon({ s: 13 }), it.venue + " '" + String(it.year).slice(2)),
        React.createElement('span', { style: { color: it.pdf ? 'var(--accent)' : 'var(--faint)', display: 'flex' } }, it.pdf ? Icon.attach({ s: 15 }) : Icon.doc({ s: 15 })),
      ),
      React.createElement('div', { className: 'niu-serif', style: { fontSize: 16.5, lineHeight: 1.27, color: 'var(--text)', fontWeight: 500, marginBottom: 8, letterSpacing: '-.005em' } }, it.title),
      React.createElement('div', { style: { fontSize: 12.5, color: 'var(--text-2)', marginBottom: 11 } }, it.creator),
      React.createElement('div', { style: { display: 'flex', flexWrap: 'wrap', gap: 6, alignItems: 'center' } },
        it.tags.slice(0, 3).map((t) => TagChip(t, { style: { height: 20, background: 'var(--surface-2)', color: 'var(--text-2)' } })),
        React.createElement('div', { style: { flex: 1 } }),
        React.createElement('span', { style: { display: 'flex', gap: 2, color: 'var(--accent)' } }, Array.from({ length: it.stars }).map((_, i) => React.createElement('span', { key: i, style: { width: 5, height: 5, borderRadius: '50%', background: 'var(--accent)', display: 'inline-block' } }))),
      ),
    );
  };

  const Board = React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, display: 'flex', gap: 18, padding: '18px 22px', alignItems: 'flex-start', overflowX: 'auto' } },
    cols.map((c) => {
      const list = items.filter((it) => it.status === c.key);
      return React.createElement('div', { key: c.key, style: { flex: '1 1 0', minWidth: 270, display: 'flex', flexDirection: 'column', maxHeight: '100%' } },
        React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 8, padding: '0 4px 12px' } },
          React.createElement('span', { style: { width: 9, height: 9, borderRadius: '50%', background: c.color } }),
          React.createElement('span', { style: { fontSize: 13.5, fontWeight: 700, color: 'var(--text)', letterSpacing: '.01em' } }, c.label),
          React.createElement('span', { style: { fontSize: 12, color: 'var(--muted)', fontVariantNumeric: 'tabular-nums' } }, list.length),
          React.createElement('div', { style: { flex: 1 } }),
          React.createElement('button', { className: 'niu-icbtn', style: { width: 26, height: 26 } }, Icon.plus({ s: 15 })),
        ),
        React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, padding: '2px 2px 4px', overflowY: 'auto' } }, list.map(Card),
          React.createElement('button', { style: { width: '100%', padding: '11px', borderRadius: 11, border: '1px dashed var(--border)', background: 'transparent', color: 'var(--muted)', fontFamily: 'var(--sans)', fontSize: 12.5, fontWeight: 600, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6 } }, Icon.plus({ s: 14 }), 'Add paper'),
        ),
      );
    }),
  );

  const tm = cur ? typeMeta(cur.type) : null;
  const Drawer = cur ? React.createElement('div', { style: { position: 'absolute', top: 0, right: 0, bottom: 0, width: 400, background: 'var(--surface)', borderLeft: '1px solid var(--border)', boxShadow: 'var(--shadow-lg)', display: 'flex', flexDirection: 'column', zIndex: 20 } },
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '14px 16px', borderBottom: '1px solid var(--border)' } },
      React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 6, padding: '4px 10px', borderRadius: 7, background: 'var(--accent-tint)', color: 'var(--accent)', fontSize: 11, fontWeight: 700, letterSpacing: '.03em', textTransform: 'uppercase' } }, tm.icon({ s: 13 }), tm.label),
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 6 } },
        React.createElement(LockBtn, { locked, onToggle: () => setLocked((v) => !v) }),
        React.createElement('button', { className: 'niu-icbtn', onClick: () => setOpen(null) }, React.createElement('svg', { width: 18, height: 18, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8, strokeLinecap: 'round' }, React.createElement('path', { d: 'M6 6l12 12M18 6L6 18' }))),
      ),
    ),
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, padding: '20px 20px' } },
      React.createElement(EditField, { tag: 'h1', locked, value: cur.title, className: 'niu-serif', style: { display: 'block', fontSize: 25, lineHeight: 1.2, margin: '0 0 10px', fontWeight: 500, letterSpacing: '-.01em' } }),
      React.createElement(AuthorsField, { authors: cur.authors, locked: locked, lockedStyle: { fontSize: 14, color: 'var(--text-2)', marginBottom: 4 } }),
      React.createElement(EditField, { tag: 'div', locked, value: cur.fullVenue + ' · ' + cur.year, style: { display: 'block', fontSize: 13, color: 'var(--muted)', marginBottom: 20 } }),
      React.createElement('div', { style: { fontSize: 11, fontWeight: 700, letterSpacing: '.05em', textTransform: 'uppercase', color: 'var(--muted)', marginBottom: 9 } }, 'Abstract'),
      React.createElement(EditField, { tag: 'p', locked, value: cur.abstract, style: { fontSize: 14, lineHeight: 1.62, color: 'var(--text-2)', margin: '0 0 20px', textWrap: 'pretty' } }),
      React.createElement('div', { style: { display: 'flex', alignItems: 'baseline', gap: 12, padding: '9px 0', borderTop: '1px solid var(--border-2)' } }, React.createElement('span', { style: metaKey }, 'Citation Key'), React.createElement(EditField, { tag: 'span', locked, value: cur.cite, className: 'niu-mono', style: { fontSize: 12, color: 'var(--accent)', wordBreak: 'break-all' } })),
      React.createElement('div', { style: { display: 'flex', alignItems: 'baseline', gap: 12, padding: '9px 0', borderTop: '1px solid var(--border-2)' } }, React.createElement('span', { style: metaKey }, 'DOI'), React.createElement(EditField, { tag: 'span', locked, value: cur.doi || '—', style: { fontSize: 13, color: 'var(--accent)' } })),
      React.createElement('div', { style: { marginTop: 16, display: 'flex', flexWrap: 'wrap', gap: 7 } }, cur.tags.map((t) => TagChip(t, { full: true }))),
    ),
    React.createElement('div', { style: { borderTop: '1px solid var(--border)', padding: '12px 16px', display: 'flex', gap: 8 } },
      React.createElement('button', { className: 'niu-btn pri', style: { flex: 1, justifyContent: 'center' } }, Icon.quote({ s: 16 }), 'Cite'),
      React.createElement('button', { className: 'niu-btn' }, 'BibTeX'),
      React.createElement('button', { className: 'niu-icbtn', style: { border: '1px solid var(--border)' } }, Icon.link({ s: 16 })),
    ),
  ) : null;

  const Content = React.createElement('section', { style: { flex: 1, minWidth: 0, position: 'relative', display: 'flex', flexDirection: 'column', background: 'var(--bg)' } },
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 12, padding: '13px 22px', borderBottom: '1px solid var(--border)' } },
      React.createElement('div', null,
        React.createElement('div', { style: { fontSize: 17, fontWeight: 700, letterSpacing: '-.01em', display: 'flex', alignItems: 'center', gap: 8 } }, 'Sparse Autoencoders', React.createElement('span', { style: { fontSize: 12, fontWeight: 600, color: 'var(--muted)', background: 'var(--surface-2)', padding: '2px 8px', borderRadius: 20 } }, '18')),
      ),
      React.createElement('div', { style: { flex: 1 } }),
      React.createElement('div', { className: 'niu-search', style: { maxWidth: 300, height: 34 } }, Icon.search({ s: 16 }), React.createElement('input', { placeholder: 'Search & filter' })),
      React.createElement('div', { style: { display: 'flex', background: 'var(--surface-2)', borderRadius: 9, padding: 3, gap: 2 } },
        React.createElement('button', { className: 'niu-icbtn', style: { width: 30, height: 28, background: 'var(--surface)', color: 'var(--accent)', boxShadow: 'var(--shadow)' } }, Icon.grid({ s: 16 })),
        React.createElement('button', { className: 'niu-icbtn', style: { width: 30, height: 28 } }, Icon.rows({ s: 16 })),
      ),
      React.createElement('button', { className: 'niu-btn pri' }, Icon.plus({ s: 16 }), 'Add'),
    ),
    Board,
    Drawer,
  );

  return React.createElement(React.Fragment, null, Content);
}

function DirectionC({ theme, setTheme }) {
  return React.createElement(WindowChrome, { theme, setTheme, libName: 'BibVault', active: 'library', entries: 184 },
    React.createElement(LibraryViewC, { theme, setTheme }));
}

window.DirectionC = DirectionC;
window.LibraryViewC = LibraryViewC;
