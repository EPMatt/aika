//! Explicit, checked retirement of local intent for calendar or account recovery.
use crate::{
    contracts::*,
    repository::Repository,
    service::{error, CalendarService},
    sync::IncomingItem,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

type Result<T> = std::result::Result<T, ServiceError>;

fn calendar_review(db: &Connection, calendar: &str) -> Result<CalendarRemovalReview> {
    let account: Option<String> = db.query_row(
        "SELECT account_id FROM calendars WHERE id=?1 AND selected=1 AND account_id IS NOT NULL",
        [calendar], |r| r.get(0),
    ).optional()?;
    let account = account
        .ok_or_else(|| error("notFound", "This imported calendar is no longer included."))?;
    let mut digest = Sha256::new();
    for sql in [
        "SELECT c.*,a.sync_epoch,a.connection_state FROM calendars c JOIN accounts a ON a.id=c.account_id WHERE c.id=?1",
        "SELECT o.*,e.payload,i.payload FROM outbox o LEFT JOIN events e ON e.id=o.event_id LEFT JOIN incoming_items i ON i.id=o.event_id WHERE o.group_id IN (SELECT group_id FROM active_group_series WHERE calendar_id=?1) ORDER BY o.id",
        "SELECT * FROM active_group_series WHERE calendar_id=?1 ORDER BY group_id,event_id",
        "SELECT f.*,e.payload FROM conflicts f LEFT JOIN events e ON e.id=f.event_id WHERE f.calendar_id=?1 ORDER BY f.id",
        "SELECT i.id,i.payload FROM incoming_items i JOIN remote_events r ON r.id=i.id WHERE r.calendar_id=?1 ORDER BY i.id",
    ] {
        let mut statement = db.prepare(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query([calendar])?;
        digest.update(sql.as_bytes());
        while let Some(row) = rows.next()? {
            for index in 0..columns {
                let value = format!("{:?}", row.get_ref(index)?);
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            }
        }
    }
    let (pending_count, attempted_count) = db.query_row(
        "SELECT count(DISTINCT a.group_id),count(DISTINCT CASE WHEN EXISTS(SELECT 1 FROM outbox o WHERE o.group_id=a.group_id AND (o.attempts>0 OR o.state='inFlight')) THEN a.group_id END) FROM active_group_series a WHERE a.calendar_id=?1",
        [calendar], |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let conflict_count = db.query_row(
        "SELECT count(*) FROM conflicts WHERE calendar_id=?1",
        [calendar],
        |r| r.get(0),
    )?;
    Ok(CalendarRemovalReview {
        calendar_id: calendar.into(),
        account_id: account,
        pending_count,
        conflict_count,
        attempted_count,
        revision: format!("{:x}", digest.finalize()),
    })
}

fn review(db: &Connection, account: &str) -> Result<DisconnectReview> {
    let mut digest = Sha256::new();
    // Include effective local data and incoming revisions as well as operation identities.
    // A second process editing/retrying/synchronizing invalidates the reviewed snapshot.
    for sql in [
        "SELECT sync_epoch,connection_state FROM accounts WHERE id=?1",
        "SELECT o.*,e.payload,i.payload FROM outbox o LEFT JOIN events e ON e.id=o.event_id LEFT JOIN incoming_items i ON i.id=o.event_id WHERE o.group_id IN (SELECT group_id FROM active_group_series WHERE account_id=?1) ORDER BY o.id",
        "SELECT * FROM active_group_series WHERE account_id=?1 ORDER BY group_id,event_id",
        "SELECT f.*,e.payload FROM conflicts f LEFT JOIN events e ON e.id=f.event_id WHERE f.account_id=?1 ORDER BY f.id",
        "SELECT i.id,i.payload FROM incoming_items i JOIN remote_events r ON r.id=i.id WHERE r.account_id=?1 ORDER BY i.id",
    ] {
        let mut statement = db.prepare(sql)?;
        let columns = statement.column_count();
        let mut rows = statement.query([account])?;
        digest.update(sql.as_bytes());
        while let Some(row) = rows.next()? {
            for index in 0..columns {
                // Debug formats each SQLite value with its type and boundaries; never exposed.
                let value = format!("{:?}", row.get_ref(index)?);
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            }
        }
    }
    let (pending_count, attempted_count) = db.query_row(
        "SELECT count(DISTINCT a.group_id),count(DISTINCT CASE WHEN EXISTS(SELECT 1 FROM outbox o WHERE o.group_id=a.group_id AND (o.attempts>0 OR o.state='inFlight')) THEN a.group_id END) FROM active_group_series a WHERE a.account_id=?1",
        [account], |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let conflict_count = db.query_row(
        "SELECT count(*) FROM conflicts WHERE account_id=?1",
        [account],
        |r| r.get(0),
    )?;
    Ok(DisconnectReview {
        account_id: account.into(),
        pending_count,
        conflict_count,
        attempted_count,
        revision: format!("{:x}", digest.finalize()),
    })
}

impl CalendarService {
    pub fn review_calendar_removal(&mut self, calendar: &str) -> Result<CalendarRemovalReview> {
        let tx = self.providers.repository.db.transaction()?;
        let result = calendar_review(&tx, calendar)?;
        tx.commit()?;
        Ok(result)
    }

    /// Retire only this calendar's intent and fence the account's current worker.
    /// Requests already sent may still succeed; this never sends compensating HTTP.
    pub fn discard_and_remove_calendar(&mut self, expected: CalendarRemovalReview) -> Result<()> {
        let repo = &mut self.providers.repository;
        let tx = repo
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if calendar_review(&tx, &expected.calendar_id)? != expected {
            return Err(error(
                "conflict",
                "Pending changes changed. Review the calendar removal again.",
            ));
        }
        let calendar = &expected.calendar_id;
        let affected = {
            let mut q = tx.prepare("SELECT id FROM events WHERE calendar_id=?1 UNION SELECT event_id FROM outbox WHERE calendar_id=?1 UNION SELECT event_id FROM conflicts WHERE calendar_id=?1")?;
            let rows = q
                .query_map([calendar], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        tx.execute("DELETE FROM conflicts WHERE calendar_id=?1", [calendar])?;
        // Keep dependency identities so unrelated work retains its durable history.
        tx.execute("UPDATE outbox SET state='acknowledged',retry_at=NULL,acknowledgement=NULL WHERE calendar_id=?1 AND state!='acknowledged'", [calendar])?;
        tx.execute(
            "UPDATE event_exceptions SET payload=NULL,calendar_id=NULL WHERE calendar_id=?1",
            [calendar],
        )?;
        tx.execute("DELETE FROM events WHERE calendar_id=?1", [calendar])?;
        tx.execute("DELETE FROM sync_cursors WHERE calendar_id=?1", [calendar])?;
        tx.execute("DELETE FROM coverage WHERE calendar_id=?1", [calendar])?;
        // The selected change triggers an epoch increment, fencing incoming and outgoing
        // publication for the old worker, including any already in-flight request.
        tx.execute(
            "UPDATE calendars SET selected=0,writable=0 WHERE id=?1",
            [calendar],
        )?;
        Repository::repair_default(&tx)?;
        tx.commit()?;
        repo.forget_event_history(&affected);
        self.discoveries
            .retain(|_, (d, _)| d.account.id != expected.account_id);
        Ok(())
    }

    pub fn review_disconnect(&mut self, account: &str) -> Result<DisconnectReview> {
        self.account(account)?;
        let tx = self.providers.repository.db.transaction()?;
        let result = review(&tx, account)?;
        tx.commit()?;
        Ok(result)
    }

    /// Local discard never sends compensating requests. Already sent writes may still land.
    /// Credentials are removed by the caller only after this transaction fences workers.
    pub fn discard_and_disconnect(&mut self, expected: DisconnectReview) -> Result<()> {
        self.account(&expected.account_id)?;
        let repo = &mut self.providers.repository;
        let tx = repo
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if review(&tx, &expected.account_id)? != expected {
            return Err(error(
                "conflict",
                "Pending changes changed. Review the disconnect again.",
            ));
        }
        let affected = {
            let mut q = tx.prepare("SELECT event_id FROM active_group_series WHERE account_id=?1 UNION SELECT event_id FROM conflicts WHERE account_id=?1")?;
            let rows = q
                .query_map([&expected.account_id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for id in &affected {
            restore_incoming(&tx, id)?;
        }
        tx.execute(
            "DELETE FROM conflicts WHERE account_id=?1",
            [&expected.account_id],
        )?;
        // Retain durable identities/history, but never replay discarded work on reconnect.
        tx.execute("UPDATE outbox SET state='acknowledged',retry_at=NULL,acknowledgement=NULL WHERE account_id=?1 AND state!='acknowledged'", [&expected.account_id])?;
        tx.execute(
            "UPDATE calendars SET writable=0 WHERE account_id=?1",
            [&expected.account_id],
        )?;
        tx.execute(
            "UPDATE accounts SET connection_state=?2,sync_failure=NULL WHERE id=?1",
            params![
                expected.account_id,
                serde_json::to_string(&ConnectionState::Disconnected)?
            ],
        )?;
        Repository::repair_default(&tx)?;
        tx.commit()?;
        repo.forget_event_history(&affected);
        self.discoveries
            .retain(|_, (d, _)| d.account.id != expected.account_id);
        Ok(())
    }
}

pub(crate) fn observed_series(tx: &Connection, id: &str) -> Result<Option<EventSeries>> {
    let payload: Option<String> = tx.query_row(
        "SELECT (SELECT payload FROM incoming_items WHERE id=?1)",
        [id],
        |r| r.get(0),
    )?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let item: IncomingItem = serde_json::from_str(&payload)?;
    let Some(mut event) = item
        .event
        .filter(|_| !item.unsupported && item.master.is_none())
    else {
        return Ok(None);
    };
    let calendar: String = tx.query_row(
        "SELECT calendar_id FROM remote_events WHERE id=?1",
        [id],
        |r| r.get(0),
    )?;
    event.id = id.into();
    event.calendar_id = calendar.clone();
    let mut result = EventSeries {
        event,
        exceptions: vec![],
    };
    if result.event.recurrence.is_some() {
        let mut q = tx.prepare("SELECT i.payload FROM incoming_items i JOIN remote_events r ON r.id=i.id WHERE r.calendar_id=?1 AND json_extract(i.payload,'$.master')=?2 ORDER BY i.id")?;
        let items = q
            .query_map(params![calendar, item.remote_id], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for payload in items {
            let exception: IncomingItem = serde_json::from_str(&payload)?;
            if let Some(original_start) = exception.original_start {
                // Trims invalidate cached future exceptions. Preserve only keys
                // that still belong to the observed master schedule.
                if aika_domain::recurrence::get_occurrence(
                    &EventSeries {
                        event: result.event.clone(),
                        exceptions: vec![],
                    },
                    &original_start,
                )
                .is_err()
                {
                    continue;
                }
                let event = exception.event.map(|mut e| {
                    e.id = id.into();
                    e.calendar_id = calendar.clone();
                    e
                });
                result.exceptions.push(EventException {
                    original_start,
                    event,
                });
            }
        }
        result
            .exceptions
            .sort_by(|a, b| a.original_start.cmp(&b.original_start));
    }
    Ok(Some(result))
}
pub(crate) fn restore_incoming(tx: &Connection, id: &str) -> Result<()> {
    let observed = observed_series(tx, id)?;
    tx.execute("DELETE FROM events WHERE id=?1", [id])?;
    if let Some(series) = observed {
        Repository::write_series(tx, &series)?;
    }
    Ok(())
}

/// Covers acknowledged siblings too: partial progress or a changed incoming shadow
/// invalidates conflict review even if the conflicting resource itself is unchanged.
pub(crate) fn group_revision(db: &Connection, event: &str) -> Result<String> {
    let mut digest = Sha256::new();
    for sql in [
        "SELECT o.* FROM outbox o WHERE group_id IN (WITH RECURSIVE affected(group_id) AS (SELECT group_id FROM active_group_series WHERE event_id=?1 UNION SELECT b.group_id FROM active_group_series a JOIN active_group_series b ON a.event_id=b.event_id JOIN affected g ON g.group_id=a.group_id) SELECT group_id FROM affected) ORDER BY o.id",
        "SELECT m.*,e.payload,i.payload FROM active_group_series m LEFT JOIN events e ON e.id=m.event_id LEFT JOIN incoming_items i ON i.id=m.event_id WHERE m.group_id IN (WITH RECURSIVE affected(group_id) AS (SELECT group_id FROM active_group_series WHERE event_id=?1 UNION SELECT b.group_id FROM active_group_series a JOIN active_group_series b ON a.event_id=b.event_id JOIN affected g ON g.group_id=a.group_id) SELECT group_id FROM affected) ORDER BY m.group_id,m.event_id",
        "SELECT x.* FROM event_exceptions x WHERE series_id IN (SELECT event_id FROM active_group_series WHERE group_id IN (WITH RECURSIVE affected(group_id) AS (SELECT group_id FROM active_group_series WHERE event_id=?1 UNION SELECT b.group_id FROM active_group_series a JOIN active_group_series b ON a.event_id=b.event_id JOIN affected g ON g.group_id=a.group_id) SELECT group_id FROM affected)) ORDER BY series_id,original_start",
        "SELECT i.id,i.payload FROM incoming_items i JOIN remote_events r ON json_extract(i.payload,'$.master')=r.remote_id JOIN active_group_series a ON a.event_id=r.id WHERE a.group_id IN (WITH RECURSIVE affected(group_id) AS (SELECT group_id FROM active_group_series WHERE event_id=?1 UNION SELECT b.group_id FROM active_group_series a JOIN active_group_series b ON a.event_id=b.event_id JOIN affected g ON g.group_id=a.group_id) SELECT group_id FROM affected) ORDER BY i.id",
    ] {
        let mut q = db.prepare(sql)?;
        let columns = q.column_count();
        let mut rows = q.query([event])?;
        digest.update(sql.as_bytes());
        while let Some(row) = rows.next()? {
            for i in 0..columns {
                let value = format!("{:?}", row.get_ref(i)?);
                digest.update((value.len() as u64).to_le_bytes());
                digest.update(value.as_bytes());
            }
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub(crate) fn affected_groups(db: &Connection, event: &str) -> Result<Vec<String>> {
    let mut q = db.prepare(
        "WITH RECURSIVE affected(group_id) AS (
        SELECT group_id FROM active_group_series WHERE event_id=?1
        UNION
        SELECT b.group_id FROM active_group_series a
        JOIN active_group_series b ON a.event_id=b.event_id
        JOIN affected g ON g.group_id=a.group_id
    ) SELECT group_id FROM affected ORDER BY group_id",
    )?;
    let rows = q
        .query_map([event], |r| r.get(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
