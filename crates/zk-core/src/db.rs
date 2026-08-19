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
        // execute_batch runs inside an implicit transaction per statement; wrap
        // the whole migration so a failure part-way leaves nothing behind.
        conn.execute_batch(&format!("BEGIN; {sql} COMMIT;")).map_err(|e| {
            Error::Invalid(format!("migration {name} failed: {e}"))
        })?;
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

        let shifts: i64 = conn.query_row("SELECT count(*) FROM shifts", [], |r| r.get(0)).unwrap();
        assert_eq!(shifts, 6);

        // Every timetable must define all seven weekdays or scheduling silently
        // has holes.
        let bad: i64 = conn
            .query_row(
                "SELECT count(*) FROM (
                     SELECT timetable_id FROM timetable_days
                     GROUP BY timetable_id HAVING count(*) <> 7)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(bad, 0, "every timetable must cover all 7 weekdays");

        // Saturday off for the two Sun-Fri timetables.
        for tt in [1, 2] {
            let sat: Option<i64> = conn
                .query_row(
                    "SELECT shift_id FROM timetable_days WHERE timetable_id=?1 AND weekday=6",
                    params![tt],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(sat.is_none(), "timetable {tt} should have Saturday off");
        }
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
