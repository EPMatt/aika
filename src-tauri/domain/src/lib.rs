pub mod contracts;
pub mod recurrence;
#[cfg(feature = "wasm")]
mod wasm;
use chrono::{
    DateTime, LocalResult, NaiveDate, NaiveDateTime, Offset, SecondsFormat, TimeZone, Utc,
};
use chrono_tz::Tz;
use contracts::*;
pub type Result<T> = std::result::Result<T, ServiceError>;
pub fn error(code: &str, message: &str) -> ServiceError {
    ServiceError {
        code: code.into(),
        message: message.into(),
        offsets: vec![],
    }
}
pub(crate) fn date(value: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| error("validation", "Use a valid calendar date."))?;
    if parsed.to_string() != value {
        return Err(error("validation", "Use an ISO calendar date."));
    }
    Ok(parsed)
}
pub fn instant(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|_| error("validation", "Use a valid timestamp with an offset."))
}
pub(crate) fn zone(value: &str) -> Result<Tz> {
    value
        .parse()
        .map_err(|_| error("validation", "Use a valid IANA time zone."))
}
pub fn iso(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// Resolve wall-clock input before conversion: never silently shift a DST gap.
pub fn resolve_local(local: &str, time_zone: &str, offset: Option<i32>) -> Result<String> {
    let local = NaiveDateTime::parse_from_str(local, "%Y-%m-%dT%H:%M")
        .map_err(|_| error("validation", "Use a valid local date and time."))?;
    let candidates =
        match zone(time_zone)?.from_local_datetime(&local) {
            LocalResult::None => return Err(error(
                "nonexistentTime",
                "This time does not exist because the clocks move forward. Choose another time.",
            )),
            LocalResult::Single(value) => vec![value],
            LocalResult::Ambiguous(a, b) => vec![a, b],
        };
    let offsets: Vec<i32> = candidates
        .iter()
        .map(|v| v.offset().fix().local_minus_utc() / 60)
        .collect();
    if candidates.len() > 1 && offset.is_none() {
        return Err(ServiceError {
            code: "ambiguousTime".into(),
            message: "This time occurs twice. Choose its UTC offset.".into(),
            offsets,
        });
    }
    let chosen = candidates
        .into_iter()
        .find(|v| offset.is_none_or(|o| v.offset().fix().local_minus_utc() / 60 == o))
        .ok_or_else(|| {
            error(
                "validation",
                "The selected offset does not match this time.",
            )
        })?;
    Ok(iso(chosen.with_timezone(&Utc)))
}

/// Shared normalized validation used before provider dispatch.
pub fn validate_event(mut event: CalendarEvent) -> Result<CalendarEvent> {
    if uuid::Uuid::parse_str(&event.id).is_err() {
        return Err(error("validation", "Invalid event identity."));
    }
    event.title = event.title.trim().into();
    if event.title.is_empty() || event.title.chars().count() > 300 {
        return Err(error("validation", "Enter a title of 1 to 300 characters."));
    }
    match &mut event.time {
        EventTime::Timed {
            start,
            end,
            time_zone,
        } => {
            zone(time_zone)?;
            let from = instant(start)?;
            let until = instant(end)?;
            if until <= from {
                return Err(error("validation", "The event must end after it starts."));
            }
            *start = iso(from);
            *end = iso(until);
        }
        EventTime::AllDay {
            start_date,
            end_date,
        } => {
            if date(end_date)? <= date(start_date)? {
                return Err(error(
                    "validation",
                    "The exclusive end date must follow the start date.",
                ));
            }
        }
    }
    crate::recurrence::validate_recurrence(&event)?;
    if let Some(rule) = &mut event.recurrence {
        rule.weekdays.sort_unstable();
    }
    Ok(event)
}
pub fn validate_range(range: &EventRange) -> Result<()> {
    let start = instant(&range.start)?;
    let end = instant(&range.end)?;
    let dates = date(&range.end_date)?
        .signed_duration_since(date(&range.start_date)?)
        .num_days();
    if end <= start
        || dates <= 0
        || dates > 366
        || end.signed_duration_since(start).num_hours() > 24 * 367
    {
        return Err(error(
            "validation",
            "Choose an event range of at most 366 days.",
        ));
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
impl From<rusqlite::Error> for ServiceError {
    fn from(_: rusqlite::Error) -> Self {
        error(
            "storage",
            "Could not access the calendar database. Your data has not been reset.",
        )
    }
}
impl From<serde_json::Error> for ServiceError {
    fn from(_: serde_json::Error) -> Self {
        error("storage", "Could not read stored calendar data.")
    }
}
