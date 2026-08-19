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

    list_shifts: () => shifts,
    list_timetables: () => timetables,
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
    set_member_timetable: ({ memberId, timetableId }) => {
      const m = members.find((x) => x.id === memberId);
      if (m) m.timetable_id = timetableId;
    },

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
        leaves: [], timetable_days: [], device_commands: [],
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
