//! Time frames for analysis, shared by every endpoint that takes one.
//!
//! A time frame is either a `range` or an explicit `from`/`to`:
//!
//! - `range`: `all`, `today`, `yesterday`, or a number and a unit: `24h`,
//!   `7d`, `2w`, `1m` (calendar months), `1y`. Rolling: it ends now.
//! - `from` / `to`: a date (`2026-11-01`, UTC; a `to` date includes that
//!   whole day), an RFC 3339 time, or unix seconds. `to` defaults to now.
//!
//! Endpoints that work on whole UTC days (traction reports, daily tables)
//! use [`TimeFrame::days`] to turn the frame into a day span.

use chrono::{DateTime, Months, NaiveDate, TimeZone, Utc};
use serde::Serialize;

use crate::errors::AppError;

/// The default when an endpoint gets no time frame.
pub const DEFAULT_RANGE: &str = "7d";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TimeFrame {
    /// Start, unix seconds (inclusive).
    pub from: i64,
    /// End, unix seconds (exclusive).
    pub to: i64,
    /// What was asked for, e.g. "7d", "all", or "custom".
    pub label: String,
    /// True for `all`: from the start of collected data.
    pub all_time: bool,
}

/// A span of whole UTC days, both ends included.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DaySpan {
    pub from: String,
    pub to: String,
    pub days: i64,
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::BadRequest(msg.into())
}

fn midnight(d: NaiveDate) -> i64 {
    d.and_hms_opt(0, 0, 0).map(|t| t.and_utc().timestamp()).unwrap_or(0)
}

fn date_of(ts: i64) -> NaiveDate {
    DateTime::from_timestamp(ts, 0).map(|t| t.date_naive()).unwrap_or_default()
}

/// A point in time: a date, an RFC 3339 time or unix seconds. A date is
/// its midnight, or the next midnight when `end_of_day` (so `to=2026-11-01`
/// includes the 1st).
fn instant(s: &str, end_of_day: bool) -> Result<i64, AppError> {
    let s = s.trim();
    if let Ok(n) = s.parse::<i64>() {
        return Ok(n);
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Ok(midnight(if end_of_day { d.succ_opt().unwrap_or(d) } else { d }));
    }
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.timestamp())
        .map_err(|_| bad(format!("Can't read the time \"{s}\": use a date (2026-11-01), an ISO time or unix seconds")))
}

/// Parses a time frame. `now` is the current time and `data_start` the time
/// of the oldest collected data (the start of `all`).
pub fn parse(
    range: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
    now: i64,
    data_start: Option<i64>,
) -> Result<TimeFrame, AppError> {
    let range = range.map(str::trim).filter(|r| !r.is_empty());
    let from = from.map(str::trim).filter(|r| !r.is_empty());
    let to_given = to.map(str::trim).filter(|r| !r.is_empty());
    if range.is_some() && (from.is_some() || to_given.is_some()) {
        return Err(bad("Give either range or from/to, not both"));
    }
    let frame = if let Some(f) = from {
        let start = instant(f, false)?;
        let end = match to_given {
            Some(t) => instant(t, true)?,
            None => now,
        }
        .min(now);
        TimeFrame { from: start, to: end, label: "custom".into(), all_time: false }
    } else if to_given.is_some() {
        return Err(bad("A custom time frame needs from (to is optional)"));
    } else {
        let range = range.unwrap_or(DEFAULT_RANGE).to_ascii_lowercase();
        let today = midnight(date_of(now));
        let start = match range.as_str() {
            "all" | "all_time" | "alltime" => data_start.unwrap_or(now).min(now),
            "today" => today,
            "yesterday" => {
                return Ok(TimeFrame { from: today - 86_400, to: today, label: range, all_time: false });
            }
            other => {
                let split = other.find(|c: char| !c.is_ascii_digit()).ok_or_else(|| unknown(other))?;
                let (n, unit) = other.split_at(split);
                let n: i64 = n.parse().map_err(|_| unknown(other))?;
                if n <= 0 {
                    return Err(bad(format!("The range \"{other}\" must be longer than zero")));
                }
                match unit {
                    "h" | "hour" | "hours" => now - n * 3600,
                    "d" | "day" | "days" => now - n * 86_400,
                    "w" | "week" | "weeks" => now - n * 7 * 86_400,
                    "m" | "mo" | "month" | "months" => months_before(now, n)?,
                    "y" | "year" | "years" => months_before(now, n.saturating_mul(12))?,
                    _ => return Err(unknown(other)),
                }
            }
        };
        TimeFrame { from: start, to: now, all_time: range.starts_with("all"), label: range }
    };
    if frame.from >= frame.to {
        return Err(bad("The time frame is empty: from must be before to (and before now)"));
    }
    Ok(frame)
}

fn unknown(r: &str) -> AppError {
    bad(format!(
        "Unknown range \"{r}\": use all, today, yesterday, or a number and unit such as 24h, 7d, 2w, 1m, 1y"
    ))
}

fn months_before(now: i64, n: i64) -> Result<i64, AppError> {
    let t = Utc.timestamp_opt(now, 0).single().ok_or_else(|| bad("Bad current time"))?;
    let months = u32::try_from(n).map_err(|_| bad("Range too long"))?;
    Ok(t.checked_sub_months(Months::new(months)).map(|t| t.timestamp()).unwrap_or(0))
}

impl TimeFrame {
    /// Length in seconds.
    pub fn seconds(&self) -> i64 {
        self.to - self.from
    }

