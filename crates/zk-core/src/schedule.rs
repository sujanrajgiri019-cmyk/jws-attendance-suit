//! Resolving what a member of staff was *meant* to be doing on a given day.
//!
//! Three tiers, each built from the one below:
//!
//! ```text
//!   timetable          one block of duty        10:00 - 16:30
//!   shift              a cycle of timetables    Sun-Fri regular, Sat off
//!   employee_schedule  who is on which shift    from 2025-01-15, no end
//! ```
//!
//! A recompute asks this question once per member per day — thirty staff over a
//! month is nine hundred lookups — so the whole roster is read into memory once
//! and answered from there. Querying per day made a month-end recompute take
//! long enough that the office assumed the app had frozen.

use crate::calendar::days_from_civil_pub;
use crate::rules::parse_hhmm;
use crate::{Error, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One block of duty, flattened to minutes from midnight.
///
/// `off_min` may exceed 1440 for a block that runs past midnight; the whole
/// engine works in "minutes since the start of the work date", so an overnight
/// guard finishing at 06:00 has `off_min = 1800`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timetable {
    pub id: i64,
    pub name: String,
    pub on_min: i32,
    pub off_min: i32,
    /// Scans inside this window are arrivals.
    pub in_begin: i32,
    pub in_end: i32,
    /// Scans inside this window are departures.
    pub out_begin: i32,
    pub out_end: i32,
    pub late_grace: i32,
    pub early_grace: i32,
    pub break_min: i32,
    pub workday_value: f64,
    /// Overrides the on/off span when non-zero.
    pub work_minutes: i32,
    pub must_c_in: bool,
    pub must_c_out: bool,
    pub count_ot: bool,
    pub min_ot_block: i32,
    pub colour: String,
}

impl Timetable {
    /// Paid length of the block, before any break is deducted.
    pub fn span(&self) -> i32 {
        if self.work_minutes > 0 {
            self.work_minutes
        } else {
            self.off_min - self.on_min
        }
    }

    /// Does this block run past midnight?
    pub fn is_overnight(&self) -> bool {
        self.off_min > 24 * 60
    }

    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Timetable> {
        let t = |name: &str| -> rusqlite::Result<i32> {
            let raw: String = r.get(name)?;
            // A malformed clock value would otherwise become midnight and
            // silently turn a day into an eighteen-hour shift.
            parse_hhmm(&raw).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(Error::Invalid(format!(
                        "timetable '{}' has '{raw}' where a HH:MM time belongs",
                        r.get::<_, String>("name").unwrap_or_default()
                    ))),
                )
            })
        };

        let on = t("on_duty")?;
        let mut off = t("off_duty")?;
        if off <= on {
            off += 24 * 60; // crosses midnight
        }

        let mut in_begin = t("in_begin")?;
        let mut in_end = t("in_end")?;
        let mut out_begin = t("out_begin")?;
        let mut out_end = t("out_end")?;

        // The windows are stored as clock times. On an overnight block the ones
        // that fall after midnight have to be rolled forward onto the same
        // timeline as off_min, or a 00:30 boundary sorts before a 19:00 arrival
        // and every punch lands in the wrong bucket.
        if off > 24 * 60 {
            if in_begin > on {
                // an in-window that starts before on-duty is fine as-is
            }
            if in_end < on {
                in_end += 24 * 60;
            }
            if out_begin < on {
                out_begin += 24 * 60;
            }
            if out_end < on {
                out_end += 24 * 60;
            }
            if in_begin < on && in_begin + 24 * 60 <= in_end {
                in_begin += 24 * 60;
            }
        }

        Ok(Timetable {
            id: r.get("id")?,
            name: r.get("name")?,
            on_min: on,
            off_min: off,
            in_begin,
            in_end,
            out_begin,
            out_end,
            late_grace: r.get::<_, i64>("late_grace")? as i32,
            early_grace: r.get::<_, i64>("early_grace")? as i32,
            break_min: r.get::<_, i64>("break_min")? as i32,
            workday_value: r.get("workday_value")?,
            work_minutes: r.get::<_, i64>("work_minutes")? as i32,
            must_c_in: r.get::<_, i64>("must_c_in")? != 0,
            must_c_out: r.get::<_, i64>("must_c_out")? != 0,
            count_ot: r.get::<_, i64>("count_ot")? != 0,
            min_ot_block: r.get::<_, i64>("min_ot_block")? as i32,
            colour: r.get("colour")?,
        })
    }
}

