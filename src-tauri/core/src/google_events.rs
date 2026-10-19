//! Conservative Google conversion. Unproven rules use Google's bounded instance projection.
use crate::{
    contracts::*,
    service::error,
    sync::{IncomingItem, IncomingPage},
};
use chrono::{NaiveDate, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, ServiceError>;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct EventPage {
    #[serde(default)]
    pub items: Vec<Value>,
    pub next_page_token: Option<String>,
    pub next_sync_token: Option<String>,
    #[serde(default = "utc")]
    pub time_zone: String,
}
fn utc() -> String {
    "UTC".into()
}
fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}
fn invalid() -> ServiceError {
    error("google", "Google returned an invalid event response.")
}
fn date(v: &str) -> Result<String> {
    let d = NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| invalid())?;
    if d.to_string() != v {
        return Err(invalid());
    }
    Ok(v.into())
}
fn instant(v: &str) -> Result<String> {
    Ok(aika_domain::iso(
        chrono::DateTime::parse_from_rfc3339(v)
            .map_err(|_| invalid())?
            .with_timezone(&Utc),
    ))
}
fn original(v: &Value) -> Result<String> {
    if let Some(v) = text(v, "date") {
        date(v)
    } else {
        instant(text(v, "dateTime").ok_or_else(invalid)?)
    }
}
fn time(v: &Value, fallback: &str) -> Result<EventTime> {
    let start = v.get("start").ok_or_else(invalid)?;
    let end = v.get("end").ok_or_else(invalid)?;
    if let (Some(s), Some(e)) = (text(start, "date"), text(end, "date")) {
        let s = date(s)?;
        let e = date(e)?;
        if e <= s {
            return Err(invalid());
        }
        Ok(EventTime::AllDay {
            start_date: s,
            end_date: e,
        })
    } else {
        let s = instant(text(start, "dateTime").ok_or_else(invalid)?)?;
        let e = instant(text(end, "dateTime").ok_or_else(invalid)?)?;
        if e <= s {
            return Err(invalid());
        }
        let zone = text(start, "timeZone").unwrap_or(fallback);
        let zone = if zone.parse::<chrono_tz::Tz>().is_ok() {
            zone
        } else {
            "UTC"
        };
        Ok(EventTime::Timed {
            start: s,
            end: e,
            time_zone: zone.into(),
        })
    }
}
fn compatible(v: &Value, event: &CalendarEvent) -> Option<Recurrence> {
    // UTC and all-day patterns have proven duration/clock semantics. Source-zone/DST
    // patterns and richer RFC5545 constructs remain intact in shadow + projections.
    if matches!(&event.time,EventTime::Timed{time_zone,..} if time_zone!="UTC" && time_zone!="Etc/UTC")
    {
        return None;
    }
    let lines = v.get("recurrence")?.as_array()?;
    if lines.len() != 1 {
        return None;
    }
    let line = lines[0].as_str()?.strip_prefix("RRULE:")?;
    let mut fields = BTreeMap::new();
    for field in line.split(';') {
        let (k, v) = field.split_once('=')?;
        if fields.insert(k, v).is_some() {
            return None;
        }
    }
    if fields.keys().any(|k| {
        !matches!(
            *k,
            "FREQ" | "INTERVAL" | "COUNT" | "UNTIL" | "WKST" | "BYDAY"
        )
    }) {
        return None;
    }
    if fields.get("WKST").is_some_and(|v| *v != "MO") {
        return None;
    }
    let frequency = match *fields.get("FREQ")? {
        "DAILY" => RepeatFrequency::Daily,
        "WEEKLY" => RepeatFrequency::Weekly,
        "MONTHLY" => RepeatFrequency::Monthly,
        "YEARLY" => RepeatFrequency::Yearly,
        _ => return None,
    };
    let weekdays = if let Some(days) = fields.get("BYDAY") {
        if frequency != RepeatFrequency::Weekly {
            return None;
        }
        days.split(',')
            .map(|d| match d {
                "MO" => Some(1),
                "TU" => Some(2),
                "WE" => Some(3),
                "TH" => Some(4),
                "FR" => Some(5),
                "SA" => Some(6),
                "SU" => Some(7),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()?
    } else {
        vec![]
    };
    let until = if let Some(u) = fields.get("UNTIL") {
        // Domain UNTIL is an inclusive date, so only date-valued all-day limits match.
        if !matches!(event.time, EventTime::AllDay { .. }) {
            return None;
        }
        Some(NaiveDate::parse_from_str(u, "%Y%m%d").ok()?.to_string())
    } else {
        None
    };
    let rule = Recurrence {
        frequency,
        interval: fields
            .get("INTERVAL")
            .map(|v| v.parse().ok())
            .unwrap_or(Some(1))?,
        count: fields
            .get("COUNT")
            .map(|v| v.parse().ok())
            .transpose_option()?,
        weekdays,
        monthly_on: MonthlyOn::Date,
        until,
        segment: None,
    };
    let e = CalendarEvent {
        recurrence: Some(rule.clone()),
        ..event.clone()
    };
    crate::service::validate_event(e).ok()?;
    Some(rule)
}
// Option<Option<T>> is valid only if a supplied value parses.
trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            Some(Some(v)) => Some(Some(v)),
            Some(None) => None,
            None => Some(None),
        }
    }
}
pub(super) fn normalized(page: EventPage) -> Result<IncomingPage> {
    let items = page
        .items
        .into_iter()
        .map(|v| {
            let remote_id = text(&v, "id")
                .filter(|s| !s.is_empty())
                .ok_or_else(invalid)?
                .to_string();
            let master = text(&v, "recurringEventId").map(str::to_string);
            let original_start = v.get("originalStartTime").map(original).transpose()?;
            if master.is_some() && original_start.is_none() {
                return Err(invalid());
            }
            let mut item = IncomingItem {
                remote_id,
                revision: text(&v, "etag").unwrap_or("").into(),
                shadow: serde_json::to_string(&v)?,
                event: None,
                master,
                original_start,
                unsupported: false,
            };
            if text(&v, "status") != Some("cancelled") {
                let mut event = CalendarEvent {
                    id: String::new(),
                    calendar_id: String::new(),
                    title: text(&v, "summary").unwrap_or("").into(),
                    location: text(&v, "location").unwrap_or("").into(),
                    notes: text(&v, "description").unwrap_or("").into(),
                    time: time(&v, &page.time_zone)?,
                    recurrence: None,
                    occurrence: None,
                };
                if v.get("recurrence")
                    .is_some_and(|v| v.as_array().is_some_and(|a| !a.is_empty()))
                {
                    // Validation needs scoped UUIDs; conversion never exposes them as remote IDs.
                    let probe = CalendarEvent {
                        id: uuid::Uuid::new_v4().to_string(),
                        calendar_id: "local-default".into(),
                        ..event.clone()
                    };
                    event.recurrence = compatible(&v, &probe);
                    item.unsupported = event.recurrence.is_none();
                }
                item.event = Some(event);
            }
            Ok(item)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(IncomingPage {
        items,
        next_page: page.next_page_token,
        cursor: page.next_sync_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn convert(value: Value) -> IncomingItem {
        normalized(EventPage {
            items: vec![value],
            next_page_token: None,
            next_sync_token: Some("cursor".into()),
            time_zone: "UTC".into(),
        })
        .unwrap()
        .items
        .remove(0)
    }
    fn all_day() -> Value {
        serde_json::json!({"id":"fixture","etag":"revision","summary":"Daily","start":{"date":"2026-10-09"},"end":{"date":"2026-10-10"},"providerOnly":{"preserve":true}})
    }
    #[test]
    fn missing_redacted_fields_and_provider_payload_survive_without_local_validation() {
        let mut v = all_day();
        v.as_object_mut().unwrap().remove("summary");
        let i = convert(v.clone());
        assert_eq!(i.event.unwrap().title, "");
        assert_eq!(serde_json::from_str::<Value>(&i.shadow).unwrap(), v);
        let mut v = all_day();
        v["summary"] = Value::String("x".repeat(500));
        assert_eq!(convert(v).event.unwrap().title.len(), 500);
    }
    #[test]
    fn utc_instants_source_zone_and_exclusive_dates_are_preserved() {
        let i = convert(
            serde_json::json!({"id":"timed","start":{"dateTime":"2026-10-25T02:30:00+02:00","timeZone":"Europe/Zurich"},"end":{"dateTime":"2026-10-25T02:30:00+01:00"}}),
        );
        assert_eq!(
            i.event.unwrap().time,
            EventTime::Timed {
                start: "2026-10-25T00:30:00.000Z".into(),
                end: "2026-10-25T01:30:00.000Z".into(),
                time_zone: "Europe/Zurich".into()
            }
        );
        assert_eq!(
            convert(all_day()).event.unwrap().time,
            EventTime::AllDay {
                start_date: "2026-10-09".into(),
                end_date: "2026-10-10".into()
            }
        );
    }
    #[test]
    fn conservative_rules_expand_in_common_domain_and_richer_rules_use_projection() {
        for rule in [
            "FREQ=DAILY;COUNT=3",
            "FREQ=WEEKLY;BYDAY=MO,FR;INTERVAL=2;COUNT=3",
            "FREQ=MONTHLY;COUNT=3",
            "FREQ=YEARLY;COUNT=3",
            "FREQ=DAILY;UNTIL=20261011",
        ] {
            let mut v = all_day();
            v["recurrence"] = serde_json::json!([format!("RRULE:{rule}")]);
            let mut i = convert(v);
            assert!(!i.unsupported, "{rule}");
            let e = i.event.as_mut().unwrap();
            e.id = uuid::Uuid::new_v4().to_string();
            e.calendar_id = "local-default".into();
            let preview = crate::recurrence::preview(e.clone()).unwrap();
            assert_eq!(preview.len(), 3, "{rule}");
        }
        for rule in [
            "FREQ=DAILY;BYHOUR=10",
            "FREQ=WEEKLY;WKST=SU",
            "FREQ=MONTHLY;BYDAY=1FR",
            "FREQ=DAILY;UNTIL=20261011T120000Z",
            "FREQ=DAILY;FREQ=WEEKLY",
        ] {
            let mut v = all_day();
            v["recurrence"] = serde_json::json!([format!("RRULE:{rule}")]);
            assert!(convert(v).unsupported, "{rule}");
        }
        let mut v = all_day();
        v["recurrence"] = serde_json::json!(["RRULE:FREQ=DAILY", "EXDATE;VALUE=DATE:20261010"]);
        assert!(convert(v).unsupported);
    }
    #[test]
    fn dst_series_and_empty_title_masters_are_projected_and_sparse_cancellations_need_no_time() {
        let i = convert(
            serde_json::json!({"id":"series","summary":"DST","start":{"dateTime":"2026-10-24T09:00:00+02:00","timeZone":"Europe/Zurich"},"end":{"dateTime":"2026-10-24T10:00:00+02:00"},"recurrence":["RRULE:FREQ=DAILY"]}),
        );
        assert!(i.unsupported);
        let mut v = all_day();
        v["summary"] = serde_json::json!("");
        v["recurrence"] = serde_json::json!(["RRULE:FREQ=DAILY"]);
        assert!(convert(v).unsupported);
        let i = convert(
            serde_json::json!({"id":"cancel","status":"cancelled","recurringEventId":"series","originalStartTime":{"date":"2026-10-09"}}),
        );
        assert!(i.event.is_none());
        assert_eq!(i.original_start.as_deref(), Some("2026-10-09"));
    }
}
