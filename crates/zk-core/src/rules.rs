//! The attendance rules engine.
//!
//! This is the part of the system that decides whether someone was present,
//! late, early away, half-day or absent — the numbers that end up on a payroll
//! sheet. It is a pure function from (punches, plan, rules) to a result, so
//! every branch can be tested without a device or a database, and so a changed
//! rule can be replayed over untouched raw punches rather than mutating
//! history.
//!
//! All times are minutes from midnight of the *work date*. An overnight block
//! ending at 06:00 the next morning is 1800, not 360; this keeps every
//! comparison a plain integer comparison and removes the class of bug where a
//! night guard appears to leave before he arrived.
//!
//! ## Which grace period wins
//!
//! A block of duty carries its own late/early grace, and the rule set carries a
//! school-wide one. The block wins when it sets a value; the school-wide figure
//! fills in for blocks that leave it at zero. That way the exam-duty timetable
//! can demand punctuality without the office having to restate the ordinary
//! rule on every other block.

use crate::ruleset::{AttendanceRules, StateHandling};
use crate::schedule::{DayPlan, Timetable};
use serde::{Deserialize, Serialize};

/// Outcome for one member on one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Present,
    Late,
    /// Left before the end of duty by more than the grace allows.
    EarlyLeave,
    HalfDay,
    Absent,
    Leave,
    Holiday,
    WeeklyOff,
    /// Scanned in but never out (or the reverse) on a block that demands both.
    MissingPunch,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Present => "Present",
            Status::Late => "Late",
            Status::EarlyLeave => "EarlyLeave",
            Status::HalfDay => "HalfDay",
            Status::Absent => "Absent",
            Status::Leave => "Leave",
            Status::Holiday => "Holiday",
            Status::WeeklyOff => "WeeklyOff",
            Status::MissingPunch => "MissingPunch",
        }
    }

    pub fn parse(s: &str) -> Status {
        match s {
            "Present" => Status::Present,
            "Late" => Status::Late,
            "EarlyLeave" => Status::EarlyLeave,
            "HalfDay" => Status::HalfDay,
            "Leave" => Status::Leave,
            "Holiday" => Status::Holiday,
            "WeeklyOff" => Status::WeeklyOff,
            "MissingPunch" => Status::MissingPunch,
            _ => Status::Absent,
        }
    }

    /// Does this count towards "days worked"?
    pub fn is_worked(&self) -> bool {
        matches!(self, Status::Present | Status::Late | Status::EarlyLeave | Status::HalfDay)
    }

    /// Should this day count in the attendance-rate denominator?
    pub fn is_working_day(&self) -> bool {
        !matches!(self, Status::Holiday | Status::WeeklyOff)
    }

    /// Worse of two outcomes, used when a day is made of several blocks: a
    /// teacher who missed the afternoon has not had a normal day because the
    /// morning went well.
    fn worse(self, other: Status) -> Status {
        let rank = |s: Status| match s {
            Status::Present => 0,
            Status::Late => 1,
            Status::EarlyLeave => 2,
            Status::MissingPunch => 3,
            Status::HalfDay => 4,
            Status::Absent => 5,
            // Whole-day states never mix with block outcomes.
            Status::Leave | Status::Holiday | Status::WeeklyOff => 6,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }

    /// The symbol this status prints as on a report.
    pub fn symbol(&self, r: &AttendanceRules) -> String {
        match self {
            Status::Present => r.sym_normal.clone(),
            Status::Late => r.sym_late.clone(),
            Status::EarlyLeave => r.sym_early.clone(),
            Status::HalfDay => r.sym_half_day.clone(),
            Status::Absent => r.sym_absent.clone(),
            Status::Leave => r.sym_leave.clone(),
            Status::Holiday => r.sym_holiday.clone(),
            Status::WeeklyOff => r.weekend_symbol.clone(),
            Status::MissingPunch => r.sym_missing.clone(),
        }
    }
}

/// What the day was, before looking at any punches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayKind {
    /// A day the member is rostered to work.
    Working,
    /// A weekly rest day, from the weekend set or an empty day in the cycle.
    Weekend,
    Holiday,
    ApprovedLeave,
}

/// Which half of a day's pair of scans is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exception {
    MissingIn,
    MissingOut,
    Both,
    /// The two scans are too far apart to be one person's day — almost always
    /// a missed departure yesterday joined to today's arrival.
    ImplausibleSpan,
}

impl Exception {
    pub fn as_str(&self) -> &'static str {
        match self {
            Exception::MissingIn => "MissingIn",
            Exception::MissingOut => "MissingOut",
            Exception::Both => "Both",
            Exception::ImplausibleSpan => "ImplausibleSpan",
        }
    }
}

