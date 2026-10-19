//! Durable ordinary-event delivery. HTTP runs between fenced claim/apply transactions.
use crate::{
    contracts::*,
    service::{error, CalendarService},
    sync::{guard, mapping, IncomingItem, SyncClaim},
};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
type Result<T> = std::result::Result<T, ServiceError>;
#[derive(Clone, Debug)]
pub struct OutgoingOperation {
    pub id: String,
    pub event_id: String,
    pub calendar_id: String,
    pub remote_calendar: String,
    pub remote_id: String,
    pub operation: String,
    pub before: Option<EventSeries>,
    pub after: Option<EventSeries>,
    pub revision: Option<String>,
    pub recovering: bool,
    pub shadow: String,
}
pub enum OutgoingResult {
    Applied(Option<IncomingItem>),
    Conflict(Option<IncomingItem>),
}
fn series(item: &IncomingItem, id: &str, calendar: &str) -> Option<EventSeries> {
    item.event.as_ref().map(|event| {
        let mut event = event.clone();
        event.id = id.into();
        event.calendar_id = calendar.into();
        EventSeries {
            event,
            exceptions: vec![],
        }
    })
}
impl CalendarService {
    pub fn claim_outgoing(&mut self, claim: &SyncClaim) -> Result<Option<OutgoingOperation>> {
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard(&tx, claim)?;
        // In-flight operations belong to this account lease. A new lease recovers them;
        // acknowledged dependencies unblock exactly one operation per event.
        let row = tx.query_row("SELECT o.id,o.event_id,o.calendar_id,c.remote_id,o.operation,o.before_payload,o.after_payload,o.expected_revision,o.attempts,o.depends_on FROM outbox o JOIN calendars c ON c.id=o.calendar_id WHERE o.account_id=?1 AND o.state IN ('pending','inFlight') AND (o.retry_at IS NULL OR julianday(o.retry_at)<=julianday('now')) AND c.active=1 AND c.selected=1 AND NOT EXISTS(SELECT 1 FROM conflicts f WHERE f.event_id=o.event_id) AND (o.depends_on IS NULL OR EXISTS(SELECT 1 FROM outbox d WHERE d.id=o.depends_on AND d.state='acknowledged')) ORDER BY o.rowid LIMIT 1", [&claim.account.id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,u32>(8)?,r.get::<_,Option<String>>(9)?))).optional()?;
        let Some((
            id,
            event_id,
            calendar_id,
            remote_calendar,
            operation,
            before,
            after,
            mut revision,
            attempts,
            dependency,
        )) = row
        else {
            tx.commit()?;
            return Ok(None);
        };
        if let Some(dependency) = dependency {
            // The revision captured at mutation time may predate the predecessor's HTTP.
            let ack: Option<String> = tx.query_row(
                "SELECT acknowledgement FROM outbox WHERE id=?1",
                [dependency],
                |r| r.get(0),
            )?;
            if let Some(ack) = ack {
                let ack: serde_json::Value = serde_json::from_str(&ack)?;
                revision = ack["revision"].as_str().map(str::to_owned);
            }
        }
        if operation == "create" {
            revision = None;
        }
        let writable: bool = tx.query_row(
            "SELECT writable FROM calendars WHERE id=?1",
            [&calendar_id],
            |r| r.get(0),
        )?;
        if !writable {
            let failure = error(
                "permission",
                "This calendar is read-only. Restore permission, then retry pending changes.",
            );
            tx.execute(
                "UPDATE outbox SET state='blocked',acknowledgement=?2 WHERE id=?1",
                params![id, serde_json::to_string(&failure)?],
            )?;
            tx.commit()?;
            return Err(failure);
        }
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT remote_id,shadow_payload FROM remote_events WHERE id=?1",
                [&event_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let shadow = existing
            .as_ref()
            .map(|(_, shadow)| shadow.clone())
            .unwrap_or_else(|| "{}".into());
        // A compensation after a delete uses a fresh persisted ID: Google retains tombstones.
        let remote_id = match existing {
            Some((remote, _)) if !(operation == "create" && attempts == 0) => remote,
            _ => {
                let remote = uuid::Uuid::new_v4().simple().to_string();
                tx.execute("INSERT INTO remote_events(id,account_id,calendar_id,remote_id,revision,shadow_payload,capabilities) VALUES(?1,?2,?3,?4,NULL,?6,?5) ON CONFLICT(id) DO UPDATE SET remote_id=excluded.remote_id,revision=NULL,capabilities=excluded.capabilities",params![event_id,claim.account.id,calendar_id,remote,serde_json::to_string(&Capabilities::ordinary())?,shadow])?;
                tx.execute("DELETE FROM incoming_items WHERE id=?1", [&event_id])?;
                remote
            }
        };
        tx.execute("UPDATE outbox SET state='inFlight',attempts=attempts+1,expected_revision=?2 WHERE id=?1",params![id,revision])?;
        tx.commit()?;
        Ok(Some(OutgoingOperation {
            id,
            event_id,
            calendar_id,
            remote_calendar,
            remote_id,
            operation,
            before: before.map(|v| serde_json::from_str(&v)).transpose()?,
            after: after.map(|v| serde_json::from_str(&v)).transpose()?,
            revision,
            recovering: attempts > 0,
            shadow,
        }))
    }
    pub fn apply_outgoing(
        &mut self,
        claim: &SyncClaim,
        op: &OutgoingOperation,
        result: Result<OutgoingResult>,
    ) -> Result<()> {
        let result = match result {
            Ok(
                OutgoingResult::Applied(Some(ref item)) | OutgoingResult::Conflict(Some(ref item)),
            ) if item.remote_id != op.remote_id => Err(error(
                "providerIdentity",
                "Unexpected outgoing event identity.",
            )),
            other => other,
        };
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard(&tx, claim)?;
        let state: Option<String> = tx
            .query_row(
                "SELECT state FROM outbox WHERE id=?1 AND account_id=?2",
                params![op.id, claim.account.id],
                |r| r.get(0),
            )
            .optional()?;
        if state.as_deref() != Some("inFlight") {
            return Err(error(
                "syncSuperseded",
                "Outgoing operation changed. Refresh again.",
            ));
        }
        match result {
            Ok(result) => {
                let (conflict, item) = match result {
                    OutgoingResult::Applied(i) => (false, i),
                    OutgoingResult::Conflict(i) => (true, i),
                };
                let revision = item.as_ref().map(|i| i.revision.clone());
                if let Some(item) = &item {
                    mapping(
                        &tx,
                        &claim.account.id,
                        &op.calendar_id,
                        item,
                        claim.adapter.event_capabilities(item),
                    )?;
                    tx.execute("INSERT INTO incoming_items(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![op.event_id,serde_json::to_string(item)?])?;
                } else {
                    // Keep a normalized tombstone so full/incremental replay cannot resurrect it.
                    tx.execute(
                        "UPDATE remote_events SET revision=NULL WHERE id=?1",
                        [&op.event_id],
                    )?;
                    let tombstone = IncomingItem {
                        remote_id: op.remote_id.clone(),
                        revision: String::new(),
                        shadow: "{}".into(),
                        event: None,
                        master: None,
                        original_start: None,
                        unsupported: false,
                    };
                    tx.execute("INSERT INTO incoming_items(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![op.event_id,serde_json::to_string(&tombstone)?])?;
                }
                if conflict {
                    let remote = item
                        .as_ref()
                        .and_then(|i| series(i, &op.event_id, &op.calendar_id));
                    tx.execute("INSERT INTO conflicts(id,account_id,calendar_id,event_id,local_payload,remote_payload,remote_revision) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![uuid::Uuid::new_v4().to_string(),claim.account.id,op.calendar_id,op.event_id,op.after.as_ref().map(serde_json::to_string).transpose()?,remote.as_ref().map(serde_json::to_string).transpose()?,revision])?;
                    tx.execute("UPDATE outbox SET state='blocked',retry_at=NULL,acknowledgement=NULL WHERE id=?1", [&op.id])?;
                } else {
                    tx.execute("UPDATE outbox SET state='acknowledged',retry_at=NULL,acknowledgement=?2 WHERE id=?1",params![op.id,serde_json::to_string(&serde_json::json!({"revision":revision}))?])?;
                    // A late response must never replace a newer edit or undo compensation.
                    let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE event_id=?1 AND state!='acknowledged')",[&op.event_id],|r|r.get(0))?;
                    if !pending {
                        tx.execute("DELETE FROM events WHERE id=?1", [&op.event_id])?;
                        if let Some(s) = item
                            .as_ref()
                            .and_then(|i| series(i, &op.event_id, &op.calendar_id))
                        {
                            crate::repository::Repository::write(&tx, &s.event)?;
                        }
                    }
                }
            }
            Err(f) => {
                let transient = matches!(
                    f.code.as_str(),
                    "network" | "retryable" | "credentialUnavailable" | "authenticationRequired"
                );
                let attempts: u32 =
                    tx.query_row("SELECT attempts FROM outbox WHERE id=?1", [&op.id], |r| {
                        r.get(0)
                    })?;
                let jitter = (uuid::Uuid::new_v4().as_u128() % 17) as i64;
                let retry = (Utc::now()
                    + chrono::Duration::seconds((2i64.pow(attempts.min(10))).min(3600) + jitter))
                .to_rfc3339();
                tx.execute(
                    "UPDATE outbox SET state=?2,retry_at=?3,acknowledgement=?4 WHERE id=?1",
                    params![
                        op.id,
                        if transient { "pending" } else { "blocked" },
                        retry,
                        serde_json::to_string(&f)?
                    ],
                )?;
                tx.commit()?;
                return Err(f);
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn sync_conflicts(&self) -> Result<Vec<SyncConflict>> {
        read_conflicts(&self.providers.repository.db)
    }
    pub fn resolve_sync_conflict(
        &mut self,
        expected: SyncConflict,
        keep_local: bool,
    ) -> Result<()> {
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = read_conflicts(&tx)?
            .into_iter()
            .find(|c| c.id == expected.id)
            .ok_or_else(|| {
                error(
                    "conflict",
                    "This conflict has already changed. Refresh again.",
                )
            })?;
        if current != expected {
            return Err(error(
                "conflict",
                "The remote version changed. Review it again.",
            ));
        }
        // The current effective event includes any subsequent local edits, including deletion.
        let local: Option<String> = tx
            .query_row(
                "SELECT payload FROM events WHERE id=?1",
                [&current.event_id],
                |r| r.get(0),
            )
            .optional()?;
        let local: Option<CalendarEvent> = local.map(|v| serde_json::from_str(&v)).transpose()?;
        tx.execute("DELETE FROM conflicts WHERE id=?1", [&current.id])?;
        // Retain acknowledged identities for dependency/history recovery, retire all paused intent.
        tx.execute(
            "UPDATE outbox SET state='acknowledged' WHERE event_id=?1 AND state!='acknowledged'",
            [&current.event_id],
        )?;
        if keep_local {
            let before = current
                .remote
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            let after = local
                .map(|event| EventSeries {
                    event,
                    exceptions: vec![],
                })
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            if before.is_some() || after.is_some() {
                tx.execute("INSERT INTO outbox(id,account_id,calendar_id,event_id,operation,before_payload,after_payload,expected_revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![uuid::Uuid::new_v4().to_string(),current.account_id,current.calendar_id,current.event_id,if after.is_none(){"delete"}else if before.is_none(){"create"}else{"update"},before,after,current.remote_revision])?;
            }
        } else {
            tx.execute("DELETE FROM events WHERE id=?1", [&current.event_id])?;
            if let Some(s) = current.remote {
                crate::repository::Repository::write(&tx, &s.event)?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    /// Retry a reviewed permission/validation failure without dropping durable intent.
    pub fn retry_outgoing(&mut self, account: &str) -> Result<()> {
        self.account(account)?;
        self.providers.repository.db.execute("UPDATE outbox SET state='pending',retry_at=NULL WHERE account_id=?1 AND state IN ('pending','blocked') AND NOT EXISTS(SELECT 1 FROM conflicts WHERE event_id=outbox.event_id)",[account])?;
        Ok(())
    }
}

fn read_conflicts(db: &rusqlite::Connection) -> Result<Vec<SyncConflict>> {
    let mut q=db.prepare("SELECT id,account_id,calendar_id,event_id,(SELECT json_object('event',json(payload),'exceptions',json('[]')) FROM events WHERE id=conflicts.event_id),remote_payload,remote_revision FROM conflicts ORDER BY rowid")?;
    let rows = q
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(id, account_id, calendar_id, event_id, local, remote, remote_revision)| {
                Ok(SyncConflict {
                    id,
                    account_id,
                    calendar_id,
                    event_id,
                    local: local.map(|v| serde_json::from_str(&v)).transpose()?,
                    remote: remote.map(|v| serde_json::from_str(&v)).transpose()?,
                    remote_revision,
                })
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        providers::{LocalProvider, Provider, ProviderRegistry},
        repository::Repository,
    };
    use std::path::Path;
    struct Adapter;
    impl Provider for Adapter {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "fixture".into(),
                name: "Fixture".into(),
                connection_available: true,
                unavailable_reason: None,
                capabilities: self.capabilities(),
            }
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::ordinary()
        }
        fn event_capabilities(&self, _item: &IncomingItem) -> Capabilities {
            Capabilities::ordinary()
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
    fn open(path: &Path) -> CalendarService {
        let mut registry = ProviderRegistry::new(Repository::open(path).unwrap());
        registry.register(Box::new(LocalProvider)).unwrap();
        registry.register(Box::new(Adapter)).unwrap();
        CalendarService {
            providers: registry,
            discoveries: Default::default(),
            recorded_connections: Default::default(),
        }
    }
    fn setup(path: &Path) -> (CalendarService, SyncClaim, String) {
        let mut s = open(path);
        let account = s
            .providers
            .repository
            .upsert_account("fixture", "subject", "Fixture", ConnectionState::Connected)
            .unwrap();
        let discovered = DiscoveredCalendar {
            remote_id: "remote-calendar".into(),
            name: "Fixture".into(),
            color: "#718675".into(),
            description: "".into(),
            access_role: "writer".into(),
            eligible: true,
            disabled_reason: None,
            selected: true,
        };
        let a = s.account(&account).unwrap();
        let d = s.prepare_discovery(&a, vec![discovered.clone()]).unwrap();
        s.commit_selection(&d.id, vec![discovered.remote_id.clone()])
            .unwrap();
        let mut claim = s.begin_sync(&account, range()).unwrap().unwrap();
        let jobs = s.reconcile_sync(&mut claim, vec![discovered]).unwrap();
        (s, claim, jobs[0].id.clone())
    }
    fn event(calendar: &str) -> CalendarEvent {
        CalendarEvent {
            id: uuid::Uuid::new_v4().to_string(),
            calendar_id: calendar.into(),
            title: "Original".into(),
            location: "".into(),
            notes: "".into(),
            time: EventTime::AllDay {
                start_date: "2026-10-01".into(),
                end_date: "2026-10-02".into(),
            },
            recurrence: None,
            occurrence: None,
        }
    }
    fn ack(op: &OutgoingOperation, rev: &str) -> IncomingItem {
        IncomingItem {
            remote_id: op.remote_id.clone(),
            revision: rev.into(),
            shadow: "{}".into(),
            event: op.after.as_ref().map(|s| s.event.clone()),
            master: None,
            original_start: None,
            unsupported: false,
        }
    }
    fn sent(s: &mut CalendarService, c: &SyncClaim) -> OutgoingOperation {
        let op = s.claim_outgoing(c).unwrap().unwrap();
        let item = ack(&op, "revision-1");
        s.apply_outgoing(c, &op, Ok(OutgoingResult::Applied(Some(item))))
            .unwrap();
        op
    }
    #[test]
    fn offline_cli_intent_reopens_and_dependencies_use_ack_revision() {
        let path =
            std::env::temp_dir().join(format!("aika-outgoing-{}.sqlite3", uuid::Uuid::new_v4()));
        let (mut desktop, c, calendar) = setup(&path);
        let mut cli = open(&path);
        let original = cli.save(event(&calendar), false).unwrap();
        let mut changed = original.clone();
        changed.title = "Later local edit".into();
        cli.save(changed.clone(), true).unwrap();
        drop(cli);
        let first = desktop.claim_outgoing(&c).unwrap().unwrap();
        assert!(first.remote_id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(first.remote_id.len(), 32);
        let a = ack(&first, "ack-1");
        desktop
            .apply_outgoing(&c, &first, Ok(OutgoingResult::Applied(Some(a))))
            .unwrap();
        assert_eq!(
            desktop.providers.repository.get(&original.id).unwrap(),
            Some(changed)
        );
        let second = desktop.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(second.revision.as_deref(), Some("ack-1"));
        assert_eq!(second.remote_id, first.remote_id);
        let a = ack(&second, "ack-2");
        desktop
            .apply_outgoing(&c, &second, Ok(OutgoingResult::Applied(Some(a))))
            .unwrap();
        assert!(desktop.claim_outgoing(&c).unwrap().is_none());
        desktop.end_sync(&c, None).unwrap();
        drop(desktop);
        let reopened = open(&path);
        assert_eq!(
            reopened
                .providers
                .repository
                .get(&original.id)
                .unwrap()
                .unwrap()
                .title,
            "Later local edit"
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    fn import(s: &mut CalendarService, c: &SyncClaim, cal: &str, items: Vec<IncomingItem>) {
        let job = crate::sync::CalendarJob {
            id: cal.into(),
            remote_id: "remote-calendar".into(),
            cursor: None,
        };
        let generation = s.start_generation(c, &job, true).unwrap();
        s.stage_page(
            c,
            &generation,
            crate::sync::IncomingPage {
                items,
                next_page: None,
                cursor: Some("rebuilt".into()),
            },
            false,
        )
        .unwrap();
        s.stage_page(
            c,
            &generation,
            crate::sync::IncomingPage {
                items: vec![],
                next_page: None,
                cursor: None,
            },
            true,
        )
        .unwrap();
        s.publish_generation(c, &generation).unwrap();
    }
    #[test]
    fn incoming_rebuild_preserves_pending_intent_and_refreshes_conflict_remote_side() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let original = s.save(event(&cal), false).unwrap();
        let created = sent(&mut s, &c);
        let mut local = original.clone();
        local.title = "Pending local".into();
        s.save(local.clone(), true).unwrap();
        let mut remote = ack(&created, "external-1");
        remote.event.as_mut().unwrap().title = "External".into();
        import(&mut s, &c, &cal, vec![remote.clone()]);
        assert_eq!(
            s.providers.repository.get(&local.id).unwrap(),
            Some(local.clone())
        );
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(op.revision.as_deref(), Some("revision-1"));
        s.apply_outgoing(&c, &op, Ok(OutgoingResult::Conflict(Some(remote.clone()))))
            .unwrap();
        let stale = s.sync_conflicts().unwrap().remove(0);
        remote.revision = "external-2".into();
        remote.event.as_mut().unwrap().title = "New external".into();
        import(&mut s, &c, &cal, vec![remote]);
        assert_eq!(
            s.resolve_sync_conflict(stale, false).unwrap_err().code,
            "conflict"
        );
        let fresh = s.sync_conflicts().unwrap().remove(0);
        assert_eq!(fresh.remote.as_ref().unwrap().event.title, "New external");
        assert_eq!(fresh.local.as_ref().unwrap().event.title, "Pending local");
        // A full feed omission becomes a deletion conflict, retaining the local version.
        import(&mut s, &c, &cal, vec![]);
        let deleted = s.sync_conflicts().unwrap().remove(0);
        assert!(deleted.remote.is_none());
        assert_eq!(s.providers.repository.get(&local.id).unwrap(), Some(local));
        s.resolve_sync_conflict(deleted, true).unwrap();
        let restore = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(restore.operation, "create");
        assert_ne!(restore.remote_id, created.remote_id);
    }
    #[test]
    fn fresh_edit_uses_latest_incoming_revision_after_earlier_ack() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let e = s.save(event(&cal), false).unwrap();
        let created = sent(&mut s, &c);
        let mut remote = ack(&created, "new-external-revision");
        remote.event.as_mut().unwrap().location = "New remote location".into();
        import(&mut s, &c, &cal, vec![remote]);
        let mut next = s.providers.repository.get(&e.id).unwrap().unwrap();
        next.title = "Next local edit".into();
        s.save(next, true).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(op.revision.as_deref(), Some("new-external-revision"));
    }
    #[test]
    fn selection_retains_eligible_default_and_permission_loss_falls_back() {
        let (mut s, mut c, cal) = setup(Path::new(":memory:"));
        s.providers.set_default_calendar(&cal).unwrap();
        let mut discovered = DiscoveredCalendar {
            remote_id: "remote-calendar".into(),
            name: "Fixture".into(),
            color: "#718675".into(),
            description: "".into(),
            access_role: "writer".into(),
            eligible: true,
            disabled_reason: None,
            selected: true,
        };
        let d = s
            .prepare_discovery(&s.account(&c.account.id).unwrap(), vec![discovered.clone()])
            .unwrap();
        s.commit_selection(&d.id, vec![discovered.remote_id.clone()])
            .unwrap();
        assert_eq!(
            s.default_calendar_id().unwrap().as_deref(),
            Some(cal.as_str())
        );
        s.end_sync(&c, None).unwrap();
        c = s.begin_sync(&c.account.id, range()).unwrap().unwrap();
        discovered.access_role = "reader".into();
        s.reconcile_sync(&mut c, vec![discovered]).unwrap();
        assert_eq!(
            s.default_calendar_id().unwrap().as_deref(),
            Some("local-default")
        );
        assert_eq!(s.save(event(&cal), false).unwrap_err().code, "readOnly");
    }
    #[test]
    fn checked_discard_disconnect_restores_cache_and_fences_late_writes() {
        let path =
            std::env::temp_dir().join(format!("aika-discard-{}.sqlite", uuid::Uuid::new_v4()));
        let (mut s, c, cal) = setup(&path);
        let original = s.save(event(&cal), false).unwrap();
        sent(&mut s, &c);
        let mut edited = original.clone();
        edited.title = "Local pending".into();
        s.save_checked(edited.clone(), true, Some(original.clone()))
            .unwrap();
        let stale = s.review_disconnect(&c.account.id).unwrap();
        let mut other = open(&path);
        let mut newer = edited.clone();
        newer.title = "Newer pending".into();
        other
            .save_checked(newer.clone(), true, Some(edited))
            .unwrap();
        assert_eq!(
            s.discard_and_disconnect(stale).unwrap_err().code,
            "conflict"
        );
        assert_eq!(s.get_series(&original.id).unwrap().event, newer);
        // Simulate a request that has left the process; its result must not republish.
        let late = s.claim_outgoing(&c).unwrap().unwrap();
        let local_create = s.save(event(&cal), false).unwrap();
        let reviewed = s.review_disconnect(&c.account.id).unwrap();
        assert_eq!(reviewed.pending_count, 3);
        assert_eq!(reviewed.attempted_count, 1);
        s.discard_and_disconnect(reviewed).unwrap();
        assert_eq!(s.get_series(&original.id).unwrap().event, original);
        assert!(s
            .providers
            .repository
            .get(&local_create.id)
            .unwrap()
            .is_none());
        assert_eq!(
            s.account(&c.account.id).unwrap().connection_state,
            ConnectionState::Disconnected
        );
        assert_eq!(s.review_disconnect(&c.account.id).unwrap().pending_count, 0);
        assert_eq!(
            s.apply_outgoing(
                &c,
                &late,
                Ok(OutgoingResult::Applied(Some(ack(&late, "late"))))
            )
            .unwrap_err()
            .code,
            "syncSuperseded"
        );
        assert_eq!(s.save(event(&cal), false).unwrap_err().code, "readOnly");
        drop(other);
        drop(s);
        let mut restarted = open(&path);
        assert_eq!(restarted.get_series(&original.id).unwrap().event, original);
        assert_eq!(
            restarted
                .review_disconnect(&c.account.id)
                .unwrap()
                .pending_count,
            0
        );
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn discard_disconnect_restores_conflict_remote_and_retains_unrelated_undo() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let original = s.save(event(&cal), false).unwrap();
        sent(&mut s, &c);
        let mut edited = original.clone();
        edited.title = "Local".into();
        s.save_checked(edited, true, Some(original.clone()))
            .unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        let mut remote = ack(&op, "remote-2");
        remote.event.as_mut().unwrap().title = "Google".into();
        s.apply_outgoing(&c, &op, Ok(OutgoingResult::Conflict(Some(remote))))
            .unwrap();
        let local = s.save(event("local-default"), false).unwrap();
        let review = s.review_disconnect(&c.account.id).unwrap();
        assert_eq!(review.conflict_count, 1);
        s.discard_and_disconnect(review).unwrap();
        assert_eq!(s.get_series(&original.id).unwrap().event.title, "Google");
        assert!(s.sync_conflicts().unwrap().is_empty());
        s.undo().unwrap();
        assert!(s.providers.repository.get(&local.id).unwrap().is_none());
        assert_eq!(s.undo().unwrap_err().code, "nothingToUndo");
    }
    #[test]
    fn discard_disconnect_restores_pending_delete_and_accepts_remote_deletion() {
        for remote_deleted in [false, true] {
            let (mut s, c, cal) = setup(Path::new(":memory:"));
            let original = s.save(event(&cal), false).unwrap();
            sent(&mut s, &c);
            s.delete(&original.id).unwrap();
            if remote_deleted {
                let op = s.claim_outgoing(&c).unwrap().unwrap();
                s.apply_outgoing(&c, &op, Ok(OutgoingResult::Conflict(None)))
                    .unwrap();
            }
            let review = s.review_disconnect(&c.account.id).unwrap();
            s.discard_and_disconnect(review).unwrap();
            assert_eq!(
                s.providers.repository.get(&original.id).unwrap().is_none(),
                remote_deleted
            );
            assert!(s.sync_conflicts().unwrap().is_empty());
        }
    }
    #[test]
    fn discard_restores_exceptions_when_google_converts_a_pending_event_to_a_series() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let original = s.save(event(&cal), false).unwrap();
        let created = sent(&mut s, &c);
        let mut local = original.clone();
        local.title = "Local edit".into();
        s.save_checked(local, true, Some(original.clone())).unwrap();
        let mut master = ack(&created, "series-revision");
        master.event.as_mut().unwrap().recurrence = Some(Recurrence {
            frequency: RepeatFrequency::Daily,
            interval: 1,
            weekdays: vec![],
            monthly_on: MonthlyOn::Date,
            until: None,
            count: Some(3),
            segment: None,
        });
        let mut moved = master.clone();
        moved.remote_id = "moved".into();
        moved.master = Some(master.remote_id.clone());
        moved.original_start = Some("2026-10-02".into());
        moved.event.as_mut().unwrap().recurrence = None;
        moved.event.as_mut().unwrap().title = "Moved in Google".into();
        let mut cancelled = moved.clone();
        cancelled.remote_id = "cancelled".into();
        cancelled.original_start = Some("2026-10-03".into());
        cancelled.event = None;
        import(&mut s, &c, &cal, vec![cancelled, moved, master]);
        let review = s.review_disconnect(&c.account.id).unwrap();
        s.discard_and_disconnect(review).unwrap();
        let series = s.get_series(&original.id).unwrap();
        assert!(series.event.recurrence.is_some());
        assert_eq!(series.exceptions.len(), 2);
        assert_eq!(
            series.exceptions[0].event.as_ref().unwrap().title,
            "Moved in Google"
        );
        assert!(series.exceptions[1].event.is_none());
    }
    #[test]
    fn discard_disconnect_isolates_accounts_and_never_replays_on_reconnect() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let other_account = s
            .providers
            .repository
            .upsert_account(
                "fixture",
                "other-subject",
                "Other",
                ConnectionState::Connected,
            )
            .unwrap();
        let other = s.account(&other_account).unwrap();
        let d = s
            .prepare_discovery(
                &other,
                vec![DiscoveredCalendar {
                    remote_id: "remote-calendar".into(),
                    name: "Other".into(),
                    color: "#718675".into(),
                    description: "".into(),
                    access_role: "writer".into(),
                    eligible: true,
                    disabled_reason: None,
                    selected: true,
                }],
            )
            .unwrap();
        s.commit_selection(&d.id, vec!["remote-calendar".into()])
            .unwrap();
        let other_cal = s.account(&other_account).unwrap().selected_calendar_ids[0].clone();
        let other_event = s.save(event(&other_cal), false).unwrap();
        s.save(event(&cal), false).unwrap();
        let review = s.review_disconnect(&c.account.id).unwrap();
        s.discard_and_disconnect(review).unwrap();
        assert_eq!(
            s.review_disconnect(&other_account).unwrap().pending_count,
            1
        );
        assert_eq!(s.get_series(&other_event.id).unwrap().event, other_event);
        assert_eq!(
            s.account(&other_account).unwrap().connection_state,
            ConnectionState::Connected
        );
        s.providers
            .repository
            .upsert_account("fixture", "subject", "Fixture", ConnectionState::Connected)
            .unwrap();
        // Reconnection restores the account, but retired operations stay retired.
        assert_eq!(s.review_disconnect(&c.account.id).unwrap().pending_count, 0);
    }
    #[test]
    fn disconnect_requires_empty_outbox_and_disables_cached_calendar_writes() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let e = s.save(event(&cal), false).unwrap();
        s.providers.set_default_calendar(&cal).unwrap();
        assert_eq!(
            s.record_disconnect(&c.account.id).unwrap_err().code,
            "pendingWork"
        );
        assert_eq!(
            s.account(&c.account.id).unwrap().connection_state,
            ConnectionState::Connected
        );
        sent(&mut s, &c);
        s.record_disconnect(&c.account.id).unwrap();
        assert_eq!(
            s.default_calendar_id().unwrap().as_deref(),
            Some("local-default")
        );
        assert_eq!(s.save(event(&cal), false).unwrap_err().code, "readOnly");
        assert!(s.providers.repository.get(&e.id).unwrap().is_some());
    }
    #[test]
    fn undo_delete_after_incoming_tombstone_uses_creation_permissions_and_fresh_id() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let original = s.save(event(&cal), false).unwrap();
        let created = sent(&mut s, &c);
        s.delete(&original.id).unwrap();
        let deletion = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &deletion, Ok(OutgoingResult::Applied(None)))
            .unwrap();
        s.providers
            .repository
            .db
            .execute(
                "UPDATE remote_events SET shadow_payload=?2 WHERE id=?1",
                params![original.id, "{\"reminders\":{\"useDefault\":true}} "],
            )
            .unwrap();
        let mut tombstone = ack(&created, "cancelled-etag");
        tombstone.event = None;
        import(&mut s, &c, &cal, vec![tombstone]);
        s.providers
            .repository
            .db
            .execute(
                "UPDATE remote_events SET capabilities=?2 WHERE id=?1",
                params![
                    original.id,
                    serde_json::to_string(&Capabilities::read_only()).unwrap()
                ],
            )
            .unwrap();
        s.undo().unwrap();
        let restore = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(restore.operation, "create");
        assert!(restore.revision.is_none());
        assert_ne!(restore.remote_id, created.remote_id);
        assert!(restore.shadow.contains("reminders"));
    }
    #[test]
    fn undo_unsent_cancels_but_uncertain_send_queues_compensation() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let e = s.save(event(&cal), false).unwrap();
        s.undo().unwrap();
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        s.save(e.clone(), false).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        assert!(s
            .apply_outgoing(&c, &op, Err(error("network", "Offline")))
            .is_err());
        s.undo().unwrap();
        assert!(s.providers.repository.get(&e.id).unwrap().is_none());
        let count: i64 = s
            .providers
            .repository
            .db
            .query_row(
                "SELECT count(*) FROM outbox WHERE state!='acknowledged'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        // Retry deadline persists and fences automatic polling; simulate its expiry.
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        s.providers
            .repository
            .db
            .execute("UPDATE outbox SET retry_at=NULL", [])
            .unwrap();
        let retry = s.claim_outgoing(&c).unwrap().unwrap();
        assert!(retry.recovering);
        assert_eq!(retry.remote_id, op.remote_id);
        s.apply_outgoing(
            &c,
            &retry,
            Ok(OutgoingResult::Applied(Some(ack(&retry, "r1")))),
        )
        .unwrap();
        assert!(s.providers.repository.get(&e.id).unwrap().is_none());
        let compensation = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(compensation.operation, "delete");
        assert_eq!(compensation.revision.as_deref(), Some("r1"));
        s.apply_outgoing(&c, &compensation, Ok(OutgoingResult::Applied(None)))
            .unwrap();
        assert!(s.claim_outgoing(&c).unwrap().is_none());
    }
    #[test]
    fn undo_during_send_and_after_ack_preserves_dependencies() {
        for during in [true, false] {
            let (mut s, c, cal) = setup(Path::new(":memory:"));
            let e = s.save(event(&cal), false).unwrap();
            let op = s.claim_outgoing(&c).unwrap().unwrap();
            if during {
                s.undo().unwrap();
            }
            s.apply_outgoing(&c, &op, Ok(OutgoingResult::Applied(Some(ack(&op, "r1")))))
                .unwrap();
            if !during {
                s.undo().unwrap();
            }
            assert!(s.providers.repository.get(&e.id).unwrap().is_none());
            let undo = s.claim_outgoing(&c).unwrap().unwrap();
            assert_eq!(undo.operation, "delete");
            assert_eq!(undo.revision.as_deref(), Some("r1"));
        }
    }
    #[test]
    fn undo_acknowledged_delete_recreates_with_fresh_remote_identity() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let e = s.save(event(&cal), false).unwrap();
        let created = sent(&mut s, &c);
        s.delete(&e.id).unwrap();
        let deletion = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &deletion, Ok(OutgoingResult::Applied(None)))
            .unwrap();
        s.undo().unwrap();
        let restore = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(restore.operation, "create");
        assert_ne!(restore.remote_id, created.remote_id);
        assert!(restore.revision.is_none());
    }
    #[test]
    fn lease_recovery_reuses_identity_and_rejects_late_owner() {
        let path = std::env::temp_dir().join(format!(
            "aika-outgoing-crash-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let (mut s, c, cal) = setup(&path);
        s.save(event(&cal), false).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        drop(s); // Simulate closure with an unacknowledged in-flight request.
        let mut s = open(&path);
        s.providers
            .repository
            .db
            .execute("UPDATE account_leases SET expires_at='2000-01-01'", [])
            .unwrap();
        let next = s.begin_sync(&c.account.id, range()).unwrap().unwrap();
        let retry = s.claim_outgoing(&next).unwrap().unwrap();
        assert_eq!(retry.remote_id, op.remote_id);
        assert!(retry.recovering);
        assert_eq!(
            s.apply_outgoing(&c, &op, Ok(OutgoingResult::Applied(Some(ack(&op, "old")))))
                .unwrap_err()
                .code,
            "syncSuperseded"
        );
        s.apply_outgoing(
            &next,
            &retry,
            Ok(OutgoingResult::Applied(Some(ack(&retry, "new")))),
        )
        .unwrap();
        drop(s);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn conflicts_pause_one_event_and_checked_resolution_keeps_latest_intent() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let e = s.save(event(&cal), false).unwrap();
        sent(&mut s, &c);
        let mut changed = e.clone();
        changed.title = "Local".into();
        s.save(changed.clone(), true).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        let mut remote = ack(&op, "remote-revision");
        remote.event.as_mut().unwrap().title = "Google".into();
        s.apply_outgoing(&c, &op, Ok(OutgoingResult::Conflict(Some(remote))))
            .unwrap();
        let reviewed = s.sync_conflicts().unwrap().remove(0);
        assert_eq!(reviewed.remote.as_ref().unwrap().event.title, "Google");
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        s.save(event(&cal), false).unwrap();
        sent(&mut s, &c); // Another event continues.
        changed.title = "Newer local".into();
        s.save(changed.clone(), true).unwrap();
        assert_eq!(
            s.resolve_sync_conflict(reviewed, true).unwrap_err().code,
            "conflict"
        );
        let current = s.sync_conflicts().unwrap().remove(0);
        assert_eq!(current.local.as_ref().unwrap().event.title, "Newer local");
        s.resolve_sync_conflict(current, true).unwrap();
        let retry = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(retry.revision.as_deref(), Some("remote-revision"));
        assert_eq!(retry.after.as_ref().unwrap().event.title, "Newer local");
    }
    #[test]
    fn keep_google_handles_remote_edit_and_remote_deletion() {
        for deleted in [false, true] {
            let (mut s, c, cal) = setup(Path::new(":memory:"));
            let e = s.save(event(&cal), false).unwrap();
            sent(&mut s, &c);
            let mut local = e.clone();
            local.title = "Local".into();
            s.save(local, true).unwrap();
            let op = s.claim_outgoing(&c).unwrap().unwrap();
            let mut remote = ack(&op, "r2");
            remote.event.as_mut().unwrap().title = "Google".into();
            s.apply_outgoing(
                &c,
                &op,
                Ok(OutgoingResult::Conflict(if deleted {
                    None
                } else {
                    Some(remote)
                })),
            )
            .unwrap();
            let conflict = s.sync_conflicts().unwrap().remove(0);
            s.resolve_sync_conflict(conflict, false).unwrap();
            let effective = s.providers.repository.get(&e.id).unwrap();
            assert_eq!(
                effective.map(|e| e.title),
                if deleted { None } else { Some("Google".into()) }
            );
            assert!(s.claim_outgoing(&c).unwrap().is_none());
        }
    }
    #[test]
    fn permission_failure_remains_visible_and_retry_is_explicit() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        s.save(event(&cal), false).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        assert!(s
            .apply_outgoing(&c, &op, Err(error("permission", "Denied")))
            .is_err());
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        s.end_sync(&c, None).unwrap();
        assert_eq!(
            s.providers.repository.sync_status(&range()).unwrap()[0]
                .failure
                .as_ref()
                .unwrap()
                .code,
            "permission"
        );
        s.retry_outgoing(&c.account.id).unwrap();
        let next = s.begin_sync(&c.account.id, range()).unwrap().unwrap();
        let retry = s.claim_outgoing(&next).unwrap().unwrap();
        assert_eq!(retry.remote_id, op.remote_id);
    }
    #[test]
    fn permission_downgrade_blocks_http_and_authentication_pauses_account() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        s.save(event(&cal), false).unwrap();
        s.providers
            .repository
            .db
            .execute("UPDATE calendars SET writable=0 WHERE id=?1", [&cal])
            .unwrap();
        assert_eq!(s.claim_outgoing(&c).unwrap_err().code, "permission");
        s.providers
            .repository
            .db
            .execute("UPDATE calendars SET writable=1 WHERE id=?1", [&cal])
            .unwrap();
        s.retry_outgoing(&c.account.id).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        let f = error("authenticationRequired", "Reconnect");
        s.apply_outgoing(&c, &op, Err(f.clone())).unwrap_err();
        s.end_sync(&c, Some(f)).unwrap();
        assert!(s.begin_sync(&c.account.id, range()).unwrap().is_none());
    }
}
