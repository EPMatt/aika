//! Common recurrence schedules and pure, atomic mutation plans.
use crate::{contracts::*, date, error, instant, iso, validate_event, zone, Result};
use chrono::{Datelike, Duration, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};

const MAX_STEPS: usize = 100_000;
const MAX_RESULTS: usize = 10_000;

pub fn wall(time: &EventTime) -> Result<NaiveDateTime> {
    match time {
        EventTime::AllDay { start_date, .. } => Ok(date(start_date)?.and_hms_opt(0, 0, 0).unwrap()),
        EventTime::Timed {
            start, time_zone, ..
        } => Ok(instant(start)?
            .with_timezone(&zone(time_zone)?)
            .naive_local()),
    }
}
fn boundary(time: &EventTime) -> &str {
    match time {
        EventTime::Timed { start, .. } => start,
        EventTime::AllDay { start_date, .. } => start_date,
    }
}
fn valid_key(value: &str, time: &EventTime) -> Result<()> {
    match time {
        EventTime::AllDay { .. } => {
            date(value)?;
        }
        EventTime::Timed { .. } => {
            if iso(instant(value)?) != value {
                return Err(error(
                    "validation",
                    "Use the exact occurrence selector from listing.",
                ));
            }
        }
    }
    Ok(())
}
pub fn validate_recurrence(event: &CalendarEvent) -> Result<()> {
    let Some(rule) = &event.recurrence else {
        return Ok(());
    };
    if rule.interval == 0
        || rule.interval > 10_000
        || rule.count.is_some_and(|v| v == 0 || v > 100_000)
    {
        return Err(error(
            "validation",
            "Repeat interval must be 1–10000 and count 1–100000.",
        ));
    }
    if rule.until.is_some() && rule.count.is_some() {
        return Err(error(
            "validation",
            "Choose either an ending date or occurrence count.",
        ));
    }
    let anchor = rule.segment.as_ref().map_or(&event.time, |s| &s.anchor);
    let start = wall(anchor)?;
    if let Some(until) = &rule.until {
        if date(until)? < start.date() {
            return Err(error(
                "validation",
                "Repeat ending must not precede its start.",
            ));
        }
    }
    if rule.weekdays.iter().any(|d| !(1..=7).contains(d))
        || rule
            .weekdays
            .iter()
            .enumerate()
            .any(|(i, d)| rule.weekdays[..i].contains(d))
    {
        return Err(error(
            "validation",
            "Choose distinct weekdays from Monday (1) through Sunday (7).",
        ));
    }
    if rule.frequency != RepeatFrequency::Weekly && !rule.weekdays.is_empty() {
        return Err(error(
            "validation",
            "Weekday selection is available for weekly repeats.",
        ));
    }
    if rule.frequency == RepeatFrequency::Weekly
        && !rule.weekdays.is_empty()
        && !rule
            .weekdays
            .contains(&start.weekday().number_from_monday())
    {
        return Err(error(
            "validation",
            "The starting weekday must be included in the weekly pattern.",
        ));
    }
    if let Some(s) = &rule.segment {
        validate_event(CalendarEvent {
            time: s.anchor.clone(),
            recurrence: None,
            occurrence: None,
            ..event.clone()
        })?;
        if let Some(v) = &s.from {
            valid_key(v, anchor)?;
        }
        if let Some(v) = &s.before {
            valid_key(v, anchor)?;
        }
        if s.from
            .as_ref()
            .zip(s.before.as_ref())
            .is_some_and(|(a, b)| a >= b)
        {
            return Err(error(
                "validation",
                "Repeat segment must have a positive range.",
            ));
        }
        if s.shift_seconds.unsigned_abs() > 366 * 86400 * 100 {
            return Err(error("validation", "Repeat time shift exceeds 100 years."));
        }
    }
    Ok(())
}

pub fn expectation(series: &EventSeries) -> Result<String> {
    let mut value = series.clone();
    value
        .exceptions
        .sort_by(|a, b| a.original_start.cmp(&b.original_start));
    serde_json::to_string(&value).map_err(Into::into)
}

