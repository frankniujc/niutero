// Settings tab — library config + appearance (theme control is live).
//
// ============================================================================
// BUTTON REFERENCE — Settings tool
// ----------------------------------------------------------------------------
// SubNav: Library / Workflow / Appearance / Keymap / Sync & sharing / Integrations.
// Library:   library-name + citation-key-pattern text inputs; profile dropdown.
// Workflow:  "Enrich on import" toggle, "Auto-commit" toggle, duplicate-policy segmented.
// Appearance:Theme segmented (live), accent-colour swatches, density segmented, font dropdowns.
// Sync:      git-remote input, "Push after commit" toggle, "Create share link" button.
// Keymap / Integrations: placeholder ("coming soon").
// ============================================================================
function stRow(title, desc, control, last) {
  return React.createElement('div', { style: { display: 'flex', alignItems: 'flex-start', gap: 32, padding: '20px 0', borderBottom: last ? 'none' : '1px solid var(--border-2)' } },
    React.createElement('div', { style: { flex: 1, minWidth: 0, maxWidth: 440 } },
      React.createElement('div', { style: { fontSize: 15, fontWeight: 700, color: 'var(--text)', marginBottom: 4 } }, title),
      React.createElement('div', { style: { fontSize: 13, lineHeight: 1.55, color: 'var(--muted)', textWrap: 'pretty' } }, desc)),
    React.createElement('div', { style: { flex: '0 0 320px', display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 8 } }, control),
  );
}
const stInput = { width: '100%', height: 36, padding: '0 12px', borderRadius: 9, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text)', font: '500 13.5px var(--sans)', outline: 'none' };
function stToggle(on, onClick) {
  return React.createElement('button', { onClick, style: { width: 42, height: 25, borderRadius: 20, border: 'none', cursor: 'pointer', background: on ? 'var(--accent)' : 'var(--faint)', position: 'relative', transition: 'background .15s' } },
    React.createElement('span', { style: { position: 'absolute', top: 3, left: on ? 20 : 3, width: 19, height: 19, borderRadius: '50%', background: '#fff', transition: 'left .15s', boxShadow: '0 1px 3px rgba(0,0,0,.3)' } }));
}
function stSegmented(opts, val, onChange) {
  return React.createElement('div', { style: { display: 'inline-flex', background: 'var(--surface-2)', borderRadius: 10, padding: 3, gap: 2 } },
    opts.map((o) => React.createElement('button', { key: o.v, onClick: () => onChange(o.v),
      style: { display: 'inline-flex', alignItems: 'center', gap: 6, height: 30, padding: '0 13px', borderRadius: 8, border: 'none', cursor: 'pointer', font: '600 13px var(--sans)', background: val === o.v ? 'var(--surface)' : 'transparent', color: val === o.v ? 'var(--accent)' : 'var(--text-2)', boxShadow: val === o.v ? 'var(--shadow)' : 'none' } },
      o.icon ? o.icon({ s: 15 }) : null, o.label)));
}

function SettingsLibrary() {
  const [name, setName] = React.useState('BibVault');
  const [pat, setPat] = React.useState('{auth}{year}{title.1}{Title.2}');
  const tokens = ['{auth}', '{year}', '{title}', '{title.N}', '{Title.N}', '{title-content-word}'];
  return React.createElement('div', null,
    React.createElement('h1', { style: stH1 }, 'Library'),
    stRow('Library name', 'A label for this library. Synced with the library; saved immediately.',
      React.createElement('input', { style: stInput, value: name, onChange: (e) => setName(e.target.value) })),
    stRow('Default profile', 'Profile applied to new entries when none is given.',
      React.createElement('div', { style: { ...stInput, display: 'flex', alignItems: 'center', justifyContent: 'space-between', color: 'var(--muted)' } }, 'None', Icon.chevDown({ s: 16 }))),
    stRow('Citation key pattern', 'Tokens take an optional .N index. Casing follows the token ({Title}\u2192Attention, {TITLE}\u2192ATTENTION); other text is literal. Imports get this key automatically; Re-key applies it to existing entries.',
      React.createElement(React.Fragment, null,
        React.createElement('input', { className: 'niu-mono', style: { ...stInput, fontSize: 12.5 }, value: pat, onChange: (e) => setPat(e.target.value) }),
        React.createElement('div', { style: { display: 'flex', flexWrap: 'wrap', gap: 6, justifyContent: 'flex-end' } },
          tokens.map((t) => React.createElement('span', { key: t, className: 'niu-mono', style: { fontSize: 11, color: 'var(--text-2)', background: 'var(--surface-2)', padding: '3px 7px', borderRadius: 5 } }, t))),
        React.createElement('div', { style: { fontSize: 12, color: 'var(--muted)' } }, 'Example: ', React.createElement('span', { className: 'niu-mono', style: { color: 'var(--accent)' } }, 'vaswani2017attentionIsAll')),
      )),
    stRow('Schema version', 'The .niutero config format version. Read-only.',
      React.createElement('span', { className: 'niu-mono', style: { fontSize: 14, color: 'var(--muted)' } }, '1'), true),
  );
}