/// The computed record for one member-day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayResult {
    pub status: Status,
    /// Minutes from midnight, or `None` when there was no scan.
    pub in_min: Option<i32>,
    pub out_min: Option<i32>,
    pub worked_min: i32,
    pub late_min: i32,
    pub early_min: i32,
    pub ot_min: i32,
    /// Overtime earned on a rest day, kept apart because schools usually pay it
    /// at a different rate.
    pub weekend_ot_min: i32,
    /// Credit towards the month: 1.0, 0.5, 0.0, after the rounding rules.
    pub workday_value: f64,
    pub exception: Option<Exception>,
    /// The block of duty this was measured against, for the report.
    pub timetable_id: Option<i64>,
    pub remark: Option<String>,
}

impl DayResult {
    pub fn blank(status: Status) -> Self {
        DayResult {
            status,
            in_min: None,
            out_min: None,
            worked_min: 0,
            late_min: 0,
            early_min: 0,
            ot_min: 0,
            weekend_ot_min: 0,
            workday_value: 0.0,
            exception: None,
            timetable_id: None,
            remark: None,
        }
    }
    pub fn in_time(&self) -> Option<String> {
        self.in_min.map(fmt_hhmmss)
    }
    pub fn out_time(&self) -> Option<String> {
        self.out_min.map(fmt_hhmmss)
    }
}

/// A single raw scan, as minutes from midnight of the work date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Punch {
    pub minute: i32,
    /// Seconds within the minute, kept only so dedupe is accurate.
    pub second: i32,
    /// Device-reported state. The engine prefers the time windows over trusting
    /// this, because staff routinely press the wrong key.
    pub state: u8,
}

