// AI Assistant tab — chat across the library. The assistant answers grounded
// in the user's entries and cites them inline; offers librarian actions.
//
// ============================================================================
// BUTTON REFERENCE — AI Assistant tool
// ----------------------------------------------------------------------------
// Context bar: "Scope: My Library ▾" (choose what the AI reads), "New chat".
// Inline citation chips: click to jump to that entry.
// Answer actions: "Draft related-work ¶", "Tag these 3", "Copy citations".
// Composer: suggestion chips (prefill a prompt), text box, send (primary) button.
// ============================================================================

function aiCite(item, onOpen) {
  return React.createElement('button', {
    key: 'c' + item.id, onClick: () => onOpen && onOpen(item.id), title: item.title,
    style: { display: 'inline-flex', alignItems: 'center', gap: 4, padding: '1px 7px', margin: '0 1px', borderRadius: 6, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--accent)', font: '600 12px var(--sans)', cursor: 'pointer', verticalAlign: 'baseline', lineHeight: 1.4 },
  }, item.creator.replace(' et al.', ' et al.'), ' ', String(item.year));
}

function AssistantMsg({ children }) {
  return React.createElement('div', { style: { display: 'flex', gap: 14, marginBottom: 26 } },
    React.createElement('div', { style: { flex: '0 0 30px', width: 30, height: 30, borderRadius: 9, background: 'var(--accent-tint)', color: 'var(--accent)', display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: 2 } }, Icon.sparkle({ s: 17 })),
    React.createElement('div', { style: { flex: 1, minWidth: 0, fontSize: 14.5, lineHeight: 1.68, color: 'var(--text)' } }, children),
  );
}
function UserMsg({ children }) {
  return React.createElement('div', { style: { display: 'flex', justifyContent: 'flex-end', marginBottom: 26 } },
    React.createElement('div', { style: { maxWidth: '78%', background: 'var(--accent)', color: '#fff', padding: '11px 16px', borderRadius: '16px 16px 4px 16px', fontSize: 14.5, lineHeight: 1.55 } }, children),
  );
}

function AITab() {
  const items = window.NIU.items;
  const get = (id) => items.find((x) => x.id === id);
  const open = () => {};

  const SUGGEST = [
    'Find gaps in my SAE coverage',
    'Draft a related-work paragraph on unlearning',
    'Which papers should I read next?',
  ];

  const thread = React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0 } },
    React.createElement('div', { style: { maxWidth: 760, margin: '0 auto', padding: '28px 28px 8px' } },
      React.createElement(UserMsg, null, 'Which papers in my library use sparse autoencoders for unlearning, and do they actually agree on whether it works?'),
      React.createElement(AssistantMsg, null,
        React.createElement('p', { style: { margin: '0 0 12px' } },
          'Three entries sit squarely in SAE-based unlearning, and they don\u2019t fully agree:'),
        React.createElement('ul', { style: { margin: '0 0 12px', paddingLeft: 18, display: 'flex', flexDirection: 'column', gap: 7 } },
          React.createElement('li', null, aiCite(get(5), open), ' is the most optimistic \u2014 Dynamic SAE Guardrails remove targeted knowledge with ', React.createElement('em', null, 'less'), ' collateral damage than gradient baselines.'),
          React.createElement('li', null, aiCite(get(3), open), ' is more cautious: clamping features lowers forget-set performance but causes unintended degradation elsewhere, questioning feature locality.'),
          React.createElement('li', null, aiCite(get(6), open), ' takes a training-free, subspace-projection angle and reports better capability retention on WMDP.'),
        ),
        React.createElement('p', { style: { margin: '0 0 14px' } },
          'So the tension is ', React.createElement('strong', null, 'precision vs. collateral damage'), ' \u2014 the newer two argue the damage is manageable with the right feature selection, which echoes the steering result in ', aiCite(get(1), open), '.'),
        React.createElement('div', { style: { display: 'flex', gap: 9, flexWrap: 'wrap' } },
          React.createElement('button', { className: 'niu-btn', style: { height: 30 } }, Icon.quote({ s: 15 }), 'Draft related-work \u00b6'),
          React.createElement('button', { className: 'niu-btn', style: { height: 30 } }, Icon.tag({ s: 15 }), 'Tag these 3 \u201cunlearning\u201d'),
          React.createElement('button', { className: 'niu-btn', style: { height: 30 } }, Icon.copy({ s: 15 }), 'Copy citations'),
        ),
      ),
    ),
  );

  const composer = React.createElement('div', { style: { borderTop: '1px solid var(--border)', background: 'var(--bg)', padding: '14px 28px 18px' } },
    React.createElement('div', { style: { maxWidth: 760, margin: '0 auto' } },
      React.createElement('div', { style: { display: 'flex', gap: 8, marginBottom: 11, flexWrap: 'wrap' } },
        SUGGEST.map((s) => React.createElement('button', { key: s, style: { padding: '6px 12px', borderRadius: 20, border: '1px solid var(--border)', background: 'var(--surface)', color: 'var(--text-2)', font: '500 12.5px var(--sans)', cursor: 'pointer' } }, s))),
      React.createElement('div', { style: { display: 'flex', alignItems: 'flex-end', gap: 10, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 14, padding: '10px 10px 10px 16px', boxShadow: 'var(--shadow)' } },
        React.createElement('textarea', { rows: 1, placeholder: 'Ask across your library\u2026', style: { flex: 1, border: 'none', outline: 'none', resize: 'none', background: 'transparent', font: '400 14.5px var(--sans)', color: 'var(--text)', lineHeight: 1.5, paddingTop: 5, maxHeight: 120 } }),
        React.createElement('button', { className: 'niu-btn pri', style: { width: 38, padding: 0, justifyContent: 'center', flex: '0 0 38px', height: 38, borderRadius: 11 } }, Icon.send({ s: 17 })),
      ),
      React.createElement('div', { style: { fontSize: 11.5, color: 'var(--faint)', marginTop: 9, textAlign: 'center' } }, 'Answers are grounded in your 184 entries · responses can be wrong, verify citations'),
    ),
  );

  const ctxBar = React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 10, padding: '13px 28px', borderBottom: '1px solid var(--border)' } },
    React.createElement('h1', { style: { fontSize: 17, fontWeight: 700, letterSpacing: '-.01em', margin: 0 } }, 'AI Assistant'),
    React.createElement('div', { style: { flex: 1 } }),
    React.createElement('button', { className: 'niu-btn', style: { height: 30 } }, Icon.library({ s: 15 }), 'Scope: My Library', Icon.chevDown({ s: 14 })),
    React.createElement('button', { className: 'niu-btn', style: { height: 30 } }, Icon.plus({ s: 15 }), 'New chat'),
  );

  return React.createElement('section', { style: { flex: 1, display: 'flex', flexDirection: 'column', minWidth: 0, background: 'var(--bg)' } },
    ctxBar, thread, composer);
}

window.AITab = AITab;
