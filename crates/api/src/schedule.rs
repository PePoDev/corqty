//! Standard five-field cron schedules in an IANA timezone. Day-of-month and
//! day-of-week use traditional OR semantics when both fields are restricted.
use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;

use crate::error::ApiError;

pub struct CronSchedule {
    schedules: Vec<Schedule>,
    timezone: Tz,
}

impl CronSchedule {
    pub fn parse(expression: &str, timezone: &str) -> Result<Self, ApiError> {
        if expression.len() > 200 {
            return Err(ApiError::Validation("invalid_cron"));
        }
        let fields: Vec<_> = expression.split_whitespace().collect();
        if fields.len() != 5 || fields.iter().any(|field| field.contains('?')) {
            return Err(ApiError::Validation("invalid_cron"));
        }
        let timezone = timezone
            .parse()
            .map_err(|_| ApiError::Validation("invalid_timezone"))?;
        let weekday = weekdays(fields[4]).ok_or(ApiError::Validation("invalid_cron"))?;
        let expressions = if !fields[2].starts_with('*') && !fields[4].starts_with('*') {
            vec![
                format!(
                    "0 {} {} {} {} *",
                    fields[0], fields[1], fields[2], fields[3]
                ),
                format!("0 {} {} * {} {}", fields[0], fields[1], fields[3], weekday),
            ]
        } else {
            vec![format!(
                "0 {} {} {} {} {}",
                fields[0], fields[1], fields[2], fields[3], weekday
            )]
        };
        let schedules = expressions
            .iter()
            .map(|expression| {
                Schedule::from_str(expression).map_err(|_| ApiError::Validation("invalid_cron"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            schedules,
            timezone,
        })
    }

    pub fn next_after(&self, time: DateTime<Utc>) -> Result<DateTime<Utc>, ApiError> {
        // A fresh cron iterator forgets its cached second instant of a DST fold.
        // Walk UTC minutes near the cursor so restarts cannot skip that instant or
        // return an earlier UTC instant from the repeated local hour. This also
        // handles non-hour transitions without assumptions about a zone's offset.
        let mut candidate = DateTime::from_timestamp(time.timestamp().div_euclid(60) * 60 + 60, 0)
            .ok_or(ApiError::Validation("cron_has_no_future_occurrence"))?;
        for _ in 0..(48 * 60) {
            let local = candidate.with_timezone(&self.timezone);
            if self
                .schedules
                .iter()
                .any(|schedule| schedule.includes(local))
            {
                return Ok(candidate);
            }
            candidate += chrono::Duration::minutes(1);
        }
        // Sparse schedules jump efficiently across months/years. Any repeated
        // instant whose earlier twin is near the original cursor was covered above.
        let local = time.with_timezone(&self.timezone);
        self.schedules
            .iter()
            .filter_map(|schedule| {
                schedule
                    .after(&local)
                    .find(|next| next.with_timezone(&Utc) > time)
            })
            .map(|time| time.with_timezone(&Utc))
            .min()
            .ok_or(ApiError::Validation("cron_has_no_future_occurrence"))
    }
}

fn weekday(value: &str) -> Option<u32> {
    match value.to_ascii_lowercase().as_str() {
        "sun" => Some(0),
        "mon" => Some(1),
        "tue" => Some(2),
        "wed" => Some(3),
        "thu" => Some(4),
        "fri" => Some(5),
        "sat" => Some(6),
        _ => value.parse::<u32>().ok().filter(|value| *value <= 7),
    }
}

fn weekdays(field: &str) -> Option<String> {
    if field == "*" {
        return Some("*".to_owned());
    }
    let mut days = std::collections::BTreeSet::new();
    for item in field.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => (
                range,
                step.parse::<usize>()
                    .ok()
                    .filter(|step| (1..=7).contains(step))?,
            ),
            None => (item, 1),
        };
        let (start, end) = if range == "*" {
            (0, 6)
        } else if let Some((start, end)) = range.split_once('-') {
            (weekday(start)?, weekday(end)?)
        } else {
            let day = weekday(range)?;
            (day, if item.contains('/') { 7 } else { day })
        };
        if end < start {
            return None;
        }
        for day in (start..=end).step_by(step) {
            days.insert(day % 7 + 1);
        }
    }
    if days.is_empty() {
        return None;
    }
    Some(
        days.into_iter()
            .map(|day| day.to_string())
            .collect::<Vec<_>>()
            .join(","),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }

    #[test]
    fn five_fields_timezone_and_standard_sunday_are_enforced() {
        let schedule = CronSchedule::parse("0 9 * * 0", "Asia/Bangkok").unwrap();
        assert_eq!(
            schedule.next_after(at("2026-11-04T00:00:00Z")).unwrap(),
            at("2026-11-08T02:00:00Z")
        );
        assert!(CronSchedule::parse("0 * * * * *", "UTC").is_err());
        assert!(CronSchedule::parse("* * * * *", "Invalid/Zone").is_err());
    }

    #[test]
    fn restricted_days_use_or_semantics_and_dst_gaps_are_skipped() {
        let schedule = CronSchedule::parse("0 0 1 * MON", "UTC").unwrap();
        assert_eq!(
            schedule.next_after(at("2026-11-03T00:00:00Z")).unwrap(),
            at("2026-11-09T00:00:00Z")
        );
        let schedule = CronSchedule::parse("30 2 * * *", "America/New_York").unwrap();
        assert_eq!(
            schedule.next_after(at("2026-03-08T00:00:00Z")).unwrap(),
            at("2026-03-09T06:30:00Z")
        );
    }

    #[test]
    fn fresh_iterators_resume_both_fold_instants_and_never_go_backwards() {
        let schedule = CronSchedule::parse("30 1 * * *", "America/New_York").unwrap();
        assert_eq!(
            schedule.next_after(at("2026-11-01T05:30:00Z")).unwrap(),
            at("2026-11-01T06:30:00Z")
        );
        assert_eq!(
            schedule.next_after(at("2026-11-01T06:15:00Z")).unwrap(),
            at("2026-11-01T06:30:00Z")
        );
        assert_eq!(
            schedule.next_after(at("2026-11-01T06:30:00Z")).unwrap(),
            at("2026-11-02T06:30:00Z")
        );
        let every_minute = CronSchedule::parse("* * * * *", "America/New_York").unwrap();
        assert_eq!(
            every_minute.next_after(at("2026-11-01T05:59:00Z")).unwrap(),
            at("2026-11-01T06:00:00Z")
        );
    }

    #[test]
    fn wildcard_day_steps_preserve_the_other_day_restriction() {
        let monday = CronSchedule::parse("0 0 */1 * MON", "UTC").unwrap();
        assert_eq!(
            monday.next_after(at("2026-11-04T00:00:00Z")).unwrap(),
            at("2026-11-09T00:00:00Z")
        );
        let first = CronSchedule::parse("0 0 1 * */1", "UTC").unwrap();
        assert_eq!(
            first.next_after(at("2026-11-04T00:00:00Z")).unwrap(),
            at("2026-12-01T00:00:00Z")
        );
    }
}
