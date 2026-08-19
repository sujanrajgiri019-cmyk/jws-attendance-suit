//! The eight reports the office prints.
//!
//! Every report returns the same shape — a list of typed columns and a list of
//! rows keyed by column — so the grid on screen, the CSV writer and the email
//! body all read one structure instead of eight. Adding a ninth report means
//! adding one function here and one line to [`run`]; no screen changes.
//!
//! The figures come from the `attendance` table, which is derived from raw
//! punches by [`crate::service::recompute`]. Reports never recompute: what is
//! printed is exactly what the office saw on screen, including any correction
//! made by hand.

use crate::ruleset::AttendanceRules;
use crate::{Error, Result};
use rusqlite::{params_from_iter, Connection, ToSql};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// How a column should be rendered and aligned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColKind {
    Text,
    Num,
    /// Minutes, printed as `7:15`.
    Mins,
    Time,
    Date,
    Status,
    Pct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub key: String,
    pub label: String,
    pub kind: ColKind,
}

fn col(key: &str, label: &str, kind: ColKind) -> Column {
    Column { key: key.into(), label: label.into(), kind }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportResult {
    pub key: String,
    pub title: String,
    pub subtitle: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Map<String, Value>>,
    /// Column totals, where a total means anything.
    pub totals: Map<String, Value>,
}

/// What the toolbar above the grid is set to.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filters {
    pub from: String,
    pub to: String,
    pub dept_id: Option<i64>,
    pub member_id: Option<i64>,
}

impl Filters {
    fn check(&self) -> Result<()> {
        if self.from.len() != 10 || self.to.len() != 10 {
            return Err(Error::Invalid("Choose both a start and an end date.".into()));
        }
        if self.to < self.from {
            return Err(Error::Invalid(format!(
                "The end date ({}) is before the start date ({}).",
                self.to, self.from
            )));
        }
        Ok(())
    }

    /// `AND` clauses and bound values shared by most reports.
    fn narrow(&self, alias_member: &str) -> (String, Vec<Box<dyn ToSql>>) {
        let mut sql = String::new();
        let mut args: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(d) = self.dept_id {
            sql.push_str(&format!(" AND {alias_member}.dept_id = ?"));
            args.push(Box::new(d));
        }
        if let Some(m) = self.member_id {
            sql.push_str(&format!(" AND {alias_member}.id = ?"));
            args.push(Box::new(m));
        }
        (sql, args)
    }
}

/// Every report this build can produce, in the order the selector lists them.
pub const REPORTS: &[(&str, &str)] = &[
    ("daily_stat", "Daily Attendance Statistic Report"),
    ("general", "Attendance General Report"),
    ("dept_stat", "Depart Attendance Statistic Report"),
    ("duty_timetable", "Staff's On-Duty/Off-Duty Timetable"),
    ("daily_shifts", "Daily Attendance Shifts"),
    ("daily_ot", "Daily Attendance OT Report"),
    ("ot_summary", "Summary of Overtime"),
    ("daily_overtime", "Daily Overtime"),
];

pub fn title_of(key: &str) -> &'static str {
    REPORTS.iter().find(|(k, _)| *k == key).map(|(_, t)| *t).unwrap_or("Report")
}

/// Build one report.
pub fn run(conn: &Connection, key: &str, f: &Filters) -> Result<ReportResult> {
    f.check()?;
    let rules = AttendanceRules::load(conn)?;
    let mut r = match key {
        "daily_stat" => daily_stat(conn, f, &rules),
        "general" => general(conn, f, &rules),
        "dept_stat" => dept_stat(conn, f),
        "duty_timetable" => duty_timetable(conn, f),
        "daily_shifts" => daily_shifts(conn, f),
        "daily_ot" => daily_ot(conn, f),
        "ot_summary" => ot_summary(conn, f),
        "daily_overtime" => daily_overtime(conn, f),
        other => {
            return Err(Error::Invalid(format!(
                "'{other}' is not a report this version knows how to produce."
            )))
        }
    }?;
    r.subtitle = format!("{} to {}", f.from, f.to);
    Ok(r)
}

// ---------------------------------------------------------------------------
// Shared query plumbing
// ---------------------------------------------------------------------------

