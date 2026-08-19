import { api } from '../api.js';
import {
  icon, esc, table, modal, toast, person, confirmDialog, readForm, duration,
} from '../ui.js';

const DAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];

export default {
  async mount(host) {
    let tab = 'shifts';

    host.innerHTML = `
      <div class="tabs" id="tabs">
        <button class="on" data-tab="shifts">Shifts</button>
        <button data-tab="grid">Weekly Timetables</button>
        <button data-tab="schedule">Staff Schedule</button>
        <button data-tab="holidays">Holiday Calendar</button>
      </div>
      <div id="body"></div>`;

    const body = host.querySelector('#body');

    host.querySelector('#tabs').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-tab]');
      if (!b) return;
      host.querySelectorAll('#tabs button').forEach((x) => x.classList.remove('on'));
      b.classList.add('on');
      tab = b.dataset.tab;
      render();
    });

    async function render() {
      body.innerHTML = '<div class="card"><div class="card-b"><div class="skel" style="height:180px"></div></div></div>';
      if (tab === 'shifts') return shiftsTab(body);
      if (tab === 'grid') return gridTab(body);
      if (tab === 'schedule') return scheduleTab(body);
      return holidaysTab(body);
    }

    await render();
  },
};

// ---------------------------------------------------------------------------

async function shiftsTab(body) {
  const shifts = await api.listShifts();

  body.innerHTML = `
    <div class="g3">
      ${shifts.slice(0, 3).map((s) => {
        const dur = span(s.start_time, s.end_time);
        return `<div class="card">
          <div class="card-h">
            <div style="width:36px;height:36px;border-radius:9px;background:var(--brand-50);
              display:grid;place-items:center;color:var(--brand);font-weight:700;font-size:11px;flex:none">
              ${esc(s.code)}</div>
            <div class="ht"><h3>${esc(s.name)}</h3>
              <p>${esc(s.start_time)} – ${esc(s.end_time)} · used by ${s.used_in} day${s.used_in === 1 ? '' : 's'}</p></div>
          </div>
          <div class="card-b" style="padding-top:12px">
            <div style="position:relative;height:34px;background:var(--line-2);border-radius:7px;
              overflow:hidden;margin-bottom:11px">
              <div style="position:absolute;inset:0;display:grid;place-items:center;
                background:var(--brand);opacity:.9;color:#fff;font-size:11.5px;font-weight:650">
                ${esc(s.start_time)} – ${esc(s.end_time)}</div></div>
            <div class="stat-row"><span class="sl">Length</span><span class="sv">${duration(dur)}</span></div>
            <div class="stat-row"><span class="sl">Break</span><span class="sv">${s.break_min} min</span></div>
            <div class="stat-row"><span class="sl">Late grace</span><span class="sv">${s.late_grace} min</span></div>
            <div class="stat-row"><span class="sl">Full day needs</span>
              <span class="sv">${duration(s.min_full_day)}</span></div>
          </div></div>`;
      }).join('')}
    </div>

    <div class="card mt14">
      <div class="card-h"><div class="ht"><h3>Shift Definitions</h3>
        <p>Timings, grace periods and thresholds</p></div></div>
      <div class="tbl-wrap"><table class="tbl" id="tbl"></table></div>
    </div>`;

  table(body.querySelector('#tbl'), [
    { label: 'Code', get: (s) => `<span class="tag o">${esc(s.code)}</span>` },
    { label: 'Shift', get: (s) => `<b>${esc(s.name)}</b>` },
    { label: 'Start', cls: 'mono', get: (s) => esc(s.start_time) },
    { label: 'End', cls: 'mono', get: (s) => esc(s.end_time) },
    { label: 'Length', cls: 'num', get: (s) => duration(span(s.start_time, s.end_time)) },
    { label: 'Break', cls: 'num', get: (s) => `${s.break_min} min` },
    { label: 'Late grace', cls: 'num', get: (s) => `${s.late_grace} min` },
    { label: 'Full day', cls: 'num', get: (s) => duration(s.min_full_day) },
    {
      label: 'Achievable', cls: 'ctr',
      get: (s) => {
        // A full-day threshold above what the shift can produce would mark
        // everyone on it as a half day, so it is surfaced rather than buried.
        const cap = span(s.start_time, s.end_time) - s.break_min;
        return s.min_full_day <= cap
          ? '<span class="tag g">Yes</span>'
          : `<span class="tag r" title="Only ${cap} minutes available">No</span>`;
      },
    },
    { label: 'Overtime', cls: 'ctr', get: (s) => (s.count_ot ? '<span class="tag g">Counted</span>' : '<span class="tag n">No</span>') },
    { label: 'Overnight', cls: 'ctr', get: (s) => (s.overnight ? '<span class="tag b">Yes</span>' : '—') },
  ], shifts, { empty: 'No shifts defined' });
}