fn at_wall(template: &EventTime, start: NaiveDateTime) -> Result<Option<EventTime>> {
    match template {
        EventTime::AllDay {
            start_date,
            end_date,
        } => {
            let length = date(end_date)?.signed_duration_since(date(start_date)?);
            let end = start
                .date()
                .checked_add_signed(length)
                .ok_or_else(|| error("validation", "Date exceeds supported range."))?;
            Ok(Some(EventTime::AllDay {
                start_date: start.date().to_string(),
                end_date: end.to_string(),
            }))
        }
        EventTime::Timed {
            start: old_start,
            end: old_end,
            time_zone,
        } => {
            let tz = zone(time_zone)?;
            let resolved = match tz.from_local_datetime(&start) {
                LocalResult::None => return Ok(None),
                LocalResult::Single(v) => v,
                LocalResult::Ambiguous(a, b) => a.min(b),
            }
            .with_timezone(&Utc);
            let duration = instant(old_end)?.signed_duration_since(instant(old_start)?);
            let end = resolved
                .checked_add_signed(duration)
                .ok_or_else(|| error("validation", "Time exceeds supported range."))?;
            Ok(Some(EventTime::Timed {
                start: iso(resolved),
                end: iso(end),
                time_zone: time_zone.clone(),
            }))
        }
    }
}
fn next_date(anchor: NaiveDate, rule: &Recurrence, step: usize) -> Option<NaiveDate> {
    let interval = i64::from(rule.interval);
    match rule.frequency {
        RepeatFrequency::Daily => anchor.checked_add_signed(Duration::days(step as i64 * interval)),
        RepeatFrequency::Weekly => anchor.checked_add_signed(Duration::days(step as i64)),
        RepeatFrequency::Monthly => {
            let month =
                i64::from(anchor.year()) * 12 + i64::from(anchor.month0()) + step as i64 * interval;
            let year = i32::try_from(month.div_euclid(12)).ok()?;
            let month = (month.rem_euclid(12) + 1) as u32;
            match rule.monthly_on {
                MonthlyOn::Date => NaiveDate::from_ymd_opt(year, month, anchor.day()),
                MonthlyOn::Weekday => {
                    let first = NaiveDate::from_ymd_opt(year, month, 1)?;
                    let ordinal = (anchor.day() - 1) / 7;
                    let offset = (anchor.weekday().num_days_from_monday() + 7
                        - first.weekday().num_days_from_monday())
                        % 7;
                    NaiveDate::from_ymd_opt(year, month, 1 + offset + ordinal * 7)
                }
            }
        }
        RepeatFrequency::Yearly => NaiveDate::from_ymd_opt(
            i32::try_from(i64::from(anchor.year()) + step as i64 * interval).ok()?,
            anchor.month(),
            anchor.day(),
        ),
    }
}
fn weekly_match(anchor: NaiveDate, candidate: NaiveDate, rule: &Recurrence) -> bool {
    let monday = anchor - Duration::days(i64::from(anchor.weekday().num_days_from_monday()));
    let week = candidate.signed_duration_since(monday).num_days() / 7;
    week % i64::from(rule.interval) == 0
        && if rule.weekdays.is_empty() {
            candidate.weekday() == anchor.weekday()
        } else {
            rule.weekdays
                .contains(&candidate.weekday().number_from_monday())
        }
}
fn overlaps(event: &CalendarEvent, range: &EventRange) -> Result<bool> {
    Ok(match &event.time {
        EventTime::Timed { start, end, .. } => {
            instant(start)? < instant(&range.end)? && instant(end)? > instant(&range.start)?
        }
        EventTime::AllDay {
            start_date,
            end_date,
        } => start_date < &range.end_date && end_date > &range.start_date,
    })
}