/// Run a SELECT and turn each row into a map keyed by column name.
///
/// Reports are read-only and their shapes differ, so mapping generically here
/// beats eight near-identical structs that all end up serialised to JSON.
fn collect(
    conn: &Connection,
    sql: &str,
    args: Vec<Box<dyn ToSql>>,
) -> Result<Vec<Map<String, Value>>> {
    let mut stmt = conn.prepare(sql)?;
    let names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    let rows = stmt.query_map(params_from_iter(args.iter().map(|b| b.as_ref())), |row| {
        let mut m = Map::new();
        for (i, name) in names.iter().enumerate() {
            let v = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => Value::Null,
                rusqlite::types::ValueRef::Integer(n) => json!(n),
                rusqlite::types::ValueRef::Real(x) => json!(x),
                rusqlite::types::ValueRef::Text(t) => {
                    json!(String::from_utf8_lossy(t).to_string())
                }
                rusqlite::types::ValueRef::Blob(_) => Value::Null,
            };
            m.insert(name.clone(), v);
        }
        Ok(m)
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Sum the named numeric columns across every row.
fn total(rows: &[Map<String, Value>], keys: &[&str]) -> Map<String, Value> {
    let mut t = Map::new();
    for k in keys {
        let sum: f64 = rows.iter().filter_map(|r| r.get(*k)).filter_map(|v| v.as_f64()).sum();
        // Keep whole numbers whole, so a count of days does not print as 12.0.
        if sum.fract() == 0.0 && sum.abs() < 9e15 {
            t.insert((*k).to_string(), json!(sum as i64));
        } else {
            t.insert((*k).to_string(), json!((sum * 100.0).round() / 100.0));
        }
    }
    t
}

// ---------------------------------------------------------------------------
// 1. Daily Attendance Statistic Report
// ---------------------------------------------------------------------------

/// One line per member: how their period added up.
fn daily_stat(conn: &Connection, f: &Filters, rules: &AttendanceRules) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT m.enroll_no AS ac_no, m.full_name AS name,
                COALESCE(d.name,'Unassigned') AS dept,
                SUM(CASE WHEN a.status IN ('Present','Late','EarlyLeave') THEN 1 ELSE 0 END) AS present,
                SUM(CASE WHEN a.status = 'Late'         THEN 1 ELSE 0 END) AS late,
                SUM(CASE WHEN a.status = 'EarlyLeave'   THEN 1 ELSE 0 END) AS early,
                SUM(CASE WHEN a.status = 'HalfDay'      THEN 1 ELSE 0 END) AS half_day,
                SUM(CASE WHEN a.status = 'Absent'       THEN 1 ELSE 0 END) AS absent,
                SUM(CASE WHEN a.status = 'Leave'        THEN 1 ELSE 0 END) AS leave_days,
                SUM(CASE WHEN a.status = 'MissingPunch' THEN 1 ELSE 0 END) AS exceptions,
                SUM(a.late_min)     AS late_min,
                SUM(a.early_min)    AS early_min,
                SUM(a.worked_min)   AS worked_min,
                SUM(a.ot_min)       AS ot_min,
                SUM(a.workday_value) AS workdays,
                -- Days that count towards the rate: everything except the
                -- school's own holidays and the member's rest days.
                SUM(CASE WHEN a.status NOT IN ('Holiday','WeeklyOff') THEN 1 ELSE 0 END) AS working_days
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN departments d ON d.id = m.dept_id
         WHERE a.work_date BETWEEN ? AND ?{narrow}
         GROUP BY m.id
         ORDER BY d.name, m.enroll_no"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let mut rows = collect(conn, &sql, bound)?;
    for r in &mut rows {
        let working = r.get("working_days").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let credited = r.get("present").and_then(|v| v.as_f64()).unwrap_or(0.0)
            + r.get("half_day").and_then(|v| v.as_f64()).unwrap_or(0.0) * 0.5;
        // A period with no working days in it prints a dash, not NaN%.
        let rate = if working > 0.0 { (credited / working) * 100.0 } else { 0.0 };
        r.insert("rate".into(), json!((rate * 10.0).round() / 10.0));

        if rules.round_at_acc {
            let w = r.get("workdays").and_then(|v| v.as_f64()).unwrap_or(0.0);
            r.insert("workdays".into(), json!(rules.round_unit(w)));
        }
    }

    let totals = total(
        &rows,
        &[
            "present", "late", "early", "half_day", "absent", "leave_days", "exceptions",
            "late_min", "early_min", "worked_min", "ot_min", "workdays",
        ],
    );

    Ok(ReportResult {
        key: "daily_stat".into(),
        title: title_of("daily_stat").into(),
        subtitle: String::new(),
        columns: vec![
            col("ac_no", "AC No.", ColKind::Num),
            col("name", "Name", ColKind::Text),
            col("dept", "Department", ColKind::Text),
            col("present", "Present", ColKind::Num),
            col("late", "Late", ColKind::Num),
            col("early", "Early", ColKind::Num),
            col("half_day", "Half Day", ColKind::Num),
            col("absent", "Absent", ColKind::Num),
            col("leave_days", "Leave", ColKind::Num),
            col("exceptions", "Exceptions", ColKind::Num),
            col("late_min", "Late Time", ColKind::Mins),
            col("early_min", "Early Time", ColKind::Mins),
            col("worked_min", "Worked", ColKind::Mins),
            col("ot_min", "OT", ColKind::Mins),
            col("workdays", "Workdays", ColKind::Num),
            col("rate", "Attendance %", ColKind::Pct),
        ],
        rows,
        totals,
    })
}

// ---------------------------------------------------------------------------
// 2. Attendance General Report
// ---------------------------------------------------------------------------

/// Every member-day in the period, with its exceptions spelled out.
fn general(conn: &Connection, f: &Filters, rules: &AttendanceRules) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT m.enroll_no AS ac_no, m.full_name AS name,
                COALESCE(d.name,'Unassigned') AS dept,
                a.work_date AS work_date,
                a.in_time AS clock_in, a.out_time AS clock_out,
                a.status AS status,
                a.worked_min AS worked_min, a.late_min AS late_min,
                a.early_min AS early_min, a.ot_min AS ot_min,
                COALESCE(t.name,'') AS timetable,
                a.exception AS exception,
                a.manual AS manual, a.remark AS remark
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN departments d  ON d.id = m.dept_id
         LEFT JOIN timetables t   ON t.id = a.timetable_id
         WHERE a.work_date BETWEEN ? AND ?{narrow}
         ORDER BY a.work_date, d.name, m.enroll_no"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let mut rows = collect(conn, &sql, bound)?;
    for r in &mut rows {
        // Turn the exception code into something an office reads, and add the
        // report symbol the rules screen defines for the status.
        let ex = r.get("exception").and_then(|v| v.as_str()).unwrap_or("");
        let text = match ex {
            "MissingIn" => "No check-in",
            "MissingOut" => "No check-out",
            "Both" => "No scans",
            "ImplausibleSpan" => "Scans too far apart",
            _ => "",
        };
        r.insert("exception".into(), json!(text));

        let st = r.get("status").and_then(|v| v.as_str()).unwrap_or("Absent");
        r.insert("symbol".into(), json!(crate::rules::Status::parse(st).symbol(rules)));
        // A hand-corrected row must be visible as such on a printed sheet.
        let manual = r.get("manual").and_then(|v| v.as_i64()).unwrap_or(0);
        r.insert("manual".into(), json!(if manual == 1 { "Corrected" } else { "" }));
    }

    let totals = total(&rows, &["worked_min", "late_min", "early_min", "ot_min"]);

    Ok(ReportResult {
        key: "general".into(),
        title: title_of("general").into(),
        subtitle: String::new(),
        columns: vec![
            col("ac_no", "AC No.", ColKind::Num),
            col("name", "Name", ColKind::Text),
            col("dept", "Department", ColKind::Text),
            col("work_date", "Date", ColKind::Date),
            col("timetable", "Timetable", ColKind::Text),
            col("clock_in", "Clock In", ColKind::Time),
            col("clock_out", "Clock Out", ColKind::Time),
            col("status", "Status", ColKind::Status),
            col("symbol", "Sym", ColKind::Text),
            col("worked_min", "Worked", ColKind::Mins),
            col("late_min", "Late", ColKind::Mins),
            col("early_min", "Early", ColKind::Mins),
            col("ot_min", "OT", ColKind::Mins),
            col("exception", "Exception", ColKind::Text),
            col("manual", "Edited", ColKind::Text),
            col("remark", "Remark", ColKind::Text),
        ],
        rows,
        totals,
    })
}

