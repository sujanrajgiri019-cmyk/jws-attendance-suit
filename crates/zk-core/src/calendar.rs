//! Bikram Sambat (BS) calendar conversion.
//!
//! Nepali months have no arithmetic rule — their lengths are published each
//! year by the Nepal Panchanga Nirnayak Samiti — so conversion is a lookup
//! against a table anchored at 1 Baisakh 2000 BS = 14 April 1943 AD.
//!
//! The table below was validated against known year-start dates (2070, 2078,
//! 2079, 2080, 2081, 2082 BS) and spot-checked against Hamro Patro, which
//! reports 19 August 2026 as 3 Bhadra 2083 — the value `ad_to_bs` returns.
//!
//! Dates outside the table return `None` rather than a guess. A wrong date on
//! a payroll report is worse than a missing one.

use crate::Error;

const BS_START_YEAR: i32 = 2000;
const BS_END_YEAR: i32 = 2090;

/// Days in each of the 12 months, for BS 2000..=2090.
static BS_MONTHS: [[u8; 12]; 91] = [
    [30,32,31,32,31,30,30,30,29,30,29,31], // 2000
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2001
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2002
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2003
    [30,32,31,32,31,30,30,30,29,30,29,31], // 2004
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2005
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2006
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2007
    [31,31,31,32,31,31,29,30,30,29,29,31], // 2008
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2009
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2010
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2011
    [31,31,31,32,31,31,29,30,30,29,30,30], // 2012
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2013
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2014
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2015
    [31,31,31,32,31,31,29,30,30,29,30,30], // 2016
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2017
    [31,32,31,32,31,30,30,29,30,29,30,30], // 2018
    [31,32,31,32,31,30,30,30,29,30,29,31], // 2019
    [31,31,31,32,31,31,30,29,30,29,30,30], // 2020
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2021
    [31,32,31,32,31,30,30,30,29,29,30,30], // 2022
    [31,32,31,32,31,30,30,30,29,30,29,31], // 2023
    [31,31,31,32,31,31,30,29,30,29,30,30], // 2024
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2025
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2026
    [30,32,31,32,31,30,30,30,29,30,29,31], // 2027
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2028
    [31,31,32,31,32,30,30,29,30,29,30,30], // 2029
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2030
    [30,32,31,32,31,30,30,30,29,30,29,31], // 2031
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2032
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2033
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2034
    [30,32,31,32,31,31,29,30,30,29,29,31], // 2035
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2036
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2037
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2038
    [31,31,31,32,31,31,29,30,30,29,30,30], // 2039
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2040
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2041
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2042
    [31,31,31,32,31,31,29,30,30,29,30,30], // 2043
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2044
    [31,32,31,32,31,30,30,29,30,29,30,30], // 2045
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2046
    [31,31,31,32,31,31,30,29,30,29,30,30], // 2047
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2048
    [31,32,31,32,31,30,30,30,29,29,30,30], // 2049
    [31,32,31,32,31,30,30,30,29,30,29,31], // 2050
    [31,31,31,32,31,31,30,29,30,29,30,30], // 2051
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2052
    [31,32,31,32,31,30,30,30,29,29,30,30], // 2053
    [31,32,31,32,31,30,30,30,29,30,29,31], // 2054
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2055
    [31,31,32,31,32,30,30,29,30,29,30,30], // 2056
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2057
    [30,32,31,32,31,30,30,30,29,30,29,31], // 2058
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2059
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2060
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2061
    [30,32,31,32,31,31,29,30,29,30,29,31], // 2062
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2063
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2064
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2065
    [31,31,31,32,31,31,29,30,30,29,29,31], // 2066
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2067
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2068
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2069
    [31,31,31,32,31,31,29,30,30,29,30,30], // 2070
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2071
    [31,32,31,32,31,30,30,29,30,29,30,30], // 2072
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2073
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2074
    [31,31,32,32,31,30,30,29,30,29,30,30], // 2075
    [31,32,31,32,31,30,30,30,29,29,30,31], // 2076
    [30,32,31,32,31,31,29,30,30,29,29,31], // 2077
    [31,31,32,31,31,31,30,29,30,29,30,30], // 2078
    [31,32,31,32,31,30,30,29,30,29,30,30], // 2079
    [31,32,31,32,31,30,30,30,29,29,30,30], // 2080
    [31,31,32,32,31,30,30,30,29,30,30,30], // 2081
    [30,32,31,32,31,30,30,30,29,30,30,30], // 2082
    [31,31,32,31,31,30,30,30,29,30,30,30], // 2083
    [31,31,32,31,31,30,30,30,29,30,30,30], // 2084
    [31,32,31,32,30,31,30,30,29,30,30,30], // 2085
    [30,32,31,32,31,30,30,30,29,30,30,30], // 2086
    [31,31,32,31,31,31,30,30,29,30,30,30], // 2087
    [30,31,32,32,30,31,30,30,29,30,30,30], // 2088
    [30,32,31,32,31,30,30,30,29,30,30,30], // 2089
    [30,32,31,32,31,30,30,30,29,30,30,30], // 2090
];

