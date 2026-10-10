use cal_desktop_lib::contracts::{
    AppInfo, Calendar, CalendarCollectionSnapshot, CalendarDetails, CalendarEvent,
    CalendarSnapshot, EventRange, EventTime, Preferences, ServiceError,
};
use cal_desktop_lib::contracts::{
    EventException, EventMutation, EventSeries, MonthlyOn, MutationScope, Occurrence, Recurrence,
    RepeatFrequency, RepeatSegment, SeriesChange,
};
use ts_rs::{Config, TS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/generated");
    let config = Config::default().with_out_dir(directory);
    Recurrence::export_all(&config)?;
    RepeatFrequency::export_all(&config)?;
    MonthlyOn::export_all(&config)?;
    RepeatSegment::export_all(&config)?;
    Occurrence::export_all(&config)?;
    EventException::export_all(&config)?;
    EventSeries::export_all(&config)?;
    EventMutation::export_all(&config)?;
    MutationScope::export_all(&config)?;
    SeriesChange::export_all(&config)?;
    AppInfo::export_all(&config)?;
    Calendar::export_all(&config)?;
    CalendarDetails::export_all(&config)?;
    CalendarCollectionSnapshot::export_all(&config)?;
    EventTime::export_all(&config)?;
    CalendarEvent::export_all(&config)?;
    Preferences::export_all(&config)?;
    ServiceError::export_all(&config)?;
    CalendarSnapshot::export_all(&config)?;
    EventRange::export_all(&config)?;
    Ok(())
}