// ---------------------------------------------------------------------------
// 3. Depart Attendance Statistic Report
// ---------------------------------------------------------------------------

/// One line per department per day.
fn dept_stat(conn: &Connection, f: &Filters) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT COALESCE(d.name,'Unassigned') AS dept,
                a.work_date AS work_date,
                COUNT(DISTINCT m.id) AS total_staff,
                SUM(CASE WHEN a.status IN ('Present','Late','EarlyLeave') THEN 1 ELSE 0 END) AS present,
                SUM(CASE WHEN a.status = 'Absent'  THEN 1 ELSE 0 END) AS absent,
                SUM(CASE WHEN a.status = 'Late'    THEN 1 ELSE 0 END) AS late,
                SUM(CASE WHEN a.status = 'HalfDay' THEN 1 ELSE 0 END) AS half_day,
                SUM(CASE WHEN a.status = 'Leave'   THEN 1 ELSE 0 END) AS leave_days,
                SUM(a.ot_min) AS ot_min,
                SUM(CASE WHEN a.status NOT IN ('Holiday','WeeklyOff') THEN 1 ELSE 0 END) AS working_days
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN departments d ON d.id = m.dept_id
         WHERE a.work_date BETWEEN ? AND ?{narrow}
         GROUP BY d.id, a.work_date
         ORDER BY a.work_date, d.name"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let mut rows = collect(conn, &sql, bound)?;
    for r in &mut rows {
        let working = r.get("working_days").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let credited = r.get("present").and_then(|v| v.as_f64()).unwrap_or(0.0)
            + r.get("half_day").and_then(|v| v.as_f64()).unwrap_or(0.0) * 0.5;
        let rate = if working > 0.0 { (credited / working) * 100.0 } else { 0.0 };
        r.insert("rate".into(), json!((rate * 10.0).round() / 10.0));
    }

    let totals = total(&rows, &["present", "absent", "late", "half_day", "leave_days", "ot_min"]);

    Ok(ReportResult {
        key: "dept_stat".into(),
        title: title_of("dept_stat").into(),
        subtitle: String::new(),
        columns: vec![
            col("dept", "Department", ColKind::Text),
            col("work_date", "Date", ColKind::Date),
            col("total_staff", "Total Staff", ColKind::Num),
            col("present", "Present", ColKind::Num),
            col("absent", "Absent", ColKind::Num),
            col("late", "Late", ColKind::Num),
            col("half_day", "Half Day", ColKind::Num),
            col("leave_days", "Leave", ColKind::Num),
            col("ot_min", "OT", ColKind::Mins),
            col("rate", "Attendance %", ColKind::Pct),
        ],
        rows,
        totals,
    })
}

