// Bridge to the Rust backend.
//
// Every call goes through `call()`, which does two useful things: it turns a
// backend error into a thrown Error carrying the message the Rust side wrote
// (those messages are written for the office to read, so they are worth
// surfacing verbatim), and it falls back to an in-memory demo dataset when the
// page is opened in a plain browser rather than inside the app window.
//
// That fallback is what makes `npm run dev` useful for working on the
// interface without building the Rust half.

const tauri = () => window.__TAURI__?.core?.invoke;
export const isDesktop = () => typeof tauri() === 'function';

let demo = null;

export async function call(cmd, args = {}) {
  const invoke = tauri();
  if (invoke) {
    try {
      return await invoke(cmd, args);
    } catch (e) {
      throw new Error(typeof e === 'string' ? e : e?.message || String(e));
    }
  }
  if (!demo) demo = (await import('./demo.js')).createDemo();
  return demo.call(cmd, args);
}

/** Subscribe to a backend event (live punches, device coming online). */
export async function listen(event, handler) {
  const l = window.__TAURI__?.event?.listen;
  if (!l) return () => {};
  return l(event, (e) => handler(e.payload));
}

// --- typed wrappers -------------------------------------------------------
// Thin, but they keep command names in one place so a rename is a single edit.

export const api = {
  appInfo: () => call('app_info'),

  dashboard: (date) => call('dashboard', { date }),
  trend: (days) => call('attendance_trend', { days }),
  departmentStats: (date) => call('department_stats', { date }),
  punchFeed: (limit = 12) => call('punch_feed', { limit }),

  listMembers: (search, deptId, status) =>
    call('list_members', { search: search || null, deptId: deptId ?? null, status: status || null }),
  saveMember: (member) => call('save_member', { member }),
  deleteMembers: (ids) => call('delete_members', { ids }),
  setMembersDepartment: (ids, deptId) => call('set_members_department', { ids, deptId }),

  listDepartments: () => call('list_departments'),
  saveDepartment: (dept) => call('save_department', { dept }),
  deleteDepartment: (id) => call('delete_department', { id }),

  listDevices: () => call('list_devices'),
  saveDevice: (device) => call('save_device', { device }),
  devicePing: (ip, port) => call('device_ping', { ip, port }),
  deviceInfo: (ip, port, commKey = 0) => call('device_info', { ip, port, commKey }),
  downloadLogs: (ip, port, commKey, serial, clearAfter) =>
    call('device_download_logs', { ip, port, commKey, serial, clearAfter }),
  downloadUsers: (ip, port, commKey) => call('device_download_users', { ip, port, commKey }),
  uploadUsers: (memberIds) => call('device_upload_users', { memberIds }),

  pushStart: (port) => call('push_start', { port: port ?? null }),
  pushStop: () => call('push_stop'),
  localAddresses: () => call('local_addresses'),

  attendance: (from, to, deptId, memberId, withBs) =>
    call('attendance_range', {
      from, to, deptId: deptId ?? null, memberId: memberId ?? null, withBs: !!withBs,
    }),
  recompute: (from, to) => call('recompute', { from, to }),
  overrideAttendance: (p) => call('override_attendance', p),

  listHolidays: () => call('list_holidays'),
  saveHoliday: (holiday) => call('save_holiday', { holiday }),
  deleteHoliday: (id) => call('delete_holiday', { id }),

  // --- Timetables: the atomic blocks of duty ------------------------------
  listTimetables: () => call('list_timetables_full'),
  saveTimetable: (tt) => call('save_timetable', { tt }),
  deleteTimetable: (id) => call('delete_timetable', { id }),

  // --- Shifts: the repeating cycles ---------------------------------------
  listShifts: () => call('list_shift_cycles'),
  saveShift: (shift) => call('save_shift', { shift }),
  deleteShift: (id) => call('delete_shift', { id }),
  shiftGrid: (shiftId) => call('shift_grid', { shiftId }),
  addShiftItem: (shiftId, dayIndex, timetableId) =>
    call('add_shift_item', { shiftId, dayIndex, timetableId }),
  deleteShiftItem: (id) => call('delete_shift_item', { id }),
  clearShiftGrid: (shiftId) => call('clear_shift_grid', { shiftId }),

  // --- Who works which shift ----------------------------------------------
  departmentTree: () => call('department_tree'),
  roster: (deptId) => call('roster', { deptId: deptId ?? null }),
  saveSchedule: (row) => call('save_schedule', { row }),
  deleteSchedule: (id) => call('delete_schedule', { id }),
  arrangeShifts: (memberIds, shiftId, startDate, endDate, isTemporary) =>
    call('arrange_shifts', {
      memberIds, shiftId, startDate, endDate: endDate || null, isTemporary: !!isTemporary,
    }),
  memberCalendar: (memberId, from, to) => call('member_calendar', { memberId, from, to }),

  // --- The Nepali calendar -------------------------------------------------
  bsCalendar: () => call('bs_calendar'),

  // --- Attendance rules ----------------------------------------------------
  getAttendanceRules: () => call('get_attendance_rules'),
  saveAttendanceRules: (rules) => call('save_attendance_rules', { rules }),

  // --- Reports -------------------------------------------------------------
  reportKinds: () => call('report_kinds'),
  runReport: (key, filters) => call('run_report', { key, filters }),
  exportReport: (key, filters, format, path) =>
    call('export_report', { key, filters, format, path }),
  listRecipients: () => call('list_recipients'),
  saveRecipient: (r) => call('save_recipient', { r }),
  deleteRecipient: (id) => call('delete_recipient', { id }),
  sendReportEmail: (key, filters, recipientIds, note) =>
    call('send_report_email', { key, filters, recipientIds, note: note || null }),
  reportMailLog: (limit = 25) => call('report_mail_log', { limit }),

  getRules: () => call('get_rules'),
  setRules: (rules) => call('set_rules', { rules }),
  getSettings: () => call('get_settings'),
  setSettings: (settings) => call('set_settings', { settings }),

  browseTable: (table, limit, offset) => call('browse_table', { table, limit, offset }),
  syncHistory: (limit = 10) => call('sync_history', { limit }),

  reportSummary: (from, to, deptId) => call('report_summary', { from, to, deptId: deptId ?? null }),
  exportCsv: (from, to, deptId, withBs) =>
    call('export_csv', { from, to, deptId: deptId ?? null, withBs: !!withBs }),

  login: (username, password) => call('auth_login', { username, password }),
  changePassword: (current, next) => call('auth_change_password', { current, new: next }),
  requestReset: () => call('auth_request_reset'),
  verifyReset: (code, newPassword) => call('auth_verify_reset', { code, newPassword }),

  sendTestMail: (to) => call('send_test_mail', { to: to || null }),
  sendAbsenceEmails: (date) => call('send_absence_emails', { date: date || null }),

  backupNow: () => call('backup_now'),
  listBackups: () => call('list_backups'),
  openPath: (path) => call('open_path', { path }),
  saveTextFile: (path, contents) => call('save_text_file', { path, contents }),
};
