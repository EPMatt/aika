//! Checked, durable deletion of local account data. No remote calendar/grant deletion.
use crate::{
    contracts::*,
    lifecycle,
    repository::Repository,
    service::{error, CalendarService},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
type Result<T> = std::result::Result<T, ServiceError>;
fn review(db: &Connection, account: &str) -> Result<AccountRemovalReview> {
    let work = crate::recovery::review(db, account)?;
    let removing = db.query_row(
        "SELECT removal_in_progress FROM accounts WHERE id=?1",
        [account],
        |r| r.get(0),
    )?;
    let (selected, excluded) = db.query_row("SELECT coalesce(sum(selected=1),0),coalesce(sum(selected=0),0) FROM calendars WHERE account_id=?1",[account],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let cached = db.query_row(
        "SELECT count(*) FROM (SELECT id FROM remote_events WHERE account_id=?1 UNION SELECT id FROM events WHERE calendar_id IN (SELECT id FROM calendars WHERE account_id=?1))",
        [account],
        |r| r.get(0),
    )?;
    let mut digest = Sha256::new();
    digest.update(work.revision.as_bytes());
    for sql in [
        "SELECT * FROM accounts WHERE id=?1",
        "SELECT * FROM calendars WHERE account_id=?1 ORDER BY id",
        "SELECT * FROM remote_events WHERE account_id=?1 ORDER BY id",
        "SELECT e.* FROM events e JOIN calendars c ON c.id=e.calendar_id WHERE c.account_id=?1 ORDER BY e.id",
        "SELECT x.* FROM event_exceptions x JOIN events e ON e.id=x.series_id JOIN calendars c ON c.id=e.calendar_id WHERE c.account_id=?1 ORDER BY x.series_id,x.original_start",
    ] {
        digest.update(sql.as_bytes());
        let mut q=db.prepare(sql)?;
        let columns=q.column_count();
        let mut rows=q.query([account])?;
        while let Some(row)=rows.next()? { for i in 0..columns {
            let value=format!("{:?}",row.get_ref(i)?);
            digest.update((value.len() as u64).to_le_bytes()); digest.update(value.as_bytes());
        }}
    }
    Ok(AccountRemovalReview {
        account_id: account.into(),
        selected_calendar_count: selected,
        excluded_calendar_count: excluded,
        cached_event_count: cached,
        pending_count: work.pending_count,
        conflict_count: work.conflict_count,
        attempted_count: work.attempted_count,
        removal_in_progress: removing,
        revision: format!("{:x}", digest.finalize()),
    })
}
impl CalendarService {
    pub fn review_account_removal(&mut self, account: &str) -> Result<AccountRemovalReview> {
        self.account(account)?;
        let tx = self.providers.repository.db.transaction()?;
        let result = review(&tx, account)?;
        tx.commit()?;
        Ok(result)
    }
    /// The explicit discard flag acknowledges uncertain writes; it never compensates them.
    pub fn remove_account(&mut self, expected: AccountRemovalReview, discard: bool) -> Result<()> {
        let account = self.account(&expected.account_id)?;
        let lifecycle = self.lifecycle.clone();
        {
            let _lock = lifecycle.as_ref().map(|l| l.lock()).transpose()?;
            let tx = self
                .providers
                .repository
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if review(&tx, &account.id)? != expected {
                return Err(error(
                    "conflict",
                    "Account data changed. Review removal again.",
                ));
            }
            if !discard && (expected.pending_count > 0 || expected.conflict_count > 0) {
                return Err(error(
                    "pendingWork",
                    "Resolve pending changes or explicitly discard them before removal.",
                ));
            }
            if !expected.removal_in_progress {
                tx.execute("UPDATE lifecycle_clock SET epoch=epoch+1", [])?;
                tx.execute("INSERT INTO identity_lifecycles(identity_hash,epoch,state) VALUES(?1,(SELECT epoch FROM lifecycle_clock),'removing') ON CONFLICT(identity_hash) DO UPDATE SET epoch=excluded.epoch,state=excluded.state",[lifecycle::identity(&account.provider_id,&account.remote_subject)])?;
                // The connection-state trigger increments the worker epoch, even if disconnected already.
                tx.execute("UPDATE accounts SET removal_in_progress=1,connection_state='\"disconnected\"',sync_failure=NULL WHERE id=?1",[&account.id])?;
                tx.execute(
                    "UPDATE calendars SET writable=0 WHERE account_id=?1",
                    [&account.id],
                )?;
                Repository::repair_default(&tx)?;
            }
            tx.commit()?;
        }
        self.discoveries
            .retain(|_, (d, _)| d.account.id != account.id);
        self.finish_account_removal(&account)
    }
    fn finish_account_removal(&mut self, account: &AccountSummary) -> Result<()> {
        // Hold the identity lock across store deletion AND database cleanup. Concurrent
        // retries must check the cleanup record before touching a newly signed-in identity.
        let lifecycle = self.lifecycle.clone();
        let _lock = lifecycle.as_ref().map(|l| l.lock()).transpose()?;
        let removing: Option<bool> = self
            .providers
            .repository
            .db
            .query_row(
                "SELECT removal_in_progress FROM accounts WHERE id=?1",
                [&account.id],
                |r| r.get(0),
            )
            .optional()?;
        if removing.is_none() {
            return Ok(());
        }
        if removing != Some(true) {
            return Err(error("conflict", "Account cleanup is no longer current."));
        }
        self.providers
            .adapter(&account.provider_id)?
            .remove_credentials(&account.remote_subject)?;
        let repo = &mut self.providers.repository;
        let tx = repo
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let affected = {
            let mut q=tx.prepare("SELECT id FROM events WHERE calendar_id IN (SELECT id FROM calendars WHERE account_id=?1) UNION SELECT event_id FROM outbox WHERE account_id=?1 UNION SELECT event_id FROM conflicts WHERE account_id=?1 UNION SELECT id FROM remote_events WHERE account_id=?1")?;
            let rows = q
                .query_map([&account.id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        tx.execute(
            "UPDATE outbox SET depends_on=NULL WHERE account_id=?1",
            [&account.id],
        )?;
        for table in [
            "conflicts",
            "outbox",
            "operation_groups",
            "remote_events",
            "account_leases",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE account_id=?1"),
                [&account.id],
            )?;
        }
        for table in ["sync_generations", "sync_cursors", "coverage", "events"] {
            tx.execute(&format!("DELETE FROM {table} WHERE calendar_id IN (SELECT id FROM calendars WHERE account_id=?1)"),[&account.id])?;
        }
        tx.execute("UPDATE event_exceptions SET payload=NULL,calendar_id=NULL WHERE calendar_id IN (SELECT id FROM calendars WHERE account_id=?1)",[&account.id])?;
        tx.execute("DELETE FROM calendars WHERE account_id=?1", [&account.id])?;
        tx.execute("DELETE FROM accounts WHERE id=?1", [&account.id])?;
        tx.execute("UPDATE lifecycle_clock SET epoch=epoch+1", [])?;
        tx.execute(
            "UPDATE identity_lifecycles SET state='removed',epoch=(SELECT epoch FROM lifecycle_clock) WHERE identity_hash=?1",
            [lifecycle::identity(
                &account.provider_id,
                &account.remote_subject,
            )],
        )?;
        Repository::repair_default(&tx)?;
        tx.commit()?;
        repo.forget_event_history(&affected);
        self.recorded_connections
            .retain(|_, (id, _)| id != &account.id);
        Ok(())
    }
    pub fn resume_account_removals(&mut self) -> Result<()> {
        let ids = {
            let mut q = self
                .providers
                .repository
                .db
                .prepare("SELECT id FROM accounts WHERE removal_in_progress=1")?;
            let rows = q
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        let mut failure = None;
        for id in ids {
            let account = self.account(&id)?;
            if let Err(e) = self.finish_account_removal(&account) {
                failure = Some(e);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{LocalProvider, Provider, ProviderRegistry};
    use std::{
        path::Path,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
    };
    struct Adapter {
        fail: Arc<AtomicBool>,
        deletes: Arc<AtomicUsize>,
    }
    impl Provider for Adapter {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "removal-test".into(),
                name: "Removal fixture".into(),
                connection_available: true,
                unavailable_reason: None,
                capabilities: self.capabilities(),
            }
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::ordinary()
        }
        fn remove_credentials(&self, _: &str) -> Result<()> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            if self.fail.load(Ordering::SeqCst) {
                Err(error("credentialUnavailable", "Unlock store."))
            } else {
                Ok(())
            }
        }
    }
    fn open(path: &Path, fail: Arc<AtomicBool>, deletes: Arc<AtomicUsize>) -> CalendarService {
        let mut providers = ProviderRegistry::new(Repository::open(path).unwrap());
        providers.register(Box::new(LocalProvider)).unwrap();
        providers
            .register(Box::new(Adapter { fail, deletes }))
            .unwrap();
        CalendarService {
            providers,
            lifecycle: Some(lifecycle::Lifecycle::new(path)),
            discoveries: Default::default(),
            recorded_connections: Default::default(),
        }
    }
    fn calendar(s: &mut CalendarService, subject: &str) -> (String, String) {
        let repo = &mut s.providers.repository;
        let account = repo
            .upsert_account("removal-test", subject, subject, ConnectionState::Connected)
            .unwrap();
        let cal = repo
            .import_calendar(
                &account,
                "calendar",
                CalendarDetails {
                    name: "Cache".into(),
                    color: "#718675".into(),
                    description: "".into(),
                },
                Capabilities::ordinary(),
            )
            .unwrap();
        (account, cal)
    }
    fn event(calendar: &str) -> CalendarEvent {
        CalendarEvent {
            id: uuid::Uuid::new_v4().to_string(),
            calendar_id: calendar.into(),
            title: "Pending".into(),
            location: "".into(),
            notes: "".into(),
            recurrence: None,
            occurrence: None,
            time: EventTime::AllDay {
                start_date: "2026-10-19".into(),
                end_date: "2026-10-20".into(),
            },
        }
    }
    #[test]
    fn checked_discard_store_failure_restart_isolation_and_complete_cleanup() {
        let path =
            std::env::temp_dir().join(format!("aika-removal-{}.sqlite3", uuid::Uuid::new_v4()));
        let fail = Arc::new(AtomicBool::new(true));
        let deletes = Arc::new(AtomicUsize::new(0));
        let mut s = open(&path, fail.clone(), deletes.clone());
        let (a, cal) = calendar(&mut s, "one");
        let (b, other) = calendar(&mut s, "two");
        let pending = s.providers.save(event(&cal), false).unwrap();
        let unrelated = s.providers.save(event(&other), false).unwrap();
        let db = &s.providers.repository.db;
        db.execute(
            "UPDATE accounts SET selection_confirmed=1 WHERE id=?1",
            [&a],
        )
        .unwrap();
        db.execute(
            "INSERT INTO remote_events VALUES(?1,?2,?3,'remote','revision','{}',?4)",
            rusqlite::params![
                pending.id,
                a,
                cal,
                serde_json::to_string(&Capabilities::ordinary()).unwrap()
            ],
        )
        .unwrap();
        db.execute("INSERT INTO incoming_items VALUES(?1,'{}')", [&pending.id])
            .unwrap();
        db.execute(
            "INSERT INTO incoming_projections VALUES(?1,'master')",
            [&pending.id],
        )
        .unwrap();
        db.execute(
            "INSERT INTO sync_generations VALUES('staging',?1,'{}',NULL,0)",
            [&cal],
        )
        .unwrap();
        db.execute("INSERT INTO sync_cursors VALUES(?1,'cursor',NULL)", [&cal])
            .unwrap();
        db.execute(
            "INSERT INTO coverage VALUES(?1,'start','end','staging')",
            [&cal],
        )
        .unwrap();
        db.execute(
            "INSERT INTO conflicts VALUES('conflict',?1,?2,?3,NULL,NULL,'revision')",
            rusqlite::params![a, cal, pending.id],
        )
        .unwrap();
        db.execute("INSERT INTO outbox(id,account_id,calendar_id,event_id,operation,group_id,step_index,attempts,state) SELECT 'ack-step',account_id,calendar_id,event_id,'update',group_id,1,1,'acknowledged' FROM outbox WHERE account_id=?1",[&a]).unwrap();
        let range = EventRange {
            start: "2026-10-01T00:00:00Z".into(),
            end: "2026-11-01T00:00:00Z".into(),
            start_date: "2026-10-01".into(),
            end_date: "2026-11-01".into(),
        };
        let claim = s.begin_sync(&a, range).unwrap().unwrap();
        s.providers.set_default_calendar(&cal).unwrap();
        let first = s.review_account_removal(&a).unwrap();
        assert_eq!(first.pending_count, 1);
        assert_eq!(first.attempted_count, 1);
        assert_eq!(first.conflict_count, 1);
        assert_eq!(first.cached_event_count, 1);
        assert_eq!(
            s.remove_account(first.clone(), false).unwrap_err().code,
            "pendingWork"
        );
        let mut second = open(&path, fail.clone(), deletes.clone());
        second
            .providers
            .set_calendar_visibility(&cal, false)
            .unwrap();
        assert_eq!(s.remove_account(first, true).unwrap_err().code, "conflict");
        let review = s.review_account_removal(&a).unwrap();
        assert_eq!(
            s.remove_account(review, true).unwrap_err().code,
            "credentialUnavailable"
        );
        assert!(s.review_account_removal(&a).unwrap().removal_in_progress);
        assert_eq!(s.account(&a).unwrap().removal_in_progress, Some(true));
        assert_eq!(s.check_sync(&claim).unwrap_err().code, "syncSuperseded");
        assert_eq!(
            s.account(&a).unwrap().connection_state,
            ConnectionState::Disconnected
        );
        assert!(s.providers.save(event(&cal), false).is_err());
        assert!(s
            .providers
            .repository
            .upsert_account("removal-test", "one", "late", ConnectionState::Connected)
            .is_err());
        let during_removal = s.lifecycle.as_ref().unwrap().epoch().unwrap();
        drop(s);
        let mut s = open(&path, fail.clone(), deletes.clone());
        assert!(s.resume_account_removals().is_err());
        fail.store(false, Ordering::SeqCst);
        s.resume_account_removals().unwrap();
        assert!(s.account(&a).is_err());
        assert!(s.account(&b).is_ok());
        assert!(lifecycle::check(
            &s.providers.repository.db,
            "removal-test",
            "one",
            Some(during_removal)
        )
        .is_err());
        let fresh_epoch = s.lifecycle.as_ref().unwrap().epoch().unwrap();
        assert!(lifecycle::check(
            &s.providers.repository.db,
            "removal-test",
            "one",
            Some(fresh_epoch)
        )
        .is_ok());
        assert_eq!(
            s.providers
                .repository
                .get_series(&unrelated.id)
                .unwrap()
                .event,
            unrelated
        );
        for table in [
            "incoming_items",
            "incoming_projections",
            "sync_generations",
            "sync_cursors",
            "coverage",
        ] {
            let count: i64 = s
                .providers
                .repository
                .db
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
        for table in [
            "calendars",
            "remote_events",
            "outbox",
            "operation_groups",
            "conflicts",
            "account_leases",
        ] {
            let count: i64 = s
                .providers
                .repository
                .db
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE account_id=?1"),
                    [&a],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
        assert_ne!(s.providers.default_calendar_id().unwrap(), Some(cal));
        assert!(s
            .providers
            .repository
            .db
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .optional()
            .unwrap()
            .is_none());
        assert_eq!(deletes.load(Ordering::SeqCst), 3);

        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn credential_deleted_then_crash_and_duplicate_cleanup_cannot_touch_fresh_signin() {
        let path = std::env::temp_dir().join(format!(
            "aika-removal-crash-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let fail = Arc::new(AtomicBool::new(true));
        let deletes = Arc::new(AtomicUsize::new(0));
        let mut s = open(&path, fail.clone(), deletes.clone());
        let (a, _) = calendar(&mut s, "one");
        let old = s.account(&a).unwrap();
        let review = s.review_account_removal(&a).unwrap();
        assert!(s.remove_account(review, false).is_err());
        // Successful store deletion with no DB cleanup models the crash boundary.
        fail.store(false, Ordering::SeqCst);
        s.providers
            .adapter("removal-test")
            .unwrap()
            .remove_credentials("one")
            .unwrap();
        drop(s);
        let mut s = open(&path, fail, deletes.clone());
        s.resume_account_removals().unwrap();
        let fresh = calendar(&mut s, "one").0;
        assert_ne!(fresh, a);
        let calls = deletes.load(Ordering::SeqCst);
        s.finish_account_removal(&old).unwrap();
        assert_eq!(deletes.load(Ordering::SeqCst), calls);
        assert!(s.account(&fresh).is_ok());
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn v6_pending_work_upgrade_is_transactional_and_preserves_intent() {
        let path =
            std::env::temp_dir().join(format!("aika-v7-upgrade-{}.sqlite3", uuid::Uuid::new_v4()));
        let fail = Arc::new(AtomicBool::new(false));
        let deletes = Arc::new(AtomicUsize::new(0));
        let mut s = open(&path, fail.clone(), deletes.clone());
        let (_, cal) = calendar(&mut s, "one");
        let event = s.providers.save(event(&cal), false).unwrap();
        s.providers.repository.db.execute_batch("UPDATE outbox SET attempts=1,state='inFlight'; ALTER TABLE outbox DROP COLUMN meeting_intent; DROP TRIGGER fence_removing_account; ALTER TABLE accounts DROP COLUMN removal_in_progress; DROP TABLE identity_lifecycles; DROP TABLE lifecycle_clock; PRAGMA user_version=6; CREATE TABLE identity_lifecycles(marker TEXT); INSERT INTO identity_lifecycles VALUES('preserve');").unwrap();
        drop(s);
        assert!(Repository::open(&path).is_err());
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            6
        );
        assert_eq!(
            db.query_row("SELECT marker FROM identity_lifecycles", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "preserve"
        );
        db.execute_batch("DROP TABLE identity_lifecycles").unwrap();
        drop(db);
        let s = open(&path, fail, deletes);
        assert_eq!(
            s.providers.repository.get_series(&event.id).unwrap().event,
            event
        );
        let (attempts, state, resource): (i64, String, String) = s
            .providers
            .repository
            .db
            .query_row("SELECT attempts,state,resource_id FROM outbox", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap();
        assert_eq!(attempts, 1);
        assert_eq!(state, "inFlight");
        assert!(!resource.is_empty());
        std::fs::remove_file(path).unwrap();
    }
}
