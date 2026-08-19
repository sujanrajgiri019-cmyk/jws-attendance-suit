//! SQLite storage: one local file, numbered migrations, no server.
//!
//! Migrations are embedded in the binary and applied in order using SQLite's
//! own `user_version` pragma as the version counter. That keeps upgrades atomic
//! and means a school PC that has been offline for months catches up correctly
//! the first time the new build starts.

use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// Migrations, applied in array order. Never reorder or edit a shipped entry —
/// add a new one. `user_version` is the index of the last applied migration.
static MIGRATIONS: &[(&str, &str)] = &[
    ("001_initial", include_str!("../migrations/001_initial.sql")),
    ("002_seed", include_str!("../migrations/002_seed.sql")),
    ("003_scheduling", include_str!("../migrations/003_scheduling.sql")),
    ("004_device_log", include_str!("../migrations/004_device_log.sql")),
];

/// Open (creating if needed) the attendance database and bring it up to date.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// An in-memory database, used by tests and by the UI's demo mode.
pub fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL keeps the UI responsive while the push listener writes punches, and
    // survives the power cuts that a Kathmandu school PC will see.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

/// Apply any migrations the file has not seen yet.
pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    let current = current as usize;

    if current > MIGRATIONS.len() {
        return Err(Error::Invalid(format!(
            "database is at version {current} but this build only knows {}. \
             It was written by a newer version of JWS Attendance — update the app \
             rather than downgrading the database.",
            MIGRATIONS.len()
        )));
    }

    for (i, (name, sql)) in MIGRATIONS.iter().enumerate().skip(current) {
        tracing::info!("applying migration {name}");

        // Restructuring a table in SQLite means building a new one, copying the
        // rows across and dropping the original. With foreign keys enforced,
        // the drop trips over rows that legitimately still point at the table
        // being replaced. SQLite's own documented recipe is to switch
        // enforcement off for the rebuild and verify afterwards — and the
        // pragma is silently ignored inside a transaction, so it has to be set
        // out here rather than at the top of the .sql file.
        conn.pragma_update(None, "foreign_keys", "OFF")?;

        let applied = conn
            .execute_batch(&format!("BEGIN; {sql} COMMIT;"))
            .map_err(|e| Error::Invalid(format!("migration {name} failed: {e}")));

        // Whatever happened, enforcement goes back on before returning.
        let restored = conn.pragma_update(None, "foreign_keys", "ON");
        applied?;
        restored?;

        // A migration that leaves a dangling reference has corrupted the file
        // quietly. Refuse to record it as applied.
        let orphans: i64 = conn
            .prepare("PRAGMA foreign_key_check")?
            .query_map([], |r| r.get::<_, String>(0))?
            .count() as i64;
        if orphans > 0 {
            return Err(Error::Invalid(format!(
                "migration {name} left {orphans} row(s) pointing at records that \
                 no longer exist. The database has not been marked as upgraded; \
                 restore the most recent backup and report this."
            )));
        }

        conn.pragma_update(None, "user_version", (i + 1) as i64)?;
    }
    Ok(())
}

/// Current schema version.
pub fn version(conn: &Connection) -> Result<usize> {
    let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    Ok(v as usize)
}

// ---------------------------------------------------------------------------
// Small helpers used across the command layer
// ---------------------------------------------------------------------------

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_rule(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM rules WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

pub fn set_rule(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO rules(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

/// Rule value as an integer, falling back when unset or unparseable.
pub fn rule_i64(conn: &Connection, key: &str, default: i64) -> i64 {
    get_rule(conn, key).ok().flatten().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Rule value as a boolean ("1"/"true"/"yes").
pub fn rule_bool(conn: &Connection, key: &str, default: bool) -> bool {
    match get_rule(conn, key).ok().flatten() {
        Some(v) => matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        None => default,
    }
}

pub fn audit(conn: &Connection, actor: &str, action: &str, detail: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO audit_log(actor, action, detail) VALUES(?1, ?2, ?3)",
        params![actor, action, detail],
    )?;
    Ok(())
}

pub fn log_sync(conn: &Connection, job: &str, device: &str, result: &str, ok: bool) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_log(job, device, result, ok) VALUES(?1, ?2, ?3, ?4)",
        params![job, device, result, ok as i64],
    )?;
    Ok(())
}

/// Record one inbound request from a terminal.
///
/// Deliberately never fails the caller: losing a log line must not cost a
/// punch. Errors are swallowed here rather than propagated into the socket
/// handler.
pub fn log_device_request(
    conn: &Connection,
    serial: &str,
    method: &str,
    endpoint: &str,
    table: &str,
    body_bytes: usize,
    records: usize,
    reply: &str,
) {
    let _ = conn.execute(
        "INSERT INTO device_requests
            (device_serial, method, endpoint, table_name, body_bytes, records, reply)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            serial,
            method,
            endpoint,
            table,
            body_bytes as i64,
            records as i64,
            // A reply is one short line; anything longer is a command payload
            // and is trimmed so the log stays readable.
            reply.chars().take(120).collect::<String>().trim().to_string()
        ],
    );
}