/// Counts valid starts in the active segment before applying individual exceptions.
fn scheduled(
    series: &EventSeries,
    stop: NaiveDate,
    mut visit: impl FnMut(String, CalendarEvent) -> Result<bool>,
) -> Result<()> {
    validate_event(series.event.clone())?;
    let Some(rule) = &series.event.recurrence else {
        visit(
            boundary(&series.event.time).to_string(),
            series.event.clone(),
        )?;
        return Ok(());
    };
    let source = rule
        .segment
        .as_ref()
        .map_or(&series.event.time, |s| &s.anchor);
    let anchor = wall(source)?;
    let shift = rule.segment.as_ref().map_or(0, |s| s.shift_seconds);
    let mut count = 0;
    for step in 0..MAX_STEPS {
        // A missing monthly/yearly date is skipped, not normalized or counted.
        let Some(candidate) = next_date(anchor.date(), rule, step) else {
            continue;
        };
        if candidate > stop {
            return Ok(());
        }
        if rule.frequency == RepeatFrequency::Weekly
            && !weekly_match(anchor.date(), candidate, rule)
        {
            continue;
        }
        let local = candidate.and_time(anchor.time());
        let original = if step == 0 {
            Some(source.clone())
        } else {
            at_wall(source, local)?
        };
        let Some(original) = original else {
            continue;
        };
        let key = boundary(&original).to_string();
        if let Some(segment) = &rule.segment {
            if segment.before.as_ref().is_some_and(|before| &key >= before) {
                return Ok(());
            }
            if segment.from.as_ref().is_some_and(|from| &key < from) {
                continue;
            }
        }
        let shifted = local
            .checked_add_signed(Duration::seconds(shift))
            .ok_or_else(|| error("validation", "Time exceeds supported range."))?;
        let first_active = rule
            .segment
            .as_ref()
            .and_then(|s| s.from.as_ref())
            .is_some_and(|from| from == &key);
        let time = if first_active && wall(&series.event.time)? == shifted {
            Some(series.event.time.clone())
        } else if step == 0 && shift == 0 && series.event.time == *source {
            Some(source.clone())
        } else {
            at_wall(&series.event.time, shifted)?
        };
        let Some(time) = time else {
            continue;
        };
        if rule
            .until
            .as_ref()
            .is_some_and(|until| shifted.date().to_string() > *until)
        {
            return Ok(());
        }
        count += 1;
        if rule.count.is_some_and(|limit| count > limit) {
            return Ok(());
        }
        let event = CalendarEvent {
            time,
            occurrence: None,
            ..series.event.clone()
        };
        if !visit(key, event)? {
            return Ok(());
        }
        if rule.count == Some(count) {
            return Ok(());
        }
    }
    Err(error(
        "recurrenceLimit",
        "Repeat expansion exceeds 100000 steps. Choose a closer range or shorter series.",
    ))
}
fn displayed(
    series: &EventSeries,
    key: String,
    mut event: CalendarEvent,
    expected: &str,
) -> CalendarEvent {
    event.id = format!("{}@{}", series.event.id, key);
    event.recurrence = series.event.recurrence.clone();
    event.occurrence = Some(Occurrence {
        series_id: series.event.id.clone(),
        original_start: key,
        expected: expected.into(),
    });
    event
}

pub fn expand(series: &EventSeries, range: &EventRange) -> Result<Vec<CalendarEvent>> {
    crate::validate_range(range)?;
    if series.event.recurrence.is_none() {
        return Ok(if overlaps(&series.event, range)? {
            vec![series.event.clone()]
        } else {
            vec![]
        });
    }
    let expected = expectation(series)?;
    // Source and display zones can differ, and a split can shift the schedule.
    let shift = series
        .event
        .recurrence
        .as_ref()
        .and_then(|r| r.segment.as_ref())
        .map_or(0, |s| s.shift_seconds);
    let stop = date(&range.end_date)?
        .checked_add_signed(Duration::days(2) - Duration::seconds(shift))
        .ok_or_else(|| error("validation", "Date exceeds supported range."))?;
    let mut result = vec![];
    scheduled(series, stop, |key, event| {
        if series.exceptions.iter().any(|e| e.original_start == key) {
            return Ok(true);
        }
        if overlaps(&event, range)? {
            result.push(displayed(series, key, event, &expected));
        }
        if result.len() > MAX_RESULTS {
            return Err(error(
                "recurrenceLimit",
                "Repeat range exceeds 10000 occurrences.",
            ));
        }
        Ok(true)
    })?;
    for exception in &series.exceptions {
        if let Some(event) = &exception.event {
            if overlaps(event, range)? {
                result.push(displayed(
                    series,
                    exception.original_start.clone(),
                    event.clone(),
                    &expected,
                ));
            }
        }
    }
    if result.len() > MAX_RESULTS {
        return Err(error(
            "recurrenceLimit",
            "Repeat range exceeds 10000 occurrences.",
        ));
    }
    Ok(result)
}

pub fn get_occurrence(series: &EventSeries, key: &str) -> Result<CalendarEvent> {
    if series.event.recurrence.is_none() {
        return Err(error(
            "validation",
            "An occurrence selector requires a recurring event.",
        ));
    }
    let anchor = series
        .event
        .recurrence
        .as_ref()
        .and_then(|r| r.segment.as_ref())
        .map_or(&series.event.time, |s| &s.anchor);
    valid_key(key, anchor)?;
    let mut result = None;
    let stop = match anchor {
        EventTime::AllDay { .. } => date(key)?,
        EventTime::Timed { .. } => instant(key)?.date_naive() + Duration::days(2),
    };
    scheduled(series, stop, |candidate, event| {
        if candidate == key {
            result = Some(event);
            return Ok(false);
        }
        Ok(true)
    })?;
    let result = result.ok_or_else(|| error("notFound", "This occurrence no longer exists."))?;
    let expected = expectation(series)?;
    if let Some(exception) = series.exceptions.iter().find(|e| e.original_start == key) {
        return exception
            .event
            .clone()
            .map(|event| displayed(series, key.into(), event, &expected))
            .ok_or_else(|| error("notFound", "This occurrence was deleted."));
    }
    Ok(displayed(series, key.into(), result, &expected))
}

