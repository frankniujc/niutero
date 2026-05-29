// Niutero — shared foundations: design tokens, theming, icons, app shell.
// Exports components to window for the direction files.
//
// ============================================================================
// BUTTON / CONTROL REFERENCE (shared building blocks)
// ----------------------------------------------------------------------------
// ToolRail (left rail, every screen):
//   • NiuMark (logo)         — brand mark, non-interactive.
//   • Library  rail button   — switch to the Library tool (onNav('library')).
//   • Normalize rail button  — switch to the Normalize tool (onNav('normalize')).
//   • AI Assistant rail btn  — switch to the AI Assistant tool (onNav('ai')).
//   • Settings rail button   — switch to Settings (onNav('settings')).
//   • Sync button (bottom)   — push/pull the git-backed library (decorative here).
// WindowChrome titlebar:
//   • 3 traffic-light dots   — macOS window close/min/zoom (decorative).
//   • Theme toggle (sun/moon)— flip light <-> dark for the whole app.
//   • `center` slot          — hosts the Library view switcher (see FullApp).
// StatusBar (bottom): read-only status — connector address, git branch,
//   modified flag, entry count. No interactive buttons.
// LockBtn: toggles a detail panel between locked (read-only) and editing.
// Segmented: generic segmented control; each option is a button -> onChange(v).
// SubNav: vertical sub-navigation list (Normalize/Settings); each row -> onChange(id).
// TagTree: group headers collapse/expand a namespace; each tag row filters by tag.
// AuthorsField (editing mode): per-author "−" removes a row; "+ Add author" appends.
// EditField: inline contentEditable text; editable only when unlocked.
// ============================================================================