// ---------------------------------------------------------------------------

async function gridTab(body) {
  const timetables = await api.listTimetables();

  body.innerHTML = `
    <div class="card">
      <div class="card-h"><div class="ht"><h3>Weekly Timetables</h3>
        <p>Which shift applies on each day of the week</p></div></div>
      <div class="tbl-wrap"><table class="tbl" id="tbl"></table></div>
    </div>
    <div class="note b mt14">${icon('info')}<div>
      A timetable maps each weekday to a shift, or marks it as a day off. Assign one to a
      department or to an individual and the system knows the expected hours for every date.
      <b>Off</b> days are recorded as a weekly off, never as an absence.
    </div></div>`;

  table(body.querySelector('#tbl'), [
    { label: 'Timetable', get: (t) => `<b>${esc(t.name)}</b>` },
    ...DAYS.map((d, i) => ({
      label: d, cls: 'ctr',
      get: (t) => {
        const code = String(t.days || '').split(',')[i];
        return !code || code === '-'
          ? '<span style="color:var(--ink-4)">Off</span>'
          : `<span class="tag o">${esc(code)}</span>`;
      },
    })),
    { label: 'Staff assigned', cls: 'num', get: (t) => t.assigned },
  ], timetables, { empty: 'No timetables defined' });
}

// ---------------------------------------------------------------------------

async function scheduleTab(body) {
  const [members, timetables, depts] = await Promise.all([
    api.listMembers(), api.listTimetables(), api.listDepartments(),
  ]);

  body.innerHTML = `
    <div class="tbar">
      <div class="srch">${icon('search')}
        <input class="inp" id="q" placeholder="Search staff…"></div>
      <select class="inp" style="width:180px" id="fDept">
        <option value="">All departments</option>
        ${depts.map((d) => `<option value="${d.id}">${esc(d.name)}</option>`).join('')}
      </select>
      <div class="sp"></div>
    </div>
    <div class="card">
      <div class="card-h"><div class="ht"><h3>Staff Schedule</h3>
        <p>Change a timetable here and it applies from the next recalculation</p></div></div>
      <div class="tbl-wrap" style="max-height:calc(100vh - 300px)">
        <table class="tbl" id="tbl"></table></div>
    </div>`;

  const draw = () => {
    const q = body.querySelector('#q').value.trim().toLowerCase();
    const dept = Number(body.querySelector('#fDept').value) || null;
    const rows = members.filter(
      (m) => (!q || m.full_name.toLowerCase().includes(q)) && (!dept || m.dept_id === dept),
    );

    table(body.querySelector('#tbl'), [
      { label: 'Staff', get: (m) => person(m.full_name, `Enrolment ${m.enroll_no}`, m.enroll_no) },
      { label: 'Department', get: (m) => esc(m.dept_name || 'Unassigned') },
      {
        label: 'Timetable', width: '260px',
        get: (m) => `<select class="inp" data-mid="${m.id}" style="height:29px;font-size:12px">
          <option value="">Department default</option>
          ${timetables.map((t) => `<option value="${t.id}" ${m.timetable_id === t.id ? 'selected' : ''}>${esc(t.name)}</option>`).join('')}
        </select>`,
      },
      { label: 'Status', get: (m) => `<span class="tag ${m.status === 'Active' ? 'g' : 'n'}">${esc(m.status)}</span>` },
    ], rows, { empty: 'No staff match' });
  };

  body.querySelector('#tbl').addEventListener('change', async (e) => {
    const sel = e.target.closest('[data-mid]');
    if (!sel) return;
    const id = Number(sel.dataset.mid);
    const tt = sel.value ? Number(sel.value) : null;
    try {
      await api.setMemberTimetable(id, tt);
      const m = members.find((x) => x.id === id);
      if (m) m.timetable_id = tt;
      toast('ok', `${m?.full_name || 'Member'} moved to ${sel.options[sel.selectedIndex].text}`);
    } catch (err) {
      toast('err', err.message);
    }
  });

  let t;
  body.querySelector('#q').addEventListener('input', () => {
    clearTimeout(t);
    t = setTimeout(draw, 180);
  });
  body.querySelector('#fDept').addEventListener('change', draw);

  draw();
}

// ---------------------------------------------------------------------------

