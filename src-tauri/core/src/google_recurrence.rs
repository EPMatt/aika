//! Exact compiler for canonical all-day and UTC schedules.
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

/// The domain split retains its source anchor to model phase. For these fixed
/// date patterns every pivot is itself a scheduled start: daily/monthly/yearly
/// modulus is unchanged, and weekly phase is the pivot's Monday week. Therefore
/// reanchoring the tail is exact, including interval>1 and skipped invalid dates.
/// Head COUNT is computed by the domain before applying exceptions, so it ends
/// strictly before the pivot. No preview-based equivalence decision is used.
pub(crate) fn canonicalize_split(changes: &mut [SeriesChange]) -> Result<(), ServiceError> {
    if let Some(before) = changes.first().and_then(|c| c.before.as_ref()) {
        compile(&before.event)?;
    }
    for change in changes {
        if let Some(after) = &mut change.after {
            if let Some(rule) = &mut after.event.recurrence {
                if let Some(segment) = rule.segment.take() {
                    if segment.shift_seconds != 0 {
                        return Err(error(
                            "unsupported",
                            "Shifted Google segments cannot be split exactly.",
                        ));
                    }
                    // Only the domain planner constructs these segments. A bounded
                    // head already has its exact count; a tail starts at its pivot.
                    if segment.before.is_some() && rule.count.is_none() {
                        return Err(error(
                            "unsupported",
                            "A Google split head requires an exact count.",
                        ));
                    }
                }
            }
            compile(&after.event)?;
        }
    }
    Ok(())
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
    #[test]
    fn split_canonicalization_preserves_phase_counts_endings_and_exclusive_boundary() {
        for timed in [false, true] {
            for frequency in [
                RepeatFrequency::Daily,
                RepeatFrequency::Weekly,
                RepeatFrequency::Monthly,
                RepeatFrequency::Yearly,
            ] {
                for ending in [0, 1, 2] {
                    let mut source = event(timed);
                    let r = source.recurrence.as_mut().unwrap();
                    r.frequency = frequency.clone();
                    if frequency != RepeatFrequency::Weekly {
                        r.weekdays.clear();
                    }
                    if ending != 0 {
                        r.count = None;
                    }
                    if ending == 1 {
                        r.until = Some("2039-12-31".into());
                    }
                    let source = EventSeries {
                        event: source,
                        exceptions: vec![],
                    };
                    let preview = aika_domain::recurrence::preview(source.event.clone()).unwrap();
                    for index in [0, 1, 3] {
                        let pivot = &preview[index];
                        let mut wanted = pivot.clone();
                        wanted.occurrence = None;
                        wanted.title = "Tail".into();
                        let request = EventMutation {
                            target: pivot.id.clone(),
                            scope: MutationScope::Following,
                            event: Some(wanted),
                            expected: aika_domain::recurrence::expectation(&source).unwrap(),
                            reset_exceptions: true,
                            exception_policy: Some(ExceptionPolicy::Reset),
                            count_changed: false,
                        };
                        let mut changes = aika_domain::recurrence::plan_mutation(
                            &source,
                            &request,
                            &uuid::Uuid::new_v4().to_string(),
                        )
                        .unwrap();
                        let domain = changes.clone();
                        canonicalize_split(&mut changes).unwrap();
                        for (expected, actual) in domain.iter().zip(&changes) {
                            match (&expected.after, &actual.after) {
                                (Some(a), Some(b)) => {
                                    let times = |s: &EventSeries| {
                                        aika_domain::recurrence::preview(s.event.clone())
                                            .unwrap()
                                            .into_iter()
                                            .map(|e| e.time)
                                            .collect::<Vec<_>>()
                                    };
                                    assert_eq!(times(a), times(b));
                                    assert!(b.event.recurrence.as_ref().unwrap().segment.is_none());
                                    compile(&b.event).unwrap();
                                }
                                (None, None) => {}
                                _ => panic!("split changed head emptiness"),
                            }
                        }
                    }
                }
            }
        }
    }
}