pub fn preview(event: CalendarEvent) -> Result<Vec<CalendarEvent>> {
    let event = validate_event(event)?;
    let start = wall(&event.time)?.date();
    let stop = start
        .checked_add_signed(Duration::days(3660))
        .ok_or_else(|| error("validation", "Date exceeds supported range."))?;
    let series = EventSeries {
        event,
        exceptions: vec![],
    };
    let expected = expectation(&series)?;
    let mut results = vec![];
    scheduled(&series, stop, |key, event| {
        results.push(if series.event.recurrence.is_some() {
            displayed(&series, key, event, &expected)
        } else {
            event
        });
        Ok(results.len() < 10)
    })?;
    Ok(results)
}

fn same_pattern(a: &Recurrence, b: &Recurrence) -> bool {
    a.frequency == b.frequency
        && a.interval == b.interval
        && a.weekdays == b.weekdays
        && a.monthly_on == b.monthly_on
        && a.until == b.until
        && a.count == b.count
}
fn adjust_weekdays(event: &mut CalendarEvent, original: &CalendarEvent) -> Result<()> {
    let delta = wall(&event.time)?
        .date()
        .signed_duration_since(wall(&original.time)?.date())
        .num_days();
    if let Some(rule) = &mut event.recurrence {
        if rule.frequency == RepeatFrequency::Weekly
            && original.recurrence.as_ref().is_some_and(|old| {
                old.frequency == RepeatFrequency::Weekly && old.weekdays == rule.weekdays
            })
        {
            for day in &mut rule.weekdays {
                *day = (i64::from(*day) - 1 + delta).rem_euclid(7) as u32 + 1;
            }
            rule.weekdays.sort_unstable();
        }
    }
    Ok(())
}
fn clean(mut event: CalendarEvent, id: &str) -> CalendarEvent {
    event.id = id.into();
    event.occurrence = None;
    event
}
fn shift_master(
    mut master: CalendarEvent,
    selected: &CalendarEvent,
    changed: &CalendarEvent,
) -> Result<CalendarEvent> {
    let delta = wall(&changed.time)?.signed_duration_since(wall(&selected.time)?);
    let local = wall(&master.time)?
        .checked_add_signed(delta)
        .ok_or_else(|| error("validation", "Date exceeds supported range."))?;
    master.time = at_wall(&changed.time, local)?.ok_or_else(|| {
        error(
            "nonexistentTime",
            "The changed series start does not exist in its time zone.",
        )
    })?;
    Ok(master)
}

