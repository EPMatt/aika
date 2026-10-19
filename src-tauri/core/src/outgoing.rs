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
    pub original_start: Option<String>,
    pub series_remote_id: Option<String>,
}
pub enum OutgoingResult {
    Applied(Option<IncomingItem>),
    Conflict(Option<IncomingItem>),
}
pub(crate) fn occurrence_event(
    series: &EventSeries,
    original: &str,
) -> Result<Option<CalendarEvent>> {
    if let Some(exception) = series
        .exceptions
        .iter()
        .find(|e| e.original_start == original)
    {
        return Ok(exception.event.clone());
    }
    let mut event = aika_domain::recurrence::get_occurrence(series, original)?;
    event.recurrence = None;
    event.occurrence = None;
    Ok(Some(event))
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
        let row = tx.query_row("SELECT o.id,o.event_id,o.calendar_id,c.remote_id,o.operation,o.before_payload,o.after_payload,o.expected_revision,o.attempts,o.depends_on FROM outbox o JOIN calendars c ON c.id=o.calendar_id WHERE o.account_id=?1 AND o.state IN ('pending','inFlight') AND (o.retry_at IS NULL OR julianday(o.retry_at)<=julianday('now')) AND c.active=1 AND c.selected=1 AND NOT EXISTS(SELECT 1 FROM outbox b WHERE b.group_id=o.group_id AND b.state='blocked') AND NOT EXISTS(SELECT 1 FROM conflicts f JOIN operation_group_series m ON m.event_id=f.event_id WHERE m.group_id=o.group_id) AND NOT EXISTS(SELECT 1 FROM outbox p WHERE p.group_id=o.group_id AND p.step_index<o.step_index AND p.state!='acknowledged') AND NOT EXISTS(SELECT 1 FROM active_group_series a JOIN operation_group_series m ON m.event_id=a.event_id JOIN operation_groups older ON older.id=a.group_id JOIN operation_groups own ON own.id=o.group_id WHERE m.group_id=o.group_id AND older.rowid<own.rowid) AND (o.depends_on IS NULL OR EXISTS(SELECT 1 FROM outbox d WHERE d.id=o.depends_on AND d.state='acknowledged')) ORDER BY o.rowid LIMIT 1", [&claim.account.id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,Option<String>>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,u32>(8)?,r.get::<_,Option<String>>(9)?))).optional()?;
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
                [&dependency],
                |r| r.get(0),
            )?;
            let same_resource_kind: bool = tx.query_row("SELECT (SELECT original_start IS NULL FROM outbox WHERE id=?1) AND original_start IS NULL FROM outbox WHERE id=?2", params![id,dependency], |r| r.get(0))?;
            if let Some(ack) = ack.filter(|_| same_resource_kind) {
                let ack: serde_json::Value = serde_json::from_str(&ack)?;
                revision = ack["revision"].as_str().map(str::to_owned);
            }
        }
        if let Some(predecessor) = revision
            .as_deref()
            .and_then(|r| r.strip_prefix("aikaPredecessor:"))
        {
            let ack: Option<String> = tx.query_row("SELECT acknowledgement FROM outbox WHERE id=?1 AND event_id=?2 AND state='acknowledged' AND original_start IS NULL", params![predecessor,event_id], |r| r.get(0)).optional()?.flatten();
            revision = ack
                .map(|a| serde_json::from_str::<serde_json::Value>(&a))
                .transpose()?
                .and_then(|a| a["revision"].as_str().map(str::to_owned));
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
        let (original_start, mut series_remote_id, resource): (
            Option<String>,
            Option<String>,
            Option<String>,
        ) = tx.query_row(
            "SELECT original_start,series_remote_id,resource_id FROM outbox WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if let Some(original) = &original_start {
            let before_series = before
                .as_deref()
                .map(serde_json::from_str::<EventSeries>)
                .transpose()?;
            let after_series = after
                .as_deref()
                .map(serde_json::from_str::<EventSeries>)
                .transpose()?;
            if operation != "update"
                || !matches!((&before_series, &after_series), (Some(b),Some(a)) if b.event == a.event && b.event.recurrence.is_some())
            {
                let failure = error(
                    "unsupported",
                    "Occurrence delivery requires a checked recurring aggregate update.",
                );
                tx.execute(
                    "UPDATE outbox SET state='blocked',acknowledgement=?2 WHERE id=?1",
                    params![id, serde_json::to_string(&failure)?],
                )?;
                tx.commit()?;
                return Err(failure);
            }
            if series_remote_id.is_none() {
                series_remote_id = existing.as_ref().map(|(id, _)| id.clone());
            }
            if resource.is_none() {
                let master = series_remote_id
                    .as_ref()
                    .ok_or_else(|| {
                        error(
                            "providerIdentity",
                            "The recurring master has no provider identity.",
                        )
                    })?
                    .clone();
                // Resolve with no SQLite transaction held. A fresh fence and row check
                // reject a discarded operation or replaced worker before binding.
                tx.commit()?;
                let resolved = claim
                    .adapter
                    .resolve_outgoing_instance(
                        &claim.account.remote_subject,
                        &remote_calendar,
                        &master,
                        original,
                    )
                    .and_then(|item| {
                        if item.master.as_deref() != Some(&master)
                            || item.original_start.as_deref() != Some(original)
                            || item.remote_id.is_empty()
                            || item.revision.is_empty()
                        {
                            Err(error(
                                "providerIdentity",
                                "Google returned an invalid occurrence identity or revision.",
                            ))
                        } else {
                            Ok(item)
                        }
                    });
                let tx = self
                    .providers
                    .repository
                    .db
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                guard(&tx, claim)?;
                let unchanged: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE id=?1 AND resource_id IS NULL AND state IN ('pending','inFlight'))", [&id], |r| r.get(0))?;
                if !unchanged {
                    return Err(error(
                        "syncSuperseded",
                        "The occurrence operation changed during lookup.",
                    ));
                }
                match resolved {
                    Ok(item) => {
                        let before: EventSeries =
                            serde_json::from_str(before.as_deref().ok_or_else(|| {
                                error("validation", "Occurrence intent requires a before state.")
                            })?)?;
                        let expected = occurrence_event(&before, original)?;
                        let same = match (&expected, &item.event) {
                            (Some(a), Some(b)) => {
                                a.title == b.title
                                    && a.time == b.time
                                    && a.location == b.location
                                    && a.notes == b.notes
                            }
                            (None, None) => true,
                            _ => false,
                        };
                        tx.execute("UPDATE outbox SET resource_id=?2,series_remote_id=?3,expected_revision=?4 WHERE id=?1", params![id,item.remote_id,master,if same {item.revision.as_str()} else {"unobserved-conflict"}])?;
                        let mapped = mapping(
                            &tx,
                            &claim.account.id,
                            &calendar_id,
                            &item,
                            claim.adapter.event_capabilities(&item),
                        )?;
                        tx.execute("INSERT INTO incoming_items(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", params![mapped,serde_json::to_string(&item)?])?;
                    }
                    Err(f) => {
                        if !matches!(
                            f.code.as_str(),
                            "network"
                                | "retryable"
                                | "authenticationRequired"
                                | "credentialUnavailable"
                        ) {
                            tx.execute(
                                "UPDATE outbox SET state='blocked',acknowledgement=?2 WHERE id=?1",
                                params![id, serde_json::to_string(&f)?],
                            )?;
                        }
                        tx.commit()?;
                        return Err(f);
                    }
                }
                tx.commit()?;
                return self.claim_outgoing(claim);
            }
            // Instance revisions are independent of master/predecessor revisions.
            revision = tx.query_row(
                "SELECT expected_revision FROM outbox WHERE id=?1",
                [&id],
                |r| r.get(0),
            )?;
        }
        let instance_shadow: Option<String> = if original_start.is_some() {
            tx.query_row("SELECT shadow_payload FROM remote_events WHERE account_id=?1 AND calendar_id=?2 AND remote_id=?3", params![claim.account.id,calendar_id,resource], |r| r.get(0)).optional()?
        } else {
            None
        };
        let shadow = instance_shadow
            .or_else(|| existing.as_ref().map(|(_, shadow)| shadow.clone()))
            .unwrap_or_else(|| "{}".into());
        // A compensation after a delete uses a fresh persisted ID: Google retains tombstones.
        let persisted: Option<String> =
            tx.query_row("SELECT resource_id FROM outbox WHERE id=?1", [&id], |r| {
                r.get(0)
            })?;
        let remote_id = if let Some(remote) = persisted {
            if operation == "create" && attempts == 0 {
                tx.execute("INSERT INTO remote_events(id,account_id,calendar_id,remote_id,revision,shadow_payload,capabilities) VALUES(?1,?2,?3,?4,NULL,?6,?5) ON CONFLICT(id) DO UPDATE SET remote_id=excluded.remote_id,revision=NULL,capabilities=excluded.capabilities",params![event_id,claim.account.id,calendar_id,remote,serde_json::to_string(&Capabilities::ordinary())?,shadow])?;
                tx.execute("DELETE FROM incoming_items WHERE id=?1", [&event_id])?;
            }
            remote
        } else {
            match existing {
                Some((remote, _)) if !(operation == "create" && attempts == 0) => remote,
                _ => {
                    let remote = uuid::Uuid::new_v4().simple().to_string();
                    tx.execute("INSERT INTO remote_events(id,account_id,calendar_id,remote_id,revision,shadow_payload,capabilities) VALUES(?1,?2,?3,?4,NULL,?6,?5) ON CONFLICT(id) DO UPDATE SET remote_id=excluded.remote_id,revision=NULL,capabilities=excluded.capabilities",params![event_id,claim.account.id,calendar_id,remote,serde_json::to_string(&Capabilities::ordinary())?,shadow])?;
                    tx.execute("DELETE FROM incoming_items WHERE id=?1", [&event_id])?;
                    remote
                }
            }
        };
        tx.execute("UPDATE outbox SET state='inFlight',attempts=attempts+1,expected_revision=?2,resource_id=?3 WHERE id=?1",params![id,revision,remote_id])?;
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
            original_start,
            series_remote_id,
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
                    let mapped = mapping(
                        &tx,
                        &claim.account.id,
                        &op.calendar_id,
                        item,
                        claim.adapter.event_capabilities(item),
                    )?;
                    tx.execute("INSERT INTO incoming_items(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![mapped,serde_json::to_string(item)?])?;
                } else if let Some(original) = &op.original_start {
                    let tombstone = IncomingItem {
                        remote_id: op.remote_id.clone(),
                        revision: String::new(),
                        shadow: "{}".into(),
                        event: None,
                        master: op.series_remote_id.clone(),
                        original_start: Some(original.clone()),
                        unsupported: false,
                    };
                    let mapped = mapping(
                        &tx,
                        &claim.account.id,
                        &op.calendar_id,
                        &tombstone,
                        Capabilities::read_only(),
                    )?;
                    tx.execute("INSERT INTO incoming_items(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![mapped,serde_json::to_string(&tombstone)?])?;
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
                    let remote = crate::recovery::observed_series(&tx, &op.event_id)?;
                    tx.execute("INSERT INTO conflicts(id,account_id,calendar_id,event_id,local_payload,remote_payload,remote_revision) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![uuid::Uuid::new_v4().to_string(),claim.account.id,op.calendar_id,op.event_id,op.after.as_ref().map(serde_json::to_string).transpose()?,remote.as_ref().map(serde_json::to_string).transpose()?,revision])?;
                    tx.execute("UPDATE outbox SET state='blocked',retry_at=NULL,acknowledgement=NULL WHERE id=?1", [&op.id])?;
                } else {
                    tx.execute("UPDATE outbox SET state='acknowledged',retry_at=NULL,acknowledgement=?2 WHERE id=?1",params![op.id,serde_json::to_string(&serde_json::json!({"revision":revision}))?])?;
                    // Release the complete overlay only after every step is acknowledged.
                    let members = {
                        let mut q = tx.prepare("SELECT event_id FROM operation_group_series WHERE group_id=(SELECT group_id FROM outbox WHERE id=?1)")?;
                        let rows = q
                            .query_map([&op.id], |r| r.get::<_, String>(0))?
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        rows
                    };
                    for id in members {
                        if !crate::sync::pending(&tx, &id)? {
                            crate::recovery::restore_incoming(&tx, &id)?;
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
        // Capture the transitive component before retiring anything. Later actions
        // may bridge this group to additional series; never discard just one side.
        let groups = crate::recovery::affected_groups(&tx, &current.event_id)?;
        let mut members = std::collections::BTreeSet::from([current.event_id.clone()]);
        let mut multi = false;
        for group in &groups {
            let count: u32 = tx.query_row(
                "SELECT count(*) FROM outbox WHERE group_id=?1",
                [group],
                |r| r.get(0),
            )?;
            multi |= count > 1;
            let mut q =
                tx.prepare("SELECT event_id FROM operation_group_series WHERE group_id=?1")?;
            members.extend(
                q.query_map([group], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            );
        }
        if multi && keep_local {
            return Err(error(
                "groupRecoveryRequired",
                "This grouped action requires a checked replan before keeping local changes.",
            ));
        }
        if !keep_local {
            for group in &groups {
                tx.execute(
                    "UPDATE outbox SET state='acknowledged',retry_at=NULL WHERE group_id=?1",
                    [group],
                )?;
            }
            for id in members {
                tx.execute("DELETE FROM conflicts WHERE event_id=?1", [&id])?;
                crate::recovery::restore_incoming(&tx, &id)?;
            }
            tx.commit()?;
            return Ok(());
        }
        let mut local = crate::repository::Repository::read_series(&tx, &current.event_id)?;
        let remote = current.remote.clone();
        if let (Some(before), Some(after)) = (&remote, &local) {
            if before.event.recurrence.is_some()
                && (before.event.time != after.event.time
                    || before.event.recurrence != after.event.recurrence)
            {
                return Err(error(
                    "unsupported",
                    "Keeping this Google schedule cannot prove preservation of exceptions.",
                ));
            }
        }
        if let (Some(observed), Some(desired)) = (&remote, &local) {
            if observed.event.recurrence.is_some() {
                let mut q = tx.prepare("SELECT original_start,before_payload,after_payload FROM outbox WHERE event_id=?1 AND state!='acknowledged' ORDER BY rowid")?;
                let rows = q
                    .query_map([&current.event_id], |r| {
                        Ok((
                            r.get::<_, Option<String>>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let resources: std::collections::BTreeSet<_> = rows
                    .iter()
                    .map(|(original, _, _)| original.clone())
                    .collect();
                if resources.len() > 1 {
                    return Err(error("groupRecoveryRequired", "Reconcile combined master and occurrence intent before keeping local changes."));
                }
                let mut rebased = observed.clone();
                if let Some(Some(original)) = resources.iter().next() {
                    let mut replacement = occurrence_event(desired, original)?;
                    if let (Some(wanted), Some(mut actual)) =
                        (&replacement, occurrence_event(observed, original)?)
                    {
                        let mut dirty = [false; 4];
                        for (_, before, after) in &rows {
                            let b: EventSeries =
                                serde_json::from_str(before.as_deref().ok_or_else(|| {
                                    error(
                                        "groupRecoveryRequired",
                                        "Missing occurrence before state.",
                                    )
                                })?)?;
                            let a: EventSeries =
                                serde_json::from_str(after.as_deref().ok_or_else(|| {
                                    error(
                                        "groupRecoveryRequired",
                                        "Missing occurrence after state.",
                                    )
                                })?)?;
                            match (
                                occurrence_event(&b, original)?,
                                occurrence_event(&a, original)?,
                            ) {
                                (Some(b), Some(a)) => {
                                    dirty[0] |= b.title != a.title;
                                    dirty[1] |= b.location != a.location;
                                    dirty[2] |= b.notes != a.notes;
                                    dirty[3] |= b.time != a.time;
                                }
                                _ => dirty.fill(true),
                            }
                        }
                        if dirty[0] {
                            actual.title = wanted.title.clone();
                        }
                        if dirty[1] {
                            actual.location = wanted.location.clone();
                        }
                        if dirty[2] {
                            actual.notes = wanted.notes.clone();
                        }
                        if dirty[3] {
                            actual.time = wanted.time.clone();
                        }
                        replacement = Some(actual);
                    }
                    // Validate correspondence against the current master, even for cancellation.
                    let mut probe = observed.clone();
                    probe.exceptions.retain(|e| &e.original_start != original);
                    aika_domain::recurrence::get_occurrence(&probe, original)?;
                    rebased.exceptions.retain(|e| &e.original_start != original);
                    rebased.exceptions.push(EventException {
                        original_start: original.clone(),
                        event: replacement,
                    });
                    rebased
                        .exceptions
                        .sort_by(|a, b| a.original_start.cmp(&b.original_start));
                } else {
                    let mut dirty = [false; 3];
                    for (_, before, after) in rows {
                        let b: EventSeries =
                            serde_json::from_str(before.as_deref().ok_or_else(|| {
                                error(
                                    "groupRecoveryRequired",
                                    "Reconcile recreated series before keeping local changes.",
                                )
                            })?)?;
                        let a: EventSeries =
                            serde_json::from_str(after.as_deref().ok_or_else(|| {
                                error(
                                    "groupRecoveryRequired",
                                    "Reconcile deleted series before keeping local changes.",
                                )
                            })?)?;
                        dirty[0] |= b.event.title != a.event.title;
                        dirty[1] |= b.event.location != a.event.location;
                        dirty[2] |= b.event.notes != a.event.notes;
                    }
                    if dirty[0] {
                        rebased.event.title = desired.event.title.clone();
                    }
                    if dirty[1] {
                        rebased.event.location = desired.event.location.clone();
                    }
                    if dirty[2] {
                        rebased.event.notes = desired.event.notes.clone();
                    }
                }
                local = Some(rebased);
            }
        }
        tx.execute("DELETE FROM conflicts WHERE id=?1", [&current.id])?;
        tx.execute(
            "UPDATE outbox SET state='acknowledged' WHERE event_id=?1 AND state!='acknowledged'",
            [&current.event_id],
        )?;
        if let Some(series) = &local {
            crate::repository::Repository::write_series(&tx, series)?;
        }
        if remote.is_some() || local.is_some() {
            let change = SeriesChange {
                id: current.event_id.clone(),
                before: remote,
                after: local,
            };
            crate::repository::Repository::queue_change(
                &tx,
                &change,
                false,
                &uuid::Uuid::new_v4().to_string(),
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Retry a reviewed permission/validation failure without dropping durable intent.
    pub fn retry_outgoing(&mut self, account: &str) -> Result<()> {
        self.account(account)?;
        self.providers.repository.db.execute("UPDATE outbox SET state='pending',retry_at=NULL WHERE account_id=?1 AND state IN ('pending','blocked') AND NOT EXISTS(SELECT 1 FROM conflicts f JOIN operation_group_series m ON m.event_id=f.event_id WHERE m.group_id=outbox.group_id)",[account])?;
        Ok(())
    }
}

fn read_conflicts(db: &rusqlite::Connection) -> Result<Vec<SyncConflict>> {
    let mut q=db.prepare("SELECT id,account_id,calendar_id,event_id,(SELECT json_object('event',json(payload),'exceptions',json((SELECT coalesce(json_group_array(json_object('originalStart',original_start,'event',json(payload))),'[]') FROM event_exceptions WHERE series_id=conflicts.event_id))) FROM events WHERE id=conflicts.event_id),remote_payload,remote_revision FROM conflicts ORDER BY rowid")?;
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
                    event_id: event_id.clone(),
                    local: local.map(|v| serde_json::from_str(&v)).transpose()?,
                    remote: if db.query_row(
                        "SELECT EXISTS(SELECT 1 FROM incoming_items WHERE id=?1)",
                        [&event_id],
                        |r| r.get::<_, bool>(0),
                    )? {
                        crate::recovery::observed_series(db, &event_id)?
                    } else {
                        remote.map(|v| serde_json::from_str(&v)).transpose()?
                    },
                    remote_revision,
                    group_revision: Some(crate::recovery::group_revision(db, &event_id)?),
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
    fn calendar_discard_is_checked_isolated_and_fences_late_publication() {
        let path = std::env::temp_dir().join(format!(
            "aika-calendar-discard-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let (mut s, mut c, cal) = setup(&path);
        let mut discovered = DiscoveredCalendar {
            remote_id: "other-calendar".into(),
            name: "Other".into(),
            color: "#718675".into(),
            description: "".into(),
            access_role: "writer".into(),
            eligible: true,
            disabled_reason: None,
            selected: true,
        };
        let other = s
            .reconcile_sync(
                &mut c,
                vec![
                    discovered.clone(),
                    DiscoveredCalendar {
                        remote_id: "remote-calendar".into(),
                        ..discovered.clone()
                    },
                ],
            )
            .unwrap()
            .into_iter()
            .find(|j| j.id != cal)
            .unwrap()
            .id;
        let original = s.save(event(&cal), false).unwrap();
        sent(&mut s, &c);
        s.delete(&original.id).unwrap();
        let late = s.claim_outgoing(&c).unwrap().unwrap();
        let created = s.save(event(&cal), false).unwrap();
        let other_event = s.save(event(&other), false).unwrap();
        let stale = s.review_calendar_removal(&cal).unwrap();
        assert_eq!(stale.pending_count, 2);
        assert_eq!(stale.attempted_count, 1);
        let mut second = open(&path);
        let mut edited = created.clone();
        edited.title = "Newer local intent".into();
        second
            .save_checked(edited, true, Some(created.clone()))
            .unwrap();
        assert_eq!(
            s.discard_and_remove_calendar(stale).unwrap_err().code,
            "conflict"
        );
        assert_eq!(s.review_calendar_removal(&cal).unwrap().pending_count, 3);
        s.providers.set_default_calendar(&cal).unwrap();
        let review = s.review_calendar_removal(&cal).unwrap();
        s.discard_and_remove_calendar(review.clone()).unwrap();
        assert!(s.discard_and_remove_calendar(review).is_err());
        assert_eq!(
            s.account(&c.account.id).unwrap().connection_state,
            ConnectionState::Connected
        );
        assert_eq!(
            s.account(&c.account.id).unwrap().selected_calendar_ids,
            vec![other.clone()]
        );
        assert_eq!(s.review_disconnect(&c.account.id).unwrap().pending_count, 1);
        assert_eq!(s.get_series(&other_event.id).unwrap().event, other_event);
        assert!(s.providers.repository.get(&created.id).unwrap().is_none());
        assert_eq!(
            s.default_calendar_id().unwrap().as_deref(),
            Some("local-default")
        );
        assert_eq!(
            s.apply_outgoing(&c, &late, Ok(OutgoingResult::Applied(None)))
                .unwrap_err()
                .code,
            "syncSuperseded"
        );
        assert_eq!(s.save(event(&cal), false).unwrap_err().code, "notFound");
        s.undo().unwrap(); // Only the other calendar's create remains in session history.
        assert!(s
            .providers
            .repository
            .get(&other_event.id)
            .unwrap()
            .is_none());
        assert_eq!(s.undo().unwrap_err().code, "nothingToUndo");
        drop(second);
        drop(s);
        let mut restarted = open(&path);
        assert!(!restarted
            .account(&c.account.id)
            .unwrap()
            .selected_calendar_ids
            .contains(&cal));
        // Explicit re-inclusion keeps its stable identity and cannot replay discarded work.
        let account = restarted.account(&c.account.id).unwrap();
        discovered.remote_id = "remote-calendar".into();
        let d = restarted
            .prepare_discovery(&account, vec![discovered])
            .unwrap();
        restarted
            .commit_selection(&d.id, vec!["remote-calendar".into()])
            .unwrap();
        assert_eq!(
            restarted
                .review_calendar_removal(&cal)
                .unwrap()
                .pending_count,
            0
        );
        drop(restarted);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn calendar_discard_clears_conflicts_without_touching_other_intent() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let original = s.save(event(&cal), false).unwrap();
        sent(&mut s, &c);
        let mut edited = original.clone();
        edited.title = "Local".into();
        s.save_checked(edited, true, Some(original)).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &op, Ok(OutgoingResult::Conflict(None)))
            .unwrap();
        let local = s.save(event("local-default"), false).unwrap();
        // A standalone conflict must still block the ordinary exclusion paths.
        s.providers
            .repository
            .db
            .execute(
                "UPDATE outbox SET state='acknowledged' WHERE calendar_id=?1",
                [&cal],
            )
            .unwrap();
        assert_eq!(s.delete_calendar(&cal).unwrap_err().code, "pendingWork");
        let review = s.review_calendar_removal(&cal).unwrap();
        assert_eq!(review.pending_count, 0);
        assert_eq!(review.conflict_count, 1);
        let mut forged = review.clone();
        forged.conflict_count = 0;
        assert_eq!(
            s.discard_and_remove_calendar(forged).unwrap_err().code,
            "conflict"
        );
        s.providers
            .repository
            .set_account_state(&c.account.id, ConnectionState::AuthenticationRequired)
            .unwrap();
        s.providers
            .repository
            .db
            .execute(
                "UPDATE calendars SET writable=0,active=0 WHERE id=?1",
                [&cal],
            )
            .unwrap();
        assert_eq!(
            s.discard_and_remove_calendar(review).unwrap_err().code,
            "conflict"
        );
        let review = s.review_calendar_removal(&cal).unwrap();
        s.discard_and_remove_calendar(review).unwrap();
        assert_eq!(
            s.account(&c.account.id).unwrap().connection_state,
            ConnectionState::AuthenticationRequired
        );
        assert!(s.sync_conflicts().unwrap().is_empty());
        s.undo().unwrap();
        assert!(s.providers.repository.get(&local.id).unwrap().is_none());
        assert!(s.review_calendar_removal("local-default").is_err());
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
        drop(restarted);
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
        let mut edited = e.clone();
        edited.title = "Edited after undo".into();
        s.save(edited, true).unwrap();
        let restore = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(restore.operation, "create");
        assert_ne!(restore.remote_id, created.remote_id);
        assert!(restore.revision.is_none());
        s.apply_outgoing(
            &c,
            &restore,
            Ok(OutgoingResult::Applied(Some(ack(&restore, "restored")))),
        )
        .unwrap();
        let edit = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(edit.operation, "update");
        assert_eq!(edit.remote_id, restore.remote_id);
        assert_eq!(edit.revision.as_deref(), Some("restored"));
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
    // Test-only compiler for two resource steps. Product recurrence planners remain gated.
    fn grouped(s: &mut CalendarService, cal: &str) -> (CalendarEvent, CalendarEvent) {
        let a = s.save(event(cal), false).unwrap();
        let b = s.save(event(cal), false).unwrap();
        merge_groups(s, &a.id, &b.id);
        (a, b)
    }
    fn merge_groups(s: &mut CalendarService, a: &str, b: &str) {
        let db = &mut s.providers.repository.db;
        let tx = db.transaction().unwrap();
        let group: String = tx
            .query_row(
                "SELECT group_id FROM outbox WHERE event_id=?1 ORDER BY rowid DESC LIMIT 1",
                [a],
                |r| r.get(0),
            )
            .unwrap();
        let old: String = tx
            .query_row(
                "SELECT group_id FROM outbox WHERE event_id=?1 ORDER BY rowid DESC LIMIT 1",
                [b],
                |r| r.get(0),
            )
            .unwrap();
        tx.execute("INSERT INTO operation_group_series SELECT ?1,event_id,before_payload,after_payload FROM operation_group_series WHERE group_id=?2",params![group,old]).unwrap();
        tx.execute(
            "UPDATE outbox SET group_id=?1,step_index=1 WHERE group_id=?2",
            params![group, old],
        )
        .unwrap();
        tx.execute("DELETE FROM operation_groups WHERE id=?1", [old])
            .unwrap();
        tx.commit().unwrap();
    }
    #[test]
    fn grouped_ack_keeps_complete_overlay_and_serializes_every_member() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let (a, b) = grouped(&mut s, &cal);
        assert_eq!(s.review_disconnect(&c.account.id).unwrap().pending_count, 1);
        let first = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(first.event_id, a.id);
        let mut response = ack(&first, "a1");
        response.event.as_mut().unwrap().title = "Provider normalized".into();
        s.apply_outgoing(&c, &first, Ok(OutgoingResult::Applied(Some(response))))
            .unwrap();
        assert_eq!(s.get_series(&a.id).unwrap().event, a);
        // A later action touching the acknowledged member still waits for its sibling.
        let mut changed = a.clone();
        changed.title = "Later intent".into();
        s.save(changed.clone(), true).unwrap();
        let second = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(second.event_id, b.id);
        s.apply_outgoing(
            &c,
            &second,
            Ok(OutgoingResult::Applied(Some(ack(&second, "b1")))),
        )
        .unwrap();
        assert_eq!(s.get_series(&a.id).unwrap().event, changed);
        let later = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(later.event_id, a.id);
        assert_eq!(later.revision.as_deref(), Some("a1"));
    }
    #[test]
    fn grouped_block_retry_conflict_and_discard_are_action_scoped() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let (a, b) = grouped(&mut s, &cal);
        let first = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &first, Err(error("permission", "Denied")))
            .unwrap_err();
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        let unrelated = s.save(event(&cal), false).unwrap();
        assert_eq!(sent(&mut s, &c).event_id, unrelated.id);
        s.retry_outgoing(&c.account.id).unwrap();
        let retry = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(retry.remote_id, first.remote_id);
        assert!(retry.recovering);
        s.apply_outgoing(
            &c,
            &retry,
            Ok(OutgoingResult::Applied(Some(ack(&retry, "a1")))),
        )
        .unwrap();
        let second = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &second, Ok(OutgoingResult::Conflict(None)))
            .unwrap();
        let reviewed = s.sync_conflicts().unwrap().remove(0);
        assert_eq!(
            s.resolve_sync_conflict(reviewed.clone(), true)
                .unwrap_err()
                .code,
            "groupRecoveryRequired"
        );
        // A change to an acknowledged member invalidates the group's review.
        s.providers.repository.db.execute("UPDATE incoming_items SET payload=json_set(payload,'$.revision','changed') WHERE id=?1",[&a.id]).unwrap();
        assert_eq!(
            s.resolve_sync_conflict(reviewed, false).unwrap_err().code,
            "conflict"
        );
        let review = s.review_disconnect(&c.account.id).unwrap();
        assert_eq!(review.pending_count, 1);
        assert_eq!(review.attempted_count, 1);
        s.discard_and_disconnect(review).unwrap();
        assert!(s.get_series(&a.id).is_ok()); // successful first step remains remote
        assert!(s.get_series(&b.id).is_err());
        assert_eq!(s.get_series(&unrelated.id).unwrap().event, unrelated);
    }
    #[test]
    fn grouped_restart_recovers_unacknowledged_step_without_releasing_overlay() {
        let path =
            std::env::temp_dir().join(format!("aika-group-crash-{}.sqlite3", uuid::Uuid::new_v4()));
        let (mut s, c, cal) = setup(&path);
        let (a, b) = grouped(&mut s, &cal);
        let first = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(
            &c,
            &first,
            Ok(OutgoingResult::Applied(Some(ack(&first, "a1")))),
        )
        .unwrap();
        let second = s.claim_outgoing(&c).unwrap().unwrap();
        drop(s);
        let mut s = open(&path);
        let other = open(&path);
        assert!(other
            .providers
            .repository
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM active_group_series WHERE event_id=?1)",
                [&a.id],
                |r| r.get::<_, bool>(0)
            )
            .unwrap());
        assert_eq!(s.get_series(&b.id).unwrap().event, b);
        s.providers
            .repository
            .db
            .execute("UPDATE account_leases SET expires_at='2000-01-01'", [])
            .unwrap();
        let next = s.begin_sync(&c.account.id, range()).unwrap().unwrap();
        let recovered = s.claim_outgoing(&next).unwrap().unwrap();
        assert_eq!(recovered.id, second.id);
        assert_eq!(recovered.remote_id, second.remote_id);
        assert!(recovered.recovering);
        assert_eq!(
            s.apply_outgoing(
                &c,
                &second,
                Ok(OutgoingResult::Applied(Some(ack(&second, "late"))))
            )
            .unwrap_err()
            .code,
            "syncSuperseded"
        );
        s.apply_outgoing(
            &next,
            &recovered,
            Ok(OutgoingResult::Applied(Some(ack(&recovered, "b1")))),
        )
        .unwrap();
        assert_eq!(s.review_disconnect(&c.account.id).unwrap().pending_count, 0);
        drop(other);
        drop(s);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn v5_pending_work_migrates_atomically_with_stable_attempted_identities() {
        let path = std::env::temp_dir().join(format!(
            "aika-v6-migration-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let (mut s, c, cal) = setup(&path);
        let a = s.save(event(&cal), false).unwrap();
        let attempted = s.claim_outgoing(&c).unwrap().unwrap();
        s.save(event(&cal), false).unwrap();
        s.providers.repository.db.execute_batch("DROP VIEW active_group_series; DROP TRIGGER outbox_group_scope; DROP TRIGGER outbox_group_scope_update; DROP TRIGGER outbox_group_member; DROP INDEX outbox_group_order; ALTER TABLE outbox DROP COLUMN group_id; ALTER TABLE outbox DROP COLUMN step_index; ALTER TABLE outbox DROP COLUMN resource_id; ALTER TABLE outbox DROP COLUMN series_remote_id; ALTER TABLE outbox DROP COLUMN original_start; DROP TABLE operation_group_series; DROP TABLE operation_groups; PRAGMA user_version=5; CREATE TABLE operation_groups(marker TEXT); INSERT INTO operation_groups VALUES('preserve');").unwrap();
        drop(s);
        assert!(Repository::open(&path).is_err());
        let db = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            5
        );
        assert_eq!(
            db.query_row("SELECT marker FROM operation_groups", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "preserve"
        );
        db.execute("DROP TABLE operation_groups", []).unwrap();
        drop(db);
        let mut s = open(&path);
        let identity: (String, String, u32) = s
            .providers
            .repository
            .db
            .query_row(
                "SELECT group_id,resource_id,attempts FROM outbox WHERE id=?1",
                [&attempted.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            identity,
            (attempted.id.clone(), attempted.remote_id.clone(), 1)
        );
        assert_eq!(s.get_series(&a.id).unwrap().event, a);
        assert_eq!(s.review_disconnect(&c.account.id).unwrap().pending_count, 2);
        s.providers
            .repository
            .db
            .execute("UPDATE account_leases SET expires_at='2000-01-01'", [])
            .unwrap();
        let next = s.begin_sync(&c.account.id, range()).unwrap().unwrap();
        let recovered = s.claim_outgoing(&next).unwrap().unwrap();
        assert_eq!(recovered.id, attempted.id);
        assert_eq!(recovered.remote_id, attempted.remote_id);
        assert!(recovered.recovering);
        drop(s);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn group_review_counts_acknowledged_attempts_and_partial_undo_never_retires_a_member() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let (a, b) = grouped(&mut s, &cal);
        assert_eq!(s.undo().unwrap_err().code, "groupRecoveryRequired");
        assert_eq!(s.get_series(&b.id).unwrap().event, b);
        let first = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(
            &c,
            &first,
            Ok(OutgoingResult::Applied(Some(ack(&first, "a1")))),
        )
        .unwrap();
        let review = s.review_disconnect(&c.account.id).unwrap();
        assert_eq!((review.pending_count, review.attempted_count), (1, 1));
        assert_eq!(s.review_calendar_removal(&cal).unwrap().attempted_count, 1);
        assert_eq!(s.undo().unwrap_err().code, "groupRecoveryRequired");
        assert_eq!(s.get_series(&a.id).unwrap().event, a);
        assert_eq!(s.get_series(&b.id).unwrap().event, b);
    }
    #[test]
    fn keep_google_retires_transitively_overlapping_groups_together() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let (a, b) = grouped(&mut s, &cal);
        let first = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(
            &c,
            &first,
            Ok(OutgoingResult::Applied(Some(ack(&first, "a1")))),
        )
        .unwrap();
        let second = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &second, Ok(OutgoingResult::Conflict(None)))
            .unwrap();
        let reviewed = s.sync_conflicts().unwrap().remove(0);
        let mut edited = b.clone();
        edited.title = "Later local intent".into();
        s.save(edited, true).unwrap();
        let tail = s.save(event(&cal), false).unwrap();
        merge_groups(&mut s, &b.id, &tail.id);
        assert_eq!(
            s.resolve_sync_conflict(reviewed, false).unwrap_err().code,
            "conflict"
        );
        let current = s.sync_conflicts().unwrap().remove(0);
        s.resolve_sync_conflict(current, false).unwrap();
        assert!(s.sync_conflicts().unwrap().is_empty());
        assert_eq!(s.review_disconnect(&c.account.id).unwrap().pending_count, 0);
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        assert_eq!(s.get_series(&a.id).unwrap().event, a);
        assert!(s.get_series(&b.id).is_err());
        assert!(s.get_series(&tail.id).is_err());
    }
    #[test]
    fn group_identity_constraints_and_selector_gate_prevent_unsafe_delivery() {
        let (mut s, c, cal) = setup(Path::new(":memory:"));
        let other = s
            .providers
            .repository
            .upsert_account("fixture", "other", "Other", ConnectionState::Connected)
            .unwrap();
        let e = s.save(event(&cal), false).unwrap();
        let db = &s.providers.repository.db;
        assert!(db
            .execute(
                "INSERT INTO operation_groups VALUES('wrong',?1,?2)",
                params![other, cal]
            )
            .is_err());
        assert!(db
            .execute(
                "UPDATE outbox SET account_id=?1 WHERE event_id=?2",
                params![other, e.id]
            )
            .is_err());
        db.execute("UPDATE outbox SET series_remote_id='series',original_start='2026-10-01' WHERE event_id=?1",[&e.id]).unwrap();
        assert_eq!(s.claim_outgoing(&c).unwrap_err().code, "unsupported");
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        let unrelated = s.save(event(&cal), false).unwrap();
        assert_eq!(sent(&mut s, &c).event_id, unrelated.id);
    }
    type InstanceState = std::sync::Arc<std::sync::Mutex<(IncomingItem, u32)>>;
    struct InstanceAdapter(InstanceState);
    impl Provider for InstanceAdapter {
        fn descriptor(&self) -> ProviderDescriptor {
            Adapter.descriptor()
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::google_recurring()
        }
        fn event_capabilities(&self, _: &IncomingItem) -> Capabilities {
            Capabilities::google_recurring()
        }
        fn resolve_outgoing_instance(
            &self,
            _: &str,
            _: &str,
            master: &str,
            original: &str,
        ) -> Result<IncomingItem> {
            let mut state = self.0.lock().unwrap();
            state.1 += 1;
            assert_eq!(state.0.master.as_deref(), Some(master));
            assert_eq!(state.0.original_start.as_deref(), Some(original));
            Ok(state.0.clone())
        }
    }
    fn recurring_setup(
        path: &Path,
    ) -> (
        CalendarService,
        SyncClaim,
        String,
        EventSeries,
        InstanceState,
    ) {
        let (mut s, mut claim, cal) = setup(path);
        s.providers
            .repository
            .db
            .execute(
                "UPDATE calendars SET capabilities=?2 WHERE id=?1",
                params![
                    cal,
                    serde_json::to_string(&Capabilities::google_recurring()).unwrap()
                ],
            )
            .unwrap();
        let mut e = event(&cal);
        e.recurrence = Some(Recurrence {
            frequency: RepeatFrequency::Daily,
            interval: 2,
            weekdays: vec![],
            monthly_on: MonthlyOn::Date,
            until: None,
            count: Some(6),
            segment: None,
        });
        e.id = s
            .providers
            .repository
            .cache_event(
                &claim.account.id,
                "actual-master",
                e.clone(),
                "master-r1",
                "{}",
                Capabilities::google_recurring(),
            )
            .unwrap();
        let master = IncomingItem {
            remote_id: "actual-master".into(),
            revision: "master-r1".into(),
            shadow: "{}".into(),
            event: Some(e.clone()),
            master: None,
            original_start: None,
            unsupported: false,
        };
        s.providers
            .repository
            .db
            .execute(
                "INSERT INTO incoming_items VALUES(?1,?2)",
                params![e.id, serde_json::to_string(&master).unwrap()],
            )
            .unwrap();
        let mut replacement = e.clone();
        replacement.recurrence = None;
        replacement.time = EventTime::AllDay {
            start_date: "2026-10-07".into(),
            end_date: "2026-10-08".into(),
        };
        let instance = IncomingItem {
            remote_id: "provider-moved-id".into(),
            revision: "instance-r1".into(),
            shadow: "{}".into(),
            event: Some(replacement.clone()),
            master: Some("actual-master".into()),
            original_start: Some("2026-10-03".into()),
            unsupported: false,
        };
        let cancelled = IncomingItem {
            remote_id: "provider-cancelled-id".into(),
            revision: "cancel-r1".into(),
            shadow: "{}".into(),
            event: None,
            master: Some("actual-master".into()),
            original_start: Some("2026-10-05".into()),
            unsupported: false,
        };
        for item in [&instance, &cancelled] {
            let id = mapping(
                &s.providers.repository.db,
                &claim.account.id,
                &cal,
                item,
                Capabilities::google_recurring(),
            )
            .unwrap();
            s.providers
                .repository
                .db
                .execute(
                    "INSERT INTO incoming_items VALUES(?1,?2)",
                    params![id, serde_json::to_string(item).unwrap()],
                )
                .unwrap();
        }
        let series = EventSeries {
            event: e,
            exceptions: vec![
                EventException {
                    original_start: "2026-10-03".into(),
                    event: Some(replacement),
                },
                EventException {
                    original_start: "2026-10-05".into(),
                    event: None,
                },
            ],
        };
        Repository::write_series(&s.providers.repository.db, &series).unwrap();
        let state = std::sync::Arc::new(std::sync::Mutex::new((instance, 0)));
        claim.adapter = std::sync::Arc::new(InstanceAdapter(state.clone()));
        (s, claim, cal, series, state)
    }
    fn recurring_request(
        series: &EventSeries,
        scope: MutationScope,
        event: Option<CalendarEvent>,
    ) -> EventMutation {
        EventMutation {
            target: if scope == MutationScope::Series {
                series.event.id.clone()
            } else {
                format!("{}@2026-10-03", series.event.id)
            },
            scope,
            event,
            expected: aika_domain::recurrence::expectation(series).unwrap(),
            reset_exceptions: false,
            exception_policy: Some(ExceptionPolicy::Preserve),
            count_changed: false,
        }
    }
    #[test]
    fn recurring_metadata_preserves_exceptions_rejects_reset_structure_and_stale_connections() {
        let path =
            std::env::temp_dir().join(format!("aika-phase02-{}.sqlite3", uuid::Uuid::new_v4()));
        let (mut s, c, _, series, _) = recurring_setup(&path);
        let mut other = Repository::open(&path).unwrap();
        let mut e = series.event.clone();
        e.title = "Series metadata".into();
        let request = recurring_request(&series, MutationScope::Series, Some(e.clone()));
        s.providers
            .repository
            .mutate_event(request.clone())
            .unwrap();
        let saved = s.providers.repository.get_series(&series.event.id).unwrap();
        assert_eq!(saved.exceptions, series.exceptions);
        assert_eq!(other.mutate_event(request).unwrap_err().code, "conflict");
        e = saved.event.clone();
        if let EventTime::AllDay { start_date, .. } = &mut e.time {
            *start_date = "2026-09-30".into();
        }
        assert_eq!(
            s.providers
                .repository
                .mutate_event(recurring_request(&saved, MutationScope::Series, Some(e)))
                .unwrap_err()
                .code,
            "unsupported"
        );
        let mut reset = recurring_request(&saved, MutationScope::Series, Some(saved.event.clone()));
        reset.reset_exceptions = true;
        assert_eq!(
            s.providers.repository.mutate_event(reset).unwrap_err().code,
            "unsupported"
        );
        assert_eq!(
            s.providers
                .repository
                .mutate_event(recurring_request(&saved, MutationScope::Following, None))
                .unwrap_err()
                .code,
            "unsupported"
        );
        assert_eq!(
            s.providers.repository.sync_status(&range()).unwrap()[0].pending_count,
            1
        );
        s.end_sync(&c, None).unwrap();
        drop(other);
        drop(s);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn occurrence_identity_binding_restart_overlay_and_checked_compensation() {
        let path = std::env::temp_dir().join(format!(
            "aika-phase02-restart-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let (mut s, c, _, series, state) = recurring_setup(&path);
        let mut changed = aika_domain::recurrence::get_occurrence(&series, "2026-10-03").unwrap();
        changed.title = "Moved edit".into();
        let request = recurring_request(&series, MutationScope::Occurrence, Some(changed));
        s.providers
            .repository
            .mutate_event(request.clone())
            .unwrap();
        s.providers.repository.undo().unwrap();
        assert_eq!(
            s.providers.repository.get_series(&series.event.id).unwrap(),
            series
        );
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        s.providers.repository.mutate_event(request).unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(op.remote_id, "provider-moved-id");
        assert_eq!(op.revision.as_deref(), Some("instance-r1"));
        assert_eq!(state.lock().unwrap().1, 1);
        let mut remote = state.lock().unwrap().0.clone();
        remote.event.as_mut().unwrap().title = "Moved edit".into();
        remote.revision = "instance-r2".into();
        s.apply_outgoing(&c, &op, Ok(OutgoingResult::Applied(Some(remote.clone()))))
            .unwrap();
        assert_eq!(
            s.providers
                .repository
                .get_series(&series.event.id)
                .unwrap()
                .exceptions
                .len(),
            2
        );
        state.lock().unwrap().0 = remote;
        s.providers.repository.undo().unwrap();
        let compensation = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(compensation.remote_id, "provider-moved-id");
        assert_eq!(compensation.revision.as_deref(), Some("instance-r2"));
        // Crash after identity/attempt persistence and before acknowledgement.
        s.end_sync(&c, None).unwrap();
        drop(s);
        let mut reopened = open(&path);
        let mut fresh = reopened
            .begin_sync(&c.account.id, range())
            .unwrap()
            .unwrap();
        fresh.adapter = std::sync::Arc::new(InstanceAdapter(state.clone()));
        let recovered = reopened.claim_outgoing(&fresh).unwrap().unwrap();
        assert!(recovered.recovering);
        assert_eq!(recovered.id, compensation.id);
        assert_eq!(recovered.remote_id, compensation.remote_id);
        assert_eq!(state.lock().unwrap().1, 2);
        let mut restored = state.lock().unwrap().0.clone();
        restored.event.as_mut().unwrap().title = "Original".into();
        restored.revision = "instance-r3".into();
        reopened
            .apply_outgoing(
                &fresh,
                &recovered,
                Ok(OutgoingResult::Applied(Some(restored))),
            )
            .unwrap();
        assert_eq!(
            reopened
                .providers
                .repository
                .get_series(&series.event.id)
                .unwrap(),
            series
        );
        reopened.end_sync(&fresh, None).unwrap();
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn deleted_series_compensation_restores_exceptions_as_ordered_checked_steps() {
        let (mut s, mut c, _, series, state) = recurring_setup(Path::new(":memory:"));
        s.providers
            .repository
            .mutate_event(recurring_request(&series, MutationScope::Series, None))
            .unwrap();
        let delete = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &delete, Ok(OutgoingResult::Applied(None)))
            .unwrap();
        s.providers.repository.undo().unwrap();
        let create = s.claim_outgoing(&c).unwrap().unwrap();
        assert_eq!(create.operation, "create");
        assert_ne!(create.remote_id, delete.remote_id);
        s.apply_outgoing(
            &c,
            &create,
            Ok(OutgoingResult::Applied(Some(ack(&create, "new-master-r1")))),
        )
        .unwrap();
        // Master success does not release the complete local overlay or count siblings separately.
        assert_eq!(
            s.providers.repository.get_series(&series.event.id).unwrap(),
            series
        );
        assert_eq!(
            s.providers.repository.sync_status(&range()).unwrap()[0].pending_count,
            1
        );
        for exception in &series.exceptions {
            let mut base = series.clone();
            base.exceptions.clear();
            let event = occurrence_event(&base, &exception.original_start).unwrap();
            let item = IncomingItem {
                remote_id: format!("resolved-{}", exception.original_start),
                revision: "new-instance-r1".into(),
                shadow: "{}".into(),
                event,
                master: Some(create.remote_id.clone()),
                original_start: Some(exception.original_start.clone()),
                unsupported: false,
            };
            state.lock().unwrap().0 = item;
            c.adapter = std::sync::Arc::new(InstanceAdapter(state.clone()));
            let step = s.claim_outgoing(&c).unwrap().unwrap();
            assert_eq!(
                step.series_remote_id.as_deref(),
                Some(create.remote_id.as_str())
            );
            assert_ne!(step.remote_id, create.remote_id);
            let mut observed = state.lock().unwrap().0.clone();
            observed.event = exception.event.clone();
            observed.revision = "new-instance-r2".into();
            s.apply_outgoing(&c, &step, Ok(OutgoingResult::Applied(Some(observed))))
                .unwrap();
        }
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        assert_eq!(
            s.providers.repository.get_series(&series.event.id).unwrap(),
            series
        );
        assert_eq!(
            s.providers.repository.sync_status(&range()).unwrap()[0].pending_count,
            0
        );
    }
    #[test]
    fn missing_occurrence_conflict_never_tombstones_the_master() {
        let (mut s, c, _, series, _) = recurring_setup(Path::new(":memory:"));
        s.providers
            .repository
            .mutate_event(recurring_request(&series, MutationScope::Occurrence, None))
            .unwrap();
        let op = s.claim_outgoing(&c).unwrap().unwrap();
        s.apply_outgoing(&c, &op, Ok(OutgoingResult::Conflict(None)))
            .unwrap();
        let conflict = s.sync_conflicts().unwrap().remove(0);
        assert!(conflict.remote.as_ref().unwrap().event.recurrence.is_some());
        assert_eq!(conflict.local.as_ref().unwrap().exceptions.len(), 2);
        s.resolve_sync_conflict(conflict, false).unwrap();
        let observed = s.providers.repository.get_series(&series.event.id).unwrap();
        assert!(observed.event.recurrence.is_some());
        assert!(observed
            .exceptions
            .iter()
            .find(|e| e.original_start == "2026-10-03")
            .unwrap()
            .event
            .is_none());
    }
    #[test]
    fn recurring_keep_local_rebases_metadata_and_occurrence_without_resetting_remote_exceptions() {
        for occurrence in [false, true] {
            let (mut s, c, _, series, state) = recurring_setup(Path::new(":memory:"));
            let scope = if occurrence {
                MutationScope::Occurrence
            } else {
                MutationScope::Series
            };
            let mut changed = if occurrence {
                aika_domain::recurrence::get_occurrence(&series, "2026-10-03").unwrap()
            } else {
                series.event.clone()
            };
            changed.title = "Local intent".into();
            s.providers
                .repository
                .mutate_event(recurring_request(&series, scope, Some(changed)))
                .unwrap();
            let op = s.claim_outgoing(&c).unwrap().unwrap();
            let mut remote = if occurrence {
                state.lock().unwrap().0.clone()
            } else {
                ack(&op, "master-r2")
            };
            if !occurrence {
                remote.event = Some(series.event.clone());
            }
            remote.event.as_mut().unwrap().notes = "External notes".into();
            remote.revision = "remote-r2".into();
            s.apply_outgoing(&c, &op, Ok(OutgoingResult::Conflict(Some(remote))))
                .unwrap();
            let stale = s.sync_conflicts().unwrap().remove(0);
            let mut external = state.lock().unwrap().0.clone();
            external.original_start = Some("2026-10-05".into());
            external.remote_id = "provider-cancelled-id".into();
            external.revision = "exception-r2".into();
            external.event.as_mut().unwrap().title = "External restoration".into();
            let id = mapping(
                &s.providers.repository.db,
                &c.account.id,
                &op.calendar_id,
                &external,
                Capabilities::google_recurring(),
            )
            .unwrap();
            s.providers
                .repository
                .db
                .execute(
                    "UPDATE incoming_items SET payload=?2 WHERE id=?1",
                    params![id, serde_json::to_string(&external).unwrap()],
                )
                .unwrap();
            assert_eq!(
                s.resolve_sync_conflict(stale, true).unwrap_err().code,
                "conflict"
            );
            let fresh = s.sync_conflicts().unwrap().remove(0);
            s.resolve_sync_conflict(fresh, true).unwrap();
            let saved = s.providers.repository.get_series(&series.event.id).unwrap();
            assert_eq!(
                saved
                    .exceptions
                    .iter()
                    .find(|e| e.original_start == "2026-10-05")
                    .unwrap()
                    .event
                    .as_ref()
                    .unwrap()
                    .title,
                "External restoration"
            );
            if occurrence {
                assert_eq!(saved.event.title, "Original");
                assert_eq!(
                    saved.exceptions[0].event.as_ref().unwrap().title,
                    "Local intent"
                );
                assert_eq!(
                    saved.exceptions[0].event.as_ref().unwrap().notes,
                    "External notes"
                );
            } else {
                assert_eq!(saved.event.title, "Local intent");
                assert_eq!(saved.event.notes, "External notes");
            }
        }
    }
    #[test]
    fn invalid_instance_revision_blocks_only_its_group() {
        let (mut s, c, cal, series, state) = recurring_setup(Path::new(":memory:"));
        state.lock().unwrap().0.revision.clear();
        s.providers
            .repository
            .mutate_event(recurring_request(&series, MutationScope::Occurrence, None))
            .unwrap();
        assert_eq!(s.claim_outgoing(&c).unwrap_err().code, "providerIdentity");
        assert!(s.claim_outgoing(&c).unwrap().is_none());
        let unrelated = s.save(event(&cal), false).unwrap();
        assert_eq!(sent(&mut s, &c).event_id, unrelated.id);
        assert_eq!(
            s.providers.repository.sync_status(&range()).unwrap()[0].pending_count,
            1
        );
    }
    #[test]
    fn interleaved_master_and_instance_steps_use_resource_specific_revision_dependencies() {
        for preceding_master in [false, true] {
            let (mut s, c, _, mut series, state) = recurring_setup(Path::new(":memory:"));
            if preceding_master {
                let mut e = series.event.clone();
                e.title = "First metadata".into();
                s.providers
                    .repository
                    .mutate_event(recurring_request(&series, MutationScope::Series, Some(e)))
                    .unwrap();
                series = s.providers.repository.get_series(&series.event.id).unwrap();
            }
            let mut occurrence =
                aika_domain::recurrence::get_occurrence(&series, "2026-10-03").unwrap();
            occurrence.title = "Occurrence edit".into();
            s.providers
                .repository
                .mutate_event(recurring_request(
                    &series,
                    MutationScope::Occurrence,
                    Some(occurrence),
                ))
                .unwrap();
            series = s.providers.repository.get_series(&series.event.id).unwrap();
            let mut e = series.event.clone();
            e.title = "Last metadata".into();
            s.providers
                .repository
                .mutate_event(recurring_request(&series, MutationScope::Series, Some(e)))
                .unwrap();
            if preceding_master {
                let op = s.claim_outgoing(&c).unwrap().unwrap();
                assert_eq!(op.revision.as_deref(), Some("master-r1"));
                s.apply_outgoing(
                    &c,
                    &op,
                    Ok(OutgoingResult::Applied(Some(ack(&op, "master-r2")))),
                )
                .unwrap();
            }
            let instance = s.claim_outgoing(&c).unwrap().unwrap();
            assert_eq!(instance.revision.as_deref(), Some("instance-r1"));
            let mut observed = state.lock().unwrap().0.clone();
            observed.revision = "instance-r2".into();
            observed.event.as_mut().unwrap().title = "Occurrence edit".into();
            s.apply_outgoing(&c, &instance, Ok(OutgoingResult::Applied(Some(observed))))
                .unwrap();
            let master = s.claim_outgoing(&c).unwrap().unwrap();
            assert_eq!(
                master.revision.as_deref(),
                Some(if preceding_master {
                    "master-r2"
                } else {
                    "master-r1"
                })
            );
            assert!(master.original_start.is_none());
        }
    }
}
