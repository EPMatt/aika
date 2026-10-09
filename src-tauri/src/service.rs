//! Local application service. All callers serialize access, including undo and preferences.
use crate::contracts::*;
use chrono::{
    DateTime, LocalResult, NaiveDate, NaiveDateTime, Offset, SecondsFormat, TimeZone, Utc,
};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

type Result<T> = std::result::Result<T, ServiceError>;
pub fn error(code: &str, message: &str) -> ServiceError {
    ServiceError {
        code: code.into(),
        message: message.into(),
        offsets: vec![],
    }
}
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
fn date(value: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| error("validation", "Use a valid calendar date."))?;
    if parsed.to_string() != value {
        return Err(error("validation", "Use an ISO calendar date."));
    }
    Ok(parsed)
}
fn instant(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|v| v.with_timezone(&Utc))
        .map_err(|_| error("validation", "Use a valid timestamp with an offset."))
}
fn zone(value: &str) -> Result<Tz> {
    value
        .parse()
        .map_err(|_| error("validation", "Use a valid IANA time zone."))
}
fn iso(value: DateTime<Utc>) -> String {
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

pub struct CalendarService {
    db: Connection,
    history: Vec<(String, Option<CalendarEvent>)>,
}
impl CalendarService {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys=ON;")?;
        let tx = db.transaction()?;
        let version: i32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(error(
                "migration",
                "This database was created by a newer version of aika.",
            ));
        }
        if version == 0 {
            tx.execute_batch("CREATE TABLE calendars(id TEXT PRIMARY KEY, name TEXT NOT NULL);
                INSERT INTO calendars VALUES('local-default','Personal');
                CREATE TABLE events(id TEXT PRIMARY KEY, calendar_id TEXT NOT NULL REFERENCES calendars(id), kind TEXT NOT NULL,
                    start TEXT NOT NULL, end TEXT NOT NULL, payload TEXT NOT NULL);
                CREATE INDEX events_range ON events(kind,start,end);
                CREATE TABLE preferences(id INTEGER PRIMARY KEY CHECK(id=1), payload TEXT NOT NULL);
                PRAGMA user_version=1;")?;
            tx.execute(
                "INSERT INTO preferences VALUES(1,?1)",
                [serde_json::to_string(&Preferences::default())?],
            )?;
        }
        tx.commit()?;
        Ok(Self {
            db,
            history: vec![],
        })
    }
    pub fn preferences(&self) -> Result<Preferences> {
        let json: String =
            self.db
                .query_row("SELECT payload FROM preferences WHERE id=1", [], |r| {
                    r.get(0)
                })?;
        Ok(serde_json::from_str(&json)?)
    }
    pub fn set_preferences(&mut self, value: Preferences) -> Result<()> {
        if !["system", "light", "dark"].contains(&value.appearance.as_str())
            || !["month", "week", "day"].contains(&value.view.as_str())
        {
            return Err(error("validation", "Invalid calendar preferences."));
        }
        self.db.execute(
            "UPDATE preferences SET payload=?1 WHERE id=1",
            [serde_json::to_string(&value)?],
        )?;
        Ok(())
    }
    pub fn snapshot(&self, range: EventRange) -> Result<CalendarSnapshot> {
        let start = instant(&range.start)?;
        let end = instant(&range.end)?;
        if end <= start || date(&range.end_date)? <= date(&range.start_date)? {
            return Err(error("validation", "Invalid event range."));
        }
        let mut statement = self.db.prepare("SELECT payload FROM events WHERE (kind='timed' AND start < ?2 AND end > ?1) OR (kind='allDay' AND start < ?4 AND end > ?3) ORDER BY start,id")?;
        let rows = statement.query_map(
            params![iso(start), iso(end), range.start_date, range.end_date],
            |r| r.get::<_, String>(0),
        )?;
        let events = rows
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(CalendarSnapshot {
            calendar: Calendar {
                id: "local-default".into(),
                provider_id: "local".into(),
                name: "Personal".into(),
                color: "#718675".into(),
                writable: true,
            },
            events,
            can_undo: !self.history.is_empty(),
        })
    }
    fn get(&self, id: &str) -> Result<Option<CalendarEvent>> {
        let json: Option<String> = self
            .db
            .query_row("SELECT payload FROM events WHERE id=?1", [id], |r| r.get(0))
            .optional()?;
        json.map(|s| serde_json::from_str(&s).map_err(Into::into))
            .transpose()
    }
    fn write(db: &Connection, event: &CalendarEvent) -> Result<()> {
        let (kind, start, end) = match &event.time {
            EventTime::Timed { start, end, .. } => ("timed", start, end),
            EventTime::AllDay {
                start_date,
                end_date,
            } => ("allDay", start_date, end_date),
        };
        db.execute("INSERT INTO events VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET kind=excluded.kind,start=excluded.start,end=excluded.end,payload=excluded.payload", params![event.id,event.calendar_id,kind,start,end,serde_json::to_string(event)?])?;
        Ok(())
    }
    pub fn save(&mut self, mut event: CalendarEvent, existing: bool) -> Result<CalendarEvent> {
        if uuid::Uuid::parse_str(&event.id).is_err() || event.calendar_id != "local-default" {
            return Err(error("validation", "Invalid event or calendar identity."));
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
        let previous = self.get(&event.id)?;
        if existing != previous.is_some() {
            return Err(error(
                "conflict",
                "The event no longer matches the editor. Reload the calendar.",
            ));
        }
        let tx = self.db.transaction()?;
        Self::write(&tx, &event)?;
        tx.commit()?;
        self.history.push((event.id.clone(), previous));
        Ok(event)
    }
    pub fn delete(&mut self, id: &str) -> Result<()> {
        let previous = self
            .get(id)?
            .ok_or_else(|| error("notFound", "This event no longer exists."))?;
        self.db.execute("DELETE FROM events WHERE id=?1", [id])?;
        self.history.push((id.into(), Some(previous)));
        Ok(())
    }
    pub fn undo(&mut self) -> Result<()> {
        let Some((id, previous)) = self.history.last() else {
            return Err(error("nothingToUndo", "There is no change to undo."));
        };
        let tx = self.db.transaction()?;
        match previous {
            Some(event) => Self::write(&tx, event)?,
            None => {
                tx.execute("DELETE FROM events WHERE id=?1", [id])?;
            }
        }
        tx.commit()?;
        self.history.pop();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event() -> CalendarEvent {
        CalendarEvent {
            id: uuid::Uuid::new_v4().to_string(),
            calendar_id: "local-default".into(),
            title: "Leap day".into(),
            location: "Home".into(),
            notes: "Notes".into(),
            time: EventTime::Timed {
                start: "2024-02-29T23:00:00Z".into(),
                end: "2024-03-01T02:00:00Z".into(),
                time_zone: "Europe/Zurich".into(),
            },
        }
    }
    fn range() -> EventRange {
        EventRange {
            start: "2024-03-01T00:00:00Z".into(),
            end: "2024-03-02T00:00:00Z".into(),
            start_date: "2024-03-01".into(),
            end_date: "2024-03-02".into(),
        }
    }
    fn service() -> CalendarService {
        CalendarService::open(Path::new(":memory:")).unwrap()
    }
    #[test]
    fn crud_restart_preferences_and_session_undo() {
        let path = std::env::temp_dir().join(format!("aika-{}.sqlite3", uuid::Uuid::new_v4()));
        let e = event();
        let preferences = Preferences {
            appearance: "dark".into(),
            view: "week".into(),
            sidebar: false,
        };
        {
            let mut s = CalendarService::open(&path).unwrap();
            assert!(s.snapshot(range()).unwrap().events.is_empty());
            s.save(e.clone(), false).unwrap();
            s.set_preferences(preferences.clone()).unwrap();
        }
        {
            let mut s = CalendarService::open(&path).unwrap();
            assert_eq!(s.preferences().unwrap(), preferences);
            assert!(!s.snapshot(range()).unwrap().can_undo);
            let stored = s.snapshot(range()).unwrap().events.pop().unwrap();
            assert_eq!(stored.notes, "Notes");
            let mut changed = stored.clone();
            changed.title = "Changed".into();
            s.save(changed, true).unwrap();
            s.delete(&e.id).unwrap();
            assert!(s.snapshot(range()).unwrap().events.is_empty());
            s.undo().unwrap();
            assert_eq!(s.get(&e.id).unwrap().unwrap().title, "Changed");
            s.undo().unwrap();
            assert_eq!(s.get(&e.id).unwrap().unwrap().title, "Leap day");
            assert!(s.undo().is_err());
            s.delete(&e.id).unwrap();
        }
        assert!(CalendarService::open(&path)
            .unwrap()
            .snapshot(range())
            .unwrap()
            .events
            .is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn ranges_exclude_touching_and_include_multiday() {
        let mut s = service();
        let mut e = event();
        s.save(e.clone(), false).unwrap();
        assert_eq!(s.snapshot(range()).unwrap().events.len(), 1);
        if let EventTime::Timed { end, .. } = &mut e.time {
            *end = range().start;
        }
        s.save(e.clone(), true).unwrap();
        assert!(s.snapshot(range()).unwrap().events.is_empty());
        e.time = EventTime::AllDay {
            start_date: "2024-02-28".into(),
            end_date: "2024-03-02".into(),
        };
        s.save(e.clone(), true).unwrap();
        assert_eq!(s.snapshot(range()).unwrap().events.len(), 1);
        e.time = EventTime::AllDay {
            start_date: "2024-02-28".into(),
            end_date: "2024-03-01".into(),
        };
        s.save(e, true).unwrap();
        assert!(s.snapshot(range()).unwrap().events.is_empty());
    }
    #[test]
    fn invalid_mutations_do_not_change_history_or_data() {
        let mut s = service();
        let mut e = event();
        e.title = "  ".into();
        assert!(s.save(e.clone(), false).is_err());
        e.title = "Valid".into();
        e.calendar_id = "other".into();
        assert!(s.save(e.clone(), false).is_err());
        e.calendar_id = "local-default".into();
        e.time = EventTime::AllDay {
            start_date: "2023-02-29".into(),
            end_date: "2023-03-01".into(),
        };
        assert!(s.save(e, false).is_err());
        assert!(s.delete("missing").is_err());
        assert!(!s.snapshot(range()).unwrap().can_undo);
        let e = event();
        s.save(e.clone(), false).unwrap();
        assert!(s.save(e.clone(), false).is_err());
        s.db.execute_batch("PRAGMA query_only=ON").unwrap();
        assert!(s.delete(&e.id).is_err());
        assert!(s.undo().is_err());
        assert!(s.get(&e.id).unwrap().is_some());
        s.db.execute_batch("PRAGMA query_only=OFF").unwrap();
        s.undo().unwrap();
        assert!(s.get(&e.id).unwrap().is_none());
    }
    #[test]
    fn migration_and_open_failure_preserve_database() {
        let path = std::env::temp_dir().join(format!("aika-{}.sqlite3", uuid::Uuid::new_v4()));
        let db = Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES('preserve'); PRAGMA user_version=99;").unwrap();
        assert!(CalendarService::open(&path).is_err());
        assert_eq!(
            db.query_row("SELECT value FROM marker", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "preserve"
        );
        db.execute_batch("PRAGMA user_version=0; CREATE TABLE events(id TEXT);")
            .unwrap();
        assert!(CalendarService::open(&path).is_err());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='calendars'",
                [],
                |r| r.get::<_, i32>(0)
            )
            .unwrap(),
            0
        );
        drop(db);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, b"not a database").unwrap();
        assert!(CalendarService::open(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a database");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn shared_wall_time_fixtures() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/time.json")).unwrap();
        for fixture in fixtures.as_array().unwrap() {
            let result = resolve_local(
                fixture["local"].as_str().unwrap(),
                fixture["zone"].as_str().unwrap(),
                fixture["offset"].as_i64().map(|v| v as i32),
            );
            if let Some(code) = fixture["error"].as_str() {
                let error = result.unwrap_err();
                assert_eq!(error.code, code);
                if let Some(offsets) = fixture["offsets"].as_array() {
                    assert_eq!(
                        error.offsets,
                        offsets
                            .iter()
                            .map(|v| v.as_i64().unwrap() as i32)
                            .collect::<Vec<_>>()
                    );
                }
            } else {
                assert_eq!(result.unwrap(), fixture["utc"].as_str().unwrap());
            }
        }
    }
    #[test]
    fn dst_gap_fold_offsets_and_zone_changes() {
        assert_eq!(
            resolve_local("2026-03-29T02:30", "Europe/Zurich", None)
                .unwrap_err()
                .code,
            "nonexistentTime"
        );
        let err = resolve_local("2026-10-25T02:30", "Europe/Zurich", None).unwrap_err();
        assert_eq!(err.code, "ambiguousTime");
        assert_eq!(err.offsets, vec![120, 60]);
        assert_eq!(
            resolve_local("2026-10-25T02:30", "Europe/Zurich", Some(120)).unwrap(),
            "2026-10-25T00:30:00.000Z"
        );
        assert_eq!(
            resolve_local("2026-10-25T02:30", "Europe/Zurich", Some(60)).unwrap(),
            "2026-10-25T01:30:00.000Z"
        );
        assert!(resolve_local("2026-10-25T02:30", "Europe/Zurich", Some(0)).is_err());
        assert!(resolve_local("2026-01-01T12:00", "Bad/Zone", None).is_err());
        assert_eq!(
            resolve_local("2026-01-01T12:00", "America/New_York", None).unwrap(),
            "2026-01-01T17:00:00.000Z"
        );
        let mut s = service();
        let mut e = event();
        e.time = EventTime::Timed {
            start: "2025-12-31T23:00:00Z".into(),
            end: "2026-01-01T01:00:00Z".into(),
            time_zone: "Europe/Zurich".into(),
        };
        s.save(e.clone(), false).unwrap();
        if let EventTime::Timed { time_zone, .. } = &mut e.time {
            *time_zone = "America/New_York".into();
        }
        let saved = s.save(e, true).unwrap();
        if let EventTime::Timed { start, .. } = saved.time {
            assert_eq!(start, "2025-12-31T23:00:00.000Z");
        }
    }
}
