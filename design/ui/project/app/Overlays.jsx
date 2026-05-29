// Floating overlays mounted inside the app window: AI assistant popup + background task toast.
//
// ============================================================================
// BUTTON REFERENCE — Overlays
// ----------------------------------------------------------------------------
// AIPopup:
//   • FAB (sparkle, bottom-right) — open/close the compact chat popup.
//   • "Open full tab" (expand)    — close popup and switch to the AI Assistant tab.
//   • close (✕)                   — close the popup.
//   • answer actions / send       — librarian actions and submit (mock).
// TaskToast (background task):
//   • "Run in background" / close — dismiss the running toast.
//   • "Review changes"            — (on completion) jump to Normalize (task.onReview).
//   • "Dismiss"                   — (on completion) close the toast.
// ============================================================================

// ---- AI assistant popup (floating action button -> compact chat) ----
function AIPopup({ onOpenTab }) {
  const [open, setOpen] = React.useState(false);
  const items = window.NIU.items;
  const get = (id) => items.find((x) => x.id === id);
  const cite = (it) => React.createElement('span', { key: 'c' + it.id, style: { display: 'inline-flex', alignItems: 'center', padding: '0 6px', borderRadius: 5, border: '1px solid var(--border)', background: 'var(--surface-2)', color: 'var(--accent)', font: '600 11.5px var(--sans)', whiteSpace: 'nowrap' } }, it.creator.replace(' et al.', ''), ' ', String(it.year));

  const panel = React.createElement('div', { style: { position: 'absolute', right: 0, bottom: 60, width: 360, height: 460, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 16, boxShadow: 'var(--shadow-lg)', display: 'flex', flexDirection: 'column', overflow: 'hidden', transformOrigin: 'bottom right' } },
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 9, padding: '12px 12px 12px 16px', borderBottom: '1px solid var(--border-2)' } },
      React.createElement('span', { style: { width: 24, height: 24, borderRadius: 7, background: 'var(--accent-tint)', color: 'var(--accent)', display: 'flex', alignItems: 'center', justifyContent: 'center' } }, Icon.sparkle({ s: 15 })),
      React.createElement('span', { style: { flex: 1, fontSize: 14, fontWeight: 700, color: 'var(--text)' } }, 'Assistant'),
      React.createElement('button', { className: 'niu-icbtn', style: { width: 28, height: 28 }, title: 'Open full tab', onClick: () => { setOpen(false); onOpenTab && onOpenTab(); } }, Icon.expand({ s: 15 })),
      React.createElement('button', { className: 'niu-icbtn', style: { width: 28, height: 28 }, onClick: () => setOpen(false) }, Icon.close({ s: 16 })),
    ),
    React.createElement('div', { className: 'niu-scroll', style: { flex: 1, minHeight: 0, padding: '16px' } },
      React.createElement('div', { style: { display: 'flex', justifyContent: 'flex-end', marginBottom: 16 } },
        React.createElement('div', { style: { maxWidth: '85%', background: 'var(--accent)', color: '#fff', padding: '9px 13px', borderRadius: '14px 14px 4px 14px', fontSize: 13.5, lineHeight: 1.5 } }, 'Any SAE unlearning papers I haven\u2019t tagged yet?')),
      React.createElement('div', { style: { display: 'flex', gap: 10, marginBottom: 6 } },
        React.createElement('span', { style: { flex: '0 0 26px', width: 26, height: 26, borderRadius: 8, background: 'var(--accent-tint)', color: 'var(--accent)', display: 'flex', alignItems: 'center', justifyContent: 'center', marginTop: 1 } }, Icon.sparkle({ s: 14 })),
        React.createElement('div', { style: { flex: 1, fontSize: 13.5, lineHeight: 1.6, color: 'var(--text)' } },
          'Two match but lack ', React.createElement('span', { style: { color: 'var(--accent)', fontWeight: 600 } }, 'wf:to-cite'), ': ',
          cite(get(3)), ' and ', cite(get(6)), '. Want me to tag both?',
          React.createElement('div', { style: { display: 'flex', gap: 7, marginTop: 11, flexWrap: 'wrap' } },
            React.createElement('button', { className: 'niu-btn', style: { height: 28 } }, Icon.tag({ s: 14 }), 'Tag both'),
            React.createElement('button', { className: 'niu-btn', style: { height: 28 } }, 'Show in list'),
          ),
        ),
      ),
    ),
    React.createElement('div', { style: { borderTop: '1px solid var(--border-2)', padding: '10px 12px' } },
      React.createElement('div', { style: { display: 'flex', alignItems: 'flex-end', gap: 8, background: 'var(--surface-2)', borderRadius: 12, padding: '6px 6px 6px 13px' } },
        React.createElement('input', { placeholder: 'Ask across your library\u2026', style: { flex: 1, border: 'none', outline: 'none', background: 'transparent', font: '400 13.5px var(--sans)', color: 'var(--text)' } }),
        React.createElement('button', { className: 'niu-btn pri', style: { width: 32, height: 32, padding: 0, justifyContent: 'center', borderRadius: 9 } }, Icon.send({ s: 15 })),
      ),
    ),
  );

  return React.createElement('div', { style: { position: 'absolute', right: 22, bottom: 44, zIndex: 60 } },
    open ? panel : null,
    React.createElement('button', { onClick: () => setOpen((o) => !o), title: 'AI assistant',
      style: { width: 50, height: 50, borderRadius: 16, border: 'none', cursor: 'pointer', background: 'var(--accent)', color: '#fff', display: 'flex', alignItems: 'center', justifyContent: 'center', boxShadow: 'var(--shadow-lg)', marginLeft: 'auto', transition: 'transform .12s' } },
      open ? Icon.close({ s: 22 }) : Icon.sparkle({ s: 24 })),
  );
}

