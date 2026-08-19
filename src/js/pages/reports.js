import { api } from '../api.js';
import {
  icon, esc, table, person, statusTag, hhmm, duration, toast,
  withBusy, todayIso, monthStart, addDays, loadingTable,
} from '../ui.js';

const TYPES = [
  { key: 'general', name: 'General Attendance', hint: 'One row per member with totals for the period' },
  { key: 'daywise', name: 'Day-wise', hint: 'Everyone’s in and out for one chosen date' },
  { key: 'individual', name: 'Individual', hint: 'Day-by-day detail for a single member of staff' },
  { key: 'monthly', name: 'Monthly Grid', hint: 'Calendar grid of the whole period, ready for payroll' },
  { key: 'department', name: 'Department-wise', hint: 'Rolled up by department' },
  { key: 'late', name: 'Late Arrivals', hint: 'Only the exceptions' },
  { key: 'absent', name: 'Absentees', hint: 'Days with no attendance recorded' },
  { key: 'overtime', name: 'Overtime', hint: 'Hours worked beyond the end of duty' },
];

export default {
  async mount(host) {
    const [depts, members, settings] = await Promise.all([
      api.listDepartments(), api.listMembers(), api.getSettings().catch(() => ({})),
    ]);

    let type = 'general';
    const today = todayIso();

    host.innerHTML = `
      <div class="g-1-2">
        <div class="card" style="align-self:start">
          <div class="card-h"><div class="ht"><h3>Report Builder</h3>
            <p>Choose what to produce</p></div></div>
          <div class="card-b">
            <div class="fld"><label>Report type</label>
              <div id="types">
                ${TYPES.map((t) => `<label class="cb" style="margin-bottom:9px">
                  <input type="radio" name="rtype" value="${t.key}" ${t.key === type ? 'checked' : ''}>
                  <div class="ct"><b>${esc(t.name)}</b><span>${esc(t.hint)}</span></div></label>`).join('')}
              </div>
            </div>
            <div class="divider"></div>
            <div class="fld"><label>Quick period</label>
              <select class="inp" id="period">
                <option value="month">This month so far</option>
                <option value="lastmonth">Last month</option>
                <option value="week">Last 7 days</option>
                <option value="today">Today</option>
                <option value="custom">Custom range</option>
              </select></div>
            <div class="grid2">
              <div class="fld"><label>From</label>
                <input type="date" class="inp" id="from" value="${monthStart(today)}"></div>
              <div class="fld"><label>To</label>
                <input type="date" class="inp" id="to" value="${today}"></div>
            </div>
            <div class="fld"><label>Department</label>
              <select class="inp" id="dept">
                <option value="">All departments</option>
                ${depts.map((d) => `<option value="${d.id}">${esc(d.name)}</option>`).join('')}
              </select></div>
            <div class="fld" id="memWrap" style="display:none"><label>Member</label>
              <select class="inp" id="member">
                ${members.map((m) => `<option value="${m.id}">${esc(m.full_name)} (${m.enroll_no})</option>`).join('')}
              </select></div>
            <div class="fld"><label class="cb"><input type="checkbox" id="withBs">
              <div class="ct"><b>Show Nepali (BS) dates</b></div></label></div>
            <button class="btn pri" style="width:100%" id="btnRun">${icon('chart')} Generate</button>
          </div>
        </div>

        <div>
          <div class="tbar">
            <div class="sp"></div>
            <button class="btn" id="btnPrint">${icon('print')} Print</button>
            <button class="btn" id="btnCsv">${icon('file')} Export CSV</button>
          </div>
          <div class="card" id="sheet">
            <div class="card-h">
              <img src="assets/crest.png" style="height:34px" alt="">
              <div class="ht"><h3 id="repTitle">General Attendance Report</h3>
                <p>${esc(settings.school_name || 'Janapremi World School')} · ${esc(settings.school_address || '')}</p></div>
              <div style="text-align:right;font-size:11px;color:var(--ink-3)">
                <div id="repRange">—</div><div id="repGen">—</div></div>
            </div>
            <div class="card-b" id="summary" style="border-bottom:1px solid var(--line)"></div>
            <div class="tbl-wrap" style="max-height:calc(100vh - 380px)">
              <table class="tbl" id="tbl"></table></div>
            <div class="card-f" style="display:flex;justify-content:space-between;font-size:11px;color:var(--ink-3)">
              <span>JWS Attendance</span><span id="repCount"></span>
            </div>
          </div>
        </div>
      </div>`;

    const $ = (s) => host.querySelector(s);

    $('#types').addEventListener('change', (e) => {
      if (e.target.name !== 'rtype') return;
      type = e.target.value;
      $('#memWrap').style.display = type === 'individual' ? 'block' : 'none';
      run();
    });

    $('#period').addEventListener('change', () => {
      const v = $('#period').value;
      if (v === 'month') { $('#from').value = monthStart(today); $('#to').value = today; }
      else if (v === 'lastmonth') {
        const d = new Date(`${today}T00:00:00`);
        d.setDate(1); d.setMonth(d.getMonth() - 1);
        const start = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-01`;
        const end = new Date(d.getFullYear(), d.getMonth() + 1, 0);
        $('#from').value = start;
        $('#to').value = `${end.getFullYear()}-${String(end.getMonth() + 1).padStart(2, '0')}-${String(end.getDate()).padStart(2, '0')}`;
      } else if (v === 'week') { $('#from').value = addDays(today, -6); $('#to').value = today; }
      else if (v === 'today') { $('#from').value = today; $('#to').value = today; }
      if (v !== 'custom') run();
    });

    $('#dept').addEventListener('change', run);
    $('#member').addEventListener('change', run);
    $('#btnRun').addEventListener('click', (e) => withBusy(e.currentTarget, run).catch(() => {}));

    $('#btnPrint').addEventListener('click', () => window.print());

    $('#btnCsv').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const csv = await api.exportCsv(
          $('#from').value, $('#to').value,
          Number($('#dept').value) || null, $('#withBs').checked,
        );
        const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
        const a = document.createElement('a');
        a.href = URL.createObjectURL(blob);
        a.download = `jws-attendance-${$('#from').value}-to-${$('#to').value}.csv`;
        a.click();
        setTimeout(() => URL.revokeObjectURL(a.href), 2000);
        toast('ok', 'CSV downloaded');
      }).catch(() => {}),
    );

    async function run() {
      const from = $('#from').value;
      const to = $('#to').value;
      const deptId = Number($('#dept').value) || null;
      const memberId = Number($('#member').value) || null;
      const withBs = $('#withBs').checked;

      if (!from || !to) return toast('err', 'Choose both a start and an end date.');
      if (to < from) return toast('err', 'The end date is before the start date.');

      const meta = TYPES.find((t) => t.key === type);
      $('#repTitle').textContent = `${meta.name} Report`;
      $('#repRange').textContent = from === to ? from : `${from} to ${to}`;
      $('#repGen').textContent = `Generated ${today}`;
      loadingTable($('#tbl'), 8);
      $('#summary').innerHTML = '';

      try {
        if (type === 'general') return await general(from, to, deptId);
        if (type === 'department') return await byDepartment(from, to);
        if (type === 'individual') return await individual(from, to, memberId, withBs);
        if (type === 'monthly') return await monthly(from, to, deptId);
        return await detail(from, to, deptId, withBs, type);
      } catch (e) {
        $('#tbl').innerHTML = `<tbody><tr><td><div class="empty">
          <div class="ei">${icon('warn')}</div><b>Could not build this report</b>
          <p>${esc(e.message)}</p></div></td></tr></tbody>`;
      }
    }

    const stat = (label, value) => `
      <div style="padding:11px 13px;background:#FAFBFC;border:1px solid var(--line);
        border-radius:9px;margin-bottom:10px">
        <div style="font-size:10.5px;color:var(--ink-3);text-transform:uppercase;
          letter-spacing:.05em;font-weight:650">${esc(label)}</div>
        <div style="font-size:18px;font-weight:700;letter-spacing:-.02em;margin-top:3px">${value}</div></div>`;

    const setCount = (n, noun = 'row') =>
      ($('#repCount').textContent = `${n} ${noun}${n === 1 ? '' : 's'}`);

    // --- general ---
    async function general(from, to, deptId) {
      const rows = await api.reportSummary(from, to, deptId);
      const tot = rows.reduce((a, r) => ({
        present: a.present + r.present, late: a.late + r.late,
        absent: a.absent + r.absent, ot: a.ot + r.ot_min,
      }), { present: 0, late: 0, absent: 0, ot: 0 });
      const avg = rows.length ? rows.reduce((a, r) => a + r.rate, 0) / rows.length : 0;

      $('#summary').innerHTML = `<div class="grid4">
        ${stat('Staff', rows.length)}${stat('Working days', rows[0]?.working_days ?? 0)}
        ${stat('Average attendance', `${avg.toFixed(1)}%`)}${stat('Total late', tot.late)}
        ${stat('Total absent', tot.absent)}${stat('Total overtime', duration(tot.ot))}
        ${stat('Below 85%', rows.filter((r) => r.rate < 85).length)}
        ${stat('Perfect attendance', rows.filter((r) => r.rate >= 100).length)}</div>`;

      table($('#tbl'), [
        { label: '#', cls: 'num mono', get: (_, i) => i + 1 },
        { label: 'Staff', get: (r) => person(r.full_name, `Enrolment ${r.enroll_no}`, r.enroll_no) },
        { label: 'Department', get: (r) => esc(r.dept_name || '—') },
        { label: 'Designation', get: (r) => esc(r.designation || '—') },
        { label: 'Days', cls: 'num', get: (r) => r.working_days },
        { label: 'Present', cls: 'num', get: (r) => `<b>${r.present}</b>` },
        { label: 'Late', cls: 'num', get: (r) => r.late || '—' },
        { label: 'Half', cls: 'num', get: (r) => r.half_day || '—' },
        { label: 'Absent', cls: 'num', get: (r) => r.absent || '—' },
        { label: 'Leave', cls: 'num', get: (r) => r.leave || '—' },
        { label: 'Hours', cls: 'num mono', get: (r) => duration(r.worked_min) },
        { label: 'OT', cls: 'num mono', get: (r) => duration(r.ot_min) },
        {
          label: 'Rate', width: '130px',
          get: (r) => `<div style="display:flex;align-items:center;gap:8px">
            <div class="bar" style="flex:1"><i style="width:${Math.min(100, r.rate)}%;
              background:${r.rate >= 90 ? '#1F9D55' : r.rate >= 80 ? '#C9820B' : '#D64545'}"></i></div>
            <b style="font-size:12px;width:42px;text-align:right">${r.rate.toFixed(1)}%</b></div>`,
        },
      ], rows, { empty: 'No staff in this selection' });
      setCount(rows.length, 'member');
    }

    // --- department ---
    async function byDepartment(from, to) {
      const all = await api.reportSummary(from, to, null);
      const grouped = depts.map((d) => {
        const rows = all.filter((r) => r.dept_name === d.name);
        const sum = rows.reduce((a, r) => ({
          present: a.present + r.present, late: a.late + r.late,
          half: a.half + r.half_day, absent: a.absent + r.absent,
          leave: a.leave + r.leave, days: a.days + r.working_days,
        }), { present: 0, late: 0, half: 0, absent: 0, leave: 0, days: 0 });
        const rate = sum.days ? ((sum.present + sum.late + sum.half * 0.5) / sum.days) * 100 : 0;
        return { ...d, staff: rows.length, ...sum, rate };
      });

      const best = [...grouped].filter((g) => g.staff).sort((a, b) => b.rate - a.rate)[0];
      $('#summary').innerHTML = `<div class="grid4">
        ${stat('Departments', grouped.filter((g) => g.staff).length)}
        ${stat('Total staff', all.length)}
        ${stat('Average', `${(grouped.reduce((a, g) => a + g.rate, 0) / (grouped.length || 1)).toFixed(1)}%`)}
        ${stat('Best', esc(best?.name || '—'))}</div>`;

      table($('#tbl'), [
        { label: 'Code', get: (g) => `<span class="tag n" style="background:${esc(g.colour)}18;color:${esc(g.colour)}">${esc(g.code)}</span>` },
        { label: 'Department', get: (g) => `<b>${esc(g.name)}</b>` },
        { label: 'Head', get: (g) => esc(g.head_name || '—') },
        { label: 'Staff', cls: 'num', get: (g) => g.staff },
        { label: 'Member-days', cls: 'num', get: (g) => g.days },
        { label: 'Present', cls: 'num', get: (g) => g.present },
        { label: 'Late', cls: 'num', get: (g) => g.late },
        { label: 'Absent', cls: 'num', get: (g) => g.absent },
        { label: 'Leave', cls: 'num', get: (g) => g.leave },
        {
          label: 'Rate', width: '150px',
          get: (g) => `<div style="display:flex;align-items:center;gap:9px">
            <div class="bar" style="flex:1"><i style="width:${Math.min(100, g.rate)}%;background:${esc(g.colour)}"></i></div>
            <b style="font-size:12px;width:42px;text-align:right">${g.rate.toFixed(1)}%</b></div>`,
        },
      ], grouped, { empty: 'No departments' });
      setCount(grouped.length, 'department');
    }

    // --- individual ---
    async function individual(from, to, memberId, withBs) {
      if (!memberId) throw new Error('Choose a member of staff first.');
      const m = members.find((x) => x.id === memberId);
      const rows = await api.attendance(from, to, null, memberId, withBs);
      const worked = rows.filter((r) => ['Present', 'Late', 'HalfDay'].includes(r.status));
      const workDays = rows.filter((r) => !['Holiday', 'WeeklyOff'].includes(r.status)).length;
      const rate = workDays ? (worked.length / workDays) * 100 : 0;

      $('#summary').innerHTML = `
        <div style="display:flex;align-items:center;gap:14px;margin-bottom:14px">
          <div style="width:44px;height:44px;border-radius:50%;background:var(--brand);color:#fff;
            display:grid;place-items:center;font-weight:700">${esc((m.full_name[0] || '') + (m.full_name.split(' ')[1]?.[0] || ''))}</div>
          <div style="flex:1"><b style="font-size:15px">${esc(m.full_name)}</b>
            <div style="font-size:12px;color:var(--ink-3)">Enrolment ${m.enroll_no} ·
              ${esc(m.designation || '')} · ${esc(m.dept_name || '')}</div></div>
        </div>
        <div class="grid4">
          ${stat('Working days', workDays)}${stat('Present', rows.filter((r) => r.status === 'Present').length)}
          ${stat('Late', rows.filter((r) => r.status === 'Late').length)}
          ${stat('Absent', rows.filter((r) => r.status === 'Absent').length)}
          ${stat('Total hours', duration(rows.reduce((a, r) => a + r.worked_min, 0)))}
          ${stat('Overtime', duration(rows.reduce((a, r) => a + r.ot_min, 0)))}
          ${stat('Late minutes', rows.reduce((a, r) => a + r.late_min, 0))}
          ${stat('Attendance', `${rate.toFixed(1)}%`)}</div>`;

      table($('#tbl'), [
        { label: 'Date', cls: 'mono', get: (r) => esc(r.work_date) },
        ...(withBs ? [{ label: 'Nepali date', get: (r) => esc(r.work_date_bs || '—') }] : []),
        { label: 'Day', get: (r) => new Date(`${r.work_date}T00:00:00`).toLocaleDateString('en-GB', { weekday: 'short' }) },
        { label: 'In', cls: 'mono', get: (r) => hhmm(r.in_time) },
        { label: 'Out', cls: 'mono', get: (r) => hhmm(r.out_time) },
        { label: 'Worked', cls: 'num mono', get: (r) => duration(r.worked_min) },
        { label: 'Late', cls: 'num', get: (r) => r.late_min || '—' },
        { label: 'OT', cls: 'num mono', get: (r) => (r.ot_min ? duration(r.ot_min) : '—') },
        { label: 'Status', get: (r) => statusTag(r.status) },
        { label: 'Remark', get: (r) => `<span style="color:var(--ink-3)">${esc(r.remark || '')}</span>` },
      ], rows, { empty: 'No records in this period' });
      setCount(rows.length, 'day');
    }

    // --- monthly grid ---
    async function monthly(from, to, deptId) {
      const rows = await api.attendance(from, to, deptId, null, false);
      const dates = [...new Set(rows.map((r) => r.work_date))].sort();
      const byMember = new Map();
      for (const r of rows) {
        if (!byMember.has(r.member_id)) {
          byMember.set(r.member_id, { name: r.full_name, enroll: r.enroll_no, days: {} });
        }
        byMember.get(r.member_id).days[r.work_date] = r.status;
      }
      const people = [...byMember.values()];

      $('#summary').innerHTML = `<div class="grid4">
          ${stat('Period', `${from} → ${to}`)}${stat('Days shown', dates.length)}
          ${stat('Staff', people.length)}${stat('Records', rows.length)}</div>
        <div class="legend" style="margin-top:12px">
          <div class="li"><i style="background:#1F9D55"></i>Present (P)</div>
          <div class="li"><i style="background:#C9820B"></i>Late (L)</div>
          <div class="li"><i style="background:#D64545"></i>Absent (A)</div>
          <div class="li"><i style="background:#6B4EBB"></i>Leave (V)</div>
          <div class="li"><i style="background:#E7E9EC"></i>Holiday / weekly off (H)</div></div>`;

      const MARK = {
        Present: ['P', '#1F9D55'], Late: ['L', '#C9820B'], HalfDay: ['½', '#C9820B'],
        Absent: ['A', '#D64545'], Leave: ['V', '#6B4EBB'],
        Holiday: ['H', '#868D96'], WeeklyOff: ['H', '#868D96'], MissingPunch: ['?', '#868D96'],
      };

      $('#tbl').innerHTML =
        `<thead><tr><th>Staff</th>
          ${dates.map((d) => `<th class="ctr" style="padding:9px 4px">${d.slice(8)}</th>`).join('')}
          <th class="num">P</th><th class="num">L</th><th class="num">A</th><th class="num">%</th></tr></thead>
        <tbody>${people.map((p) => {
          let P = 0, L = 0, A = 0, days = 0;
          const cells = dates.map((d) => {
            const st = p.days[d];
            if (!st) return '<td class="ctr">—</td>';
            const [mark, colour] = MARK[st] || ['?', '#868D96'];
            if (!['Holiday', 'WeeklyOff'].includes(st)) days++;
            if (st === 'Present') P++;
            else if (st === 'Late' || st === 'HalfDay') { L++; P++; }
            else if (st === 'Absent' || st === 'MissingPunch') A++;
            return `<td class="ctr" style="padding:7px 4px">
              <span class="mcell" style="background:${colour}1a;color:${colour}" title="${esc(d)} — ${esc(st)}">${mark}</span></td>`;
          }).join('');
          const pc = days ? ((P / days) * 100).toFixed(0) : '0';
          return `<tr><td style="white-space:nowrap"><b>${esc(p.name)}</b>
            <div style="font-size:10.5px;color:var(--ink-3)">Enrolment ${p.enroll}</div></td>
            ${cells}<td class="num"><b>${P}</b></td><td class="num">${L}</td><td class="num">${A}</td>
            <td class="num"><span class="tag ${pc >= 90 ? 'g' : pc >= 80 ? 'y' : 'r'}">${pc}%</span></td></tr>`;
        }).join('')}</tbody>`;

      if (!people.length) {
        $('#tbl').innerHTML = `<tbody><tr><td><div class="empty">
          <div class="ei">${icon('chart')}</div><b>No records in this period</b>
          <p>Fetch punch records, or recalculate attendance, and try again.</p></div></td></tr></tbody>`;
      }
      setCount(people.length, 'member');
    }

    // --- day-wise / late / absent / overtime ---
    async function detail(from, to, deptId, withBs, kind) {
      let rows = await api.attendance(from, to, deptId, null, withBs);
      if (kind === 'late') rows = rows.filter((r) => r.late_min > 0 || r.early_min > 0);
      if (kind === 'absent') rows = rows.filter((r) => ['Absent', 'MissingPunch', 'Leave'].includes(r.status));
      if (kind === 'overtime') rows = rows.filter((r) => r.ot_min > 0);

      const totals = {
        late: rows.reduce((a, r) => a + r.late_min, 0),
        ot: rows.reduce((a, r) => a + r.ot_min, 0),
        staff: new Set(rows.map((r) => r.member_id)).size,
      };
      $('#summary').innerHTML = `<div class="grid4">
        ${stat('Records', rows.length)}${stat('Staff affected', totals.staff)}
        ${stat('Total late', `${totals.late} min`)}${stat('Total overtime', duration(totals.ot))}</div>`;

      table($('#tbl'), [
        { label: 'Date', cls: 'mono', get: (r) => esc(r.work_date) },
        ...(withBs ? [{ label: 'Nepali date', get: (r) => esc(r.work_date_bs || '—') }] : []),
        { label: 'Staff', get: (r) => person(r.full_name, `Enrolment ${r.enroll_no}`, r.enroll_no) },
        { label: 'Department', get: (r) => esc(r.dept_name || '—') },
        { label: 'In', cls: 'mono', get: (r) => hhmm(r.in_time) },
        { label: 'Out', cls: 'mono', get: (r) => hhmm(r.out_time) },
        { label: 'Worked', cls: 'num mono', get: (r) => duration(r.worked_min) },
        { label: 'Late', cls: 'num', get: (r) => r.late_min || '—' },
        { label: 'Early', cls: 'num', get: (r) => r.early_min || '—' },
        { label: 'OT', cls: 'num mono', get: (r) => (r.ot_min ? duration(r.ot_min) : '—') },
        { label: 'Status', get: (r) => statusTag(r.status) },
      ], rows, {
        empty: kind === 'late' ? 'No late arrivals in this period'
          : kind === 'absent' ? 'No absences in this period'
            : kind === 'overtime' ? 'No overtime in this period'
              : 'No records in this period',
        emptyHint: 'That is a good result, but check the dates if it looks wrong.',
      });
      setCount(rows.length);
    }

    await run();
  },
};
