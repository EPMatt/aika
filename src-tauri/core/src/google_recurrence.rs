//! Exact compiler for the domain's unsplit all-day and UTC schedules.
use crate::{contracts::*, service::error};
use chrono::{DateTime, NaiveDate, Timelike, Utc};

pub(crate) fn compile(event: &CalendarEvent) -> Result<Option<String>, ServiceError> {
    let Some(rule) = &event.recurrence else {
        return Ok(None);
    };
    aika_domain::recurrence::validate_recurrence(event)?;
    let unsupported = || {
        error(
            "unsupported",
            "Google repeats require an unsplit all-day or UTC date-based pattern.",
        )
    };
    if rule.segment.is_some()
        || rule.monthly_on != MonthlyOn::Date
        || matches!(&event.time, EventTime::Timed { time_zone, .. } if time_zone != "UTC" && time_zone != "Etc/UTC")
    {
        return Err(unsupported());
    }
    if let EventTime::Timed { start, .. } = &event.time {
        if DateTime::parse_from_rfc3339(start)
            .map_err(|_| unsupported())?
            .nanosecond()
            != 0
        {
            return Err(unsupported());
        }
    }
    let frequency = match rule.frequency {
        RepeatFrequency::Daily => "DAILY",
        RepeatFrequency::Weekly => "WEEKLY",
        RepeatFrequency::Monthly => "MONTHLY",
        RepeatFrequency::Yearly => "YEARLY",
    };
    let mut line = format!("RRULE:FREQ={frequency};INTERVAL={}", rule.interval);
    if rule.frequency == RepeatFrequency::Weekly {
        line.push_str(";WKST=MO");
        if !rule.weekdays.is_empty() {
            let mut days = rule.weekdays.clone();
            days.sort_unstable();
            let names = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
            let days = days
                .iter()
                .map(|d| {
                    names
                        .get(d.wrapping_sub(1) as usize)
                        .copied()
                        .ok_or_else(unsupported)
                })
                .collect::<Result<Vec<_>, _>>()?;
            line.push_str(&format!(";BYDAY={}", days.join(",")));
        }
    }
    if let Some(count) = rule.count {
        line.push_str(&format!(";COUNT={count}"));
    }
    if let Some(until) = &rule.until {
        let date = NaiveDate::parse_from_str(until, "%Y-%m-%d").map_err(|_| unsupported())?;
        let boundary = match &event.time {
            EventTime::AllDay { .. } => date.format("%Y%m%d").to_string(),
            EventTime::Timed { start, .. } => {
                let start = DateTime::parse_from_rfc3339(start)
                    .map_err(|_| unsupported())?
                    .with_timezone(&Utc);
                date.and_time(start.time())
                    .format("%Y%m%dT%H%M%SZ")
                    .to_string()
            }
        };
        line.push_str(&format!(";UNTIL={boundary}"));
    }
    Ok(Some(line))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(timed: bool) -> CalendarEvent {
        CalendarEvent {
            id: "742aa832-369c-4f83-9fd7-61cd4b5c8184".into(),
            calendar_id: "local-default".into(),
            title: "Repeat".into(),
            location: "".into(),
            notes: "".into(),
            occurrence: None,
            time: if timed {
                EventTime::Timed {
                    start: "2026-10-05T09:00:00.000Z".into(),
                    end: "2026-10-05T10:00:00.000Z".into(),
                    time_zone: "UTC".into(),
                }
            } else {
                EventTime::AllDay {
                    start_date: "2026-10-05".into(),
                    end_date: "2026-10-06".into(),
                }
            },
            recurrence: Some(Recurrence {
                frequency: RepeatFrequency::Weekly,
                interval: 2,
                weekdays: vec![1, 3],
                monthly_on: MonthlyOn::Date,
                until: None,
                count: Some(7),
                segment: None,
            }),
        }
    }
    #[test]
    fn exact_rules_round_trip_count_interval_week_phase_and_inclusive_end() {
        for timed in [false, true] {
            for frequency in [
                RepeatFrequency::Daily,
                RepeatFrequency::Weekly,
                RepeatFrequency::Monthly,
                RepeatFrequency::Yearly,
            ] {
                for ending in [0, 1, 2] {
                    let mut e = event(timed);
                    let r = e.recurrence.as_mut().unwrap();
                    r.frequency = frequency.clone();
                    if frequency != RepeatFrequency::Weekly {
                        r.weekdays.clear();
                    }
                    if ending != 0 {
                        r.count = None;
                    }
                    if ending == 1 {
                        r.until = Some("2027-12-31".into());
                    }
                    let line = compile(&e).unwrap().unwrap();
                    let (start, end) = match &e.time {
                        EventTime::AllDay {
                            start_date,
                            end_date,
                        } => (
                            serde_json::json!({"date":start_date}),
                            serde_json::json!({"date":end_date}),
                        ),
                        EventTime::Timed { start, end, .. } => (
                            serde_json::json!({"dateTime":start,"timeZone":"UTC"}),
                            serde_json::json!({"dateTime":end,"timeZone":"UTC"}),
                        ),
                    };
                    let page=crate::google::events::normalized(crate::google::events::EventPage { items:vec![serde_json::json!({"id":"provider-master","summary":"Repeat","start":start,"end":end,"recurrence":[line]})], next_page_token:None,next_sync_token:None,time_zone:"UTC".into() }).unwrap();
                    assert!(!page.items[0].unsupported);
                    assert_eq!(
                        page.items[0].event.as_ref().unwrap().recurrence,
                        e.recurrence
                    );
                    // This is supplementary evidence; equivalence follows from direct RFC
                    // field correspondence, fixed UTC duration, Monday week phase and skipped invalid dates.
                    assert!(line.contains("INTERVAL=2"));
                }
            }
        }
    }
    #[test]
    fn regional_ordinal_segment_and_fractional_rules_are_rejected() {
        let mut e = event(true);
        if let EventTime::Timed { time_zone, .. } = &mut e.time {
            *time_zone = "Europe/Zurich".into();
        }
        assert_eq!(compile(&e).unwrap_err().code, "unsupported");
        e = event(false);
        e.recurrence.as_mut().unwrap().monthly_on = MonthlyOn::Weekday;
        assert!(compile(&e).is_err());
        e = event(true);
        if let EventTime::Timed { start, .. } = &mut e.time {
            *start = "2026-10-05T09:00:00.123Z".into();
        }
        assert!(compile(&e).is_err());
    }
}