// ---- background task progress toast ----
function TaskToast({ task, onDismiss }) {
  if (!task) return null;
  const pct = Math.round(task.progress * 100);
  const done = task.progress >= 1;
  return React.createElement('div', { style: { position: 'absolute', left: 22, bottom: 44, width: 320, background: 'var(--surface)', border: '1px solid var(--border)', borderRadius: 14, boxShadow: 'var(--shadow-lg)', padding: '14px 16px', zIndex: 60 } },
    React.createElement('div', { style: { display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 } },
      React.createElement('span', { style: { width: 26, height: 26, borderRadius: 8, background: done ? 'var(--accent-tint)' : 'var(--surface-2)', color: done ? 'var(--accent)' : 'var(--text-2)', display: 'flex', alignItems: 'center', justifyContent: 'center', flex: '0 0 auto' } },
        done ? Icon.checkCircle({ s: 16 }) : React.createElement('span', { className: 'niu-spin', style: { display: 'flex' } }, Icon.sync({ s: 15 }))),
      React.createElement('div', { style: { flex: 1, minWidth: 0 } },
        React.createElement('div', { style: { fontSize: 13.5, fontWeight: 700, color: 'var(--text)' } }, done ? task.doneLabel || 'Done' : task.label),
        React.createElement('div', { style: { fontSize: 11.5, color: 'var(--muted)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' } }, done ? task.doneSub || '' : task.sub)),
      done
        ? React.createElement('button', { className: 'niu-icbtn', style: { width: 26, height: 26 }, onClick: onDismiss }, Icon.close({ s: 15 }))
        : React.createElement('span', { className: 'niu-mono', style: { fontSize: 12.5, fontWeight: 600, color: 'var(--text-2)', fontVariantNumeric: 'tabular-nums' } }, pct + '%'),
    ),
    React.createElement('div', { style: { height: 6, borderRadius: 4, background: 'var(--surface-2)', overflow: 'hidden' } },
      React.createElement('div', { style: { height: '100%', width: pct + '%', borderRadius: 4, background: 'var(--accent)', transition: 'width .35s ease' } })),
    !done ? React.createElement('div', { style: { display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginTop: 9 } },
      React.createElement('span', { style: { fontSize: 11.5, color: 'var(--muted)' } }, task.count),
      React.createElement('button', { onClick: onDismiss, style: { border: 'none', background: 'transparent', cursor: 'pointer', color: 'var(--muted)', font: '600 11.5px var(--sans)' } }, 'Run in background'),
    ) : React.createElement('div', { style: { marginTop: 10, display: 'flex', gap: 8 } },
      React.createElement('button', { className: 'niu-btn pri', style: { height: 28, flex: 1, justifyContent: 'center' }, onClick: task.onReview }, 'Review changes'),
      React.createElement('button', { className: 'niu-btn', style: { height: 28 }, onClick: onDismiss }, 'Dismiss'),
    ),
  );
}

(function () {
  if (document.getElementById('niu-spin-style')) return;
  const s = document.createElement('style'); s.id = 'niu-spin-style';
  s.textContent = '@keyframes niuSpin{to{transform:rotate(360deg)}} .niu-spin{animation:niuSpin 1.1s linear infinite;}';
  document.head.appendChild(s);
})();

window.AIPopup = AIPopup;
window.TaskToast = TaskToast;