impl Punch {
    pub fn at(h: i32, m: i32) -> Self {
        Punch { minute: h * 60 + m, second: 0, state: 0 }
    }
    fn total_secs(&self) -> i32 {
        self.minute * 60 + self.second
    }
    /// Did the terminal tag this as an out-of-office or overtime scan?
    fn is_special(&self) -> bool {
        matches!(self.state, 2..=5)
    }
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

/// Compute one member-day.
///
/// The punch list need not be sorted or deduplicated; this does both.
pub fn compute_day(
    punches: &[Punch],
    kind: DayKind,
    plan: &DayPlan,
    rules: &AttendanceRules,
) -> DayResult {
    match kind {
        DayKind::Holiday => return rest_day(punches, Status::Holiday, rules),
        DayKind::ApprovedLeave => {
            // Leave stands even if the person dropped in: turning up for an
            // hour on an approved leave day has not cancelled the leave.
            return DayResult::blank(Status::Leave);
        }
        DayKind::Weekend => return rest_day(punches, Status::WeeklyOff, rules),
        DayKind::Working => {}
    }

    // A working day with nothing rostered is a rest day in all but name.
    if plan.is_rest() {
        return rest_day(punches, Status::WeeklyOff, rules);
    }

    let clean = dedupe(punches, rules.dedupe_secs);

    // Each block of duty is measured on its own, then the day is the sum.
    let mut day: Option<DayResult> = None;
    for tt in &plan.timetables {
        let block = compute_block(&clean, tt, rules);
        day = Some(match day {
            None => block,
            Some(acc) => combine(acc, block),
        });
    }

    let mut result = day.unwrap_or_else(|| DayResult::blank(Status::Absent));

    // Rounding happens once, here, unless the rules ask for it to be deferred
    // to the end of the reporting period. Rounding every day and then adding is
    // how a month quietly drifts by several hours.
    if !rules.round_at_acc {
        result.workday_value = rules.round_unit(result.workday_value);
    }
    result
}

/// A day nobody was rostered for. Scans still count, as overtime, if the rules
/// say weekend work is overtime.
fn rest_day(punches: &[Punch], status: Status, rules: &AttendanceRules) -> DayResult {
    let mut r = DayResult::blank(status);
    let clean = dedupe(punches, rules.dedupe_secs);
    let (Some(first), Some(last)) = (clean.first(), clean.last()) else {
        return r;
    };

    r.in_min = Some(first.minute);
    // A single scan on a rest day is somebody checking the machine works.
    if clean.len() > 1 {
        r.out_min = Some(last.minute);
        let span = last.minute - first.minute;
        if rules.weekend_as_ot && rules.span_is_plausible(span) {
            r.weekend_ot_min = span.min(rules.ot_max_daily_min);
            r.remark = Some(format!("Worked {} on a rest day", fmt_duration(span)));
        }
    }
    r
}

/// Measure one block of duty.
fn compute_block(clean: &[Punch], tt: &Timetable, rules: &AttendanceRules) -> DayResult {
    // The block's own grace wins; the school-wide figure fills in for a block
    // that does not set one.
    let late_grace = if tt.late_grace > 0 { tt.late_grace } else { rules.late_after_min };
    let early_grace = if tt.early_grace > 0 { tt.early_grace } else { rules.early_after_min };

    // Roll early-morning scans forward onto the block's timeline, so a 05:40
    // check-out does not sort ahead of an 18:55 arrival and invert the day.
    let rolled: Vec<Punch> = clean
        .iter()
        .map(|p| {
            if tt.is_overnight() && p.minute < tt.on_min {
                let up = p.minute + 24 * 60;
                if up <= tt.off_min + 180 {
                    return Punch { minute: up, ..*p };
                }
            }
            *p
        })
        .collect();

    // Classify by window, not by the key the person pressed.
    let in_scan = rolled
        .iter()
        .filter(|p| p.minute >= tt.in_begin && p.minute <= tt.in_end)
        .filter(|p| !(rules.out_state == StateHandling::Ignore && p.is_special()))
        .min_by_key(|p| p.minute)
        .copied();

    let out_scan = rolled
        .iter()
        .filter(|p| p.minute >= tt.out_begin && p.minute <= tt.out_end)
        .filter(|p| !(rules.out_state == StateHandling::Ignore && p.is_special()))
        .max_by_key(|p| p.minute)
        .copied();

    let mut r = DayResult::blank(Status::Present);
    r.timetable_id = Some(tt.id);

    match (in_scan, out_scan) {
        (None, None) => return absent_block(tt, rules, Exception::Both),
        (Some(i), None) => {
            r.in_min = Some(i.minute);
            r.late_min = (i.minute - (tt.on_min + late_grace)).max(0);
            return missing_half(r, tt, rules, Exception::MissingOut);
        }
        (None, Some(o)) => {
            r.out_min = Some(o.minute);
            r.early_min = ((tt.off_min - early_grace) - o.minute).max(0);
            return missing_half(r, tt, rules, Exception::MissingIn);
        }
        (Some(i), Some(o)) => {
            r.in_min = Some(i.minute);
            r.out_min = Some(o.minute);
        }
    }

    let (in_min, out_min) = (r.in_min.unwrap(), r.out_min.unwrap());
    let span = out_min - in_min;

    // Two scans that cannot describe one person's day. Almost always yesterday's
    // missed departure joined to this morning's arrival — recording it as a
    // thirty-hour day would poison the month's totals.
    if !rules.span_is_plausible(span) {
        let mut bad = DayResult::blank(Status::MissingPunch);
        bad.timetable_id = Some(tt.id);
        bad.in_min = Some(in_min);
        bad.out_min = Some(out_min);
        bad.exception = Some(Exception::ImplausibleSpan);
        bad.remark = Some(format!(
            "{} between the two scans is outside the {}-{} minute range a day can be",
            fmt_duration(span),
            rules.shortest_zone_min,
            rules.longest_zone_min
        ));
        return bad;
    }

    r.late_min = (in_min - (tt.on_min + late_grace)).max(0);
    r.early_min = ((tt.off_min - early_grace) - out_min).max(0);

    // Only deduct the break once the person was here long enough to take it;
    // otherwise a two-hour visit is charged for a lunch it never had.
    r.worked_min = if span > tt.break_min { span - tt.break_min } else { span };

    // --- overtime ---
    if tt.count_ot {
        let mut ot = 0;
        if rules.ot_after_shift_enabled {
            let after = out_min - tt.off_min;
            if after >= tt.min_ot_block.max(rules.ot_after_shift_min) {
                ot += after;
            }
        }
        if rules.ot_before_shift_enabled {
            let before = tt.on_min - in_min;
            if before >= rules.ot_before_shift_min {
                ot += before;
            }
        }
        r.ot_min = ot.clamp(0, rules.ot_max_daily_min);
    }

    // --- grading ---
    let expected = if tt.work_minutes > 0 { tt.work_minutes } else { rules.min_full_day_min };
    let full_day_needs = rules.min_full_day_min.min(expected);

    r.status = if rules.late_to_absent_enabled && r.late_min > rules.late_to_absent_min {
        r.remark = Some(format!("Arrived {} min late", r.late_min));
        Status::Absent
    } else if rules.early_to_absent_enabled && r.early_min > rules.early_to_absent_min {
        r.remark = Some(format!("Left {} min early", r.early_min));
        Status::Absent
    } else if r.late_min > rules.half_day_after_min {
        r.remark = Some(format!("Arrived {} min late", r.late_min));
        Status::HalfDay
    } else if r.worked_min < full_day_needs {
        r.remark = Some(format!("Worked {} of {} min", r.worked_min, full_day_needs));
        Status::HalfDay
    } else if r.late_min > 0 {
        Status::Late
    } else if r.early_min > 0 {
        Status::EarlyLeave
    } else {
        Status::Present
    };

    r.workday_value = credit(r.status, tt);
    r
}

/// The block earned nothing at all.
fn absent_block(tt: &Timetable, rules: &AttendanceRules, ex: Exception) -> DayResult {
    let mut r = DayResult::blank(Status::Absent);
    r.timetable_id = Some(tt.id);
    r.exception = Some(ex);
    if rules.no_clock_in_enabled && rules.no_clock_in_as == "Late" {
        r.late_min = rules.no_clock_in_min;
    }
    if rules.no_clock_out_enabled && rules.no_clock_out_as == "EarlyLeave" {
        r.early_min = rules.no_clock_out_min;
    }
    r
}

/// One scan only: apply the missing-punch rules.
fn missing_half(
    mut r: DayResult,
    tt: &Timetable,
    rules: &AttendanceRules,
    ex: Exception,
) -> DayResult {
    r.exception = Some(ex);

    let (enabled, treat_as, charge, must, label) = match ex {
        Exception::MissingIn => (
            rules.no_clock_in_enabled,
            rules.no_clock_in_as.as_str(),
            rules.no_clock_in_min,
            tt.must_c_in,
            "check-in",
        ),
        _ => (
            rules.no_clock_out_enabled,
            rules.no_clock_out_as.as_str(),
            rules.no_clock_out_min,
            tt.must_c_out,
            "check-out",
        ),
    };

    r.remark = Some(format!("No {label} was recorded"));

    if enabled {
        match treat_as {
            "Absent" => {
                r.status = Status::Absent;
                r.workday_value = 0.0;
                return r;
            }
            "Late" => r.late_min = r.late_min.max(charge),
            "EarlyLeave" => r.early_min = r.early_min.max(charge),
            _ => {}
        }
    }

    // The block insists on both scans, so the day is flagged rather than
    // silently credited — but it is not written off as an absence, because the
    // person demonstrably was here.
    r.status = if must && !rules.lone_punch_half_day {
        Status::MissingPunch
    } else if rules.lone_punch_half_day {
        Status::HalfDay
    } else {
        Status::Absent
    };
    r.workday_value = credit(r.status, tt);
    r
}

/// Workday credit for an outcome, before rounding.
fn credit(status: Status, tt: &Timetable) -> f64 {
    match status {
        Status::Present | Status::Late | Status::EarlyLeave => tt.workday_value,
        Status::HalfDay => tt.workday_value * 0.5,
        _ => 0.0,
    }
}

/// Fold a second block of duty into the day's running total.
fn combine(a: DayResult, b: DayResult) -> DayResult {
    DayResult {
        status: a.status.worse(b.status),
        in_min: match (a.in_min, b.in_min) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (x, y) => x.or(y),
        },
        out_min: match (a.out_min, b.out_min) {
            (Some(x), Some(y)) => Some(x.max(y)),
            (x, y) => x.or(y),
        },
        worked_min: a.worked_min + b.worked_min,
        late_min: a.late_min + b.late_min,
        early_min: a.early_min + b.early_min,
        ot_min: a.ot_min + b.ot_min,
        weekend_ot_min: a.weekend_ot_min + b.weekend_ot_min,
        workday_value: a.workday_value + b.workday_value,
        exception: a.exception.or(b.exception),
        timetable_id: a.timetable_id.or(b.timetable_id),
        remark: match (a.remark, b.remark) {
            (Some(x), Some(y)) => Some(format!("{x}; {y}")),
            (x, y) => x.or(y),
        },
    }
}

