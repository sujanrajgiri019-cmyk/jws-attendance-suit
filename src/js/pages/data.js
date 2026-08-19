// Data Transfer — everything that moves between the terminal and this PC.
//
// Laid out as a console rather than a form: an action list on the left, a live
// status bar across the top, and a scrolling log of what actually happened.
// Transfers take tens of seconds and sometimes fail halfway; a spinner that
// says nothing is what makes an office restart the app mid-sync.

import { api, listen } from '../api.js';
import { icon, esc, toast, table, todayIso, monthStart, confirmDialog } from '../ui.js';
import { dateField, wireDateFields, bsPretty } from '../nepali.js';

const ACTIONS = [
  { id: 'logs', label: 'Download attendance logs', icon: 'down' },
  { id: 'users_down', label: 'Download user info and FP', icon: 'users' },
  { id: 'users_up', label: 'Upload user info and FP', icon: 'up' },
  { id: 'photos', label: 'Attendance photo management', icon: 'file' },
  { id: 'ac', label: 'AC manage', icon: 'lock' },
  { id: 'recompute', label: 'Recalculate attendance', icon: 'sync' },
];

export default {
  async mount(host) {
    const [devices, members, depts] = await Promise.all([
      api.listDevices(), api.listMembers(), api.listDepartments(),
    ]);

    let action = 'logs';
    let device = devices[0] || null;
    const log = [];

    host.innerHTML = `
      <div class="card">
        <div class="toolbar wrap">
          <div class="tb-f"><label>Terminal</label>
            <select class="inp sm" id="dvPick">
              ${devices.map((d) => `<option value="${d.id}">${esc(d.name)} · ${esc(d.ip)}</option>`).join('')}
              ${devices.length ? '' : '<option value="">No terminal registered</option>'}
            </select></div>
          <div class="dev-chip" id="dvState"><span class="d"></span><span id="dvText">Not checked</span></div>
          <button class="btn sm" id="dvTest">${icon('plug')} Test connection</button>
          <span class="grow"></span>
          <div id="dvGauges" style="min-width:260px"></div>
        </div>
      </div>

      <div class="split mt14">
        <div class="card pane-l">
          <div class="card-h"><div class="ht"><h3>Actions</h3><p>Pick a job</p></div></div>
          <div class="actions-nav" id="actNav">
            ${ACTIONS.map((a) => `
              <button data-act="${a.id}">${icon(a.icon)}<span>${esc(a.label)}</span></button>`).join('')}
          </div>
        </div>

        <div class="card pane-r">
          <div class="card-b" id="actBody"></div>
          <div class="card-b" style="border-top:1px solid var(--line)">
            <div class="form-head" style="margin-bottom:10px;padding-bottom:9px">
              <h3>Console</h3>
              <div class="fh-b">
                <button class="btn sm" id="logClear">Clear</button>
              </div>
            </div>
            <div class="progress" id="prog" hidden><i style="width:0%"></i></div>
            <div class="console" id="console"></div>
          </div>
        </div>
      </div>

      <div class="card mt14">
        <div class="card-h"><div class="ht"><h3>Transfer history</h3><p>Recent jobs</p></div></div>
        <div class="tbl-wrap"><table class="tbl" id="hist"></table></div>
      </div>`;

    const consoleEl = host.querySelector('#console');
    const body = host.querySelector('#actBody');
    const prog = host.querySelector('#prog');

    /** Write one timestamped line to the console. */
    function say(level, message) {
      const ts = new Date().toTimeString().slice(0, 8);
      log.push({ ts, level, message });
      // Only the last few hundred lines are worth keeping in the DOM.
      if (log.length > 400) log.shift();
      consoleEl.innerHTML = log.map((l) =>
        `<div class="ln"><span class="ts">${esc(l.ts)}</span><span class="${l.level}">${esc(l.message)}</span></div>`).join('');
      consoleEl.scrollTop = consoleEl.scrollHeight;
    }

    const progress = (percent) => {
      prog.hidden = percent === null;
      if (percent !== null) prog.querySelector('i').style.width = `${percent}%`;
    };

    /** Run a job with console framing, so every action reports the same way. */
    async function job(name, fn) {
      if (!device && name !== 'Recalculate attendance') {
        say('error', 'No terminal is selected.');
        toast('err', 'Register a terminal on the Devices screen first.');
        return;
      }
      say('info', `${name}: starting…`);
      progress(15);
      // A transfer can sit waiting on the terminal's check-in. Creep the bar so
      // the screen never looks frozen, but stop short of 90 so it cannot claim
      // to be finished before it is.
      const creep = setInterval(() => {
        const cur = parseFloat(prog.querySelector('i').style.width) || 15;
        if (cur < 88) progress(cur + (88 - cur) * 0.06);
      }, 700);
      try {
        const msg = await fn();
        clearInterval(creep);
        progress(100);
        say('ok', `${name}: ${msg}`);
        toast('ok', msg);
        await loadHistory();
      } catch (e) {
        clearInterval(creep);
        progress(null);
        // Backend messages are written to be read; keep the line breaks.
        String(e.message || e).split('\n').filter(Boolean).forEach((line, i) =>
          say('error', i === 0 ? `${name} failed — ${line}` : line));
        toast('err', e.message || String(e));
        return;
      }
      setTimeout(() => progress(null), 700);
    }

    // --- device status ------------------------------------------------------

    async function testDevice() {
      const chip = host.querySelector('#dvState');
      const text = host.querySelector('#dvText');
      if (!device) {
        chip.classList.add('off');
        text.textContent = 'No terminal registered';
        return;
      }
      text.textContent = 'Checking…';
      try {
        const ms = await api.devicePing(device.ip, device.port);
        chip.classList.remove('off');
        text.textContent = `Connected · ${device.ip}:${device.port} · ${ms} ms`;
        say('ok', `${device.name} answered in ${ms} ms`);
        await loadGauges();
      } catch (e) {
        chip.classList.add('off');
        text.textContent = `Offline · ${device.ip}:${device.port}`;
        say('warn', `${device.name} did not answer: ${e.message || e}`);
      }
    }

    /** Memory gauges, when the terminal will tell us. */
    async function loadGauges() {
      const el = host.querySelector('#dvGauges');
      if (!device) return (el.innerHTML = '');
      try {
        const info = await api.deviceInfo(device.ip, device.port, device.comm_key || 0);
        // The K40 Pro reports its own capacities; where it does not, show the
        // count alone rather than inventing a denominator.
        const bar = (label, used, cap, colour) => {
          const p = cap ? Math.min(100, (used / cap) * 100) : 0;
          return `<div class="gauge">
            <div class="gauge-t"><b>${esc(label)}</b>
              <span>${used}${cap ? ` / ${cap}` : ''}${cap ? ` · ${Math.round(p)}%` : ''}</span></div>
            <div class="gauge-b"><i style="width:${p}%;background:${
              p > 85 ? 'var(--bad)' : p > 60 ? 'var(--warn)' : colour}"></i></div>
          </div>`;
        };
        el.innerHTML =
          bar('Users', info.user_count, 1000, 'var(--brand)') +
          bar('Log capacity', info.log_count, 100000, 'var(--good)');
        say('info', `${info.name || 'Terminal'} · firmware ${info.platform || '—'} · serial ${info.serial || '—'}`);
      } catch {
        el.innerHTML = '<span class="hint">Memory figures need a direct TCP connection.</span>';
      }
    }

    // --- the action panes ---------------------------------------------------

    const devOptions = () =>
      devices.map((d) => `<option value="${d.id}">${esc(d.name)}</option>`).join('');

    function paintAction() {
      host.querySelectorAll('#actNav button').forEach((b) =>
        b.classList.toggle('on', b.dataset.act === action));

      if (action === 'logs') {
        body.innerHTML = `
          <div class="form-head"><h3>Download attendance logs</h3></div>
          <p class="hint mb8">Pull everything the terminal has stored. Records already held
            are skipped, so this is safe to run at any time.</p>
          <div class="note b" id="modeNote">${icon('info')}<div></div></div>
          <label class="cb"><input type="checkbox" id="clearAfter">
            <div class="ct"><b>Clear the terminal log afterwards</b>
              <span>Frees memory on the device. Only happens if the transfer succeeds,
                and it cannot be undone.</span></div></label>
          <button class="btn pri" id="go">${icon('down')} Download logs</button>`;
        paintModeNote();
        return;
      }

      if (action === 'users_down') {
        body.innerHTML = `
          <div class="form-head"><h3>Download user info and fingerprints</h3></div>
          <p class="hint mb8">Picks up anyone enrolled directly on the terminal keypad and
            adds them here. Existing records are never overwritten — only new
            enrolment numbers are added.</p>
          <div class="note b" id="modeNote">${icon('info')}<div></div></div>
          <button class="btn pri" id="go">${icon('users')} Read users from terminal</button>`;
        paintModeNote();
        return;
      }

      if (action === 'users_up') {
        body.innerHTML = `
          <div class="form-head"><h3>Upload user info and fingerprints</h3></div>
          <p class="hint mb8">Push names, privileges and card numbers to the terminal.</p>
          <div class="fld"><label>Who to send</label>
            <div class="radios">
              <label class="rd"><input type="radio" name="scope" value="all" checked><span>Everyone</span></label>
              <label class="rd"><input type="radio" name="scope" value="dept"><span>One department</span></label>
              <label class="rd"><input type="radio" name="scope" value="pick"><span>Choose people</span></label>
            </div></div>
          <div id="scopeExtra"></div>
          <button class="btn pri" id="go">${icon('up')} Upload to terminal</button>`;
        wireScope();
        return;
      }

      if (action === 'photos') {
        body.innerHTML = `
          <div class="form-head"><h3>Attendance photo management</h3></div>
          <div class="note y">${icon('warn')}<div>
            <b>The K40 Pro does not capture attendance photos.</b><br>
            This screen is here for a terminal that does. Photos would be stored
            beside the database rather than inside it — a year of captures is
            several gigabytes, and that does not belong in a file the office
            copies to a memory stick.
          </div></div>
          <button class="btn" id="go" disabled>${icon('file')} Download photos</button>`;
        return;
      }

      if (action === 'ac') {
        body.innerHTML = `
          <div class="form-head"><h3>Access control</h3></div>
          <p class="hint mb8">Time zones say when the door relay may open; a group binds up
            to three of them and is what each member of staff belongs to.</p>
          <div class="note b">${icon('info')}<div>
            Two time zones and two groups are set up: <b>Always open</b> and
            <b>School hours</b> (06:00–19:00, closed Saturday). Members are in
            group 1 unless changed on their record.
          </div></div>
          <div class="note y">${icon('warn')}<div>
            Pushing access rules to the relay is not enabled in this version. The
            definitions are stored and will be sent once the door hardware is
            wired to the terminal.
          </div></div>`;
        return;
      }

      paintModeNote();
      body.innerHTML = `
        <div class="form-head"><h3>Recalculate attendance</h3></div>
        <p class="hint mb8">Run this after changing shifts, timetables, holidays or rules.
          Days an administrator corrected by hand, and any locked month, are left untouched.</p>
        <div class="grid2">
          ${dateField('recFrom', 'From', monthStart(todayIso()), { id: 'recFrom' })}
          ${dateField('recTo', 'To', todayIso(), { id: 'recTo' })}
        </div>
        <button class="btn pri" id="go">${icon('sync')} Recalculate</button>`;
    }

    /** Say which way round this terminal talks, because it changes what happens. */
    function paintModeNote() {
      const note = body.querySelector('#modeNote div');
      if (!note) return;
      note.innerHTML = device?.mode === 'push'
        ? `<b>${esc(device.name)} is in push mode.</b> It reports to this PC rather than
           being dialled, so this asks the terminal to send everything and then waits for
           it to arrive — usually a few seconds, up to two minutes if the terminal has a
           long check-in interval. Progress appears in the console below.`
        : 'This dials the terminal directly on port 4370. A full terminal takes up to a '
          + 'minute; the app stays usable while it works.';
    }

    function wireScope() {
      const extra = body.querySelector('#scopeExtra');
      const paint = () => {
        const v = [...body.querySelectorAll('[name=scope]')].find((r) => r.checked)?.value;
        if (v === 'dept') {
          extra.innerHTML = `<div class="fld"><label>Department</label>
            <select class="inp" id="scopeDept">
              ${depts.map((d) => `<option value="${d.id}">${esc(d.name)} (${d.member_count})</option>`).join('')}
            </select></div>`;
        } else if (v === 'pick') {
          extra.innerHTML = `<div class="fld"><label>Members</label>
            <div class="picklist">
              ${members.map((m) => `<label class="cb">
                <input type="checkbox" data-mid="${m.id}">
                <div class="ct"><b>${esc(m.full_name)}</b>
                  <span>Enrolment ${m.enroll_no} · ${esc(m.dept_name || 'Unassigned')}</span></div>
              </label>`).join('')}
            </div></div>`;
        } else {
          extra.innerHTML = '';
        }
      };
      body.querySelectorAll('[name=scope]').forEach((r) => r.addEventListener('change', paint));
      paint();
    }

    function chosenIds() {
      const v = [...body.querySelectorAll('[name=scope]')].find((r) => r.checked)?.value;
      if (v === 'all') return members.map((m) => m.id);
      if (v === 'dept') {
        const d = Number(body.querySelector('#scopeDept')?.value);
        return members.filter((m) => m.dept_id === d).map((m) => m.id);
      }
      return [...body.querySelectorAll('[data-mid]:checked')].map((c) => Number(c.dataset.mid));
    }

    // --- wiring -------------------------------------------------------------

    host.querySelector('#actNav').addEventListener('click', (e) => {
      const b = e.target.closest('[data-act]');
      if (!b) return;
      action = b.dataset.act;
      paintAction();
    });

    host.querySelector('#dvPick').addEventListener('change', (e) => {
      device = devices.find((d) => String(d.id) === e.target.value) || null;
      testDevice();
    });
    host.querySelector('#dvTest').addEventListener('click', testDevice);
    host.querySelector('#logClear').addEventListener('click', () => {
      log.length = 0;
      consoleEl.innerHTML = '';
    });

    body.addEventListener('click', async (e) => {
      if (!e.target.closest('#go')) return;
      const btn = e.target.closest('#go');
      btn.disabled = true;
      try {
        if (action === 'logs') {
          const clear = body.querySelector('#clearAfter')?.checked;
          if (clear && !(await confirmDialog('Clear the terminal after downloading?',
            'The device log will be erased once the records are safely stored here. This cannot be undone.',
            'Download and clear'))) return;
          await job('Download attendance logs', async () => {
            const r = await api.downloadLogs(
              device.ip, device.port, device.comm_key || 0, device.serial || '', !!clear);
            return device.mode === 'push'
              ? `${r.accepted} new record${r.accepted === 1 ? '' : 's'} received${
                  r.recomputed ? `, ${r.recomputed} day records rebuilt` : ''}`
              : `${r.fetched} records read, ${r.accepted} new, ${r.duplicates} already held`;
          });
        } else if (action === 'users_down') {
          await job('Download users', async () => {
            const n = await api.downloadUsers(device.ip, device.port, device.comm_key || 0);
            // A push-mode terminal is asked rather than dialled, so nothing has
            // arrived yet. Saying "0 users" would read as a failure.
            return `${n} staff now in the database`;
          });
        } else if (action === 'users_up') {
          const ids = chosenIds();
          if (!ids.length) throw new Error('Nobody is selected.');
          await job('Upload users', async () => {
            const n = await api.uploadUsers(ids);
            return `${n} update${n === 1 ? '' : 's'} queued for the terminal`;
          });
        } else if (action === 'recompute') {
          const from = body.querySelector('#recFrom').value;
          const to = body.querySelector('#recTo').value;
          if (!from || !to) throw new Error('Choose both a start and an end date.');
          await job('Recalculate attendance', async () => {
            const n = await api.recompute(from, to);
            return `${n} day record${n === 1 ? '' : 's'} rebuilt`;
          });
        }
      } catch (err) {
        say('error', err.message || String(err));
        toast('err', err.message || String(err));
      } finally {
        btn.disabled = false;
      }
    });

    async function loadHistory() {
      const rows = await api.syncHistory(15);
      table(host.querySelector('#hist'), [
        { label: 'When', get: (s) => `<span class="mono">${esc(s.ts)}</span>` },
        { label: 'Job', get: (s) => `<b>${esc(s.job)}</b>` },
        { label: 'Device', get: (s) => esc(s.device || '—') },
        { label: 'Result', get: (s) => esc(s.result || '') },
        { label: 'Status', get: (s) => `<span class="tag ${s.ok ? 'g' : 'r'}">${s.ok ? 'OK' : 'Failed'}</span>` },
      ], rows, { empty: 'Nothing transferred yet' });
    }

    // Punches arriving over the push listener are worth a console line too:
    // they are the clearest sign the terminal is actually talking to this PC.
    const stop = await listen('punch', (p) => {
      say('info', `Scan · enrolment ${p.enroll_no} at ${String(p.punch_time).slice(11, 16)}`);
    });

    // The backend narrates a transfer as it happens — waiting for the terminal
    // to check in, then what each batch contained.
    const stopProgress = await listen('transfer-progress', (line) => {
      say('info', String(line));
    });

    wireDateFields(body);
    paintAction();
    say('info', 'Console ready.');
    await testDevice();
    await loadHistory();

    return () => {
      if (typeof stop === 'function') stop();
      if (typeof stopProgress === 'function') stopProgress();
    };
  },
};
