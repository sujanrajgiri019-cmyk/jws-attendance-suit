//! The attendance rules engine.
//!
//! This is the part of the system that decides whether someone was present,
//! late, half-day or absent — the numbers that end up on a payroll sheet. It is
//! written as a pure function from (punches, shift, policy) to a result so that
//! every branch can be tested, and so that changing a rule can be replayed over
//! untouched raw punches rather than mutating history.
//!
//! All times are minutes from midnight of the *work date*. An overnight shift
//! that ends at 06:00 the next morning is 1800, not 360; this keeps every
//! comparison a plain integer comparison and removes the class of bug where a
//! night guard appears to leave before arriving.

use serde::{Deserialize, Serialize};

/// Outcome for one member on one day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Present,
    Late,
    HalfDay,
    Absent,
    Leave,
    Holiday,
    WeeklyOff,
    /// Scanned in but never out (or vice versa) and policy says flag it.
    MissingPunch,
}

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Present => "Present",
            Status::Late => "Late",
            Status::HalfDay => "HalfDay",
            Status::Absent => "Absent",
            Status::Leave => "Leave",
            Status::Holiday => "Holiday",
            Status::WeeklyOff => "WeeklyOff",
            Status::MissingPunch => "MissingPunch",
        }
    }
    /// Does this count towards "days worked"?
    pub fn is_worked(&self) -> bool {
        matches!(self, Status::Present | Status::Late | Status::HalfDay)
    }
    /// Should this day count in the attendance-rate denominator?
    pub fn is_working_day(&self) -> bool {
        !matches!(self, Status::Holiday | Status::WeeklyOff)
    }
}

/// A shift, flattened into minutes for the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shift {
    pub start_min: i32,
    pub end_min: i32,
    pub late_grace: i32,
    pub early_grace: i32,
    pub break_min: i32,
    /// Minutes of work needed to earn a full day.
    pub min_full_day: i32,
    /// Late by more than this many minutes and the day becomes a half day.
    pub half_day_after: i32,
    /// Late by more than this and the day is written off as absent.
    pub absent_after: i32,
    pub count_ot: bool,
    pub min_ot_block: i32,
}

impl Default for Shift {
    fn default() -> Self {
        // JWS regular duty.
        Shift {
            start_min: 9 * 60,
            end_min: 16 * 60,
            late_grace: 10,
            early_grace: 10,
            break_min: 40,
            // 09:00-16:00 is 420 minutes on site; after a 40-minute break the
            // most anyone can work is 380. A threshold above that would mark
            // every punctual member of staff as a half day.
            min_full_day: 350,
            half_day_after: 120,
            absent_after: 240,
            count_ot: true,
            min_ot_block: 30,
        }
    }
}

impl Shift {
    /// Build from `HH:MM` strings, normalising an overnight end time.
    pub fn from_times(start: &str, end: &str) -> Option<Shift> {
        let s = parse_hhmm(start)?;
        let mut e = parse_hhmm(end)?;
        if e <= s {
            e += 24 * 60; // crosses midnight
        }
        Some(Shift { start_min: s, end_min: e, ..Shift::default() })
    }
    pub fn duration(&self) -> i32 {
        self.end_min - self.start_min
    }
}

/// Policy switches that are not shift-specific.
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Ignore repeat scans within this many seconds (double-taps at the sensor).
    pub dedupe_secs: i32,
    /// A day with only one punch becomes a half day instead of an absence.
    pub lone_punch_half_day: bool,
    /// Flag days that are missing one of the two punches.
    pub require_both_punches: bool,
    pub count_ot: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            dedupe_secs: 60,
            lone_punch_half_day: true,
            require_both_punches: true,
            count_ot: true,
        }
    }
}

/// What the day was, before looking at any punches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayKind {
    /// A normal working day governed by this shift.
    Working(Shift),
    /// Weekly off (Saturday, for JWS).
    Off,
    Holiday,
    ApprovedLeave,
}

/// The computed record for one member-day.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DayResult {
    pub status: Status,
    /// Minutes from midnight, or `None` when there was no punch.
    pub in_min: Option<i32>,
    pub out_min: Option<i32>,
    pub worked_min: i32,
    pub late_min: i32,
    pub early_min: i32,
    pub ot_min: i32,
    pub remark: Option<String>,
}

impl DayResult {
    fn blank(status: Status) -> Self {
        DayResult {
            status,
            in_min: None,
            out_min: None,
            worked_min: 0,
            late_min: 0,
            early_min: 0,
            ot_min: 0,
            remark: None,
        }
    }
    /// In time as `HH:MM:SS`, if present.
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
    /// Device-reported state; the engine prefers ordering over trusting this,
    /// because staff routinely press the wrong key.
    pub state: u8,
}

