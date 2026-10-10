//! Backend-only provider registration. Adapters receive normalized, scoped operations.
use crate::contracts::*;
use crate::service::{
    error, validate_event, validate_range, CalendarManagementSnapshot, LocalProvider,
};
use std::collections::BTreeMap;

type Result<T> = std::result::Result<T, ServiceError>;
#[derive(Clone)]
pub struct ProviderDescriptor {
    pub id: String,
    pub name: String,
    pub writable: bool,
    pub authentication: bool,
    pub synchronization: bool,
}

pub trait Provider: Send {
    fn descriptor(&self) -> ProviderDescriptor;
    fn calendars(&self) -> Result<Vec<Calendar>>;
    fn snapshot(&self, calendar: &str, range: EventRange) -> Result<CalendarSnapshot>;
    fn get(&self, calendar: &str, id: &str) -> Result<CalendarEvent>;
    fn save_checked(
        &mut self,
        event: CalendarEvent,
        existing: bool,
        expected: Option<CalendarEvent>,
    ) -> Result<CalendarEvent>;
    fn delete_checked(&mut self, calendar: &str, id: &str, expected: &CalendarEvent) -> Result<()>;
    fn undo(&mut self) -> Result<()> {
        Err(error("unsupported", "This provider does not support undo."))
    }
    fn preferences(&self) -> Result<Preferences> {
        Err(error(
            "unsupported",
            "This provider does not store preferences.",
        ))
    }
    fn set_preferences(&mut self, _: Preferences) -> Result<()> {
        Err(error(
            "unsupported",
            "This provider does not store preferences.",
        ))
    }

    fn collection_snapshot(&mut self, _range: EventRange) -> Result<CalendarCollectionSnapshot> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn calendar_management_snapshot(&self) -> Result<CalendarManagementSnapshot> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn delete_calendar_guarded(&mut self, _id: &str, _force: bool) -> Result<()> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn default_calendar_id(&self) -> Result<Option<String>> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn create_calendar(&mut self, _details: CalendarDetails) -> Result<Calendar> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn update_calendar(
        &mut self,
        _id: &str,
        _details: CalendarDetails,
        _expected: Calendar,
    ) -> Result<()> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn delete_calendar(&mut self, _id: &str) -> Result<()> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn calendar_event_count(&self, _id: &str) -> Result<usize> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn set_default_calendar(&mut self, _id: &str) -> Result<()> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    fn set_calendar_visibility(&mut self, _id: &str, _visible: bool) -> Result<()> {
        Err(error("unsupported", "Calendar management is unavailable."))
    }
    // Auth and synchronization are explicit extension points, not renderer commands.
    fn authenticate(&mut self, _: &crate::credentials::CredentialService) -> Result<()> {
        Err(error(
            "unsupported",
            "Authentication is unavailable for this provider.",
        ))
    }
    fn synchronize(&mut self) -> Result<()> {
        Err(error(
            "unsupported",
            "Synchronization is unavailable for this provider.",
        ))
    }
}

