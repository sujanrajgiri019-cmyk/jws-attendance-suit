//! The attendance rule set: one row, read on every recompute.
//!
//! These are the settings behind the four sub-tabs of the Attendance Rules
//! screen. They live in a single wide row rather than the key/value `rules`
//! table because the engine reads all of them together, SQLite can enforce
//! their types and ranges, and a mistyped column name is then a loud error
//! rather than a rule that silently falls back to a default.
//!
//! Everything here is plain data. The decisions it drives live in
//! [`crate::rules`], and the schedule it is applied to lives in
//! [`crate::schedule`].

use crate::{Error, Result};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

/// What to do with a punch the terminal tagged as an out-of-office or overtime
/// scan. Schools use these keys inconsistently, so every option is offered
/// rather than assuming the staff press the right one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateHandling {
    /// Treat it as an ordinary scan; ordering decides what it means.
    Ignore,
    /// Honour the key: this really was a departure / overtime scan.
    AsMarked,
    /// Count it as official business away from school, not an absence.
    AsBusinessOut,
    /// Record it but leave it for a human to confirm.
    Audit,
}

impl StateHandling {
    pub fn as_str(&self) -> &'static str {
        match self {
            StateHandling::Ignore => "ignore",
            StateHandling::AsMarked => "as_out",
            StateHandling::AsBusinessOut => "as_business_out",
            StateHandling::Audit => "audit",
        }
    }
    fn parse(s: &str) -> StateHandling {
        match s {
            "ignore" => StateHandling::Ignore,
            "as_business_out" => StateHandling::AsBusinessOut,
            "audit" => StateHandling::Audit,
            // "as_out" and "as_ot" are the same choice on two different tabs.
            _ => StateHandling::AsMarked,
        }
    }
}

/// How a fractional workday or hour figure is snapped to the reporting unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rounding {
    Down,
    /// Nearest, halves away from zero.
    Off,
    Up,
}

impl Rounding {
    pub fn as_str(&self) -> &'static str {
        match self {
            Rounding::Down => "down",
            Rounding::Off => "off",
            Rounding::Up => "up",
        }
    }
    fn parse(s: &str) -> Rounding {
        match s {
            "down" => Rounding::Down,
            "up" => Rounding::Up,
            _ => Rounding::Off,
        }
    }
}

/// The complete rule set. Field order follows the four sub-tabs so the screen
/// and the struct can be read side by side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttendanceRules {
    // --- Basic settings -----------------------------------------------------
    pub unit_name: String,
    pub unit_abbr: String,
    /// 0 = Sunday .. 6 = Saturday.
    pub week_start: u32,
    /// Day of the month an attendance month is counted from.
    pub month_start_day: u32,
    /// Which calendar day owns a shift that runs past midnight.
    pub cross_day_belongs_to_first: bool,
    /// A span longer than this cannot be one person's working day; it is two
    /// days run together by a missed scan.
    pub longest_zone_min: i32,
    /// A span shorter than this is someone testing the sensor, not a day.
    pub shortest_zone_min: i32,
    /// Two blocks of duty closer together than this are treated as one.
    pub least_shift_interval_min: i32,
    pub out_state: StateHandling,
    pub ot_state: StateHandling,

    // --- Calculation --------------------------------------------------------
    /// Clock minutes that constitute one full workday.
    pub workday_minutes: i32,
    /// Arriving more than this many minutes after on-duty counts as late.
    pub late_after_min: i32,
    /// Leaving more than this many minutes before off-duty counts as early.
    pub early_after_min: i32,

    pub no_clock_in_enabled: bool,
    /// "Late" or "Absent".
    pub no_clock_in_as: String,
    /// Minutes to charge when the arrival scan is missing.
    pub no_clock_in_min: i32,
    pub no_clock_out_enabled: bool,
    /// "EarlyLeave" or "Absent".
    pub no_clock_out_as: String,
    pub no_clock_out_min: i32,

    pub late_to_absent_enabled: bool,
    pub late_to_absent_min: i32,
    pub early_to_absent_enabled: bool,
    pub early_to_absent_min: i32,
    /// Late by more than this, but less than the absent threshold: half day.
    pub half_day_after_min: i32,
    /// Worked minutes below this earn half a day however punctual the arrival.
    pub min_full_day_min: i32,

    pub ot_after_shift_enabled: bool,
    /// Staying at least this long past off-duty starts counting as overtime.
    pub ot_after_shift_min: i32,
    pub ot_before_shift_enabled: bool,
    pub ot_before_shift_min: i32,
    /// Nobody works more overtime than this in one day; beyond it, the scan is
    /// wrong rather than the person heroic.
    pub ot_max_daily_min: i32,

    /// Repeat scans inside this many seconds are one press of the sensor.
    pub dedupe_secs: i32,
    /// A day with a single scan is half a day rather than an absence.
    pub lone_punch_half_day: bool,

    // --- Statistic items ----------------------------------------------------
    pub sym_normal: String,
    pub sym_late: String,
    pub sym_early: String,
    pub sym_absent: String,
    pub sym_ot: String,
    pub sym_leave: String,
    pub sym_holiday: String,
    pub sym_half_day: String,
    pub sym_missing: String,

    /// Smallest reportable unit, e.g. 0.5 of a workday.
    pub min_unit: f64,
    /// "workday" or "hours".
    pub min_unit_basis: String,
    pub rounding: Rounding,
    /// Count occurrences as well as minutes.
    pub acc_by_times: bool,
    /// Round once at the end of the period rather than on each day. Rounding
    /// daily and then adding is how a month drifts by several hours.
    pub round_at_acc: bool,
    pub group_by_periods: bool,

    // --- Weekend set --------------------------------------------------------
    /// Index 0 = Sunday .. 6 = Saturday.
    pub weekend_days: [bool; 7],
    pub weekend_as_ot: bool,
    pub weekend_symbol: String,
    pub weekend_colour: String,
}