(function () {
  if (document.getElementById('niu-tokens')) return;
  const s = document.createElement('style');
  s.id = 'niu-tokens';
  s.textContent = `
  @import url('https://fonts.googleapis.com/css2?family=Hanken+Grotesk:wght@400;500;600;700&family=Newsreader:ital,opsz,wght@0,6..72,400;0,6..72,500;1,6..72,400&family=JetBrains+Mono:wght@400;500&display=swap');

  .niu {
    --bg:#FAFAF8; --surface:#FFFFFF; --surface-2:#F4F4F1; --raise:#FFFFFF;
    --text:#1B1C19; --text-2:#54574F; --muted:#888B82; --faint:#B6B8B0;
    --border:rgba(20,22,18,0.09); --border-2:rgba(20,22,18,0.06);
    --accent:#1F8A5B; --accent-press:#176F49; --accent-tint:rgba(31,138,91,0.10);
    --accent-tint-2:rgba(31,138,91,0.16);
    --sel:rgba(31,138,91,0.12); --sel-line:#1F8A5B;
    --shadow:0 1px 2px rgba(20,22,18,.05), 0 8px 24px rgba(20,22,18,.06);
    --shadow-lg:0 4px 12px rgba(20,22,18,.08), 0 24px 60px rgba(20,22,18,.12);
    --sans:'Hanken Grotesk', system-ui, sans-serif;
    --serif:'Newsreader', Georgia, serif;
    --mono:'JetBrains Mono', ui-monospace, monospace;
    color:var(--text); font-family:var(--sans);
    -webkit-font-smoothing:antialiased; text-rendering:optimizeLegibility;
  }
  .niu[data-theme="dark"] {
    --bg:#121411; --surface:#191C17; --surface-2:#1F231D; --raise:#22261F;
    --text:#E9EBE4; --text-2:#AEB2A6; --muted:#7E8378; --faint:#5A5F53;
    --border:rgba(255,255,255,0.09); --border-2:rgba(255,255,255,0.05);
    --accent:#3BB178; --accent-press:#2E9866; --accent-tint:rgba(59,177,120,0.13);
    --accent-tint-2:rgba(59,177,120,0.20);
    --sel:rgba(59,177,120,0.15); --sel-line:#3BB178;
    --shadow:0 1px 2px rgba(0,0,0,.3), 0 8px 24px rgba(0,0,0,.35);
    --shadow-lg:0 4px 12px rgba(0,0,0,.4), 0 24px 60px rgba(0,0,0,.5);
  }
  .niu *{box-sizing:border-box;}
  .niu ::selection{background:var(--accent-tint-2);}
  .niu-serif{font-family:var(--serif);}
  .niu-mono{font-family:var(--mono);}
  .niu-win{width:100%;height:100%;display:flex;flex-direction:column;overflow:hidden;
    background:var(--bg);border-radius:12px;}
  /* window titlebar */
  .niu-tb{height:38px;flex:0 0 38px;display:flex;align-items:center;gap:8px;padding:0 14px;
    background:var(--surface);border-bottom:1px solid var(--border);}
  .niu-dot{width:11px;height:11px;border-radius:50%;}
  .niu-tb-title{font-size:12.5px;font-weight:600;color:var(--text-2);letter-spacing:.01em;
    margin-left:6px;display:flex;align-items:center;gap:7px;}
  .niu-body{flex:1;display:flex;min-height:0;}
  /* tool rail */
  .niu-rail{width:60px;flex:0 0 60px;background:var(--surface);border-right:1px solid var(--border);
    display:flex;flex-direction:column;align-items:center;padding:12px 0;gap:4px;}
  .niu-railbtn{width:42px;height:42px;border-radius:11px;border:none;background:transparent;
    color:var(--muted);display:flex;align-items:center;justify-content:center;cursor:pointer;
    position:relative;transition:background .14s,color .14s;}
  .niu-railbtn:hover{background:var(--surface-2);color:var(--text-2);}
  .niu-railbtn.on{background:var(--accent-tint);color:var(--accent);}
  .niu-railbtn.on::before{content:'';position:absolute;left:-12px;top:11px;bottom:11px;width:3px;
    border-radius:0 3px 3px 0;background:var(--accent);}
  .niu-raillabel{font-size:8.5px;font-weight:600;letter-spacing:.04em;text-transform:uppercase;}
  .niu-rail-tip{position:absolute;left:50px;background:var(--text);color:var(--bg);font-size:11px;
    font-weight:600;padding:4px 8px;border-radius:6px;white-space:nowrap;opacity:0;pointer-events:none;
    transform:translateX(-4px);transition:opacity .12s,transform .12s;z-index:30;}
  .niu-railbtn:hover .niu-rail-tip{opacity:1;transform:translateX(0);}
  /* generic */
  .niu-btn{height:32px;padding:0 13px;border-radius:8px;border:1px solid var(--border);
    background:var(--surface);color:var(--text);font-family:var(--sans);font-size:13px;font-weight:600;
    display:inline-flex;align-items:center;gap:7px;cursor:pointer;transition:background .12s,border-color .12s;}
  .niu-btn:hover{background:var(--surface-2);}
  .niu-btn.pri{background:var(--accent);border-color:var(--accent);color:#fff;}
  .niu-btn.pri:hover{background:var(--accent-press);}
  .niu-icbtn{width:32px;height:32px;border-radius:8px;border:1px solid transparent;background:transparent;
    color:var(--muted);display:inline-flex;align-items:center;justify-content:center;cursor:pointer;transition:background .12s,color .12s;}
  .niu-icbtn:hover{background:var(--surface-2);color:var(--text);}
  .niu-search{flex:1;height:34px;display:flex;align-items:center;gap:9px;padding:0 12px;border-radius:9px;
    background:var(--surface-2);border:1px solid transparent;color:var(--muted);transition:border-color .14s,background .14s;}
  .niu-search:focus-within{border-color:var(--accent);background:var(--surface);}
  .niu-search input{flex:1;border:none;background:transparent;outline:none;font-family:var(--sans);
    font-size:13.5px;color:var(--text);}
  .niu-search input::placeholder{color:var(--muted);}
  .niu-tag{display:inline-flex;align-items:center;gap:5px;height:21px;padding:0 8px;border-radius:6px;
    font-size:11px;font-weight:600;letter-spacing:.01em;}
  .niu-tagdot{width:6px;height:6px;border-radius:2px;}
  .niu-scroll{overflow-y:auto;}
  .niu-scroll::-webkit-scrollbar{width:9px;height:9px;}
  .niu-scroll::-webkit-scrollbar-thumb{background:var(--border);border-radius:6px;border:2px solid transparent;background-clip:padding-box;}
  .niu-scroll:hover::-webkit-scrollbar-thumb{background:var(--faint);background-clip:padding-box;}
  /* editable fields (unlocked detail panel) */
  .niu-edit{border-radius:6px;outline:none;transition:background .12s, box-shadow .12s;cursor:text;}
  .niu-edit.on{background:var(--surface-2);box-shadow:inset 0 0 0 1px var(--border);padding-left:7px;padding-right:7px;margin-left:-7px;margin-right:-7px;}
  .niu-edit.on:hover{box-shadow:inset 0 0 0 1px var(--faint);}
  .niu-edit.on:focus{background:var(--surface);box-shadow:inset 0 0 0 1.5px var(--accent);}
  .niu-edit.locked{cursor:default;}
  .niu-authdel{opacity:0;transition:opacity .12s;}
  .niu-authrow:hover .niu-authdel{opacity:1;}
  .niu-edit.on:empty::before{content:attr(data-placeholder);color:var(--faint);}
  @keyframes niu-spin{to{transform:rotate(360deg);}}
  .niu-spin{animation:niu-spin .8s linear infinite;}
  @keyframes niu-prog{0%{width:8%;}50%{width:72%;}100%{width:96%;}}
  .niu-prog{animation:niu-prog 1.5s ease-out forwards;}
  `;
  document.head.appendChild(s);
})();

