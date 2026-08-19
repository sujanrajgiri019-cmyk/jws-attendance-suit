//! Business logic: everything the UI asks for, expressed as plain functions
//! over a `Connection`.
//!
//! Kept out of the Tauri layer deliberately — the command handlers in
//! `src-tauri` are one-line wrappers around these, so the logic that decides
//! what goes on a payroll sheet is testable without a window.

use crate::calendar;
use crate::db;
use crate::rules::{self, DayKind, DayResult, Policy, Shift, Status, Summary};
use crate::{Error, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Shapes handed to the frontend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: i64,
    pub enroll_no: i64,
    pub staff_id: Option<String>,
    pub full_name: String,
    pub device_name: Option<String>,
    pub dept_id: Option<i64>,
    pub dept_name: Option<String>,
    pub dept_code: Option<String>,
    pub dept_colour: Option<String>,
    pub designation: Option<String>,
    pub gender: Option<String>,
    pub dob: Option<String>,
    pub mobile: Option<String>,
    pub email: Option<String>,
    pub card_no: Option<String>,
    pub privilege: i64,
    pub fp_count: i64,
    pub status: String,
    pub joined_on: Option<String>,
    pub timetable_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemberInput {
    pub id: Option<i64>,
    pub enroll_no: i64,
    pub staff_id: Option<String>,
    pub full_name: String,
    pub device_name: Option<String>,
    pub dept_id: Option<i64>,
    pub designation: Option<String>,
    pub gender: Option<String>,
    pub dob: Option<String>,
    pub mobile: Option<String>,
    pub email: Option<String>,
    pub card_no: Option<String>,
    pub privilege: i64,
    pub device_password: Option<String>,
    pub status: String,
    pub joined_on: Option<String>,
    pub timetable_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Department {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub colour: String,
    pub head_member_id: Option<i64>,
    pub head_name: Option<String>,
    pub default_timetable_id: Option<i64>,
    pub timetable_name: Option<String>,
    pub member_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub id: i64,
    pub name: String,
    pub machine_no: i64,
    pub model: String,
    pub serial: Option<String>,
    pub mac: Option<String>,
    pub ip: String,
    pub port: i64,
    pub comm_key: i64,
    pub mode: String,
    pub location: Option<String>,
    pub auto_connect: bool,
    pub last_seen: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttendanceRow {
    pub member_id: i64,
    pub enroll_no: i64,
    pub full_name: String,
    pub dept_name: Option<String>,
    pub dept_colour: Option<String>,
    pub designation: Option<String>,
    pub work_date: String,
    pub work_date_bs: Option<String>,
    pub in_time: Option<String>,
    pub out_time: Option<String>,
    pub worked_min: i64,
    pub late_min: i64,
    pub early_min: i64,
    pub ot_min: i64,
    pub status: String,
    pub remark: Option<String>,
    pub manual: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DashboardStats {
    pub date: String,
    pub date_bs: Option<String>,
    pub total_staff: i64,
    pub present: i64,
    pub late: i64,
    pub absent: i64,
    pub not_in: i64,
    pub leave: i64,
    pub holiday_name: Option<String>,
    pub is_working_day: bool,
    pub rate: f64,
    pub month_rate: f64,
}

// ---------------------------------------------------------------------------
// Members
// ---------------------------------------------------------------------------

fn member_from_row(r: &Row) -> rusqlite::Result<Member> {
    Ok(Member {
        id: r.get("id")?,
        enroll_no: r.get("enroll_no")?,
        staff_id: r.get("staff_id")?,
        full_name: r.get("full_name")?,
        device_name: r.get("device_name")?,
        dept_id: r.get("dept_id")?,
        dept_name: r.get("dept_name")?,
        dept_code: r.get("dept_code")?,
        dept_colour: r.get("dept_colour")?,
        designation: r.get("designation")?,
        gender: r.get("gender")?,
        dob: r.get("dob")?,
        mobile: r.get("mobile")?,
        email: r.get("email")?,
        card_no: r.get("card_no")?,
        privilege: r.get("privilege")?,
        fp_count: r.get("fp_count")?,
        status: r.get("status")?,
        joined_on: r.get("joined_on")?,
        timetable_id: r.get("timetable_id")?,
    })
}

const MEMBER_SELECT: &str = "
    SELECT m.id, m.enroll_no, m.staff_id, m.full_name, m.device_name, m.dept_id,
           d.name AS dept_name, d.code AS dept_code, d.colour AS dept_colour,
           m.designation, m.gender, m.dob, m.mobile, m.email, m.card_no,
           m.privilege, m.fp_count, m.status, m.joined_on, m.timetable_id
    FROM members m LEFT JOIN departments d ON d.id = m.dept_id";

pub fn list_members(
    conn: &Connection,
    search: Option<&str>,
    dept_id: Option<i64>,
    status: Option<&str>,
) -> Result<Vec<Member>> {
    let mut sql = String::from(MEMBER_SELECT);
    let mut where_parts: Vec<String> = Vec::new();
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(q) = search.map(str::trim).filter(|s| !s.is_empty()) {
        where_parts.push(
            "(m.full_name LIKE ?  OR m.staff_id LIKE ?  OR m.card_no LIKE ? \
              OR CAST(m.enroll_no AS TEXT) LIKE ?)"
                .into(),
        );
        let like = format!("%{q}%");
        for _ in 0..4 {
            args.push(Box::new(like.clone()));
        }
    }
    if let Some(d) = dept_id {
        where_parts.push("m.dept_id = ?".into());
        args.push(Box::new(d));
    }
    if let Some(s) = status.filter(|s| !s.is_empty()) {
        where_parts.push("m.status = ?".into());
        args.push(Box::new(s.to_string()));
    }
    if !where_parts.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&where_parts.join(" AND "));
    }
    sql.push_str(" ORDER BY m.enroll_no");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter().map(|b| b.as_ref())), member_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get_member(conn: &Connection, id: i64) -> Result<Option<Member>> {
    let sql = format!("{MEMBER_SELECT} WHERE m.id = ?1");
    Ok(conn.query_row(&sql, params![id], member_from_row).optional()?)
}

/// Create or update a member.
///
/// Enrolment numbers are how the terminal identifies a person, so a clash is
/// rejected with a message the office can act on rather than a raw SQL error.
pub fn save_member(conn: &Connection, m: &MemberInput) -> Result<i64> {
    if m.full_name.trim().is_empty() {
        return Err(Error::Invalid("Full name is required.".into()));
    }
    if m.enroll_no <= 0 || m.enroll_no > 65535 {
        return Err(Error::Invalid(
            "Enrolment number must be between 1 and 65535 — that is the range the terminal accepts."
                .into(),
        ));
    }

    let clash: Option<i64> = conn
        .query_row(
            "SELECT id FROM members WHERE enroll_no = ?1 AND id IS NOT ?2",
            params![m.enroll_no, m.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(other) = clash {
        let name: String =
            conn.query_row("SELECT full_name FROM members WHERE id=?1", params![other], |r| {
                r.get(0)
            })?;
        return Err(Error::Invalid(format!(
            "Enrolment number {} is already used by {name}.",
            m.enroll_no
        )));
    }

    // The K40 Pro screen truncates at 24 characters; store what will actually
    // be shown so the office is not surprised by a cut-off name on the device.
    let device_name = m
        .device_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| m.full_name.chars().take(24).collect());

    match m.id {
        Some(id) => {
            conn.execute(
                "UPDATE members SET enroll_no=?1, staff_id=?2, full_name=?3, device_name=?4,
                    dept_id=?5, designation=?6, gender=?7, dob=?8, mobile=?9, email=?10,
                    card_no=?11, privilege=?12, device_password=?13, status=?14, joined_on=?15,
                    timetable_id=?16, updated_at=datetime('now','localtime')
                 WHERE id=?17",
                params![
                    m.enroll_no, m.staff_id, m.full_name, device_name, m.dept_id, m.designation,
                    m.gender, m.dob, m.mobile, m.email, m.card_no, m.privilege, m.device_password,
                    m.status, m.joined_on, m.timetable_id, id
                ],
            )?;
            db::audit(conn, "admin", "member.update", &format!("{} ({})", m.full_name, m.enroll_no))?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO members (enroll_no, staff_id, full_name, device_name, dept_id,
                    designation, gender, dob, mobile, email, card_no, privilege, device_password,
                    status, joined_on, timetable_id)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                params![
                    m.enroll_no, m.staff_id, m.full_name, device_name, m.dept_id, m.designation,
                    m.gender, m.dob, m.mobile, m.email, m.card_no, m.privilege, m.device_password,
                    m.status, m.joined_on, m.timetable_id
                ],
            )?;
            let id = conn.last_insert_rowid();
            db::audit(conn, "admin", "member.create", &format!("{} ({})", m.full_name, m.enroll_no))?;
            Ok(id)
        }
    }
}

pub fn delete_members(conn: &Connection, ids: &[i64]) -> Result<usize> {
    let mut n = 0;
    for id in ids {
        let name: Option<String> = conn
            .query_row("SELECT full_name FROM members WHERE id=?1", params![id], |r| r.get(0))
            .optional()?;
        n += conn.execute("DELETE FROM members WHERE id=?1", params![id])?;
        if let Some(name) = name {
            db::audit(conn, "admin", "member.delete", &name)?;
        }
    }
    Ok(n)
}

pub fn set_members_department(conn: &Connection, ids: &[i64], dept_id: i64) -> Result<usize> {
    let mut n = 0;
    for id in ids {
        n += conn.execute("UPDATE members SET dept_id=?1 WHERE id=?2", params![dept_id, id])?;
    }
    db::audit(conn, "admin", "member.bulk_department", &format!("{} members", ids.len()))?;
    Ok(n)
}

// ---------------------------------------------------------------------------
// Departments / devices
// ---------------------------------------------------------------------------

pub fn list_departments(conn: &Connection) -> Result<Vec<Department>> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.name, d.code, d.colour, d.head_member_id, h.full_name AS head_name,
                d.default_timetable_id, t.name AS timetable_name,
                (SELECT count(*) FROM members m WHERE m.dept_id = d.id) AS member_count
         FROM departments d
         LEFT JOIN members h ON h.id = d.head_member_id
         LEFT JOIN timetables t ON t.id = d.default_timetable_id
         WHERE d.active = 1
         ORDER BY d.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Department {
            id: r.get("id")?,
            name: r.get("name")?,
            code: r.get("code")?,
            colour: r.get("colour")?,
            head_member_id: r.get("head_member_id")?,
            head_name: r.get("head_name")?,
            default_timetable_id: r.get("default_timetable_id")?,
            timetable_name: r.get("timetable_name")?,
            member_count: r.get("member_count")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn save_department(
    conn: &Connection,
    id: Option<i64>,
    name: &str,
    code: &str,
    colour: &str,
    head: Option<i64>,
    timetable: Option<i64>,
) -> Result<i64> {
    if name.trim().is_empty() {
        return Err(Error::Invalid("Department name is required.".into()));
    }
    match id {
        Some(id) => {
            conn.execute(
                "UPDATE departments SET name=?1, code=?2, colour=?3, head_member_id=?4,
                    default_timetable_id=?5 WHERE id=?6",
                params![name, code, colour, head, timetable, id],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO departments(name, code, colour, head_member_id, default_timetable_id)
                 VALUES(?1,?2,?3,?4,?5)",
                params![name, code, colour, head, timetable],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// Delete a department, refusing while staff are still assigned to it.
pub fn delete_department(conn: &Connection, id: i64) -> Result<()> {
    let n: i64 =
        conn.query_row("SELECT count(*) FROM members WHERE dept_id=?1", params![id], |r| r.get(0))?;
    if n > 0 {
        return Err(Error::Invalid(format!(
            "This department still has {n} staff. Move them first, then delete it."
        )));
    }
    conn.execute("DELETE FROM departments WHERE id=?1", params![id])?;
    Ok(())
}

pub fn list_devices(conn: &Connection) -> Result<Vec<Device>> {
    let mut stmt = conn.prepare(
        "SELECT id,name,machine_no,model,serial,mac,ip,port,comm_key,mode,location,
                auto_connect,last_seen FROM devices WHERE active=1 ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Device {
            id: r.get("id")?,
            name: r.get("name")?,
            machine_no: r.get("machine_no")?,
            model: r.get("model")?,
            serial: r.get("serial")?,
            mac: r.get("mac")?,
            ip: r.get("ip")?,
            port: r.get("port")?,
            comm_key: r.get("comm_key")?,
            mode: r.get("mode")?,
            location: r.get("location")?,
            auto_connect: r.get::<_, i64>("auto_connect")? != 0,
            last_seen: r.get("last_seen")?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[allow(clippy::too_many_arguments)]
pub fn save_device(
    conn: &Connection,
    id: Option<i64>,
    name: &str,
    machine_no: i64,
    model: &str,
    ip: &str,
    port: i64,
    comm_key: i64,
    location: Option<&str>,
) -> Result<i64> {
    if name.trim().is_empty() || ip.trim().is_empty() {
        return Err(Error::Invalid("Device name and IP address are required.".into()));
    }
    match id {
        Some(id) => {
            conn.execute(
                "UPDATE devices SET name=?1,machine_no=?2,model=?3,ip=?4,port=?5,comm_key=?6,
                    location=?7 WHERE id=?8",
                params![name, machine_no, model, ip, port, comm_key, location, id],
            )?;
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO devices(name,machine_no,model,ip,port,comm_key,location)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![name, machine_no, model, ip, port, comm_key, location],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

pub fn touch_device(conn: &Connection, serial: &str) -> Result<()> {
    conn.execute(
        "UPDATE devices SET last_seen = datetime('now','localtime') WHERE serial = ?1",
        params![serial],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Recomputing attendance
// ---------------------------------------------------------------------------

fn weekday_of(iso: &str) -> Result<u32> {
    let p: Vec<i64> = iso.split('-').filter_map(|v| v.parse().ok()).collect();
    if p.len() != 3 {
        return Err(Error::Invalid(format!("'{iso}' is not a YYYY-MM-DD date")));
    }
    // Sakamoto's algorithm; 0 = Sunday.
    let (mut y, m, d) = (p[0], p[1], p[2]);
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 {
        y -= 1;
    }
    Ok(((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d) % 7) as u32)
}

fn date_range(from: &str, to: &str) -> Result<Vec<String>> {
    let parse = |s: &str| -> Result<(i32, u32, u32)> {
        let p: Vec<i64> = s.split('-').filter_map(|v| v.parse().ok()).collect();
        if p.len() != 3 {
            return Err(Error::Invalid(format!("'{s}' is not a YYYY-MM-DD date")));
        }
        Ok((p[0] as i32, p[1] as u32, p[2] as u32))
    };
    let (fy, fm, fd) = parse(from)?;
    let (ty, tm, td) = parse(to)?;
    let start = calendar::days_from_civil_pub(fy, fm, fd);
    let end = calendar::days_from_civil_pub(ty, tm, td);
    if end < start {
        return Err(Error::Invalid("The 'to' date is before the 'from' date.".into()));
    }
    if end - start > 3660 {
        return Err(Error::Invalid("That range is longer than ten years.".into()));
    }
    Ok((start..=end)
        .map(|d| {
            let (y, m, dd) = calendar::civil_from_days_pub(d);
            format!("{y:04}-{m:02}-{dd:02}")
        })
        .collect())
}

/// Rebuild `attendance` from raw punches for a date range.
///
/// Rows an administrator has edited by hand, or that are locked after a
/// month-end close, are left alone — recomputing must never silently discard a
/// correction the office made deliberately.
pub fn recompute(conn: &mut Connection, from: &str, to: &str) -> Result<usize> {
    let dates = date_range(from, to)?;
    let policy = Policy {
        dedupe_secs: db::rule_i64(conn, "dedupe_window_sec", 60) as i32,
        lone_punch_half_day: db::rule_bool(conn, "lone_punch_half_day", true),
        require_both_punches: db::rule_bool(conn, "require_both_punches", true),
        count_ot: db::rule_bool(conn, "count_ot", true),
    };

    // Load everything we need up front — per-day queries would make a month
    // recompute unbearably slow on a school office PC.
    let members: Vec<(i64, i64, Option<i64>, Option<i64>, String)> = {
        let mut s = conn.prepare(
            "SELECT m.id, m.enroll_no, m.timetable_id, d.default_timetable_id, m.status
             FROM members m LEFT JOIN departments d ON d.id = m.dept_id",
        )?;
        let r = s.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        r.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let holidays: Vec<(String, String)> = {
        let mut s = conn.prepare("SELECT from_date, to_date FROM holidays")?;
        let r = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        r.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let is_holiday = |d: &str| holidays.iter().any(|(f, t)| d >= f.as_str() && d <= t.as_str());

    let leaves: Vec<(i64, String, String)> = {
        let mut s =
            conn.prepare("SELECT member_id, from_date, to_date FROM leaves WHERE approved = 1")?;
        let r = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        r.collect::<rusqlite::Result<Vec<_>>>()?
    };

    // All punches in range, grouped by (enroll, date).
    let mut punches: std::collections::HashMap<(i64, String), Vec<rules::Punch>> =
        std::collections::HashMap::new();
    {
        let mut s = conn.prepare(
            "SELECT enroll_no, punch_time, punch_state FROM punches
             WHERE date(punch_time) BETWEEN ?1 AND ?2",
        )?;
        let rows = s.query_map(params![from, to], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
        })?;
        for row in rows {
            let (enroll, ts, state) = row?;
            let Some((d, t)) = ts.split_once(' ') else { continue };
            let tp: Vec<i32> = t.split(':').filter_map(|v| v.parse().ok()).collect();
            if tp.len() < 2 {
                continue;
            }
            punches.entry((enroll, d.to_string())).or_default().push(rules::Punch {
                minute: tp[0] * 60 + tp[1],
                second: *tp.get(2).unwrap_or(&0),
                state: state as u8,
            });
        }
    }

    let mut written = 0usize;
    let tx = conn.transaction()?;
    {
        let mut up = tx.prepare(
            "INSERT INTO attendance (member_id, work_date, in_time, out_time, worked_min,
                 late_min, early_min, ot_min, status, remark, computed_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10, datetime('now','localtime'))
             ON CONFLICT(member_id, work_date) DO UPDATE SET
                 in_time=excluded.in_time, out_time=excluded.out_time,
                 worked_min=excluded.worked_min, late_min=excluded.late_min,
                 early_min=excluded.early_min, ot_min=excluded.ot_min,
                 status=excluded.status, remark=excluded.remark,
                 computed_at=excluded.computed_at
             WHERE attendance.manual = 0 AND attendance.locked = 0",
        )?;

        // Cache plans per (timetable, weekday) — 4 timetables x 7 days at most.
        let mut plan_cache: std::collections::HashMap<(i64, u32), Option<Shift>> =
            std::collections::HashMap::new();

        for date in &dates {
            let weekday = weekday_of(date)?;
            let holiday = is_holiday(date);

            for (mid, enroll, tt, dept_tt, status) in &members {
                let timetable = tt.or(*dept_tt);

                let shift = match timetable {
                    Some(t) => match plan_cache.entry((t, weekday)) {
                        std::collections::hash_map::Entry::Occupied(e) => *e.get(),
                        std::collections::hash_map::Entry::Vacant(e) => {
                            // Cannot call plan_for on `tx` borrow inside; query directly.
                            let p = plan_for_tx(&e.key().0, weekday, &tx)?;
                            *e.insert(p)
                        }
                    },
                    None => None,
                };

                let on_leave = status == "On Leave"
                    || leaves.iter().any(|(m, f, t)| {
                        m == mid && date.as_str() >= f.as_str() && date.as_str() <= t.as_str()
                    });

                let kind = if holiday {
                    DayKind::Holiday
                } else if on_leave {
                    DayKind::ApprovedLeave
                } else {
                    match shift {
                        Some(s) => DayKind::Working(s),
                        None => DayKind::Off,
                    }
                };

                let empty = Vec::new();
                let p = punches.get(&(*enroll, date.clone())).unwrap_or(&empty);
                let r: DayResult = rules::compute_day(p, kind, &policy);

                written += up.execute(params![
                    mid,
                    date,
                    r.in_time(),
                    r.out_time(),
                    r.worked_min as i64,
                    r.late_min as i64,
                    r.early_min as i64,
                    r.ot_min as i64,
                    r.status.as_str(),
                    r.remark,
                ])?;
            }
        }
    }
    tx.commit()?;
    Ok(written)
}

/// Look up the shift that applies to a timetable on a weekday.
fn plan_for_tx(tt: &i64, weekday: u32, tx: &rusqlite::Transaction) -> Result<Option<Shift>> {
    let row: Option<(String, String, i64, i64, i64, i64, i64, i64, i64, i64)> = tx
        .query_row(
            "SELECT s.start_time, s.end_time, s.late_grace, s.early_grace, s.break_min,
                    s.min_full_day, s.half_day_after, s.absent_after, s.count_ot, s.min_ot_block
             FROM timetable_days td JOIN shifts s ON s.id = td.shift_id
             WHERE td.timetable_id = ?1 AND td.weekday = ?2",
            params![tt, weekday],
            |r| {
                Ok((
                    r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
                    r.get(7)?, r.get(8)?, r.get(9)?,
                ))
            },
        )
        .optional()?;

    let Some((start, end, lg, eg, br, mfd, hda, aa, ot, mob)) = row else {
        return Ok(None);
    };
    let base = Shift::from_times(&start, &end)
        .ok_or_else(|| Error::Invalid(format!("shift has invalid times {start}-{end}")))?;
    Ok(Some(Shift {
        late_grace: lg as i32,
        early_grace: eg as i32,
        break_min: br as i32,
        min_full_day: mfd as i32,
        half_day_after: hda as i32,
        absent_after: aa as i32,
        count_ot: ot != 0,
        min_ot_block: mob as i32,
        ..base
    }))
}

// ---------------------------------------------------------------------------
// Reading attendance
// ---------------------------------------------------------------------------

pub fn attendance_range(
    conn: &Connection,
    from: &str,
    to: &str,
    dept_id: Option<i64>,
    member_id: Option<i64>,
    with_bs: bool,
) -> Result<Vec<AttendanceRow>> {
    let mut sql = String::from(
        "SELECT a.member_id, m.enroll_no, m.full_name, d.name AS dept_name, d.colour AS dept_colour,
                m.designation, a.work_date, a.in_time, a.out_time, a.worked_min, a.late_min,
                a.early_min, a.ot_min, a.status, a.remark, a.manual, a.locked
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN departments d ON d.id = m.dept_id
         WHERE a.work_date BETWEEN ?1 AND ?2",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(from.to_string()), Box::new(to.to_string())];
    if let Some(d) = dept_id {
        sql.push_str(" AND m.dept_id = ?");
        args.push(Box::new(d));
    }
    if let Some(m) = member_id {
        sql.push_str(" AND a.member_id = ?");
        args.push(Box::new(m));
    }
    sql.push_str(" ORDER BY a.work_date, m.enroll_no");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter().map(|b| b.as_ref())), |r| {
        let work_date: String = r.get("work_date")?;
        Ok(AttendanceRow {
            member_id: r.get("member_id")?,
            enroll_no: r.get("enroll_no")?,
            full_name: r.get("full_name")?,
            dept_name: r.get("dept_name")?,
            dept_colour: r.get("dept_colour")?,
            designation: r.get("designation")?,
            work_date_bs: None,
            work_date,
            in_time: r.get("in_time")?,
            out_time: r.get("out_time")?,
            worked_min: r.get("worked_min")?,
            late_min: r.get("late_min")?,
            early_min: r.get("early_min")?,
            ot_min: r.get("ot_min")?,
            status: r.get("status")?,
            remark: r.get("remark")?,
            manual: r.get::<_, i64>("manual")? != 0,
            locked: r.get::<_, i64>("locked")? != 0,
        })
    })?;

    let mut out = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if with_bs {
        for r in &mut out {
            r.work_date_bs = calendar::iso_to_bs(&r.work_date).ok().map(|b| b.pretty());
        }
    }
    Ok(out)
}

/// Edit one attendance row by hand and mark it so recompute leaves it alone.
pub fn override_attendance(
    conn: &Connection,
    member_id: i64,
    work_date: &str,
    status: &str,
    in_time: Option<&str>,
    out_time: Option<&str>,
    remark: Option<&str>,
) -> Result<()> {
    let locked: Option<i64> = conn
        .query_row(
            "SELECT locked FROM attendance WHERE member_id=?1 AND work_date=?2",
            params![member_id, work_date],
            |r| r.get(0),
        )
        .optional()?;
    if locked == Some(1) {
        return Err(Error::Invalid(
            "That day is locked because the month has been closed. Reopen the month to edit it."
                .into(),
        ));
    }

    conn.execute(
        "INSERT INTO attendance(member_id, work_date, status, in_time, out_time, remark, manual)
         VALUES(?1,?2,?3,?4,?5,?6,1)
         ON CONFLICT(member_id, work_date) DO UPDATE SET
             status=excluded.status, in_time=excluded.in_time, out_time=excluded.out_time,
             remark=excluded.remark, manual=1",
        params![member_id, work_date, status, in_time, out_time, remark],
    )?;
    db::audit(
        conn,
        "admin",
        "attendance.override",
        &format!("member {member_id} on {work_date} set to {status}"),
    )?;
    Ok(())
}

pub fn summary_for(
    conn: &Connection,
    from: &str,
    to: &str,
    member_id: i64,
) -> Result<Summary> {
    let rows = attendance_range(conn, from, to, None, Some(member_id), false)?;
    Ok(rules::summarise(
        &rows
            .iter()
            .map(|r| DayResult {
                status: status_from_str(&r.status),
                in_min: None,
                out_min: None,
                worked_min: r.worked_min as i32,
                late_min: r.late_min as i32,
                early_min: r.early_min as i32,
                ot_min: r.ot_min as i32,
                remark: None,
            })
            .collect::<Vec<_>>(),
    ))
}

pub fn status_from_str(s: &str) -> Status {
    match s {
        "Present" => Status::Present,
        "Late" => Status::Late,
        "HalfDay" => Status::HalfDay,
        "Leave" => Status::Leave,
        "Holiday" => Status::Holiday,
        "WeeklyOff" => Status::WeeklyOff,
        "MissingPunch" => Status::MissingPunch,
        _ => Status::Absent,
    }
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

pub fn dashboard(conn: &Connection, date: &str) -> Result<DashboardStats> {
    let total_staff: i64 = conn.query_row(
        "SELECT count(*) FROM members WHERE status <> 'Inactive'",
        [],
        |r| r.get(0),
    )?;

    let mut s = DashboardStats {
        date: date.to_string(),
        date_bs: calendar::iso_to_bs(date).ok().map(|b| b.pretty()),
        total_staff,
        ..Default::default()
    };

    s.holiday_name = conn
        .query_row(
            "SELECT name FROM holidays WHERE ?1 BETWEEN from_date AND to_date LIMIT 1",
            params![date],
            |r| r.get(0),
        )
        .optional()?;

    let mut stmt = conn.prepare(
        "SELECT status, count(*) FROM attendance WHERE work_date = ?1 GROUP BY status",
    )?;
    let rows = stmt.query_map(params![date], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;

    let mut counted = 0i64;
    let mut weekly_off = 0i64;
    for row in rows {
        let (st, n) = row?;
        counted += n;
        match st.as_str() {
            "Present" => s.present = n,
            "Late" => s.late = n,
            "HalfDay" => s.late += n,
            "Absent" => s.absent = n,
            "Leave" => s.leave = n,
            "MissingPunch" => s.not_in = n,
            "WeeklyOff" => weekly_off = n,
            _ => {}
        }
    }

    s.is_working_day = s.holiday_name.is_none() && weekly_off < total_staff.max(1);

    // Staff with no computed row yet simply have not been processed; showing
    // them as absent before the day is over would be wrong.
    if counted < total_staff {
        s.not_in += total_staff - counted;
    }

    let marked = s.present + s.late;
    s.rate = if total_staff > 0 { marked as f64 / total_staff as f64 * 100.0 } else { 0.0 };

    // Month to date.
    let month_start = format!("{}-01", &date[..7]);
    let (mp, mt): (i64, i64) = conn.query_row(
        "SELECT
            sum(CASE WHEN status IN ('Present','Late') THEN 1 ELSE 0 END),
            sum(CASE WHEN status NOT IN ('Holiday','WeeklyOff') THEN 1 ELSE 0 END)
         FROM attendance WHERE work_date BETWEEN ?1 AND ?2",
        params![month_start, date],
        |r| Ok((r.get::<_, Option<i64>>(0)?.unwrap_or(0), r.get::<_, Option<i64>>(1)?.unwrap_or(0))),
    )?;
    s.month_rate = if mt > 0 { mp as f64 / mt as f64 * 100.0 } else { 0.0 };

    Ok(s)
}

/// Staff with no check-in on a date — the list the absence mailer uses.
pub fn absentees(conn: &Connection, date: &str) -> Result<Vec<Member>> {
    let sql = format!(
        "{MEMBER_SELECT}
         JOIN attendance a ON a.member_id = m.id AND a.work_date = ?1
         WHERE a.status IN ('Absent','MissingPunch') AND m.status = 'Active'
         ORDER BY m.enroll_no"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![date], member_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::AttLog;

    fn seeded() -> Connection {
        db::open_memory().unwrap()
    }

    fn add_member(conn: &Connection, enroll: i64, name: &str, dept: i64) -> i64 {
        save_member(
            conn,
            &MemberInput {
                enroll_no: enroll,
                full_name: name.into(),
                dept_id: Some(dept),
                status: "Active".into(),
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn punch(conn: &mut Connection, enroll: i64, ts: &str) {
        let (d, t) = ts.split_once(' ').unwrap();
        let dp: Vec<u32> = d.split('-').map(|v| v.parse().unwrap()).collect();
        let tp: Vec<u32> = t.split(':').map(|v| v.parse().unwrap()).collect();
        let l = AttLog {
            uid: 0,
            user_id: enroll.to_string(),
            verify: 1,
            punch: 0,
            year: dp[0] as i32,
            month: dp[1],
            day: dp[2],
            hour: tp[0],
            minute: tp[1],
            second: *tp.get(2).unwrap_or(&0),
        };
        db::insert_punches(conn, "GED7253800740", "push", &[l]).unwrap();
    }

    #[test]
    fn weekday_calculation_is_correct() {
        // 2026-08-19 is a Wednesday, 2026-08-22 a Saturday, 2026-08-23 a Sunday.
        assert_eq!(weekday_of("2026-08-19").unwrap(), 3);
        assert_eq!(weekday_of("2026-08-22").unwrap(), 6);
        assert_eq!(weekday_of("2026-08-23").unwrap(), 0);
        assert!(weekday_of("nonsense").is_err());
    }

    #[test]
    fn date_range_walks_inclusive_and_rejects_backwards() {
        let r = date_range("2026-08-18", "2026-08-21").unwrap();
        assert_eq!(r, vec!["2026-08-18", "2026-08-19", "2026-08-20", "2026-08-21"]);
        assert_eq!(date_range("2026-08-18", "2026-08-18").unwrap().len(), 1);
        // Month and year boundaries.
        assert_eq!(date_range("2026-02-28", "2026-03-01").unwrap().len(), 2, "2026 is not a leap year");
        assert_eq!(date_range("2024-02-28", "2024-03-01").unwrap().len(), 3, "2024 is a leap year");
        assert!(date_range("2026-08-20", "2026-08-18").is_err());
    }

    #[test]
    fn saving_a_member_requires_a_name_and_valid_enrolment() {
        let conn = seeded();
        let bad = MemberInput { enroll_no: 1, full_name: "  ".into(), ..Default::default() };
        assert!(save_member(&conn, &bad).unwrap_err().to_string().contains("Full name"));

        let bad2 = MemberInput { enroll_no: 70000, full_name: "X".into(), ..Default::default() };
        assert!(save_member(&conn, &bad2).unwrap_err().to_string().contains("65535"));
    }

    #[test]
    fn duplicate_enrolment_is_reported_with_the_clashing_name() {
        let conn = seeded();
        add_member(&conn, 41, "Sarita Maharjan", 2);
        let dup = MemberInput {
            enroll_no: 41,
            full_name: "Someone Else".into(),
            status: "Active".into(),
            ..Default::default()
        };
        let err = save_member(&conn, &dup).unwrap_err().to_string();
        assert!(err.contains("already used by Sarita Maharjan"), "got: {err}");
    }

    #[test]
    fn editing_a_member_keeps_their_own_enrolment() {
        let conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        let same = MemberInput {
            id: Some(id),
            enroll_no: 41,
            full_name: "Sarita Maharjan".into(),
            status: "Active".into(),
            ..Default::default()
        };
        assert!(save_member(&conn, &same).is_ok(), "must not clash with itself");
        assert_eq!(get_member(&conn, id).unwrap().unwrap().full_name, "Sarita Maharjan");
    }

    #[test]
    fn long_names_are_truncated_for_the_device_only() {
        let conn = seeded();
        let id = add_member(&conn, 5, "Bishwoprakash Bhattarai Chaudhary Junior", 1);
        let m = get_member(&conn, id).unwrap().unwrap();
        assert_eq!(m.full_name.len(), 40, "official name kept in full");
        assert_eq!(m.device_name.unwrap().chars().count(), 24, "device name truncated to 24");
    }

    #[test]
    fn member_search_and_filters() {
        let conn = seeded();
        add_member(&conn, 1, "Sarita Maharjan", 2);
        add_member(&conn, 2, "Bikash Shrestha", 3);
        add_member(&conn, 3, "Ramesh Tamang", 2);

        assert_eq!(list_members(&conn, None, None, None).unwrap().len(), 3);
        assert_eq!(list_members(&conn, Some("sarita"), None, None).unwrap().len(), 1);
        assert_eq!(list_members(&conn, Some("SARITA"), None, None).unwrap().len(), 1);
        assert_eq!(list_members(&conn, None, Some(2), None).unwrap().len(), 2);
        assert_eq!(list_members(&conn, Some("2"), None, None).unwrap().len(), 1, "enrolment search");
        assert_eq!(list_members(&conn, Some("zzz"), None, None).unwrap().len(), 0);
        assert_eq!(list_members(&conn, None, None, Some("Active")).unwrap().len(), 3);

        // Department join must be populated for the UI colour chips.
        let m = &list_members(&conn, Some("Bikash"), None, None).unwrap()[0];
        assert_eq!(m.dept_name.as_deref(), Some("Lower Secondary"));
        assert_eq!(m.dept_code.as_deref(), Some("LSE"));
    }

    #[test]
    fn search_input_cannot_break_the_query() {
        let conn = seeded();
        add_member(&conn, 1, "Sarita", 2);
        // A quote and a wildcard must be treated as literal text.
        assert_eq!(list_members(&conn, Some("' OR 1=1 --"), None, None).unwrap().len(), 0);
        let all = list_members(&conn, Some("%"), None, None).unwrap();
        assert_eq!(all.len(), 1, "LIKE wildcard is not injected as a pattern break");
    }

    #[test]
    fn a_department_with_staff_cannot_be_deleted() {
        let conn = seeded();
        add_member(&conn, 1, "Sarita", 2);
        let err = delete_department(&conn, 2).unwrap_err().to_string();
        assert!(err.contains("still has 1 staff"), "got: {err}");
        // Empty one is fine.
        assert!(delete_department(&conn, 7).is_ok());
    }

    #[test]
    fn recompute_turns_punches_into_a_present_day() {
        let mut conn = seeded();
        let id = add_member(&conn, 41, "Sarita Maharjan", 2);
        // 2026-08-19 is a Wednesday — a working day on timetable 1.
        punch(&mut conn, 41, "2026-08-19 08:55:00");
        punch(&mut conn, 41, "2026-08-19 16:10:00");

        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();

        let rows = attendance_range(&conn, "2026-08-19", "2026-08-19", None, Some(id), true).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "Present");
        assert_eq!(rows[0].in_time.as_deref(), Some("08:55:00"));
        assert_eq!(rows[0].out_time.as_deref(), Some("16:10:00"));
        assert_eq!(rows[0].late_min, 0);
        assert_eq!(rows[0].work_date_bs.as_deref(), Some("3 Bhadra 2083"));
    }

    #[test]
    fn recompute_marks_saturday_as_weekly_off() {
        let mut conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        // 2026-08-22 is a Saturday.
        recompute(&mut conn, "2026-08-22", "2026-08-22").unwrap();
        let rows = attendance_range(&conn, "2026-08-22", "2026-08-22", None, Some(id), false).unwrap();
        assert_eq!(rows[0].status, "WeeklyOff");
    }

    #[test]
    fn recompute_marks_seeded_holidays() {
        let mut conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        // Janai Purnima is seeded on 2026-08-28.
        recompute(&mut conn, "2026-08-28", "2026-08-28").unwrap();
        let rows = attendance_range(&conn, "2026-08-28", "2026-08-28", None, Some(id), false).unwrap();
        assert_eq!(rows[0].status, "Holiday");
    }

    #[test]
    fn no_punches_on_a_working_day_is_absent() {
        let mut conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();
        let rows = attendance_range(&conn, "2026-08-19", "2026-08-19", None, Some(id), false).unwrap();
        assert_eq!(rows[0].status, "Absent");
    }

    #[test]
    fn recompute_is_idempotent() {
        let mut conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        punch(&mut conn, 41, "2026-08-19 09:30:00");
        punch(&mut conn, 41, "2026-08-19 16:10:00");

        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();
        let first = attendance_range(&conn, "2026-08-19", "2026-08-19", None, Some(id), false).unwrap();
        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();
        let second = attendance_range(&conn, "2026-08-19", "2026-08-19", None, Some(id), false).unwrap();

        assert_eq!(first.len(), second.len(), "no duplicate rows");
        assert_eq!(first[0].status, second[0].status);
        assert_eq!(first[0].late_min, second[0].late_min);
        assert_eq!(first[0].status, "Late");
        assert_eq!(first[0].late_min, 20);
    }

    #[test]
    fn a_manual_correction_survives_recompute() {
        // This is the one that protects the office's work: an admin fixes a
        // day, then a rule change triggers a rebuild. The fix must stand.
        let mut conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();
        assert_eq!(
            attendance_range(&conn, "2026-08-19", "2026-08-19", None, Some(id), false).unwrap()[0]
                .status,
            "Absent"
        );

        override_attendance(
            &conn, id, "2026-08-19", "Present",
            Some("09:00:00"), Some("16:00:00"), Some("Forgot card"),
        )
        .unwrap();

        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();

        let r = &attendance_range(&conn, "2026-08-19", "2026-08-19", None, Some(id), false).unwrap()[0];
        assert_eq!(r.status, "Present", "manual override must not be overwritten");
        assert_eq!(r.remark.as_deref(), Some("Forgot card"));
        assert!(r.manual);
    }

    #[test]
    fn a_locked_day_cannot_be_edited() {
        let conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        conn.execute(
            "INSERT INTO attendance(member_id, work_date, status, locked)
             VALUES(?1,'2026-08-19','Present',1)",
            params![id],
        )
        .unwrap();
        let err = override_attendance(&conn, id, "2026-08-19", "Absent", None, None, None)
            .unwrap_err()
            .to_string();
        assert!(err.contains("locked"), "got: {err}");
    }

    #[test]
    fn dashboard_counts_add_up() {
        let mut conn = seeded();
        let a = add_member(&conn, 1, "Present Person", 2);
        add_member(&conn, 2, "Late Person", 2);
        add_member(&conn, 3, "Absent Person", 2);

        punch(&mut conn, 1, "2026-08-19 08:50:00");
        punch(&mut conn, 1, "2026-08-19 16:10:00");
        punch(&mut conn, 2, "2026-08-19 09:40:00");
        punch(&mut conn, 2, "2026-08-19 16:10:00");

        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();
        let s = dashboard(&conn, "2026-08-19").unwrap();

        assert_eq!(s.total_staff, 3);
        assert_eq!(s.present, 1);
        assert_eq!(s.late, 1);
        assert_eq!(s.absent, 1);
        assert_eq!(s.date_bs.as_deref(), Some("3 Bhadra 2083"));
        assert!((s.rate - 66.666).abs() < 0.01, "got {}", s.rate);
        assert!(s.rate.is_finite());
        let _ = a;
    }

    #[test]
    fn dashboard_on_an_empty_database_does_not_divide_by_zero() {
        let conn = seeded();
        let s = dashboard(&conn, "2026-08-19").unwrap();
        assert_eq!(s.total_staff, 0);
        assert_eq!(s.rate, 0.0);
        assert!(s.rate.is_finite() && s.month_rate.is_finite());
    }

    #[test]
    fn dashboard_names_the_holiday() {
        let conn = seeded();
        let s = dashboard(&conn, "2026-08-28").unwrap();
        assert_eq!(s.holiday_name.as_deref(), Some("Janai Purnima"));
    }

    #[test]
    fn absentee_list_only_includes_active_staff_with_no_check_in() {
        let mut conn = seeded();
        add_member(&conn, 1, "Came In", 2);
        add_member(&conn, 2, "Did Not", 2);
        punch(&mut conn, 1, "2026-08-19 08:50:00");
        punch(&mut conn, 1, "2026-08-19 16:10:00");
        recompute(&mut conn, "2026-08-19", "2026-08-19").unwrap();

        let list = absentees(&conn, "2026-08-19").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].full_name, "Did Not");
    }

    #[test]
    fn summary_over_a_week_is_consistent() {
        let mut conn = seeded();
        let id = add_member(&conn, 41, "Sarita", 2);
        // Sun 16th to Sat 22nd August 2026.
        for d in ["16", "17", "18", "19", "20"] {
            punch(&mut conn, 41, &format!("2026-08-{d} 08:50:00"));
            punch(&mut conn, 41, &format!("2026-08-{d} 16:10:00"));
        }
        recompute(&mut conn, "2026-08-16", "2026-08-22").unwrap();

        let s = summary_for(&conn, "2026-08-16", "2026-08-22", id).unwrap();
        assert_eq!(s.weekly_off, 1, "Saturday");
        assert_eq!(s.present, 5);
        assert_eq!(s.absent, 1, "Friday 21st had no punches");
        assert_eq!(s.working_days, 6);
        assert!(s.rate() > 80.0 && s.rate() <= 100.0);
    }

    #[test]
    fn recompute_over_a_month_completes_for_a_full_staff_roll() {
        // Guards against the per-day-per-member query explosion.
        let mut conn = seeded();
        for i in 1..=45 {
            add_member(&conn, i, &format!("Staff {i}"), (i % 8) + 1);
        }
        let started = std::time::Instant::now();
        recompute(&mut conn, "2026-08-01", "2026-08-31").unwrap();
        let n: i64 = conn.query_row("SELECT count(*) FROM attendance", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 45 * 31);
        assert!(started.elapsed().as_secs() < 10, "month recompute took {:?}", started.elapsed());
    }
}
