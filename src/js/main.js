// Application shell: sign-in, navigation, and the shared chrome.

import { api, isDesktop, listen } from './api.js';
import { icon, toast, esc, modal, withBusy } from './ui.js';

import dashboard from './pages/dashboard.js';
import devices from './pages/devices.js';
import data from './pages/data.js';
import members from './pages/members.js';
import departments from './pages/departments.js';
import database from './pages/database.js';
import rules from './pages/rules.js';
import timetables from './pages/timetables.js';
import reports from './pages/reports.js';
import settings from './pages/settings.js';

const PAGES = {
  dashboard, devices, data, members, departments,
  database, rules, timetables, reports, settings,
};

const NAV = [
  { group: 'Overview' },
  { id: 'dashboard', label: 'Dashboard', icon: 'grid', title: 'Dashboard', sub: 'Live attendance overview for today' },
  { group: 'Hardware' },
  { id: 'devices', label: 'Devices', icon: 'cpu', title: 'Devices', sub: 'Attendance terminals connected to this computer' },
  { id: 'data', label: 'Data Transfer', icon: 'sync', title: 'Data Transfer', sub: 'Move users and punch records between this computer and the terminal' },
  { group: 'People' },
  { id: 'members', label: 'Members', icon: 'users', title: 'Members', sub: 'Staff records — the master list every terminal is synced from' },
  { id: 'departments', label: 'Departments', icon: 'layers', title: 'Departments', sub: 'Organise staff into reporting groups' },
  { id: 'database', label: 'Database', icon: 'db', title: 'Database', sub: 'Every record held in the local database file' },
  { group: 'Policy' },
  { id: 'rules', label: 'Attendance Rules', icon: 'shield', title: 'Attendance Rules', sub: 'How raw scans become a day of attendance' },
  { id: 'timetables', label: 'Timetables', icon: 'clock', title: 'Timetables', sub: 'Shifts, weekly grids, schedules and holidays' },
  { group: 'Output' },
  { id: 'reports', label: 'Reports', icon: 'chart', title: 'Reports', sub: 'Generate, print and export attendance reports' },
  { id: 'settings', label: 'Settings', icon: 'cog', title: 'Settings', sub: 'School profile, security, email and updates' },
];

export const state = {
  info: null,
  current: null,
  /** Cleanup returned by the current page, called before navigating away. */
  teardown: null,
};

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

function buildNav() {
  document.getElementById('nav').innerHTML = NAV.map((n) =>
    n.group
      ? `<div class="nav-lbl">${esc(n.group)}</div>`
      : `<a data-page="${n.id}">${icon(n.icon)}<span>${esc(n.label)}</span></a>`,
  ).join('');
}

export async function go(id) {
  const entry = NAV.find((n) => n.id === id);
  if (!entry) return;

  if (state.teardown) {
    try { state.teardown(); } catch { /* a failing teardown must not block navigation */ }
    state.teardown = null;
  }

  state.current = id;
  document.querySelectorAll('#nav a').forEach((a) =>
    a.classList.toggle('on', a.dataset.page === id),
  );
  document.getElementById('pgTitle').textContent = entry.title;
  document.getElementById('pgSub').textContent = entry.sub;

  const host = document.getElementById('page');
  host.scrollTop = 0;
  host.innerHTML = '';

  try {
    state.teardown = (await PAGES[id].mount(host)) || null;
  } catch (e) {
    host.innerHTML = `<div class="card"><div class="card-b">
      <div class="note y">${icon('warn')}<div>
        <b>This screen could not be loaded.</b><br>${esc(e.message || e)}
      </div></div></div></div>`;
  }
}

// ---------------------------------------------------------------------------
// Chrome
// ---------------------------------------------------------------------------

function clock() {
  const tick = () => {
    const now = new Date();
    document.getElementById('clkTime').textContent = now.toTimeString().slice(0, 8);
    const ad = now.toLocaleDateString('en-GB', {
      weekday: 'short', day: 'numeric', month: 'short', year: 'numeric',
    });
    const bs = state.info?.today_bs ? ` · ${state.info.today_bs}` : '';
    document.getElementById('clkDate').textContent = ad + bs;
  };
  tick();
  setInterval(tick, 1000);
}

export async function refreshDeviceChip() {
  const chip = document.getElementById('devChip');
  const text = document.getElementById('devChipText');
  try {
    const [devs, info] = await Promise.all([api.listDevices(), api.appInfo()]);
    state.info = info;
    if (info.push_running) {
      chip.classList.remove('off');
      text.textContent = `Listening on port ${info.push_port} · ${devs.length} terminal${devs.length === 1 ? '' : 's'}`;
    } else {
      chip.classList.add('off');
      text.textContent = devs.length ? 'Listener stopped' : 'No terminal registered';
    }
  } catch {
    chip.classList.add('off');
    text.textContent = 'Status unavailable';
  }
}

async function loadInfo() {
  try {
    state.info = await api.appInfo();
  } catch {
    state.info = null;
  }
  const v = state.info?.version ? `JWS Attendance v${state.info.version}` : 'JWS Attendance';
  document.getElementById('verLabel').textContent = v;

  const banner = document.getElementById('defaultPwBanner');
  if (state.info?.password_is_default) {
    document.getElementById('bannerIcon').innerHTML = icon('warn');
    banner.hidden = false;
  } else {
    banner.hidden = true;
  }
}