// ---------------------------------------------------------------------------
// 4. Staff's On-Duty/Off-Duty Timetable
// ---------------------------------------------------------------------------

/// What each member was *scheduled* to work — the roster, not the outcome.
///
/// This joins through the whole three-tier model, so it doubles as the check
/// that a member's schedule actually resolves. A blank Expected On column here
/// means the office has left somebody unassigned.
fn duty_timetable(conn: &Connection, f: &Filters) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT m.enroll_no AS ac_no, m.full_name AS name,
                COALESCE(d.name,'Unassigned') AS dept,
                a.work_date AS work_date,
                COALESCE(s.name,'Not scheduled') AS shift_name,
                COALESCE(t.name,'')  AS timetable,
                COALESCE(t.on_duty,'')  AS on_duty,
                COALESCE(t.off_duty,'') AS off_duty,
                COALESCE(t.late_grace, 0)  AS late_grace,
                COALESCE(t.early_grace, 0) AS early_grace,
                a.in_time AS clock_in, a.out_time AS clock_out,
                a.status AS status
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN departments d ON d.id = m.dept_id
         LEFT JOIN shifts s      ON s.id = a.shift_id
         LEFT JOIN timetables t  ON t.id = a.timetable_id
         WHERE a.work_date BETWEEN ? AND ?{narrow}
         ORDER BY m.enroll_no, a.work_date"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let rows = collect(conn, &sql, bound)?;

    Ok(ReportResult {
        key: "duty_timetable".into(),
        title: title_of("duty_timetable").into(),
        subtitle: String::new(),
        columns: vec![
            col("ac_no", "AC No.", ColKind::Num),
            col("name", "Name", ColKind::Text),
            col("dept", "Department", ColKind::Text),
            col("work_date", "Date", ColKind::Date),
            col("shift_name", "Assigned Shift", ColKind::Text),
            col("timetable", "Timetable", ColKind::Text),
            col("on_duty", "Expected On", ColKind::Text),
            col("off_duty", "Expected Off", ColKind::Text),
            col("late_grace", "Late Grace", ColKind::Num),
            col("early_grace", "Early Grace", ColKind::Num),
            col("clock_in", "Actual In", ColKind::Time),
            col("clock_out", "Actual Out", ColKind::Time),
            col("status", "Status", ColKind::Status),
        ],
        rows,
        totals: Map::new(),
    })
}

// ---------------------------------------------------------------------------
// 5. Daily Attendance Shifts
// ---------------------------------------------------------------------------

/// How many people each shift actually had on the floor, day by day.
fn daily_shifts(conn: &Connection, f: &Filters) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT a.work_date AS work_date,
                COALESCE(s.name,'Not scheduled') AS shift_name,
                COALESCE(t.name,'Rest day')      AS timetable,
                COALESCE(t.on_duty,'')  AS on_duty,
                COALESCE(t.off_duty,'') AS off_duty,
                COUNT(*) AS rostered,
                SUM(CASE WHEN a.status IN ('Present','Late','EarlyLeave','HalfDay') THEN 1 ELSE 0 END) AS attended,
                SUM(CASE WHEN a.status = 'Absent' THEN 1 ELSE 0 END) AS absent,
                SUM(a.late_min) AS late_min,
                SUM(a.ot_min)   AS ot_min
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN shifts s     ON s.id = a.shift_id
         LEFT JOIN timetables t ON t.id = a.timetable_id
         WHERE a.work_date BETWEEN ? AND ?{narrow}
         GROUP BY a.work_date, a.shift_id, a.timetable_id
         ORDER BY a.work_date, s.name, t.on_duty"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let rows = collect(conn, &sql, bound)?;
    let totals = total(&rows, &["rostered", "attended", "absent", "late_min", "ot_min"]);

    Ok(ReportResult {
        key: "daily_shifts".into(),
        title: title_of("daily_shifts").into(),
        subtitle: String::new(),
        columns: vec![
            col("work_date", "Date", ColKind::Date),
            col("shift_name", "Shift", ColKind::Text),
            col("timetable", "Timetable", ColKind::Text),
            col("on_duty", "On Duty", ColKind::Text),
            col("off_duty", "Off Duty", ColKind::Text),
            col("rostered", "Rostered", ColKind::Num),
            col("attended", "Attended", ColKind::Num),
            col("absent", "Absent", ColKind::Num),
            col("late_min", "Late", ColKind::Mins),
            col("ot_min", "OT", ColKind::Mins),
        ],
        rows,
        totals,
    })
}