impl Punch {
    pub fn at(h: i32, m: i32) -> Self {
        Punch { minute: h * 60 + m, second: 0, state: 0 }
    }
    fn total_secs(&self) -> i32 {
        self.minute * 60 + self.second
    }
}

/// Compute one member-day.
///
/// The punch list need not be sorted or deduplicated; this does both.
pub fn compute_day(punches: &[Punch], kind: DayKind, policy: &Policy) -> DayResult {
    let shift = match kind {
        DayKind::Holiday => return DayResult::blank(Status::Holiday),
        DayKind::Off => return DayResult::blank(Status::WeeklyOff),
        // Leave still records any punches, but the status stands: someone who
        // drops in on an approved leave day has not cancelled their leave.
        DayKind::ApprovedLeave => return DayResult::blank(Status::Leave),
        DayKind::Working(s) => s,
    };

    // On an overnight shift the early-morning scans belong to the *next*
    // calendar day. Roll them forward before sorting, otherwise a 05:40
    // check-out sorts ahead of the 18:55 arrival and the whole day inverts.
    let overnight = shift.end_min > 24 * 60;
    let normalised: Vec<Punch> = punches
        .iter()
        .map(|p| {
            if overnight && p.minute < shift.start_min {
                let rolled = p.minute + 24 * 60;
                // Only roll if it lands inside the shift's window; an 18:00
                // scan on a 19:00 shift is early arrival, not tomorrow.
                if rolled <= shift.end_min + 180 {
                    return Punch { minute: rolled, ..*p };
                }
            }
            *p
        })
        .collect();

    let clean = dedupe(&normalised, policy.dedupe_secs);

    let Some(&first) = clean.first() else {
        return DayResult::blank(Status::Absent);
    };

    // A single scan cannot tell us a span. Policy decides whether that is a
    // half day (someone forgot to scan out) or a straight absence.
    if clean.len() == 1 {
        let mut r = DayResult::blank(if policy.lone_punch_half_day {
            Status::HalfDay
        } else if policy.require_both_punches {
            Status::MissingPunch
        } else {
            Status::Absent
        });
        // Record which end we have so the office can correct it.
        if first.state == 1 {
            r.out_min = Some(first.minute);
            r.remark = Some("Only a check-out was recorded".into());
        } else {
            r.in_min = Some(first.minute);
            r.late_min = (first.minute - (shift.start_min + shift.late_grace)).max(0);
            r.remark = Some("Only a check-in was recorded".into());
        }
        return r;
    }

    let last = *clean.last().unwrap();
    let in_min = first.minute;
    let mut out_min = last.minute;

    // On an overnight shift a check-out before the check-in belongs to the
    // following morning.
    if out_min < in_min {
        out_min += 24 * 60;
    }

    let late_min = (in_min - (shift.start_min + shift.late_grace)).max(0);
    let early_min = ((shift.end_min - shift.early_grace) - out_min).max(0);

    let span = out_min - in_min;
    // Only deduct the break once the person was present long enough to have
    // taken it; otherwise a two-hour visit is penalised for lunch they missed.
    let worked_min = if span > shift.break_min { span - shift.break_min } else { span };

    let ot_min = if shift.count_ot && policy.count_ot {
        let over = out_min - shift.end_min;
        if over >= shift.min_ot_block {
            over
        } else {
            0
        }
    } else {
        0
    };

    let mut remark = None;
    let status = if late_min > shift.absent_after {
        remark = Some(format!("Arrived {late_min} min late"));
        Status::Absent
    } else if late_min > shift.half_day_after {
        remark = Some(format!("Arrived {late_min} min late"));
        Status::HalfDay
    } else if worked_min < shift.min_full_day {
        remark = Some(format!("Worked {} of {} min", worked_min, shift.min_full_day));
        Status::HalfDay
    } else if late_min > 0 {
        Status::Late
    } else {
        Status::Present
    };

    DayResult {
        status,
        in_min: Some(in_min),
        out_min: Some(out_min),
        worked_min,
        late_min,
        early_min,
        ot_min,
        remark,
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

/// Totals across a set of days, as shown on reports.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub working_days: i32,
    pub present: i32,
    pub late: i32,
    pub half_day: i32,
    pub absent: i32,
    pub leave: i32,
    pub holiday: i32,
    pub weekly_off: i32,
    pub missing_punch: i32,
    pub worked_min: i32,
    pub late_min: i32,
    pub ot_min: i32,
}

impl Summary {
    /// Attendance rate as a percentage of working days, counting a half day as
    /// half. Returns 0.0 when there were no working days, rather than NaN —
    /// a blank month must not print "NaN%" on a report.
    pub fn rate(&self) -> f64 {
        if self.working_days == 0 {
            return 0.0;
        }
        let credited = self.present as f64 + self.late as f64 + self.half_day as f64 * 0.5;
        (credited / self.working_days as f64) * 100.0
    }
}

/// Roll a list of computed days into a summary.
pub fn summarise(days: &[DayResult]) -> Summary {
    let mut s = Summary::default();
    for d in days {
        if d.status.is_working_day() {
            s.working_days += 1;
        }
        match d.status {
            Status::Present => s.present += 1,
            Status::Late => s.late += 1,
            Status::HalfDay => s.half_day += 1,
            Status::Absent => s.absent += 1,
            Status::Leave => s.leave += 1,
            Status::Holiday => s.holiday += 1,
            Status::WeeklyOff => s.weekly_off += 1,
            Status::MissingPunch => s.missing_punch += 1,
        }
        s.worked_min += d.worked_min;
        s.late_min += d.late_min;
        s.ot_min += d.ot_min;
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

fn fmt_hhmmss(min: i32) -> String {
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

    fn regular() -> DayKind {
        DayKind::Working(Shift::default())
    }

    #[test]
    fn on_time_full_day_is_present() {
        let r = compute_day(&[Punch::at(8, 55), Punch::at(16, 5)], regular(), &Policy::default());
        assert_eq!(r.status, Status::Present);
        assert_eq!(r.late_min, 0);
        assert_eq!(r.early_min, 0);
        assert_eq!(r.in_time().unwrap(), "08:55:00");
        assert_eq!(r.out_time().unwrap(), "16:05:00");
        // 7h10m span minus the 40 minute break.
        assert_eq!(r.worked_min, 430 - 40);
    }

    #[test]
    fn a_punctual_person_on_the_default_shift_is_a_full_day() {
        // Regression guard: min_full_day must stay below what the shift can
        // actually deliver (420 on site minus a 40 minute break = 380), or
        // every single member of staff silently becomes a half day.
        let s = Shift::default();
        let achievable = s.duration() - s.break_min;
        assert!(
            s.min_full_day <= achievable,
            "min_full_day {} exceeds the {achievable} minutes this shift can produce",
            s.min_full_day
        );

        let r = compute_day(&[Punch::at(9, 0), Punch::at(16, 0)], regular(), &Policy::default());
        assert_eq!(r.status, Status::Present, "exact duty hours must be a full day");
    }

    #[test]
    fn arriving_inside_the_grace_period_is_not_late() {
        // 09:10 with a 10-minute grace is exactly on the boundary.
        let r = compute_day(&[Punch::at(9, 10), Punch::at(16, 30)], regular(), &Policy::default());
        assert_eq!(r.late_min, 0);
        assert_eq!(r.status, Status::Present);
    }

    #[test]
    fn one_minute_past_grace_is_late() {
        let r = compute_day(&[Punch::at(9, 11), Punch::at(16, 30)], regular(), &Policy::default());
        assert_eq!(r.late_min, 1);
        assert_eq!(r.status, Status::Late);
    }

    #[test]
    fn very_late_becomes_half_day_then_absent() {
        // half_day_after = 120, absent_after = 240, measured past the grace.
        let half =
            compute_day(&[Punch::at(11, 30), Punch::at(18, 30)], regular(), &Policy::default());
        assert_eq!(half.status, Status::HalfDay);
        assert_eq!(half.late_min, 140);

        let absent =
            compute_day(&[Punch::at(13, 30), Punch::at(18, 30)], regular(), &Policy::default());
        assert_eq!(absent.status, Status::Absent);
        assert_eq!(absent.late_min, 260);
        // Even written off, the actual times are preserved for the record.
        assert_eq!(absent.in_time().unwrap(), "13:30:00");
    }

    #[test]
    fn leaving_early_is_measured_against_the_grace() {
        let r = compute_day(&[Punch::at(9, 0), Punch::at(15, 30)], regular(), &Policy::default());
        // Duty ends 16:00 with 10 min grace, so 15:50 is the threshold.
        assert_eq!(r.early_min, 20);
    }

    #[test]
    fn short_day_is_a_half_day_even_when_on_time() {
        let r = compute_day(&[Punch::at(9, 0), Punch::at(12, 0)], regular(), &Policy::default());
        assert_eq!(r.status, Status::HalfDay);
        assert!(r.remark.unwrap().contains("of 350 min"));
    }

    #[test]
    fn no_punches_is_absent() {
        let r = compute_day(&[], regular(), &Policy::default());
        assert_eq!(r.status, Status::Absent);
        assert!(r.in_min.is_none() && r.out_min.is_none());
        assert_eq!(r.worked_min, 0);
    }

    #[test]
    fn a_single_punch_becomes_a_half_day_by_default() {
        let r = compute_day(&[Punch::at(8, 58)], regular(), &Policy::default());
        assert_eq!(r.status, Status::HalfDay);
        assert_eq!(r.in_time().unwrap(), "08:58:00");
        assert!(r.out_min.is_none());
        assert!(r.remark.unwrap().contains("check-in"));
    }

    #[test]
    fn a_single_punch_can_instead_be_flagged_for_review() {
        let p = Policy { lone_punch_half_day: false, ..Policy::default() };
        let r = compute_day(&[Punch::at(8, 58)], regular(), &p);
        assert_eq!(r.status, Status::MissingPunch);
    }

    #[test]
    fn a_lone_checkout_records_the_out_side() {
        let p = Punch { minute: 16 * 60 + 5, second: 0, state: 1 };
        let r = compute_day(&[p], regular(), &Policy::default());
        assert!(r.in_min.is_none());
        assert_eq!(r.out_time().unwrap(), "16:05:00");
        assert!(r.remark.unwrap().contains("check-out"));
    }

    #[test]
    fn double_taps_at_the_sensor_are_ignored() {
        // Three scans a few seconds apart is one arrival, not three.
        let punches = vec![
            Punch { minute: 8 * 60 + 55, second: 0, state: 0 },
            Punch { minute: 8 * 60 + 55, second: 12, state: 0 },
            Punch { minute: 8 * 60 + 55, second: 41, state: 0 },
            Punch { minute: 16 * 60 + 5, second: 0, state: 1 },
        ];
        let r = compute_day(&punches, regular(), &Policy::default());
        assert_eq!(r.status, Status::Present);
        assert_eq!(r.in_time().unwrap(), "08:55:00");
        assert_eq!(r.out_time().unwrap(), "16:05:00");
    }

    #[test]
    fn middle_punches_are_ignored_first_and_last_win() {
        // Staff scanning at lunch must not shorten the day.
        let r = compute_day(
            &[Punch::at(8, 50), Punch::at(12, 30), Punch::at(13, 10), Punch::at(16, 20)],
            regular(),
            &Policy::default(),
        );
        assert_eq!(r.in_time().unwrap(), "08:50:00");
        assert_eq!(r.out_time().unwrap(), "16:20:00");
        assert_eq!(r.status, Status::Present);
    }

    #[test]
    fn punches_arriving_out_of_order_are_sorted() {
        let r = compute_day(&[Punch::at(16, 5), Punch::at(8, 55)], regular(), &Policy::default());
        assert_eq!(r.in_time().unwrap(), "08:55:00");
        assert_eq!(r.out_time().unwrap(), "16:05:00");
    }

    #[test]
    fn overtime_needs_a_minimum_block() {
        // 20 minutes over is below the 30-minute block, so it does not count.
        let short = compute_day(&[Punch::at(9, 0), Punch::at(16, 20)], regular(), &Policy::default());
        assert_eq!(short.ot_min, 0);

        let real = compute_day(&[Punch::at(9, 0), Punch::at(17, 30)], regular(), &Policy::default());
        assert_eq!(real.ot_min, 90);
    }

    #[test]
    fn overtime_can_be_switched_off_entirely() {
        let p = Policy { count_ot: false, ..Policy::default() };
        let r = compute_day(&[Punch::at(9, 0), Punch::at(19, 0)], regular(), &p);
        assert_eq!(r.ot_min, 0);
    }

    #[test]
    fn overnight_shift_does_not_go_backwards() {
        // Night guard 19:00 to 06:00: the 05:40 scan is the next morning.
        let shift = Shift { break_min: 0, min_full_day: 600, ..Shift::from_times("19:00", "06:00").unwrap() };
        assert_eq!(shift.end_min, 30 * 60, "end must be normalised past midnight");

        let r = compute_day(
            &[Punch::at(18, 55), Punch::at(5, 40)],
            DayKind::Working(shift),
            &Policy::default(),
        );
        assert_eq!(r.in_time().unwrap(), "18:55:00");
        assert_eq!(r.out_time().unwrap(), "05:40:00");
        assert_eq!(r.worked_min, 645, "18:55 to 05:40 is 10h45m");
        assert!(r.worked_min > 0, "an overnight day must never compute negative work");
    }

    #[test]
    fn early_arrival_on_a_night_shift_is_not_mistaken_for_tomorrow() {
        // 18:00 on a 19:00-06:00 shift is someone arriving an hour early, not
        // a scan belonging to the next morning.
        let shift = Shift {
            break_min: 0,
            min_full_day: 600,
            ..Shift::from_times("19:00", "06:00").unwrap()
        };
        let r = compute_day(
            &[Punch::at(18, 0), Punch::at(5, 30)],
            DayKind::Working(shift),
            &Policy::default(),
        );
        assert_eq!(r.in_time().unwrap(), "18:00:00");
        assert_eq!(r.out_time().unwrap(), "05:30:00");
        assert_eq!(r.worked_min, 690);
    }

    #[test]
    fn a_short_visit_is_not_charged_for_a_break_it_could_not_take() {
        // 20 minutes on site, with a 40-minute break configured.
        let r = compute_day(&[Punch::at(9, 0), Punch::at(9, 20)], regular(), &Policy::default());
        assert_eq!(r.worked_min, 20, "break must not make worked time negative");
        assert!(r.worked_min >= 0);
    }

    #[test]
    fn holidays_and_weekly_offs_short_circuit() {
        for (kind, expected) in
            [(DayKind::Holiday, Status::Holiday), (DayKind::Off, Status::WeeklyOff)]
        {
            let r = compute_day(&[Punch::at(9, 0), Punch::at(16, 0)], kind, &Policy::default());
            assert_eq!(r.status, expected);
            assert_eq!(r.worked_min, 0);
            assert_eq!(r.late_min, 0);
        }
    }

    #[test]
    fn approved_leave_stays_leave_even_if_they_drop_in() {
        let r = compute_day(
            &[Punch::at(9, 0), Punch::at(16, 0)],
            DayKind::ApprovedLeave,
            &Policy::default(),
        );
        assert_eq!(r.status, Status::Leave);
    }

    #[test]
    fn shift_parsing_rejects_nonsense() {
        assert!(Shift::from_times("09:00", "16:00").is_some());
        assert!(Shift::from_times("25:00", "16:00").is_none());
        assert!(Shift::from_times("09:70", "16:00").is_none());
        assert!(Shift::from_times("nine", "16:00").is_none());
        assert_eq!(parse_hhmm("09:00:00"), Some(540), "seconds are tolerated");
    }

    #[test]
    fn summary_counts_and_rate() {
        let days = vec![
            DayResult::blank(Status::Present),
            DayResult::blank(Status::Present),
            DayResult::blank(Status::Late),
            DayResult::blank(Status::HalfDay),
            DayResult::blank(Status::Absent),
            DayResult::blank(Status::Holiday),
            DayResult::blank(Status::WeeklyOff),
        ];
        let s = summarise(&days);
        assert_eq!(s.working_days, 5, "holiday and weekly off are excluded");
        assert_eq!(s.present, 2);
        assert_eq!(s.late, 1);
        assert_eq!(s.half_day, 1);
        assert_eq!(s.absent, 1);
        // (2 + 1 + 0.5) / 5 = 70%
        assert!((s.rate() - 70.0).abs() < 1e-9, "got {}", s.rate());
    }

    #[test]
    fn empty_month_reports_zero_not_nan() {
        let s = summarise(&[]);
        assert_eq!(s.rate(), 0.0);
        assert!(s.rate().is_finite(), "a blank month must not print NaN%");
    }

    #[test]
    fn summary_of_only_holidays_is_zero_not_nan() {
        let s = summarise(&[DayResult::blank(Status::Holiday), DayResult::blank(Status::WeeklyOff)]);
        assert_eq!(s.working_days, 0);
        assert!(s.rate().is_finite());
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(fmt_duration(390), "6:30");
        assert_eq!(fmt_duration(60), "1:00");
        assert_eq!(fmt_duration(5), "0:05");
        assert_eq!(fmt_duration(0), "0:00");
    }

    #[test]
    fn status_classification_is_consistent() {
        assert!(Status::Present.is_worked() && Status::Late.is_worked() && Status::HalfDay.is_worked());
        assert!(!Status::Absent.is_worked() && !Status::Leave.is_worked());
        assert!(!Status::Holiday.is_working_day() && !Status::WeeklyOff.is_working_day());
        assert!(Status::Absent.is_working_day(), "an absence still consumes a working day");
    }
}
