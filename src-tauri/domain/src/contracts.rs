use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, TS)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub provider: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub color: String,
    pub description: String,
    pub writable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EventTime {
    #[serde(rename_all = "camelCase")]
    Timed {
        start: String,
        end: String,
        time_zone: String,
    },
    #[serde(rename_all = "camelCase")]
    AllDay {
        start_date: String,
        end_date: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub time: EventTime,
    pub location: String,
    pub notes: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub recurrence: Option<Recurrence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub occurrence: Option<Occurrence>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub appearance: String,
    pub view: String,
    pub sidebar: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            appearance: "system".into(),
            view: "month".into(),
            sidebar: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ServiceError {
    pub code: String,
    pub message: String,
    pub offsets: Vec<i32>,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSnapshot {
    pub calendar: Calendar,
    pub events: Vec<CalendarEvent>,
    pub can_undo: bool,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EventRange {
    pub start: String,
    pub end: String,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDetails {
    pub name: String,
    pub description: String,
    pub color: String,
}

#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarCollectionSnapshot {
    pub calendars: Vec<Calendar>,
    pub events: Vec<CalendarEvent>,
    pub default_calendar_id: Option<String>,
    pub hidden_calendar_ids: Vec<String>,
    pub can_undo: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RepeatFrequency {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MonthlyOn {
    Date,
    Weekday,
}

/// Segment metadata retains the original schedule when splitting a series.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RepeatSegment {
    pub anchor: EventTime,
    pub from: Option<String>,
    pub before: Option<String>,
    pub shift_seconds: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Recurrence {
    pub frequency: RepeatFrequency,
    pub interval: u32,
    pub weekdays: Vec<u32>,
    pub monthly_on: MonthlyOn,
    pub until: Option<String>,
    pub count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub segment: Option<RepeatSegment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub series_id: String,
    pub original_start: String,
    pub expected: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EventException {
    pub original_start: String,
    pub event: Option<CalendarEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
pub struct EventSeries {
    pub event: CalendarEvent,
    pub exceptions: Vec<EventException>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum MutationScope {
    Occurrence,
    Following,
    Series,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ExceptionPolicy {
    Reset,
    Preserve,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EventMutation {
    pub target: String,
    pub scope: MutationScope,
    pub event: Option<CalendarEvent>,
    pub expected: String,
    pub reset_exceptions: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub exception_policy: Option<ExceptionPolicy>,
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub count_changed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SeriesChange {
    pub id: String,
    pub before: Option<EventSeries>,
    pub after: Option<EventSeries>,
}