// ---------------------------------------------------------------------------
// 6. Daily Attendance OT Report
// ---------------------------------------------------------------------------

/// Every day on which somebody earned overtime.
fn daily_ot(conn: &Connection, f: &Filters) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT m.enroll_no AS ac_no, m.full_name AS name,
                COALESCE(d.name,'Unassigned') AS dept,
                a.work_date AS work_date,
                a.worked_min AS standard_min,
                a.ot_min AS ot_min,
                a.weekend_ot_min AS weekend_ot_min,
                (a.ot_min + a.weekend_ot_min) AS total_ot_min,
                a.in_time AS clock_in, a.out_time AS clock_out,
                COALESCE(t.off_duty,'') AS off_duty
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN departments d ON d.id = m.dept_id
         LEFT JOIN timetables t  ON t.id = a.timetable_id
         WHERE a.work_date BETWEEN ? AND ?
           -- Only days that produced overtime; a list of zeroes is not a report.
           AND (a.ot_min > 0 OR a.weekend_ot_min > 0){narrow}
         ORDER BY a.work_date, m.enroll_no"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let rows = collect(conn, &sql, bound)?;
    let totals = total(&rows, &["standard_min", "ot_min", "weekend_ot_min", "total_ot_min"]);

    Ok(ReportResult {
        key: "daily_ot".into(),
        title: title_of("daily_ot").into(),
        subtitle: String::new(),
        columns: vec![
            col("ac_no", "AC No.", ColKind::Num),
            col("name", "Name", ColKind::Text),
            col("dept", "Department", ColKind::Text),
            col("work_date", "Date", ColKind::Date),
            col("clock_in", "In", ColKind::Time),
            col("clock_out", "Out", ColKind::Time),
            col("off_duty", "Due Off", ColKind::Text),
            col("standard_min", "Standard Hours", ColKind::Mins),
            col("ot_min", "OT Hours", ColKind::Mins),
            col("weekend_ot_min", "Weekend OT", ColKind::Mins),
            col("total_ot_min", "Total OT", ColKind::Mins),
        ],
        rows,
        totals,
    })
}

// ---------------------------------------------------------------------------
// 7. Summary of Overtime
// ---------------------------------------------------------------------------

/// One line per member: the whole period's overtime, for payroll.
fn ot_summary(conn: &Connection, f: &Filters) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT m.enroll_no AS ac_no, m.full_name AS name,
                COALESCE(d.name,'Unassigned') AS dept,
                COALESCE(m.designation,'') AS designation,
                SUM(a.ot_min)         AS regular_ot_min,
                SUM(a.weekend_ot_min) AS weekend_ot_min,
                SUM(a.ot_min + a.weekend_ot_min) AS total_ot_min,
                SUM(CASE WHEN a.ot_min > 0 OR a.weekend_ot_min > 0 THEN 1 ELSE 0 END) AS ot_days
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         LEFT JOIN departments d ON d.id = m.dept_id
         WHERE a.work_date BETWEEN ? AND ?{narrow}
         GROUP BY m.id
         HAVING total_ot_min > 0
         ORDER BY total_ot_min DESC"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let rows = collect(conn, &sql, bound)?;
    let totals = total(&rows, &["regular_ot_min", "weekend_ot_min", "total_ot_min", "ot_days"]);

    Ok(ReportResult {
        key: "ot_summary".into(),
        title: title_of("ot_summary").into(),
        subtitle: String::new(),
        columns: vec![
            col("ac_no", "AC No.", ColKind::Num),
            col("name", "Name", ColKind::Text),
            col("dept", "Department", ColKind::Text),
            col("designation", "Designation", ColKind::Text),
            col("ot_days", "Days with OT", ColKind::Num),
            col("regular_ot_min", "Regular OT", ColKind::Mins),
            col("weekend_ot_min", "Weekend OT", ColKind::Mins),
            col("total_ot_min", "Total OT", ColKind::Mins),
        ],
        rows,
        totals,
    })
}

// ---------------------------------------------------------------------------
// 8. Daily Overtime
// ---------------------------------------------------------------------------

