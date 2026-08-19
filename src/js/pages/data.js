import { api } from '../api.js';
import { icon, esc, table, toast, withBusy, todayIso, monthStart } from '../ui.js';

export default {
  async mount(host) {
    const [devices, depts, members] = await Promise.all([
      api.listDevices(), api.listDepartments(), api.listMembers(),
    ]);

    const devOptions = devices.length
      ? devices.map((d) => `<option value="${d.id}">${esc(d.name)} — ${esc(d.ip)}</option>`).join('')
      : '<option value="">No terminal registered</option>';

    const active = members.filter((m) => m.status === 'Active');

    host.innerHTML = `
      ${devices.length ? '' : `<div class="note y" style="margin-bottom:14px">${icon('warn')}<div>
        No terminal is registered yet. Add one on the <b>Devices</b> screen before transferring anything.
      </div></div>`}

      <div class="g3">
        <div class="card">
          <div class="card-h"><div class="ico">${icon('up')}</div>
            <div class="ht"><h3>Send Users to Terminal</h3><p>This computer → device</p></div></div>
          <div class="card-b">
            <p style="font-size:12.5px;color:var(--ink-2);margin-bottom:13px">
              Queue staff records so the terminal knows who each enrolment number belongs to.
            </p>
            <div class="fld"><label>Scope</label>
              <label class="cb" style="margin-bottom:8px">
                <input type="radio" name="scope" value="all" checked>
                <div class="ct"><b>All active members</b><span>${active.length} currently active</span></div></label>
              <label class="cb" style="margin-bottom:8px">
                <input type="radio" name="scope" value="dept">
                <div class="ct"><b>One department</b><span>Only staff in the chosen group</span></div></label>
              <label class="cb">
                <input type="radio" name="scope" value="pick">
                <div class="ct"><b>Chosen members</b><span>Hand-pick from the list</span></div></label>
            </div>
            <div id="scopeExtra"></div>
            <button class="btn pri" style="width:100%" id="btnUpload">${icon('up')} Queue upload</button>
            <div class="hint" style="margin-top:8px">
              The terminal collects queued changes on its next check-in, usually within a minute.
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-h"><div class="ico b">${icon('down')}</div>
            <div class="ht"><h3>Read Users from Terminal</h3><p>Device → this computer</p></div></div>
          <div class="card-b">
            <p style="font-size:12.5px;color:var(--ink-2);margin-bottom:13px">
              Pick up anyone enrolled directly on the terminal keypad and add them here.
            </p>
            <div class="fld"><label>Terminal</label>
              <select class="inp" id="dnDev">${devOptions}</select></div>
            <div class="note b" style="margin-bottom:14px">${icon('info')}<div>
              Existing records are never overwritten — only new enrolment numbers are added.
            </div></div>
            <button class="btn" style="width:100%" id="btnDownUsers">${icon('down')} Read users</button>
          </div>
        </div>

        <div class="card">
          <div class="card-h"><div class="ico g">${icon('sync')}</div>
            <div class="ht"><h3>Fetch Punch Records</h3><p>Catch up on missed scans</p></div></div>
          <div class="card-b">
            <p style="font-size:12.5px;color:var(--ink-2);margin-bottom:13px">
              Pull everything the terminal has stored. Safe to run at any time —
              records already held are skipped.
            </p>
            <div class="fld"><label>Terminal</label>
              <select class="inp" id="lgDev">${devOptions}</select></div>
            <div class="fld">
              <label class="cb"><input type="checkbox" id="clearAfter">
                <div class="ct"><b>Clear the terminal log afterwards</b>
                  <span>Frees memory on the device. Only happens if the transfer succeeds, but it cannot be undone.</span></div>
              </label></div>
            <button class="btn ok" style="width:100%" id="btnLogs">${icon('sync')} Fetch records</button>
          </div>
        </div>
      </div>

      <div class="g2 mt14">
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Recalculate Attendance</h3>
            <p>Rebuild daily records from the raw scans</p></div></div>
          <div class="card-b">
            <p style="font-size:12.5px;color:var(--ink-2);margin-bottom:14px">
              Run this after changing shifts, timetables, holidays or attendance rules.
              Days an administrator corrected by hand, and any locked month, are left untouched.
            </p>
            <div class="grid2">
              <div class="fld"><label>From</label>
                <input type="date" class="inp" id="recFrom" value="${monthStart(todayIso())}"></div>
              <div class="fld"><label>To</label>
                <input type="date" class="inp" id="recTo" value="${todayIso()}"></div>
            </div>
            <button class="btn pri" id="btnRecompute">${icon('sync')} Recalculate</button>
          </div>
        </div>

        <div class="card">
          <div class="card-h"><div class="ht"><h3>Transfer History</h3><p>Most recent jobs</p></div></div>
          <div class="tbl-wrap"><table class="tbl" id="hist"></table></div>
        </div>
      </div>`;

    // --- scope picker ---
    const extra = host.querySelector('#scopeExtra');
    const scopeInputs = host.querySelectorAll('[name=scope]');
    const renderScope = () => {
      const v = [...scopeInputs].find((r) => r.checked)?.value;
      if (v === 'dept') {
        extra.innerHTML = `<div class="fld"><label>Department</label>
          <select class="inp" id="scopeDept">
            ${depts.map((d) => `<option value="${d.id}">${esc(d.name)} (${d.member_count})</option>`).join('')}
          </select></div>`;
      } else if (v === 'pick') {
        extra.innerHTML = `<div class="fld"><label>Members</label>
          <div style="max-height:190px;overflow:auto;border:1px solid var(--line);border-radius:8px;padding:10px">
            ${members.map((m) => `<label class="cb" style="margin-bottom:7px">
              <input type="checkbox" data-mid="${m.id}">
              <div class="ct"><b>${esc(m.full_name)}</b>
                <span>Enrolment ${m.enroll_no} · ${esc(m.dept_name || 'Unassigned')}</span></div></label>`).join('')}
          </div></div>`;
      } else {
        extra.innerHTML = '';
      }
    };
    scopeInputs.forEach((r) => r.addEventListener('change', renderScope));
    renderScope();

    const chosenIds = () => {
      const v = [...scopeInputs].find((r) => r.checked)?.value;
      if (v === 'all') return active.map((m) => m.id);
      if (v === 'dept') {
        const id = Number(host.querySelector('#scopeDept').value);
        return members.filter((m) => m.dept_id === id).map((m) => m.id);
      }
      return [...host.querySelectorAll('[data-mid]:checked')].map((c) => Number(c.dataset.mid));
    };

    const deviceById = (sel) => devices.find((d) => d.id === Number(host.querySelector(sel).value));

    async function loadHistory() {
      const rows = await api.syncHistory(10);
      table(host.querySelector('#hist'), [
        { label: 'When', get: (s) => `<span style="color:var(--ink-3)">${esc(s.ts)}</span>` },
        { label: 'Job', get: (s) => `<b>${esc(s.job)}</b>` },
        { label: 'Device', get: (s) => esc(s.device || '—') },
        { label: 'Result', get: (s) => esc(s.result || '') },
        { label: 'Status', get: (s) => `<span class="tag ${s.ok ? 'g' : 'r'}">${s.ok ? 'OK' : 'Failed'}</span>` },
      ], rows, { empty: 'Nothing transferred yet' });
    }

    // --- actions ---
    host.querySelector('#btnUpload').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const ids = chosenIds();
        if (!ids.length) throw new Error('No members are selected.');
        const n = await api.uploadUsers(ids);
        toast('ok', `${n} update${n === 1 ? '' : 's'} queued for the terminal`);
        await loadHistory();
      }).catch(() => {}),
    );

    host.querySelector('#btnDownUsers').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const d = deviceById('#dnDev');
        if (!d) throw new Error('No terminal is selected.');
        const n = await api.downloadUsers(d.ip, d.port, d.comm_key);
        toast('ok', n ? `${n} new user${n === 1 ? '' : 's'} added` : 'No new users on the terminal');
        await loadHistory();
      }).catch(() => {}),
    );

    host.querySelector('#btnLogs').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const d = deviceById('#lgDev');
        if (!d) throw new Error('No terminal is selected.');
        const clear = host.querySelector('#clearAfter').checked;
        const r = await api.downloadLogs(d.ip, d.port, d.comm_key, d.serial || '', clear);
        toast('ok', `${r.fetched} records read · ${r.accepted} new · ${r.duplicates} already held`);
        await loadHistory();
      }).catch(() => {}),
    );

    host.querySelector('#btnRecompute').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const from = host.querySelector('#recFrom').value;
        const to = host.querySelector('#recTo').value;
        if (!from || !to) throw new Error('Choose both a start and an end date.');
        const n = await api.recompute(from, to);
        toast('ok', `${n} day record${n === 1 ? '' : 's'} rebuilt`);
      }).catch(() => {}),
    );

    await loadHistory();
  },
};
