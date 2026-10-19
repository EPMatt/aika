//! Shared desktop/CLI service; adapters never own application persistence.
use crate::contracts::*;
pub use aika_domain::{error, resolve_local, validate_event, validate_range};
use std::path::Path;
type Result<T> = std::result::Result<T, ServiceError>;
pub use crate::repository::CalendarManagementSnapshot;

/// Production facade keeps the renderer contract stable while dispatching providers.
pub struct CalendarService {
    pub(crate) providers: crate::providers::ProviderRegistry,
    pub(crate) recorded_connections:
        std::collections::BTreeMap<String, (String, std::time::Instant)>,
    pub(crate) discoveries: std::collections::BTreeMap<String, (CalendarDiscovery, String)>,
}
impl CalendarService {
    pub fn open(path: &Path) -> Result<Self> {
        let mut providers =
            crate::providers::ProviderRegistry::new(crate::repository::Repository::open(path)?);
        providers.register(Box::new(crate::providers::LocalProvider))?;
        providers.register(Box::<crate::google::GoogleProvider>::default())?;
        Ok(Self {
            providers,
            discoveries: Default::default(),
            recorded_connections: Default::default(),
        })
    }
    pub fn providers(&self) -> Vec<ProviderDescriptor> {
        self.providers.descriptors()
    }
    /// Clone the adapter while holding service state; do HTTP/browser/store work after releasing it.
    pub fn connection_adapter(
        &self,
        provider: &str,
    ) -> Result<std::sync::Arc<dyn crate::providers::Provider>> {
        self.providers.adapter(provider)
    }
    pub fn account(&self, id: &str) -> Result<AccountSummary> {
        self.accounts()?
            .into_iter()
            .find(|a| a.id == id)
            .ok_or_else(|| error("notFound", "Unknown connected account."))
    }
    pub fn record_connection(
        &mut self,
        provider: &str,
        mut session: ConnectionSession,
        identity: Option<crate::providers::ConnectedIdentity>,
    ) -> Result<ConnectionSession> {
        self.providers.adapter(provider)?;
        let key = format!("{provider}:{}", session.id);
        self.recorded_connections
            .retain(|_, (_, recorded)| recorded.elapsed() < std::time::Duration::from_secs(600));
        if let Some((account, _)) = self.recorded_connections.get(&key) {
            session.account_id = Some(account.clone());
            return Ok(session);
        }
        if let Some(identity) = identity {
            session.account_id = Some(self.providers.repository.upsert_account(
                provider,
                &identity.subject,
                &identity.display_name,
                ConnectionState::Connected,
            )?);
            self.recorded_connections.insert(
                key,
                (
                    session.account_id.clone().expect("account recorded"),
                    std::time::Instant::now(),
                ),
            );
        }
        Ok(session)
    }
    pub fn prepare_discovery(
        &mut self,
        account: &AccountSummary,
        mut calendars: Vec<DiscoveredCalendar>,
    ) -> Result<CalendarDiscovery> {
        let current = self.account(&account.id)?;
        if current.connection_state != ConnectionState::Connected
            || current.remote_subject != account.remote_subject
        {
            return Err(error(
                "authenticationRequired",
                "Reconnect the account before discovering calendars.",
            ));
        }
        let expected = self
            .providers
            .repository
            .prepare_selection(&account.id, &mut calendars)?;
        let discovery = CalendarDiscovery {
            id: uuid::Uuid::new_v4().to_string(),
            account: current,
            calendars,
        };
        // Only the newest discovery for an account remains committable; no unbounded session cache.
        self.discoveries
            .retain(|_, (d, _)| d.account.id != account.id);
        self.discoveries
            .insert(discovery.id.clone(), (discovery.clone(), expected));
        Ok(discovery)
    }
    pub fn commit_selection(&mut self, discovery_id: &str, selected: Vec<String>) -> Result<()> {
        let (discovery, expected) = self
            .discoveries
            .get(discovery_id)
            .ok_or_else(|| error("notFound", "Discovery expired. Discover calendars again."))?;
        let capabilities = self
            .providers
            .adapter(&discovery.account.provider_id)?
            .capabilities();
        self.providers.repository.commit_selection(
            &discovery.account.id,
            &discovery.calendars,
            &selected,
            expected,
            &capabilities,
        )?;
        self.discoveries.remove(discovery_id);
        Ok(())
    }
    pub fn account_failure(&mut self, account: &str, failure: &ServiceError) -> Result<()> {
        if failure.code == "authenticationRequired" {
            self.providers
                .repository
                .set_account_state(account, ConnectionState::AuthenticationRequired)?;
        }
        Ok(())
    }
    pub fn record_disconnect(&mut self, id: &str) -> Result<()> {
        self.providers
            .repository
            .set_account_state(id, ConnectionState::Disconnected)?;
        self.discoveries.retain(|_, (d, _)| d.account.id != id);
        Ok(())
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

#[cfg(test)]
mod connection_tests {
    use super::*;
    fn discovered(id: &str, eligible: bool) -> DiscoveredCalendar {
        DiscoveredCalendar {
            remote_id: id.into(),
            name: format!("Remote {id}"),
            color: "#123456".into(),
            description: "".into(),
            access_role: if eligible { "reader" } else { "freeBusyReader" }.into(),
            eligible,
            disabled_reason: (!eligible).then(|| "Free/busy only".into()),
            selected: eligible,
        }
    }
    fn account(s: &mut CalendarService, subject: &str) -> AccountSummary {
        let id = s
            .providers
            .repository
            .upsert_account("google", subject, "Disposable", ConnectionState::Connected)
            .unwrap();
        s.account(&id).unwrap()
    }
    #[test]
    fn selection_atomic_reconnect_exclusions_visibility_and_account_isolation() {
        let directory =
            std::env::temp_dir().join(format!("aika-selection-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("db");
        let mut s = CalendarService::open(&path).unwrap();
        let a = account(&mut s, "subject-a");
        let b = account(&mut s, "subject-b");
        let entries = vec![
            discovered("shared", true),
            discovered("excluded", true),
            discovered("busy", false),
        ];
        let d = s.prepare_discovery(&a, entries.clone()).unwrap();
        assert_eq!(d.calendars.iter().filter(|c| c.selected).count(), 2);
        assert_eq!(s.providers.calendars().unwrap().len(), 1); // discovery alone never imports
        assert!(s.commit_selection(&d.id, vec!["busy".into()]).is_err());
        s.commit_selection(&d.id, vec!["shared".into()]).unwrap();
        assert!(s.commit_selection(&d.id, vec![]).is_err()); // one use
        let id = s.account(&a.id).unwrap().selected_calendar_ids[0].clone();
        assert!(!s.providers.capabilities(&id, None).unwrap().create);
        assert_eq!(
            s.default_calendar_id().unwrap().as_deref(),
            Some("local-default")
        );
        s.set_calendar_visibility(&id, false).unwrap();
        let calendar = s
            .providers
            .calendars()
            .unwrap()
            .into_iter()
            .find(|c| c.id == id)
            .unwrap();
        s.update_calendar(
            &id,
            CalendarDetails {
                name: "My name".into(),
                color: "#5B7FA3".into(),
                description: "My description".into(),
            },
            calendar,
        )
        .unwrap();
        let d = s.prepare_discovery(&b, entries.clone()).unwrap();
        s.commit_selection(&d.id, vec!["shared".into()]).unwrap();
        assert_ne!(id, s.account(&b.id).unwrap().selected_calendar_ids[0]);
        assert_eq!(account(&mut s, "subject-a").id, a.id);
        let mut rediscovered = entries.clone();
        rediscovered.push(discovered("new", true));
        let d = s.prepare_discovery(&a, rediscovered).unwrap();
        assert!(
            !d.calendars
                .iter()
                .find(|c| c.remote_id == "excluded")
                .unwrap()
                .selected
        );
        assert!(
            d.calendars
                .iter()
                .find(|c| c.remote_id == "new")
                .unwrap()
                .selected
        );
        s.commit_selection(&d.id, vec!["shared".into(), "new".into()])
            .unwrap();
        let calendar = s
            .providers
            .calendars()
            .unwrap()
            .into_iter()
            .find(|c| c.id == id)
            .unwrap();
        assert_eq!(calendar.name, "My name");
        assert_eq!(calendar.color, "#5B7FA3");
        assert!(s
            .calendar_management_snapshot()
            .unwrap()
            .hidden_calendar_ids
            .contains(&id));
        // CLI/process selection races invalidate a prepared selection.
        let stale = s.prepare_discovery(&a, entries.clone()).unwrap();
        let mut other = CalendarService::open(&path).unwrap();
        other.delete_calendar(&id).unwrap();
        assert_eq!(
            s.commit_selection(&stale.id, vec!["shared".into()])
                .unwrap_err()
                .code,
            "conflict"
        );
        let d = s.prepare_discovery(&a, entries.clone()).unwrap();
        assert!(
            !d.calendars
                .iter()
                .find(|c| c.remote_id == "shared")
                .unwrap()
                .selected
        );
        s.commit_selection(&d.id, vec!["shared".into()]).unwrap();
        assert!(s
            .account(&a.id)
            .unwrap()
            .selected_calendar_ids
            .contains(&id));
        drop(other);
        drop(s);
        let s = CalendarService::open(&path).unwrap();
        assert_eq!(s.accounts().unwrap().len(), 2);
        assert!(s
            .calendar_management_snapshot()
            .unwrap()
            .hidden_calendar_ids
            .contains(&id));
        drop(s);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn permission_loss_is_not_an_explicit_exclusion() {
        let mut s = CalendarService::open(Path::new(":memory:")).unwrap();
        let a = account(&mut s, "permissions");
        let d = s
            .prepare_discovery(&a, vec![discovered("one", true)])
            .unwrap();
        s.commit_selection(&d.id, vec!["one".into()]).unwrap();
        let id = s.account(&a.id).unwrap().selected_calendar_ids[0].clone();
        let d = s
            .prepare_discovery(
                &a,
                vec![discovered("one", false), discovered("new-busy", false)],
            )
            .unwrap();
        s.commit_selection(&d.id, vec![]).unwrap();
        assert!(s
            .account(&a.id)
            .unwrap()
            .selected_calendar_ids
            .contains(&id));
        let d = s
            .prepare_discovery(
                &a,
                vec![discovered("one", true), discovered("new-busy", true)],
            )
            .unwrap();
        assert!(d.calendars.iter().all(|c| c.selected));
        s.commit_selection(&d.id, vec!["one".into(), "new-busy".into()])
            .unwrap();
        assert!(s
            .account(&a.id)
            .unwrap()
            .selected_calendar_ids
            .contains(&id));
    }
    #[test]
    fn selection_rolls_back_pending_work_and_disconnect_does_not_clear_cache() {
        let mut s = CalendarService::open(Path::new(":memory:")).unwrap();
        let a = account(&mut s, "subject");
        let d = s
            .prepare_discovery(&a, vec![discovered("one", true), discovered("two", true)])
            .unwrap();
        s.commit_selection(&d.id, vec!["one".into(), "two".into()])
            .unwrap();
        let ids = s.account(&a.id).unwrap().selected_calendar_ids;
        s.providers.repository.db.execute("INSERT INTO outbox(id,account_id,calendar_id,event_id,operation) VALUES('pending',?1,?2,'event','create')",rusqlite::params![a.id,ids[1]]).unwrap();
        let d = s
            .prepare_discovery(&a, vec![discovered("one", true), discovered("two", true)])
            .unwrap();
        assert_eq!(
            s.commit_selection(&d.id, vec![]).unwrap_err().code,
            "pendingWork"
        );
        assert_eq!(s.account(&a.id).unwrap().selected_calendar_ids, ids);
        assert_eq!(s.record_disconnect(&a.id).unwrap_err().code, "pendingWork");
        s.providers
            .repository
            .db
            .execute("UPDATE outbox SET state='acknowledged'", [])
            .unwrap();
        s.record_disconnect(&a.id).unwrap();
        assert_eq!(s.providers.calendars().unwrap().len(), 3);
        assert_eq!(
            s.account(&a.id).unwrap().connection_state,
            ConnectionState::Disconnected
        );
        assert!(s.commit_selection(&d.id, vec![]).is_err());
        assert!(s
            .create_calendar(CalendarDetails {
                name: "Local still works".into(),
                color: "#718675".into(),
                description: "".into()
            })
            .is_ok());
    }
    #[test]
    fn completed_session_poll_does_not_reconnect_after_disconnect_and_secrets_never_enter_db() {
        let mut s = CalendarService::open(Path::new(":memory:")).unwrap();
        let session = ConnectionSession {
            id: "opaque-session".into(),
            progress: ConnectionProgress::Connected,
            account_id: None,
            failure: None,
        };
        let identity = crate::providers::ConnectedIdentity {
            subject: "fictional-subject".into(),
            display_name: "test@example.invalid".into(),
        };
        let result = s
            .record_connection("google", session.clone(), Some(identity.clone()))
            .unwrap();
        let id = result.account_id.unwrap();
        s.record_disconnect(&id).unwrap();
        assert_eq!(
            s.record_connection("google", session, Some(identity))
                .unwrap()
                .account_id
                .as_deref(),
            Some(id.as_str())
        );
        assert_eq!(
            s.account(&id).unwrap().connection_state,
            ConnectionState::Disconnected
        );
        assert_eq!(s.accounts().unwrap().len(), 1);
        let columns: String = s
            .providers
            .repository
            .db
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='accounts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        for secret in [
            "access_token",
            "refresh_token",
            "verifier",
            "authorization_code",
        ] {
            assert!(!columns.contains(secret));
        }
        s.account_failure(&id, &error("authenticationRequired", "Reconnect."))
            .unwrap();
        assert_eq!(
            s.account(&id).unwrap().connection_state,
            ConnectionState::AuthenticationRequired
        );
    }
}
