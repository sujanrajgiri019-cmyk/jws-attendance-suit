// Timetables — the three-tier roster, one sub-tab per tier.
//
//   1. Timetable Maintenance   one block of duty      "10:00 to 16:30"
//   2. Shift Management        a cycle of blocks      Sun-Fri regular, Sat off
//   3. Employee Schedule       who is on which shift  from 15 Jan, no end
//
// Each tab reads the tier below it, so the order on screen is the order the
// office has to fill them in — and an empty tab tells you which tier is missing.

import { api } from '../api.js';
import {
  icon, esc, toast, modal, confirmDialog, withBusy, table, emptyState,
  todayIso, addDays, person, hhmm,
} from '../ui.js';
import { dateField, wireDateFields, bsPretty, adPretty } from '../nepali.js';

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
const SHORT = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

const TABS = [
  { id: 'blocks', label: 'Timetable Maintenance', icon: 'clock' },
  { id: 'shifts', label: 'Shift Management', icon: 'layers' },
  { id: 'roster', label: 'Employee Schedule', icon: 'users' },
];

/** Minutes from midnight, tolerating a blank or malformed value. */
const mins = (hhmmStr) => {
  const m = /^(\d{1,2}):(\d{2})/.exec(String(hhmmStr || ''));
  return m ? Number(m[1]) * 60 + Number(m[2]) : 0;
};

/** Percentage across a 24-hour bar. */
const pct = (m) => `${Math.max(0, Math.min(100, (m / 1440) * 100))}%`;

// ===========================================================================
// 1. Timetable Maintenance
// ===========================================================================

const BLANK_TT = {
  id: null, name: '', on_duty: '09:00', off_duty: '16:00',
  in_begin: '06:00', in_end: '12:30', out_begin: '12:30', out_end: '21:00',
  late_grace: 10, early_grace: 10, break_min: 0, workday_value: 1,
  work_minutes: 0, must_c_in: true, must_c_out: true, count_ot: true,
  min_ot_block: 30, colour: '#F16522', active: true,
};