/// A repeating cycle of days.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShiftCycle {
    pub id: i64,
    pub name: String,
    pub begin_date: String,
    pub cycle_num: i64,
    /// "Week" or "Month".
    pub cycle_unit: String,
}

#[derive(Debug, Clone)]
struct ScheduleRow {
    member_id: i64,
    shift_id: i64,
    start_date: String,
    end_date: Option<String>,
    is_temporary: bool,
}

/// What a member is expected to work on one day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayPlan {
    /// The cycle this came from, or `None` when nothing is assigned.
    pub shift_id: Option<i64>,
    /// Empty means a rest day.
    pub timetables: Vec<Timetable>,
}

impl DayPlan {
    pub fn rest() -> DayPlan {
        DayPlan { shift_id: None, timetables: Vec::new() }
    }
    pub fn is_rest(&self) -> bool {
        self.timetables.is_empty()
    }
}

/// The whole roster, in memory.
#[derive(Debug)]
pub struct Roster {
    timetables: HashMap<i64, Timetable>,
    shifts: HashMap<i64, ShiftCycle>,
    /// (shift_id, day_index) -> timetable ids, in start-time order.
    items: HashMap<(i64, i64), Vec<i64>>,
    /// Sorted by member then start_date, so the last match is the newest.
    schedules: Vec<ScheduleRow>,
    /// member_id -> the default shift of their department.
    dept_default: HashMap<i64, i64>,
    /// Two blocks closer together than this are one block with a gap.
    least_interval_min: i32,
}