// ---------- Icons (simple stroke set) ----------
const I = (p) => React.createElement('svg', {
  width: p.s || 18, height: p.s || 18, viewBox: '0 0 24 24', fill: 'none',
  stroke: 'currentColor', strokeWidth: p.w || 1.7, strokeLinecap: 'round',
  strokeLinejoin: 'round', style: p.style,
}, p.children);

const Icon = {
  library: (p={}) => I({ ...p, children: [
    React.createElement('path', { key: 1, d: 'M4 5h5v14H4zM10 5h4v14h-4zM16 6l4 1-3 12-4-1z' }),
  ]}),
  normalize: (p={}) => I({ ...p, children: [
    React.createElement('path', { key: 1, d: 'M3 21l10.5-10.5' }),
    React.createElement('path', { key: 2, d: 'M16.5 4.2L17.5 6.5L19.8 7.5L17.5 8.5L16.5 10.8L15.5 8.5L13.2 7.5L15.5 6.5Z' }),
    React.createElement('path', { key: 3, d: 'M6.4 4.5v2.2M5.3 5.6h2.2', strokeWidth: 1.3 }),
  ]}),
  ai: (p={}) => I({ ...p, children: [
    React.createElement('path', { key: 1, d: 'M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6z' }),
    React.createElement('path', { key: 2, d: 'M18 15l.8 2.2L21 18l-2.2.8L18 21l-.8-2.2L15 18l2.2-.8z', strokeWidth: 1.3 }),
  ]}),
  settings: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 12, cy: 12, r: 3 }),
    React.createElement('path', { key: 2, d: 'M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z' }),
  ]}),
  search: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 11, cy: 11, r: 7 }),
    React.createElement('path', { key: 2, d: 'M20 20l-3.5-3.5' }),
  ]}),
  plus: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M12 5v14M5 12h14' }) }),
  folder: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M3 7a2 2 0 0 1 2-2h4l2 2h6a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z' }) }),
  star: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M12 4l2.3 4.8 5.2.7-3.8 3.6.9 5.2L12 16.6 7.4 18.3l.9-5.2L4.5 9.5l5.2-.7z' }) }),
  chevron: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M9 6l6 6-6 6' }) }),
  chevDown: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M6 9l6 6 6-6' }) }),
  attach: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M21 9.5l-8.5 8.5a4 4 0 0 1-5.7-5.7l8.5-8.5a2.7 2.7 0 0 1 3.8 3.8l-8.5 8.5a1.3 1.3 0 0 1-1.9-1.9l7.8-7.8' }) }),
  tag: (p={}) => I({ ...p, children: [
    React.createElement('path', { key: 1, d: 'M3 12l8.5-8.5a2 2 0 0 1 1.4-.6H19a2 2 0 0 1 2 2v5.7a2 2 0 0 1-.6 1.4L12 20.5a2 2 0 0 1-2.8 0l-6-6a2 2 0 0 1 0-2.8z' }),
    React.createElement('circle', { key: 2, cx: 16, cy: 8, r: 1.2, fill: 'currentColor', stroke: 'none' }),
  ]}),
  quote: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M7 7H4v6h3l-1 4M17 7h-3v6h3l-1 4', strokeWidth: 1.5 }) }),
  link: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M10 14a4 4 0 0 0 6 .4l2-2a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-6-.4l-2 2a4 4 0 0 0 5.7 5.7l1-1' }) }),
  sun: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 12, cy: 12, r: 4 }),
    React.createElement('path', { key: 2, d: 'M12 2v2M12 20v2M4 12H2M22 12h-2M5 5l1.4 1.4M17.6 17.6L19 19M19 5l-1.4 1.4M6.4 17.6L5 19' }),
  ]}),
  moon: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M20 14.5A8 8 0 0 1 9.5 4a7 7 0 1 0 10.5 10.5z' }) }),
  sync: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M4 11a8 8 0 0 1 14-4.5L20 8M20 13a8 8 0 0 1-14 4.5L4 16M20 4v4h-4M4 20v-4h4' }) }),
  more: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 5, cy: 12, r: 1.4, fill: 'currentColor', stroke: 'none' }),
    React.createElement('circle', { key: 2, cx: 12, cy: 12, r: 1.4, fill: 'currentColor', stroke: 'none' }),
    React.createElement('circle', { key: 3, cx: 19, cy: 12, r: 1.4, fill: 'currentColor', stroke: 'none' }),
  ]}),
  doc: (p={}) => I({ ...p, children: [
    React.createElement('path', { key: 1, d: 'M6 3h8l4 4v14H6z' }),
    React.createElement('path', { key: 2, d: 'M14 3v4h4M9 13h6M9 17h6', strokeWidth: 1.3 }),
  ]}),
  grid: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z' }) }),
  rows: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M4 6h16M4 12h16M4 18h16' }) }),
  filter: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M3 5h18l-7 8v5l-4 2v-7z' }) }),
  check: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M5 12l5 5 9-11' }) }),
  clock: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 12, cy: 12, r: 8 }),
    React.createElement('path', { key: 2, d: 'M12 8v4l3 2' }),
  ]}),
  book: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M4 5a2 2 0 0 1 2-2h6v16H6a2 2 0 0 0-2 2zM20 5a2 2 0 0 0-2-2h-6v16h6a2 2 0 0 1 2 2z' }) }),
  send: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M5 12l15-7-6 16-3-7z' }) }),
  copy: (p={}) => I({ ...p, children: [
    React.createElement('rect', { key: 1, x: 9, y: 9, width: 11, height: 11, rx: 2 }),
    React.createElement('path', { key: 2, d: 'M5 15V5a2 2 0 0 1 2-2h8' }),
  ]}),
  warn: (p={}) => I({ ...p, children: [
    React.createElement('path', { key: 1, d: 'M12 4l9 16H3z' }),
    React.createElement('path', { key: 2, d: 'M12 10v4M12 17.5v.01', strokeWidth: 1.9 }),
  ]}),
  key: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 8, cy: 8, r: 4 }),
    React.createElement('path', { key: 2, d: 'M11 11l8 8M16 16l2-2M19 19l2-2' }),
  ]}),
  refresh: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M4 11a8 8 0 0 1 13.5-5l2.5 2.5M20 13a8 8 0 0 1-13.5 5L4 15.5M19 4v4h-4M5 20v-4h4' }) }),
  sparkle: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M12 3l1.7 5.3L19 10l-5.3 1.7L12 17l-1.7-5.3L5 10l5.3-1.7z' }) }),
  download: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M12 4v11m0 0l-4-4m4 4l4-4M5 19h14' }) }),
  info: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 12, cy: 12, r: 8.5 }),
    React.createElement('path', { key: 2, d: 'M12 11v5M12 8v.01', strokeWidth: 1.9 }),
  ]}),
  checkCircle: (p={}) => I({ ...p, children: [
    React.createElement('circle', { key: 1, cx: 12, cy: 12, r: 8.5 }),
    React.createElement('path', { key: 2, d: 'M8.5 12l2.5 2.5L16 9' }),
  ]}),
  arrowRight: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M5 12h14M13 6l6 6-6 6' }) }),
  trash: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13' }) }),
  close: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M6 6l12 12M18 6L6 18' }) }),
  chat: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M21 12a8 8 0 0 1-11.5 7.2L4 20l1-4.5A8 8 0 1 1 21 12z' }) }),
  expand: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M9 4H4v5M15 4h5v5M9 20H4v-5M15 20h5v-5' }) }),
  pause: (p={}) => I({ ...p, children: React.createElement('path', { d: 'M8 5v14M16 5v14' }) }),
  panelLeft: (p={}) => I({ ...p, children: [
    React.createElement('rect', { key: 1, x: 3, y: 4, width: 18, height: 16, rx: 2 }),
    React.createElement('path', { key: 2, d: 'M9 4v16', strokeWidth: 1.5 }),
  ]}),
  panelRight: (p={}) => I({ ...p, children: [
    React.createElement('rect', { key: 1, x: 3, y: 4, width: 18, height: 16, rx: 2 }),
    React.createElement('path', { key: 2, d: 'M15 4v16', strokeWidth: 1.5 }),
  ]}),
};

