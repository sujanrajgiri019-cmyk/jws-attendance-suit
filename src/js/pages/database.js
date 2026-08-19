import { api } from '../api.js';
import { icon, esc, toast, loadingTable, withBusy } from '../ui.js';

const TABS = [
  { key: 'attendance', label: 'Attendance', hint: 'One computed row per member per day' },
  { key: 'punches', label: 'Raw Punches', hint: 'Exactly as the terminal reported them — never edited' },
  { key: 'members', label: 'Members', hint: 'The staff master list' },
  { key: 'departments', label: 'Departments', hint: '' },
  { key: 'shifts', label: 'Shifts', hint: '' },
  { key: 'holidays', label: 'Holidays', hint: '' },
  { key: 'device_commands', label: 'Device Queue', hint: 'Commands waiting for a terminal to collect' },
  { key: 'sync_log', label: 'Sync Log', hint: '' },
  { key: 'audit_log', label: 'Audit Trail', hint: 'Who changed what, and when' },
  { key: 'settings', label: 'Settings', hint: '' },
  { key: 'rules', label: 'Rules', hint: '' },
];

export default {
  async mount(host) {
    let current = 'attendance';
    let filter = '';

    host.innerHTML = `
      <div class="tabs" id="tabs">
        ${TABS.map((t) => `<button data-key="${t.key}" class="${t.key === current ? 'on' : ''}">${esc(t.label)}</button>`).join('')}
      </div>
      <div class="tbar">
        <div class="srch">${icon('search')}
          <input class="inp" id="q" placeholder="Filter the rows shown below…"></div>
        <div class="sp"></div>
        <span class="tag n" id="meta">—</span>
        <button class="btn" id="btnCsv">${icon('file')} Export CSV</button>
        <button class="btn" id="btnBackup">${icon('save')} Back up database</button>
      </div>
      <div class="card">
        <div class="card-h">
          <div class="ht"><h3 id="title">Attendance</h3><p id="hint"></p></div>
        </div>
        <div class="tbl-wrap" style="max-height:calc(100vh - 290px)">
          <table class="tbl" id="tbl"></table>
        </div>
      </div>`;

    const tblEl = host.querySelector('#tbl');
    let rows = [];

    async function load() {
      const tab = TABS.find((t) => t.key === current);
      host.querySelector('#title').textContent = tab.label;
      host.querySelector('#hint').textContent = tab.hint;
      loadingTable(tblEl, 8);
      try {
        const res = await api.browseTable(current, 2000, 0);
        rows = res.rows;
        host.querySelector('#meta').textContent =
          `${res.total.toLocaleString()} row${res.total === 1 ? '' : 's'} · table: ${current}`;
        render();
      } catch (e) {
        tblEl.innerHTML = `<tbody><tr><td><div class="empty">
          <div class="ei">${icon('warn')}</div><b>Could not read this table</b>
          <p>${esc(e.message)}</p></div></td></tr></tbody>`;
      }
    }

    function render() {
      const q = filter.toLowerCase();
      const shown = q
        ? rows.filter((r) => Object.values(r).some((v) => String(v ?? '').toLowerCase().includes(q)))
        : rows;

      if (!shown.length) {
        tblEl.innerHTML = `<tbody><tr><td><div class="empty">
          <div class="ei">${icon('db')}</div>
          <b>${rows.length ? 'Nothing matches that filter' : 'This table is empty'}</b>
          <p>${rows.length ? 'Try a different search term.' : 'Rows appear here once the system has data.'}</p>
        </div></td></tr></tbody>`;
        return;
      }

      const cols = Object.keys(shown[0]);
      // Cap the rendered rows: a browser table of 2000 rows in a WebView is
      // sluggish, and nobody reads past the first few hundred anyway.
      const cap = 400;
      const body = shown.slice(0, cap);

      tblEl.innerHTML =
        `<thead><tr>${cols.map((c) => `<th>${esc(c)}</th>`).join('')}</tr></thead>` +
        `<tbody>${body.map((r) => `<tr>${cols.map((c) => {
          const v = r[c];
          if (v === null || v === undefined) return '<td style="color:var(--ink-4)">null</td>';
          if (c === 'status') return `<td>${esc(String(v))}</td>`;
          const isNum = typeof v === 'number';
          return `<td class="${isNum ? 'num mono' : ''}">${esc(String(v))}</td>`;
        }).join('')}</tr>`).join('')}</tbody>`;

      if (shown.length > cap) {
        host.querySelector('#meta').textContent += ` · showing first ${cap}`;
      }
    }

    host.querySelector('#tabs').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-key]');
      if (!b) return;
      host.querySelectorAll('#tabs button').forEach((x) => x.classList.remove('on'));
      b.classList.add('on');
      current = b.dataset.key;
      load();
    });

    let t;
    host.querySelector('#q').addEventListener('input', (e) => {
      filter = e.target.value.trim();
      clearTimeout(t);
      t = setTimeout(render, 160);
    });

    host.querySelector('#btnCsv').addEventListener('click', () => {
      if (!rows.length) return toast('inf', 'Nothing to export');
      const cols = Object.keys(rows[0]);
      const esc2 = (v) => {
        const s = String(v ?? '');
        return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
      };
      const csv = [cols.join(','), ...rows.map((r) => cols.map((c) => esc2(r[c])).join(','))].join('\n');
      const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = `jws-${current}.csv`;
      a.click();
      setTimeout(() => URL.revokeObjectURL(a.href), 2000);
      toast('ok', `${current}.csv downloaded`);
    });

    host.querySelector('#btnBackup').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const path = await api.backupNow();
        toast('ok', `Backup written to ${path}`);
      }).catch(() => {}),
    );

    await load();
  },
};
