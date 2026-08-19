-- 001_initial: the whole schema for JWS Attendance.
--
-- Times are stored as TEXT in ISO form ('YYYY-MM-DD' / 'HH:MM' /
-- 'YYYY-MM-DD HH:MM:SS') in local Nepal time. SQLite has no date type and the
-- app is single-timezone, so ISO text sorts and compares correctly and stays
-- readable when the office opens the file in a DB browser.

CREATE TABLE departments (
    id                   INTEGER PRIMARY KEY,
    name                 TEXT NOT NULL UNIQUE,
    code                 TEXT NOT NULL,
    head_member_id       INTEGER REFERENCES members(id) ON DELETE SET NULL,
    default_timetable_id INTEGER REFERENCES timetables(id) ON DELETE SET NULL,
    colour               TEXT NOT NULL DEFAULT '#F16522',
    in_reports           INTEGER NOT NULL DEFAULT 1,
    active               INTEGER NOT NULL DEFAULT 1,
    created_at           TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

CREATE TABLE members (
    id              INTEGER PRIMARY KEY,
    -- The number typed on the terminal keypad. This is the join key between
    -- the device and this database, so it is unique and required.
    enroll_no       INTEGER NOT NULL UNIQUE,
    staff_id        TEXT,
    full_name       TEXT NOT NULL,
    -- The K40 Pro screen truncates past 24 characters, so the name pushed to
    -- the device is stored separately from the official name.
    device_name     TEXT,
    dept_id         INTEGER REFERENCES departments(id) ON DELETE SET NULL,
    designation     TEXT,
    gender          TEXT,
    dob             TEXT,
    mobile          TEXT,
    email           TEXT,
    card_no         TEXT,
    privilege       INTEGER NOT NULL DEFAULT 0,   -- 0 user, 2 enroller, 6 manager, 14 super admin
    device_password TEXT,
    fp_count        INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'Active',  -- Active | Inactive | On Leave
    joined_on       TEXT,
    timetable_id    INTEGER REFERENCES timetables(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    updated_at      TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX idx_members_dept   ON members(dept_id);
CREATE INDEX idx_members_status ON members(status);

CREATE TABLE devices (
    id           INTEGER PRIMARY KEY,
    name         TEXT NOT NULL,
    machine_no   INTEGER NOT NULL DEFAULT 1,
    model        TEXT NOT NULL DEFAULT 'ZKTeco K40 Pro',
    serial       TEXT UNIQUE,
    mac          TEXT,
    ip           TEXT NOT NULL,
    port         INTEGER NOT NULL DEFAULT 4370,
    comm_key     INTEGER NOT NULL DEFAULT 0,
    mode         TEXT NOT NULL DEFAULT 'push',   -- push | pull
    location     TEXT,
    auto_connect INTEGER NOT NULL DEFAULT 1,
    active       INTEGER NOT NULL DEFAULT 1,
    last_seen    TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);

-- Raw punches exactly as the terminal reported them. Never edited, never
-- recomputed: this is the audit source that `attendance` is derived from, so
-- changing a rule can always be replayed against untouched history.
CREATE TABLE punches (
    id            INTEGER PRIMARY KEY,
    device_serial TEXT NOT NULL DEFAULT '',
    enroll_no     INTEGER NOT NULL,
    punch_time    TEXT NOT NULL,              -- 'YYYY-MM-DD HH:MM:SS'
    punch_state   INTEGER NOT NULL DEFAULT 0, -- 0 in, 1 out, 2/3 break, 4/5 ot
    verify_mode   INTEGER NOT NULL DEFAULT 1, -- 1 finger, 4 card, 15 face
    source        TEXT NOT NULL DEFAULT 'push', -- push | pull | manual
    imported_at   TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    -- The terminal resends its whole buffer after any network hiccup, so the
    -- same punch arrives many times. This is what makes re-import idempotent.
    UNIQUE (device_serial, enroll_no, punch_time)
);
CREATE INDEX idx_punches_enroll_time ON punches(enroll_no, punch_time);
CREATE INDEX idx_punches_time        ON punches(punch_time);

-- One computed row per member per day. Rebuilt from `punches` whenever rules
-- change, except where an administrator has overridden or locked it.
CREATE TABLE attendance (
    id          INTEGER PRIMARY KEY,
    member_id   INTEGER NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    work_date   TEXT NOT NULL,                -- 'YYYY-MM-DD'
    shift_id    INTEGER REFERENCES shifts(id) ON DELETE SET NULL,
    in_time     TEXT,                         -- 'HH:MM:SS'
    out_time    TEXT,
    worked_min  INTEGER NOT NULL DEFAULT 0,
    late_min    INTEGER NOT NULL DEFAULT 0,
    early_min   INTEGER NOT NULL DEFAULT 0,
    ot_min      INTEGER NOT NULL DEFAULT 0,
    status      TEXT NOT NULL,                -- Present|Late|HalfDay|Absent|Leave|Holiday|WeeklyOff|NoPunch
    remark      TEXT,
    -- 1 when an admin edited this row by hand; recompute must not clobber it.
    manual      INTEGER NOT NULL DEFAULT 0,
    -- 1 after month-end close; recompute must not touch it.
    locked      INTEGER NOT NULL DEFAULT 0,
    computed_at TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    UNIQUE (member_id, work_date)
);
CREATE INDEX idx_att_date   ON attendance(work_date);
CREATE INDEX idx_att_member ON attendance(member_id, work_date);
CREATE INDEX idx_att_status ON attendance(status);

CREATE TABLE shifts (
    id              INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    code            TEXT NOT NULL,
    start_time      TEXT NOT NULL,            -- 'HH:MM'
    end_time        TEXT NOT NULL,
    in_window_start TEXT NOT NULL DEFAULT '05:00',
    out_window_end  TEXT NOT NULL DEFAULT '23:00',
    late_grace      INTEGER NOT NULL DEFAULT 10,
    early_grace     INTEGER NOT NULL DEFAULT 10,
    break_min       INTEGER NOT NULL DEFAULT 0,
    min_full_day    INTEGER NOT NULL DEFAULT 390,  -- minutes
    half_day_after  INTEGER NOT NULL DEFAULT 120,  -- late by more than this = half day
    absent_after    INTEGER NOT NULL DEFAULT 240,  -- late by more than this = absent
    overnight       INTEGER NOT NULL DEFAULT 0,
    count_ot        INTEGER NOT NULL DEFAULT 1,
    min_ot_block    INTEGER NOT NULL DEFAULT 30,
    active          INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE timetables (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE,
    active INTEGER NOT NULL DEFAULT 1
);

-- weekday: 0 = Sunday .. 6 = Saturday. A NULL shift_id means a day off, which
-- is how Saturday is modelled for JWS.
CREATE TABLE timetable_days (
    timetable_id INTEGER NOT NULL REFERENCES timetables(id) ON DELETE CASCADE,
    weekday      INTEGER NOT NULL CHECK (weekday BETWEEN 0 AND 6),
    shift_id     INTEGER REFERENCES shifts(id) ON DELETE SET NULL,
    PRIMARY KEY (timetable_id, weekday)
);

CREATE TABLE holidays (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    from_date  TEXT NOT NULL,
    to_date    TEXT NOT NULL,
    applies_to TEXT NOT NULL DEFAULT 'all',
    paid       INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX idx_holidays_range ON holidays(from_date, to_date);

CREATE TABLE leaves (
    id         INTEGER PRIMARY KEY,
    member_id  INTEGER NOT NULL REFERENCES members(id) ON DELETE CASCADE,
    from_date  TEXT NOT NULL,
    to_date    TEXT NOT NULL,
    leave_type TEXT NOT NULL DEFAULT 'Casual',
    reason     TEXT,
    approved   INTEGER NOT NULL DEFAULT 0,
    approved_by TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now','localtime'))
);
CREATE INDEX idx_leaves_member ON leaves(member_id, from_date);

-- Free-form key/value so the rules screen can add toggles without a migration.
CREATE TABLE rules (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE audit_log (
    id     INTEGER PRIMARY KEY,
    ts     TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    actor  TEXT NOT NULL DEFAULT 'system',
    action TEXT NOT NULL,
    detail TEXT
);
CREATE INDEX idx_audit_ts ON audit_log(ts);

-- Commands waiting for a push-mode terminal to collect on its next poll.
CREATE TABLE device_commands (
    id            INTEGER PRIMARY KEY,
    device_serial TEXT NOT NULL,
    payload       TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    sent_at       TEXT,
    result        TEXT
);
CREATE INDEX idx_devcmd_pending ON device_commands(device_serial, sent_at);

CREATE TABLE sync_log (
    id         INTEGER PRIMARY KEY,
    ts         TEXT NOT NULL DEFAULT (datetime('now','localtime')),
    job        TEXT NOT NULL,
    device     TEXT,
    result     TEXT,
    ok         INTEGER NOT NULL DEFAULT 1
);