impl Roster {
    /// Read everything the resolver needs in five queries.
    pub fn load(conn: &Connection) -> Result<Roster> {
        let mut timetables = HashMap::new();
        {
            let mut s = conn.prepare("SELECT * FROM timetables")?;
            let rows = s.query_map([], Timetable::from_row)?;
            for t in rows {
                let t = t?;
                timetables.insert(t.id, t);
            }
        }

        let mut shifts = HashMap::new();
        {
            let mut s =
                conn.prepare("SELECT id, name, begin_date, cycle_num, cycle_unit FROM shifts")?;
            let rows = s.query_map([], |r| {
                Ok(ShiftCycle {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    begin_date: r.get(2)?,
                    cycle_num: r.get(3)?,
                    cycle_unit: r.get(4)?,
                })
            })?;
            for c in rows {
                let c = c?;
                shifts.insert(c.id, c);
            }
        }

        let mut items: HashMap<(i64, i64), Vec<i64>> = HashMap::new();
        {
            let mut s = conn.prepare(
                "SELECT si.shift_id, si.day_index, si.timetable_id
                 FROM shift_items si
                 JOIN timetables t ON t.id = si.timetable_id
                 ORDER BY si.shift_id, si.day_index, t.on_duty",
            )?;
            let rows = s.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            })?;
            for row in rows {
                let (shift, day, tt) = row?;
                items.entry((shift, day)).or_default().push(tt);
            }
        }

        let mut schedules = Vec::new();
        {
            let mut s = conn.prepare(
                "SELECT member_id, shift_id, start_date, end_date, is_temporary
                 FROM employee_schedules
                 ORDER BY member_id, start_date, id",
            )?;
            let rows = s.query_map([], |r| {
                Ok(ScheduleRow {
                    member_id: r.get(0)?,
                    shift_id: r.get(1)?,
                    start_date: r.get(2)?,
                    end_date: r.get(3)?,
                    is_temporary: r.get::<_, i64>(4)? != 0,
                })
            })?;
            for row in rows {
                schedules.push(row?);
            }
        }

        let mut dept_default = HashMap::new();
        {
            let mut s = conn.prepare(
                "SELECT m.id, d.default_shift_id FROM members m
                 JOIN departments d ON d.id = m.dept_id
                 WHERE d.default_shift_id IS NOT NULL",
            )?;
            let rows = s.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (m, s) = row?;
                dept_default.insert(m, s);
            }
        }

        let least_interval_min: i32 = conn
            .query_row("SELECT least_shift_interval_min FROM attendance_rules WHERE id=1", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(30) as i32;

        Ok(Roster { timetables, shifts, items, schedules, dept_default, least_interval_min })
    }

    pub fn timetable(&self, id: i64) -> Option<&Timetable> {
        self.timetables.get(&id)
    }
    pub fn shift(&self, id: i64) -> Option<&ShiftCycle> {
        self.shifts.get(&id)
    }

    /// Which shift governs this member on this date.
    ///
    /// A temporary assignment wins over a permanent one covering the same day —
    /// that is the whole point of marking it temporary: exam week is arranged
    /// without disturbing the standing contract underneath it. Among rows of
    /// equal standing, the one that started most recently wins.
    pub fn shift_for(&self, member_id: i64, date: &str) -> Option<i64> {
        let covers = |r: &ScheduleRow| {
            r.member_id == member_id
                && date >= r.start_date.as_str()
                // `is_none_or` needs a newer compiler than this crate targets.
                && r.end_date.as_deref().map_or(true, |e| date <= e)
        };

        // `schedules` is sorted by start_date, so searching from the back
        // finds the most recently started row that covers this date.
        let pick = |temporary: bool| {
            self.schedules
                .iter()
                .rfind(|r| covers(r) && r.is_temporary == temporary)
                .map(|r| r.shift_id)
        };

        pick(true)
            .or_else(|| pick(false))
            .or_else(|| self.dept_default.get(&member_id).copied())
    }

    /// The full plan for one member on one day.
    pub fn plan_for(&self, member_id: i64, date: &str) -> Result<DayPlan> {
        let Some(shift_id) = self.shift_for(member_id, date) else {
            return Ok(DayPlan::rest());
        };
        let Some(cycle) = self.shifts.get(&shift_id) else {
            return Ok(DayPlan::rest());
        };

        let day_index = day_index_in_cycle(cycle, date)?;
        let ids = match self.items.get(&(shift_id, day_index)) {
            Some(v) => v,
            None => return Ok(DayPlan { shift_id: Some(shift_id), timetables: Vec::new() }),
        };

        let mut blocks: Vec<Timetable> =
            ids.iter().filter_map(|id| self.timetables.get(id).cloned()).collect();
        blocks.sort_by_key(|t| t.on_min);

        Ok(DayPlan { shift_id: Some(shift_id), timetables: self.merge_close_blocks(blocks) })
    }

    /// Fold blocks separated by less than the minimum interval into one.
    ///
    /// A morning block ending 12:00 and an afternoon block starting 12:15 is
    /// not two duties with a break between them — it is one duty and a walk to
    /// the staff room. Treating them separately would charge the person with
    /// leaving early at noon and arriving late at quarter past.
    fn merge_close_blocks(&self, blocks: Vec<Timetable>) -> Vec<Timetable> {
        let mut out: Vec<Timetable> = Vec::with_capacity(blocks.len());
        for b in blocks {
            match out.last_mut() {
                Some(prev) if b.on_min - prev.off_min < self.least_interval_min => {
                    // The gap becomes an unpaid break rather than vanishing.
                    let gap = (b.on_min - prev.off_min).max(0);
                    prev.name = format!("{} + {}", prev.name, b.name);
                    prev.off_min = b.off_min.max(prev.off_min);
                    prev.out_begin = b.out_begin;
                    prev.out_end = b.out_end;
                    prev.early_grace = b.early_grace;
                    prev.break_min += b.break_min + gap;
                    prev.workday_value += b.workday_value;
                    if prev.work_minutes > 0 || b.work_minutes > 0 {
                        prev.work_minutes = prev.span() + b.span();
                    }
                    prev.must_c_out = b.must_c_out;
                    prev.count_ot = prev.count_ot || b.count_ot;
                }
                _ => out.push(b),
            }
        }
        out
    }
}