    /// The whole UTC days the frame touches, e.g. a 24-hour window ending at
    /// 10:00 touches yesterday and today.
    pub fn days(&self) -> DaySpan {
        let first = date_of(self.from);
        let last = date_of(self.to - 1);
        DaySpan {
            from: first.to_string(),
            to: last.to_string(),
            days: (last - first).num_days() + 1,
        }
    }

    /// Whole UTC days for day-based results (reports): `today` and anything
    /// under a day is one day, `Nd` is N days ending today, longer ranges are
    /// the days they touch; `all` starts on the first collected day.
    pub fn report_days(&self) -> DaySpan {
        if self.label.ends_with('d') && self.label[..self.label.len() - 1].chars().all(|c| c.is_ascii_digit()) {
            // "7d" means the last 7 calendar days including today, as before.
            let n: i64 = self.label[..self.label.len() - 1].parse().unwrap_or(1);
            let last = date_of(self.to - 1);
            let first = last - chrono::Duration::days(n - 1);
            return DaySpan { from: first.to_string(), to: last.to_string(), days: n };
        }
        self.days()
    }
}

/// `YYYY-MM-DD` of a unix time, UTC.
pub fn day(ts: i64) -> String {
    date_of(ts).to_string()
}

/// Start of the UTC day `YYYY-MM-DD`.
pub fn day_start(day: &str) -> Option<i64> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").ok().map(midnight)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 2026-11-16 15:00:00 UTC
    const NOW: i64 = 1_794_841_200;

    fn p(range: Option<&str>, from: Option<&str>, to: Option<&str>) -> Result<TimeFrame, AppError> {
        parse(range, from, to, NOW, Some(NOW - 40 * 86_400))
    }

    #[test]
    fn rolling_ranges_end_now() {
        assert_eq!(day(NOW), "2026-11-16");
        let f = p(Some("24h"), None, None).unwrap();
        assert_eq!((f.from, f.to, f.seconds()), (NOW - 86_400, NOW, 86_400));
        assert_eq!(p(Some("7d"), None, None).unwrap().seconds(), 7 * 86_400);
        assert_eq!(p(Some("2w"), None, None).unwrap().seconds(), 14 * 86_400);
        assert_eq!(p(None, None, None).unwrap().label, "7d", "default");
        // Calendar month: 2026-10-16 15:00.
        let m = p(Some("1m"), None, None).unwrap();
        assert_eq!(day(m.from), "2026-10-16");
        assert_eq!(day(p(Some("1y"), None, None).unwrap().from), "2025-11-16");
        assert_eq!(p(Some("90D"), None, None).unwrap().seconds(), 90 * 86_400, "case-insensitive");
    }

    #[test]
    fn named_ranges() {
        let all = p(Some("all"), None, None).unwrap();
        assert!(all.all_time);
        assert_eq!(all.from, NOW - 40 * 86_400);
        // No data yet: all-time is empty.
        assert!(parse(Some("all"), None, None, NOW, None).is_err());
        let today = p(Some("today"), None, None).unwrap();
        assert_eq!((day(today.from), today.seconds()), ("2026-11-16".into(), 15 * 3600));
        let y = p(Some("yesterday"), None, None).unwrap();
        assert_eq!((day(y.from), y.seconds()), ("2026-11-15".into(), 86_400));
    }

    #[test]
    fn custom_frames() {
        let f = p(None, Some("2026-11-01"), Some("2026-11-03")).unwrap();
        assert_eq!(f.days(), DaySpan { from: "2026-11-01".into(), to: "2026-11-03".into(), days: 3 });
        assert_eq!(f.label, "custom");
        // `to` defaults to now and is capped at now.
        let open = p(None, Some("2026-11-10T12:00:00Z"), None).unwrap();
        assert_eq!(open.to, NOW);
        assert_eq!(p(None, Some("2026-11-10"), Some("2030-01-01")).unwrap().to, NOW);
        let unix = p(None, Some(&(NOW - 3600).to_string()), None).unwrap();
        assert_eq!(unix.seconds(), 3600);
    }

    #[test]
    fn rejects_bad_frames() {
        for (r, f, t) in [
            (Some("7x"), None, None),
            (Some("0d"), None, None),
            (Some("week"), None, None),
            (Some("-3d"), None, None),
            (Some("7d"), Some("2026-11-01"), None),
            (None, None, Some("2026-11-01")),
            (None, Some("2026-11-05"), Some("2026-11-01")),
            (None, Some("2027-01-01"), None),
            (None, Some("last tuesday"), None),
        ] {
            assert!(p(r, f, t).is_err(), "{r:?} {f:?} {t:?} should be rejected");
        }
    }

    #[test]
    fn day_spans() {
        // A 24-hour window ending at 15:00 touches two days.
        assert_eq!(p(Some("24h"), None, None).unwrap().days().days, 2);
        // Reports keep "7d" as the last 7 calendar days including today.
        let r = p(Some("7d"), None, None).unwrap().report_days();
        assert_eq!((r.from.as_str(), r.to.as_str(), r.days), ("2026-11-10", "2026-11-16", 7));
        assert_eq!(p(Some("today"), None, None).unwrap().report_days().days, 1);
        assert_eq!(p(Some("all"), None, None).unwrap().report_days().days, 41);
        assert_eq!(day_start("2026-11-16"), Some(NOW - 15 * 3600));
    }
}
