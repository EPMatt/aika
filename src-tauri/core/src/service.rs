//! Local application service. All callers serialize access, including undo and preferences.
use crate::contracts::*;
use chrono::{
    DateTime, LocalResult, NaiveDate, NaiveDateTime, Offset, SecondsFormat, TimeZone, Utc,
};
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
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

struct HistoryEntry {
    id: String,
    before: Option<CalendarEvent>,
    after: Option<CalendarEvent>,
}

/// Calendar metadata and settings read together, without querying events.
pub struct CalendarManagementSnapshot {
    pub calendars: Vec<Calendar>,
    pub default_calendar_id: Option<String>,
    pub hidden_calendar_ids: Vec<String>,
}

pub struct LocalProvider {
    db: Connection,
    history: Vec<HistoryEntry>,
}
impl LocalProvider {
    pub fn open(path: &Path) -> Result<Self> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch("PRAGMA foreign_keys=ON;")?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 2 {
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
        if version < 2 {
            tx.execute_batch("ALTER TABLE calendars ADD COLUMN color TEXT NOT NULL DEFAULT '#718675';
                ALTER TABLE calendars ADD COLUMN description TEXT NOT NULL DEFAULT '';
                ALTER TABLE calendars ADD COLUMN visible INTEGER NOT NULL DEFAULT 1 CHECK(visible IN (0,1));
                CREATE TABLE calendar_settings(id INTEGER PRIMARY KEY CHECK(id=1), default_calendar_id TEXT REFERENCES calendars(id) ON DELETE SET NULL);
                INSERT INTO calendar_settings VALUES(1,(SELECT id FROM calendars ORDER BY rowid LIMIT 1));
                CREATE INDEX events_calendar_range ON events(calendar_id,kind,start,end);
                PRAGMA user_version=2;")?;
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
    fn read_calendars(db: &Connection) -> Result<Vec<Calendar>> {
        let mut statement =
            db.prepare("SELECT id,name,color,description FROM calendars ORDER BY rowid")?;
        let rows = statement.query_map([], |row| {
            Ok(Calendar {
                id: row.get(0)?,
                provider_id: "local".into(),
                name: row.get(1)?,
                color: row.get(2)?,
                description: row.get(3)?,
                writable: true,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        Self::read_calendars(&self.db)
    }
    pub fn calendar_management_snapshot(&self) -> Result<CalendarManagementSnapshot> {
        let tx = self.db.unchecked_transaction()?;
        let calendars = Self::read_calendars(&tx)?;
        let default_calendar_id = tx.query_row(
            "SELECT default_calendar_id FROM calendar_settings WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let hidden_calendar_ids = {
            let mut statement =
                tx.prepare("SELECT id FROM calendars WHERE visible=0 ORDER BY rowid")?;
            let rows = statement.query_map([], |r| r.get(0))?;
            rows.collect::<std::result::Result<Vec<String>, _>>()?
        };
        tx.commit()?;
        Ok(CalendarManagementSnapshot {
            calendars,
            default_calendar_id,
            hidden_calendar_ids,
        })
    }
    fn require_calendar(db: &Connection, id: &str) -> Result<()> {
        if !db.query_row(
            "SELECT EXISTS(SELECT 1 FROM calendars WHERE id=?1)",
            [id],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(error("notFound", "This calendar no longer exists."));
        }
        Ok(())
    }
    pub fn validate_details(mut details: CalendarDetails) -> Result<CalendarDetails> {
        details.name = details.name.trim().into();
        if details.name.is_empty()
            || details.name.chars().count() > 100
            || details.description.chars().count() > 2000
        {
            return Err(error("validation", "Use a calendar name of 1 to 100 characters and a description of up to 2000 characters."));
        }
        if ![
            "#718675", "#5B7FA3", "#8A6FA8", "#B56F86", "#B38B43", "#4F918A", "#B77B50", "#768390",
        ]
        .contains(&details.color.as_str())
        {
            return Err(error(
                "validation",
                "Choose a calendar color from the palette.",
            ));
        }
        Ok(details)
    }
    pub fn create_calendar(&mut self, details: CalendarDetails) -> Result<Calendar> {
        let details = Self::validate_details(details)?;
        let id = uuid::Uuid::new_v4().to_string();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO calendars(id,name,color,description) VALUES(?1,?2,?3,?4)",
            params![id, details.name, details.color, details.description],
        )?;
        tx.execute(
            "UPDATE calendar_settings SET default_calendar_id=?1 WHERE default_calendar_id IS NULL",
            [&id],
        )?;
        tx.commit()?;
        Ok(Calendar {
            id,
            provider_id: "local".into(),
            name: details.name,
            color: details.color,
            description: details.description,
            writable: true,
        })
    }
    pub fn update_calendar(
        &mut self,
        id: &str,
        details: CalendarDetails,
        expected: Calendar,
    ) -> Result<()> {
        let details = Self::validate_details(details)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = Self::read_calendars(&tx)?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| error("notFound", "This calendar no longer exists."))?;
        if current != expected {
            return Err(error(
                "conflict",
                "The calendar changed. Reopen its settings before saving.",
            ));
        }
        tx.execute(
            "UPDATE calendars SET name=?2,color=?3,description=?4 WHERE id=?1",
            params![id, details.name, details.color, details.description],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn calendar_event_count(&self, id: &str) -> Result<usize> {
        Self::require_calendar(&self.db, id)?;
        Ok(self.db.query_row(
            "SELECT count(*) FROM events WHERE calendar_id=?1",
            [id],
            |r| r.get::<_, i64>(0),
        )? as usize)
    }
    pub fn delete_calendar(&mut self, id: &str) -> Result<()> {
        self.delete_calendar_guarded(id, true)
    }
    pub fn delete_calendar_guarded(&mut self, id: &str, force: bool) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::require_calendar(&tx, id)?;
        let count: i64 = tx.query_row(
            "SELECT count(*) FROM events WHERE calendar_id=?1",
            [id],
            |r| r.get(0),
        )?;
        if count > 0 && !force {
            return Err(error("validation", &format!("This calendar contains {count} event(s). Supply --force to delete the calendar and its events.")));
        }
        tx.execute("DELETE FROM events WHERE calendar_id=?1", [id])?;
        tx.execute("DELETE FROM calendars WHERE id=?1", [id])?;
        tx.execute("UPDATE calendar_settings SET default_calendar_id=(SELECT id FROM calendars ORDER BY rowid LIMIT 1) WHERE default_calendar_id IS NULL", [])?;
        let calendars = Self::read_calendars(&tx)?;
        tx.commit()?;
        self.retain_calendar_history(&calendars);
        Ok(())
    }
    pub fn set_default_calendar(&mut self, id: &str) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        Self::require_calendar(&tx, id)?;
        tx.execute("UPDATE calendar_settings SET default_calendar_id=?1", [id])?;
        tx.commit()?;
        Ok(())
    }
    pub fn set_calendar_visibility(&mut self, id: &str, visible: bool) -> Result<()> {
        if self.db.execute(
            "UPDATE calendars SET visible=?2 WHERE id=?1",
            params![id, visible],
        )? == 0
        {
            return Err(error("notFound", "This calendar no longer exists."));
        }
        Ok(())
    }
    pub fn default_calendar_id(&self) -> Result<Option<String>> {
        Ok(self.db.query_row(
            "SELECT default_calendar_id FROM calendar_settings WHERE id=1",
            [],
            |r| r.get(0),
        )?)
    }
    fn read_events(
        db: &Connection,
        range: &EventRange,
        calendar: Option<&str>,
    ) -> Result<Vec<CalendarEvent>> {
        validate_range(range)?;
        let mut statement = db.prepare("SELECT payload FROM events WHERE (?5 IS NULL OR calendar_id=?5) AND ((kind='timed' AND start < ?2 AND end > ?1) OR (kind='allDay' AND start < ?4 AND end > ?3)) ORDER BY start,id")?;
        let rows = statement.query_map(
            params![
                iso(instant(&range.start)?),
                iso(instant(&range.end)?),
                range.start_date,
                range.end_date,
                calendar
            ],
            |r| r.get::<_, String>(0),
        )?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    fn prune_history(&mut self) -> Result<()> {
        let calendars = self.calendars()?;
        self.retain_calendar_history(&calendars);
        Ok(())
    }
    fn retain_calendar_history(&mut self, calendars: &[Calendar]) {
        let ids: Vec<_> = calendars.iter().map(|c| &c.id).collect();
        let removed: Vec<_> = self
            .history
            .iter()
            .filter(|entry| {
                entry
                    .before
                    .iter()
                    .chain(entry.after.iter())
                    .any(|e| !ids.contains(&&e.calendar_id))
            })
            .map(|entry| entry.id.clone())
            .collect();
        self.history.retain(|entry| !removed.contains(&entry.id));
    }
    pub fn collection_snapshot(&mut self, range: EventRange) -> Result<CalendarCollectionSnapshot> {
        let tx = self.db.transaction()?;
        let calendars = Self::read_calendars(&tx)?;
        let events = Self::read_events(&tx, &range, None)?;
        let default_calendar_id = tx.query_row(
            "SELECT default_calendar_id FROM calendar_settings WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let mut statement =
            tx.prepare("SELECT id FROM calendars WHERE visible=0 ORDER BY rowid")?;
        let hidden_calendar_ids = statement
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        drop(statement);
        tx.commit()?;
        self.retain_calendar_history(&calendars);
        Ok(CalendarCollectionSnapshot {
            calendars,
            events,
            default_calendar_id,
            hidden_calendar_ids,
            can_undo: !self.history.is_empty(),
        })
    }
    pub fn snapshot_calendar(&self, calendar: &str, range: EventRange) -> Result<CalendarSnapshot> {
        let tx = self.db.unchecked_transaction()?;
        let metadata = Self::read_calendars(&tx)?
            .into_iter()
            .find(|c| c.id == calendar)
            .ok_or_else(|| error("notFound", "Unknown calendar."))?;
        let events = Self::read_events(&tx, &range, Some(calendar))?;
        tx.commit()?;
        Ok(CalendarSnapshot {
            calendar: metadata,
            events,
            can_undo: !self.history.is_empty(),
        })
    }
    pub fn snapshot(&self, range: EventRange) -> Result<CalendarSnapshot> {
        let id = self
            .default_calendar_id()?
            .ok_or_else(|| error("notFound", "Create a calendar first."))?;
        self.snapshot_calendar(&id, range)
    }
    pub fn get(&self, id: &str) -> Result<Option<CalendarEvent>> {
        Self::read(&self.db, id)
    }
    fn read(db: &Connection, id: &str) -> Result<Option<CalendarEvent>> {
        let json: Option<String> = db
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
        let changed = db.execute("INSERT INTO events VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET calendar_id=excluded.calendar_id,kind=excluded.kind,start=excluded.start,end=excluded.end,payload=excluded.payload", params![event.id,event.calendar_id,kind,start,end,serde_json::to_string(event)?])?;
        if changed != 1 {
            return Err(error("conflict", "Event belongs to another calendar."));
        }
        Ok(())
    }
    pub fn save(&mut self, event: CalendarEvent, existing: bool) -> Result<CalendarEvent> {
        let expected = if existing { self.get(&event.id)? } else { None };
        self.save_checked(event, existing, expected)
    }
    pub fn save_checked(
        &mut self,
        event: CalendarEvent,
        existing: bool,
        expected: Option<CalendarEvent>,
    ) -> Result<CalendarEvent> {
        let event = validate_event(event)?;
        if existing != expected.is_some() || expected.as_ref().is_some_and(|e| e.id != event.id) {
            return Err(error(
                "conflict",
                "An update requires the original event. Reload the calendar.",
            ));
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = Self::read(&tx, &event.id)?;
        if previous != expected {
            return Err(error(
                "conflict",
                "The event changed. Reload the calendar before saving.",
            ));
        }
        Self::require_calendar(&tx, &event.calendar_id)?;
        if let Some(original) = &expected {
            Self::require_calendar(&tx, &original.calendar_id)?;
        }
        Self::write(&tx, &event)?;
        if previous
            .as_ref()
            .is_none_or(|e| e.calendar_id != event.calendar_id)
        {
            tx.execute(
                "UPDATE calendars SET visible=1 WHERE id=?1",
                [&event.calendar_id],
            )?;
        }
        tx.commit()?;
        self.history.push(HistoryEntry {
            id: event.id.clone(),
            before: previous,
            after: Some(event.clone()),
        });
        Ok(event)
    }
    pub fn delete(&mut self, id: &str) -> Result<()> {
        let expected = self
            .get(id)?
            .ok_or_else(|| error("notFound", "This event no longer exists."))?;
        self.delete_checked(id, &expected)
    }
    pub fn delete_checked(&mut self, id: &str, expected: &CalendarEvent) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous = Self::read(&tx, id)?
            .ok_or_else(|| error("notFound", "This event no longer exists."))?;
        if &previous != expected {
            return Err(error(
                "conflict",
                "The event changed. Reload the calendar before deleting.",
            ));
        }
        tx.execute("DELETE FROM events WHERE id=?1", [id])?;
        tx.commit()?;
        self.history.push(HistoryEntry {
            id: id.into(),
            before: Some(previous),
            after: None,
        });
        Ok(())
    }
    pub fn undo(&mut self) -> Result<()> {
        self.prune_history()?;
        let Some(entry) = self.history.last() else {
            return Err(error("nothingToUndo", "There is no change to undo."));
        };
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if Self::read(&tx, &entry.id)? != entry.after {
            return Err(error(
                "conflict",
                "The event changed outside this action. Undo cannot overwrite it.",
            ));
        }
        match &entry.before {
            Some(event) => Self::write(&tx, event)?,
            None => {
                tx.execute("DELETE FROM events WHERE id=?1", [&entry.id])?;
            }
        }
        tx.commit()?;
        self.history.pop();
        Ok(())
    }
}

/// Production facade keeps the renderer contract stable while dispatching providers.
pub struct CalendarService {
    providers: crate::providers::ProviderRegistry,
}
impl CalendarService {
    pub fn open(path: &Path) -> Result<Self> {
        let mut providers = crate::providers::ProviderRegistry::default();
        providers.register(Box::new(LocalProvider::open(path)?))?;
        Ok(Self { providers })
    }
    pub fn preferences(&self) -> Result<Preferences> {
        self.providers.preferences()
    }
    pub fn collection_snapshot(&mut self, range: EventRange) -> Result<CalendarCollectionSnapshot> {
        self.providers.collection_snapshot(range)
    }
    pub fn default_calendar_id(&self) -> Result<Option<String>> {
        self.providers.default_calendar_id()
    }
    pub fn calendar_management_snapshot(&self) -> Result<CalendarManagementSnapshot> {
        self.providers.calendar_management_snapshot()
    }
    pub fn delete_calendar_guarded(&mut self, id: &str, force: bool) -> Result<()> {
        self.providers.delete_calendar_guarded(id, force)
    }
    pub fn create_calendar(&mut self, details: CalendarDetails) -> Result<Calendar> {
        self.providers.create_calendar(details)
    }
    pub fn update_calendar(
        &mut self,
        id: &str,
        details: CalendarDetails,
        expected: Calendar,
    ) -> Result<()> {
        self.providers.update_calendar(id, details, expected)
    }
    pub fn delete_calendar(&mut self, id: &str) -> Result<()> {
        self.providers.delete_calendar(id)
    }
    pub fn calendar_event_count(&self, id: &str) -> Result<usize> {
        self.providers.calendar_event_count(id)
    }
    pub fn set_default_calendar(&mut self, id: &str) -> Result<()> {
        self.providers.set_default_calendar(id)
    }
    pub fn set_calendar_visibility(&mut self, id: &str, visible: bool) -> Result<()> {
        self.providers.set_calendar_visibility(id, visible)
    }

    pub fn set_preferences(&mut self, value: Preferences) -> Result<()> {
        self.providers.set_preferences(value)
    }
    pub fn snapshot(&self, range: EventRange) -> Result<CalendarSnapshot> {
        let id = self
            .providers
            .default_calendar_id()?
            .ok_or_else(|| error("notFound", "Create a calendar first."))?;
        self.providers.snapshot(&id, range)
    }
    pub fn save(&mut self, event: CalendarEvent, existing: bool) -> Result<CalendarEvent> {
        self.providers.save(event, existing)
    }
    pub fn delete(&mut self, id: &str) -> Result<()> {
        let expected = self.get(id)?;
        self.delete_checked(id, &expected)
    }
    pub fn get(&self, id: &str) -> Result<CalendarEvent> {
        for calendar in self.providers.live_calendars()? {
            match self.providers.get(&calendar.id, id) {
                Ok(event) => return Ok(event),
                Err(e) if e.code == "notFound" => {}
                Err(e) => return Err(e),
            }
        }
        Err(error("notFound", "This event no longer exists."))
    }
    pub fn save_checked(
        &mut self,
        event: CalendarEvent,
        existing: bool,
        expected: Option<CalendarEvent>,
    ) -> Result<CalendarEvent> {
        self.providers.save_checked(event, existing, expected)
    }
    pub fn delete_checked(&mut self, id: &str, expected: &CalendarEvent) -> Result<()> {
        self.providers
            .delete_checked(&expected.calendar_id, id, expected)
    }
    pub fn undo(&mut self) -> Result<()> {
        self.providers.undo_local()
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
    fn service() -> LocalProvider {
        LocalProvider::open(Path::new(":memory:")).unwrap()
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
            let mut s = LocalProvider::open(&path).unwrap();
            assert!(s.snapshot(range()).unwrap().events.is_empty());
            s.save(e.clone(), false).unwrap();
            s.set_preferences(preferences.clone()).unwrap();
        }
        {
            let mut s = LocalProvider::open(&path).unwrap();
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
        assert!(LocalProvider::open(&path)
            .unwrap()
            .snapshot(range())
            .unwrap()
            .events
            .is_empty());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn concurrent_changes_reject_stale_writes_and_undo() {
        let path =
            std::env::temp_dir().join(format!("aika-concurrent-{}.sqlite3", uuid::Uuid::new_v4()));
        let mut desktop = CalendarService::open(&path).unwrap();
        let mut cli = CalendarService::open(&path).unwrap();
        let original = desktop.save_checked(event(), false, None).unwrap();
        let mut changed = original.clone();
        changed.title = "CLI changed".into();
        let changed = cli
            .save_checked(changed, true, Some(original.clone()))
            .unwrap();
        let mut stale = original.clone();
        stale.notes = "Desktop draft".into();
        assert_eq!(
            desktop
                .save_checked(stale, true, Some(original.clone()))
                .unwrap_err()
                .code,
            "conflict"
        );
        assert_eq!(
            desktop
                .delete_checked(&original.id, &original)
                .unwrap_err()
                .code,
            "conflict"
        );
        assert_eq!(desktop.undo().unwrap_err().code, "conflict");
        assert_eq!(desktop.get(&original.id).unwrap(), changed);
        // An unrelated external change does not block safe undo of another event.
        let other = desktop.save_checked(event(), false, None).unwrap();
        desktop.undo().unwrap();
        assert_eq!(cli.get(&other.id).unwrap_err().code, "notFound");
        desktop.delete_checked(&changed.id, &changed).unwrap();
        cli.save_checked(changed.clone(), false, None).unwrap();
        assert_eq!(desktop.undo().unwrap_err().code, "conflict");
        assert_eq!(cli.get(&changed.id).unwrap(), changed);
        drop(desktop);
        drop(cli);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn transaction_contention_preserves_event_and_history() {
        let path = std::env::temp_dir().join(format!("aika-lock-{}.sqlite3", uuid::Uuid::new_v4()));
        let mut local = LocalProvider::open(&path).unwrap();
        let original = local.save(event(), false).unwrap();
        local
            .db
            .busy_timeout(std::time::Duration::from_millis(10))
            .unwrap();
        let other = Connection::open(&path).unwrap();
        other.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert_eq!(
            local
                .delete_checked(&original.id, &original)
                .unwrap_err()
                .code,
            "storage"
        );
        other.execute_batch("ROLLBACK").unwrap();
        assert_eq!(local.get(&original.id).unwrap(), Some(original));
        local.undo().unwrap();
        assert!(local.snapshot(range()).unwrap().events.is_empty());
        drop(local);
        drop(other);
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
        assert!(LocalProvider::open(&path).is_err());
        assert_eq!(
            db.query_row("SELECT value FROM marker", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "preserve"
        );
        db.execute_batch("PRAGMA user_version=0; CREATE TABLE events(id TEXT);")
            .unwrap();
        assert!(LocalProvider::open(&path).is_err());
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
        assert!(LocalProvider::open(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a database");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn database_calendar_identity_isolation() {
        let mut s = service();
        let mut foreign = event();
        foreign.calendar_id = "foreign".into();
        s.db.execute(
            "INSERT INTO calendars(id,name) VALUES('foreign','Other')",
            [],
        )
        .unwrap();
        LocalProvider::write(&s.db, &foreign).unwrap();
        assert!(s.snapshot(range()).unwrap().events.is_empty());
        let mut wrong = foreign.clone();
        wrong.calendar_id = "local-default".into();
        assert!(s.delete_checked(&foreign.id, &wrong).is_err());
        let mut impostor = foreign.clone();
        impostor.calendar_id = "local-default".into();
        assert!(s.save(impostor, false).is_err());
        assert!(!s.snapshot(range()).unwrap().can_undo);
        let stored: String =
            s.db.query_row(
                "SELECT payload FROM events WHERE id=?1",
                [&foreign.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<CalendarEvent>(&stored)
                .unwrap()
                .calendar_id,
            "foreign"
        );
    }
    fn details(name: &str) -> CalendarDetails {
        CalendarDetails {
            name: name.into(),
            description: "Calendar notes".into(),
            color: "#5B7FA3".into(),
        }
    }
    #[test]
    fn multiple_calendars_moves_visibility_and_undo() {
        let mut s = service();
        let work = s.create_calendar(details("Work")).unwrap();
        assert_eq!(
            s.default_calendar_id().unwrap().as_deref(),
            Some("local-default")
        );
        let original = s.save(event(), false).unwrap();
        s.set_calendar_visibility(&work.id, false).unwrap();
        let mut moved = original.clone();
        moved.calendar_id = work.id.clone();
        let moved = s.save_checked(moved, true, Some(original.clone())).unwrap();
        let snapshot = s.collection_snapshot(range()).unwrap();
        assert_eq!(snapshot.calendars.len(), 2);
        assert!(snapshot.hidden_calendar_ids.is_empty());
        assert_eq!(snapshot.events, vec![moved.clone()]);
        assert!(s
            .snapshot_calendar("local-default", range())
            .unwrap()
            .events
            .is_empty());
        assert_eq!(s.calendar_event_count(&work.id).unwrap(), 1);
        s.undo().unwrap();
        assert_eq!(s.get(&original.id).unwrap(), Some(original.clone()));
        // A move must compare the original complete event, not just its ID.
        let mut stale = moved;
        stale.title = "Stale move".into();
        assert_eq!(
            s.save_checked(
                stale,
                true,
                Some({
                    let mut wrong = original.clone();
                    wrong.title = "Stale".into();
                    wrong
                })
            )
            .unwrap_err()
            .code,
            "conflict"
        );
        // Hiding the default does not prevent creation; saving reveals it.
        s.set_calendar_visibility("local-default", false).unwrap();
        s.save(event(), false).unwrap();
        assert!(s
            .collection_snapshot(range())
            .unwrap()
            .hidden_calendar_ids
            .is_empty());
    }
    #[test]
    fn deletion_default_fallback_and_empty_state_survive_restart() {
        let path =
            std::env::temp_dir().join(format!("aika-calendars-{}.sqlite3", uuid::Uuid::new_v4()));
        {
            let mut s = LocalProvider::open(&path).unwrap();
            let work = s.create_calendar(details("Work")).unwrap();
            let other = s.create_calendar(details("Other")).unwrap();
            s.set_default_calendar(&work.id).unwrap();
            s.set_calendar_visibility(&other.id, false).unwrap();
            let original = s.save(event(), false).unwrap();
            let mut moved = original.clone();
            moved.calendar_id = work.id.clone();
            s.save_checked(moved, true, Some(original)).unwrap();
            s.delete_calendar(&work.id).unwrap();
            let snapshot = s.collection_snapshot(range()).unwrap();
            assert!(snapshot.events.is_empty());
            assert!(!snapshot.can_undo);
            assert_eq!(
                snapshot.default_calendar_id.as_deref(),
                Some("local-default")
            );
            assert_eq!(snapshot.hidden_calendar_ids, vec![other.id.clone()]);
            s.delete_calendar("local-default").unwrap();
            assert_eq!(s.default_calendar_id().unwrap(), Some(other.id.clone()));
            s.delete_calendar(&other.id).unwrap();
        }
        {
            let mut s = LocalProvider::open(&path).unwrap();
            let snapshot = s.collection_snapshot(range()).unwrap();
            assert!(snapshot.calendars.is_empty());
            assert!(snapshot.events.is_empty());
            assert_eq!(snapshot.default_calendar_id, None);
            assert!(s.save(event(), false).is_err());
            let created = s.create_calendar(details("New first calendar")).unwrap();
            assert_eq!(s.default_calendar_id().unwrap(), Some(created.id));
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn guarded_calendar_deletion_checks_events_added_by_another_connection() {
        let path =
            std::env::temp_dir().join(format!("aika-guarded-{}.sqlite3", uuid::Uuid::new_v4()));
        let mut cli = CalendarService::open(&path).unwrap();
        let mut desktop = CalendarService::open(&path).unwrap();
        assert_eq!(cli.calendar_event_count("local-default").unwrap(), 0);
        let original = desktop.save_checked(event(), false, None).unwrap();
        assert_eq!(
            cli.delete_calendar_guarded("local-default", false)
                .unwrap_err()
                .code,
            "validation"
        );
        assert_eq!(desktop.get(&original.id).unwrap(), original);
        assert_eq!(
            cli.default_calendar_id().unwrap().as_deref(),
            Some("local-default")
        );
        cli.delete_calendar_guarded("local-default", true).unwrap();
        assert_eq!(desktop.get(&original.id).unwrap_err().code, "notFound");
        assert!(cli
            .calendar_management_snapshot()
            .unwrap()
            .calendars
            .is_empty());
        assert_eq!(
            cli.delete_calendar_guarded("local-default", false)
                .unwrap_err()
                .code,
            "notFound"
        );
        drop(cli);
        drop(desktop);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn metadata_visibility_default_and_external_discovery() {
        let path =
            std::env::temp_dir().join(format!("aika-discovery-{}.sqlite3", uuid::Uuid::new_v4()));
        let mut desktop = CalendarService::open(&path).unwrap();
        let mut external = CalendarService::open(&path).unwrap();
        let work = external.create_calendar(details("Work")).unwrap();
        external
            .update_calendar(&work.id, details("Office"), work.clone())
            .unwrap();
        assert_eq!(
            desktop
                .update_calendar(&work.id, details("Stale"), work.clone())
                .unwrap_err()
                .code,
            "conflict"
        );
        external.set_default_calendar(&work.id).unwrap();
        let mut e = event();
        e.calendar_id = work.id.clone();
        let original = desktop.save_checked(e, false, None).unwrap();
        external.set_calendar_visibility(&work.id, false).unwrap();
        let snapshot = desktop.collection_snapshot(range()).unwrap();
        assert_eq!(snapshot.calendars[1].name, "Office");
        assert_eq!(snapshot.calendars[1].description, "Calendar notes");
        assert_eq!(snapshot.hidden_calendar_ids, vec![work.id.clone()]);
        assert_eq!(snapshot.default_calendar_id, Some(work.id.clone()));
        assert_eq!(desktop.get(&original.id).unwrap(), original.clone());
        external.delete_calendar(&work.id).unwrap();
        assert!(desktop
            .save_checked(original.clone(), true, Some(original))
            .is_err());
        assert!(!desktop.collection_snapshot(range()).unwrap().can_undo);
        drop(desktop);
        drop(external);
        let mut reopened = CalendarService::open(&path).unwrap();
        assert_eq!(
            reopened
                .collection_snapshot(range())
                .unwrap()
                .calendars
                .len(),
            1
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn v1_migration_preserves_events_and_preferences() {
        let path = std::env::temp_dir().join(format!("aika-v1-{}.sqlite3", uuid::Uuid::new_v4()));
        let original = validate_event(event()).unwrap();
        let preferences = Preferences {
            appearance: "dark".into(),
            view: "week".into(),
            sidebar: false,
        };
        {
            let db = Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE calendars(id TEXT PRIMARY KEY,name TEXT NOT NULL);
                INSERT INTO calendars VALUES('local-default','Personal');
                CREATE TABLE events(id TEXT PRIMARY KEY,calendar_id TEXT NOT NULL REFERENCES calendars(id),kind TEXT NOT NULL,start TEXT NOT NULL,end TEXT NOT NULL,payload TEXT NOT NULL);
                CREATE INDEX events_range ON events(kind,start,end);
                CREATE TABLE preferences(id INTEGER PRIMARY KEY CHECK(id=1),payload TEXT NOT NULL);
                PRAGMA user_version=1;").unwrap();
            db.execute(
                "INSERT INTO preferences VALUES(1,?1)",
                [serde_json::to_string(&preferences).unwrap()],
            )
            .unwrap();
            LocalProvider::write(&db, &original).unwrap();
        }
        {
            let mut s = LocalProvider::open(&path).unwrap();
            let snapshot = s.collection_snapshot(range()).unwrap();
            assert_eq!(snapshot.events, vec![original]);
            assert_eq!(snapshot.calendars[0].id, "local-default");
            assert_eq!(snapshot.calendars[0].color, "#718675");
            assert_eq!(snapshot.calendars[0].description, "");
            assert_eq!(
                snapshot.default_calendar_id.as_deref(),
                Some("local-default")
            );
            assert!(snapshot.hidden_calendar_ids.is_empty());
            assert_eq!(s.preferences().unwrap(), preferences);
            assert_eq!(
                s.db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                    .unwrap(),
                2
            );
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn failed_calendar_operations_preserve_state() {
        let mut s = service();
        let mut invalid = details(" ");
        assert!(s.create_calendar(invalid.clone()).is_err());
        invalid.name = "Name".into();
        invalid.color = "#ffffff".into();
        assert!(s.create_calendar(invalid).is_err());
        let original = s.save(event(), false).unwrap();
        s.db.execute_batch("PRAGMA query_only=ON").unwrap();
        assert!(s.delete_calendar("local-default").is_err());
        assert!(s.set_default_calendar("local-default").is_err());
        assert!(s.set_calendar_visibility("local-default", false).is_err());
        assert_eq!(s.get(&original.id).unwrap(), Some(original));
        assert_eq!(s.calendars().unwrap().len(), 1);
        s.db.execute_batch("PRAGMA query_only=OFF").unwrap();
        s.undo().unwrap();
        assert!(s.collection_snapshot(range()).unwrap().events.is_empty());
    }

    #[test]
    fn shared_wall_time_fixtures() {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/time.json")).unwrap();
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