function SettingsAppearance({ theme, setTheme }) {
  const [density, setDensity] = React.useState('comfortable');
  const [accent, setAccent] = React.useState('#1F8A5B');
  const swatches = ['#1F8A5B', '#2A6FDB', '#D97757', '#7C5CD9', '#B91C1C'];
  return React.createElement('div', null,
    React.createElement('h1', { style: stH1 }, 'Appearance'),
    stRow('Theme', 'Light, dark, or follow the system. Changes apply instantly.',
      stSegmented([{ v: 'light', label: 'Light', icon: Icon.sun }, { v: 'dark', label: 'Dark', icon: Icon.moon }], theme, setTheme)),
    stRow('Accent color', 'Used for selection, links, and primary actions.',
      React.createElement('div', { style: { display: 'flex', gap: 9 } },
        swatches.map((c) => React.createElement('button', { key: c, onClick: () => setAccent(c), title: c,
          style: { width: 28, height: 28, borderRadius: 8, border: 'none', cursor: 'pointer', background: c, boxShadow: accent === c ? '0 0 0 2px var(--bg), 0 0 0 4px ' + c : 'none' } })))),
    stRow('Density', 'How tightly list rows are packed. Researchers with large libraries often prefer compact.',
      stSegmented([{ v: 'comfortable', label: 'Comfortable' }, { v: 'compact', label: 'Compact' }], density, setDensity)),
    stRow('Interface font', 'The sans-serif used throughout the UI.',
      React.createElement('div', { style: { ...stInput, display: 'flex', alignItems: 'center', justifyContent: 'space-between', color: 'var(--text)' } }, 'Hanken Grotesk', Icon.chevDown({ s: 16 }))),
    stRow('Reading font', 'Serif used for paper titles and abstracts.',
      React.createElement('div', { style: { ...stInput, display: 'flex', alignItems: 'center', justifyContent: 'space-between', color: 'var(--text)', fontFamily: 'var(--serif)' } }, 'Newsreader', Icon.chevDown({ s: 16 })), true),
  );
}

function SettingsWorkflow() {
  const [enrich, setEnrich] = React.useState(true);
  const [commit, setCommit] = React.useState(true);
  const [dupes, setDupes] = React.useState('ask');
  return React.createElement('div', null,
    React.createElement('h1', { style: stH1 }, 'Workflow'),
    stRow('Enrich on import', 'When the browser connector captures an entry, look up a published version automatically.',
      stToggle(enrich, () => setEnrich(!enrich))),
    stRow('Auto-commit changes', 'Commit to git after each batch of edits so the library has a full history.',
      stToggle(commit, () => setCommit(!commit))),
    stRow('On duplicate capture', 'What to do when a captured paper already exists in the library.',
      stSegmented([{ v: 'ask', label: 'Ask' }, { v: 'merge', label: 'Merge' }, { v: 'skip', label: 'Skip' }], dupes, setDupes), true),
  );
}

function SettingsSync() {
  const [autopush, setAutopush] = React.useState(true);
  return React.createElement('div', null,
    React.createElement('h1', { style: stH1 }, 'Sync & sharing'),
    stRow('Git remote', 'The repository your .bib library is committed and pushed to.',
      React.createElement('input', { className: 'niu-mono', style: { ...stInput, fontSize: 12.5 }, defaultValue: 'git@github.com:lab/bibvault.git' })),
    stRow('Push after commit', 'Automatically push to the remote after each auto-commit.',
      stToggle(autopush, () => setAutopush(!autopush))),
    stRow('Browser connector', 'Local port the capture extension talks to. Currently connected.',
      React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 8 } },
        React.createElement('span', { style: { width: 8, height: 8, borderRadius: '50%', background: 'var(--accent)' } }),
        React.createElement('span', { className: 'niu-mono', style: { fontSize: 13, color: 'var(--text-2)' } }, '127.0.0.1:23510'))),
    stRow('Share library', 'Generate a read-only link to the current state of the library.',
      React.createElement('button', { className: 'niu-btn' }, Icon.link({ s: 15 }), 'Create share link'), true),
  );
}

function SettingsStub({ title, note }) {
  return React.createElement('div', null,
    React.createElement('h1', { style: stH1 }, title),
    React.createElement('div', { style: { fontSize: 14, color: 'var(--muted)', padding: '20px 0' } }, note));
}

const stH1 = { fontSize: 26, fontWeight: 700, letterSpacing: '-.02em', margin: '0 0 8px', color: 'var(--text)' };

function SettingsTab({ theme, setTheme }) {
  const [view, setView] = React.useState('library');
  const nav = [
    { id: 'library', label: 'Library', icon: Icon.library },
    { id: 'workflow', label: 'Workflow', icon: Icon.refresh },
    { id: 'appearance', label: 'Appearance', icon: Icon.sun },
    { id: 'keymap', label: 'Keymap', icon: Icon.more },
    { id: 'sync', label: 'Sync & sharing', icon: Icon.sync },
    { id: 'integrations', label: 'Integrations', icon: Icon.link },
  ];
  const header = React.createElement('div', { className: 'niu-search', style: { height: 34, flex: '0 0 34px', marginBottom: 12 } }, Icon.search({ s: 15 }), React.createElement('input', { placeholder: 'Search settings\u2026' }));
  const body = view === 'appearance' ? React.createElement(SettingsAppearance, { theme, setTheme })
    : view === 'workflow' ? React.createElement(SettingsWorkflow)
    : view === 'sync' ? React.createElement(SettingsSync)
    : view === 'keymap' ? React.createElement(SettingsStub, { title: 'Keymap', note: 'Keyboard shortcuts — coming soon.' })
    : view === 'integrations' ? React.createElement(SettingsStub, { title: 'Integrations', note: 'Zotero import, Obsidian, and Overleaf connectors — coming soon.' })
    : React.createElement(SettingsLibrary);
  return React.createElement('section', { style: { flex: 1, display: 'flex', minWidth: 0, background: 'var(--bg)' } },
    React.createElement(SubNav, { items: nav, active: view, onChange: setView, header }),
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, padding: '30px 44px 48px' } },
      React.createElement('div', { style: { maxWidth: 820 } }, body)),
  );
}

window.SettingsTab = SettingsTab;