/// Where a date falls inside a shift's cycle.
///
/// For the ordinary case — a one-week cycle — this is simply the weekday, so a
/// plan reads exactly as it looks on screen: index 0 is Sunday. Longer cycles
/// count whole weeks or months from the shift's begin date.
pub fn day_index_in_cycle(cycle: &ShiftCycle, date: &str) -> Result<i64> {
    let (y, m, d) = parse_iso(date)?;
    let days = days_from_civil_pub(y, m, d);
    let cycles = cycle.cycle_num.max(1);

    if cycle.cycle_unit.eq_ignore_ascii_case("month") {
        let (by, bm, _) = parse_iso(&cycle.begin_date).unwrap_or((y, m, 1));
        let months = (y as i64 - by as i64) * 12 + (m as i64 - bm as i64);
        let offset = months.rem_euclid(cycles);
        // 31 slots per month so that the 31st of a long month has somewhere to
        // live; short months simply leave the tail slots empty.
        return Ok(offset * 31 + (d as i64 - 1));
    }

    // Week cycles. Weeks are counted from the Sunday that starts them, so a
    // cycle beginning mid-week still lines up with the grid on screen.
    let weekday = (days + 4).rem_euclid(7); // 1970-01-01 was a Thursday
    if cycles == 1 {
        return Ok(weekday);
    }

    let week_of = |n: i64| (n - 3).div_euclid(7);
    let (by, bm, bd) = parse_iso(&cycle.begin_date).unwrap_or((y, m, d));
    let begin_days = days_from_civil_pub(by, bm, bd);
    let offset = (week_of(days) - week_of(begin_days)).rem_euclid(cycles);
    Ok(offset * 7 + weekday)
}