/// Sort and drop repeat scans inside the dedupe window.
fn dedupe(punches: &[Punch], window_secs: i32) -> Vec<Punch> {
    let mut v: Vec<Punch> = punches.to_vec();
    v.sort_by_key(|p| p.total_secs());
    let mut out: Vec<Punch> = Vec::with_capacity(v.len());
    for p in v {
        match out.last() {
            Some(prev) if p.total_secs() - prev.total_secs() < window_secs => {}
            _ => out.push(p),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Totals
// ---------------------------------------------------------------------------

/// Totals across a set of days, as shown on reports.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub working_days: i32,
    pub present: i32,
    pub late: i32,
    pub early_leave: i32,
    pub half_day: i32,
    pub absent: i32,
    pub leave: i32,
    pub holiday: i32,
    pub weekly_off: i32,
    pub missing_punch: i32,
    pub worked_min: i32,
    pub late_min: i32,
    pub early_min: i32,
    pub ot_min: i32,
    pub weekend_ot_min: i32,
    pub workdays: f64,
}

impl Summary {
    /// Attendance rate as a percentage of working days, counting a half day as
    /// half. Returns 0.0 when there were no working days, rather than NaN — a
    /// blank month must not print "NaN%" on a report.
    pub fn rate(&self) -> f64 {
        if self.working_days == 0 {
            return 0.0;
        }
        let credited = self.present as f64
            + self.late as f64
            + self.early_leave as f64
            + self.half_day as f64 * 0.5;
        (credited / self.working_days as f64) * 100.0
    }
}

/// Roll a list of computed days into a summary.
pub fn summarise(days: &[DayResult], rules: &AttendanceRules) -> Summary {
    let mut s = Summary::default();
    for d in days {
        if d.status.is_working_day() {
            s.working_days += 1;
        }
        match d.status {
            Status::Present => s.present += 1,
            Status::Late => s.late += 1,
            Status::EarlyLeave => s.early_leave += 1,
            Status::HalfDay => s.half_day += 1,
            Status::Absent => s.absent += 1,
            Status::Leave => s.leave += 1,
            Status::Holiday => s.holiday += 1,
            Status::WeeklyOff => s.weekly_off += 1,
            Status::MissingPunch => s.missing_punch += 1,
        }
        s.worked_min += d.worked_min;
        s.late_min += d.late_min;
        s.early_min += d.early_min;
        s.ot_min += d.ot_min;
        s.weekend_ot_min += d.weekend_ot_min;
        s.workdays += d.workday_value;
    }
    // Rounding the period once, at the end, is the accurate way round when the
    // rules ask for it — see the note in compute_day.
    if rules.round_at_acc {
        s.workdays = rules.round_unit(s.workdays);
    }
    s
}

// --- small helpers ---------------------------------------------------------

pub fn parse_hhmm(s: &str) -> Option<i32> {
    let (h, m) = s.trim().split_once(':')?;
    let h: i32 = h.parse().ok()?;
    let m: i32 = m.split(':').next()?.parse().ok()?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 60 + m)
}

pub fn fmt_hhmmss(min: i32) -> String {
    let m = min.rem_euclid(24 * 60);
    format!("{:02}:{:02}:00", m / 60, m % 60)
}

/// `7:15` style duration for reports.
pub fn fmt_duration(mins: i32) -> String {
    format!("{}:{:02}", mins / 60, mins % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The school's ordinary duty: 09:00-16:00, ten minutes' grace, forty
    /// minutes for lunch.
    fn regular() -> Timetable {
        Timetable {
            id: 1,
            name: "Regular Duty".into(),
            on_min: 9 * 60,
            off_min: 16 * 60,
            in_begin: 4 * 60,
            in_end: 12 * 60 + 30,
            out_begin: 12 * 60 + 30,
            out_end: 21 * 60,
            late_grace: 10,
            early_grace: 10,
            break_min: 40,
            workday_value: 1.0,
            work_minutes: 0,
            must_c_in: true,
            must_c_out: true,
            count_ot: true,
            min_ot_block: 30,
            colour: "#F16522".into(),
        }
    }

    fn plan(tt: Timetable) -> DayPlan {
        DayPlan { shift_id: Some(1), timetables: vec![tt] }
    }

    fn day(punches: &[Punch], rules: &AttendanceRules) -> DayResult {
        compute_day(punches, DayKind::Working, &plan(regular()), rules)
    }

    #[test]
    fn a_punctual_day_is_present() {
        let r = AttendanceRules::default();
        let d = day(&[Punch::at(8, 55), Punch::at(16, 5)], &r);
        assert_eq!(d.status, Status::Present);
        assert_eq!(d.late_min, 0);
        assert_eq!(d.early_min, 0);
        // 08:55 to 16:05 is 430 minutes, less 40 for lunch.
        assert_eq!(d.worked_min, 390);
        assert_eq!(d.workday_value, 1.0);
        assert!(d.exception.is_none());
    }

    #[test]
    fn arriving_inside_the_grace_is_not_late() {
        let r = AttendanceRules::default();
        let d = day(&[Punch::at(9, 9), Punch::at(16, 30)], &r);
        assert_eq!(d.status, Status::Present, "nine minutes is inside the ten-minute grace");
        assert_eq!(d.late_min, 0);
    }

    #[test]
    fn arriving_past_the_grace_is_late_by_the_excess_only() {
        let r = AttendanceRules::default();
        let d = day(&[Punch::at(9, 25), Punch::at(16, 30)], &r);
        assert_eq!(d.status, Status::Late);
        assert_eq!(d.late_min, 15, "late is measured from the end of the grace, not from 09:00");
    }

    #[test]
    fn leaving_early_is_its_own_status() {
        let r = AttendanceRules::default();
        // In at 08:00 so there is enough worked time to avoid a half day.
        let d = day(&[Punch::at(7, 0), Punch::at(15, 30)], &r);
        assert_eq!(d.status, Status::EarlyLeave);
        assert_eq!(d.early_min, 20, "16:00 less 10 min grace, less 15:30");
    }

    #[test]
    fn a_very_late_arrival_is_a_half_day_then_an_absence() {
        let r = AttendanceRules::default(); // half day after 120, absent after 240
        let half = day(&[Punch::at(11, 30), Punch::at(18, 0)], &r);
        assert_eq!(half.status, Status::HalfDay);
        assert_eq!(half.workday_value, 0.5);

        let gone = day(&[Punch::at(13, 30), Punch::at(20, 0)], &r);
        assert_eq!(gone.status, Status::Absent);
        assert_eq!(gone.workday_value, 0.0);
    }

    #[test]
    fn a_punctual_day_is_never_graded_as_a_half_day() {
        // The bug this pins: min_full_day was once set above what the shift
        // could physically produce, and every punctual member of staff came out
        // as a half day. The rule set now refuses that, and this proves the
        // engine agrees.
        let r = AttendanceRules::default();
        let tt = regular();
        let achievable = (tt.off_min - tt.on_min) - tt.break_min;
        assert!(
            r.min_full_day_min <= achievable,
            "a full day needs {} min but the shift can only produce {}",
            r.min_full_day_min,
            achievable
        );
        let d = day(&[Punch::at(9, 0), Punch::at(16, 0)], &r);
        assert_eq!(d.status, Status::Present);
    }

    #[test]
    fn a_lone_scan_is_a_half_day_and_says_which_half_is_missing() {
        let r = AttendanceRules::default();
        let d = day(&[Punch::at(9, 0)], &r);
        assert_eq!(d.status, Status::HalfDay);
        assert_eq!(d.exception, Some(Exception::MissingOut));
        assert_eq!(d.in_min, Some(9 * 60));
        assert_eq!(d.out_min, None);
        assert!(d.remark.unwrap().contains("check-out"));
    }

    #[test]
    fn a_lone_afternoon_scan_is_recorded_as_the_departure() {
        let r = AttendanceRules::default();
        let d = day(&[Punch::at(16, 10)], &r);
        assert_eq!(d.exception, Some(Exception::MissingIn));
        assert_eq!(d.out_min, Some(16 * 60 + 10));
        assert_eq!(d.in_min, None, "an afternoon scan is not an arrival");
    }

    #[test]
    fn a_missing_scan_can_be_configured_as_an_absence() {
        let mut r = AttendanceRules::default();
        r.no_clock_out_as = "Absent".into();
        let d = day(&[Punch::at(9, 0)], &r);
        assert_eq!(d.status, Status::Absent);
        assert_eq!(d.workday_value, 0.0);
    }

    #[test]
    fn no_scans_at_all_on_a_working_day_is_an_absence() {
        let r = AttendanceRules::default();
        let d = day(&[], &r);
        assert_eq!(d.status, Status::Absent);
        assert_eq!(d.exception, Some(Exception::Both));
        assert_eq!(d.worked_min, 0);
    }

    #[test]
    fn a_midday_scan_does_not_become_the_days_departure() {
        // Someone scans on their way out to lunch and again on the way back.
        // Reading the noon scan as the day's check-out would cut the day in
        // half; the in/out windows are what prevent it.
        let r = AttendanceRules::default();
        let d = day(&[Punch::at(8, 55), Punch::at(12, 10), Punch::at(12, 45), Punch::at(16, 10)], &r);
        assert_eq!(d.in_min, Some(8 * 60 + 55));
        assert_eq!(d.out_min, Some(16 * 60 + 10));
        assert_eq!(d.status, Status::Present);
    }

    #[test]
    fn repeat_taps_at_the_sensor_are_one_scan() {
        let r = AttendanceRules::default(); // 60-second window
        let d = day(
            &[
                Punch { minute: 9 * 60, second: 0, state: 0 },
                Punch { minute: 9 * 60, second: 12, state: 0 },
                Punch { minute: 9 * 60, second: 40, state: 0 },
                Punch::at(16, 30),
            ],
            &r,
        );
        assert_eq!(d.in_min, Some(9 * 60));
        assert_eq!(d.status, Status::Present);
    }

    #[test]
    fn overtime_needs_to_clear_the_minimum_block() {
        let r = AttendanceRules::default(); // 30-minute minimum
        let short = day(&[Punch::at(9, 0), Punch::at(16, 20)], &r);
        assert_eq!(short.ot_min, 0, "twenty minutes over is not overtime");

        let real = day(&[Punch::at(9, 0), Punch::at(18, 0)], &r);
        assert_eq!(real.ot_min, 120);
    }

    #[test]
    fn overtime_is_capped_at_the_daily_maximum() {
        let r = AttendanceRules::default(); // 240-minute cap
        // A block whose out-window runs to midnight, so the late scan really is
        // read as a departure rather than being discarded as out of hours.
        let mut late_block = regular();
        late_block.out_end = 23 * 60 + 59;
        let d = compute_day(
            &[Punch::at(9, 0), Punch::at(23, 0)],
            DayKind::Working,
            &plan(late_block),
            &r,
        );
        assert_eq!(d.ot_min, 240, "seven hours of overtime is a wrong scan, not a hero");
    }

    #[test]
    fn a_scan_outside_the_out_window_is_not_a_departure() {
        // regular()'s out-window closes at 21:00. A scan at 23:00 is the
        // cleaner locking up, not a member of staff finishing their day.
        let r = AttendanceRules::default();
        let d = day(&[Punch::at(9, 0), Punch::at(23, 0)], &r);
        assert_eq!(d.out_min, None);
        assert_eq!(d.exception, Some(Exception::MissingOut));
    }

    #[test]
    fn early_arrival_only_counts_as_overtime_when_asked_for() {
        let mut r = AttendanceRules::default();
        let punches = [Punch::at(7, 0), Punch::at(16, 0)];
        assert_eq!(day(&punches, &r).ot_min, 0);

        r.ot_before_shift_enabled = true;
        r.ot_before_shift_min = 30;
        assert_eq!(day(&punches, &r).ot_min, 120, "two hours before duty");
    }

    #[test]
    fn a_span_no_day_could_contain_is_flagged_not_credited() {
        // Yesterday's missed departure joined to this morning's arrival.
        let mut r = AttendanceRules::default();
        r.longest_zone_min = 720; // twelve hours
        let mut late_block = regular();
        late_block.out_end = 23 * 60 + 59;
        let d = compute_day(
            &[Punch::at(9, 0), Punch::at(23, 30)],
            DayKind::Working,
            &plan(late_block),
            &r,
        );
        assert_eq!(d.status, Status::MissingPunch);
        assert_eq!(d.exception, Some(Exception::ImplausibleSpan));
        assert_eq!(d.worked_min, 0, "an impossible day must not be credited");
        assert_eq!(d.ot_min, 0);
    }

    #[test]
    fn an_overnight_block_does_not_invert() {
        let mut night = regular();
        night.name = "Night Guard".into();
        night.on_min = 19 * 60;
        night.off_min = 30 * 60; // 06:00 next day
        night.in_begin = 17 * 60;
        night.in_end = 21 * 60;
        night.out_begin = 28 * 60; // 04:00
        night.out_end = 33 * 60; // 09:00
        night.break_min = 0;

        let r = AttendanceRules::default();
        // Arrives 18:55, leaves 05:40 the next morning.
        let d = compute_day(
            &[Punch::at(18, 55), Punch::at(5, 40)],
            DayKind::Working,
            &plan(night),
            &r,
        );
        assert_eq!(d.in_min, Some(18 * 60 + 55));
        assert_eq!(d.out_min, Some(29 * 60 + 40), "05:40 is minute 1780 of the work date");
        assert!(d.worked_min > 600, "a full night must be credited, got {}", d.worked_min);
        assert_eq!(d.late_min, 0);
    }

    #[test]
    fn a_rest_day_with_no_scans_is_simply_off() {
        let r = AttendanceRules::default();
        let d = compute_day(&[], DayKind::Weekend, &plan(regular()), &r);
        assert_eq!(d.status, Status::WeeklyOff);
        assert_eq!(d.weekend_ot_min, 0);
        assert_eq!(d.workday_value, 0.0);
    }

    #[test]
    fn working_a_rest_day_earns_weekend_overtime() {
        let r = AttendanceRules::default(); // weekend_as_ot on
        let d = compute_day(
            &[Punch::at(9, 0), Punch::at(13, 0)],
            DayKind::Weekend,
            &plan(regular()),
            &r,
        );
        assert_eq!(d.status, Status::WeeklyOff, "it is still a rest day");
        assert_eq!(d.weekend_ot_min, 240);
        assert_eq!(d.ot_min, 0, "weekend overtime is counted apart from ordinary overtime");
    }

    #[test]
    fn weekend_overtime_can_be_switched_off() {
        let mut r = AttendanceRules::default();
        r.weekend_as_ot = false;
        let d = compute_day(
            &[Punch::at(9, 0), Punch::at(13, 0)],
            DayKind::Weekend,
            &plan(regular()),
            &r,
        );
        assert_eq!(d.weekend_ot_min, 0);
    }

    #[test]
    fn a_holiday_stays_a_holiday_and_leave_stays_leave() {
        let r = AttendanceRules::default();
        let h = compute_day(&[Punch::at(9, 0), Punch::at(16, 0)], DayKind::Holiday, &plan(regular()), &r);
        assert_eq!(h.status, Status::Holiday);

        let l = compute_day(
            &[Punch::at(9, 0), Punch::at(16, 0)],
            DayKind::ApprovedLeave,
            &plan(regular()),
            &r,
        );
        assert_eq!(l.status, Status::Leave, "turning up does not cancel approved leave");
        assert_eq!(l.workday_value, 0.0);
    }

    #[test]
    fn a_split_day_adds_up_and_takes_the_worse_grade() {
        let mut morning = regular();
        morning.id = 10;
        morning.on_min = 8 * 60;
        morning.off_min = 11 * 60;
        morning.in_begin = 6 * 60;
        morning.in_end = 9 * 60;
        morning.out_begin = 10 * 60;
        morning.out_end = 11 * 60 + 30;
        morning.break_min = 0;
        morning.workday_value = 0.5;

        let mut evening = regular();
        evening.id = 11;
        evening.on_min = 15 * 60;
        evening.off_min = 18 * 60;
        evening.in_begin = 14 * 60;
        evening.in_end = 16 * 60;
        evening.out_begin = 17 * 60;
        evening.out_end = 20 * 60;
        evening.break_min = 0;
        evening.workday_value = 0.5;

        let mut r = AttendanceRules::default();
        // Each block is three hours; do not demand a full day of either.
        r.min_full_day_min = 150;
        r.half_day_after_min = 120;

        let p = DayPlan { shift_id: Some(1), timetables: vec![morning, evening] };
        let d = compute_day(
            &[Punch::at(8, 0), Punch::at(11, 0), Punch::at(15, 0), Punch::at(18, 0)],
            DayKind::Working,
            &p,
            &r,
        );
        assert_eq!(d.worked_min, 360, "three hours plus three hours");
        assert_eq!(d.workday_value, 1.0, "two halves make a whole");
        assert_eq!(d.in_min, Some(8 * 60), "the day starts at the earlier block");
        assert_eq!(d.out_min, Some(18 * 60), "and ends at the later one");
        assert_eq!(d.status, Status::Present);
    }

    #[test]
    fn missing_the_second_half_of_a_split_day_shows_in_the_grade() {
        let mut morning = regular();
        morning.id = 10;
        morning.on_min = 8 * 60;
        morning.off_min = 11 * 60;
        morning.in_begin = 6 * 60;
        morning.in_end = 9 * 60;
        morning.out_begin = 10 * 60;
        morning.out_end = 11 * 60 + 30;
        morning.break_min = 0;
        morning.workday_value = 0.5;

        let mut evening = regular();
        evening.id = 11;
        evening.on_min = 15 * 60;
        evening.off_min = 18 * 60;
        evening.in_begin = 14 * 60;
        evening.in_end = 16 * 60;
        evening.out_begin = 17 * 60;
        evening.out_end = 20 * 60;
        evening.break_min = 0;
        evening.workday_value = 0.5;

        let mut r = AttendanceRules::default();
        r.min_full_day_min = 150;

        let p = DayPlan { shift_id: Some(1), timetables: vec![morning, evening] };
        // Morning only.
        let d = compute_day(&[Punch::at(8, 0), Punch::at(11, 0)], DayKind::Working, &p, &r);
        assert_eq!(d.status, Status::Absent, "the evening block was not worked at all");
        assert_eq!(d.workday_value, 0.5, "the morning is still credited");
    }

    #[test]
    fn rounding_is_deferred_to_the_total_when_asked() {
        let mut r = AttendanceRules::default();
        r.min_unit = 0.5;
        r.round_at_acc = true;

        // Three half days: 1.5 days. Rounding each first would still be 1.5
        // here, so use a unit that makes the difference visible.
        r.min_unit = 1.0;
        let days: Vec<DayResult> = (0..3)
            .map(|_| {
                let mut d = DayResult::blank(Status::HalfDay);
                d.workday_value = 0.5;
                d
            })
            .collect();

        let s = summarise(&days, &r);
        assert_eq!(s.workdays, 2.0, "1.5 days rounds to 2 once, at the end");

        // Rounding each day first would give 0 + 0 + 0 with round-down, or
        // 3 with round-up: both wrong.
        r.round_at_acc = false;
        let each = days
            .iter()
            .map(|d| r.round_unit(d.workday_value))
            .sum::<f64>();
        assert_ne!(each, 2.0, "per-day rounding is what this setting avoids");
    }

    #[test]
    fn a_blank_month_reports_zero_not_nan() {
        let r = AttendanceRules::default();
        let s = summarise(&[], &r);
        assert_eq!(s.rate(), 0.0);
        assert!(s.rate().is_finite());
    }

    #[test]
    fn the_summary_counts_every_status_exactly_once() {
        let r = AttendanceRules::default();
        let days: Vec<DayResult> = [
            Status::Present,
            Status::Late,
            Status::EarlyLeave,
            Status::HalfDay,
            Status::Absent,
            Status::Leave,
            Status::Holiday,
            Status::WeeklyOff,
            Status::MissingPunch,
        ]
        .iter()
        .map(|s| DayResult::blank(*s))
        .collect();

        let s = summarise(&days, &r);
        let counted = s.present
            + s.late
            + s.early_leave
            + s.half_day
            + s.absent
            + s.leave
            + s.holiday
            + s.weekly_off
            + s.missing_punch;
        assert_eq!(counted, days.len() as i32, "a status is being dropped or double counted");
        assert_eq!(s.working_days, 7, "holidays and rest days are out of the denominator");
    }

    #[test]
    fn status_round_trips_through_its_string() {
        for s in [
            Status::Present,
            Status::Late,
            Status::EarlyLeave,
            Status::HalfDay,
            Status::Absent,
            Status::Leave,
            Status::Holiday,
            Status::WeeklyOff,
            Status::MissingPunch,
        ] {
            assert_eq!(Status::parse(s.as_str()), s, "{} does not survive", s.as_str());
        }
    }

    #[test]
    fn symbols_come_from_the_rule_set() {
        let mut r = AttendanceRules::default();
        r.sym_late = "TARDY".into();
        r.weekend_symbol = "OFF".into();
        assert_eq!(Status::Late.symbol(&r), "TARDY");
        assert_eq!(Status::WeeklyOff.symbol(&r), "OFF");
    }

    #[test]
    fn parse_hhmm_rejects_nonsense() {
        assert_eq!(parse_hhmm("09:30"), Some(570));
        assert_eq!(parse_hhmm("23:59"), Some(1439));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("09:60"), None);
        assert_eq!(parse_hhmm("9am"), None);
        assert_eq!(parse_hhmm(""), None);
    }

    #[test]
    fn duration_formatting_is_payroll_shaped() {
        assert_eq!(fmt_duration(0), "0:00");
        assert_eq!(fmt_duration(65), "1:05");
        assert_eq!(fmt_duration(435), "7:15");
    }
}