async function blocksTab(host) {
  let all = await api.listTimetables();
  let current = all[0] ? { ...all[0] } : { ...BLANK_TT };
  let filter = '';

  host.innerHTML = `
    <div class="split">
      <div class="card pane-l">
        <div class="card-h">
          <div class="ht"><h3>Timetables</h3><p>Blocks of duty</p></div>
          <button class="btn sm pri" id="ttNew">${icon('plus')} Add</button>
        </div>
        <div class="pane-search">
          <input class="inp" id="ttSearch" placeholder="Search timetables…">
        </div>
        <div class="pane-list" id="ttList"></div>
      </div>
      <div class="card pane-r"><div class="card-b" id="ttForm"></div></div>
    </div>`;

    const list = host.querySelector('#ttList');
    const form = host.querySelector('#ttForm');

  function paintList() {
    const q = filter.toLowerCase();
    const shown = all.filter((t) => !q || t.name.toLowerCase().includes(q));
    if (!shown.length) {
      list.innerHTML = `<div class="pane-empty">${
        all.length ? 'Nothing matches that search.' : 'No timetables yet. Add the first one.'}</div>`;
      return;
    }
    list.innerHTML = shown.map((t) => `
      <button class="pane-item${t.id === current.id ? ' on' : ''}" data-id="${t.id}">
        <span class="swatch" style="background:${esc(t.colour)}"></span>
        <span class="pi-t">
          <b>${esc(t.name)}</b>
          <span>${esc(hhmm(t.on_duty))} – ${esc(hhmm(t.off_duty))}${
            t.active ? '' : ' · off'}</span>
        </span>
        ${t.used_by ? `<span class="tag n">${t.used_by}</span>` : ''}
      </button>`).join('');
  }

  function paintForm() {
    const t = current;
    const f = (k, label, type = 'time', extra = '') => `
      <div class="fld"><label>${esc(label)}</label>
        <input class="inp" type="${type}" name="${k}" value="${esc(t[k] ?? '')}" ${extra}></div>`;

    form.innerHTML = `
      <div class="form-head">
        <h3>${t.id ? 'Edit timetable' : 'New timetable'}</h3>
        <div class="fh-b">
          ${t.id ? `<button class="btn dan sm" id="ttDel">${icon('trash')} Delete</button>` : ''}
          <button class="btn pri sm" id="ttSave">${icon('check')} Post</button>
        </div>
      </div>

      <div class="fld"><label>Timetable name</label>
        <input class="inp" name="name" value="${esc(t.name)}" placeholder="e.g. 10 to 4.30"></div>

      <div class="grid2">
        ${f('on_duty', 'On duty time')}
        ${f('off_duty', 'Off duty time')}
      </div>

      <div class="sec-lbl">Valid check-in window</div>
      <div class="grid2">
        ${f('in_begin', 'Beginning in')}
        ${f('in_end', 'Ending in')}
      </div>

      <div class="sec-lbl">Valid check-out window</div>
      <div class="grid2">
        ${f('out_begin', 'Beginning out')}
        ${f('out_end', 'Ending out')}
      </div>

      <div class="sec-lbl">Grace and value</div>
      <div class="grid2">
        ${f('late_grace', 'Late grace (mins)', 'number', 'min="0" max="600"')}
        ${f('early_grace', 'Early leave grace (mins)', 'number', 'min="0" max="600"')}
        ${f('break_min', 'Unpaid break (mins)', 'number', 'min="0" max="600"')}
        ${f('min_ot_block', 'Minimum OT block (mins)', 'number', 'min="0" max="600"')}
        ${f('workday_value', 'Workday value', 'number', 'min="0" max="2" step="0.25"')}
        ${f('work_minutes', 'Custom paid minutes', 'number', 'min="0" max="1440"')}
      </div>

      <div class="sec-lbl">Rules</div>
      <label class="cb"><input type="checkbox" name="must_c_in" ${t.must_c_in ? 'checked' : ''}>
        <div class="ct"><b>Must C/In</b><span>The day is incomplete without an arrival scan</span></div></label>
      <label class="cb"><input type="checkbox" name="must_c_out" ${t.must_c_out ? 'checked' : ''}>
        <div class="ct"><b>Must C/Out</b><span>The day is incomplete without a departure scan</span></div></label>
      <label class="cb"><input type="checkbox" name="count_ot" ${t.count_ot ? 'checked' : ''}>
        <div class="ct"><b>Count overtime</b><span>Time past off duty can earn OT</span></div></label>
      <label class="cb"><input type="checkbox" name="active" ${t.active ? 'checked' : ''}>
        <div class="ct"><b>In use</b><span>Switch off to retire it without deleting</span></div></label>

      <div class="sec-lbl">Display colour</div>
      <div class="colr">
        <input type="color" name="colour" value="${esc(t.colour)}">
        <input class="inp mono" type="text" name="colour_hex" value="${esc(t.colour)}" maxlength="7">
      </div>

      <div class="sec-lbl">How this looks on the timeline</div>
      ${dayBar([{ ...t, timetable_name: t.name || 'This timetable' }])}`;
  }

  /** Read the form back into `current`. */
  function harvest() {
    const g = (n) => form.querySelector(`[name="${n}"]`);
    const v = (n) => g(n)?.value ?? '';
    const b = (n) => !!g(n)?.checked;
    return {
      ...current,
      name: v('name').trim(),
      on_duty: v('on_duty'), off_duty: v('off_duty'),
      in_begin: v('in_begin'), in_end: v('in_end'),
      out_begin: v('out_begin'), out_end: v('out_end'),
      late_grace: Number(v('late_grace')) || 0,
      early_grace: Number(v('early_grace')) || 0,
      break_min: Number(v('break_min')) || 0,
      min_ot_block: Number(v('min_ot_block')) || 0,
      workday_value: Number(v('workday_value')) || 0,
      work_minutes: Number(v('work_minutes')) || 0,
      must_c_in: b('must_c_in'), must_c_out: b('must_c_out'),
      count_ot: b('count_ot'), active: b('active'),
      colour: v('colour') || '#F16522',
    };
  }

  host.querySelector('#ttSearch').addEventListener('input', (e) => {
    filter = e.target.value;
    paintList();
  });

  host.querySelector('#ttNew').addEventListener('click', () => {
    current = { ...BLANK_TT };
    paintList();
    paintForm();
    form.querySelector('[name=name]')?.focus();
  });

  list.addEventListener('click', (e) => {
    const b = e.target.closest('[data-id]');
    if (!b) return;
    const found = all.find((t) => String(t.id) === b.dataset.id);
    if (found) {
      current = { ...found };
      paintList();
      paintForm();
    }
  });

  form.addEventListener('input', (e) => {
    // Keep the colour swatch and hex box together, and redraw the preview bar.
    if (e.target.name === 'colour' || e.target.name === 'colour_hex') {
      const val = e.target.value;
      if (/^#[0-9a-f]{6}$/i.test(val)) {
        form.querySelector('[name=colour]').value = val;
        form.querySelector('[name=colour_hex]').value = val;
      }
    }
    if (['on_duty', 'off_duty', 'colour', 'colour_hex', 'name'].includes(e.target.name)) {
      current = harvest();
      const bar = form.querySelector('.daybar');
      if (bar) {
        bar.outerHTML = dayBar([{ ...current, timetable_name: current.name || 'This timetable' }]);
      }
    }
  });

  form.addEventListener('click', async (e) => {
    if (e.target.closest('#ttSave')) {
      const btn = e.target.closest('#ttSave');
      await withBusy(btn, async () => {
        current = harvest();
        const id = await api.saveTimetable(current);
        all = await api.listTimetables();
        current = { ...(all.find((t) => t.id === id) || current) };
        paintList();
        paintForm();
        toast('ok', `Timetable "${current.name}" saved`);
      }).catch(() => {});
    }
    if (e.target.closest('#ttDel')) {
      const name = current.name;
      if (!(await confirmDialog('Delete this timetable?',
        `"${name}" will be removed. Shifts using it must be changed first.`, 'Delete'))) return;
      try {
        await api.deleteTimetable(current.id);
        all = await api.listTimetables();
        current = all[0] ? { ...all[0] } : { ...BLANK_TT };
        paintList();
        paintForm();
        toast('ok', `"${name}" deleted`);
      } catch (err) {
        toast('err', err.message);
      }
    }
  });

  paintList();
  paintForm();
}