async function holidaysTab(body) {
  body.innerHTML = `
    <div class="tbar">
      <div class="sp"></div>
      <button class="btn pri" id="btnAdd">${icon('plus')} Add holiday</button>
    </div>
    <div class="card">
      <div class="card-h"><div class="ht"><h3>Holiday Calendar</h3>
        <p>These dates are excluded from attendance calculations</p></div></div>
      <div class="tbl-wrap"><table class="tbl" id="tbl"></table></div>
    </div>`;

  async function load() {
    const rows = await api.listHolidays();
    table(body.querySelector('#tbl'), [
      { label: 'Holiday', get: (h) => `<b>${esc(h.name)}</b>` },
      { label: 'From', cls: 'mono', get: (h) => esc(h.from_date) },
      { label: 'To', cls: 'mono', get: (h) => esc(h.to_date) },
      { label: 'Days', cls: 'num', get: (h) => Math.round(h.days ?? 1) },
      { label: 'Applies to', get: (h) => esc(h.applies_to || 'all') },
      { label: 'Paid', cls: 'ctr', get: (h) => (h.paid ? '<span class="tag g">Paid</span>' : '<span class="tag n">Unpaid</span>') },
      {
        label: 'Actions', cls: 'ctr',
        get: (h) => `<div style="display:flex;gap:5px;justify-content:center">
          <button class="btn sm ic" data-edit="${h.id}">${icon('edit')}</button>
          <button class="btn sm ic dan" data-del="${h.id}">${icon('trash')}</button></div>`,
      },
    ], rows, {
      empty: 'No holidays recorded',
      emptyHint: 'Add Dashain, Tihar and the rest so those days are not counted as absences.',
    });

    body.querySelector('#tbl').onclick = async (e) => {
      const edit = e.target.closest('[data-edit]');
      if (edit) {
        const h = rows.find((x) => x.id === Number(edit.dataset.edit));
        if (await dialog(h)) await load();
        return;
      }
      const del = e.target.closest('[data-del]');
      if (del) {
        const h = rows.find((x) => x.id === Number(del.dataset.del));
        if (!(await confirmDialog('Remove this holiday?',
          `${h.name} will count as a normal working day after the next recalculation.`, 'Remove'))) return;
        await api.deleteHoliday(h.id);
        toast('ok', `${h.name} removed`);
        await load();
      }
    };
  }

  body.querySelector('#btnAdd').addEventListener('click', async () => {
    if (await dialog({})) await load();
  });

  await load();
}

function dialog(h) {
  const isNew = !h.id;
  return modal({
    title: isNew ? 'Add holiday' : `Edit ${h.name}`,
    subtitle: 'Excluded from attendance calculations',
    body: `
      <div class="fld"><label class="req">Holiday name</label>
        <input class="inp" name="name" value="${esc(h.name || '')}" placeholder="Dashain"></div>
      <div class="grid2">
        <div class="fld"><label class="req">From</label>
          <input type="date" class="inp" name="from_date" value="${esc(h.from_date || '')}"></div>
        <div class="fld"><label class="req">To</label>
          <input type="date" class="inp" name="to_date" value="${esc(h.to_date || '')}"></div>
      </div>
      <div class="fld"><label>Applies to</label>
        <select class="inp" name="applies_to">
          <option value="all">All staff</option>
          <option value="teaching" ${h.applies_to === 'teaching' ? 'selected' : ''}>Teaching staff only</option>
          <option value="support" ${h.applies_to === 'support' ? 'selected' : ''}>Support staff only</option>
        </select></div>
      <div class="fld"><label class="cb"><input type="checkbox" name="paid" ${h.paid !== 0 ? 'checked' : ''}>
        <div class="ct"><b>Paid holiday</b></div></label></div>`,
    buttons: [
      { label: 'Cancel', value: null },
      {
        label: isNew ? 'Add' : 'Save', kind: 'pri',
        onClick: async (ov) => {
          const f = readForm(ov);
          if (!f.name?.trim()) throw new Error('Give the holiday a name.');
          if (!f.from_date || !f.to_date) throw new Error('Choose both dates.');
          await api.saveHoliday({
            id: h.id ?? null,
            name: f.name.trim(),
            from_date: f.from_date,
            to_date: f.to_date,
            applies_to: f.applies_to,
            paid: !!f.paid,
          });
          toast('ok', isNew ? 'Holiday added' : 'Holiday saved');
          return true;
        },
      },
    ],
  });
}

function span(start, end) {
  const m = (t) => {
    const [h, mm] = String(t).split(':').map(Number);
    return h * 60 + mm;
  };
  let d = m(end) - m(start);
  if (d <= 0) d += 24 * 60;
  return d;
}
