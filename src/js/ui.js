// Shared interface helpers: icons, toasts, modals, tables, formatting.

// ---------------------------------------------------------------------------
// Icons — inline SVG so the app has no icon-font dependency and works offline.
// ---------------------------------------------------------------------------

const PATHS = {
  grid: '<rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/>',
  cpu: '<rect x="5" y="5" width="14" height="14" rx="2"/><rect x="9" y="9" width="6" height="6"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/>',
  sync: '<path d="M21 2v6h-6M3 22v-6h6"/><path d="M3.5 9a9 9 0 0114.9-3.4L21 8M20.5 15a9 9 0 01-14.9 3.4L3 16"/>',
  users: '<path d="M17 21v-2a4 4 0 00-4-4H5a4 4 0 00-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 00-3-3.87M16 3.13a4 4 0 010 7.75"/>',
  layers: '<path d="M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5"/>',
  db: '<ellipse cx="12" cy="5" rx="9" ry="3"/><path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"/><path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"/>',
  shield: '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M9 12l2 2 4-4"/>',
  clock: '<circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/>',
  trend: '<path d="M23 6l-9.5 9.5-5-5L1 18"/><path d="M17 6h6v6"/>',
  chart: '<path d="M3 3v18h18"/><rect x="7" y="12" width="3" height="6" rx="1"/><rect x="12" y="8" width="3" height="10" rx="1"/><rect x="17" y="5" width="3" height="13" rx="1"/>',
  cog: '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 11-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 008 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 11-2.83-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H2a2 2 0 010-4h.09A1.65 1.65 0 003.6 8a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 112.83-2.83l.06.06A1.65 1.65 0 008 3.6h.09A1.65 1.65 0 009 2.09V2a2 2 0 014 0v.09A1.65 1.65 0 0016 3.6a1.65 1.65 0 001.82-.33l.06-.06a2 2 0 112.83 2.83l-.06.06A1.65 1.65 0 0020.4 8v.09a1.65 1.65 0 001.51 1H22a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z"/>',
  bell: '<path d="M18 8A6 6 0 006 8c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 01-3.4 0"/>',
  search: '<circle cx="11" cy="11" r="8"/><path d="M21 21l-4.35-4.35"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  plug: '<path d="M9 2v6M15 2v6M6 8h12v3a6 6 0 01-12 0V8zM12 17v5"/>',
  wifi: '<path d="M5 12.55a11 11 0 0114 0M8.5 16.11a6 6 0 017 0M2 8.82a15 15 0 0120 0M12 20h.01"/>',
  up: '<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M17 8l-5-5-5 5M12 3v12"/>',
  down: '<path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4M7 10l5 5 5-5M12 15V3"/>',
  file: '<path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z"/><path d="M14 2v6h6M9 15l3-3 3 3M12 12v6"/>',
  save: '<path d="M19 21H5a2 2 0 01-2-2V5a2 2 0 012-2h11l5 5v11a2 2 0 01-2 2z"/><path d="M17 21v-8H7v8M7 3v5h8"/>',
  trash: '<path d="M3 6h18M8 6V4a2 2 0 012-2h4a2 2 0 012 2v2M19 6l-1 14a2 2 0 01-2 2H8a2 2 0 01-2-2L5 6"/>',
  edit: '<path d="M11 4H4a2 2 0 00-2 2v14a2 2 0 002 2h14a2 2 0 002-2v-7"/><path d="M18.5 2.5a2.12 2.12 0 013 3L12 15l-4 1 1-4 9.5-9.5z"/>',
  mail: '<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M22 6l-10 7L2 6"/>',
  print: '<path d="M6 9V2h12v7M6 18H4a2 2 0 01-2-2v-5a2 2 0 012-2h16a2 2 0 012 2v5a2 2 0 01-2 2h-2"/><rect x="6" y="14" width="12" height="8"/>',
  x: '<path d="M18 6L6 18M6 6l12 12"/>',
  info: '<circle cx="12" cy="12" r="10"/><path d="M12 16v-4M12 8h.01"/>',
  warn: '<path d="M10.3 3.9L1.8 18a2 2 0 001.7 3h17a2 2 0 001.7-3L14.7 3.9a2 2 0 00-3.4 0z"/><path d="M12 9v4M12 17h.01"/>',
  check: '<path d="M20 6L9 17l-5-5"/>',
  lock: '<rect x="3" y="11" width="18" height="11" rx="2"/><path d="M7 11V7a5 5 0 0110 0v4"/>',
  logout: '<path d="M9 21H5a2 2 0 01-2-2V5a2 2 0 012-2h4M16 17l5-5-5-5M21 12H9"/>',
};