impl Provider for LocalProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "local".into(),
            name: "On this device".into(),
            writable: true,
            authentication: false,
            synchronization: false,
        }
    }
    fn calendars(&self) -> Result<Vec<Calendar>> {
        LocalProvider::calendars(self)
    }
    fn snapshot(&self, calendar: &str, range: EventRange) -> Result<CalendarSnapshot> {
        self.snapshot_calendar(calendar, range)
    }
    fn get(&self, calendar: &str, id: &str) -> Result<CalendarEvent> {
        LocalProvider::get(self, id)?
            .filter(|e| e.calendar_id == calendar)
            .ok_or_else(|| error("notFound", "This event no longer exists."))
    }
    fn collection_snapshot(&mut self, range: EventRange) -> Result<CalendarCollectionSnapshot> {
        LocalProvider::collection_snapshot(self, range)
    }
    fn default_calendar_id(&self) -> Result<Option<String>> {
        LocalProvider::default_calendar_id(self)
    }
    fn calendar_management_snapshot(&self) -> Result<CalendarManagementSnapshot> {
        LocalProvider::calendar_management_snapshot(self)
    }
    fn delete_calendar_guarded(&mut self, id: &str, force: bool) -> Result<()> {
        LocalProvider::delete_calendar_guarded(self, id, force)
    }
    fn create_calendar(&mut self, details: CalendarDetails) -> Result<Calendar> {
        LocalProvider::create_calendar(self, details)
    }
    fn update_calendar(
        &mut self,
        id: &str,
        details: CalendarDetails,
        expected: Calendar,
    ) -> Result<()> {
        LocalProvider::update_calendar(self, id, details, expected)
    }
    fn delete_calendar(&mut self, id: &str) -> Result<()> {
        LocalProvider::delete_calendar(self, id)
    }
    fn calendar_event_count(&self, id: &str) -> Result<usize> {
        LocalProvider::calendar_event_count(self, id)
    }
    fn set_default_calendar(&mut self, id: &str) -> Result<()> {
        LocalProvider::set_default_calendar(self, id)
    }
    fn set_calendar_visibility(&mut self, id: &str, visible: bool) -> Result<()> {
        LocalProvider::set_calendar_visibility(self, id, visible)
    }
    fn save_checked(
        &mut self,
        event: CalendarEvent,
        existing: bool,
        expected: Option<CalendarEvent>,
    ) -> Result<CalendarEvent> {
        LocalProvider::save_checked(self, event, existing, expected)
    }
    fn delete_checked(&mut self, calendar: &str, id: &str, expected: &CalendarEvent) -> Result<()> {
        if expected.calendar_id != calendar {
            return Err(error(
                "conflict",
                "Deletion requires the original calendar.",
            ));
        }
        LocalProvider::delete_checked(self, id, expected)
    }
    fn undo(&mut self) -> Result<()> {
        LocalProvider::undo(self)
    }
    fn preferences(&self) -> Result<Preferences> {
        LocalProvider::preferences(self)
    }
    fn set_preferences(&mut self, value: Preferences) -> Result<()> {
        LocalProvider::set_preferences(self, value)
    }
}

