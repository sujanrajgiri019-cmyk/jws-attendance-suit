//! Commands behind the Attendance Rules and Timetables screens.
//!
//! Every one of these is a thin wrapper: the decisions live in `zk_core`, which
//! is testable without a window. What this file adds is the database handle,
//! the audit trail, and error messages an office can act on.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;
use zk_core::db;
use zk_core::ruleset::AttendanceRules;
use zk_core::schedule::{DayPlan, Roster};
use zk_core::service;

use crate::commands::{conn, AppState, R};

// ---------------------------------------------------------------------------
// The Nepali calendar
// ---------------------------------------------------------------------------

/// Hand the whole Bikram Sambat table to the interface once, at start-up.
///
/// The date pickers convert in the browser from this table rather than asking
/// the backend per keystroke, but the numbers are still the ones `zk_core`
/// uses for every report — so the date somebody picks and the date that lands
/// on a payroll sheet can never drift apart.
#[tauri::command(async)]
pub fn bs_calendar() -> R<zk_core::calendar::BsTable> {
    Ok(zk_core::calendar::bs_table())
}

// ---------------------------------------------------------------------------
// Attendance rules
// ---------------------------------------------------------------------------

#[tauri::command(async)]
pub fn get_attendance_rules(state: State<'_, AppState>) -> R<AttendanceRules> {
    let c = conn(&state)?;
    AttendanceRules::load(&c).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn save_attendance_rules(state: State<'_, AppState>, rules: AttendanceRules) -> R<()> {
    let c = conn(&state)?;
    rules.save(&c).map_err(|e| e.to_string())?;
    let _ = db::audit(&c, "admin", "rules.save", "attendance rules updated");
    Ok(())
}

// ---------------------------------------------------------------------------
// Timetables — the atomic blocks of duty
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimetableRow {
    pub id: Option<i64>,
    pub name: String,
    pub on_duty: String,
    pub off_duty: String,
    pub in_begin: String,
    pub in_end: String,
    pub out_begin: String,
    pub out_end: String,
    pub late_grace: i64,
    pub early_grace: i64,
    pub break_min: i64,
    pub workday_value: f64,
    pub work_minutes: i64,
    pub must_c_in: bool,
    pub must_c_out: bool,
    pub count_ot: bool,
    pub min_ot_block: i64,
    pub colour: String,
    pub active: bool,
    /// How many shifts use this block. Read-only; used to warn before deleting.
    #[serde(default)]
    pub used_by: i64,
}

#[tauri::command(async)]
pub fn list_timetables_full(state: State<'_, AppState>) -> R<Vec<TimetableRow>> {
    let c = conn(&state)?;
    let mut s = c
        .prepare(
            "SELECT t.*, (SELECT COUNT(DISTINCT shift_id) FROM shift_items
                          WHERE timetable_id = t.id) AS used_by
             FROM timetables t ORDER BY t.on_duty, t.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = s
        .query_map([], |r| {
            Ok(TimetableRow {
                id: r.get("id")?,
                name: r.get("name")?,
                on_duty: r.get("on_duty")?,
                off_duty: r.get("off_duty")?,
                in_begin: r.get("in_begin")?,
                in_end: r.get("in_end")?,
                out_begin: r.get("out_begin")?,
                out_end: r.get("out_end")?,
                late_grace: r.get("late_grace")?,
                early_grace: r.get("early_grace")?,
                break_min: r.get("break_min")?,
                workday_value: r.get("workday_value")?,
                work_minutes: r.get("work_minutes")?,
                must_c_in: r.get::<_, i64>("must_c_in")? != 0,
                must_c_out: r.get::<_, i64>("must_c_out")? != 0,
                count_ot: r.get::<_, i64>("count_ot")? != 0,
                min_ot_block: r.get("min_ot_block")?,
                colour: r.get("colour")?,
                active: r.get::<_, i64>("active")? != 0,
                used_by: r.get("used_by")?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

/// Reject a block that cannot describe a working period.
///
/// These checks are here rather than in the form because the form is not the
/// only way in, and a timetable with a check-in window that closes before it
/// opens silently records every arrival as missing.
fn validate_timetable(t: &TimetableRow) -> Result<(), String> {
    use zk_core::rules::parse_hhmm;
    let at = |label: &str, v: &str| -> Result<i32, String> {
        parse_hhmm(v).ok_or_else(|| format!("{label} needs a time like 09:00, not '{v}'."))
    };

    if t.name.trim().is_empty() {
        return Err("Give the timetable a name.".into());
    }
    let on = at("On duty", &t.on_duty)?;
    let off = at("Off duty", &t.off_duty)?;
    let ib = at("Check-in window start", &t.in_begin)?;
    let ie = at("Check-in window end", &t.in_end)?;
    let ob = at("Check-out window start", &t.out_begin)?;
    let oe = at("Check-out window end", &t.out_end)?;

    // Windows that close before they open catch every punch on the wrong side.
    if ie <= ib {
        return Err(format!(
            "The check-in window closes at {} but opens at {} — it has to close later.",
            t.in_end, t.in_begin
        ));
    }
    let overnight = off <= on;
    if !overnight && oe <= ob {
        return Err(format!(
            "The check-out window closes at {} but opens at {} — it has to close later.",
            t.out_end, t.out_begin
        ));
    }
    // The on-duty time has to fall inside the window that classifies arrivals,
    // or a punctual arrival is not recognised as an arrival at all.
    if !overnight && (on < ib || on > ie) {
        return Err(format!(
            "On duty is {} but the check-in window is {} to {}. \
             A member of staff arriving on time would not be counted.",
            t.on_duty, t.in_begin, t.in_end
        ));
    }
    if t.workday_value <= 0.0 || t.workday_value > 2.0 {
        return Err("A timetable is worth between 0 and 2 workdays.".into());
    }
    if t.break_min < 0 || t.break_min >= ((off - on).abs() + 1440) as i64 {
        return Err("The break cannot be longer than the duty itself.".into());
    }
    if !t.colour.starts_with('#') || t.colour.len() != 7 {
        return Err("Pick a colour for this timetable.".into());
    }
    Ok(())
}

#[tauri::command(async)]
pub fn save_timetable(state: State<'_, AppState>, tt: TimetableRow) -> R<i64> {
    validate_timetable(&tt)?;
    let c = conn(&state)?;

    let p = params![
        tt.name.trim(),
        tt.on_duty,
        tt.off_duty,
        tt.in_begin,
        tt.in_end,
        tt.out_begin,
        tt.out_end,
        tt.late_grace,
        tt.early_grace,
        tt.break_min,
        tt.workday_value,
        tt.work_minutes,
        tt.must_c_in as i64,
        tt.must_c_out as i64,
        tt.count_ot as i64,
        tt.min_ot_block,
        tt.colour,
        tt.active as i64,
    ];

    let id = match tt.id {
        Some(id) => {
            c.execute(
                "UPDATE timetables SET name=?1, on_duty=?2, off_duty=?3, in_begin=?4, in_end=?5,
                    out_begin=?6, out_end=?7, late_grace=?8, early_grace=?9, break_min=?10,
                    workday_value=?11, work_minutes=?12, must_c_in=?13, must_c_out=?14,
                    count_ot=?15, min_ot_block=?16, colour=?17, active=?18
                 WHERE id=?19",
                rusqlite::params_from_iter(
                    p.iter().copied().chain(std::iter::once(&id as &dyn rusqlite::ToSql)),
                ),
            )
            .map_err(|e| name_clash(e, &tt.name))?;
            id
        }
        None => {
            c.execute(
                "INSERT INTO timetables (name, on_duty, off_duty, in_begin, in_end, out_begin,
                    out_end, late_grace, early_grace, break_min, workday_value, work_minutes,
                    must_c_in, must_c_out, count_ot, min_ot_block, colour, active)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                p,
            )
            .map_err(|e| name_clash(e, &tt.name))?;
            c.last_insert_rowid()
        }
    };
    let _ = db::audit(&c, "admin", "timetable.save", &tt.name);
    Ok(id)
}

/// Turn SQLite's UNIQUE failure into something the office understands.
fn name_clash(e: rusqlite::Error, name: &str) -> String {
    let s = e.to_string();
    if s.contains("UNIQUE") {
        format!("There is already something called '{name}'. Pick a different name.")
    } else {
        s
    }
}

#[tauri::command(async)]
pub fn delete_timetable(state: State<'_, AppState>, id: i64) -> R<()> {
    let c = conn(&state)?;

    // Deleting a block that a shift still uses would cascade the shift's days
    // away and silently turn them into rest days.
    let used: i64 = c
        .query_row(
            "SELECT COUNT(DISTINCT shift_id) FROM shift_items WHERE timetable_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if used > 0 {
        return Err(format!(
            "This timetable is still used by {used} shift{}. Remove it from them first, \
             or switch it off instead of deleting it.",
            if used == 1 { "" } else { "s" }
        ));
    }

    let name: Option<String> = c
        .query_row("SELECT name FROM timetables WHERE id=?1", params![id], |r| r.get(0))
        .ok();
    c.execute("DELETE FROM timetables WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    let _ = db::audit(&c, "admin", "timetable.delete", &name.unwrap_or_default());
    Ok(())
}

// ---------------------------------------------------------------------------
// Shifts — the cycles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftRow {
    pub id: Option<i64>,
    pub name: String,
    pub code: String,
    pub begin_date: String,
    pub cycle_num: i64,
    pub cycle_unit: String,
    pub active: bool,
    #[serde(default)]
    pub assigned: i64,
}

#[tauri::command(async)]
pub fn list_shift_cycles(state: State<'_, AppState>) -> R<Vec<ShiftRow>> {
    let c = conn(&state)?;
    let mut s = c
        .prepare(
            "SELECT s.*,
                    (SELECT COUNT(DISTINCT member_id) FROM employee_schedules
                     WHERE shift_id = s.id
                       AND start_date <= date('now','localtime')
                       AND (end_date IS NULL OR end_date >= date('now','localtime'))
                    ) AS assigned
             FROM shifts s ORDER BY s.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = s
        .query_map([], |r| {
            Ok(ShiftRow {
                id: r.get("id")?,
                name: r.get("name")?,
                code: r.get("code")?,
                begin_date: r.get("begin_date")?,
                cycle_num: r.get("cycle_num")?,
                cycle_unit: r.get("cycle_unit")?,
                active: r.get::<_, i64>("active")? != 0,
                assigned: r.get("assigned")?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn save_shift(state: State<'_, AppState>, shift: ShiftRow) -> R<i64> {
    if shift.name.trim().is_empty() {
        return Err("Give the shift a name.".into());
    }
    if !matches!(shift.cycle_unit.as_str(), "Week" | "Month") {
        return Err("A cycle repeats by week or by month.".into());
    }
    if !(1..=12).contains(&shift.cycle_num) {
        return Err("A cycle is between 1 and 12 units long.".into());
    }
    let c = conn(&state)?;
    let id = match shift.id {
        Some(id) => {
            c.execute(
                "UPDATE shifts SET name=?1, code=?2, begin_date=?3, cycle_num=?4,
                    cycle_unit=?5, active=?6 WHERE id=?7",
                params![
                    shift.name.trim(),
                    shift.code,
                    shift.begin_date,
                    shift.cycle_num,
                    shift.cycle_unit,
                    shift.active as i64,
                    id
                ],
            )
            .map_err(|e| name_clash(e, &shift.name))?;

            // Shortening a cycle strands the days beyond its new end: they stay
            // in the table, invisible on screen, and reappear if it is ever
            // lengthened again. Clear them out with the change that caused it.
            let slots = if shift.cycle_unit == "Month" { 31 } else { 7 } * shift.cycle_num;
            c.execute(
                "DELETE FROM shift_items WHERE shift_id=?1 AND day_index >= ?2",
                params![id, slots],
            )
            .map_err(|e| e.to_string())?;
            id
        }
        None => {
            c.execute(
                "INSERT INTO shifts (name, code, begin_date, cycle_num, cycle_unit, active)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    shift.name.trim(),
                    shift.code,
                    shift.begin_date,
                    shift.cycle_num,
                    shift.cycle_unit,
                    shift.active as i64
                ],
            )
            .map_err(|e| name_clash(e, &shift.name))?;
            c.last_insert_rowid()
        }
    };
    let _ = db::audit(&c, "admin", "shift.save", &shift.name);
    Ok(id)
}

#[tauri::command(async)]
pub fn delete_shift(state: State<'_, AppState>, id: i64) -> R<()> {
    let c = conn(&state)?;
    let assigned: i64 = c
        .query_row(
            "SELECT COUNT(DISTINCT member_id) FROM employee_schedules WHERE shift_id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if assigned > 0 {
        return Err(format!(
            "{assigned} member{} of staff {} still on this shift. \
             Move them to another shift first.",
            if assigned == 1 { "" } else { "s" },
            if assigned == 1 { "is" } else { "are" }
        ));
    }
    let name: Option<String> =
        c.query_row("SELECT name FROM shifts WHERE id=?1", params![id], |r| r.get(0)).ok();
    c.execute("DELETE FROM shifts WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    let _ = db::audit(&c, "admin", "shift.delete", &name.unwrap_or_default());
    Ok(())
}

/// One cell of the weekly grid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiftItem {
    pub id: i64,
    pub day_index: i64,
    pub timetable_id: i64,
    pub timetable_name: String,
    pub on_duty: String,
    pub off_duty: String,
    pub colour: String,
}

#[tauri::command(async)]
pub fn shift_grid(state: State<'_, AppState>, shift_id: i64) -> R<Vec<ShiftItem>> {
    let c = conn(&state)?;
    let mut s = c
        .prepare(
            "SELECT si.id, si.day_index, si.timetable_id, t.name, t.on_duty, t.off_duty, t.colour
             FROM shift_items si JOIN timetables t ON t.id = si.timetable_id
             WHERE si.shift_id = ?1
             ORDER BY si.day_index, t.on_duty",
        )
        .map_err(|e| e.to_string())?;
    let rows = s
        .query_map(params![shift_id], |r| {
            Ok(ShiftItem {
                id: r.get(0)?,
                day_index: r.get(1)?,
                timetable_id: r.get(2)?,
                timetable_name: r.get(3)?,
                on_duty: r.get(4)?,
                off_duty: r.get(5)?,
                colour: r.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn add_shift_item(
    state: State<'_, AppState>,
    shift_id: i64,
    day_index: i64,
    timetable_id: i64,
) -> R<i64> {
    let c = conn(&state)?;

    // Refuse a block that overlaps one already on that day. Two duties running
    // at the same time is not a roster, it is a mistake, and the engine would
    // count the same minutes twice.
    let clash: Option<String> = c
        .query_row(
            "SELECT t.name FROM shift_items si
             JOIN timetables t   ON t.id = si.timetable_id
             JOIN timetables new ON new.id = ?3
             WHERE si.shift_id = ?1 AND si.day_index = ?2
               AND t.on_duty < new.off_duty AND new.on_duty < t.off_duty",
            params![shift_id, day_index, timetable_id],
            |r| r.get(0),
        )
        .ok();
    if let Some(other) = clash {
        return Err(format!("That overlaps '{other}', which is already on this day."));
    }

    c.execute(
        "INSERT OR IGNORE INTO shift_items (shift_id, day_index, timetable_id)
         VALUES (?1,?2,?3)",
        params![shift_id, day_index, timetable_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(c.last_insert_rowid())
}

#[tauri::command(async)]
pub fn delete_shift_item(state: State<'_, AppState>, id: i64) -> R<()> {
    let c = conn(&state)?;
    c.execute("DELETE FROM shift_items WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn clear_shift_grid(state: State<'_, AppState>, shift_id: i64) -> R<usize> {
    let c = conn(&state)?;
    c.execute("DELETE FROM shift_items WHERE shift_id=?1", params![shift_id])
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Employee schedules
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRow {
    pub id: Option<i64>,
    pub member_id: i64,
    pub member_name: String,
    pub enroll_no: i64,
    pub shift_id: i64,
    pub shift_name: String,
    pub start_date: String,
    pub end_date: Option<String>,
    pub is_temporary: bool,
    pub note: Option<String>,
}

/// The roster table on the right of the Employee Schedule tab.
#[tauri::command(async)]
pub fn roster(state: State<'_, AppState>, dept_id: Option<i64>) -> R<Vec<ScheduleRow>> {
    let c = conn(&state)?;
    let mut sql = String::from(
        "SELECT es.id, m.id AS member_id, m.full_name, m.enroll_no,
                es.shift_id, s.name AS shift_name, es.start_date, es.end_date,
                es.is_temporary, es.note
         FROM members m
         LEFT JOIN employee_schedules es ON es.member_id = m.id
         LEFT JOIN shifts s ON s.id = es.shift_id
         WHERE m.status <> 'Inactive'",
    );
    if dept_id.is_some() {
        sql.push_str(" AND m.dept_id = ?1");
    }
    sql.push_str(" ORDER BY m.enroll_no, es.is_temporary DESC, es.start_date DESC");

    let mut s = c.prepare(&sql).map_err(|e| e.to_string())?;
    let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<ScheduleRow> {
        Ok(ScheduleRow {
            id: r.get("id")?,
            member_id: r.get("member_id")?,
            member_name: r.get("full_name")?,
            enroll_no: r.get("enroll_no")?,
            shift_id: r.get::<_, Option<i64>>("shift_id")?.unwrap_or(0),
            shift_name: r.get::<_, Option<String>>("shift_name")?.unwrap_or_default(),
            start_date: r.get::<_, Option<String>>("start_date")?.unwrap_or_default(),
            end_date: r.get("end_date")?,
            is_temporary: r.get::<_, Option<i64>>("is_temporary")?.unwrap_or(0) != 0,
            note: r.get("note")?,
        })
    };
    let rows = match dept_id {
        Some(d) => s.query_map(params![d], map),
        None => s.query_map([], map),
    }
    .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn save_schedule(state: State<'_, AppState>, row: ScheduleRow) -> R<i64> {
    if let Some(end) = row.end_date.as_deref().filter(|e| !e.is_empty()) {
        if end < row.start_date.as_str() {
            return Err(format!(
                "The schedule ends on {end} but starts on {}. Check the dates.",
                row.start_date
            ));
        }
    }
    if row.start_date.len() != 10 {
        return Err("Choose a start date for this schedule.".into());
    }

    let c = conn(&state)?;
    let end = row.end_date.filter(|e| !e.is_empty());
    let id = match row.id {
        Some(id) => {
            c.execute(
                "UPDATE employee_schedules SET shift_id=?1, start_date=?2, end_date=?3,
                    is_temporary=?4, note=?5 WHERE id=?6",
                params![row.shift_id, row.start_date, end, row.is_temporary as i64, row.note, id],
            )
            .map_err(|e| e.to_string())?;
            id
        }
        None => {
            c.execute(
                "INSERT INTO employee_schedules
                    (member_id, shift_id, start_date, end_date, is_temporary, note)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    row.member_id,
                    row.shift_id,
                    row.start_date,
                    end,
                    row.is_temporary as i64,
                    row.note
                ],
            )
            .map_err(|e| e.to_string())?;
            c.last_insert_rowid()
        }
    };
    let _ = db::audit(
        &c,
        "admin",
        "schedule.save",
        &format!("member {} on shift {}", row.member_id, row.shift_id),
    );
    Ok(id)
}

#[tauri::command(async)]
pub fn delete_schedule(state: State<'_, AppState>, id: i64) -> R<()> {
    let c = conn(&state)?;
    c.execute("DELETE FROM employee_schedules WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Put a whole group of people on a shift in one go — the "Arrange Shifts"
/// button.
#[tauri::command(async)]
pub fn arrange_shifts(
    state: State<'_, AppState>,
    member_ids: Vec<i64>,
    shift_id: i64,
    start_date: String,
    end_date: Option<String>,
    is_temporary: bool,
) -> R<usize> {
    if member_ids.is_empty() {
        return Err("Select at least one member of staff.".into());
    }
    let mut c = conn(&state)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let mut n = 0usize;
    for m in &member_ids {
        if is_temporary {
            tx.execute(
                "INSERT INTO employee_schedules
                    (member_id, shift_id, start_date, end_date, is_temporary, note)
                 VALUES (?1,?2,?3,?4,1,'Temporary arrangement')",
                params![m, shift_id, start_date, end_date],
            )
            .map_err(|e| e.to_string())?;
        } else {
            service::set_standing_shift(&tx, *m, Some(shift_id), Some(&start_date))
                .map_err(|e| e.to_string())?;
        }
        n += 1;
    }
    let _ = db::audit(
        &tx,
        "admin",
        "schedule.arrange",
        &format!("{n} staff onto shift {shift_id}"),
    );
    tx.commit().map_err(|e| e.to_string())?;
    Ok(n)
}

/// The date-by-date calendar at the bottom of the Employee Schedule tab.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarDay {
    pub date: String,
    pub date_bs: Option<String>,
    pub weekday: u32,
    pub is_weekend: bool,
    pub holiday: Option<String>,
    pub plan: DayPlan,
}

#[tauri::command(async)]
pub fn member_calendar(
    state: State<'_, AppState>,
    member_id: i64,
    from: String,
    to: String,
) -> R<Vec<CalendarDay>> {
    let c = conn(&state)?;
    let roster = Roster::load(&c).map_err(|e| e.to_string())?;
    let rules = AttendanceRules::load(&c).map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for date in date_span(&c, &from, &to)? {
        let weekday = weekday_of(&date);
        let holiday: Option<String> = c
            .query_row(
                "SELECT name FROM holidays WHERE ?1 BETWEEN from_date AND to_date LIMIT 1",
                params![date],
                |r| r.get(0),
            )
            .ok();
        let plan = roster.plan_for(member_id, &date).map_err(|e| e.to_string())?;
        out.push(CalendarDay {
            date_bs: zk_core::calendar::iso_to_bs(&date).ok().map(|b| b.pretty()),
            weekday,
            is_weekend: rules.is_weekend(weekday) || plan.is_rest(),
            holiday,
            plan,
            date,
        });
    }
    Ok(out)
}

/// Ask SQLite to walk the dates, so month lengths and leap years are its
/// problem rather than ours.
fn date_span(c: &Connection, from: &str, to: &str) -> Result<Vec<String>, String> {
    if to < from {
        return Err(format!("{to} is before {from}."));
    }
    let mut s = c
        .prepare(
            "WITH RECURSIVE d(x) AS (
                 SELECT date(?1)
                 UNION ALL SELECT date(x, '+1 day') FROM d WHERE x < date(?2)
             ) SELECT x FROM d LIMIT 400",
        )
        .map_err(|e| e.to_string())?;
    let rows = s.query_map(params![from, to], |r| r.get(0)).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<String>>>().map_err(|e| e.to_string())
}

/// Sakamoto's algorithm; 0 = Sunday.
fn weekday_of(iso: &str) -> u32 {
    let p: Vec<i64> = iso.split('-').filter_map(|v| v.parse().ok()).collect();
    if p.len() != 3 {
        return 0;
    }
    let (mut y, m, d) = (p[0], p[1], p[2]);
    const T: [i64; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    if m < 3 {
        y -= 1;
    }
    ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + d) % 7) as u32
}

/// The department tree on the left of the Employee Schedule tab.
#[derive(Debug, Clone, Serialize)]
pub struct DeptNode {
    pub id: i64,
    pub name: String,
    pub code: String,
    pub colour: String,
    pub member_count: i64,
}

#[tauri::command(async)]
pub fn department_tree(state: State<'_, AppState>) -> R<Vec<DeptNode>> {
    let c = conn(&state)?;
    let mut s = c
        .prepare(
            "SELECT d.id, d.name, d.code, d.colour,
                    (SELECT COUNT(*) FROM members m
                     WHERE m.dept_id = d.id AND m.status <> 'Inactive') AS n
             FROM departments d WHERE d.active = 1 ORDER BY d.name",
        )
        .map_err(|e| e.to_string())?;
    let rows = s
        .query_map([], |r| {
            Ok(DeptNode {
                id: r.get(0)?,
                name: r.get(1)?,
                code: r.get(2)?,
                colour: r.get(3)?,
                member_count: r.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}