// type glyph + color
function typeMeta(t) {
  switch (t) {
    case 'conference': return { label: 'Conference Paper', short: 'CONF', icon: Icon.book, color: '#1F8A5B' };
    case 'journal': return { label: 'Journal Article', short: 'JRNL', icon: Icon.doc, color: '#2A6FDB' };
    case 'preprint': return { label: 'Preprint', short: 'PRE', icon: Icon.doc, color: '#B6792B' };
    default: return { label: 'Document', short: 'DOC', icon: Icon.doc, color: '#888' };
  }
}
// Inline-editable text. When unlocked it shows an edit affordance and is
// contentEditable; when locked it renders as static text. `tag` picks the element.
function EditField(props) {
  const { value, locked, style, className, tag, placeholder } = props;
  const T = tag || 'span';
  return React.createElement(T, {
    className: (className ? className + ' ' : '') + 'niu-edit ' + (locked ? 'locked' : 'on'),
    style: style,
    contentEditable: !locked,
    suppressContentEditableWarning: true,
    spellCheck: false,
    'data-placeholder': placeholder || '',
  }, value);
}

// Authors: compact line when locked; Zotero-style per-author rows when editing.
function AuthorsField({ authors, locked, keyStyle, lockedStyle }) {
  if (locked) {
    return React.createElement(EditField, { tag: 'div', locked: true, value: authors.join(' · '),
      style: Object.assign({ display: 'block', fontSize: 14, lineHeight: 1.45, color: 'var(--text-2)', marginBottom: 18 }, lockedStyle || {}) });
  }
  const authKey = Object.assign({ width: 92, flex: '0 0 92px', textAlign: 'right', fontSize: 12, fontWeight: 600, color: 'var(--muted)' }, keyStyle || {});
  const minus = React.createElement('svg', { width: 14, height: 14, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.8, strokeLinecap: 'round' }, React.createElement('path', { d: 'M6 12h12' }));
  return React.createElement('div', { style: { margin: '2px 0 16px' } },
    authors.map((a, i) => {
      const ci = a.indexOf(',');
      const last = ci >= 0 ? a.slice(0, ci).trim() : a;
      const first = ci >= 0 ? a.slice(ci + 1).trim() : '';
      return React.createElement('div', { key: i, className: 'niu-authrow', style: { display: 'flex', alignItems: 'center', gap: 12, padding: '3px 0' } },
        React.createElement('span', { style: authKey }, 'Author'),
        React.createElement('div', { style: { flex: 1, display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 } },
          React.createElement(EditField, { tag: 'span', locked: false, value: last, placeholder: 'Last', style: { fontSize: 13.5, color: 'var(--text)', fontWeight: 500 } }),
          React.createElement('span', { style: { color: 'var(--faint)' } }, ','),
          React.createElement(EditField, { tag: 'span', locked: false, value: first, placeholder: 'First', style: { fontSize: 13.5, color: 'var(--text)' } }),
        ),
        React.createElement('button', { className: 'niu-icbtn niu-authdel', style: { width: 24, height: 24, flex: '0 0 auto' }, title: 'Remove author' }, minus),
      );
    }),
    React.createElement('div', { style: { display: 'flex', gap: 12 } },
      React.createElement('span', { style: { width: 92, flex: '0 0 92px' } }),
      React.createElement('button', { style: { display: 'inline-flex', alignItems: 'center', gap: 5, marginTop: 4, padding: '3px 9px', border: '1px dashed var(--border)', borderRadius: 7, background: 'transparent', color: 'var(--muted)', font: '600 12px var(--sans)', cursor: 'pointer' } }, Icon.plus({ s: 13 }), 'Add author'),
    ),
  );
}

