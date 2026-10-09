use cal_desktop_lib::contracts::{
    AppInfo, Calendar, CalendarEvent, CalendarSnapshot, EventRange, EventTime, Preferences,
    ServiceError,
};
use ts_rs::{Config, TS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/generated");
    let config = Config::default().with_out_dir(directory);
    AppInfo::export_all(&config)?;
    Calendar::export_all(&config)?;
    EventTime::export_all(&config)?;
    CalendarEvent::export_all(&config)?;
    Preferences::export_all(&config)?;
    ServiceError::export_all(&config)?;
    CalendarSnapshot::export_all(&config)?;
    EventRange::export_all(&config)?;
    Ok(())
}