#[derive(Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, Box<dyn Provider>>,
}
impl ProviderRegistry {
    /// Registration is atomic; duplicate global calendar identities are rejected.
    pub fn register(&mut self, provider: Box<dyn Provider>) -> Result<()> {
        let descriptor = provider.descriptor();
        if descriptor.id.is_empty() || self.providers.contains_key(&descriptor.id) {
            return Err(error(
                "providerIdentity",
                "Duplicate or invalid provider identity.",
            ));
        }
        let known = self.live_calendars()?;
        let calendars = provider.calendars()?;
        let mut discovered = BTreeMap::new();
        for calendar in calendars {
            if calendar.id.is_empty()
                || calendar.provider_id != descriptor.id
                || known.iter().any(|c| c.id == calendar.id)
                || discovered.contains_key(&calendar.id)
            {
                return Err(error(
                    "providerIdentity",
                    "Duplicate or invalid calendar identity.",
                ));
            }
            discovered.insert(calendar.id.clone(), calendar);
        }
        self.providers.insert(descriptor.id, provider);
        Ok(())
    }
    pub fn calendars(&self) -> Result<Vec<Calendar>> {
        self.live_calendars()
    }
    pub fn descriptors(&self) -> Vec<ProviderDescriptor> {
        self.providers.values().map(|p| p.descriptor()).collect()
    }
    pub fn live_calendars(&self) -> Result<Vec<Calendar>> {
        let mut discovered = BTreeMap::new();
        let mut ordered = Vec::new();
        for (owner, provider) in &self.providers {
            for calendar in provider.calendars()? {
                if calendar.id.is_empty()
                    || calendar.provider_id != *owner
                    || discovered.insert(calendar.id.clone(), ()).is_some()
                {
                    return Err(error(
                        "providerIdentity",
                        "Duplicate or invalid calendar identity.",
                    ));
                }
                ordered.push(calendar);
            }
        }
        Ok(ordered)
    }
    fn owner(&self, calendar: &str, write: bool) -> Result<String> {
        let calendar = self
            .live_calendars()?
            .into_iter()
            .find(|c| c.id == calendar)
            .ok_or_else(|| error("notFound", "Unknown calendar."))?;
        let descriptor = self.providers[&calendar.provider_id].descriptor();
        if write && (!calendar.writable || !descriptor.writable) {
            return Err(error("readOnly", "This calendar is read-only."));
        }
        Ok(calendar.provider_id)
    }
    fn local(&self) -> Result<&(dyn Provider + '_)> {
        match self.providers.get("local") {
            Some(p) => Ok(p.as_ref()),
            None => Err(error("notFound", "Local provider unavailable.")),
        }
    }
    fn local_mut(&mut self) -> Result<&mut (dyn Provider + '_)> {
        match self.providers.get_mut("local") {
            Some(p) => Ok(p.as_mut()),
            None => Err(error("notFound", "Local provider unavailable.")),
        }
    }
    pub fn collection_snapshot(&mut self, range: EventRange) -> Result<CalendarCollectionSnapshot> {
        self.local_mut()?.collection_snapshot(range)
    }
    pub fn default_calendar_id(&self) -> Result<Option<String>> {
        self.local()?.default_calendar_id()
    }
    pub fn calendar_management_snapshot(&self) -> Result<CalendarManagementSnapshot> {
        self.local()?.calendar_management_snapshot()
    }
    pub fn delete_calendar_guarded(&mut self, id: &str, force: bool) -> Result<()> {
        self.local_mut()?.delete_calendar_guarded(id, force)
    }
    pub fn create_calendar(&mut self, details: CalendarDetails) -> Result<Calendar> {
        self.local_mut()?.create_calendar(details)
    }
    pub fn update_calendar(
        &mut self,
        id: &str,
        details: CalendarDetails,
        expected: Calendar,
    ) -> Result<()> {
        self.local_mut()?.update_calendar(id, details, expected)
    }
    pub fn delete_calendar(&mut self, id: &str) -> Result<()> {
        self.local_mut()?.delete_calendar(id)
    }
    pub fn calendar_event_count(&self, id: &str) -> Result<usize> {
        self.local()?.calendar_event_count(id)
    }
    pub fn set_default_calendar(&mut self, id: &str) -> Result<()> {
        self.local_mut()?.set_default_calendar(id)
    }
    pub fn set_calendar_visibility(&mut self, id: &str, visible: bool) -> Result<()> {
        self.local_mut()?.set_calendar_visibility(id, visible)
    }
    pub fn undo_local(&mut self) -> Result<()> {
        self.local_mut()?.undo()
    }
    pub fn snapshot(&self, calendar: &str, range: EventRange) -> Result<CalendarSnapshot> {
        validate_range(&range)?;
        let owner = self.owner(calendar, false)?;
        let mut result = self.providers[&owner].snapshot(calendar, range)?;
        if result.calendar.id != calendar
            || result.calendar.provider_id != owner
            || result.events.iter().any(|e| e.calendar_id != calendar)
        {
            return Err(error(
                "providerIdentity",
                "Provider returned data from another calendar.",
            ));
        }
        result.calendar = self
            .live_calendars()?
            .into_iter()
            .find(|c| c.id == calendar)
            .ok_or_else(|| error("notFound", "Unknown calendar."))?;
        Ok(result)
    }
    pub fn get(&self, calendar: &str, id: &str) -> Result<CalendarEvent> {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(error("validation", "Use the full event UUID."));
        }
        let owner = self.owner(calendar, false)?;
        let event = self.providers[&owner].get(calendar, id)?;
        if event.calendar_id != calendar || event.id != id {
            return Err(error(
                "providerIdentity",
                "Provider returned an event from another identity.",
            ));
        }
        Ok(event)
    }
    pub fn save(&mut self, event: CalendarEvent, existing: bool) -> Result<CalendarEvent> {
        let expected = if existing {
            Some(self.get(&event.calendar_id, &event.id)?)
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
        let owner = self.owner(&event.calendar_id, true)?;
        if let Some(original) = &expected {
            let source = self.owner(&original.calendar_id, true)?;
            if source != owner || (original.calendar_id != event.calendar_id && owner != "local") {
                return Err(error(
                    "conflict",
                    "Events can only move between local calendars.",
                ));
            }
        }
        if existing != expected.is_some() || expected.as_ref().is_some_and(|e| e.id != event.id) {
            return Err(error(
                "conflict",
                "An update requires the original event identity.",
            ));
        }
        let event = validate_event(event)?;
        self.providers
            .get_mut(&owner)
            .unwrap()
            .save_checked(event, existing, expected)
    }
    pub fn delete(&mut self, calendar: &str, id: &str) -> Result<()> {
        self.owner(calendar, true)?;
        let expected = self.get(calendar, id)?;
        self.delete_checked(calendar, id, &expected)
    }
    pub fn delete_checked(
        &mut self,
        calendar: &str,
        id: &str,
        expected: &CalendarEvent,
    ) -> Result<()> {
        let owner = self.owner(calendar, true)?;
        if expected.id != id || expected.calendar_id != calendar {
            return Err(error(
                "conflict",
                "Deletion requires the original event identity.",
            ));
        }
        self.providers
            .get_mut(&owner)
            .unwrap()
            .delete_checked(calendar, id, expected)
    }
    pub fn undo(&mut self, calendar: &str) -> Result<()> {
        let owner = self.owner(calendar, true)?;
        self.providers.get_mut(&owner).unwrap().undo()
    }
    pub fn preferences(&self) -> Result<Preferences> {
        self.providers
            .get("local")
            .ok_or_else(|| error("notFound", "Local provider unavailable."))?
            .preferences()
    }
    pub fn set_preferences(&mut self, value: Preferences) -> Result<()> {
        self.providers
            .get_mut("local")
            .ok_or_else(|| error("notFound", "Local provider unavailable."))?
            .set_preferences(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Deliberately independent of SQLite and compiled only into the Rust test harness.
    struct TestProvider {
        leak: bool,
        calendars: Vec<Calendar>,
        events: BTreeMap<String, CalendarEvent>,
    }
    impl TestProvider {
        fn new() -> Self {
            Self {
                leak: false,
                calendars: vec![
                    Calendar {
                        id: "test-write".into(),
                        provider_id: "test".into(),
                        name: "Writable".into(),
                        color: "#718675".into(),
                        description: "".into(),
                        writable: true,
                    },
                    Calendar {
                        id: "test-read".into(),
                        provider_id: "test".into(),
                        name: "Read only".into(),
                        color: "#718675".into(),
                        description: "".into(),
                        writable: false,
                    },
                ],
                events: BTreeMap::new(),
            }
        }
    }
    impl Provider for TestProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "test".into(),
                name: "Test adapter".into(),
                writable: true,
                authentication: false,
                synchronization: false,
            }
        }
        fn calendars(&self) -> Result<Vec<Calendar>> {
            Ok(self.calendars.clone())
        }
        fn snapshot(&self, calendar: &str, range: EventRange) -> Result<CalendarSnapshot> {
            Ok(CalendarSnapshot {
                calendar: self
                    .calendars
                    .iter()
                    .find(|c| c.id == calendar)
                    .unwrap()
                    .clone(),
                events: self
                    .events
                    .values()
                    .filter(|e| {
                        (self.leak || e.calendar_id == calendar)
                            && match &e.time {
                                EventTime::Timed { start, end, .. } => {
                                    start < &range.end && end > &range.start
                                }
                                EventTime::AllDay {
                                    start_date,
                                    end_date,
                                } => start_date < &range.end_date && end_date > &range.start_date,
                            }
                    })
                    .cloned()
                    .collect(),
                can_undo: false,
            })
        }
        fn get(&self, calendar: &str, id: &str) -> Result<CalendarEvent> {
            self.events
                .get(id)
                .filter(|e| self.leak || e.calendar_id == calendar)
                .cloned()
                .ok_or_else(|| error("notFound", "Event not in calendar."))
        }
        fn save_checked(
            &mut self,
            event: CalendarEvent,
            existing: bool,
            expected: Option<CalendarEvent>,
        ) -> Result<CalendarEvent> {
            let previous = self.events.get(&event.id);
            if previous != expected.as_ref()
                || existing != previous.is_some()
                || previous.is_some_and(|p| p.calendar_id != event.calendar_id)
            {
                return Err(error("conflict", "Event identity mismatch."));
            }
            self.events.insert(event.id.clone(), event.clone());
            Ok(event)
        }
        fn delete_checked(
            &mut self,
            calendar: &str,
            id: &str,
            expected: &CalendarEvent,
        ) -> Result<()> {
            if !self
                .events
                .get(id)
                .is_some_and(|e| e.calendar_id == calendar)
            {
                return Err(error("notFound", "Event not in calendar."));
            }
            if self.events.get(id) != Some(expected) {
                return Err(error("conflict", "Event changed."));
            }
            self.events.remove(id);
            Ok(())
        }
    }
    fn registry() -> ProviderRegistry {
        let mut r = ProviderRegistry::default();
        r.register(Box::new(
            LocalProvider::open(std::path::Path::new(":memory:")).unwrap(),
        ))
        .unwrap();
        r.register(Box::new(TestProvider::new())).unwrap();
        r
    }
    fn range() -> EventRange {
        EventRange {
            start: "2026-10-01T00:00:00.000Z".into(),
            end: "2026-11-01T00:00:00.000Z".into(),
            start_date: "2026-10-01".into(),
            end_date: "2026-11-01".into(),
        }
    }
    fn event(calendar: &str) -> CalendarEvent {
        CalendarEvent {
            id: uuid::Uuid::new_v4().to_string(),
            calendar_id: calendar.into(),
            title: "  Normalized  ".into(),
            time: EventTime::AllDay {
                start_date: "2026-10-09".into(),
                end_date: "2026-10-11".into(),
            },
            location: "".into(),
            notes: "".into(),
        }
    }
    #[test]
    fn shared_crud_contract_and_isolation() {
        let mut r = registry();
        for calendar in ["local-default", "test-write"] {
            let mut e = r.save(event(calendar), false).unwrap();
            assert_eq!(e.title, "Normalized");
            assert!(r.save(e.clone(), false).is_err());
            assert_eq!(r.snapshot(calendar, range()).unwrap().events.len(), 1);
            e.title = "Updated".into();
            r.save(e.clone(), true).unwrap();
            assert_eq!(
                r.snapshot(calendar, range()).unwrap().events[0].title,
                "Updated"
            );
            let other = if calendar == "local-default" {
                "test-write"
            } else {
                "local-default"
            };
            assert!(r.snapshot(other, range()).unwrap().events.is_empty());
            assert!(r.delete(other, &e.id).is_err());
            assert_eq!(r.snapshot(calendar, range()).unwrap().events.len(), 1);
            r.delete(calendar, &e.id).unwrap();
            assert!(r.snapshot(calendar, range()).unwrap().events.is_empty());
        }
        assert_eq!(r.calendars().unwrap().len(), 3);
        assert_eq!(r.descriptors().len(), 2);
    }
    #[test]
    fn readonly_identity_bounds_and_unsupported_operations() {
        let mut r = registry();
        assert_eq!(
            r.save(event("test-read"), false).err().unwrap().code,
            "readOnly"
        );
        assert_eq!(
            r.delete("test-read", "anything").unwrap_err().code,
            "readOnly"
        );
        assert_eq!(r.undo("test-read").unwrap_err().code, "readOnly");
        assert!(r.save(event("missing"), false).is_err());
        assert!(r.snapshot("missing", range()).is_err());
        let mut huge = range();
        huge.end_date = "2028-01-01".into();
        assert!(r.snapshot("test-write", huge).is_err());
        assert_eq!(r.undo("test-write").unwrap_err().code, "unsupported");
        let mut p = TestProvider::new();
        assert_eq!(
            p.authenticate(&crate::credentials::CredentialService::default())
                .unwrap_err()
                .code,
            "unsupported"
        );
        assert_eq!(p.synchronize().unwrap_err().code, "unsupported");
        assert!(r.register(Box::new(TestProvider::new())).is_err());
        let mut collision = TestProvider::new();
        collision.calendars[0].id = "local-default".into();
        let mut empty = ProviderRegistry::default();
        empty
            .register(Box::new(
                LocalProvider::open(std::path::Path::new(":memory:")).unwrap(),
            ))
            .unwrap();
        assert!(empty.register(Box::new(collision)).is_err());
        assert_eq!(empty.calendars().unwrap().len(), 1);
    }
    #[test]
    fn adapter_cannot_leak_calendar_data_and_local_undo_still_works() {
        let mut r = registry();
        let e = r.save(event("local-default"), false).unwrap();
        r.undo("local-default").unwrap();
        assert!(r
            .snapshot("local-default", range())
            .unwrap()
            .events
            .is_empty());
        r.save(e.clone(), false).unwrap();
        let mut wrong = e;
        wrong.calendar_id = "test-write".into();
        assert!(r.save(wrong, true).is_err());
        assert_eq!(
            r.snapshot("local-default", range()).unwrap().events.len(),
            1
        );
        // A broken adapter is rejected even if it returns foreign events under its own metadata.
        let mut p = TestProvider::new();
        let foreign = event("local-default");
        let foreign_id = foreign.id.clone();
        p.events.insert(foreign.id.clone(), foreign);
        p.leak = true;
        let mut leaking = ProviderRegistry::default();
        leaking.register(Box::new(p)).unwrap();
        assert_eq!(
            leaking.get("test-write", &foreign_id).unwrap_err().code,
            "providerIdentity"
        );
        assert_eq!(
            leaking.snapshot("test-write", range()).err().unwrap().code,
            "providerIdentity"
        );
        let mut p = TestProvider::new();
        // Discovery rejects metadata claiming another provider.
        p.calendars[0].provider_id = "local".into();
        assert!(ProviderRegistry::default().register(Box::new(p)).is_err());
    }
}