// Lock toggle for the detail panel header.
function LockBtn({ locked, onToggle }) {
  const lockIcon = locked
    ? React.createElement('svg', { width: 16, height: 16, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, strokeLinecap: 'round', strokeLinejoin: 'round' },
        React.createElement('rect', { x: 5, y: 11, width: 14, height: 9, rx: 2 }),
        React.createElement('path', { d: 'M8 11V8a4 4 0 0 1 8 0v3' }))
    : React.createElement('svg', { width: 16, height: 16, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 1.7, strokeLinecap: 'round', strokeLinejoin: 'round' },
        React.createElement('rect', { x: 5, y: 11, width: 14, height: 9, rx: 2 }),
        React.createElement('path', { d: 'M8 11V8a4 4 0 0 1 7.5-2' }));
  return React.createElement('button', {
    className: 'niu-icbtn', onClick: onToggle,
    title: locked ? 'Locked — click to edit' : 'Editing — click to lock',
    style: { color: locked ? 'var(--muted)' : 'var(--accent)', background: locked ? 'transparent' : 'var(--accent-tint)' },
  }, lockIcon);
}

function tagColor(name) {
  const t = (window.NIU.tags || []).find((x) => x.name === name);
  return t ? t.color : '#888';
}
function tagNs(name) { return name.includes(':') ? name.split(':')[0] : ''; }
function tagValue(name) { return name.includes(':') ? name.split(':').slice(1).join(':') : name; }
function tagCount(name) { return (window.NIU.items || []).filter((it) => it.tags.includes(name)).length; }
// compact tag chip: dot + value (optionally with dimmed namespace prefix)
function TagChip(name, opts) {
  opts = opts || {};
  return React.createElement('span', { key: name, className: 'niu-tag', style: Object.assign({ background: 'var(--surface-2)', color: 'var(--text)' }, opts.style) },
    React.createElement('span', { className: 'niu-tagdot', style: { background: tagColor(name) } }),
    opts.full ? React.createElement(React.Fragment, null,
      React.createElement('span', { style: { color: 'var(--muted)', fontWeight: 600 } }, tagNs(name) + ':'),
      React.createElement('span', null, tagValue(name)),
    ) : tagValue(name),
  );
}
// two-level tag tree for sidebars: groups (Topics / Workflow) -> tags
function TagTree({ active, onToggle }) {
  const [open, setOpen] = React.useState({ topics: true, wf: true });
  return React.createElement('div', null,
    (window.NIU.tagGroups || []).map((g) => {
      const tags = (window.NIU.tags || []).filter((t) => tagNs(t.name) === g.ns);
      const isOpen = open[g.ns];
      return React.createElement('div', { key: g.ns, style: { marginBottom: 4 } },
        React.createElement('button', { onClick: () => setOpen((s) => ({ ...s, [g.ns]: !s[g.ns] })),
          style: { display: 'flex', alignItems: 'center', gap: 6, width: '100%', height: 26, padding: '0 8px', border: 'none', background: 'transparent', cursor: 'pointer', color: 'var(--muted)', fontFamily: 'var(--sans)', fontSize: 11, fontWeight: 700, letterSpacing: '.06em', textTransform: 'uppercase' } },
          React.createElement('span', { style: { display: 'flex', transition: 'transform .14s', transform: isOpen ? 'rotate(90deg)' : 'none' } }, Icon.chevron({ s: 12 })),
          React.createElement('span', { style: { flex: 1, textAlign: 'left' } }, g.label),
          React.createElement('span', { style: { fontWeight: 600 } }, tags.length),
        ),
        isOpen ? tags.map((t) => {
          const on = active === t.name;
          return React.createElement('button', { key: t.name, onClick: () => onToggle && onToggle(t.name),
            style: { display: 'flex', alignItems: 'center', gap: 9, width: '100%', height: 30, padding: '0 10px 0 22px', border: 'none', borderRadius: 8, cursor: 'pointer', background: on ? 'var(--sel)' : 'transparent', color: on ? 'var(--text)' : 'var(--text-2)', fontFamily: 'var(--sans)', fontSize: 13, fontWeight: 500 } },
            React.createElement('span', { style: { width: 8, height: 8, borderRadius: 3, background: t.color, flex: '0 0 auto' } }),
            React.createElement('span', { style: { flex: 1, textAlign: 'left', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, tagValue(t.name)),
            React.createElement('span', { style: { fontSize: 11, color: 'var(--muted)', fontVariantNumeric: 'tabular-nums' } }, tagCount(t.name)),
          );
        }) : null,
      );
    }),
  );
}
function statusMeta(s) {
  switch (s) {
    case 'done': return { label: 'Read', color: '#1F8A5B' };
    case 'reading': return { label: 'Reading', color: '#B6792B' };
    default: return { label: 'Unread', color: '#888B82' };
  }
}

// ---------- App shell: window chrome + tool rail ----------
const TOOLS = [
  { id: 'library', name: 'Library', icon: Icon.library },
  { id: 'normalize', name: 'Normalize', icon: Icon.normalize },
  { id: 'ai', name: 'AI Assistant', icon: Icon.ai },
  { id: 'settings', name: 'Settings', icon: Icon.settings },
];

// Solid tile mark — white serif N on an accent squircle (the chosen logo).
// Caps reserve descender space below the baseline, so the glyph reads high
// when flex-centered; nudge DOWN to optically center. Lighter weight = finer.
function NiuMark({ size, radius }) {
  size = size || 30;
  return React.createElement('div', { style: { width: size, height: size, borderRadius: radius || Math.round(size * 0.28), background: 'var(--accent)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', marginBottom: 10, flex: '0 0 auto', overflow: 'hidden' } },
    React.createElement('span', { style: { fontFamily: 'var(--serif)', fontWeight: 500, fontSize: Math.round(size * 0.62), lineHeight: 1, letterSpacing: '0', transform: 'translateY(' + (size * 0.07).toFixed(1) + 'px)' } }, 'N'),
  );
}

function ToolRail({ active, onNav }) {
  return React.createElement('nav', { className: 'niu-rail' },
    React.createElement(NiuMark, { size: 30 }),
    // One button per tool; clicking switches the active tab via onNav(id).
    TOOLS.map((t) => React.createElement('button', {
      key: t.id, className: 'niu-railbtn' + (t.id === active ? ' on' : ''), title: t.name,
      onClick: onNav ? () => onNav(t.id) : undefined,
    },
      t.icon({ s: 21 }),
      React.createElement('span', { className: 'niu-rail-tip' }, t.name),
    )),
    React.createElement('div', { style: { flex: 1 } }),
    // Sync: commit & push the .bib to its git remote (mocked).
    React.createElement('button', { className: 'niu-railbtn', title: 'Sync' }, Icon.sync({ s: 20 })),
  );
}

function StatusBar({ entries }) {
  return React.createElement('div', { style: { height: 26, flex: '0 0 26px', display: 'flex', alignItems: 'center', gap: 14, padding: '0 14px', background: 'var(--surface)', borderTop: '1px solid var(--border)', fontSize: 11.5, color: 'var(--muted)' } },
    React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 6 } },
      React.createElement('span', { style: { width: 7, height: 7, borderRadius: '50%', background: 'var(--accent)', boxShadow: '0 0 0 3px var(--accent-tint)' } }),
      React.createElement('span', { className: 'niu-mono', style: { fontSize: 11 } }, 'connector · 127.0.0.1:23510'),
    ),
    React.createElement('div', { style: { flex: 1 } }),
    React.createElement('span', { className: 'niu-mono', style: { fontSize: 11, display: 'inline-flex', alignItems: 'center', gap: 5 } },
      React.createElement('svg', { width: 12, height: 12, viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', strokeWidth: 2, strokeLinecap: 'round', strokeLinejoin: 'round' },
        React.createElement('circle', { cx: 6, cy: 6, r: 2.4 }), React.createElement('circle', { cx: 6, cy: 18, r: 2.4 }), React.createElement('circle', { cx: 18, cy: 8, r: 2.4 }),
        React.createElement('path', { d: 'M6 8.4v7.2M18 10.4c0 3-3 3.6-6 3.6' })),
      'main'),
    React.createElement('span', { style: { display: 'inline-flex', alignItems: 'center', gap: 5, color: 'var(--text-2)' } },
      React.createElement('span', { style: { width: 6, height: 6, borderRadius: '50%', background: '#B6792B' } }), 'modified'),
    React.createElement('span', { style: { color: 'var(--faint)' } }, '·'),
    React.createElement('span', { style: { fontVariantNumeric: 'tabular-nums' } }, (entries || 184) + ' entries'),
  );
}

