-- 003_scheduling: the three-tier scheduling model, the full rule set, report
-- delivery, and the device tables for fingerprints, photos and access control.
--
-- ## Why this renames two tables
--
-- 001 used "shift" for an atomic block of time and "timetable" for the weekly
-- plan built out of them. Every ZKTeco product the office has ever used — and
-- the specification this school wrote — uses those two words the other way
-- round: a *timetable* is one block of duty ("10:00 to 16:30"), and a *shift*
-- is the weekly cycle that assigns timetables to days.
--
-- Keeping our own vocabulary would mean every screen label disagreeing with
-- every conversation held about it. So the two tables swap names here. This is
-- the only migration that will ever do so; after this the words are fixed.
--
--   timetables         one block of duty: on/off time, windows, graces
--   shifts             a cycle (weekly or monthly) with a start date
--   shift_items        which timetable applies on which day of the cycle
--   employee_schedules which shift a member is on, between which dates
--
-- Rows are carried across, not dropped: an existing installation keeps its
-- shifts, its weekly plans and every member's assignment.

-- ---------------------------------------------------------------------------
-- 1. Move the old tables aside
-- ---------------------------------------------------------------------------

ALTER TABLE shifts         RENAME TO _old_shifts;
ALTER TABLE timetables     RENAME TO _old_timetables;
ALTER TABLE timetable_days RENAME TO _old_timetable_days;

-- ---------------------------------------------------------------------------
-- 2. Timetables — the atomic block of duty
-- ---------------------------------------------------------------------------

