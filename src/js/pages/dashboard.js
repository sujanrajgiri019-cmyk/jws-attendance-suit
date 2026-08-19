import { api } from '../api.js';
import {
  icon, esc, table, statusTag, person, hhmm, duration, todayIso,
  barChart, donut, toast, withBusy, loadingTable, emptyState,
} from '../ui.js';

const KPI = (label, value, iconName, colour, bg, detail) => `
  <div class="kpi" style="--c:${colour};--cb:${bg}">
    <div class="kh"><span class="kl">${esc(label)}</span>
      <span class="ki">${icon(iconName)}</span></div>
    <div class="kv">${value}</div>
    <div class="kd">${detail}</div>
  </div>`;

export default {
  async mount(host) {
    host.innerHTML = `
      <div class="kpis" id="kpis"></div>
      <div class="g-2-1">
        <div class="card">
          <div class="card-h">
            <div class="ht"><h3>Attendance Trend</h3>
              <p>Present, late and absent across recent working days</p></div>
            <div class="seg" id="trendSeg">
              <button class="on" data-days="14">14 days</button>
              <button data-days="30">30 days</button>
            </div>
          </div>
          <div class="card-b">
            <div id="trend"><div class="skel" style="height:220px"></div></div>
            <div class="legend" style="margin-top:12px">
              <div class="li"><i style="background:#F16522"></i>Present</div>
              <div class="li"><i style="background:#F5C6A8"></i>Late</div>
              <div class="li"><i style="background:#E7E9EC"></i>Absent</div>
            </div>
          </div>
        </div>
        <div class="card">
          <div class="card-h"><div class="ht"><h3>Today at a Glance</h3>
            <p id="glanceDate">—</p></div></div>
          <div class="card-b">
            <div id="donut" style="display:grid;place-items:center;margin-bottom:6px"></div>
            <div id="glance"></div>
          </div>
        </div>
      </div>

      <div class="g-1-2 mt14">
        <div class="card">
          <div class="card-h">
            <div class="ht"><h3>Recent Punches</h3><p id="feedSub">From the terminal</p></div>
          </div>
          <div class="feed" id="feed"></div>
        </div>
        <div class="card">
          <div class="card-h">
            <div class="ht"><h3>Department Performance</h3><p>Marked in today</p></div>
          </div>
          <div class="card-b"><div id="deptBars"></div></div>
        </div>
      </div>

      <div class="card mt14">
        <div class="card-h">
          <div class="ht"><h3>Requires Attention</h3>
            <p>Staff absent or with an incomplete record today</p></div>
          <button class="btn sm pri" id="btnMail">${icon('mail')} Email absentees</button>
        </div>
        <div class="tbl-wrap"><table class="tbl" id="attn"></table></div>
      </div>`;

    loadingTable(host.querySelector('#attn'), 7);

    const today = todayIso();

    const [stats, trend, depts, feed, attendance] = await Promise.all([
      api.dashboard(today),
      api.trend(14),
      api.departmentStats(today),
      api.punchFeed(12),
      api.attendance(today, today),
    ]);

    // --- KPI tiles ---
    const notMarked = stats.absent + stats.not_in;
    host.querySelector('#kpis').innerHTML = [
      KPI('Total Staff', stats.total_staff, 'users', '#2B6CB0', '#EAF2FB',
        `<b>${depts.length}</b>&nbsp;departments`),
      KPI('Present Today', stats.present, 'check', '#1F9D55', '#E8F6EE',
        'on time, full day'),
      KPI('Late Arrivals', stats.late, 'clock', '#C9820B', '#FDF4E1',
        'past the grace period'),
      KPI('Not Marked In', notMarked, 'x', '#D64545', '#FCECEC',
        `${stats.leave} on approved leave`),
      KPI('Attendance Rate', `${stats.rate.toFixed(1)}<small>%</small>`, 'trend', '#F16522', '#FFF4EE',
        `month so far <b>${stats.month_rate.toFixed(1)}%</b>`),
    ].join('');

    // --- glance ---
    host.querySelector('#glanceDate').textContent =
      stats.holiday_name ? `${today} · ${stats.holiday_name}` : `${today}${stats.date_bs ? ` · ${stats.date_bs}` : ''}`;

    host.querySelector('#donut').innerHTML = donut(
      [
        { value: stats.present, colour: '#F16522', label: 'Present' },
        { value: stats.late, colour: '#F5A26B', label: 'Late' },
        { value: stats.not_in, colour: '#D9DDE2', label: 'Not marked in' },
        { value: stats.leave, colour: '#C7B8EC', label: 'On leave' },
        { value: stats.absent, colour: '#E88A8A', label: 'Absent' },
      ],
      `${stats.rate.toFixed(1)}%`,
      'MARKED IN',
    );

    host.querySelector('#glance').innerHTML = [
      ['Present on time', stats.present, 'g'],
      ['Late arrival', stats.late, 'y'],
      ['Not marked in', stats.not_in, 'n'],
      ['On approved leave', stats.leave, 'v'],
      ['Absent', stats.absent, 'r'],
    ].map(([l, v, tone]) =>
      `<div class="stat-row"><span class="sl">${esc(l)}</span>
        <span class="sv"><span class="tag ${tone}">${v}</span></span></div>`,
    ).join('') +
      `<div class="stat-row" style="border-top:1px solid var(--line);margin-top:4px;padding-top:11px">
        <span class="sl" style="font-weight:600">Total staff</span>
        <span class="sv">${stats.total_staff}</span></div>`;

    // --- trend ---
    const drawTrend = (points) => {
      host.querySelector('#trend').innerHTML = points.length
        ? barChart(points, { max: stats.total_staff || undefined })
        : '<div class="empty"><b>No attendance recorded yet</b><p>Records appear here once the terminal starts sending punches.</p></div>';
    };
    drawTrend(trend);

    host.querySelector('#trendSeg').addEventListener('click', async (e) => {
      const b = e.target.closest('button[data-days]');
      if (!b) return;
      host.querySelectorAll('#trendSeg button').forEach((x) => x.classList.remove('on'));
      b.classList.add('on');
      drawTrend(await api.trend(Number(b.dataset.days)));
    });

    // --- feed ---
    const feedEl = host.querySelector('#feed');
    if (!feed.length) {
      emptyState(feedEl, 'No punches yet', 'Scans appear here the moment the terminal reports them.', 'clock');
    } else {
      feedEl.innerHTML = feed.map((p) => `
        <div class="it">
          <div class="av" style="background:${esc(colourOf(p.enroll_no))}">${esc(initialsOf(p.full_name))}</div>
          <div class="ft"><b>${esc(p.full_name)}</b>
            <span>${esc(p.dept_name || 'Unassigned')} · ${p.punch_state === 1 ? 'Check-out' : 'Check-in'}</span></div>
          <div style="text-align:right">
            <div class="tm">${esc(p.punch_time.slice(11, 16))}</div>
            <span style="font-size:10.5px;color:var(--ink-4)">${esc(p.punch_time.slice(0, 10))}</span>
          </div>
        </div>`).join('');
    }

    // --- department bars ---
    host.querySelector('#deptBars').innerHTML = depts.map((d) => `
      <div style="margin-bottom:13px">
        <div style="display:flex;justify-content:space-between;font-size:12.5px;margin-bottom:5px">
          <span style="font-weight:600">${esc(d.name)}</span>
          <span style="color:var(--ink-3)"><b style="color:var(--ink)">${d.present}</b>/${d.total} · ${d.rate.toFixed(0)}%</span>
        </div>
        <div class="bar"><i style="width:${d.rate}%;background:${esc(d.colour)}"></i></div>
      </div>`).join('');

    // --- attention table ---
    const attn = attendance.filter((r) =>
      ['Absent', 'MissingPunch', 'Late', 'HalfDay'].includes(r.status),
    );
    table(
      host.querySelector('#attn'),
      [
        { label: 'Staff', get: (r) => person(r.full_name, `Enrolment ${r.enroll_no}`, r.enroll_no) },
        { label: 'Department', get: (r) => esc(r.dept_name || '—') },
        { label: 'Designation', get: (r) => esc(r.designation || '—') },
        { label: 'Check-in', cls: 'mono', get: (r) => hhmm(r.in_time) },
        { label: 'Check-out', cls: 'mono', get: (r) => hhmm(r.out_time) },
        { label: 'Late', cls: 'num', get: (r) => (r.late_min ? `${r.late_min} min` : '—') },
        { label: 'Status', get: (r) => statusTag(r.status) },
      ],
      attn,
      {
        empty: 'Everyone is accounted for',
        emptyHint: 'No absences or incomplete records today.',
      },
    );

    host.querySelector('#btnMail').addEventListener('click', (e) =>
      withBusy(e.currentTarget, async () => {
        const r = await api.sendAbsenceEmails(today);
        let msg = `${r.sent} notice${r.sent === 1 ? '' : 's'} sent`;
        if (r.skipped_no_email) msg += `, ${r.skipped_no_email} without an email address`;
        if (r.failed) msg += `, ${r.failed} failed`;
        toast(r.failed ? 'err' : 'ok', msg);
        if (r.errors?.length) r.errors.forEach((x) => toast('err', x));
      }).catch(() => {}),
    );
  },
};

// Local copies so the feed avatars match the rest of the app.
const PALETTE = ['#F16522', '#2B6CB0', '#1F9D55', '#6B4EBB', '#C9820B', '#D64545', '#0E7C86', '#B8336A', '#5A6B7B'];
const colourOf = (n) => PALETTE[Math.abs(Number(n) || 0) % PALETTE.length];
const initialsOf = (name) => {
  const p = String(name || '').trim().split(/\s+/);
  return ((p[0]?.[0] || '') + (p[1]?.[0] || '')).toUpperCase() || '?';
};
