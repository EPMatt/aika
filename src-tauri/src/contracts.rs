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