pub fn plan_mutation(
    series: &EventSeries,
    request: &EventMutation,
    new_id: &str,
) -> Result<Vec<SeriesChange>> {
    if expectation(series)? != request.expected {
        return Err(error(
            "conflict",
            "The event changed. Reload it before saving.",
        ));
    }
    let mut request = request.clone();
    if let Some(rule) = request.event.as_mut().and_then(|e| e.recurrence.as_mut()) {
        rule.weekdays.sort_unstable();
    }
    let (id, key) = request
        .target
        .split_once('@')
        .map_or((request.target.as_str(), None), |(id, key)| (id, Some(key)));
    if id != series.event.id {
        return Err(error(
            "conflict",
            "Event identity does not match its series.",
        ));
    }
    let selected = key
        .map(|k| {
            let mut probe = series.clone();
            probe
                .exceptions
                .retain(|x| x.original_start != k || x.event.is_some());
            get_occurrence(&probe, k)
        })
        .transpose()?;
    if request.scope != MutationScope::Series
        && (key.is_none() || series.event.recurrence.is_none())
    {
        return Err(error(
            "validation",
            "This scope requires a recurring occurrence selector.",
        ));
    }
    let mut after = series.clone();
    if request.scope == MutationScope::Occurrence {
        if request
            .event
            .as_ref()
            .is_some_and(|e| e.recurrence != series.event.recurrence)
        {
            return Err(error(
                "validation",
                "Changing a repeat pattern requires series or following scope.",
            ));
        }
        let key = key.unwrap();
        let changed = request
            .event
            .clone()
            .map(|event| {
                let mut e = clean(event, &series.event.id);
                e.recurrence = None;
                validate_event(e)
            })
            .transpose()?;
        after.exceptions.retain(|e| e.original_start != key);
        after.exceptions.push(EventException {
            original_start: key.into(),
            event: changed,
        });
        after
            .exceptions
            .sort_by(|a, b| a.original_start.cmp(&b.original_start));
        return Ok(vec![SeriesChange {
            id: id.into(),
            before: Some(series.clone()),
            after: Some(after),
        }]);
    }
    let affected = series.exceptions.iter().any(|e| {
        request.scope == MutationScope::Series
            || key.is_some_and(|k| e.original_start.as_str() >= k)
    });
    if request.event.is_some()
        && affected
        && !request.reset_exceptions
        && request.exception_policy != Some(ExceptionPolicy::Preserve)
    {
        return Err(error(
            "exceptionsResetRequired",
            "This edit resets individual changes. Confirm reset of affected exceptions.",
        ));
    }
    if request.scope == MutationScope::Series {
        let changed = request
            .event
            .clone()
            .map(|e| clean(e, id))
            .map(|mut e| -> Result<CalendarEvent> {
                if let Some(selected) = &selected {
                    e.time = series.event.time.clone();
                    e = shift_master(e, selected, request.event.as_ref().unwrap())?;
                }
                adjust_weekdays(&mut e, &series.event)?;
                if let Some(rule) = &mut e.recurrence {
                    if e.time != series.event.time
                        || series
                            .event
                            .recurrence
                            .as_ref()
                            .is_some_and(|old| !same_pattern(rule, old))
                    {
                        rule.segment = None;
                    }
                }
                validate_event(e)
            })
            .transpose()?;
        return Ok(vec![SeriesChange {
            id: id.into(),
            before: Some(series.clone()),
            after: changed.map(|event| EventSeries {
                event,
                exceptions: if request.exception_policy == Some(ExceptionPolicy::Preserve) {
                    series.exceptions.clone()
                } else {
                    vec![]
                },
            }),
        }]);
    }
    let key = key.unwrap();
    let source = series.event.recurrence.as_ref().unwrap();
    let segment = source.segment.clone().unwrap_or(RepeatSegment {
        anchor: series.event.time.clone(),
        from: None,
        before: None,
        shift_seconds: 0,
    });
    let mut preceding = 0;
    let pivot_date = wall(
        &get_occurrence(
            &EventSeries {
                event: series.event.clone(),
                exceptions: vec![],
            },
            key,
        )?
        .time,
    )?
    .date();
    let source_stop = pivot_date
        .checked_sub_signed(Duration::seconds(segment.shift_seconds))
        .and_then(|d| d.checked_add_signed(Duration::days(2)))
        .ok_or_else(|| error("validation", "Date exceeds supported range."))?;
    scheduled(series, source_stop, |candidate, _| {
        if candidate == key {
            return Ok(false);
        }
        preceding += 1;
        Ok(true)
    })?;
    let mut head = segment.clone();
    head.before = Some(key.into());
    // Splitting at the first active occurrence removes the empty head.
    let first = preview(series.event.clone())?
        .first()
        .and_then(|e| e.occurrence.as_ref())
        .map(|o| o.original_start.clone());
    let before_head = if first.as_deref() == Some(key) {
        None
    } else {
        let rule = after.event.recurrence.as_mut().unwrap();
        rule.segment = Some(head);
        rule.count = Some(preceding);
        rule.until = None;
        after.exceptions.retain(|e| e.original_start.as_str() < key);
        Some(after)
    };
    let mut changes = vec![SeriesChange {
        id: id.into(),
        before: Some(series.clone()),
        after: before_head,
    }];
    if let Some(event) = request.event.clone() {
        let mut event = clean(event, new_id);
        let base = get_occurrence(
            &EventSeries {
                event: series.event.clone(),
                exceptions: vec![],
            },
            key,
        )?;
        adjust_weekdays(&mut event, &base)?;
        let unchanged_time = event.time == base.time;
        if let Some(rule) = &mut event.recurrence {
            let unchanged_pattern = same_pattern(rule, source);
            if !request.count_changed && rule.count == source.count {
                rule.count = source.count.map(|count| count - preceding);
            }
            if unchanged_pattern && unchanged_time {
                rule.segment = Some(RepeatSegment {
                    from: Some(key.into()),
                    ..segment
                });
            } else {
                rule.segment = None;
            }
        }
        let event = validate_event(event)?;
        changes.push(SeriesChange {
            id: new_id.into(),
            before: None,
            after: Some(EventSeries {
                event,
                exceptions: vec![],
            }),
        });
    }
    Ok(changes)
}

#[cfg(test)]
mod tests;
