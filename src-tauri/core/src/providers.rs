//! Backend-only protocol adapters. The registry owns one shared repository.
use crate::contracts::*;
use crate::repository::{CalendarManagementSnapshot, Repository};
use crate::service::{error, validate_event, validate_range};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, ServiceError>;

pub trait Provider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn capabilities(&self) -> Capabilities;
    fn event_capabilities(&self, _item: &crate::sync::IncomingItem) -> Capabilities {
        Capabilities::read_only()
    }
    fn resolve_outgoing_instance(
        &self,
        _subject: &str,
        _calendar: &str,
        _series: &str,
        _original: &str,
    ) -> Result<crate::sync::IncomingItem> {
        Err(error(
            "unsupported",
            "Provider instance lookup unavailable.",
        ))
    }
    fn outgoing(
        &self,
        _subject: &str,
        _operation: &crate::outgoing::OutgoingOperation,
    ) -> Result<crate::outgoing::OutgoingResult> {
        Err(error(
            "unsupported",
            "Outgoing synchronization unavailable.",
        ))
    }
    fn incremental_sync(&self) -> bool {
        true
    }
    fn start_connection(&self) -> Result<ConnectionSession> {
        Err(error(
            "unsupported",
            "This provider does not connect accounts.",
        ))
    }
    fn connection(&self, _id: &str) -> Result<(ConnectionSession, Option<ConnectedIdentity>)> {
        Err(error("unsupported", "Connection unavailable."))
    }
    fn cancel_connection(&self, _id: &str) -> Result<()> {
        Err(error("unsupported", "Connection unavailable."))
    }
    fn discover(&self, _subject: &str) -> Result<Vec<DiscoveredCalendar>> {
        Err(error("unsupported", "Discovery unavailable."))
    }
    fn discover_for_sync(
        &self,
        subject: &str,
        checkpoint: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<DiscoveredCalendar>> {
        checkpoint()?;
        self.discover(subject)
    }
    fn incoming_page(
        &self,
        _subject: &str,
        _calendar: &str,
        _cursor: Option<&str>,
        _page: Option<&str>,
        _projection: Option<&EventRange>,
    ) -> Result<crate::sync::IncomingPage> {
        Err(error(
            "unsupported",
            "Incoming synchronization unavailable.",
        ))
    }
    /// Caller holds the lifecycle identity lock throughout deletion and SQLite cleanup.
    fn remove_credentials(&self, subject: &str) -> Result<()> {
        self.disconnect(subject)
    }
    fn connection_epoch(&self, _session: &str) -> Result<Option<i64>> {
        Ok(None)
    }
    fn credential_epoch(&self) -> Result<Option<i64>> {
        Ok(None)
    }
    fn disconnect_checked(&self, subject: &str, _epoch: Option<i64>) -> Result<()> {
        self.disconnect(subject)
    }
    fn disconnect(&self, _subject: &str) -> Result<()> {
        Err(error("unsupported", "Disconnection unavailable."))
    }
}
/// Backend-only identity; never includes credentials or authorization URLs.
#[derive(Clone)]
pub struct ConnectedIdentity {
    pub subject: String,
    pub display_name: String,
}
pub struct LocalProvider;
impl Provider for LocalProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "local".into(),
            name: "On this device".into(),
            connection_available: true,
            unavailable_reason: None,
            capabilities: self.capabilities(),
        }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::local()
    }
}
pub struct ProviderRegistry {
    providers: BTreeMap<String, std::sync::Arc<dyn Provider>>,
    pub(crate) repository: Repository,
}
impl ProviderRegistry {
    pub fn new(repository: Repository) -> Self {
        Self {
            providers: BTreeMap::new(),
            repository,
        }
    }
    pub fn register(&mut self, provider: Box<dyn Provider>) -> Result<()> {
        let descriptor = provider.descriptor();
        if descriptor.id.is_empty() || self.providers.contains_key(&descriptor.id) {
            return Err(error(
                "providerIdentity",
                "Duplicate or invalid provider identity.",
            ));
        }
        self.providers.insert(descriptor.id, provider.into());
        Ok(())
    }
    pub fn adapter(&self, id: &str) -> Result<std::sync::Arc<dyn Provider>> {
        self.providers
            .get(id)
            .cloned()
            .ok_or_else(|| error("unsupported", "Provider unavailable."))
    }
    pub fn descriptors(&self) -> Vec<ProviderDescriptor> {
        self.providers.values().map(|p| p.descriptor()).collect()
    }
    pub fn accounts(&self) -> Result<Vec<AccountSummary>> {
        self.repository.accounts()
    }
    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        self.repository.calendars()
    }
    pub fn live_calendars(&self) -> Result<Vec<Calendar>> {
        self.calendars()
    }
    pub fn capabilities(
        &self,
        calendar: &str,
        event: Option<&CalendarEvent>,
    ) -> Result<Capabilities> {
        let metadata = self
            .calendars()?
            .into_iter()
            .find(|c| c.id == calendar)
            .ok_or_else(|| error("notFound", "Unknown calendar."))?;
        let adapter = self.providers.get(&metadata.provider_id).ok_or_else(|| {
            error(
                "unsupported",
                "Provider unavailable; cached data remains readable.",
            )
        })?;
        let mut caps = adapter
            .capabilities()
            .intersect(&self.repository.calendar_capabilities(calendar)?);
        if let Some(event) = event {
            if event.calendar_id != calendar {
                return Err(error(
                    "providerIdentity",
                    "Event belongs to another calendar.",
                ));
            }
            caps = caps.intersect(&self.repository.event_capabilities(&event.id)?);
        }
        Ok(caps)
    }
    fn require(
        &self,
        calendar: &str,
        event: Option<&CalendarEvent>,
        operation: &str,
    ) -> Result<()> {
        let caps = self.capabilities(calendar, if operation == "create" { None } else { event })?;
        let allowed = match operation {
            "create" => caps.create,
            "edit" => caps.edit,
            "delete" => caps.delete,
            "recurrence" => caps.recurrence,
            "move" => caps.move_event,
            "customize" => caps.customize_metadata,
            _ => false,
        };
        if !allowed {
            return Err(error(
                "readOnly",
                "This operation is unavailable for this calendar or event.",
            ));
        }
        Ok(())
    }
    pub fn collection_snapshot(&mut self, range: EventRange) -> Result<CalendarCollectionSnapshot> {
        let mut snapshot = self.repository.collection_snapshot(range.clone())?;
        snapshot.accounts = self.accounts()?;
        snapshot.sync_status = self.repository.sync_status(&range)?;
        snapshot.calendar_capabilities = snapshot
            .calendars
            .iter()
            .map(|c| {
                let caps = self
                    .capabilities(&c.id, None)
                    .unwrap_or_else(|_| Capabilities::read_only());
                (c.id.clone(), caps)
            })
            .collect();
        for event in &snapshot.events {
            let caps = self
                .capabilities(&event.calendar_id, Some(event))
                .unwrap_or_else(|_| Capabilities::read_only());
            let actions = EventActions::for_event(
                &caps,
                event.recurrence.is_some() || event.occurrence.is_some(),
                self.repository
                    .calendar_account(&event.calendar_id)?
                    .is_some(),
            );
            snapshot
                .event_actions
                .insert(event.id.clone(), actions.clone());
            if let Some((master, _)) = event.id.split_once('@') {
                snapshot.event_actions.insert(master.into(), actions);
            }
            snapshot
                .event_capabilities
                .insert(event.id.clone(), caps.clone());
            if let Some((master, _)) = event.id.split_once('@') {
                snapshot
                    .event_capabilities
                    .entry(master.into())
                    .and_modify(|existing| *existing = existing.intersect(&caps))
                    .or_insert(caps);
            }
        }
        Ok(snapshot)
    }
    pub fn default_calendar_id(&self) -> Result<Option<String>> {
        self.repository.default_calendar_id()
    }
    pub fn calendar_management_snapshot(&self) -> Result<CalendarManagementSnapshot> {
        self.repository.calendar_management_snapshot()
    }
    pub fn create_calendar(&mut self, details: CalendarDetails) -> Result<Calendar> {
        self.repository.create_calendar(details)
    }
    pub fn update_calendar(
        &mut self,
        id: &str,
        details: CalendarDetails,
        expected: Calendar,
    ) -> Result<()> {
        self.require(id, None, "customize")?;
        self.repository.update_calendar(id, details, expected)
    }
    pub fn delete_calendar_guarded(&mut self, id: &str, force: bool) -> Result<()> {
        self.repository.delete_calendar_guarded(id, force)
    }
    pub fn delete_calendar(&mut self, id: &str) -> Result<()> {
        self.delete_calendar_guarded(id, true)
    }
    pub fn calendar_event_count(&self, id: &str) -> Result<usize> {
        self.repository.calendar_event_count(id)
    }
    pub fn set_default_calendar(&mut self, id: &str) -> Result<()> {
        self.require(id, None, "create")?;
        self.repository.set_default_calendar(id)
    }
    pub fn set_calendar_visibility(&mut self, id: &str, visible: bool) -> Result<()> {
        self.repository.set_calendar_visibility(id, visible)
    }
    pub fn get_series(&self, target: &str) -> Result<EventSeries> {
        self.repository.get_series(target)
    }
    pub fn event_actions(&self, target: &str) -> Result<EventActions> {
        let series = self.get_series(target)?;
        let caps = self
            .capabilities(&series.event.calendar_id, Some(&series.event))?
            .intersect(
                &self
                    .repository
                    .event_capabilities(target)
                    .unwrap_or_else(|_| Capabilities::read_only()),
            );
        Ok(EventActions::for_event(
            &caps,
            series.event.recurrence.is_some(),
            self.repository
                .calendar_account(&series.event.calendar_id)?
                .is_some(),
        ))
    }
    pub fn mutate_event(&mut self, request: EventMutation) -> Result<Vec<CalendarEvent>> {
        let series = self.get_series(&request.target)?;
        let actions = self.event_actions(&request.target)?;
        let scopes = if request.event.is_some() {
            actions.edit
        } else {
            actions.delete
        };
        let availability = scopes
            .iter()
            .find(|a| a.scope == request.scope)
            .ok_or_else(|| error("unsupported", "This scope is unavailable for this event."))?;
        if let Some(reason) = &availability.disabled_reason {
            return Err(error("readOnly", reason));
        }

        self.require(
            &series.event.calendar_id,
            Some(&series.event),
            if request.event.is_some() {
                "edit"
            } else {
                "delete"
            },
        )?;
        if series.event.recurrence.is_some() {
            self.require(&series.event.calendar_id, Some(&series.event), "recurrence")?;
        }
        if let Some(event) = &request.event {
            self.check_move(&series.event, event)?;
        }
        self.repository.mutate_event(request)
    }
    fn check_move(&self, original: &CalendarEvent, event: &CalendarEvent) -> Result<()> {
        if original.calendar_id != event.calendar_id {
            self.require(&original.calendar_id, Some(original), "move")?;
            self.require(&event.calendar_id, None, "move")?;
            if self
                .repository
                .calendar_account(&original.calendar_id)?
                .is_some()
                || self
                    .repository
                    .calendar_account(&event.calendar_id)?
                    .is_some()
            {
                return Err(error(
                    "unsupported",
                    "Remote-calendar moves are unavailable.",
                ));
            }
        }
        Ok(())
    }
    pub fn snapshot(&self, calendar: &str, range: EventRange) -> Result<CalendarSnapshot> {
        validate_range(&range)?;
        self.repository.snapshot_calendar(calendar, range)
    }
    pub fn get(&self, calendar: &str, id: &str) -> Result<CalendarEvent> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(error("validation", "Use the full event UUID."));
        }
        self.repository
            .get(id)?
            .filter(|e| e.calendar_id == calendar)
            .ok_or_else(|| error("notFound", "This event no longer exists."))
    }
    pub fn save(&mut self, event: CalendarEvent, existing: bool) -> Result<CalendarEvent> {
        let expected = if existing {
            self.repository.get(&event.id)?
        } else {
            None
        };
        self.save_checked(event, existing, expected)
    }
    pub fn save_checked(
        &mut self,
        event: CalendarEvent,
        existing: bool,
        expected: Option<CalendarEvent>,
    ) -> Result<CalendarEvent> {
        self.require(
            &event.calendar_id,
            expected
                .as_ref()
                .filter(|e| e.calendar_id == event.calendar_id),
            if existing { "edit" } else { "create" },
        )?;
        if let Some(original) = &expected {
            self.require(&original.calendar_id, Some(original), "edit")?;
            self.check_move(original, &event)?;
        }
        let event = validate_event(event)?;
        if event.recurrence.is_some() {
            self.require(&event.calendar_id, None, "recurrence")?;
        }
        self.repository.save_checked(event, existing, expected)
    }
    pub fn delete_checked(
        &mut self,
        calendar: &str,
        id: &str,
        expected: &CalendarEvent,
    ) -> Result<()> {
        if expected.id != id || expected.calendar_id != calendar {
            return Err(error(
                "conflict",
                "Deletion requires the original event identity.",
            ));
        }
        self.require(calendar, Some(expected), "delete")?;
        self.repository.delete_checked(id, expected)
    }
    pub fn undo_session(&mut self) -> Result<()> {
        for (calendar, event, operation) in self.repository.undo_requirements()? {
            self.require(&calendar, event.as_ref(), &operation)?;
        }
        self.repository.undo()
    }
    pub fn preferences(&self) -> Result<Preferences> {
        self.repository.preferences()
    }
    pub fn set_preferences(&mut self, value: Preferences) -> Result<()> {
        self.repository.set_preferences(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    use std::path::Path;
    struct TestProvider;
    impl Provider for TestProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "test".into(),
                name: "Test only".into(),
                connection_available: false,
                unavailable_reason: Some("Test fixture".into()),
                capabilities: self.capabilities(),
            }
        }
        fn capabilities(&self) -> Capabilities {
            let mut caps = Capabilities::local();
            caps.recurrence = false;
            caps.move_event = false;
            caps
        }
    }
    fn range() -> EventRange {
        EventRange {
            start: "2026-10-01T00:00:00Z".into(),
            end: "2026-11-01T00:00:00Z".into(),
            start_date: "2026-10-01".into(),
            end_date: "2026-11-01".into(),
        }
    }
    fn event(calendar: &str) -> CalendarEvent {
        CalendarEvent {
            id: uuid::Uuid::new_v4().to_string(),
            calendar_id: calendar.into(),
            title: "  Normalized  ".into(),
            location: "".into(),
            notes: "".into(),
            recurrence: None,
            occurrence: None,
            time: EventTime::AllDay {
                start_date: "2026-10-09".into(),
                end_date: "2026-10-11".into(),
            },
        }
    }
    fn details(name: &str) -> CalendarDetails {
        CalendarDetails {
            name: name.into(),
            color: "#718675".into(),
            description: "".into(),
        }
    }
    fn registry(path: &Path) -> (ProviderRegistry, String, String, String) {
        let mut r = ProviderRegistry::new(Repository::open(path).unwrap());
        r.register(Box::new(LocalProvider)).unwrap();
        r.register(Box::new(TestProvider)).unwrap();
        let a = r
            .repository
            .upsert_account("test", "subject-a", "Account A", ConnectionState::Connected)
            .unwrap();
        let b = r
            .repository
            .upsert_account(
                "test",
                "subject-b",
                "Account B",
                ConnectionState::AuthenticationRequired,
            )
            .unwrap();
        let one = r
            .repository
            .import_calendar(
                &a,
                "shared-remote-id",
                details("One"),
                Capabilities::local(),
            )
            .unwrap();
        let two = r
            .repository
            .import_calendar(
                &b,
                "shared-remote-id",
                details("Two"),
                Capabilities::local(),
            )
            .unwrap();
        let readonly = r
            .repository
            .import_calendar(&a, "read", details("Read only"), Capabilities::read_only())
            .unwrap();
        (r, one, two, readonly)
    }
    fn count(r: &ProviderRegistry) -> i64 {
        r.repository
            .db
            .query_row("SELECT count(*) FROM outbox", [], |row| row.get(0))
            .unwrap()
    }
    #[test]
    fn common_crud_collection_account_identity_and_undo_contract() {
        let (mut r, one, two, readonly) = registry(Path::new(":memory:"));
        assert_ne!(one, two);
        assert!(r.register(Box::new(TestProvider)).is_err());
        for calendar in ["local-default", &one, &two] {
            let saved = r.save(event(calendar), false).unwrap();
            assert_eq!(saved.title, "Normalized");
            assert!(r.save(saved.clone(), false).is_err());
            assert!(r.get(&readonly, &saved.id).is_err());
            assert_eq!(r.collection_snapshot(range()).unwrap().events.len(), 1);
            let changed = CalendarEvent {
                title: "Updated".into(),
                ..saved.clone()
            };
            r.save_checked(changed.clone(), true, Some(saved.clone()))
                .unwrap();
            assert_eq!(
                r.save_checked(saved.clone(), true, Some(saved.clone()))
                    .unwrap_err()
                    .code,
                "conflict"
            );
            r.undo_session().unwrap();
            assert_eq!(r.get(calendar, &saved.id).unwrap(), saved);
            r.delete_checked(calendar, &saved.id, &saved).unwrap();
            r.undo_session().unwrap();
            r.undo_session().unwrap();
            assert!(r.collection_snapshot(range()).unwrap().events.is_empty());
            assert_eq!(count(&r), 0);
        }
        let snapshot = r.collection_snapshot(range()).unwrap();
        assert_eq!(snapshot.accounts.len(), 2);
        assert_eq!(snapshot.calendars.len(), 4);
        assert!(!snapshot.calendar_capabilities[&readonly].edit);
        assert!(!snapshot.calendar_capabilities[&one].recurrence);
    }
    #[test]
    fn remote_mapping_collisions_and_wrong_account_ingestion() {
        let (mut r, one, two, _) = registry(Path::new(":memory:"));
        let a = r.repository.calendar_account(&one).unwrap().unwrap();
        let b = r.repository.calendar_account(&two).unwrap().unwrap();
        let first = r
            .repository
            .cache_event(
                &a,
                "same-event",
                event(&one),
                "rev-a",
                r#"{"untouched":"A"}"#,
                Capabilities::local(),
            )
            .unwrap();
        let second = r
            .repository
            .cache_event(
                &b,
                "same-event",
                event(&two),
                "rev-b",
                r#"{"untouched":"B"}"#,
                Capabilities::local(),
            )
            .unwrap();
        assert_ne!(first, second);
        assert_eq!(
            r.repository
                .cache_event(
                    &a,
                    "same-event",
                    event(&one),
                    "rev-c",
                    "{}",
                    Capabilities::local()
                )
                .unwrap(),
            first
        );
        assert_eq!(
            r.repository
                .cache_event(&a, "wrong", event(&two), "rev", "{}", Capabilities::local())
                .unwrap_err()
                .code,
            "providerIdentity"
        );
        assert!(r.get(&one, &second).is_err());
        assert_eq!(r.collection_snapshot(range()).unwrap().events.len(), 2);
        // Reconnecting cannot duplicate accounts or overwrite local presentation/exclusions.
        r.set_calendar_visibility(&one, false).unwrap();
        r.delete_calendar(&one).unwrap();
        assert_eq!(
            r.repository
                .upsert_account("test", "subject-a", "Updated", ConnectionState::Connected)
                .unwrap(),
            a
        );
        assert_eq!(
            r.repository
                .import_calendar(
                    &a,
                    "shared-remote-id",
                    details("Remote name"),
                    Capabilities::local()
                )
                .unwrap(),
            one
        );
        assert!(!r.calendars().unwrap().iter().any(|c| c.id == one));
        assert_eq!(r.accounts().unwrap().len(), 2);
    }
    #[test]
    fn permissions_event_restrictions_moves_and_unavailable_adapter() {
        let (mut r, one, two, readonly) = registry(Path::new(":memory:"));
        assert_eq!(
            r.save(event(&readonly), false).unwrap_err().code,
            "readOnly"
        );
        assert_eq!(
            r.set_default_calendar(&readonly).unwrap_err().code,
            "readOnly"
        );
        r.set_default_calendar(&one).unwrap();
        r.set_calendar_visibility(&readonly, false).unwrap();
        let a = r.repository.calendar_account(&one).unwrap().unwrap();
        let id = r
            .repository
            .cache_event(
                &a,
                "meeting",
                event(&one),
                "rev",
                "{\"attendees\":[1]}",
                Capabilities::read_only(),
            )
            .unwrap();
        let meeting = r.get(&one, &id).unwrap();
        assert_eq!(
            r.save_checked(meeting.clone(), true, Some(meeting.clone()))
                .unwrap_err()
                .code,
            "readOnly"
        );
        assert_eq!(
            r.delete_checked(&one, &id, &meeting).unwrap_err().code,
            "readOnly"
        );
        assert!(!r.collection_snapshot(range()).unwrap().event_capabilities[&id].edit);
        let saved = r.save(event(&one), false).unwrap();
        let moved = CalendarEvent {
            calendar_id: two.clone(),
            ..saved.clone()
        };
        assert!(r.save_checked(moved, true, Some(saved.clone())).is_err());
        r.repository
            .import_calendar(
                &a,
                "shared-remote-id",
                details("One"),
                Capabilities::read_only(),
            )
            .unwrap();
        assert_eq!(
            r.default_calendar_id().unwrap().as_deref(),
            Some("local-default")
        );
        assert_eq!(r.undo_session().unwrap_err().code, "readOnly");
        assert_eq!(r.repository.get(&saved.id).unwrap(), Some(saved));
        r.providers.remove("test");
        assert_eq!(r.collection_snapshot(range()).unwrap().events.len(), 2);
        assert!(r.save(event(&two), false).is_err());
        r.save(event("local-default"), false).unwrap();
    }
    #[test]
    fn durable_intent_restart_shadow_preservation_and_transaction_failure() {
        let path =
            std::env::temp_dir().join(format!("aika-provider-{}.sqlite3", uuid::Uuid::new_v4()));
        let (mut r, one, _, _) = registry(&path);
        let a = r.repository.calendar_account(&one).unwrap().unwrap();
        let id = r
            .repository
            .cache_event(
                &a,
                "server-event",
                event(&one),
                "r1",
                "{\"providerOnly\":true}",
                Capabilities::local(),
            )
            .unwrap();
        let original = r.get(&one, &id).unwrap();
        let changed = CalendarEvent {
            title: "Offline change".into(),
            ..original.clone()
        };
        r.save_checked(changed.clone(), true, Some(original.clone()))
            .unwrap();
        assert_eq!(count(&r), 1);
        r.repository
            .cache_event(
                &a,
                "server-event",
                original.clone(),
                "r2",
                "{\"providerOnly\":true,\"new\":1}",
                Capabilities::local(),
            )
            .unwrap();
        assert_eq!(r.get(&one, &id).unwrap(), changed);
        assert_eq!(r.delete_calendar(&one).unwrap_err().code, "pendingWork");
        // Inject failure in the outbox, proving effective mutation and history roll back.
        r.repository.db.execute_batch("CREATE TRIGGER reject_outbox BEFORE INSERT ON outbox BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        let failed = CalendarEvent {
            title: "Failed".into(),
            ..changed.clone()
        };
        assert!(r.save_checked(failed, true, Some(changed.clone())).is_err());
        assert_eq!(r.get(&one, &id).unwrap(), changed);
        assert_eq!(count(&r), 1);
        r.repository
            .db
            .execute_batch("DROP TRIGGER reject_outbox")
            .unwrap();
        r.undo_session().unwrap();
        assert_eq!(count(&r), 0);
        r.save_checked(changed.clone(), true, Some(original))
            .unwrap();
        drop(r);
        let mut reopened = ProviderRegistry::new(Repository::open(&path).unwrap());
        reopened.register(Box::new(LocalProvider)).unwrap();
        reopened.register(Box::new(TestProvider)).unwrap();
        assert_eq!(count(&reopened), 1);
        assert_eq!(reopened.get(&one, &id).unwrap(), changed);
        assert!(!reopened.collection_snapshot(range()).unwrap().can_undo);
        let (revision, shadow): (String, String) = reopened
            .repository
            .db
            .query_row(
                "SELECT revision,shadow_payload FROM remote_events WHERE id=?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(revision, "r2");
        assert!(shadow.contains("providerOnly"));
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn checked_permissions_survive_between_service_check_and_repository_transaction() {
        let (mut r, one, _, _) = registry(Path::new(":memory:"));
        let saved = r.save(event(&one), false).unwrap();
        r.repository
            .db
            .execute("UPDATE calendars SET writable=0 WHERE id=?1", [&one])
            .unwrap();
        assert_eq!(
            r.repository
                .save_checked(saved.clone(), true, Some(saved.clone()))
                .unwrap_err()
                .code,
            "readOnly"
        );
        assert_eq!(r.repository.undo().unwrap_err().code, "readOnly");
        assert_eq!(count(&r), 1);
        assert_eq!(r.repository.get(&saved.id).unwrap(), Some(saved));
    }
    #[test]
    fn service_uses_shared_series_reads_and_remote_outbox_without_network() {
        let (r, one, _, _) = registry(Path::new(":memory:"));
        let mut service = crate::service::CalendarService {
            lifecycle: None,
            providers: r,
            discoveries: Default::default(),
            recorded_connections: Default::default(),
        };
        let account = service
            .providers
            .repository
            .calendar_account(&one)
            .unwrap()
            .unwrap();
        let mut master = event(&one);
        master.recurrence = Some(Recurrence {
            frequency: RepeatFrequency::Daily,
            interval: 1,
            weekdays: vec![],
            monthly_on: MonthlyOn::Date,
            until: None,
            count: Some(3),
            segment: None,
        });
        let id = service
            .providers
            .repository
            .cache_event(
                &account,
                "recurring",
                master,
                "r",
                "{}",
                Capabilities::read_only(),
            )
            .unwrap();
        assert!(service.get_series(&id).unwrap().event.recurrence.is_some());
        assert_eq!(
            service.collection_snapshot(range()).unwrap().events.len(),
            3
        );
        let saved = service.save_checked(event(&one), false, None).unwrap();
        assert_eq!(count(&service.providers), 1);
        assert_eq!(service.get(&saved.id).unwrap(), saved);
        service.undo().unwrap();
        assert_eq!(count(&service.providers), 0);
        assert!(service.get(&saved.id).is_err());
    }
    #[test]
    fn operation_capabilities_do_not_collapse_to_a_writable_flag() {
        let (mut r, one, _, _) = registry(Path::new(":memory:"));
        let account = r.repository.calendar_account(&one).unwrap().unwrap();
        let mut caps = Capabilities::local();
        caps.create = false;
        r.repository
            .import_calendar(
                &account,
                "shared-remote-id",
                details("Edit only"),
                caps.clone(),
            )
            .unwrap();
        assert_eq!(r.save(event(&one), false).unwrap_err().code, "readOnly");
        assert_eq!(r.set_default_calendar(&one).unwrap_err().code, "readOnly");
        assert_eq!(
            r.repository.set_default_calendar(&one).unwrap_err().code,
            "readOnly"
        );
        let id = r
            .repository
            .cache_event(&account, "existing", event(&one), "r", "{}", caps)
            .unwrap();
        let original = r.get(&one, &id).unwrap();
        r.save_checked(
            CalendarEvent {
                title: "Allowed edit".into(),
                ..original.clone()
            },
            true,
            Some(original),
        )
        .unwrap();
        assert_eq!(count(&r), 1);
    }
    #[test]
    fn database_rejects_cross_account_remote_mappings() {
        let (r, one, two, _) = registry(Path::new(":memory:"));
        let a = r.repository.calendar_account(&one).unwrap().unwrap();
        assert!(r
            .repository
            .db
            .execute(
                "INSERT INTO remote_events VALUES(?1,?2,?3,'remote','r','{}','{}')",
                params![uuid::Uuid::new_v4().to_string(), a, two]
            )
            .is_err());
        assert!(r.repository.db.execute("INSERT INTO calendars(id,name,provider_id,account_id,remote_id) VALUES('bad','Bad','wrong',?1,'remote')",[a]).is_err());
    }
}