CREATE TABLE timetables (
    id             INTEGER PRIMARY KEY,
    name           TEXT NOT NULL UNIQUE,
    on_duty        TEXT NOT NULL,               -- 'HH:MM'
    off_duty       TEXT NOT NULL,

    -- Any scan inside the in-window is treated as an arrival, any scan inside
    -- the out-window as a departure. This is what stops a 12:05 lunch scan
    -- being read as the day's check-out.
    in_begin       TEXT NOT NULL DEFAULT '04:00',
    in_end         TEXT NOT NULL DEFAULT '12:00',
    out_begin      TEXT NOT NULL DEFAULT '12:00',
    out_end        TEXT NOT NULL DEFAULT '19:00',

    late_grace     INTEGER NOT NULL DEFAULT 10,
    early_grace    INTEGER NOT NULL DEFAULT 10,
    break_min      INTEGER NOT NULL DEFAULT 0,

    -- How much of a day this block is worth. 1.0 for a full day, 0.5 for a
    -- half-day block. work_minutes overrides the on/off span when non-zero,
    -- for blocks whose paid length differs from their clock length.
    workday_value  REAL NOT NULL DEFAULT 1.0,
    work_minutes   INTEGER NOT NULL DEFAULT 0,

    -- A day is incomplete without the punch, however long the person was here.
    must_c_in      INTEGER NOT NULL DEFAULT 1,
    must_c_out     INTEGER NOT NULL DEFAULT 1,

    count_ot       INTEGER NOT NULL DEFAULT 1,
    min_ot_block   INTEGER NOT NULL DEFAULT 30,

    -- Drawn on the shift timeline and the roster calendar.
    colour         TEXT NOT NULL DEFAULT '#F16522',
    active         INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

INSERT INTO timetables
    (id, name, on_duty, off_duty, in_begin, in_end, out_begin, out_end,
     late_grace, early_grace, break_min, workday_value, work_minutes,
     must_c_in, must_c_out, count_ot, min_ot_block, colour, active)
SELECT
    id,
    name,
    start_time,
    end_time,
    in_window_start,
    -- The old schema had no explicit in/out boundary. Split the day at the
    -- midpoint of the shift, which is where a lunch scan naturally falls and
    -- reproduces the behaviour the old engine had by ordering alone.
    -- substr to 'HH:MM': SQLite's time() returns seconds too, and every other
    -- clock column in this schema is five characters. One shape only.
    substr(time((strftime('%s', '2000-01-01 ' || start_time || ':00')
        + strftime('%s', '2000-01-01 ' || end_time   || ':00')
        + CASE WHEN end_time <= start_time THEN 86400 ELSE 0 END) / 2, 'unixepoch'), 1, 5),
    -- substr to 'HH:MM': SQLite's time() returns seconds too, and every other
    -- clock column in this schema is five characters. One shape only.
    substr(time((strftime('%s', '2000-01-01 ' || start_time || ':00')
        + strftime('%s', '2000-01-01 ' || end_time   || ':00')
        + CASE WHEN end_time <= start_time THEN 86400 ELSE 0 END) / 2, 'unixepoch'), 1, 5),
    out_window_end,
    late_grace,
    early_grace,
    break_min,
    1.0,
    0,
    1,
    1,
    count_ot,
    min_ot_block,
    '#F16522',
    active
FROM _old_shifts;

-- ---------------------------------------------------------------------------
-- 3. Shifts — a repeating cycle
-- ---------------------------------------------------------------------------

CREATE TABLE shifts (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    code       TEXT NOT NULL DEFAULT '',
    -- Day zero of the cycle. For a weekly cycle this only matters when the
    -- cycle is longer than one week.
    begin_date TEXT NOT NULL DEFAULT (date('now','localtime')),
    cycle_num  INTEGER NOT NULL DEFAULT 1 CHECK (cycle_num BETWEEN 1 AND 12),
    cycle_unit TEXT NOT NULL DEFAULT 'Week' CHECK (cycle_unit IN ('Week','Month')),
    active     INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

INSERT INTO shifts (id, name, code, cycle_num, cycle_unit, active)
SELECT id, name, '', 1, 'Week', active FROM _old_timetables;

-- ---------------------------------------------------------------------------
-- 4. Shift items — which timetable on which day of the cycle
-- ---------------------------------------------------------------------------

-- day_index is 0-based from the start of the cycle: 0-6 for a one-week cycle
-- (0 = Sunday), 0-13 for two weeks, and so on. A day with no row is a rest day,
-- which is how Saturday is modelled for JWS. A day may carry more than one
-- timetable — a split morning/evening duty is two rows.
CREATE TABLE shift_items (
    id           INTEGER PRIMARY KEY,
    shift_id     INTEGER NOT NULL REFERENCES shifts(id) ON DELETE CASCADE,
    day_index    INTEGER NOT NULL CHECK (day_index BETWEEN 0 AND 371),
    timetable_id INTEGER NOT NULL REFERENCES timetables(id) ON DELETE CASCADE,
    UNIQUE (shift_id, day_index, timetable_id)
);
CREATE INDEX idx_shift_items ON shift_items(shift_id, day_index);

INSERT INTO shift_items (shift_id, day_index, timetable_id)
SELECT timetable_id, weekday, shift_id
FROM _old_timetable_days
WHERE shift_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- 5. Employee schedules — who is on which shift, when
-- ---------------------------------------------------------------------------

-- end_date NULL means "until further notice". A temporary row wins over a
-- permanent one covering the same date, which is how a one-week exam duty is
-- applied without disturbing the standing assignment underneath it.
CREATE TABLE employee_schedules (
    id           INTEGER PRIMARY KEY,
    member_id    INTEGER NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    shift_id     INTEGER NOT NULL REFERENCES shifts(id) ON DELETE CASCADE,
    start_date   TEXT NOT NULL,
    end_date     TEXT,
    is_temporary INTEGER NOT NULL DEFAULT 0,
    note         TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX idx_empsched_member ON employee_schedules(member_id, start_date);
CREATE INDEX idx_empsched_shift  ON employee_schedules(shift_id);

INSERT INTO employee_schedules (member_id, shift_id, start_date, end_date, is_temporary, note)
SELECT
    id,
    timetable_id,
    -- Backdate to the joining date so historical recomputes resolve a shift.
    -- Members with no joining date get a date early enough to cover any punch
    -- already in the database.
    COALESCE(joined_on, '2000-01-01'),
    NULL,
    0,
    'Carried over from the previous version'
FROM members
WHERE timetable_id IS NOT NULL;

-- ---------------------------------------------------------------------------
-- 6. Rebuild the tables that referenced the old names
-- ---------------------------------------------------------------------------

-- departments: default_timetable_id meant a weekly plan, which is now a shift.
CREATE TABLE _new_departments (
    id               INTEGER PRIMARY KEY,
    name             TEXT NOT NULL UNIQUE,
    code             TEXT NOT NULL,
    head_member_id   INTEGER REFERENCES members(id) ON DELETE SET NULL,
    default_shift_id INTEGER REFERENCES shifts(id) ON DELETE SET NULL,
    colour           TEXT NOT NULL DEFAULT '#F16522',
    in_reports       INTEGER NOT NULL DEFAULT 1,
    active           INTEGER NOT NULL DEFAULT 1,
    created_at       TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
INSERT INTO _new_departments
    (id, name, code, head_member_id, default_shift_id, colour, in_reports, active, created_at)
SELECT id, name, code, head_member_id, default_timetable_id, colour, in_reports, active, created_at
FROM departments;
DROP TABLE departments;
ALTER TABLE _new_departments RENAME TO departments;

-- members: timetable_id is replaced by employee_schedules rows.
CREATE TABLE _new_members (
    id              INTEGER PRIMARY KEY,
    enroll_no       INTEGER NOT NULL UNIQUE,
    staff_id        TEXT,
    full_name       TEXT NOT NULL,
    device_name     TEXT,
    dept_id         INTEGER REFERENCES departments(id) ON DELETE SET NULL,
    designation     TEXT,
    gender          TEXT,
    dob             TEXT,
    mobile          TEXT,
    email           TEXT,
    card_no         TEXT,
    privilege       INTEGER NOT NULL DEFAULT 0,
    device_password TEXT,
    fp_count        INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'Active',
    joined_on       TEXT,
    -- Access control: which device group this member belongs to.
    access_group    INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
INSERT INTO _new_members
    (id, enroll_no, staff_id, full_name, device_name, dept_id, designation, gender,
     dob, mobile, email, card_no, privilege, device_password, fp_count, status,
     joined_on, created_at, updated_at)
SELECT
    id, enroll_no, staff_id, full_name, device_name, dept_id, designation, gender,
    dob, mobile, email, card_no, privilege, device_password, fp_count, status,
    joined_on, created_at, updated_at
FROM members;
DROP TABLE members;
ALTER TABLE _new_members RENAME TO members;
CREATE INDEX idx_members_dept   ON members(dept_id);
CREATE INDEX idx_members_status ON members(status);

-- attendance: shift_id pointed at an atomic block, which is now a timetable.
-- Two new columns carry what the richer rule set produces.
CREATE TABLE _new_attendance (
    id            INTEGER PRIMARY KEY,
    member_id     INTEGER NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    work_date     TEXT NOT NULL,
    timetable_id  INTEGER REFERENCES timetables(id) ON DELETE SET NULL,
    shift_id      INTEGER REFERENCES shifts(id) ON DELETE SET NULL,
    in_time       TEXT,
    out_time      TEXT,
    worked_min    INTEGER NOT NULL DEFAULT 0,
    late_min      INTEGER NOT NULL DEFAULT 0,
    early_min     INTEGER NOT NULL DEFAULT 0,
    ot_min        INTEGER NOT NULL DEFAULT 0,
    -- Overtime worked on a day the weekend rules mark as a rest day, kept
    -- apart because schools usually pay it at a different rate.
    weekend_ot_min INTEGER NOT NULL DEFAULT 0,
    -- Workday credit after the rounding rules are applied: 1.0, 0.5, 0.0.
    workday_value REAL NOT NULL DEFAULT 0.0,
    -- 'MissingIn' | 'MissingOut' | 'Both' | NULL — drives the exception column.
    exception     TEXT,
    status        TEXT NOT NULL,
    remark        TEXT,
    manual        INTEGER NOT NULL DEFAULT 0,
    locked        INTEGER NOT NULL DEFAULT 0,
    computed_at   TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    UNIQUE (member_id, work_date)
);
INSERT INTO _new_attendance
    (id, member_id, work_date, timetable_id, in_time, out_time, worked_min,
     late_min, early_min, ot_min, status, remark, manual, locked, computed_at)
SELECT
    id, member_id, work_date, shift_id, in_time, out_time, worked_min,
    late_min, early_min, ot_min, status, remark, manual, locked, computed_at
FROM attendance;
DROP TABLE attendance;
ALTER TABLE _new_attendance RENAME TO attendance;
CREATE INDEX idx_att_date   ON attendance(work_date);
CREATE INDEX idx_att_member ON attendance(member_id, work_date);
CREATE INDEX idx_att_status ON attendance(status);

DROP TABLE _old_timetable_days;
DROP TABLE _old_timetables;
DROP TABLE _old_shifts;

-- ---------------------------------------------------------------------------
-- 7. Attendance rules — one row, every setting on the four sub-tabs
-- ---------------------------------------------------------------------------

-- Deliberately one wide row rather than the key/value `rules` table. These
-- settings are read together on every single recompute, they have real types
-- that SQLite can enforce, and a typo in a key name should be a compile error
-- rather than a silently-defaulted rule. The CHECK on the id is what keeps it
-- to one row.
CREATE TABLE attendance_rules (
    id                       INTEGER PRIMARY KEY CHECK (id = 1),

    -- --- Basic settings ---
    unit_name                TEXT    NOT NULL DEFAULT 'Janapremi World School',
    unit_abbr                TEXT    NOT NULL DEFAULT 'JWS',
    week_start               INTEGER NOT NULL DEFAULT 0 CHECK (week_start BETWEEN 0 AND 6),
    month_start_day          INTEGER NOT NULL DEFAULT 1 CHECK (month_start_day BETWEEN 1 AND 31),
    -- Which calendar day owns a shift that crosses midnight.
    cross_day_belongs        TEXT    NOT NULL DEFAULT 'first'
                                     CHECK (cross_day_belongs IN ('first','second')),
    longest_zone_min         INTEGER NOT NULL DEFAULT 1440,
    shortest_zone_min        INTEGER NOT NULL DEFAULT 30,
    least_shift_interval_min INTEGER NOT NULL DEFAULT 30,
    out_state                TEXT    NOT NULL DEFAULT 'as_out'
                                     CHECK (out_state IN ('ignore','as_out','as_business_out','audit')),
    ot_state                 TEXT    NOT NULL DEFAULT 'as_ot'
                                     CHECK (ot_state IN ('ignore','as_ot','as_business_out','audit')),

    -- --- Calculation ---
    workday_minutes          INTEGER NOT NULL DEFAULT 420,
    late_after_min           INTEGER NOT NULL DEFAULT 10,
    early_after_min          INTEGER NOT NULL DEFAULT 10,

    no_clock_in_enabled      INTEGER NOT NULL DEFAULT 1,
    no_clock_in_as           TEXT    NOT NULL DEFAULT 'Absent'
                                     CHECK (no_clock_in_as IN ('Late','Absent')),
    no_clock_in_min          INTEGER NOT NULL DEFAULT 0,
    no_clock_out_enabled     INTEGER NOT NULL DEFAULT 1,
    no_clock_out_as          TEXT    NOT NULL DEFAULT 'EarlyLeave'
                                     CHECK (no_clock_out_as IN ('EarlyLeave','Absent')),
    no_clock_out_min         INTEGER NOT NULL DEFAULT 0,

    late_to_absent_enabled   INTEGER NOT NULL DEFAULT 1,
    late_to_absent_min       INTEGER NOT NULL DEFAULT 240,
    early_to_absent_enabled  INTEGER NOT NULL DEFAULT 1,
    early_to_absent_min      INTEGER NOT NULL DEFAULT 240,
    half_day_after_min       INTEGER NOT NULL DEFAULT 120,
    min_full_day_min         INTEGER NOT NULL DEFAULT 350,

    ot_after_shift_enabled   INTEGER NOT NULL DEFAULT 1,
    ot_after_shift_min       INTEGER NOT NULL DEFAULT 30,
    ot_before_shift_enabled  INTEGER NOT NULL DEFAULT 0,
    ot_before_shift_min      INTEGER NOT NULL DEFAULT 30,
    ot_max_daily_min         INTEGER NOT NULL DEFAULT 240,

    dedupe_secs              INTEGER NOT NULL DEFAULT 60,
    lone_punch_half_day      INTEGER NOT NULL DEFAULT 1,

    -- --- Statistic items ---
    sym_normal               TEXT NOT NULL DEFAULT 'P',
    sym_late                 TEXT NOT NULL DEFAULT 'L',
    sym_early                TEXT NOT NULL DEFAULT 'E',
    sym_absent               TEXT NOT NULL DEFAULT 'A',
    sym_ot                   TEXT NOT NULL DEFAULT 'O',
    sym_leave                TEXT NOT NULL DEFAULT 'V',
    sym_holiday              TEXT NOT NULL DEFAULT 'H',
    sym_half_day             TEXT NOT NULL DEFAULT 'HD',
    sym_missing              TEXT NOT NULL DEFAULT '?',

    min_unit                 REAL NOT NULL DEFAULT 0.5 CHECK (min_unit > 0),
    min_unit_basis           TEXT NOT NULL DEFAULT 'workday'
                                  CHECK (min_unit_basis IN ('workday','hours')),
    rounding                 TEXT NOT NULL DEFAULT 'off'
                                  CHECK (rounding IN ('down','off','up')),
    acc_by_times             INTEGER NOT NULL DEFAULT 0,
    round_at_acc             INTEGER NOT NULL DEFAULT 1,
    group_by_periods         INTEGER NOT NULL DEFAULT 0,

    -- --- Weekend set ---
    weekend_sun              INTEGER NOT NULL DEFAULT 0,
    weekend_mon              INTEGER NOT NULL DEFAULT 0,
    weekend_tue              INTEGER NOT NULL DEFAULT 0,
    weekend_wed              INTEGER NOT NULL DEFAULT 0,
    weekend_thu              INTEGER NOT NULL DEFAULT 0,
    weekend_fri              INTEGER NOT NULL DEFAULT 0,
    weekend_sat              INTEGER NOT NULL DEFAULT 1,
    weekend_as_ot            INTEGER NOT NULL DEFAULT 1,
    weekend_symbol           TEXT NOT NULL DEFAULT 'W',
    weekend_colour           TEXT NOT NULL DEFAULT '#94A3B8',

    updated_at               TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- Every column carries its default, so the single row needs no values.
INSERT INTO attendance_rules (id) VALUES (1);

-- ---------------------------------------------------------------------------
-- 8. Report delivery
-- ---------------------------------------------------------------------------

CREATE TABLE report_recipients (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    email      TEXT NOT NULL UNIQUE,
    role       TEXT NOT NULL DEFAULT '',
    -- NULL means the whole school; otherwise this official only receives
    -- figures for their own department.
    dept_id    INTEGER REFERENCES departments(id) ON DELETE SET NULL,
    -- JSON array of report keys this person should receive, e.g.
    -- ["general","dept_stat"]. Empty array means they are on the book but are
    -- not on any automatic list.
    reports    TEXT NOT NULL DEFAULT '[]',
    active     INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

CREATE TABLE report_email_log (
    id         INTEGER PRIMARY KEY,
    ts         TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    email      TEXT NOT NULL,
    report_key TEXT NOT NULL,
    from_date  TEXT NOT NULL,
    to_date    TEXT NOT NULL,
    ok         INTEGER NOT NULL DEFAULT 1,
    detail     TEXT
);
CREATE INDEX idx_report_mail_ts ON report_email_log(ts);

-- ---------------------------------------------------------------------------
-- 9. Device data: fingerprints, photos, access control
-- ---------------------------------------------------------------------------

-- Templates are the device's own binary blobs, base64-encoded so the file
-- stays readable and portable. They are keyed on enrolment number rather than
-- member id so a backup taken before a member row exists can still be restored.
CREATE TABLE member_fingerprints (
    id           INTEGER PRIMARY KEY,
    enroll_no    INTEGER NOT NULL,
    finger_index INTEGER NOT NULL CHECK (finger_index BETWEEN 0 AND 9),
    template     TEXT NOT NULL,
    size         INTEGER NOT NULL DEFAULT 0,
    valid        INTEGER NOT NULL DEFAULT 1,
    device_serial TEXT NOT NULL DEFAULT '',
    updated_at   TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    UNIQUE (enroll_no, finger_index)
);
CREATE INDEX idx_fp_enroll ON member_fingerprints(enroll_no);

-- The image itself stays on disk; the database holds the path. A year of
-- capture photos is gigabytes, and that does not belong inside a file the
-- office copies onto a memory stick as a backup.
CREATE TABLE attendance_photos (
    id            INTEGER PRIMARY KEY,
    enroll_no     INTEGER NOT NULL,
    punch_time    TEXT NOT NULL,
    file_path     TEXT NOT NULL,
    bytes         INTEGER NOT NULL DEFAULT 0,
    device_serial TEXT NOT NULL DEFAULT '',
    downloaded_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    UNIQUE (enroll_no, punch_time)
);
CREATE INDEX idx_photos_time ON attendance_photos(punch_time);

-- Access control. A time zone is a week of open/closed intervals the relay
-- obeys; a group binds up to three of them together. Both are device concepts
-- with fixed numbering, so the number is the key.
CREATE TABLE device_timezones (
    id         INTEGER PRIMARY KEY,
    tz_no      INTEGER NOT NULL UNIQUE CHECK (tz_no BETWEEN 1 AND 50),
    name       TEXT NOT NULL,
    -- JSON: {"sun":[["00:00","23:59"]], "mon":[...], ...}
    spec       TEXT NOT NULL DEFAULT '{}',
    updated_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

CREATE TABLE device_groups (
    id            INTEGER PRIMARY KEY,
    group_no      INTEGER NOT NULL UNIQUE CHECK (group_no BETWEEN 1 AND 99),
    name          TEXT NOT NULL,
    verify_style  INTEGER NOT NULL DEFAULT 0,
    tz1           INTEGER NOT NULL DEFAULT 1,
    tz2           INTEGER NOT NULL DEFAULT 0,
    tz3           INTEGER NOT NULL DEFAULT 0,
    holiday_valid INTEGER NOT NULL DEFAULT 1,
    updated_at    TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

INSERT INTO device_timezones (tz_no, name, spec) VALUES
    (1, 'Always open',
     '{"sun":[["00:00","23:59"]],"mon":[["00:00","23:59"]],"tue":[["00:00","23:59"]],
       "wed":[["00:00","23:59"]],"thu":[["00:00","23:59"]],"fri":[["00:00","23:59"]],
       "sat":[["00:00","23:59"]]}'),
    (2, 'School hours',
     '{"sun":[["06:00","19:00"]],"mon":[["06:00","19:00"]],"tue":[["06:00","19:00"]],
       "wed":[["06:00","19:00"]],"thu":[["06:00","19:00"]],"fri":[["06:00","19:00"]],
       "sat":[]}');

INSERT INTO device_groups (group_no, name, tz1) VALUES
    (1, 'All staff', 1),
    (2, 'School hours only', 2);

-- ---------------------------------------------------------------------------
-- 10. Transfer console history
-- ---------------------------------------------------------------------------

-- Every line the Data Transfer console prints is kept, so a failed sync can be
-- read back tomorrow instead of being lost when the window closes.
CREATE TABLE transfer_log (
    id            INTEGER PRIMARY KEY,
    ts            TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    device_serial TEXT NOT NULL DEFAULT '',
    job           TEXT NOT NULL,
    level         TEXT NOT NULL DEFAULT 'info' CHECK (level IN ('info','ok','warn','error')),
    message       TEXT NOT NULL
);
CREATE INDEX idx_transfer_ts ON transfer_log(ts);
