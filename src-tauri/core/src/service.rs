//! Shared desktop/CLI service; adapters never own application persistence.
use crate::contracts::*;
pub use aika_domain::{error, resolve_local, validate_event, validate_range};
use std::path::Path;
type Result<T> = std::result::Result<T, ServiceError>;
pub use crate::repository::CalendarManagementSnapshot;

/// Production facade keeps the renderer contract stable while dispatching providers.
pub struct CalendarService {
    pub(crate) providers: crate::providers::ProviderRegistry,
}
impl CalendarService {
    pub fn open(path: &Path) -> Result<Self> {
        let mut providers =
            crate::providers::ProviderRegistry::new(crate::repository::Repository::open(path)?);
        providers.register(Box::new(crate::providers::LocalProvider))?;
        Ok(Self { providers })
    }
    pub fn providers(&self) -> Vec<ProviderDescriptor> {
        self.providers.descriptors()
    }
    pub fn accounts(&self) -> Result<Vec<AccountSummary>> {
        self.providers.accounts()
    }
    pub fn get_series(&self, target: &str) -> Result<EventSeries> {
        self.providers.get_series(target)
    }
    pub fn mutate_event(&mut self, request: EventMutation) -> Result<Vec<CalendarEvent>> {
        self.providers.mutate_event(request)
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
        self.providers.undo_session()
    }
}