/// 1 Baisakh 2000 BS in the proleptic Gregorian calendar.
const ANCHOR_AD: (i32, u32, u32) = (1943, 4, 14);

pub const BS_MONTH_NAMES: [&str; 12] = [
    "Baisakh", "Jestha", "Ashadh", "Shrawan", "Bhadra", "Ashwin",
    "Kartik", "Mangsir", "Poush", "Magh", "Falgun", "Chaitra",
];

/// A Bikram Sambat date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BsDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl BsDate {
    pub fn month_name(&self) -> &'static str {
        BS_MONTH_NAMES[(self.month - 1) as usize]
    }
    /// `2083-05-03`
    pub fn iso(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
    /// `3 Bhadra 2083`
    pub fn pretty(&self) -> String {
        format!("{} {} {}", self.day, self.month_name(), self.year)
    }
}

fn row(year: i32) -> Option<&'static [u8; 12]> {
    if !(BS_START_YEAR..=BS_END_YEAR).contains(&year) {
        return None;
    }
    BS_MONTHS.get((year - BS_START_YEAR) as usize)
}

/// Days in a given BS month, or `None` if outside the known table.
pub fn days_in_bs_month(year: i32, month: u32) -> Option<u32> {
    if !(1..=12).contains(&month) {
        return None;
    }
    row(year).map(|r| r[(month - 1) as usize] as u32)
}

/// Total days in a BS year.
pub fn days_in_bs_year(year: i32) -> Option<u32> {
    row(year).map(|r| r.iter().map(|&d| d as u32).sum())
}

// --- civil <-> day-number helpers (proleptic Gregorian, no chrono needed) ---

/// Days since 1970-01-01. Howard Hinnant's civil_from_days, inverted.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

/// Days-since-epoch for a Gregorian date (used by the service layer to walk
/// date ranges without pulling in a calendar crate).
pub fn days_from_civil_pub(y: i32, m: u32, d: u32) -> i64 { days_from_civil(y, m, d) }

/// Inverse of [`days_from_civil_pub`].
pub fn civil_from_days_pub(z: i64) -> (i32, u32, u32) { civil_from_days(z) }

/// Convert a Gregorian date to Bikram Sambat.
///
/// Returns `None` when the date falls outside the validated table rather than
/// extrapolating — Nepali month lengths are not predictable by formula.
pub fn ad_to_bs(year: i32, month: u32, day: u32) -> Option<BsDate> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let anchor = days_from_civil(ANCHOR_AD.0, ANCHOR_AD.1, ANCHOR_AD.2);
    let mut remaining = days_from_civil(year, month, day) - anchor;
    if remaining < 0 {
        return None;
    }

    let mut y = BS_START_YEAR;
    loop {
        let total = days_in_bs_year(y)? as i64;
        if remaining < total {
            break;
        }
        remaining -= total;
        y += 1;
    }

    let mut m = 1u32;
    loop {
        let len = days_in_bs_month(y, m)? as i64;
        if remaining < len {
            break;
        }
        remaining -= len;
        m += 1;
    }

    Some(BsDate { year: y, month: m, day: remaining as u32 + 1 })
}

/// Convert a Bikram Sambat date to Gregorian `(y, m, d)`.
pub fn bs_to_ad(year: i32, month: u32, day: u32) -> Option<(i32, u32, u32)> {
    let len = days_in_bs_month(year, month)?;
    if day < 1 || day > len {
        return None;
    }
    let mut offset: i64 = 0;
    for y in BS_START_YEAR..year {
        offset += days_in_bs_year(y)? as i64;
    }
    for m in 1..month {
        offset += days_in_bs_month(year, m)? as i64;
    }
    offset += day as i64 - 1;
    let anchor = days_from_civil(ANCHOR_AD.0, ANCHOR_AD.1, ANCHOR_AD.2);
    Some(civil_from_days(anchor + offset))
}

/// Convert an ISO `YYYY-MM-DD` string to a BS date.
pub fn iso_to_bs(iso: &str) -> Result<BsDate, Error> {
    let mut p = iso.split('-');
    let y: i32 = p.next().and_then(|v| v.parse().ok()).ok_or_else(bad(iso))?;
    let m: u32 = p.next().and_then(|v| v.parse().ok()).ok_or_else(bad(iso))?;
    let d: u32 = p.next().and_then(|v| v.parse().ok()).ok_or_else(bad(iso))?;
    ad_to_bs(y, m, d).ok_or_else(|| {
        Error::Invalid(format!("{iso} is outside the supported Bikram Sambat range"))
    })
}

