use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, TS)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub provider: String,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Calendar {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub color: String,
    pub writable: bool,
}

#[derive(Clone, Serialize, Deserialize, TS)]
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

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    pub calendar_id: String,
    pub title: String,
    pub time: EventTime,
    pub location: String,
    pub notes: String,
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