/// The school's overtime bill, day by day.
fn daily_overtime(conn: &Connection, f: &Filters) -> Result<ReportResult> {
    let (narrow, mut args) = f.narrow("m");
    let sql = format!(
        "SELECT a.work_date AS work_date,
                COUNT(DISTINCT CASE WHEN a.ot_min > 0 OR a.weekend_ot_min > 0
                                    THEN m.id END) AS staff_on_ot,
                SUM(a.ot_min)         AS regular_ot_min,
                SUM(a.weekend_ot_min) AS weekend_ot_min,
                SUM(a.ot_min + a.weekend_ot_min) AS total_ot_min,
                MAX(a.ot_min + a.weekend_ot_min) AS longest_min
         FROM attendance a
         JOIN members m ON m.id = a.member_id
         WHERE a.work_date BETWEEN ? AND ?{narrow}
         GROUP BY a.work_date
         HAVING total_ot_min > 0
         ORDER BY a.work_date"
    );
    let mut bound: Vec<Box<dyn ToSql>> = vec![Box::new(f.from.clone()), Box::new(f.to.clone())];
    bound.append(&mut args);

    let rows = collect(conn, &sql, bound)?;
    let totals = total(&rows, &["regular_ot_min", "weekend_ot_min", "total_ot_min"]);

    Ok(ReportResult {
        key: "daily_overtime".into(),
        title: title_of("daily_overtime").into(),
        subtitle: String::new(),
        columns: vec![
            col("work_date", "Date", ColKind::Date),
            col("staff_on_ot", "Staff on OT", ColKind::Num),
            col("regular_ot_min", "Regular OT", ColKind::Mins),
            col("weekend_ot_min", "Weekend OT", ColKind::Mins),
            col("total_ot_min", "Total OT", ColKind::Mins),
            col("longest_min", "Longest", ColKind::Mins),
        ],
        rows,
        totals,
    })
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

impl ReportResult {
    /// CSV for Excel.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("{}\r\n{}\r\n\r\n", self.title, self.subtitle));
        out.push_str(
            &self.columns.iter().map(|c| quote(&c.label)).collect::<Vec<_>>().join(","),
        );
        out.push_str("\r\n");

        for row in &self.rows {
            let line: Vec<String> = self
                .columns
                .iter()
                .map(|c| quote(&render(row.get(&c.key), c.kind)))
                .collect();
            out.push_str(&line.join(","));
            out.push_str("\r\n");
        }

        if !self.totals.is_empty() {
            let line: Vec<String> = self
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if i == 0 {
                        quote("TOTAL")
                    } else {
                        quote(&render(self.totals.get(&c.key), c.kind))
                    }
                })
                .collect();
            out.push_str(&line.join(","));
            out.push_str("\r\n");
        }
        out
    }

    /// A self-contained HTML table, used for printing and for the body of the
    /// email sent to the school's officials.
    pub fn to_html(&self, brand: &str) -> String {
        let head = self
            .columns
            .iter()
            .map(|c| format!("<th>{}</th>", escape(&c.label)))
            .collect::<Vec<_>>()
            .join("");

        let body = self
            .rows
            .iter()
            .map(|row| {
                let cells = self
                    .columns
                    .iter()
                    .map(|c| {
                        let v = escape(&render(row.get(&c.key), c.kind));
                        let align = match c.kind {
                            ColKind::Num | ColKind::Mins | ColKind::Pct => " class=\"n\"",
                            _ => "",
                        };
                        format!("<td{align}>{v}</td>")
                    })
                    .collect::<Vec<_>>()
                    .join("");
                format!("<tr>{cells}</tr>")
            })
            .collect::<Vec<_>>()
            .join("");

        let foot = if self.totals.is_empty() {
            String::new()
        } else {
            let cells = self
                .columns
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    if i == 0 {
                        "<td><b>TOTAL</b></td>".to_string()
                    } else {
                        format!(
                            "<td class=\"n\"><b>{}</b></td>",
                            escape(&render(self.totals.get(&c.key), c.kind))
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join("");
            format!("<tfoot><tr>{cells}</tr></tfoot>")
        };

        format!(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
             <title>{title}</title><style>\
             body{{font:13px/1.5 -apple-system,Segoe UI,Roboto,sans-serif;color:#1e293b;margin:24px}}\
             h1{{font-size:18px;margin:0 0 2px}}\
             .sub{{color:#64748b;font-size:12px;margin:0 0 16px}}\
             table{{border-collapse:collapse;width:100%;font-size:12px}}\
             th,td{{border:1px solid #e2e8f0;padding:6px 8px;text-align:left}}\
             th{{background:#F16522;color:#fff;font-weight:600}}\
             td.n{{text-align:right;font-variant-numeric:tabular-nums}}\
             tbody tr:nth-child(even){{background:#fafafa}}\
             tfoot td{{background:#fff7ed;border-top:2px solid #F16522}}\
             @media print{{body{{margin:0}}th{{-webkit-print-color-adjust:exact;print-color-adjust:exact}}}}\
             </style></head><body>\
             <h1>{brand} — {title}</h1><p class=\"sub\">{sub}</p>\
             <table><thead><tr>{head}</tr></thead><tbody>{body}</tbody>{foot}</table>\
             </body></html>",
            title = escape(&self.title),
            brand = escape(brand),
            sub = escape(&self.subtitle),
        )
    }
}

/// Format one cell for export.
fn render(v: Option<&Value>, kind: ColKind) -> String {
    let Some(v) = v else { return String::new() };
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => match kind {
            ColKind::Mins => crate::rules::fmt_duration(n.as_f64().unwrap_or(0.0) as i32),
            ColKind::Pct => format!("{:.1}%", n.as_f64().unwrap_or(0.0)),
            _ => {
                let f = n.as_f64().unwrap_or(0.0);
                if f.fract() == 0.0 {
                    format!("{}", f as i64)
                } else {
                    format!("{f}")
                }
            }
        },
        other => other.to_string(),
    }
}

/// CSV quoting. Excel opens a field beginning with `=` as a formula, so any
/// value that could be read as one is prefixed with a quote — a staff member
/// named `=Sum` should not execute in the accounts office.
fn quote(s: &str) -> String {
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::service;
    use rusqlite::params;

    /// A database with one department, three staff and a week of punches.
    fn seeded() -> Connection {
        let mut conn = db::open_memory().unwrap();
        for (id, enroll, name) in [(101, 41, "Anita Shrestha"), (102, 42, "Bikash Rai"), (103, 43, "Chandra Thapa")] {
            conn.execute(
                "INSERT INTO members (id, enroll_no, full_name, dept_id, joined_on)
                 VALUES (?1, ?2, ?3, 1, '2020-01-01')",
                params![id, enroll, name],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO employee_schedules (member_id, shift_id, start_date)
                 VALUES (?1, 1, '2020-01-01')",
                params![id],
            )
            .unwrap();
        }

        // Monday 2026-08-17 to Friday 2026-08-21.
        let days = ["2026-08-17", "2026-08-18", "2026-08-19", "2026-08-20", "2026-08-21"];
        for d in days {
            // Anita punctual, Bikash late, Chandra works overtime.
            for (enroll, in_t, out_t) in
                [(41, "08:55:00", "16:05:00"), (42, "09:40:00", "16:10:00"), (43, "08:50:00", "18:30:00")]
            {
                for (t, state) in [(in_t, 0), (out_t, 1)] {
                    conn.execute(
                        "INSERT INTO punches (device_serial, enroll_no, punch_time, punch_state)
                         VALUES ('TEST', ?1, ?2, ?3)",
                        params![enroll, format!("{d} {t}"), state],
                    )
                    .unwrap();
                }
            }
        }
        service::recompute(&mut conn, "2026-08-17", "2026-08-22").unwrap();
        conn
    }

    fn week() -> Filters {
        Filters {
            from: "2026-08-17".into(),
            to: "2026-08-22".into(),
            dept_id: None,
            member_id: None,
        }
    }

    #[test]
    fn every_report_runs_and_returns_its_own_shape() {
        let conn = seeded();
        for (key, title) in REPORTS {
            let r = run(&conn, key, &week())
                .unwrap_or_else(|e| panic!("report '{key}' failed: {e}"));
            assert_eq!(&r.key, key);
            assert_eq!(&r.title, title);
            assert!(!r.columns.is_empty(), "'{key}' produced no columns");
            // Every row must answer every column, or the grid renders holes.
            for row in &r.rows {
                for c in &r.columns {
                    assert!(
                        row.contains_key(&c.key),
                        "'{key}' row is missing column '{}'",
                        c.key
                    );
                }
            }
        }
    }

    #[test]
    fn an_unknown_report_is_refused_by_name() {
        let conn = seeded();
        let e = run(&conn, "payroll_slips", &week()).unwrap_err().to_string();
        assert!(e.contains("payroll_slips"), "{e}");
    }

    #[test]
    fn backwards_dates_are_refused_before_any_query_runs() {
        let conn = seeded();
        let f = Filters { from: "2026-08-22".into(), to: "2026-08-17".into(), ..Default::default() };
        let e = run(&conn, "general", &f).unwrap_err().to_string();
        assert!(e.contains("before the start date"), "{e}");
    }

    #[test]
    fn the_general_report_shows_one_row_per_member_day() {
        let conn = seeded();
        let r = run(&conn, "general", &week()).unwrap();
        // Three staff, six days (Mon-Sat) — Saturday included as a rest day.
        assert_eq!(r.rows.len(), 18);

        let late = r
            .rows
            .iter()
            .find(|x| x["ac_no"] == json!(42) && x["work_date"] == json!("2026-08-18"))
            .expect("Bikash's Tuesday");
        assert_eq!(late["status"], json!("Late"));
        assert_eq!(late["late_min"], json!(30), "09:40 against 09:00 plus 10 grace");
    }

    #[test]
    fn the_statistic_report_totals_the_period_per_member() {
        let conn = seeded();
        let r = run(&conn, "daily_stat", &week()).unwrap();
        assert_eq!(r.rows.len(), 3, "one line per member");

        let anita = r.rows.iter().find(|x| x["ac_no"] == json!(41)).unwrap();
        assert_eq!(anita["present"], json!(5), "five working days");
        assert_eq!(anita["absent"], json!(0));
        assert_eq!(anita["rate"], json!(100.0));

        let bikash = r.rows.iter().find(|x| x["ac_no"] == json!(42)).unwrap();
        assert_eq!(bikash["late"], json!(5));
        assert_eq!(bikash["late_min"], json!(150), "thirty minutes on each of five days");
    }

    #[test]
    fn a_period_with_no_working_days_reports_zero_not_nan() {
        // Saturday alone: the school's rest day.
        let conn = seeded();
        let f = Filters { from: "2026-08-22".into(), to: "2026-08-22".into(), ..Default::default() };
        let r = run(&conn, "daily_stat", &f).unwrap();
        for row in &r.rows {
            let rate = row["rate"].as_f64().unwrap();
            assert!(rate.is_finite(), "a rest day must not print NaN%");
            assert_eq!(rate, 0.0);
        }
    }

    #[test]
    fn overtime_reports_only_list_days_that_earned_any() {
        let conn = seeded();
        let daily = run(&conn, "daily_ot", &week()).unwrap();
        assert!(!daily.rows.is_empty());
        for row in &daily.rows {
            let ot = row["total_ot_min"].as_i64().unwrap();
            assert!(ot > 0, "a row with no overtime is on the overtime report");
        }
        // Only Chandra stays late.
        assert!(daily.rows.iter().all(|r| r["ac_no"] == json!(43)));

        let summary = run(&conn, "ot_summary", &week()).unwrap();
        assert_eq!(summary.rows.len(), 1);
        let c = &summary.rows[0];
        assert_eq!(c["ac_no"], json!(43));
        // 18:30 against a 16:00 finish is 150 minutes, five times.
        assert_eq!(c["regular_ot_min"], json!(750));
    }

    #[test]
    fn the_department_report_groups_by_department_and_day() {
        let conn = seeded();
        let r = run(&conn, "dept_stat", &week()).unwrap();
        // One department, six days.
        assert_eq!(r.rows.len(), 6);
        let mon = r.rows.iter().find(|x| x["work_date"] == json!("2026-08-17")).unwrap();
        assert_eq!(mon["total_staff"], json!(3));
        assert_eq!(mon["present"], json!(3));
        assert_eq!(mon["late"], json!(1));
    }

    #[test]
    fn the_duty_timetable_resolves_the_whole_three_tier_chain() {
        let conn = seeded();
        let r = run(&conn, "duty_timetable", &week()).unwrap();
        let working = r
            .rows
            .iter()
            .find(|x| x["work_date"] == json!("2026-08-17") && x["ac_no"] == json!(41))
            .unwrap();
        assert_ne!(working["shift_name"], json!("Not scheduled"));
        assert_eq!(working["on_duty"], json!("09:00"));
        assert_eq!(working["off_duty"], json!("16:00"));
    }

    #[test]
    fn filters_narrow_to_one_member_and_one_department() {
        let conn = seeded();
        let mut f = week();
        f.member_id = Some(102);
        let r = run(&conn, "general", &f).unwrap();
        assert!(r.rows.iter().all(|x| x["ac_no"] == json!(42)));

        let mut f2 = week();
        f2.dept_id = Some(9999); // no such department
        let r2 = run(&conn, "general", &f2).unwrap();
        assert!(r2.rows.is_empty(), "an unmatched filter must return nothing, not everything");
    }

    #[test]
    fn csv_neutralises_a_value_excel_would_run_as_a_formula() {
        let conn = seeded();
        conn.execute(
            "UPDATE members SET full_name = '=cmd|calc' WHERE id = 101",
            [],
        )
        .unwrap();
        let r = run(&conn, "daily_stat", &week()).unwrap();
        let csv = r.to_csv();
        assert!(csv.contains("'=cmd|calc"), "a leading = must be defused");
        assert!(!csv.contains(",=cmd"), "the raw formula reached the file");
    }

    #[test]
    fn csv_quotes_a_name_containing_a_comma() {
        let conn = seeded();
        conn.execute("UPDATE members SET full_name = 'Rai, Bikash' WHERE id = 102", []).unwrap();
        let r = run(&conn, "daily_stat", &week()).unwrap();
        let csv = r.to_csv();
        assert!(csv.contains("\"Rai, Bikash\""), "a comma in a name must not split the column");
        // Header, blank line, column row, three members, totals.
        let cols = r.columns.len();
        for line in csv.lines().skip(4).filter(|l| !l.is_empty()) {
            let commas = line.matches(',').count();
            assert!(commas < cols + 3, "line has run over its columns: {line}");
        }
    }

    #[test]
    fn html_escapes_a_name_that_looks_like_markup() {
        let conn = seeded();
        conn.execute(
            "UPDATE members SET full_name = '<script>alert(1)</script>' WHERE id = 101",
            [],
        )
        .unwrap();
        let r = run(&conn, "daily_stat", &week()).unwrap();
        let html = r.to_html("JWS");
        assert!(!html.contains("<script>"), "a name reached the page as markup");
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn minutes_print_as_hours_and_minutes_on_export() {
        let conn = seeded();
        let r = run(&conn, "ot_summary", &week()).unwrap();
        let csv = r.to_csv();
        // 750 minutes is 12:30, not "750".
        assert!(csv.contains("12:30"), "overtime should print as hours:minutes\n{csv}");
    }

    #[test]
    fn totals_are_present_and_add_up() {
        let conn = seeded();
        let r = run(&conn, "daily_stat", &week()).unwrap();
        let summed: i64 = r.rows.iter().map(|x| x["late_min"].as_i64().unwrap()).sum();
        assert_eq!(r.totals["late_min"], json!(summed));
    }
}