/** A 24-hour bar with one coloured block per timetable. */
function dayBar(blocks, { onDelete = false } = {}) {
  const ticks = [0, 3, 6, 9, 12, 15, 18, 21, 24]
    .map((h) => `<span class="tick" style="left:${pct(h * 60)}">${h}:00</span>`)
    .join('');

  const bars = blocks.map((b) => {
    const start = mins(b.on_duty);
    let end = mins(b.off_duty);
    // A block that crosses midnight is drawn to the right edge; the remainder
    // belongs to the next day's bar, not this one.
    const overnight = end <= start;
    if (overnight) end = 1440;
    const width = Math.max(1.2, ((end - start) / 1440) * 100);
    return `<div class="blk" style="left:${pct(start)};width:${width}%;background:${esc(b.colour)}"
              title="${esc(b.timetable_name || b.name || '')} ${esc(hhmm(b.on_duty))}–${esc(hhmm(b.off_duty))}">
        <span>${esc(b.timetable_name || b.name || '')}</span>
        ${overnight ? '<i class="wrap">→</i>' : ''}
        ${onDelete && b.id ? `<button class="blk-x" data-item="${b.id}" title="Remove">×</button>` : ''}
      </div>`;
  }).join('');

  return `<div class="daybar"><div class="ticks">${ticks}</div>
    <div class="track">${bars || '<span class="rest">Rest day</span>'}</div></div>`;
}