export function icon(name, cls = '') {
  const p = PATHS[name];
  if (!p) return '';
  return `<svg viewBox="0 0 24 24" class="${cls}" fill="none" stroke="currentColor"
    stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
}

// ---------------------------------------------------------------------------
// Escaping — every value from the database goes through this before reaching
// innerHTML. A staff member called `O'Brien & <Co>` must not break the page.
// ---------------------------------------------------------------------------

export function esc(v) {
  if (v === null || v === undefined) return '';
  return String(v).replace(/[&<>"']/g, (c) => ({
    '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;',
  })[c]);
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

export const PALETTE = [
  '#F16522', '#2B6CB0', '#1F9D55', '#6B4EBB', '#C9820B',
  '#D64545', '#0E7C86', '#B8336A', '#5A6B7B',
];

/** Stable colour for an avatar, derived from the id so it never shuffles. */
export const colourFor = (n) => PALETTE[Math.abs(Number(n) || 0) % PALETTE.length];

export function initials(name) {
  const parts = String(name || '').trim().split(/\s+/);
  return ((parts[0]?.[0] || '') + (parts[1]?.[0] || '')).toUpperCase() || '?';
}

/** `7:15` from minutes. */
export function duration(mins) {
  const m = Math.max(0, Math.round(Number(mins) || 0));
  return `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}`;
}

/** `08:55` from `08:55:00`. */
export const hhmm = (t) => (t ? String(t).slice(0, 5) : '—');

export function todayIso() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

export function addDays(iso, n) {
  const d = new Date(`${iso}T00:00:00`);
  d.setDate(d.getDate() + n);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

export const monthStart = (iso) => `${iso.slice(0, 7)}-01`;

const STATUS_TONE = {
  Present: 'g', Late: 'y', HalfDay: 'y', Absent: 'r',
  Leave: 'v', Holiday: 'b', WeeklyOff: 'n', MissingPunch: 'n',
};
const STATUS_LABEL = {
  HalfDay: 'Half day', WeeklyOff: 'Weekly off', MissingPunch: 'Missing punch',
};

export const statusTone = (s) => STATUS_TONE[s] || 'n';
export const statusLabel = (s) => STATUS_LABEL[s] || s;
export const statusTag = (s) =>
  `<span class="tag ${statusTone(s)}">${esc(statusLabel(s))}</span>`;

/** Avatar + name + subtitle, the standard person cell. */
export function person(name, sub, seed) {
  return `<div class="person">
    <div class="av" style="background:${colourFor(seed)}">${esc(initials(name))}</div>
    <div class="pt"><b>${esc(name)}</b><span>${esc(sub || '')}</span></div>
  </div>`;
}

// ---------------------------------------------------------------------------
// Toasts
// ---------------------------------------------------------------------------

export function toast(kind, message) {
  const wrap = document.getElementById('toasts');
  if (!wrap) return;
  const el = document.createElement('div');
  el.className = `toast ${kind}`;
  const ico = kind === 'ok' ? 'check' : kind === 'err' ? 'warn' : 'info';
  el.innerHTML = `${icon(ico)}<span>${esc(message)}</span>`;
  wrap.appendChild(el);
  // Errors stay long enough to read and copy; confirmations can go quickly.
  const life = kind === 'err' ? 7000 : 3200;
  setTimeout(() => {
    el.style.transition = '.25s';
    el.style.opacity = '0';
    el.style.transform = 'translateX(20px)';
    setTimeout(() => el.remove(), 250);
  }, life);
}

/** Run an async action, showing a spinner on the button and reporting errors. */
export async function withBusy(btn, fn, okMessage) {
  if (!btn) return fn();
  const original = btn.innerHTML;
  btn.disabled = true;
  btn.innerHTML = `<span class="spin"></span><span>Working…</span>`;
  try {
    const r = await fn();
    if (okMessage) toast('ok', typeof okMessage === 'function' ? okMessage(r) : okMessage);
    return r;
  } catch (e) {
    toast('err', e.message || String(e));
    throw e;
  } finally {
    btn.disabled = false;
    btn.innerHTML = original;
  }
}

// ---------------------------------------------------------------------------
// Modals
// ---------------------------------------------------------------------------

let modalHost = null;

/**
 * Open a modal. `render` returns the inner HTML; `buttons` describe the footer.
 * Resolves with whatever the clicked button's handler returns, or null if
 * dismissed.
 */
export function modal({ title, subtitle, body, wide, buttons = [] }) {
  if (!modalHost) {
    modalHost = document.createElement('div');
    document.body.appendChild(modalHost);
  }
  return new Promise((resolve) => {
    const ov = document.createElement('div');
    ov.className = 'ov on';
    ov.innerHTML = `
      <div class="mdl ${wide ? 'wide' : ''}">
        <div class="mdl-h">
          <div style="flex:1">
            <h3>${esc(title)}</h3>
            ${subtitle ? `<p>${esc(subtitle)}</p>` : ''}
          </div>
          <button class="ibtn" data-close>${icon('x')}</button>
        </div>
        <div class="mdl-b">${body}</div>
        <div class="mdl-f">${buttons
          .map((b, i) => `<button class="btn ${b.kind || ''}" data-i="${i}">${esc(b.label)}</button>`)
          .join('')}</div>
      </div>`;
    modalHost.appendChild(ov);

    const close = (v) => {
      ov.remove();
      document.removeEventListener('keydown', onKey);
      resolve(v);
    };
    const onKey = (e) => {
      if (e.key === 'Escape') close(null);
    };
    document.addEventListener('keydown', onKey);

    ov.addEventListener('click', async (e) => {
      if (e.target === ov || e.target.closest('[data-close]')) return close(null);
      const btn = e.target.closest('[data-i]');
      if (!btn) return;
      const spec = buttons[Number(btn.dataset.i)];
      if (!spec.onClick) return close(spec.value ?? true);
      try {
        const r = await withBusy(btn, () => spec.onClick(ov));
        if (r !== false) close(r);
      } catch {
        /* withBusy already reported it; keep the dialog open to correct it */
      }
    });

    // Focus the first field so the dialog is usable from the keyboard.
    setTimeout(() => ov.querySelector('input,select,textarea')?.focus(), 40);
  });
}

export function confirmDialog(title, message, confirmLabel = 'Confirm', kind = 'dan') {
  return modal({
    title,
    body: `<p style="font-size:13.5px;line-height:1.6;margin:0">${esc(message)}</p>`,
    buttons: [
      { label: 'Cancel', value: false },
      { label: confirmLabel, kind, value: true },
    ],
  }).then((v) => v === true);
}

/** Read every `[name]` field inside a container into a plain object. */
export function readForm(root) {
  const out = {};
  root.querySelectorAll('[name]').forEach((el) => {
    if (el.type === 'checkbox') out[el.name] = el.checked;
    else if (el.type === 'radio') {
      if (el.checked) out[el.name] = el.value;
    } else out[el.name] = el.value;
  });
  return out;
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

/**
 * Render a table into `el`.
 * `columns` is an array of `{ label, get, cls, width }`.
 */
export function table(el, columns, rows, { empty, emptyHint } = {}) {
  if (!el) return;
  if (!rows.length) {
    el.innerHTML = `<tbody><tr><td colspan="${columns.length}">
      <div class="empty">
        <div class="ei">${icon('search')}</div>
        <b>${esc(empty || 'Nothing to show')}</b>
        <p>${esc(emptyHint || '')}</p>
      </div></td></tr></tbody>`;
    return;
  }
  el.innerHTML =
    `<thead><tr>${columns
      .map((c) => `<th class="${c.cls || ''}" ${c.width ? `style="width:${c.width}"` : ''}>${esc(c.label)}</th>`)
      .join('')}</tr></thead>` +
    `<tbody>${rows
      .map(
        (r, i) =>
          `<tr data-row="${i}">${columns
            .map((c) => `<td class="${c.cls || ''}">${c.get(r, i)}</td>`)
            .join('')}</tr>`,
      )
      .join('')}</tbody>`;
}

export function loadingTable(el, cols = 6) {
  if (!el) return;
  el.innerHTML = `<tbody>${Array.from({ length: 6 })
    .map(
      () =>
        `<tr class="loading-row">${Array.from({ length: cols })
          .map(() => `<td><div class="skel" style="height:13px"></div></td>`)
          .join('')}</tr>`,
    )
    .join('')}</tbody>`;
}

export function emptyState(el, title, hint, iconName = 'info') {
  if (!el) return;
  el.innerHTML = `<div class="empty">
    <div class="ei">${icon(iconName)}</div>
    <b>${esc(title)}</b><p>${esc(hint || '')}</p></div>`;
}

// ---------------------------------------------------------------------------
// Small chart helpers (hand-rolled SVG — no charting dependency)
// ---------------------------------------------------------------------------

export function barChart(points, { height = 220, max } = {}) {
  if (!points.length) return '<div class="empty"><b>No attendance recorded yet</b></div>';
  const W = 760;
  const PL = 34, PR = 8, PT = 10, PB = 26;
  const iw = W - PL - PR, ih = height - PT - PB;
  const bw = iw / points.length;
  const bar = Math.min(20, bw * 0.6);
  const top = Math.max(1, max ?? Math.max(...points.map((p) => p.present + p.late + p.absent)));

  let g = '';
  for (const f of [0, 0.25, 0.5, 0.75, 1]) {
    const y = PT + ih - ih * f;
    g += `<line x1="${PL}" y1="${y}" x2="${W - PR}" y2="${y}" stroke="#F0F2F4"/>`;
    g += `<text x="${PL - 7}" y="${y + 3.5}" text-anchor="end" font-size="9.5" fill="#A9B0B8">${Math.round(top * f)}</text>`;
  }
  points.forEach((p, i) => {
    const x = PL + bw * i + (bw - bar) / 2;
    let y = PT + ih;
    for (const [v, colour] of [[p.present, '#F16522'], [p.late, '#F5C6A8'], [p.absent, '#E7E9EC']]) {
      const h = (ih * v) / top;
      y -= h;
      if (h > 0) g += `<rect x="${x}" y="${y}" width="${bar}" height="${h}" fill="${colour}"/>`;
    }
    const label = p.date.slice(8) + '/' + p.date.slice(5, 7);
    g += `<rect x="${x}" y="${PT}" width="${bar}" height="${ih}" fill="transparent">
      <title>${esc(p.date)} — present ${p.present}, late ${p.late}, absent ${p.absent}</title></rect>`;
    if (points.length <= 16 || i % 2 === 0) {
      g += `<text x="${x + bar / 2}" y="${height - 8}" text-anchor="middle" font-size="9" fill="#A9B0B8">${label}</text>`;
    }
  });
  return `<svg viewBox="0 0 ${W} ${height}" style="width:100%;height:auto;display:block">${g}</svg>`;
}

export function donut(segments, centreValue, centreLabel) {
  const total = segments.reduce((a, s) => a + s.value, 0);
  const R = 62, SW = 17, C = 80, circ = 2 * Math.PI * R;
  let off = 0;
  let g = `<circle cx="${C}" cy="${C}" r="${R}" fill="none" stroke="#F0F2F4" stroke-width="${SW}"/>`;
  if (total > 0) {
    for (const s of segments) {
      if (!s.value) continue;
      const len = circ * (s.value / total);
      g += `<circle cx="${C}" cy="${C}" r="${R}" fill="none" stroke="${s.colour}" stroke-width="${SW}"
        stroke-dasharray="${Math.max(0, len - 2)} ${circ - len + 2}" stroke-dashoffset="${-off}"
        transform="rotate(-90 ${C} ${C})"><title>${esc(s.label)}: ${s.value}</title></circle>`;
      off += len;
    }
  }
  g += `<text x="${C}" y="${C - 3}" text-anchor="middle" font-size="26" font-weight="700" fill="#333">${esc(centreValue)}</text>`;
  g += `<text x="${C}" y="${C + 15}" text-anchor="middle" font-size="9.5" fill="#868D96" letter-spacing=".5">${esc(centreLabel)}</text>`;
  return `<svg viewBox="0 0 160 160" style="width:170px;height:170px">${g}</svg>`;
}
