// In-memory stand-in for the Rust backend.
//
// Used when the interface runs in a plain browser (`npm run dev`) instead of
// inside the app window. It lets the interface be developed and tested without
// building the Rust half, and it is what the Playwright suite drives.
//
// It is a fake, not a second implementation: it answers the same command names
// with the same shapes, and nothing else in the app knows the difference.

const DEPTS = [
  { id: 1, name: 'Administration', code: 'ADM', colour: '#F16522' },
  { id: 2, name: 'Primary Level', code: 'PRI', colour: '#2B6CB0' },
  { id: 3, name: 'Lower Secondary', code: 'LSE', colour: '#1F9D55' },
  { id: 4, name: 'Secondary Level', code: 'SEC', colour: '#6B4EBB' },
  { id: 5, name: 'Accounts', code: 'ACC', colour: '#C9820B' },
  { id: 6, name: 'Library & IT', code: 'LIT', colour: '#0E7C86' },
  { id: 7, name: 'Sports & Activities', code: 'SPT', colour: '#B8336A' },
  { id: 8, name: 'Support Staff', code: 'SUP', colour: '#5A6B7B' },
];

const FIRST = [
  'Sarita', 'Bikash', 'Ramesh', 'Nabin', 'Dipak', 'Anita', 'Kamala', 'Sunita',
  'Prakash', 'Rita', 'Manisha', 'Gopal', 'Sabina', 'Krishna', 'Rekha', 'Milan',
  'Bimala', 'Suresh', 'Nirmala', 'Hari', 'Anjana', 'Rajesh', 'Laxmi', 'Bishal',
];
const LAST = [
  'Rajgiri', 'Shrestha', 'Maharjan', 'Karki', 'Tamang', 'Adhikari', 'Lama',
  'Basnet', 'Thapa', 'Gurung', 'Poudel', 'Bhattarai', 'Nepali', 'Dangol',
  'Prajapati', 'Magar', 'Rai', 'Sharma', 'Joshi', 'KC',
];
const DESIG = {
  1: ['Principal', 'Vice Principal', 'Office Assistant', 'Receptionist', 'HR Officer'],
  2: ['Class Teacher', 'Assistant Teacher', 'Sr. Teacher'],
  3: ['Subject Teacher', 'Sr. Teacher', 'Lab Assistant'],
  4: ['Subject Teacher', 'Sr. Teacher', 'Coordinator'],
  5: ['Accountant', 'Account Assistant', 'Cashier'],
  6: ['Librarian', 'IT Officer', 'Lab Technician'],
  7: ['Sports Teacher', 'Activity Coordinator'],
  8: ['Peon', 'Guard', 'Cleaner', 'Driver', 'Gardener'],
};

/** Deterministic pseudo-random, so the demo looks the same on every reload. */
function rng(seed) {
  let s = seed;
  return () => {
    s = (s * 1103515245 + 12345) & 0x7fffffff;
    return s / 0x7fffffff;
  };
}

const iso = (d) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;


import { BS_TABLE } from './bs-table.generated.js';

const REPORT_KINDS = [
  ['daily_stat', 'Daily Attendance Statistic Report'],
  ['general', 'Attendance General Report'],
  ['dept_stat', 'Depart Attendance Statistic Report'],
  ['duty_timetable', "Staff's On-Duty/Off-Duty Timetable"],
  ['daily_shifts', 'Daily Attendance Shifts'],
  ['daily_ot', 'Daily Attendance OT Report'],
  ['ot_summary', 'Summary of Overtime'],
  ['daily_overtime', 'Daily Overtime'],
];

const todayIso = () => iso(new Date());
const addDaysIso = (d, n) => {
  const x = new Date(`${d}T00:00:00`);
  x.setDate(x.getDate() + n);
  return iso(x);
};