// ===========================================================================
// 2. Shift Management
// ===========================================================================

async function shiftsTab(host) {
  let shifts = await api.listShifts();
  let timetables = (await api.listTimetables()).filter((t) => t.active);
  let current = shifts[0] || null;
  let grid = current ? await api.shiftGrid(current.id) : [];
  let selectedDay = 0;

  host.innerHTML = `
    <div class="split wide-r">
      <div class="card pane-l">
        <div class="card-h">
          <div class="ht"><h3>Shifts</h3><p>Repeating cycles</p></div>
          <button class="btn sm pri" id="shNew">${icon('plus')} Add</button>
        </div>
        <div class="pane-list" id="shList"></div>
      </div>
      <div class="card pane-r"><div class="card-b" id="shBody"></div></div>
    </div>`;

  const list = host.querySelector('#shList');
  const body = host.querySelector('#shBody');

  function paintList() {
    if (!shifts.length) {
      list.innerHTML = `<div class="pane-empty">No shifts yet. Add one, then drop
        timetables onto its days.</div>`;
      return;
    }
    list.innerHTML = shifts.map((s) => `
      <button class="pane-item${current && s.id === current.id ? ' on' : ''}" data-id="${s.id}">
        <span class="pi-t">
          <b>${esc(s.name)}</b>
          <span>${s.cycle_num} ${esc(s.cycle_unit.toLowerCase())}${s.cycle_num > 1 ? 's' : ''}
            · ${s.assigned} assigned${s.active ? '' : ' · off'}</span>
        </span>
      </button>`).join('');
  }

  /** How many day slots this cycle has. */
  const slotCount = (s) => (s.cycle_unit === 'Month' ? 31 : 7) * s.cycle_num;

  const slotLabel = (s, i) =>
    s.cycle_unit === 'Month'
      ? `Day ${(i % 31) + 1}${s.cycle_num > 1 ? ` · month ${Math.floor(i / 31) + 1}` : ''}`
      : `${DAYS[i % 7]}${s.cycle_num > 1 ? ` · week ${Math.floor(i / 7) + 1}` : ''}`;

  function paintBody() {
    if (!current) {
      body.innerHTML = '';
      emptyState(body, 'No shift selected', 'Add a shift to build its weekly plan.', 'layers');
      return;
    }
    const n = slotCount(current);
    const rows = Array.from({ length: n }, (_, i) => {
      const blocks = grid.filter((g) => g.day_index === i);
      return `<div class="grow-row${i === selectedDay ? ' on' : ''}" data-day="${i}">
          <div class="gr-day">
            <b>${esc(slotLabel(current, i))}</b>
            <span>${blocks.length ? `${blocks.length} block${blocks.length > 1 ? 's' : ''}` : 'Rest day'}</span>
          </div>
          ${dayBar(blocks, { onDelete: true })}
        </div>`;
    }).join('');

    body.innerHTML = `
      <div class="form-head">
        <h3>${esc(current.name)}</h3>
        <div class="fh-b">
          <button class="btn sm" id="shEdit">${icon('cog')} Edit</button>
          <button class="btn sm dan" id="shDel">${icon('trash')} Delete</button>
        </div>
      </div>
      <div class="toolbar">
        <button class="btn sm pri" id="shAddTime">${icon('plus')} Add time</button>
        <button class="btn sm" id="shClearDay">Clear day</button>
        <button class="btn sm dan" id="shClearAll">Clear all</button>
        <span class="grow"></span>
        <span class="hint">Select a day, then add a timetable to it.</span>
      </div>
      ${n > 40 ? `<p class="hint mb8">Showing all ${n} days of the cycle.</p>` : ''}
      <div class="growgrid">${rows}</div>`;
  }

  async function reloadGrid() {
    grid = current ? await api.shiftGrid(current.id) : [];
    paintBody();
  }

  async function editShift(shift) {
    const isNew = !shift;
    const s = shift || {
      id: null, name: '', code: '', begin_date: todayIso(),
      cycle_num: 1, cycle_unit: 'Week', active: true,
    };
    const saved = await modal({
      title: isNew ? 'New shift' : 'Edit shift',
      subtitle: 'A shift is a cycle of timetables',
      body: `
        <div class="fld"><label>Shift name</label>
          <input class="inp" name="name" value="${esc(s.name)}" placeholder="e.g. Teachers Sun–Fri"></div>
        <div class="grid2">
          <div class="fld"><label>Code</label>
            <input class="inp" name="code" value="${esc(s.code)}" maxlength="8"></div>
          ${dateField('begin_date', 'Beginning date', s.begin_date)}
        </div>
        <div class="grid2">
          <div class="fld"><label>Cycle num</label>
            <input class="inp" type="number" name="cycle_num" min="1" max="12" value="${s.cycle_num}"></div>
          <div class="fld"><label>Cycle type</label>
            <select class="inp" name="cycle_unit">
              <option value="Week" ${s.cycle_unit === 'Week' ? 'selected' : ''}>Week</option>
              <option value="Month" ${s.cycle_unit === 'Month' ? 'selected' : ''}>Month</option>
            </select></div>
        </div>
        <label class="cb"><input type="checkbox" name="active" ${s.active ? 'checked' : ''}>
          <div class="ct"><b>In use</b><span>Switch off to retire it without deleting</span></div></label>
`,
      onMount: (ov) => wireDateFields(ov),
      buttons: [
        { label: 'Cancel', value: null },
        {
          label: 'Save', kind: 'pri',
          onClick: async (ov) => {
            const g = (n) => ov.querySelector(`[name=${n}]`);
            return api.saveShift({
              id: s.id,
              name: g('name').value.trim(),
              code: g('code').value.trim(),
              begin_date: g('begin_date').value,
              cycle_num: Number(g('cycle_num').value) || 1,
              cycle_unit: g('cycle_unit').value,
              active: g('active').checked,
            });
          },
        },
      ],
    });
    if (!saved) return;
    shifts = await api.listShifts();
    current = shifts.find((x) => x.id === saved) || shifts[0] || null;
    selectedDay = 0;
    paintList();
    await reloadGrid();
    toast('ok', 'Shift saved');
  }

  list.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-id]');
    if (!b) return;
    current = shifts.find((s) => String(s.id) === b.dataset.id) || null;
    selectedDay = 0;
    paintList();
    await reloadGrid();
  });

  host.querySelector('#shNew').addEventListener('click', () => editShift(null));

  body.addEventListener('click', async (e) => {
    const row = e.target.closest('[data-day]');
    if (row && !e.target.closest('.blk-x')) {
      selectedDay = Number(row.dataset.day);
      body.querySelectorAll('.grow-row').forEach((r) =>
        r.classList.toggle('on', r === row));
      return;
    }

    const x = e.target.closest('.blk-x');
    if (x) {
      await api.deleteShiftItem(Number(x.dataset.item));
      await reloadGrid();
      return;
    }

    if (e.target.closest('#shEdit')) return editShift(current);

    if (e.target.closest('#shAddTime')) {
      if (!timetables.length) {
        toast('err', 'Create a timetable first — there is nothing to add.');
        return;
      }
      const picked = await modal({
        title: `Add time to ${slotLabel(current, selectedDay)}`,
        subtitle: 'Choose a block of duty',
        body: `<div class="fld"><label>Timetable</label>
            <select class="inp" name="tt">
              ${timetables.map((t) =>
                `<option value="${t.id}">${esc(t.name)} · ${esc(hhmm(t.on_duty))}–${esc(hhmm(t.off_duty))}</option>`).join('')}
            </select></div>
`,
        buttons: [
          { label: 'Cancel', value: null },
          {
            label: 'Add', kind: 'pri',
            onClick: async (ov) => {
              const tt = Number(ov.querySelector('[name=tt]').value);
              await api.addShiftItem(current.id, selectedDay, tt);
              return true;
            },
          },
        ],
      });
      if (picked) await reloadGrid();
      return;
    }

    if (e.target.closest('#shClearDay')) {
      const onDay = grid.filter((g) => g.day_index === selectedDay);
      for (const g of onDay) await api.deleteShiftItem(g.id);
      await reloadGrid();
      return;
    }

    if (e.target.closest('#shClearAll')) {
      if (!(await confirmDialog('Clear the whole cycle?',
        `Every day of "${current.name}" becomes a rest day.`, 'Clear all'))) return;
      await api.clearShiftGrid(current.id);
      await reloadGrid();
      return;
    }

    if (e.target.closest('#shDel')) {
      if (!(await confirmDialog('Delete this shift?',
        `"${current.name}" and its whole cycle will be removed.`, 'Delete'))) return;
      try {
        await api.deleteShift(current.id);
        shifts = await api.listShifts();
        current = shifts[0] || null;
        paintList();
        await reloadGrid();
        toast('ok', 'Shift deleted');
      } catch (err) {
        toast('err', err.message);
      }
    }
  });

  paintList();
  paintBody();
}