impl Default for AttendanceRules {
    fn default() -> Self {
        AttendanceRules {
            unit_name: "Janapremi World School".into(),
            unit_abbr: "JWS".into(),
            week_start: 0,
            month_start_day: 1,
            cross_day_belongs_to_first: true,
            longest_zone_min: 1440,
            shortest_zone_min: 30,
            least_shift_interval_min: 30,
            out_state: StateHandling::AsMarked,
            ot_state: StateHandling::AsMarked,

            workday_minutes: 420,
            late_after_min: 10,
            early_after_min: 10,
            no_clock_in_enabled: true,
            no_clock_in_as: "Absent".into(),
            no_clock_in_min: 0,
            no_clock_out_enabled: true,
            no_clock_out_as: "EarlyLeave".into(),
            no_clock_out_min: 0,
            late_to_absent_enabled: true,
            late_to_absent_min: 240,
            early_to_absent_enabled: true,
            early_to_absent_min: 240,
            half_day_after_min: 120,
            min_full_day_min: 350,
            ot_after_shift_enabled: true,
            ot_after_shift_min: 30,
            ot_before_shift_enabled: false,
            ot_before_shift_min: 30,
            ot_max_daily_min: 240,
            dedupe_secs: 60,
            lone_punch_half_day: true,

            sym_normal: "P".into(),
            sym_late: "L".into(),
            sym_early: "E".into(),
            sym_absent: "A".into(),
            sym_ot: "O".into(),
            sym_leave: "V".into(),
            sym_holiday: "H".into(),
            sym_half_day: "HD".into(),
            sym_missing: "?".into(),

            min_unit: 0.5,
            min_unit_basis: "workday".into(),
            rounding: Rounding::Off,
            acc_by_times: false,
            round_at_acc: true,
            group_by_periods: false,

            // Saturday is the school's weekly holiday.
            weekend_days: [false, false, false, false, false, false, true],
            weekend_as_ot: true,
            weekend_symbol: "W".into(),
            weekend_colour: "#94A3B8".into(),
        }
    }
}

impl AttendanceRules {
    /// Is this weekday (0 = Sunday) a rest day?
    pub fn is_weekend(&self, weekday: u32) -> bool {
        self.weekend_days.get(weekday as usize).copied().unwrap_or(false)
    }

    /// Snap a workday figure to the reporting unit.
    ///
    /// With a minimum unit of 0.5 and round-off, 0.6 of a day becomes 0.5 and
    /// 0.8 becomes 1.0. A minimum unit of zero or less would divide by zero, so
    /// the value is returned untouched.
    pub fn round_unit(&self, value: f64) -> f64 {
        if self.min_unit <= 0.0 || !value.is_finite() {
            return value;
        }
        let n = value / self.min_unit;
        let snapped = match self.rounding {
            Rounding::Down => n.floor(),
            Rounding::Up => n.ceil(),
            // `round` in Rust is half-away-from-zero, which is what a payroll
            // sheet expects: 0.25 of a day at a 0.5 unit rounds up, not to even.
            Rounding::Off => n.round(),
        };
        snapped * self.min_unit
    }