export function createDemo() {
  const rand = rng(20260819);
  const pick = (a) => a[Math.floor(rand() * a.length)];
  const ri = (a, b) => Math.floor(rand() * (b - a + 1)) + a;

  // --- members ---
  const members = [];
  const dist = [5, 8, 7, 8, 3, 3, 3, 7];
  let enroll = 1;
  DEPTS.forEach((d, di) => {
    for (let k = 0; k < dist[di]; k++) {
      const name = `${pick(FIRST)} ${pick(LAST)}`;
      const desig = k < DESIG[d.id].length ? DESIG[d.id][k] : pick(DESIG[d.id]);
      members.push({
        id: enroll,
        enroll_no: enroll,
        staff_id: `JWS-${String(enroll).padStart(3, '0')}`,
        full_name: name,
        device_name: name.slice(0, 24),
        dept_id: d.id,
        dept_name: d.name,
        dept_code: d.code,
        dept_colour: d.colour,
        designation: desig,
        gender: rand() > 0.5 ? 'Female' : 'Male',
        dob: null,
        mobile: `98${ri(10000000, 99999999)}`,
        email: `${name.toLowerCase().replace(/[^a-z]/g, '.')}@jws.edu.np`,
        card_no: String(ri(1000000, 9999999)).padStart(10, '0'),
        privilege: k === 0 && d.id === 1 ? 14 : k === 0 ? 6 : 0,
        fp_count: ri(1, 3),
        status: rand() > 0.95 ? 'On Leave' : 'Active',
        joined_on: `20${ri(18, 25)}-0${ri(1, 9)}-1${ri(0, 9)}`,
        timetable_id: d.id === 8 ? 3 : d.id === 1 || d.id === 5 || d.id === 6 ? 2 : 1,
      });
      enroll++;
    }
  });

  const today = iso(new Date());

  // --- attendance for the last 40 days ---
  const attendance = new Map(); // `${memberId}|${date}` -> row
  const punches = [];
  const days = [];
  for (let i = 39; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    days.push(d);
  }

  for (const d of days) {
    const date = iso(d);
    const saturday = d.getDay() === 6;
    for (const m of members) {
      let status, inT = null, outT = null, late = 0, ot = 0, worked = 0;
      if (saturday && m.timetable_id !== 3) {
        status = 'WeeklyOff';
      } else if (m.status === 'On Leave') {
        status = 'Leave';
      } else {
        const r = rand();
        if (r > 0.93) status = 'Absent';
        else if (r > 0.80) {
          status = 'Late';
          late = ri(11, 55);
          inT = `09:${String(late).padStart(2, '0')}:00`;
        } else {
          status = 'Present';
          inT = `08:${ri(38, 59)}:00`;
        }
        if (inT) {
          const outH = rand() > 0.85 ? 17 : 16;
          outT = `${outH}:${String(ri(2, 45)).padStart(2, '0')}:00`;
          worked = 390 + ri(-25, 45);
          if (outH === 17) ot = ri(35, 90);
          punches.push({
            enroll_no: m.enroll_no, full_name: m.full_name, dept_name: m.dept_name,
            punch_time: `${date} ${inT}`, punch_state: 0,
          });
          punches.push({
            enroll_no: m.enroll_no, full_name: m.full_name, dept_name: m.dept_name,
            punch_time: `${date} ${outT}`, punch_state: 1,
          });
        }
      }
      attendance.set(`${m.id}|${date}`, {
        member_id: m.id, enroll_no: m.enroll_no, full_name: m.full_name,
        dept_name: m.dept_name, dept_colour: m.dept_colour, designation: m.designation,
        work_date: date, work_date_bs: null, in_time: inT, out_time: outT,
        worked_min: worked, late_min: late, early_min: 0, ot_min: ot,
        status, remark: null, manual: false, locked: false,
      });
    }
  }
  punches.sort((a, b) => b.punch_time.localeCompare(a.punch_time));

  const devices = [
    {
      id: 1, name: 'Main Gate', machine_no: 101, model: 'ZKTeco K40 Pro',
      serial: 'GED7253800740', mac: '00:17:61:10:c0:77', ip: '192.168.100.99',
      port: 4370, comm_key: 0, mode: 'push', location: 'Main Building Entrance',
      auto_connect: true, last_seen: `${today} 08:05`,
    },
  ];

  const settings = {
    school_name: 'Janapremi World School',
    school_address: 'Madhyapur Thimi-3, Kaushaltar, Bhaktapur',
    school_phone1: '9744570500', school_phone2: '9744570501',
    school_landline: '01-5910299', school_email: 'jws.staffattendance@gmail.com',
    admin_username: 'admin', admin_password_is_default: '1',
    recovery_email: 'jws.staffattendance@gmail.com',
    smtp_host: 'smtp.gmail.com', smtp_port: '587',
    smtp_user: 'jws.staffattendance@gmail.com', smtp_pass: '',
    calendar_mode: 'bs_with_ad', time_format: '24', weekly_holiday: '6',
    push_port: '8081', connection_mode: 'push',
    update_repo: 'jws-school/attendance-suite', update_channel: 'stable',
    update_check: 'daily', start_with_windows: '1', minimise_to_tray: '1',
    backup_dir: 'D:\\JWS Attendance System\\backup', backup_schedule: 'daily_18',
  };

  const rules = {
    working_days: '0,1,2,3,4,5', late_grace_min: '10', early_grace_min: '10',
    half_day_after_min: '120', absent_after_min: '240', min_full_day_min: '350',
    first_last_punch: '1', require_both_punches: '1', lone_punch_half_day: '1',
    dedupe_window_sec: '60', allow_manual_edit: '1', lock_after_close: '0',
    count_ot: '1', min_ot_block_min: '30', ot_needs_approval: '0',
    holidays_paid: '1', sandwich_rule: '0', late_penalty_enabled: '1',
    late_penalty_count: '3', warn_email_on_3rd_late: '1',
    exempt_heads_from_late: '0', email_absentees: '1', email_absentees_at: '10:00',
    daily_summary_principal: '1', daily_summary_at: '17:00',
    weekly_dept_report: '0', flag_below_percent: '85', recompute_on_rule_change: '1',
  };

  const holidays = [
    { id: 1, name: 'Janai Purnima', from_date: '2026-08-28', to_date: '2026-08-28', applies_to: 'all', paid: 1, days: 1 },
    { id: 2, name: 'Gai Jatra', from_date: '2026-08-29', to_date: '2026-08-29', applies_to: 'all', paid: 1, days: 1 },
    { id: 3, name: 'Krishna Janmashtami', from_date: '2026-09-04', to_date: '2026-09-04', applies_to: 'all', paid: 1, days: 1 },
    { id: 4, name: 'Constitution Day', from_date: '2026-09-19', to_date: '2026-09-19', applies_to: 'all', paid: 1, days: 1 },
    { id: 5, name: 'Dashain', from_date: '2026-10-17', to_date: '2026-10-23', applies_to: 'all', paid: 1, days: 7 },
    { id: 6, name: 'Tihar', from_date: '2026-11-07', to_date: '2026-11-11', applies_to: 'all', paid: 1, days: 5 },
  ];

  const shifts = [
    { id: 1, name: 'Regular Duty', code: 'REG', start_time: '09:00', end_time: '16:00', late_grace: 10, early_grace: 10, break_min: 40, min_full_day: 350, half_day_after: 120, absent_after: 240, overnight: 0, count_ot: 1, min_ot_block: 30, used_in: 12 },
    { id: 2, name: 'Morning Duty', code: 'MRN', start_time: '06:30', end_time: '12:30', late_grace: 5, early_grace: 5, break_min: 20, min_full_day: 315, half_day_after: 120, absent_after: 240, overnight: 0, count_ot: 1, min_ot_block: 30, used_in: 0 },
    { id: 3, name: 'Support Staff', code: 'SUP', start_time: '07:30', end_time: '17:00', late_grace: 15, early_grace: 15, break_min: 60, min_full_day: 480, half_day_after: 120, absent_after: 240, overnight: 0, count_ot: 1, min_ot_block: 30, used_in: 6 },
    { id: 4, name: 'Half Day', code: 'HAF', start_time: '09:00', end_time: '12:30', late_grace: 10, early_grace: 10, break_min: 0, min_full_day: 180, half_day_after: 60, absent_after: 120, overnight: 0, count_ot: 0, min_ot_block: 30, used_in: 1 },
    { id: 5, name: 'Exam Duty', code: 'EXM', start_time: '08:00', end_time: '17:00', late_grace: 0, early_grace: 0, break_min: 45, min_full_day: 465, half_day_after: 120, absent_after: 240, overnight: 0, count_ot: 1, min_ot_block: 30, used_in: 0 },
    { id: 6, name: 'Night Guard', code: 'NGT', start_time: '19:00', end_time: '06:00', late_grace: 15, early_grace: 15, break_min: 0, min_full_day: 600, half_day_after: 120, absent_after: 240, overnight: 1, count_ot: 0, min_ot_block: 30, used_in: 7 },
  ];

  const timetables = [
    { id: 1, name: 'Regular Teaching (Sun-Fri)', assigned: members.filter((m) => m.timetable_id === 1).length, days: 'REG,REG,REG,REG,REG,REG,-' },
    { id: 2, name: 'Administration (Sun-Fri)', assigned: members.filter((m) => m.timetable_id === 2).length, days: 'REG,REG,REG,REG,REG,REG,-' },
    { id: 3, name: 'Support Staff (Sun-Sat)', assigned: members.filter((m) => m.timetable_id === 3).length, days: 'SUP,SUP,SUP,SUP,SUP,SUP,HAF' },
    { id: 4, name: 'Night Security (7 days)', assigned: 0, days: 'NGT,NGT,NGT,NGT,NGT,NGT,NGT' },
  ];


  // The full rule set, matching the shape the desktop build stores in one row.
  const demoRules = {
    unit_name: 'Janapremi World School', unit_abbr: 'JWS',
    week_start: 0, month_start_day: 1, cross_day_belongs_to_first: true,
    longest_zone_min: 1440, shortest_zone_min: 30, least_shift_interval_min: 30,
    out_state: 'as_out', ot_state: 'as_out',
    workday_minutes: 420, late_after_min: 10, early_after_min: 10,
    no_clock_in_enabled: true, no_clock_in_as: 'Absent', no_clock_in_min: 0,
    no_clock_out_enabled: true, no_clock_out_as: 'EarlyLeave', no_clock_out_min: 0,
    late_to_absent_enabled: true, late_to_absent_min: 240,
    early_to_absent_enabled: true, early_to_absent_min: 240,
    half_day_after_min: 120, min_full_day_min: 350,
    ot_after_shift_enabled: true, ot_after_shift_min: 30,
    ot_before_shift_enabled: false, ot_before_shift_min: 30, ot_max_daily_min: 240,
    dedupe_secs: 60, lone_punch_half_day: true,
    sym_normal: 'P', sym_late: 'L', sym_early: 'E', sym_absent: 'A', sym_ot: 'O',
    sym_leave: 'V', sym_holiday: 'H', sym_half_day: 'HD', sym_missing: '?',
    min_unit: 0.5, min_unit_basis: 'workday', rounding: 'off',
    acc_by_times: false, round_at_acc: true, group_by_periods: false,
    weekend_days: [false, false, false, false, false, false, true],
    weekend_as_ot: true, weekend_symbol: 'W', weekend_colour: '#94A3B8',
  };

  const recipients = [
    { id: 1, name: 'Principal', email: 'principal@jws.edu.np', role: 'Principal',
      dept_id: null, dept_name: null, reports: ['daily_stat', 'general'], active: true },
    { id: 2, name: 'Accounts', email: 'accounts@jws.edu.np', role: 'Accountant',
      dept_id: 5, dept_name: 'Accounts', reports: ['ot_summary'], active: true },
  ];

  const syncLog = [
    { ts: `${today} 08:05`, job: 'Download logs', device: 'Main Gate', result: '312 records · 44 new', ok: 1 },
    { ts: `${today} 07:00`, job: 'Upload users', device: 'Main Gate', result: '44 users queued', ok: 1 },
    { ts: `${today} 06:00`, job: 'Download logs', device: 'Back Gate', result: 'Device unreachable', ok: 0 },
  ];

  const auditLog = [
    { id: 1, ts: `${today} 08:04`, actor: 'admin', action: 'auth.login', detail: 'administrator console' },
    { id: 2, ts: `${today} 08:05`, actor: 'system', action: 'sync.download', detail: 'Main Gate · 312 records' },
    { id: 3, ts: `${today} 09:12`, actor: 'admin', action: 'member.update', detail: 'department changed' },
  ];

  const rowsFor = (from, to, deptId, memberId) => {
    const out = [];
    for (const row of attendance.values()) {
      if (row.work_date < from || row.work_date > to) continue;
      if (deptId && members.find((m) => m.id === row.member_id)?.dept_id !== deptId) continue;
      if (memberId && row.member_id !== memberId) continue;
      out.push(row);
    }
    out.sort((a, b) => a.work_date.localeCompare(b.work_date) || a.enroll_no - b.enroll_no);
    return out;
  };

  const summarise = (rows) => {
    const s = {
      working_days: 0, present: 0, late: 0, half_day: 0, absent: 0, leave: 0,
      worked_min: 0, ot_min: 0, late_min: 0,
    };
    for (const r of rows) {
      if (r.status !== 'Holiday' && r.status !== 'WeeklyOff') s.working_days++;
      if (r.status === 'Present') s.present++;
      else if (r.status === 'Late') s.late++;
      else if (r.status === 'HalfDay') s.half_day++;
      else if (r.status === 'Absent' || r.status === 'MissingPunch') s.absent++;
      else if (r.status === 'Leave') s.leave++;
      s.worked_min += r.worked_min;
      s.ot_min += r.ot_min;
      s.late_min += r.late_min;
    }
    s.rate = s.working_days
      ? ((s.present + s.late + s.half_day * 0.5) / s.working_days) * 100
      : 0;
    return s;
  };

  const wait = (v) => new Promise((r) => setTimeout(() => r(v), 30));

  /**
   * Build one report from the demo attendance rows.
   *
   * The column shapes mirror the Rust builders exactly, so the grid, its
   * sorting, its totals row and the export dialog can all be exercised in a
   * plain browser without the Rust half.
   */
  function demoReport(key, filters) {
    const f = filters || {};
    if (f.to < f.from) {
      throw new Error(`The end date (${f.to}) is before the start date (${f.from}).`);
    }
    const label = (REPORT_KINDS.find(([k]) => k === key) || [])[1];
    if (!label) throw new Error(`'${key}' is not a report this version knows how to produce.`);

    const src = rowsFor(f.from, f.to, f.dept_id, f.member_id);
    const col = (k, l, kind) => ({ key: k, label: l, kind });
    const sum = (rows, keys) => Object.fromEntries(
      keys.map((k) => [k, rows.reduce((t, r) => t + (Number(r[k]) || 0), 0)]));

    const byMember = new Map();
    for (const r of src) {
      if (!byMember.has(r.member_id)) byMember.set(r.member_id, []);
      byMember.get(r.member_id).push(r);
    }

    let columns = [];
    let rows = [];
    let totalKeys = [];

    if (key === 'general' || key === 'duty_timetable' || key === 'daily_ot') {
      rows = src
        .filter((r) => key !== 'daily_ot' || (r.ot_min || 0) > 0)
        .map((r) => ({
          ac_no: r.enroll_no, name: r.full_name, dept: r.dept_name || 'Unassigned',
          work_date: r.work_date, timetable: 'Regular Duty',
          shift_name: 'Regular Teaching', on_duty: '09:00', off_duty: '16:00',
          late_grace: 10, early_grace: 10,
          clock_in: r.in_time, clock_out: r.out_time, status: r.status,
          symbol: demoRules.sym_normal,
          worked_min: r.worked_min, late_min: r.late_min, early_min: r.early_min,
          ot_min: r.ot_min, weekend_ot_min: 0, total_ot_min: r.ot_min,
          standard_min: r.worked_min,
          exception: r.in_time && r.out_time ? '' : 'No check-out',
          manual: r.manual ? 'Corrected' : '', remark: r.remark || '',
        }));
      columns = key === 'general' ? [
        col('ac_no', 'AC No.', 'num'), col('name', 'Name', 'text'),
        col('dept', 'Department', 'text'), col('work_date', 'Date', 'date'),
        col('timetable', 'Timetable', 'text'),
        col('clock_in', 'Clock In', 'time'), col('clock_out', 'Clock Out', 'time'),
        col('status', 'Status', 'status'), col('symbol', 'Sym', 'text'),
        col('worked_min', 'Worked', 'mins'), col('late_min', 'Late', 'mins'),
        col('early_min', 'Early', 'mins'), col('ot_min', 'OT', 'mins'),
        col('exception', 'Exception', 'text'), col('manual', 'Edited', 'text'),
        col('remark', 'Remark', 'text'),
      ] : key === 'duty_timetable' ? [
        col('ac_no', 'AC No.', 'num'), col('name', 'Name', 'text'),
        col('dept', 'Department', 'text'), col('work_date', 'Date', 'date'),
        col('shift_name', 'Assigned Shift', 'text'), col('timetable', 'Timetable', 'text'),
        col('on_duty', 'Expected On', 'text'), col('off_duty', 'Expected Off', 'text'),
        col('late_grace', 'Late Grace', 'num'), col('early_grace', 'Early Grace', 'num'),
        col('clock_in', 'Actual In', 'time'), col('clock_out', 'Actual Out', 'time'),
        col('status', 'Status', 'status'),
      ] : [
        col('ac_no', 'AC No.', 'num'), col('name', 'Name', 'text'),
        col('dept', 'Department', 'text'), col('work_date', 'Date', 'date'),
        col('clock_in', 'In', 'time'), col('clock_out', 'Out', 'time'),
        col('off_duty', 'Due Off', 'text'),
        col('standard_min', 'Standard Hours', 'mins'), col('ot_min', 'OT Hours', 'mins'),
        col('weekend_ot_min', 'Weekend OT', 'mins'), col('total_ot_min', 'Total OT', 'mins'),
      ];
      totalKeys = key === 'duty_timetable' ? []
        : key === 'daily_ot' ? ['standard_min', 'ot_min', 'weekend_ot_min', 'total_ot_min']
        : ['worked_min', 'late_min', 'early_min', 'ot_min'];
    } else if (key === 'daily_stat' || key === 'ot_summary') {
      rows = [...byMember.values()].map((rs) => {
        const n = (p) => rs.filter(p).length;
        const t = (k) => rs.reduce((a, r) => a + (Number(r[k]) || 0), 0);
        const working = n((r) => !['Holiday', 'WeeklyOff'].includes(r.status));
        const present = n((r) => ['Present', 'Late', 'EarlyLeave'].includes(r.status));
        const half = n((r) => r.status === 'HalfDay');
        return {
          ac_no: rs[0].enroll_no, name: rs[0].full_name,
          dept: rs[0].dept_name || 'Unassigned', designation: rs[0].designation || '',
          present, late: n((r) => r.status === 'Late'),
          early: n((r) => r.status === 'EarlyLeave'), half_day: half,
          absent: n((r) => r.status === 'Absent'), leave_days: n((r) => r.status === 'Leave'),
          exceptions: n((r) => r.status === 'MissingPunch'),
          late_min: t('late_min'), early_min: t('early_min'),
          worked_min: t('worked_min'), ot_min: t('ot_min'),
          regular_ot_min: t('ot_min'), weekend_ot_min: 0, total_ot_min: t('ot_min'),
          ot_days: n((r) => (r.ot_min || 0) > 0),
          workdays: present + half * 0.5, working_days: working,
          // A period with no working days in it must report zero, not NaN.
          rate: working ? Math.round(((present + half * 0.5) / working) * 1000) / 10 : 0,
        };
      });
      if (key === 'ot_summary') rows = rows.filter((r) => r.total_ot_min > 0);
      columns = key === 'daily_stat' ? [
        col('ac_no', 'AC No.', 'num'), col('name', 'Name', 'text'),
        col('dept', 'Department', 'text'), col('present', 'Present', 'num'),
        col('late', 'Late', 'num'), col('early', 'Early', 'num'),
        col('half_day', 'Half Day', 'num'), col('absent', 'Absent', 'num'),
        col('leave_days', 'Leave', 'num'), col('exceptions', 'Exceptions', 'num'),
        col('late_min', 'Late Time', 'mins'), col('early_min', 'Early Time', 'mins'),
        col('worked_min', 'Worked', 'mins'), col('ot_min', 'OT', 'mins'),
        col('workdays', 'Workdays', 'num'), col('rate', 'Attendance %', 'pct'),
      ] : [
        col('ac_no', 'AC No.', 'num'), col('name', 'Name', 'text'),
        col('dept', 'Department', 'text'), col('designation', 'Designation', 'text'),
        col('ot_days', 'Days with OT', 'num'),
        col('regular_ot_min', 'Regular OT', 'mins'),
        col('weekend_ot_min', 'Weekend OT', 'mins'),
        col('total_ot_min', 'Total OT', 'mins'),
      ];
      totalKeys = key === 'daily_stat'
        ? ['present', 'late', 'early', 'half_day', 'absent', 'leave_days', 'exceptions',
           'late_min', 'early_min', 'worked_min', 'ot_min', 'workdays']
        : ['ot_days', 'regular_ot_min', 'weekend_ot_min', 'total_ot_min'];
    } else {
      // dept_stat, daily_shifts and daily_overtime all group by day.
      const byDay = new Map();
      for (const r of src) {
        const k = key === 'dept_stat' ? `${r.dept_name}|${r.work_date}` : r.work_date;
        if (!byDay.has(k)) byDay.set(k, []);
        byDay.get(k).push(r);
      }
      rows = [...byDay.values()].map((rs) => {
        const n = (p) => rs.filter(p).length;
        const t = (k) => rs.reduce((a, r) => a + (Number(r[k]) || 0), 0);
        const working = n((r) => !['Holiday', 'WeeklyOff'].includes(r.status));
        const present = n((r) => ['Present', 'Late', 'EarlyLeave'].includes(r.status));
        const half = n((r) => r.status === 'HalfDay');
        return {
          dept: rs[0].dept_name || 'Unassigned', work_date: rs[0].work_date,
          shift_name: 'Regular Teaching', timetable: 'Regular Duty',
          on_duty: '09:00', off_duty: '16:00',
          total_staff: new Set(rs.map((r) => r.member_id)).size,
          present, absent: n((r) => r.status === 'Absent'),
          late: n((r) => r.status === 'Late'), half_day: half,
          leave_days: n((r) => r.status === 'Leave'),
          rostered: rs.length, attended: present + half,
          late_min: t('late_min'), ot_min: t('ot_min'),
          regular_ot_min: t('ot_min'), weekend_ot_min: 0, total_ot_min: t('ot_min'),
          longest_min: rs.reduce((a, r) => Math.max(a, r.ot_min || 0), 0),
          staff_on_ot: n((r) => (r.ot_min || 0) > 0),
          rate: working ? Math.round(((present + half * 0.5) / working) * 1000) / 10 : 0,
        };
      });
      if (key === 'daily_overtime') rows = rows.filter((r) => r.total_ot_min > 0);
      columns = key === 'dept_stat' ? [
        col('dept', 'Department', 'text'), col('work_date', 'Date', 'date'),
        col('total_staff', 'Total Staff', 'num'), col('present', 'Present', 'num'),
        col('absent', 'Absent', 'num'), col('late', 'Late', 'num'),
        col('half_day', 'Half Day', 'num'), col('leave_days', 'Leave', 'num'),
        col('ot_min', 'OT', 'mins'), col('rate', 'Attendance %', 'pct'),
      ] : key === 'daily_shifts' ? [
        col('work_date', 'Date', 'date'), col('shift_name', 'Shift', 'text'),
        col('timetable', 'Timetable', 'text'), col('on_duty', 'On Duty', 'text'),
        col('off_duty', 'Off Duty', 'text'), col('rostered', 'Rostered', 'num'),
        col('attended', 'Attended', 'num'), col('absent', 'Absent', 'num'),
        col('late_min', 'Late', 'mins'), col('ot_min', 'OT', 'mins'),
      ] : [
        col('work_date', 'Date', 'date'), col('staff_on_ot', 'Staff on OT', 'num'),
        col('regular_ot_min', 'Regular OT', 'mins'),
        col('weekend_ot_min', 'Weekend OT', 'mins'),
        col('total_ot_min', 'Total OT', 'mins'), col('longest_min', 'Longest', 'mins'),
      ];
      totalKeys = key === 'dept_stat'
        ? ['present', 'absent', 'late', 'half_day', 'leave_days', 'ot_min']
        : key === 'daily_shifts'
        ? ['rostered', 'attended', 'absent', 'late_min', 'ot_min']
        : ['regular_ot_min', 'weekend_ot_min', 'total_ot_min'];
    }

    return {
      key,
      title: label,
      subtitle: `${f.from} to ${f.to}`,
      columns,
      rows,
      totals: sum(rows, totalKeys),
    };
  }

  const handlers = {
    app_info: () => ({
      version: '1.0.0', today, today_bs: '3 Bhadra 2083',
      db_path: 'demo (no database — running in a browser)',
      db_size_kb: 0, schema_version: 2, push_running: false, push_port: 8081,
      password_is_default: true, uptime_secs: 0,
    }),

    dashboard: ({ date }) => {
      const d = date || today;
      const rows = rowsFor(d, d);
      const count = (s) => rows.filter((r) => r.status === s).length;
      const total = members.length;
      const present = count('Present');
      const late = count('Late') + count('HalfDay');
      const monthRows = rowsFor(`${d.slice(0, 7)}-01`, d);
      const m = summarise(monthRows);
      return {
        date: d, date_bs: '3 Bhadra 2083', total_staff: total,
        present, late, absent: count('Absent'), not_in: count('MissingPunch'),
        leave: count('Leave'), holiday_name: null, is_working_day: true,
        rate: total ? ((present + late) / total) * 100 : 0,
        month_rate: m.rate,
      };
    },

    attendance_trend: ({ days: n }) => {
      const out = [];
      const seen = new Set();
      for (const row of attendance.values()) {
        if (row.status === 'Holiday' || row.status === 'WeeklyOff') continue;
        seen.add(row.work_date);
      }
      for (const date of [...seen].sort().slice(-n)) {
        const rows = rowsFor(date, date);
        out.push({
          date,
          present: rows.filter((r) => r.status === 'Present').length,
          late: rows.filter((r) => r.status === 'Late' || r.status === 'HalfDay').length,
          absent: rows.filter((r) => r.status === 'Absent').length,
        });
      }
      return out;
    },

    department_stats: ({ date }) => {
      const d = date || today;
      return DEPTS.map((dep) => {
        const mem = members.filter((m) => m.dept_id === dep.id);
        const present = mem.filter((m) => {
          const r = attendance.get(`${m.id}|${d}`);
          return r && ['Present', 'Late', 'HalfDay'].includes(r.status);
        }).length;
        return {
          id: dep.id, name: dep.name, colour: dep.colour,
          total: mem.length, present,
          rate: mem.length ? (present / mem.length) * 100 : 0,
        };
      });
    },

    punch_feed: ({ limit }) => punches.slice(0, limit),

    list_members: ({ search, deptId, status }) => {
      let r = members;
      if (search) {
        const q = String(search).toLowerCase();
        r = r.filter(
          (m) =>
            m.full_name.toLowerCase().includes(q) ||
            (m.staff_id || '').toLowerCase().includes(q) ||
            (m.card_no || '').includes(q) ||
            String(m.enroll_no) === q,
        );
      }
      if (deptId) r = r.filter((m) => m.dept_id === deptId);
      if (status) r = r.filter((m) => m.status === status);
      return r;
    },

    save_member: ({ member }) => {
      if (!member.full_name?.trim()) throw new Error('Full name is required.');
      if (!member.enroll_no || member.enroll_no > 65535) {
        throw new Error('Enrolment number must be between 1 and 65535 — that is the range the terminal accepts.');
      }
      const clash = members.find((m) => m.enroll_no === member.enroll_no && m.id !== member.id);
      if (clash) throw new Error(`Enrolment number ${member.enroll_no} is already used by ${clash.full_name}.`);
      const dept = DEPTS.find((d) => d.id === member.dept_id);
      if (member.id) {
        const i = members.findIndex((m) => m.id === member.id);
        members[i] = { ...members[i], ...member, dept_name: dept?.name, dept_code: dept?.code, dept_colour: dept?.colour };
        return member.id;
      }
      const id = Math.max(...members.map((m) => m.id)) + 1;
      members.push({
        ...member, id, fp_count: 0,
        dept_name: dept?.name, dept_code: dept?.code, dept_colour: dept?.colour,
      });
      return id;
    },

    delete_members: ({ ids }) => {
      for (const id of ids) {
        const i = members.findIndex((m) => m.id === id);
        if (i >= 0) members.splice(i, 1);
      }
      return ids.length;
    },

    set_members_department: ({ ids, deptId }) => {
      const dept = DEPTS.find((d) => d.id === deptId);
      for (const id of ids) {
        const m = members.find((x) => x.id === id);
        if (m) Object.assign(m, { dept_id: deptId, dept_name: dept?.name, dept_code: dept?.code, dept_colour: dept?.colour });
      }
      return ids.length;
    },

    list_departments: () =>
      DEPTS.map((d) => {
        const mem = members.filter((m) => m.dept_id === d.id);
        return {
          ...d,
          head_member_id: mem[0]?.id ?? null,
          head_name: mem[0]?.full_name ?? null,
          default_timetable_id: d.id === 8 ? 3 : 1,
          timetable_name: d.id === 8 ? 'Support Staff (Sun-Sat)' : 'Regular Teaching (Sun-Fri)',
          member_count: mem.length,
        };
      }),

    save_department: ({ dept }) => {
      if (!dept.name?.trim()) throw new Error('Department name is required.');
      if (dept.id) {
        Object.assign(DEPTS.find((d) => d.id === dept.id), dept);
        return dept.id;
      }
      const id = Math.max(...DEPTS.map((d) => d.id)) + 1;
      DEPTS.push({ id, ...dept });
      return id;
    },

    delete_department: ({ id }) => {
      const n = members.filter((m) => m.dept_id === id).length;
      if (n) throw new Error(`This department still has ${n} staff. Move them first, then delete it.`);
      DEPTS.splice(DEPTS.findIndex((d) => d.id === id), 1);
    },

    list_devices: () => devices,
    save_device: ({ device }) => {
      if (!device.name?.trim() || !device.ip?.trim()) {
        throw new Error('Device name and IP address are required.');
      }
      if (device.id) {
        Object.assign(devices.find((d) => d.id === device.id), device);
        return device.id;
      }
      const id = devices.length + 1;
      devices.push({ ...device, id, serial: null, mac: null, mode: 'push', auto_connect: true, last_seen: null });
      return id;
    },
    device_ping: () => {
      throw new Error('No terminal can be reached from a web browser. Run the installed application on the school computer.');
    },
    device_info: () => {
      throw new Error('No terminal can be reached from a web browser.');
    },
    device_download_logs: () => {
      throw new Error('No terminal can be reached from a web browser.');
    },
    device_download_users: () => {
      throw new Error('No terminal can be reached from a web browser.');
    },
    device_upload_users: ({ memberIds }) => memberIds.length,

    push_start: () => 8081,
    push_stop: () => null,
    local_addresses: () => ['192.168.100.50'],

    attendance_range: ({ from, to, deptId, memberId, withBs }) =>
      rowsFor(from, to, deptId, memberId).map((r) => ({
        ...r, work_date_bs: withBs ? '— (desktop only)' : null,
      })),

    recompute: ({ from, to }) => rowsFor(from, to).length,

    override_attendance: ({ memberId, workDate, status, inTime, outTime, remark }) => {
      const key = `${memberId}|${workDate}`;
      const row = attendance.get(key);
      if (row) Object.assign(row, { status, in_time: inTime, out_time: outTime, remark, manual: true });
    },

    // --- the three-tier roster, as the desktop build now models it --------
    // `shifts` in the demo data is the old flat list of duty blocks, which is
    // exactly what a *timetable* now means; the weekly plans are the shifts.
    list_timetables_full: () =>
      shifts.map((s, i) => ({
        id: s.id, name: s.name, on_duty: s.start_time, off_duty: s.end_time,
        in_begin: '06:00', in_end: '12:30', out_begin: '12:30', out_end: '21:00',
        late_grace: s.late_grace ?? 10, early_grace: s.early_grace ?? 10,
        break_min: s.break_min ?? 0, workday_value: 1, work_minutes: 0,
        must_c_in: true, must_c_out: true, count_ot: true, min_ot_block: 30,
        colour: ['#F16522', '#2563EB', '#16A34A', '#9333EA', '#DC2626', '#0891B2'][i % 6],
        active: true, used_by: 1,
      })),
    save_timetable: ({ tt }) => tt.id || shifts.length + 1,
    delete_timetable: () => null,

    list_shift_cycles: () =>
      timetables.map((t) => ({
        id: t.id, name: t.name, code: '', begin_date: todayIso(),
        cycle_num: 1, cycle_unit: 'Week', active: true, assigned: t.assigned ?? 0,
      })),
    save_shift: ({ shift }) => shift.id || timetables.length + 1,
    delete_shift: () => null,
    shift_grid: ({ shiftId }) => {
      const t = timetables.find((x) => x.id === shiftId);
      if (!t) return [];
      // "REG,REG,REG,REG,REG,REG,-" — a dash is a rest day.
      return String(t.days || '').split(',').flatMap((code, day) => {
        if (!code || code === '-') return [];
        const block = shifts.find((s) => s.code === code) || shifts[0];
        return [{
          id: shiftId * 100 + day, day_index: day, timetable_id: block.id,
          timetable_name: block.name, on_duty: block.start_time,
          off_duty: block.end_time, colour: '#F16522',
        }];
      });
    },
    add_shift_item: () => 1,
    delete_shift_item: () => null,
    clear_shift_grid: () => 0,

    department_tree: () => DEPTS.map((d) => ({
      id: d.id, name: d.name, code: d.code, colour: d.colour,
      member_count: members.filter((m) => m.dept_id === d.id).length,
    })),
    roster: ({ deptId }) =>
      members.filter((m) => !deptId || m.dept_id === deptId).map((m) => ({
        id: m.id, member_id: m.id, member_name: m.full_name, enroll_no: m.enroll_no,
        shift_id: 1, shift_name: timetables[0]?.name || 'Regular',
        start_date: '2025-01-01', end_date: null, is_temporary: false, note: null,
      })),
    save_schedule: ({ row }) => row.id || 1,
    delete_schedule: () => null,
    arrange_shifts: ({ memberIds }) => memberIds.length,
    member_calendar: ({ memberId, from, to }) => {
      const out = [];
      let d = from;
      // Bounded so a mistyped range cannot spin the browser.
      for (let i = 0; i < 120 && d <= to; i++) {
        const wd = new Date(`${d}T00:00:00`).getDay();
        const off = wd === 6;
        out.push({
          date: d, date_bs: null, weekday: wd, is_weekend: off, holiday: null,
          plan: {
            shift_id: 1,
            timetables: off ? [] : [{
              id: 1, name: 'Regular Duty', on_min: 540, off_min: 960,
              in_begin: 360, in_end: 750, out_begin: 750, out_end: 1260,
              late_grace: 10, early_grace: 10, break_min: 40, workday_value: 1,
              work_minutes: 0, must_c_in: true, must_c_out: true,
              count_ot: true, min_ot_block: 30, colour: '#F16522',
            }],
          },
        });
        d = addDaysIso(d, 1);
      }
      return out;
    },

    list_holidays: () => holidays,
    save_holiday: ({ holiday }) => {
      if (holiday.to_date < holiday.from_date) throw new Error('The end date is before the start date.');
      if (holiday.id) {
        Object.assign(holidays.find((h) => h.id === holiday.id), holiday);
        return holiday.id;
      }
      const id = holidays.length + 1;
      holidays.push({ ...holiday, id, days: 1, paid: 1, applies_to: 'all' });
      return id;
    },
    delete_holiday: ({ id }) => {
      holidays.splice(holidays.findIndex((h) => h.id === id), 1);
    },
    bs_calendar: () => BS_TABLE,

    get_attendance_rules: () => ({ ...demoRules }),
    save_attendance_rules: ({ rules: r }) => {
      // Mirror the backend's own refusal, so the screen can be exercised.
      if (r.half_day_after_min >= r.late_to_absent_min && r.late_to_absent_enabled) {
        throw new Error('Half day must come before absent, or nobody is ever a half day.');
      }
      if (r.weekend_days.every(Boolean)) throw new Error('Every day cannot be a weekend.');
      Object.assign(demoRules, r);
    },

    report_kinds: () => REPORT_KINDS.map(([key, label]) => ({ key, label })),
    run_report: ({ key, filters }) => demoReport(key, filters),
    export_report: ({ path }) => path,
    list_recipients: () => recipients,
    save_recipient: ({ r }) => {
      if (!String(r.email).includes('@')) throw new Error(`'${r.email}' is missing an @.`);
      if (r.id) {
        Object.assign(recipients.find((x) => x.id === r.id), r);
        return r.id;
      }
      const id = recipients.length + 1;
      recipients.push({ ...r, id, dept_name: null });
      return id;
    },
    delete_recipient: ({ id }) => {
      const i = recipients.findIndex((x) => x.id === id);
      if (i >= 0) recipients.splice(i, 1);
    },
    send_report_email: ({ recipientIds }) => ({
      sent: recipientIds.length, failed: 0,
      details: recipientIds.map((i) => `Sent to recipient ${i}`),
    }),
    report_mail_log: () => [],

    get_rules: () => ({ ...rules }),
    set_rules: ({ rules: r }) => {
      Object.assign(rules, r);
      return Object.keys(r).length;
    },
    get_settings: () => ({ ...settings, smtp_pass: '' }),
    set_settings: ({ settings: s }) => {
      Object.assign(settings, s);
      return Object.keys(s).length;
    },

    browse_table: ({ table, limit = 500 }) => {
      const source = {
        members, departments: DEPTS, attendance: [...attendance.values()],
        punches, devices, shifts, timetables, holidays, audit_log: auditLog,
        sync_log: syncLog, rules: Object.entries(rules).map(([key, value]) => ({ key, value })),
        settings: Object.entries(settings).map(([key, value]) => ({ key, value })),
        leaves: [], shift_items: [], employee_schedules: [], device_commands: [],
      }[table];
      if (!source) throw new Error(`'${table}' is not a table you can browse.`);
      return { total: source.length, rows: source.slice(0, limit) };
    },

    sync_history: ({ limit }) => syncLog.slice(0, limit),

    report_summary: ({ from, to, deptId }) =>
      members
        .filter((m) => !deptId || m.dept_id === deptId)
        .map((m) => {
          const s = summarise(rowsFor(from, to, null, m.id));
          return {
            member_id: m.id, enroll_no: m.enroll_no, full_name: m.full_name,
            dept_name: m.dept_name, designation: m.designation, ...s,
          };
        }),

    export_csv: ({ from, to, deptId }) => {
      const rows = rowsFor(from, to, deptId);
      const head = 'Date,Enroll,Name,Department,In,Out,Late,Status\n';
      return head + rows
        .map((r) => `${r.work_date},${r.enroll_no},"${r.full_name}",${r.dept_name},${r.in_time || ''},${r.out_time || ''},${r.late_min},${r.status}`)
        .join('\n');
    },

    auth_login: ({ username, password }) =>
      username === 'admin' && password === 'Attendance@123',
    auth_change_password: ({ current }) => {
      if (current !== 'Attendance@123') throw new Error('The current password is not correct.');
    },
    auth_request_reset: () => 'jws***@gmail.com',
    auth_verify_reset: ({ code }) => {
      if (code !== '000000') throw new Error('That code is not correct.');
    },

    send_test_mail: () => {
      throw new Error('Email can only be sent from the installed application.');
    },
    send_absence_emails: () => {
      throw new Error('Email can only be sent from the installed application.');
    },

    backup_now: () => {
      throw new Error('Backups are only available in the installed application.');
    },
    list_backups: () => [],
    open_path: () => null,
    save_text_file: () => null,
  };

  return {
    call(cmd, args) {
      const h = handlers[cmd];
      if (!h) return Promise.reject(new Error(`Unknown command '${cmd}'.`));
      try {
        return wait(h(args || {}));
      } catch (e) {
        return Promise.reject(e);
      }
    },
  };
}