function Segmented({ options, value, onChange, size }) {
  const h = size === 'sm' ? 26 : 30;
  return React.createElement('div', { style: { display: 'inline-flex', background: 'var(--surface-2)', borderRadius: 9, padding: 3, gap: 2 } },
    options.map((o) => React.createElement('button', { key: o.v, onClick: () => onChange(o.v),
      style: { display: 'inline-flex', alignItems: 'center', gap: 6, height: h, padding: '0 12px', borderRadius: 7, border: 'none', cursor: 'pointer', font: '600 12.5px var(--sans)', background: value === o.v ? 'var(--surface)' : 'transparent', color: value === o.v ? 'var(--accent)' : 'var(--text-2)', boxShadow: value === o.v ? 'var(--shadow)' : 'none', transition: 'background .12s,color .12s' } },
      o.icon ? o.icon({ s: 15 }) : null, o.label)));
}

function WindowChrome({ theme, setTheme, children, libName, active, onNav, entries, center }) {
  return React.createElement('div', { className: 'niu', 'data-theme': theme, style: { width: '100%', height: '100%' } },
    React.createElement('div', { className: 'niu-win', style: { position: 'relative' } },
      React.createElement('div', { className: 'niu-tb' },
        React.createElement('div', { className: 'niu-dot', style: { background: '#F0584E' } }),
        React.createElement('div', { className: 'niu-dot', style: { background: '#F5BC4F' } }),
        React.createElement('div', { className: 'niu-dot', style: { background: '#5FC159' } }),
        React.createElement('div', { className: 'niu-tb-title' },
          React.createElement('span', { style: { display: 'inline-flex', width: 20, height: 20, borderRadius: 6, background: 'var(--accent)', color: '#fff', alignItems: 'center', justifyContent: 'center', flex: '0 0 auto', overflow: 'hidden' } },
            React.createElement('span', { style: { fontFamily: 'var(--serif)', fontWeight: 500, fontSize: 13, lineHeight: 1, transform: 'translateY(1px)' } }, 'N')),
          React.createElement('span', { style: { fontFamily: 'var(--serif)', fontWeight: 500, color: 'var(--text)', marginLeft: 7 } }, 'Niutero'),
          React.createElement('span', { style: { color: 'var(--faint)' } }, '—'),
          React.createElement('span', null, libName || 'BibVault'),
        ),
        center ? React.createElement('div', { style: { position: 'absolute', left: '50%', transform: 'translateX(-50%)' } }, center) : null,
        React.createElement('div', { style: { flex: 1 } }),
        // Theme toggle: light <-> dark for the whole window.
        React.createElement('button', {
          className: 'niu-icbtn', title: 'Toggle theme',
          onClick: () => setTheme(theme === 'dark' ? 'light' : 'dark'),
        }, theme === 'dark' ? Icon.sun({ s: 17 }) : Icon.moon({ s: 17 })),
      ),
      React.createElement('div', { className: 'niu-body' },
        React.createElement(ToolRail, { active: active || 'library', onNav }),
        children,
      ),
      React.createElement(StatusBar, { entries }),
    ),
  );
}