// ---------------------------------------------------------------------------
// Sign-in
// ---------------------------------------------------------------------------

function showApp() {
  document.getElementById('gate').hidden = true;
  document.getElementById('app').hidden = false;
}

async function signIn(username, password) {
  const ok = await api.login(username, password);
  if (!ok) throw new Error('That username or password is not correct.');
  sessionStorage.setItem('jws-signed-in', '1');
}

function wireGate() {
  const form = document.getElementById('gateForm');
  const err = document.getElementById('gateErr');

  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    err.hidden = true;
    const username = document.getElementById('gateUser').value.trim();
    const password = document.getElementById('gatePass').value;
    const btn = document.getElementById('gateBtn');
    try {
      await withBusy(btn, () => signIn(username, password));
      showApp();
      await start();
    } catch (e2) {
      err.textContent = e2.message || String(e2);
      err.hidden = false;
      document.getElementById('gatePass').select();
    }
  });

  document.getElementById('gateForgot').addEventListener('click', async (e) => {
    e.preventDefault();
    await forgotPasswordFlow();
  });
}

export async function forgotPasswordFlow() {
  const sentTo = await modal({
    title: 'Reset the password',
    subtitle: 'A code will be emailed to the school address',
    body: `<p style="font-size:13px;line-height:1.6;margin:0 0 10px">
        A six-digit code will be sent to the recovery address saved in Settings.
        It stops working after 15 minutes.
      </p>
      <div class="note b">${icon('info')}<div>
        This needs the Gmail app password to be saved in
        <b>Settings → Email &amp; Alerts</b> first.
      </div></div>`,
    buttons: [
      { label: 'Cancel', value: null },
      {
        label: 'Send code', kind: 'pri',
        onClick: () => api.requestReset(),
      },
    ],
  });
  if (!sentTo) return;

  await modal({
    title: 'Enter the code',
    subtitle: `Sent to ${sentTo}`,
    body: `<div class="fld">
        <label>Six-digit code</label>
        <input class="inp mono" name="code" maxlength="6" inputmode="numeric"
               style="letter-spacing:6px;font-size:18px;text-align:center">
      </div>
      <div class="fld">
        <label>New password</label>
        <input class="inp" name="pw" type="password" placeholder="At least 8 characters">
      </div>
      <div class="fld">
        <label>Confirm new password</label>
        <input class="inp" name="pw2" type="password">
      </div>`,
    buttons: [
      { label: 'Cancel', value: null },
      {
        label: 'Set password', kind: 'pri',
        onClick: async (ov) => {
          const code = ov.querySelector('[name=code]').value.trim();
          const pw = ov.querySelector('[name=pw]').value;
          const pw2 = ov.querySelector('[name=pw2]').value;
          if (pw !== pw2) throw new Error('The two passwords do not match.');
          await api.verifyReset(code, pw);
          toast('ok', 'Password updated. Sign in with the new password.');
          return true;
        },
      },
    ],
  });
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

async function start() {
  buildNav();
  await loadInfo();
  clock();
  await refreshDeviceChip();
  setInterval(refreshDeviceChip, 30000);

  document.getElementById('nav').addEventListener('click', (e) => {
    const a = e.target.closest('a[data-page]');
    if (a) go(a.dataset.page);
  });

  const refresh = document.getElementById('btnRefresh');
  refresh.innerHTML = icon('sync');
  refresh.addEventListener('click', () => {
    go(state.current);
    toast('inf', 'Refreshed');
  });

  document.getElementById('bannerFix').addEventListener('click', () => go('settings'));

  document.getElementById('whoBtn').addEventListener('click', async () => {
    const choice = await modal({
      title: 'Account',
      subtitle: 'Signed in on this computer',
      body: `<div class="stat-row"><span class="sl">Version</span>
               <span class="sv">${esc(state.info?.version || '—')}</span></div>
             <div class="stat-row"><span class="sl">Database</span>
               <span class="sv mono" style="font-size:11px">${esc(state.info?.db_path || '—')}</span></div>
             <div class="stat-row"><span class="sl">Database size</span>
               <span class="sv">${esc(state.info?.db_size_kb ?? 0)} KB</span></div>
             <div class="stat-row"><span class="sl">Push listener</span>
               <span class="sv">${state.info?.push_running ? `running on ${state.info.push_port}` : 'stopped'}</span></div>`,
      buttons: [
        { label: 'Close', value: null },
        { label: 'Sign out', kind: 'dan', value: 'out' },
      ],
    });
    if (choice === 'out') {
      sessionStorage.removeItem('jws-signed-in');
      location.reload();
    }
  });

  // Live punches: refresh the dashboard if that is what is on screen.
  listen('punch', (p) => {
    toast('inf', `${p.full_name || `Enrolment ${p.enroll_no}`} · ${p.punch_time.slice(11, 16)}`);
    if (state.current === 'dashboard') go('dashboard');
  });
  listen('device-online', (serial) => {
    toast('ok', `Terminal ${serial} connected`);
    refreshDeviceChip();
  });

  await go('dashboard');
}

async function boot() {
  wireGate();

  // In a browser the backend is a fake, so there is nothing to protect; skip
  // straight to the application. In the installed app, always ask.
  if (!isDesktop() || sessionStorage.getItem('jws-signed-in')) {
    showApp();
    await start();
  } else {
    document.getElementById('gate').hidden = false;
    document.getElementById('gatePass').focus();
  }
}

boot();