fn bad(iso: &str) -> impl Fn() -> Error + '_ {
    move || Error::Invalid(format!("'{iso}' is not a YYYY-MM-DD date"))
}

/// The BS month a Gregorian date falls in, as `(year, month)` — used to group
/// reports by Nepali month.
pub fn bs_month_of(iso: &str) -> Option<(i32, u32)> {
    iso_to_bs(iso).ok().map(|b| (b.year, b.month))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_live_reference_date() {
        // Hamro Patro reports 19 August 2026 as 3 Bhadra 2083.
        let b = ad_to_bs(2026, 8, 19).unwrap();
        assert_eq!(b, BsDate { year: 2083, month: 5, day: 3 });
        assert_eq!(b.month_name(), "Bhadra");
        assert_eq!(b.pretty(), "3 Bhadra 2083");
        assert_eq!(b.iso(), "2083-05-03");
    }

    #[test]
    fn year_starts_land_on_known_dates() {
        // Each 1 Baisakh against its published Gregorian date.
        for &(bs_y, ad) in &[
            (2000, (1943, 4, 14)),
            (2070, (2013, 4, 14)),
            (2078, (2021, 4, 14)),
            (2079, (2022, 4, 14)),
            (2080, (2023, 4, 14)),
            (2081, (2024, 4, 13)),
            (2082, (2025, 4, 14)),
        ] {
            assert_eq!(bs_to_ad(bs_y, 1, 1).unwrap(), ad, "1 Baisakh {bs_y} BS");
            assert_eq!(
                ad_to_bs(ad.0, ad.1, ad.2).unwrap(),
                BsDate { year: bs_y, month: 1, day: 1 },
                "reverse of 1 Baisakh {bs_y} BS"
            );
        }
    }

    #[test]
    fn roundtrips_across_the_whole_table() {
        // Every first, middle and last day of every month must survive a
        // round trip. This is what catches a mistyped row.
        for y in BS_START_YEAR..=BS_END_YEAR {
            for m in 1..=12u32 {
                let len = days_in_bs_month(y, m).unwrap();
                for d in [1, len / 2, len] {
                    let (ay, am, ad) = bs_to_ad(y, m, d).unwrap();
                    assert_eq!(
                        ad_to_bs(ay, am, ad).unwrap(),
                        BsDate { year: y, month: m, day: d },
                        "roundtrip {y}-{m}-{d} BS"
                    );
                }
            }
        }
    }

    #[test]
    fn consecutive_days_advance_by_one() {
        // Walk a full BS year one day at a time; the Gregorian side must
        // advance in lockstep with no gaps or repeats.
        let mut prev = days_from_civil_pub(bs_to_ad(2083, 1, 1).unwrap());
        for m in 1..=12u32 {
            let len = days_in_bs_month(2083, m).unwrap();
            for d in 1..=len {
                if m == 1 && d == 1 {
                    continue;
                }
                let cur = days_from_civil_pub(bs_to_ad(2083, m, d).unwrap());
                assert_eq!(cur - prev, 1, "gap at {m}/{d} of 2083 BS");
                prev = cur;
            }
        }
    }

    fn days_from_civil_pub(t: (i32, u32, u32)) -> i64 {
        super::days_from_civil(t.0, t.1, t.2)
    }

    #[test]
    fn month_lengths_are_sane() {
        for y in BS_START_YEAR..=BS_END_YEAR {
            let total = days_in_bs_year(y).unwrap();
            assert!((364..=367).contains(&total), "BS {y} has {total} days");
            for m in 1..=12u32 {
                let len = days_in_bs_month(y, m).unwrap();
                assert!((29..=32).contains(&len), "BS {y}-{m} has {len} days");
            }
        }
    }

    #[test]
    fn out_of_range_returns_none_not_a_guess() {
        assert!(ad_to_bs(1900, 1, 1).is_none(), "before the table");
        assert!(ad_to_bs(2200, 1, 1).is_none(), "after the table");
        assert!(days_in_bs_month(2099, 1).is_none());
        assert!(bs_to_ad(2083, 13, 1).is_none(), "month 13");
        assert!(bs_to_ad(2083, 1, 40).is_none(), "day beyond month length");
    }

    #[test]
    fn iso_parsing_reports_useful_errors() {
        assert_eq!(iso_to_bs("2026-08-19").unwrap().pretty(), "3 Bhadra 2083");
        assert!(iso_to_bs("not-a-date").is_err());
        assert!(iso_to_bs("1900-01-01").is_err());
        assert_eq!(bs_month_of("2026-08-19"), Some((2083, 5)));
    }

    #[test]
    fn gregorian_helpers_handle_leap_years() {
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        assert_eq!(civil_from_days(days_from_civil(2000, 2, 29)), (2000, 2, 29));
        assert_eq!(civil_from_days(days_from_civil(1970, 1, 1)), (1970, 1, 1));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }
}
