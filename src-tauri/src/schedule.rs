//! VCR timer programs: when should Rewind look for a channel's stream?

use crate::store::Timer;
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike, Weekday};

/// Start looking this many minutes before the programmed time
/// (streamers often go live a little early).
pub const LEAD_MINUTES: i64 = 10;

fn bit(day: Weekday) -> u8 {
    1 << day.num_days_from_monday()
}

fn usable(t: &Timer) -> bool {
    t.enabled && t.days & 0x7f != 0 && t.start < 1440 && t.end < 1440
}

fn inside<Tz: TimeZone>(t: &Timer, at: &DateTime<Tz>) -> bool {
    let minute = (at.hour() * 60 + at.minute()) as u16;
    let today = t.days & bit(at.weekday()) != 0;
    let yesterday = t.days & bit(at.weekday().pred()) != 0;
    if t.start < t.end {
        today && minute >= t.start && minute < t.end
    } else {
        // Runs past midnight (or a full day when start == end).
        (today && minute >= t.start) || (yesterday && minute < t.end)
    }
}

/// True when the channel has at least one usable timer program.
pub fn scheduled(timers: &[Timer]) -> bool {
    timers.iter().any(usable)
}

/// Inside a program window, counting the early start.
pub fn active_at<Tz: TimeZone>(timers: &[Timer], at: &DateTime<Tz>) -> bool {
    let early = at.clone() + Duration::minutes(LEAD_MINUTES);
    timers.iter().filter(|t| usable(t)).any(|t| inside(t, at) || inside(t, &early))
}

pub fn active(timers: &[Timer]) -> bool {
    active_at(timers, &Local::now())
}

/// Next programmed start time after `at` (unix ms), within the coming 8 days.
pub fn next_start_at<Tz: TimeZone>(timers: &[Timer], at: &DateTime<Tz>) -> Option<i64> {
    let tz = at.timezone();
    let mut best: Option<i64> = None;
    for offset in 0..8 {
        let date = at.date_naive() + Duration::days(offset);
        for t in timers.iter().filter(|t| usable(t) && t.days & bit(date.weekday()) != 0) {
            let Some(naive) = date.and_hms_opt((t.start / 60) as u32, (t.start % 60) as u32, 0) else {
                continue;
            };
            let Some(when) = tz.from_local_datetime(&naive).earliest() else {
                continue;
            };
            let ms = when.timestamp_millis();
            if ms > at.timestamp_millis() && best.is_none_or(|b| ms < b) {
                best = Some(ms);
            }
        }
    }
    best
}

pub fn next_start(timers: &[Timer]) -> Option<i64> {
    next_start_at(timers, &Local::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, NaiveDate};

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<FixedOffset> {
        let tz = FixedOffset::east_opt(2 * 3600).unwrap();
        tz.from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()).unwrap()
    }

    const MON: u8 = 1;
    const FRI: u8 = 1 << 4;

    #[test]
    fn window_with_early_start() {
        // Friday 21:00-23:00. 2026-11-27 is a Friday.
        let t = vec![Timer { days: FRI, start: 21 * 60, end: 23 * 60, enabled: true }];
        assert!(!active_at(&t, &at(2026, 11, 27, 20, 40)));
        assert!(active_at(&t, &at(2026, 11, 27, 20, 50)), "10 minutes early");
        assert!(active_at(&t, &at(2026, 11, 27, 22, 59)));
        assert!(!active_at(&t, &at(2026, 11, 27, 23, 0)));
        assert!(!active_at(&t, &at(2026, 11, 26, 21, 30)), "Thursday");
    }

    #[test]
    fn past_midnight() {
        // Monday 22:00 -> Tuesday 02:00. 2026-11-23 is a Monday.
        let t = vec![Timer { days: MON, start: 22 * 60, end: 2 * 60, enabled: true }];
        assert!(active_at(&t, &at(2026, 11, 23, 23, 30)));
        assert!(active_at(&t, &at(2026, 11, 24, 1, 59)), "Tuesday early morning belongs to Monday");
        assert!(!active_at(&t, &at(2026, 11, 24, 2, 0)));
        assert!(!active_at(&t, &at(2026, 11, 24, 23, 0)), "Tuesday evening is not programmed");
    }

    #[test]
    fn disabled_and_next_start() {
        let mut t = vec![Timer { days: FRI, start: 21 * 60, end: 23 * 60, enabled: false }];
        assert!(!scheduled(&t));
        assert!(!active_at(&t, &at(2026, 11, 27, 21, 30)));
        t[0].enabled = true;
        // From Tuesday noon, the next start is Friday 21:00.
        let next = next_start_at(&t, &at(2026, 11, 24, 12, 0)).unwrap();
        assert_eq!(next, at(2026, 11, 27, 21, 0).timestamp_millis());
    }
}