// ===========================================================================
// 3. Employee Schedule
// ===========================================================================

async function rosterTab(host) {
  const depts = await api.departmentTree();
  const shifts = (await api.listShifts()).filter((s) => s.active);
  let deptId = depts[0]?.id ?? null;
  let rows = [];
  let selected = new Set();
  let focusMember = null;
  let from = todayIso();
  let to = addDays(todayIso(), 13);

  host.innerHTML = `
    <div class="split">
      <div class="card pane-l">
        <div class="card-h"><div class="ht"><h3>Departments</h3><p>Pick a group</p></div></div>
        <div class="pane-list" id="tree"></div>
      </div>
      <div class="card pane-r">
        <div class="toolbar">
          <button class="btn sm pri" id="rsArrange">${icon('users')} Arrange shifts</button>
          <button class="btn sm" id="rsTemp">${icon('clock')} Temporary shift</button>
          <span class="grow"></span>
          <span class="hint" id="rsCount"></span>
        </div>
        <div class="tbl-wrap"><table class="tbl" id="rsTable"></table></div>
      </div>
    </div>

    <div class="card mt14">
      <div class="card-h">
        <div class="ht"><h3>Schedule calendar</h3><p id="calWho">Select a member of staff above</p></div>
        <div class="fh-b">
          ${dateField('calFrom', '', from, { id: 'calFrom', small: true })}
          ${dateField('calTo', '', to, { id: 'calTo', small: true })}
          <button class="btn sm" id="calGo">${icon('sync')} Show</button>
        </div>
      </div>
      <div class="card-b" id="calBody"></div>
    </div>`;

  const tree = host.querySelector('#tree');
  const tbl = host.querySelector('#rsTable');
  const calBody = host.querySelector('#calBody');

  function paintTree() {
    tree.innerHTML = `
      <button class="pane-item${deptId === null ? ' on' : ''}" data-dept="">
        <span class="pi-t"><b>All staff</b><span>Whole school</span></span>
      </button>
      ${depts.map((d) => `
        <button class="pane-item${d.id === deptId ? ' on' : ''}" data-dept="${d.id}">
          <span class="swatch" style="background:${esc(d.colour)}"></span>
          <span class="pi-t"><b>${esc(d.name)}</b><span>${d.member_count} staff</span></span>
        </button>`).join('')}`;
  }

  async function loadRoster() {
    rows = await api.roster(deptId);
    selected = new Set();
    paintTable();
  }

  function paintTable() {
    // One member may hold several schedule rows — a standing one and a
    // temporary one. Group so the table reads one line per person, with the
    // temporary arrangement named on the line it applies to.
    const byMember = new Map();
    for (const r of rows) {
      if (!byMember.has(r.member_id)) byMember.set(r.member_id, []);
      byMember.get(r.member_id).push(r);
    }

    const list = [...byMember.entries()].map(([id, rs]) => {
      const temp = rs.find((r) => r.is_temporary);
      const std = rs.find((r) => !r.is_temporary && r.shift_id);
      const base = rs[0];
      return { id, base, std, temp };
    });

    host.querySelector('#rsCount').textContent =
      `${list.length} staff · ${selected.size} selected`;

    table(tbl, [
      {
        label: '<input type="checkbox" id="rsAll">', rawLabel: true,
        get: (r) => `<input type="checkbox" data-pick="${r.id}" ${
          selected.has(r.id) ? 'checked' : ''}>`,
      },
      { label: 'AC No.', get: (r) => `<span class="mono">${r.base.enroll_no}</span>` },
      { label: 'Name', get: (r) => person(r.base.member_name, null, r.base.enroll_no) },
      {
        label: 'Current shift schedule',
        get: (r) => r.std
          ? esc(r.std.shift_name)
          : '<span class="tag y">Not scheduled</span>',
      },
      { label: 'Start date', get: (r) => esc(r.std?.start_date || '—') },
      { label: 'End date', get: (r) => esc(r.std?.end_date || 'Open') },
      {
        label: 'TempShift',
        get: (r) => r.temp
          ? `<span class="tag v">${esc(r.temp.shift_name)}</span>`
          : '<span class="dim">—</span>',
      },
      {
        label: 'Shift definition date range',
        get: (r) => r.temp
          ? esc(`${r.temp.start_date} → ${r.temp.end_date || 'open'}`)
          : '<span class="dim">—</span>',
      },
      {
        label: '',
        get: (r) => `<button class="btn sm" data-cal="${r.id}">Calendar</button>`,
      },
    ], list, { empty: 'No staff in this department' });
  }

  async function paintCalendar(memberId) {
    focusMember = memberId;
    const who = rows.find((r) => r.member_id === memberId);
    host.querySelector('#calWho').textContent = who
      ? `${who.member_name} · ${from} to ${to}`
      : 'Select a member of staff above';

    calBody.innerHTML = '<div class="pane-empty">Loading…</div>';
    let days;
    try {
      days = await api.memberCalendar(memberId, from, to);
    } catch (e) {
      calBody.innerHTML = `<div class="note y">${icon('warn')}<div>${esc(e.message)}</div></div>`;
      return;
    }

    calBody.innerHTML = `<div class="cal">${days.map((d) => {
      // Nepali date as the label, with the English one under it.
      const label = bsPretty(d.date) || d.date;
      const blocks = d.plan.timetables.map((t) => ({
        colour: t.colour,
        timetable_name: t.name,
        // The calendar draws clock time, but the resolver hands back minutes
        // that may run past midnight on a night shift.
        on_duty: `${String(Math.floor(t.on_min / 60) % 24).padStart(2, '0')}:${
          String(t.on_min % 60).padStart(2, '0')}`,
        off_duty: `${String(Math.floor(t.off_min / 60) % 24).padStart(2, '0')}:${
          String(t.off_min % 60).padStart(2, '0')}`,
      }));
      return `<div class="cal-row${d.is_weekend ? ' off' : ''}${d.holiday ? ' hol' : ''}">
          <div class="cal-d">
            <b>${esc(label)} · ${esc(SHORT[d.weekday])}</b>
            <span>${esc(adPretty(d.date))}</span>
            ${d.holiday ? `<i class="tag b">${esc(d.holiday)}</i>` : ''}
          </div>
          ${dayBar(blocks)}
        </div>`;
    }).join('')}</div>`;
  }

  async function arrange(isTemporary) {
    if (!selected.size) {
      toast('err', 'Select at least one member of staff first.');
      return;
    }
    if (!shifts.length) {
      toast('err', 'There are no shifts to assign. Create one first.');
      return;
    }
    const ok = await modal({
      title: isTemporary ? 'Temporary shift' : 'Arrange shifts',
      subtitle: `${selected.size} member${selected.size === 1 ? '' : 's'} of staff`,
      body: `
        <div class="fld"><label>Shift</label>
          <select class="inp" name="shift">
            ${shifts.map((s) => `<option value="${s.id}">${esc(s.name)}</option>`).join('')}
          </select></div>
        <div class="grid2">
          ${dateField('start', 'From', todayIso())}
          ${dateField('end', `To${isTemporary ? '' : ' (optional)'}`,
            isTemporary ? addDays(todayIso(), 6) : '')}
        </div>
`,
      onMount: (ov) => wireDateFields(ov),
      buttons: [
        { label: 'Cancel', value: null },
        {
          label: isTemporary ? 'Apply temporarily' : 'Assign', kind: 'pri',
          onClick: async (ov) => {
            const g = (n) => ov.querySelector(`[name=${n}]`);
            const end = g('end').value;
            if (isTemporary && !end) throw new Error('A temporary shift needs an end date.');
            return api.arrangeShifts(
              [...selected], Number(g('shift').value), g('start').value, end, isTemporary);
          },
        },
      ],
    });
    if (!ok) return;
    await loadRoster();
    if (focusMember) await paintCalendar(focusMember);
    toast('ok', `${ok} member${ok === 1 ? '' : 's'} of staff updated`);
  }

  tree.addEventListener('click', async (e) => {
    const b = e.target.closest('[data-dept]');
    if (!b) return;
    deptId = b.dataset.dept ? Number(b.dataset.dept) : null;
    paintTree();
    await loadRoster();
  });

  tbl.addEventListener('click', async (e) => {
    if (e.target.id === 'rsAll') {
      const on = e.target.checked;
      tbl.querySelectorAll('[data-pick]').forEach((c) => {
        c.checked = on;
        if (on) selected.add(Number(c.dataset.pick));
        else selected.delete(Number(c.dataset.pick));
      });
      host.querySelector('#rsCount').textContent =
        `${new Set(rows.map((r) => r.member_id)).size} staff · ${selected.size} selected`;
      return;
    }
    const pick = e.target.closest('[data-pick]');
    if (pick) {
      const id = Number(pick.dataset.pick);
      if (pick.checked) selected.add(id);
      else selected.delete(id);
      host.querySelector('#rsCount').textContent =
        `${new Set(rows.map((r) => r.member_id)).size} staff · ${selected.size} selected`;
      return;
    }
    const cal = e.target.closest('[data-cal]');
    if (cal) await paintCalendar(Number(cal.dataset.cal));
  });

  host.querySelector('#rsArrange').addEventListener('click', () => arrange(false));
  host.querySelector('#rsTemp').addEventListener('click', () => arrange(true));
  host.querySelector('#calGo').addEventListener('click', async () => {
    from = host.querySelector('#calFrom').value;
    to = host.querySelector('#calTo').value;
    if (focusMember) await paintCalendar(focusMember);
    else toast('inf', 'Pick a member of staff with the Calendar button first.');
  });

  wireDateFields(host);
  paintTree();
  await loadRoster();
}

// ===========================================================================

export default {
  async mount(host) {
    host.innerHTML = `
      <div class="subtabs" id="ttTabs">
        ${TABS.map((t) => `
          <button data-tab="${t.id}">${icon(t.icon)}<span>${esc(t.label)}</span></button>`).join('')}
      </div>
      <div id="ttBody"></div>`;

    const body = host.querySelector('#ttBody');

    async function show(id) {
      host.querySelectorAll('#ttTabs button').forEach((b) =>
        b.classList.toggle('on', b.dataset.tab === id));
      body.innerHTML = '<div class="pane-empty">Loading…</div>';
      try {
        if (id === 'blocks') await blocksTab(body);
        else if (id === 'shifts') await shiftsTab(body);
        else await rosterTab(body);
      } catch (e) {
        body.innerHTML = `<div class="card"><div class="card-b">
          <div class="note y">${icon('warn')}<div>${esc(e.message || e)}</div></div>
        </div></div>`;
      }
    }

    host.querySelector('#ttTabs').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-tab]');
      if (b) show(b.dataset.tab);
    });

    await show('blocks');
  },
};