// ---------- shared style objects (cross-file via window) ----------
const secLabel = { fontSize: 11, fontWeight: 700, letterSpacing: '.06em', textTransform: 'uppercase', color: 'var(--muted)', padding: '0 10px 7px' };
const rowItem = { display: 'flex', alignItems: 'center', gap: 9, width: '100%', height: 34, padding: '0 10px', border: 'none', background: 'transparent', borderRadius: 8, cursor: 'pointer', fontFamily: 'var(--sans)', fontSize: 13.5, fontWeight: 500, color: 'var(--text-2)' };
const metaKey = { width: 92, flex: '0 0 92px', fontSize: 12, fontWeight: 600, color: 'var(--muted)' };

// shared left sub-navigation column used by Normalize & Settings
function SubNav({ items, active, onChange, header, width }) {
  return React.createElement('aside', { style: { width: width || 224, flex: '0 0 ' + (width || 224) + 'px', background: 'var(--surface)', borderRight: '1px solid var(--border)', padding: '18px 12px', display: 'flex', flexDirection: 'column', gap: 2, minHeight: 0 } },
    header || null,
    items.map((it) => React.createElement('button', {
      key: it.id, onClick: () => onChange(it.id),
      style: { display: 'flex', alignItems: 'center', gap: 10, width: '100%', height: 38, padding: '0 12px', border: 'none', borderRadius: 9, cursor: 'pointer', textAlign: 'left', fontFamily: 'var(--sans)', fontSize: 14, fontWeight: 600, background: active === it.id ? 'var(--accent-tint)' : 'transparent', color: active === it.id ? 'var(--accent)' : 'var(--text-2)', transition: 'background .12s,color .12s' },
    },
      it.icon ? it.icon({ s: 17, style: { flex: '0 0 auto' } }) : null,
      React.createElement('span', { style: { flex: 1 } }, it.label),
      it.badge != null ? React.createElement('span', { style: { fontSize: 11, fontWeight: 700, fontVariantNumeric: 'tabular-nums', color: active === it.id ? 'var(--accent)' : 'var(--muted)' } }, it.badge) : null,
    )),
  );
}

Object.assign(window, { Icon, typeMeta, tagColor, tagNs, tagValue, tagCount, TagChip, TagTree, EditField, AuthorsField, LockBtn, statusMeta, ToolRail, StatusBar, WindowChrome, SubNav, Segmented, NiuMark, TOOLS, secLabel, rowItem, metaKey });