    /// The longest plausible working span. A day beyond this was produced by a
    /// missed scan, not by someone who stayed for thirty hours.
    pub fn span_is_plausible(&self, minutes: i32) -> bool {
        minutes >= self.shortest_zone_min && minutes <= self.longest_zone_min
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Read the single row. A database that somehow lost it gets the defaults
    /// rather than an error, because a missing rules row must not stop the
    /// office seeing today's attendance.
    pub fn load(conn: &Connection) -> Result<AttendanceRules> {
        let found = conn
            .query_row("SELECT * FROM attendance_rules WHERE id = 1", [], Self::from_row)
            .ok();
        Ok(found.unwrap_or_default())
    }

    fn from_row(r: &Row<'_>) -> rusqlite::Result<AttendanceRules> {
        let b = |name: &str| -> rusqlite::Result<bool> { Ok(r.get::<_, i64>(name)? != 0) };
        Ok(AttendanceRules {
            unit_name: r.get("unit_name")?,
            unit_abbr: r.get("unit_abbr")?,
            week_start: r.get::<_, i64>("week_start")? as u32,
            month_start_day: r.get::<_, i64>("month_start_day")? as u32,
            cross_day_belongs_to_first: r.get::<_, String>("cross_day_belongs")? == "first",
            longest_zone_min: r.get::<_, i64>("longest_zone_min")? as i32,
            shortest_zone_min: r.get::<_, i64>("shortest_zone_min")? as i32,
            least_shift_interval_min: r.get::<_, i64>("least_shift_interval_min")? as i32,
            out_state: StateHandling::parse(&r.get::<_, String>("out_state")?),
            ot_state: StateHandling::parse(&r.get::<_, String>("ot_state")?),

            workday_minutes: r.get::<_, i64>("workday_minutes")? as i32,
            late_after_min: r.get::<_, i64>("late_after_min")? as i32,
            early_after_min: r.get::<_, i64>("early_after_min")? as i32,
            no_clock_in_enabled: b("no_clock_in_enabled")?,
            no_clock_in_as: r.get("no_clock_in_as")?,
            no_clock_in_min: r.get::<_, i64>("no_clock_in_min")? as i32,
            no_clock_out_enabled: b("no_clock_out_enabled")?,
            no_clock_out_as: r.get("no_clock_out_as")?,
            no_clock_out_min: r.get::<_, i64>("no_clock_out_min")? as i32,
            late_to_absent_enabled: b("late_to_absent_enabled")?,
            late_to_absent_min: r.get::<_, i64>("late_to_absent_min")? as i32,
            early_to_absent_enabled: b("early_to_absent_enabled")?,
            early_to_absent_min: r.get::<_, i64>("early_to_absent_min")? as i32,
            half_day_after_min: r.get::<_, i64>("half_day_after_min")? as i32,
            min_full_day_min: r.get::<_, i64>("min_full_day_min")? as i32,
            ot_after_shift_enabled: b("ot_after_shift_enabled")?,
            ot_after_shift_min: r.get::<_, i64>("ot_after_shift_min")? as i32,
            ot_before_shift_enabled: b("ot_before_shift_enabled")?,
            ot_before_shift_min: r.get::<_, i64>("ot_before_shift_min")? as i32,
            ot_max_daily_min: r.get::<_, i64>("ot_max_daily_min")? as i32,
            dedupe_secs: r.get::<_, i64>("dedupe_secs")? as i32,
            lone_punch_half_day: b("lone_punch_half_day")?,

            sym_normal: r.get("sym_normal")?,
            sym_late: r.get("sym_late")?,
            sym_early: r.get("sym_early")?,
            sym_absent: r.get("sym_absent")?,
            sym_ot: r.get("sym_ot")?,
            sym_leave: r.get("sym_leave")?,
            sym_holiday: r.get("sym_holiday")?,
            sym_half_day: r.get("sym_half_day")?,
            sym_missing: r.get("sym_missing")?,

            min_unit: r.get("min_unit")?,
            min_unit_basis: r.get("min_unit_basis")?,
            rounding: Rounding::parse(&r.get::<_, String>("rounding")?),
            acc_by_times: b("acc_by_times")?,
            round_at_acc: b("round_at_acc")?,
            group_by_periods: b("group_by_periods")?,

            weekend_days: [
                b("weekend_sun")?,
                b("weekend_mon")?,
                b("weekend_tue")?,
                b("weekend_wed")?,
                b("weekend_thu")?,
                b("weekend_fri")?,
                b("weekend_sat")?,
            ],
            weekend_as_ot: b("weekend_as_ot")?,
            weekend_symbol: r.get("weekend_symbol")?,
            weekend_colour: r.get("weekend_colour")?,
        })
    }

    /// Validate and write the single row.
    ///
    /// Validation happens here rather than in the screen because the screen is
    /// not the only caller, and a rule set that cannot produce a sensible day
    /// is worse than a rejected save.
    pub fn save(&self, conn: &Connection) -> Result<()> {
        self.validate()?;

        conn.execute(
            "UPDATE attendance_rules SET
                unit_name=?1, unit_abbr=?2, week_start=?3, month_start_day=?4,
                cross_day_belongs=?5, longest_zone_min=?6, shortest_zone_min=?7,
                least_shift_interval_min=?8, out_state=?9, ot_state=?10,

                workday_minutes=?11, late_after_min=?12, early_after_min=?13,
                no_clock_in_enabled=?14, no_clock_in_as=?15, no_clock_in_min=?16,
                no_clock_out_enabled=?17, no_clock_out_as=?18, no_clock_out_min=?19,
                late_to_absent_enabled=?20, late_to_absent_min=?21,
                early_to_absent_enabled=?22, early_to_absent_min=?23,
                half_day_after_min=?24, min_full_day_min=?25,
                ot_after_shift_enabled=?26, ot_after_shift_min=?27,
                ot_before_shift_enabled=?28, ot_before_shift_min=?29,
                ot_max_daily_min=?30, dedupe_secs=?31, lone_punch_half_day=?32,

                sym_normal=?33, sym_late=?34, sym_early=?35, sym_absent=?36,
                sym_ot=?37, sym_leave=?38, sym_holiday=?39, sym_half_day=?40,
                sym_missing=?41,
                min_unit=?42, min_unit_basis=?43, rounding=?44,
                acc_by_times=?45, round_at_acc=?46, group_by_periods=?47,

                weekend_sun=?48, weekend_mon=?49, weekend_tue=?50, weekend_wed=?51,
                weekend_thu=?52, weekend_fri=?53, weekend_sat=?54,
                weekend_as_ot=?55, weekend_symbol=?56, weekend_colour=?57,
                updated_at=datetime('now','localtime')
             WHERE id = 1",
            params![
                self.unit_name,
                self.unit_abbr,
                self.week_start as i64,
                self.month_start_day as i64,
                if self.cross_day_belongs_to_first { "first" } else { "second" },
                self.longest_zone_min as i64,
                self.shortest_zone_min as i64,
                self.least_shift_interval_min as i64,
                // The two columns share one enum but different CHECK lists.
                self.out_state.as_str(),
                if self.ot_state == StateHandling::AsMarked { "as_ot" } else { self.ot_state.as_str() },
                self.workday_minutes as i64,
                self.late_after_min as i64,
                self.early_after_min as i64,
                self.no_clock_in_enabled as i64,
                self.no_clock_in_as,
                self.no_clock_in_min as i64,
                self.no_clock_out_enabled as i64,
                self.no_clock_out_as,
                self.no_clock_out_min as i64,
                self.late_to_absent_enabled as i64,
                self.late_to_absent_min as i64,
                self.early_to_absent_enabled as i64,
                self.early_to_absent_min as i64,
                self.half_day_after_min as i64,
                self.min_full_day_min as i64,
                self.ot_after_shift_enabled as i64,
                self.ot_after_shift_min as i64,
                self.ot_before_shift_enabled as i64,
                self.ot_before_shift_min as i64,
                self.ot_max_daily_min as i64,
                self.dedupe_secs as i64,
                self.lone_punch_half_day as i64,
                self.sym_normal,
                self.sym_late,
                self.sym_early,
                self.sym_absent,
                self.sym_ot,
                self.sym_leave,
                self.sym_holiday,
                self.sym_half_day,
                self.sym_missing,
                self.min_unit,
                self.min_unit_basis,
                self.rounding.as_str(),
                self.acc_by_times as i64,
                self.round_at_acc as i64,
                self.group_by_periods as i64,
                self.weekend_days[0] as i64,
                self.weekend_days[1] as i64,
                self.weekend_days[2] as i64,
                self.weekend_days[3] as i64,
                self.weekend_days[4] as i64,
                self.weekend_days[5] as i64,
                self.weekend_days[6] as i64,
                self.weekend_as_ot as i64,
                self.weekend_symbol,
                self.weekend_colour,
            ],
        )?;
        Ok(())
    }

    /// Reject rule sets that cannot produce a sensible day.
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(Error::Invalid(m));

        if self.week_start > 6 {
            return bad("A week has to start on one of the seven days.".into());
        }
        if !(1..=31).contains(&self.month_start_day) {
            return bad("A month has to start on a day between 1 and 31.".into());
        }
        if self.min_unit <= 0.0 {
            return bad("The minimum reporting unit must be greater than zero.".into());
        }
        if self.shortest_zone_min >= self.longest_zone_min {
            return bad(format!(
                "The shortest working span ({} min) must be less than the longest ({} min).",
                self.shortest_zone_min, self.longest_zone_min
            ));
        }
        if self.workday_minutes <= 0 {
            return bad("A workday has to be longer than zero minutes.".into());
        }
        // The order of these three thresholds is the whole grading scale. Out
        // of order, a member of staff can be marked absent for arriving before
        // they are marked late, which is how a rule screen quietly destroys a
        // month of records.
        if self.late_to_absent_enabled && self.half_day_after_min >= self.late_to_absent_min {
            return bad(format!(
                "Half day after {} min must come before absent after {} min — \
                 otherwise nobody is ever recorded as a half day.",
                self.half_day_after_min, self.late_to_absent_min
            ));
        }
        if self.min_full_day_min > self.workday_minutes {
            return bad(format!(
                "A full day needs {} worked minutes but the workday is only {} minutes long, \
                 so every member of staff would be marked as a half day.",
                self.min_full_day_min, self.workday_minutes
            ));
        }
        if self.weekend_days.iter().all(|d| *d) {
            return bad("Every day cannot be a weekend.".into());
        }
        if self.dedupe_secs < 0 {
            return bad("The repeat-scan window cannot be negative.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn defaults_survive_a_round_trip_through_the_database() {
        let conn = db::open_memory().unwrap();
        let loaded = AttendanceRules::load(&conn).unwrap();
        assert_eq!(loaded, AttendanceRules::default(), "seeded row must match the defaults");
    }

    #[test]
    fn every_field_survives_a_round_trip() {
        // Sixty columns mapped by hand is sixty chances to write the wrong
        // name. Saving a value that differs from the default in every single
        // field and reading it back is the only way to catch a crossed wire.
        let conn = db::open_memory().unwrap();
        let mut r = AttendanceRules::default();

        r.unit_name = "Test School".into();
        r.unit_abbr = "TS".into();
        r.week_start = 1;
        r.month_start_day = 16;
        r.cross_day_belongs_to_first = false;
        r.longest_zone_min = 1200;
        r.shortest_zone_min = 45;
        r.least_shift_interval_min = 60;
        r.out_state = StateHandling::Audit;
        r.ot_state = StateHandling::AsBusinessOut;

        r.workday_minutes = 480;
        r.late_after_min = 7;
        r.early_after_min = 8;
        r.no_clock_in_enabled = false;
        r.no_clock_in_as = "Late".into();
        r.no_clock_in_min = 25;
        r.no_clock_out_enabled = false;
        r.no_clock_out_as = "Absent".into();
        r.no_clock_out_min = 35;
        r.late_to_absent_enabled = false;
        r.late_to_absent_min = 300;
        r.early_to_absent_enabled = false;
        r.early_to_absent_min = 310;
        r.half_day_after_min = 90;
        r.min_full_day_min = 400;
        r.ot_after_shift_enabled = false;
        r.ot_after_shift_min = 45;
        r.ot_before_shift_enabled = true;
        r.ot_before_shift_min = 20;
        r.ot_max_daily_min = 180;
        r.dedupe_secs = 90;
        r.lone_punch_half_day = false;

        r.sym_normal = "1".into();
        r.sym_late = "2".into();
        r.sym_early = "3".into();
        r.sym_absent = "4".into();
        r.sym_ot = "5".into();
        r.sym_leave = "6".into();
        r.sym_holiday = "7".into();
        r.sym_half_day = "8".into();
        r.sym_missing = "9".into();

        r.min_unit = 0.25;
        r.min_unit_basis = "hours".into();
        r.rounding = Rounding::Up;
        r.acc_by_times = true;
        r.round_at_acc = false;
        r.group_by_periods = true;

        r.weekend_days = [true, false, true, false, true, false, false];
        r.weekend_as_ot = false;
        r.weekend_symbol = "X".into();
        r.weekend_colour = "#123456".into();

        r.save(&conn).unwrap();
        let back = AttendanceRules::load(&conn).unwrap();
        assert_eq!(back, r, "a column is mapped to the wrong field");
    }

    #[test]
    fn the_overtime_state_is_stored_under_the_name_its_column_accepts() {
        // out_state and ot_state share one enum but their CHECK constraints
        // spell the "honour the key" option differently: as_out and as_ot. A
        // save that ignores that fails at the database, not in the screen.
        let conn = db::open_memory().unwrap();
        let mut r = AttendanceRules::default();
        r.ot_state = StateHandling::AsMarked;
        r.out_state = StateHandling::AsMarked;
        r.save(&conn).unwrap();

        let stored: String =
            conn.query_row("SELECT ot_state FROM attendance_rules", [], |x| x.get(0)).unwrap();
        assert_eq!(stored, "as_ot");
        assert_eq!(AttendanceRules::load(&conn).unwrap().ot_state, StateHandling::AsMarked);
    }

    #[test]
    fn rounding_snaps_to_the_minimum_unit() {
        let mut r = AttendanceRules::default(); // 0.5, round off
        assert_eq!(r.round_unit(0.6), 0.5);
        assert_eq!(r.round_unit(0.8), 1.0);
        assert_eq!(r.round_unit(1.0), 1.0);

        r.rounding = Rounding::Down;
        assert_eq!(r.round_unit(0.9), 0.5);
        r.rounding = Rounding::Up;
        assert_eq!(r.round_unit(0.1), 0.5);

        // A quarter-day unit.
        r.min_unit = 0.25;
        r.rounding = Rounding::Off;
        assert_eq!(r.round_unit(0.6), 0.5);
        assert_eq!(r.round_unit(0.7), 0.75);
    }

    #[test]
    fn rounding_cannot_divide_by_zero_or_produce_nan() {
        let mut r = AttendanceRules::default();
        r.min_unit = 0.0;
        assert_eq!(r.round_unit(0.7), 0.7, "a zero unit must leave the value alone");
        assert!(r.round_unit(f64::NAN).is_nan());
        r.min_unit = 0.5;
        assert!(r.round_unit(f64::INFINITY).is_infinite(), "must not produce NaN");
    }

    #[test]
    fn a_rule_set_that_grades_backwards_is_refused() {
        let mut r = AttendanceRules::default();
        r.half_day_after_min = 300;
        r.late_to_absent_min = 240;
        let e = r.validate().unwrap_err().to_string();
        assert!(e.contains("half day"), "message should name the problem: {e}");

        // And it must not reach the database.
        let conn = db::open_memory().unwrap();
        assert!(r.save(&conn).is_err());
        assert_eq!(
            AttendanceRules::load(&conn).unwrap(),
            AttendanceRules::default(),
            "a refused save must leave the stored rules untouched"
        );
    }

    #[test]
    fn a_full_day_longer_than_the_workday_is_refused() {
        // This exact mistake shipped once: min_full_day was set above what the
        // shift could physically produce, and every punctual member of staff
        // was recorded as a half day.
        let mut r = AttendanceRules::default();
        r.workday_minutes = 420;
        r.min_full_day_min = 430;
        let e = r.validate().unwrap_err().to_string();
        assert!(e.contains("half day"), "{e}");
    }

    #[test]
    fn every_day_cannot_be_a_weekend() {
        let mut r = AttendanceRules::default();
        r.weekend_days = [true; 7];
        assert!(r.validate().is_err());
    }

    #[test]
    fn weekend_lookup_is_bounds_safe() {
        let r = AttendanceRules::default();
        assert!(r.is_weekend(6), "Saturday is the school holiday");
        assert!(!r.is_weekend(3));
        assert!(!r.is_weekend(99), "an impossible weekday must not panic");
    }
}