/// Insert raw punches, ignoring any that are already stored.
///
/// Returns `(accepted, duplicates)`. Terminals resend their whole buffer after
/// a network drop, so duplicates are the normal case, not an error.
pub fn insert_punches(
    conn: &mut Connection,
    serial: &str,
    source: &str,
    logs: &[crate::proto::AttLog],
) -> Result<(usize, usize)> {
    let tx = conn.transaction()?;
    let mut accepted = 0usize;
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO punches
                (device_serial, enroll_no, punch_time, punch_state, verify_mode, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for l in logs {
            // Enrolment numbers are numeric on the device; a non-numeric id
            // means a corrupt record, so skip rather than store a junk row.
            let Ok(enroll) = l.user_id.trim().parse::<i64>() else {
                continue;
            };
            accepted += stmt.execute(params![
                serial,
                enroll,
                l.timestamp(),
                l.punch as i64,
                l.verify as i64,
                source
            ])?;
        }
    }
    tx.commit()?;
    Ok((accepted, logs.len().saturating_sub(accepted)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::AttLog;

    fn log(user: &str, ts: &str, punch: u8) -> AttLog {
        let (d, t) = ts.split_once(' ').unwrap();
        let dp: Vec<u32> = d.split('-').map(|v| v.parse().unwrap()).collect();
        let tp: Vec<u32> = t.split(':').map(|v| v.parse().unwrap()).collect();
        AttLog {
            uid: 0,
            user_id: user.into(),
            verify: 1,
            punch,
            year: dp[0] as i32,
            month: dp[1],
            day: dp[2],
            hour: tp[0],
            minute: tp[1],
            second: tp[2],
        }
    }

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let conn = open_memory().unwrap();
        assert_eq!(version(&conn).unwrap(), MIGRATIONS.len());
        // Running again must be a no-op, not a duplicate-table error.
        migrate(&conn).unwrap();
        assert_eq!(version(&conn).unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn seed_data_is_present_and_consistent() {
        let conn = open_memory().unwrap();
        let depts: i64 =
            conn.query_row("SELECT count(*) FROM departments", [], |r| r.get(0)).unwrap();
        assert_eq!(depts, 8);

        // 003 swapped the two words: the six seeded blocks of duty are now
        // timetables, and the four weekly plans built from them are shifts.
        let timetables: i64 =
            conn.query_row("SELECT count(*) FROM timetables", [], |r| r.get(0)).unwrap();
        assert_eq!(timetables, 6, "the six blocks of duty must survive migration");

        let shifts: i64 = conn.query_row("SELECT count(*) FROM shifts", [], |r| r.get(0)).unwrap();
        assert_eq!(shifts, 4, "the four weekly plans must survive migration");

        // A weekday with no shift_items row is a rest day. The two Sunday-to-
        // Friday plans must therefore have six working days each, with the
        // missing one being Saturday.
        for shift in [1, 2] {
            let days: Vec<i64> = conn
                .prepare("SELECT day_index FROM shift_items WHERE shift_id=?1 ORDER BY day_index")
                .unwrap()
                .query_map(params![shift], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            assert_eq!(days, vec![0, 1, 2, 3, 4, 5], "shift {shift} should work Sun-Fri");
            assert!(!days.contains(&6), "shift {shift} should have Saturday off");
        }
    }

    #[test]
    fn migration_003_carries_every_row_across_the_rename() {
        // The rename of shifts <-> timetables rebuilds four tables. A row lost
        // here is a member with no schedule or a day of history detached from
        // its block of duty, and neither announces itself.
        let conn = open_memory().unwrap();

        // Nothing may be left pointing at a record that no longer exists.
        let orphans = conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .count();
        assert_eq!(orphans, 0, "migration left dangling references");

        // The scaffolding must be gone.
        for gone in ["_old_shifts", "_old_timetables", "_old_timetable_days"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![gone],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 0, "{gone} should have been dropped");
        }

        // Every shift_items row must resolve to a real timetable on both sides.
        let dangling: i64 = conn
            .query_row(
                "SELECT count(*) FROM shift_items si
                 LEFT JOIN shifts s     ON s.id  = si.shift_id
                 LEFT JOIN timetables t ON t.id  = si.timetable_id
                 WHERE s.id IS NULL OR t.id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(dangling, 0);

        // Departments kept their default plan under its new column name.
        let with_default: i64 = conn
            .query_row(
                "SELECT count(*) FROM departments WHERE default_shift_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(with_default > 0, "departments lost their default shift");

        // The rules row exists, exactly once, and cannot be duplicated.
        let rules: i64 =
            conn.query_row("SELECT count(*) FROM attendance_rules", [], |r| r.get(0)).unwrap();
        assert_eq!(rules, 1);
        assert!(
            conn.execute("INSERT INTO attendance_rules (id) VALUES (2)", []).is_err(),
            "attendance_rules must hold exactly one row"
        );
    }

    #[test]
    fn migration_003_preserves_existing_assignments_and_history() {
        // Build a database at version 2 — the shape a school PC already has —
        // then let 003 run against it and check the data came through.
        let conn = Connection::open_in_memory().unwrap();
        configure(&conn).unwrap();
        for (name, sql) in &MIGRATIONS[..2] {
            conn.execute_batch(&format!("BEGIN; {sql} COMMIT;"))
                .unwrap_or_else(|e| panic!("{name} failed: {e}"));
        }
        conn.pragma_update(None, "user_version", 2i64).unwrap();

        // A member on the Sunday-to-Friday plan, with a day of history against
        // the "regular" block of duty.
        conn.execute(
            "INSERT INTO members (id, enroll_no, full_name, dept_id, timetable_id, joined_on)
             VALUES (77, 4177, 'Test Teacher', 1, 1, '2025-01-15')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attendance (member_id, work_date, shift_id, in_time, out_time,
                                     worked_min, late_min, status)
             VALUES (77, '2026-08-18', 1, '09:02:00', '16:31:00', 409, 0, 'Present')",
            [],
        )
        .unwrap();

        migrate(&conn).unwrap();
        assert_eq!(version(&conn).unwrap(), MIGRATIONS.len());

        // The assignment became a schedule row, backdated to the joining date.
        let (shift_id, start, temp): (i64, String, i64) = conn
            .query_row(
                "SELECT shift_id, start_date, is_temporary FROM employee_schedules
                 WHERE member_id = 77",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(shift_id, 1, "member kept their weekly plan");
        assert_eq!(start, "2025-01-15", "schedule backdated to the joining date");
        assert_eq!(temp, 0);

        // The day of history kept its figures and now names a timetable.
        let (tt, worked, status): (i64, i64, String) = conn
            .query_row(
                "SELECT timetable_id, worked_min, status FROM attendance
                 WHERE member_id = 77 AND work_date = '2026-08-18'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(tt, 1);
        assert_eq!(worked, 409, "computed minutes must not be disturbed");
        assert_eq!(status, "Present");

        // The block of duty kept its clock times under the new column names.
        let (on, off): (String, String) = conn
            .query_row("SELECT on_duty, off_duty FROM timetables WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((on.as_str(), off.as_str()), ("09:00", "16:00"));

        // 001 had no in/out boundary, so 003 invents one at the midpoint of the
        // shift. For 09:00-16:00 that is 12:30 — lunchtime, which is exactly
        // where a stray midday scan should stop counting as the day's arrival.
        let (in_end, out_begin): (String, String) = conn
            .query_row("SELECT in_end, out_begin FROM timetables WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(in_end, "12:30");
        assert_eq!(out_begin, "12:30");

        // The night guard's 19:00-06:00 crosses midnight. Averaging the two
        // clock readings naively puts the boundary at half past noon, in the
        // middle of his sleep; it belongs at half past midnight.
        let night: String = conn
            .query_row("SELECT in_end FROM timetables WHERE name='Night Guard'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(night, "00:30", "overnight block split at the wrong end of the day");
    }

    #[test]
    fn the_school_device_is_seeded() {
        let conn = open_memory().unwrap();
        let (ip, serial): (String, String) = conn
            .query_row("SELECT ip, serial FROM devices WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(ip, "192.168.100.99");
        assert_eq!(serial, "GED7253800740");
    }


    #[test]
    fn device_requests_are_logged_and_pruned() {
        let conn = open_memory().unwrap();
        for i in 0..20 {
            log_device_request(&conn, "SN1", "POST", "/iclock/cdata", "ATTLOG", 100, i, "OK");
        }
        let n: i64 =
            conn.query_row("SELECT count(*) FROM device_requests", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 20);

        // A long reply is trimmed rather than stored whole.
        log_device_request(&conn, "SN1", "GET", "/iclock/getrequest", "", 0, 0, &"x".repeat(500));
        let reply: String = conn
            .query_row("SELECT reply FROM device_requests ORDER BY id DESC LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(reply.len() <= 120, "reply not trimmed: {} chars", reply.len());

        // Logging must never be able to fail a caller, even with odd input.
        log_device_request(&conn, "", "", "", "", 0, 0, "");
    }

    #[test]
    fn punch_insert_is_idempotent() {
        let mut conn = open_memory().unwrap();
        let batch = vec![
            log("41", "2026-08-18 09:01:23", 0),
            log("41", "2026-08-18 16:05:00", 1),
            log("12", "2026-08-18 08:55:10", 0),
        ];

        let (a, d) = insert_punches(&mut conn, "GED7253800740", "push", &batch).unwrap();
        assert_eq!((a, d), (3, 0));

        // The terminal resends the same buffer — nothing new must land.
        let (a2, d2) = insert_punches(&mut conn, "GED7253800740", "push", &batch).unwrap();
        assert_eq!(a2, 0, "re-import must not duplicate punches");
        assert_eq!(d2, 3);

        let total: i64 = conn.query_row("SELECT count(*) FROM punches", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 3);
    }

    #[test]
    fn same_punch_from_two_devices_is_kept_separately() {
        // Staff scanning out at one gate and in at another is real; the unique
        // key includes the serial so both survive.
        let mut conn = open_memory().unwrap();
        let l = vec![log("41", "2026-08-18 09:01:23", 0)];
        insert_punches(&mut conn, "DEV-A", "push", &l).unwrap();
        insert_punches(&mut conn, "DEV-B", "push", &l).unwrap();
        let total: i64 = conn.query_row("SELECT count(*) FROM punches", [], |r| r.get(0)).unwrap();
        assert_eq!(total, 2);
    }

    #[test]
    fn non_numeric_enrolment_is_skipped_not_stored() {
        let mut conn = open_memory().unwrap();
        let batch = vec![log("41", "2026-08-18 09:00:00", 0), log("ABC", "2026-08-18 09:01:00", 0)];
        let (a, _) = insert_punches(&mut conn, "X", "push", &batch).unwrap();
        assert_eq!(a, 1);
    }

    #[test]
    fn settings_and_rules_round_trip() {
        let conn = open_memory().unwrap();
        assert_eq!(get_setting(&conn, "school_name").unwrap().unwrap(), "Janapremi World School");
        set_setting(&conn, "school_name", "JWS").unwrap();
        assert_eq!(get_setting(&conn, "school_name").unwrap().unwrap(), "JWS");
        assert!(get_setting(&conn, "nope").unwrap().is_none());

        assert_eq!(rule_i64(&conn, "late_grace_min", 0), 10);
        assert_eq!(rule_i64(&conn, "missing", 42), 42);
        assert!(rule_bool(&conn, "require_both_punches", false));
        set_rule(&conn, "require_both_punches", "0").unwrap();
        assert!(!rule_bool(&conn, "require_both_punches", true));
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let conn = open_memory().unwrap();
        let r = conn.execute(
            "INSERT INTO attendance(member_id, work_date, status) VALUES(9999,'2026-08-18','Present')",
            [],
        );
        assert!(r.is_err(), "attendance for a non-existent member must be rejected");
    }

    #[test]
    fn deleting_a_member_removes_their_attendance() {
        let conn = open_memory().unwrap();
        conn.execute(
            "INSERT INTO members(id, enroll_no, full_name, dept_id) VALUES(1, 41, 'Test', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attendance(member_id, work_date, status) VALUES(1,'2026-08-18','Present')",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM members WHERE id=1", []).unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM attendance", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn enrolment_numbers_are_unique() {
        let conn = open_memory().unwrap();
        conn.execute("INSERT INTO members(enroll_no, full_name) VALUES(41,'A')", []).unwrap();
        assert!(conn.execute("INSERT INTO members(enroll_no, full_name) VALUES(41,'B')", []).is_err());
    }

    #[test]
    fn refuses_to_downgrade_a_newer_database() {
        let conn = open_memory().unwrap();
        conn.pragma_update(None, "user_version", 99i64).unwrap();
        let err = migrate(&conn).unwrap_err().to_string();
        assert!(err.contains("newer version"), "got: {err}");
    }

    #[test]
    fn opens_a_real_file_and_creates_parent_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nested").join("attendance.db");
        let conn = open(&path).unwrap();
        assert_eq!(version(&conn).unwrap(), MIGRATIONS.len());
        assert!(path.exists());
        drop(conn);
        // Reopening an existing file must not re-run migrations.
        let conn2 = open(&path).unwrap();
        assert_eq!(version(&conn2).unwrap(), MIGRATIONS.len());
    }
}