fn parse_iso(s: &str) -> Result<(i32, u32, u32)> {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 3 {
        return Err(Error::Invalid(format!("'{s}' is not a YYYY-MM-DD date")));
    }
    let y = p[0].parse::<i32>().map_err(|_| Error::Invalid(format!("bad year in '{s}'")))?;
    let m = p[1].parse::<u32>().map_err(|_| Error::Invalid(format!("bad month in '{s}'")))?;
    let d = p[2].parse::<u32>().map_err(|_| Error::Invalid(format!("bad day in '{s}'")))?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return Err(Error::Invalid(format!("'{s}' is not a real date")));
    }
    Ok((y, m, d))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::params;

    fn roster() -> (Connection, Roster) {
        let conn = db::open_memory().unwrap();
        let r = Roster::load(&conn).unwrap();
        (conn, r)
    }

    #[test]
    fn the_seeded_school_week_resolves() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO members (id, enroll_no, full_name, dept_id) VALUES (900, 9000, 'A', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO employee_schedules (member_id, shift_id, start_date)
             VALUES (900, 1, '2020-01-01')",
            [],
        )
        .unwrap();
        let r = Roster::load(&conn).unwrap();

        // 2026-08-19 is a Wednesday: a working day.
        let wed = r.plan_for(900, "2026-08-19").unwrap();
        assert!(!wed.is_rest(), "Wednesday should be a working day");
        assert_eq!(wed.timetables.len(), 1);
        assert_eq!(wed.timetables[0].on_min, 9 * 60);

        // 2026-08-22 is a Saturday: the school holiday, no shift_items row.
        let sat = r.plan_for(900, "2026-08-22").unwrap();
        assert!(sat.is_rest(), "Saturday should be a rest day");
        assert_eq!(sat.shift_id, Some(1), "still on the shift, just not working");
    }

    #[test]
    fn a_temporary_schedule_beats_the_standing_one() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO members (id, enroll_no, full_name, dept_id) VALUES (901, 9001, 'B', 1)",
            [],
        )
        .unwrap();
        // Standing assignment: regular week.
        conn.execute(
            "INSERT INTO employee_schedules (member_id, shift_id, start_date, is_temporary)
             VALUES (901, 1, '2020-01-01', 0)",
            [],
        )
        .unwrap();
        // Exam week only.
        conn.execute(
            "INSERT INTO employee_schedules (member_id, shift_id, start_date, end_date, is_temporary)
             VALUES (901, 3, '2026-08-17', '2026-08-21', 1)",
            [],
        )
        .unwrap();
        let r = Roster::load(&conn).unwrap();

        assert_eq!(r.shift_for(901, "2026-08-19"), Some(3), "exam week must win");
        // The day before and the day after fall back to the standing shift.
        assert_eq!(r.shift_for(901, "2026-08-16"), Some(1));
        assert_eq!(r.shift_for(901, "2026-08-24"), Some(1));
    }

    #[test]
    fn the_most_recent_standing_assignment_wins() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO members (id, enroll_no, full_name, dept_id) VALUES (902, 9002, 'C', 1)",
            [],
        )
        .unwrap();
        for (shift, start) in [(1i64, "2020-01-01"), (2, "2026-01-01")] {
            conn.execute(
                "INSERT INTO employee_schedules (member_id, shift_id, start_date)
                 VALUES (902, ?1, ?2)",
                params![shift, start],
            )
            .unwrap();
        }
        let r = Roster::load(&conn).unwrap();
        assert_eq!(r.shift_for(902, "2026-08-19"), Some(2), "the newer contract governs");
        assert_eq!(r.shift_for(902, "2021-06-01"), Some(1), "history keeps the old one");
    }

    #[test]
    fn a_member_with_no_schedule_falls_back_to_their_department() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO members (id, enroll_no, full_name, dept_id) VALUES (903, 9003, 'D', 1)",
            [],
        )
        .unwrap();
        let r = Roster::load(&conn).unwrap();
        let dept_shift: i64 = conn
            .query_row("SELECT default_shift_id FROM departments WHERE id=1", [], |x| x.get(0))
            .unwrap();
        assert_eq!(r.shift_for(903, "2026-08-19"), Some(dept_shift));
    }

    #[test]
    fn a_member_with_nothing_at_all_gets_a_rest_day_not_an_error() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO members (id, enroll_no, full_name) VALUES (904, 9004, 'E')",
            [],
        )
        .unwrap();
        let r = Roster::load(&conn).unwrap();
        let plan = r.plan_for(904, "2026-08-19").unwrap();
        assert!(plan.is_rest());
        assert_eq!(plan.shift_id, None);
    }

    #[test]
    fn a_weekly_cycle_of_one_indexes_by_weekday() {
        let c = ShiftCycle {
            id: 1,
            name: "w".into(),
            begin_date: "2026-08-01".into(),
            cycle_num: 1,
            cycle_unit: "Week".into(),
        };
        // 2026-08-16 Sunday .. 2026-08-22 Saturday
        for (i, d) in [
            "2026-08-16", "2026-08-17", "2026-08-18", "2026-08-19", "2026-08-20", "2026-08-21",
            "2026-08-22",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(day_index_in_cycle(&c, d).unwrap(), i as i64, "{d}");
        }
    }

    #[test]
    fn a_two_week_cycle_alternates() {
        let c = ShiftCycle {
            id: 1,
            name: "w2".into(),
            begin_date: "2026-08-16".into(), // a Sunday
            cycle_num: 2,
            cycle_unit: "Week".into(),
        };
        // First week: indices 0-6. Second week: 7-13. Third week wraps to 0.
        assert_eq!(day_index_in_cycle(&c, "2026-08-16").unwrap(), 0);
        assert_eq!(day_index_in_cycle(&c, "2026-08-19").unwrap(), 3);
        assert_eq!(day_index_in_cycle(&c, "2026-08-23").unwrap(), 7);
        assert_eq!(day_index_in_cycle(&c, "2026-08-26").unwrap(), 10);
        assert_eq!(day_index_in_cycle(&c, "2026-08-30").unwrap(), 0, "cycle repeats");
    }

    #[test]
    fn a_date_before_the_cycle_began_still_resolves() {
        // Recomputing history must not panic or produce a negative index.
        let c = ShiftCycle {
            id: 1,
            name: "w2".into(),
            begin_date: "2026-08-16".into(),
            cycle_num: 2,
            cycle_unit: "Week".into(),
        };
        let i = day_index_in_cycle(&c, "2026-08-09").unwrap();
        assert!((0..14).contains(&i), "index {i} out of range");
        assert_eq!(i, 7, "one week before a two-week cycle is the other half");
    }

    #[test]
    fn a_monthly_cycle_indexes_by_day_of_month() {
        let c = ShiftCycle {
            id: 1,
            name: "m".into(),
            begin_date: "2026-01-01".into(),
            cycle_num: 1,
            cycle_unit: "Month".into(),
        };
        assert_eq!(day_index_in_cycle(&c, "2026-08-01").unwrap(), 0);
        assert_eq!(day_index_in_cycle(&c, "2026-08-31").unwrap(), 30);
    }

    #[test]
    fn an_overnight_block_puts_its_windows_on_one_timeline() {
        let (conn, _) = roster();
        let r = Roster::load(&conn).unwrap();
        let night = r.timetables.values().find(|t| t.name == "Night Guard").unwrap();

        assert!(night.is_overnight(), "19:00-06:00 crosses midnight");
        assert_eq!(night.on_min, 19 * 60);
        assert_eq!(night.off_min, 30 * 60, "06:00 the next morning is minute 1800");
        // The out-window must sit after the on-duty time, not eight hours
        // before it, or every departure lands outside its own window.
        assert!(
            night.out_end > night.on_min,
            "out window ends at {} but duty starts at {}",
            night.out_end,
            night.on_min
        );
    }

    #[test]
    fn two_blocks_a_few_minutes_apart_become_one_duty() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO timetables (id, name, on_duty, off_duty, in_begin, in_end,
                                     out_begin, out_end, break_min, workday_value)
             VALUES (50,'Morning half','08:00','12:00','06:00','09:00','11:00','12:30',0,0.5),
                    (51,'Afternoon half','12:15','16:00','12:00','13:00','15:00','18:00',0,0.5)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO shifts (id, name) VALUES (50, 'Split day')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO shift_items (shift_id, day_index, timetable_id)
             VALUES (50, 3, 50), (50, 3, 51)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO members (id, enroll_no, full_name) VALUES (905, 9005, 'F')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO employee_schedules (member_id, shift_id, start_date)
             VALUES (905, 50, '2020-01-01')",
            [],
        )
        .unwrap();

        let r = Roster::load(&conn).unwrap();
        // 2026-08-19 is a Wednesday, day_index 3.
        let plan = r.plan_for(905, "2026-08-19").unwrap();
        assert_eq!(plan.timetables.len(), 1, "a 15-minute gap is a break, not a second duty");

        let d = &plan.timetables[0];
        assert_eq!(d.on_min, 8 * 60);
        assert_eq!(d.off_min, 16 * 60);
        assert_eq!(d.break_min, 15, "the gap between the halves is unpaid");
        assert_eq!(d.workday_value, 1.0, "two halves make a whole day");
    }

    #[test]
    fn blocks_with_a_real_gap_stay_separate() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO timetables (id, name, on_duty, off_duty, in_begin, in_end,
                                     out_begin, out_end)
             VALUES (60,'Early','06:00','09:00','05:00','06:30','08:30','10:00'),
                    (61,'Evening','17:00','20:00','16:00','17:30','19:30','21:00')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO shifts (id, name) VALUES (60, 'Two duties')", []).unwrap();
        conn.execute(
            "INSERT INTO shift_items (shift_id, day_index, timetable_id)
             VALUES (60, 3, 60), (60, 3, 61)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO members (id, enroll_no, full_name) VALUES (906, 9006, 'G')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO employee_schedules (member_id, shift_id, start_date)
             VALUES (906, 60, '2020-01-01')",
            [],
        )
        .unwrap();

        let r = Roster::load(&conn).unwrap();
        let plan = r.plan_for(906, "2026-08-19").unwrap();
        assert_eq!(plan.timetables.len(), 2, "eight hours apart is two separate duties");
    }

    #[test]
    fn a_broken_clock_value_names_the_timetable_rather_than_defaulting() {
        let (conn, _) = roster();
        conn.execute(
            "INSERT INTO timetables (id, name, on_duty, off_duty) VALUES (70,'Broken','9am','5pm')",
            [],
        )
        .unwrap();
        let e = Roster::load(&conn).unwrap_err().to_string();
        assert!(e.contains("Broken"), "the error should name the timetable: {e}");
    }
}
