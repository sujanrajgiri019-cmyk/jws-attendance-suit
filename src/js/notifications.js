// The notification tray behind the bell in the top bar.
//
// Live punches used to each raise a toast. At a school that means forty of them
// across five minutes at the gate, covering the screen at exactly the time
// somebody is trying to use it. They belong somewhere you look when you choose
// to, so they collect here and the bell shows a count.

import { icon, esc } from './ui.js';

const MAX = 200;

const state = {
  items: [],
  unread: 0,
  open: false,
};

let panel = null;

/**
 * Record something worth noticing.
 * `kind` is 'punch' | 'device' | 'system'.
 */
export function notify(kind, title, detail) {
  state.items.unshift({
    kind,
    title,
    detail,
    at: new Date(),
  });
  // Old entries are not worth the memory; the database has the real history.
  if (state.items.length > MAX) state.items.length = MAX;

  if (!state.open) {
    state.unread++;
  }
  paintBadge();
  if (state.open) paintPanel();
}

export function clearAll() {
  state.items = [];
  state.unread = 0;
  paintBadge();
  paintPanel();
}

function paintBadge() {
  const dot = document.getElementById('bellDot');
  if (!dot) return;
  if (state.unread > 0) {
    dot.textContent = state.unread > 99 ? '99+' : String(state.unread);
    dot.hidden = false;
  } else {
    dot.hidden = true;
  }
}

const KIND_ICON = { punch: 'clock', device: 'cpu', system: 'info' };
const KIND_TONE = { punch: 'o', device: 'b', system: 'n' };

function timeAgo(then) {
  const secs = Math.floor((Date.now() - then.getTime()) / 1000);
  if (secs < 45) return 'just now';
  if (secs < 90) return 'a minute ago';
  if (secs < 3600) return `${Math.round(secs / 60)} minutes ago`;
  if (secs < 7200) return 'an hour ago';
  if (secs < 86400) return `${Math.round(secs / 3600)} hours ago`;
  return then.toLocaleDateString('en-GB', { day: 'numeric', month: 'short' });
}

function paintPanel() {
  if (!panel) return;
  const body = panel.querySelector('#notifBody');
  const count = panel.querySelector('#notifCount');

  count.textContent = state.items.length
    ? `${state.items.length} recent`
    : 'Nothing yet';

  if (!state.items.length) {
    body.innerHTML = `<div class="empty" style="padding:34px 20px">
      <div class="ei">${icon('bell')}</div>
      <b>No activity yet</b>
      <p>Scans and device events appear here as they happen.</p>
    </div>`;
    return;
  }

  body.innerHTML = state.items.map((n) => `
    <div class="notif">
      <span class="tag ${KIND_TONE[n.kind] || 'n'} nk">${icon(KIND_ICON[n.kind] || 'info')}</span>
      <div class="nt">
        <b>${esc(n.title)}</b>
        ${n.detail ? `<span>${esc(n.detail)}</span>` : ''}
      </div>
      <span class="na">${esc(timeAgo(n.at))}</span>
    </div>`).join('');
}

function close() {
  state.open = false;
  panel?.remove();
  panel = null;
  document.removeEventListener('click', onOutside, true);
}

function onOutside(e) {
  if (!panel) return;
  if (panel.contains(e.target) || e.target.closest('#btnBell')) return;
  close();
}

export function togglePanel() {
  if (state.open) return close();

  state.open = true;
  state.unread = 0;
  paintBadge();

  panel = document.createElement('div');
  panel.className = 'notif-panel';
  panel.innerHTML = `
    <div class="notif-h">
      <div style="flex:1">
        <b>Activity</b>
        <span id="notifCount"></span>
      </div>
      <button class="btn sm" id="notifClear">Clear</button>
    </div>
    <div class="notif-b" id="notifBody"></div>`;
  document.body.appendChild(panel);

  panel.querySelector('#notifClear').addEventListener('click', clearAll);
  paintPanel();

  // Defer so the click that opened the panel does not immediately close it.
  setTimeout(() => document.addEventListener('click', onOutside, true), 0);
}
