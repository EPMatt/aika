//! Shared incoming coordinator state. Network work happens between short checked transactions.
use crate::{
    contracts::*,
    providers::Provider,
    repository::Repository,
    service::{error, CalendarService},
};
use chrono::{Months, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
type Result<T> = std::result::Result<T, ServiceError>;

/// Adapter-owned conversion; shadow is the untouched provider resource, never OAuth material.
#[derive(Clone, Serialize, Deserialize)]
pub struct IncomingItem {
    pub remote_id: String,
    pub revision: String,
    pub shadow: String,
    pub event: Option<CalendarEvent>,
    pub master: Option<String>,
    pub original_start: Option<String>,
    pub unsupported: bool,
}
pub struct IncomingPage {
    pub items: Vec<IncomingItem>,
    pub next_page: Option<String>,
    pub cursor: Option<String>,
}
#[derive(Clone)]
pub struct SyncClaim {
    pub account: AccountSummary,
    pub adapter: Arc<dyn Provider>,
    pub owner: String,
    epoch: i64,
    pub range: EventRange,
}
pub struct CalendarJob {
    pub id: String,
    pub remote_id: String,
    pub cursor: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Generation {
    owner: String,
    epoch: i64,
    full: bool,
    range: EventRange,
    items: Vec<IncomingItem>,
    projections: Vec<IncomingItem>,
    feed_complete: bool,
    projection_complete: bool,
    cursor: Option<String>,
}
fn stamp() -> String {
    Utc::now().to_rfc3339()
}
fn lease_end() -> String {
    (Utc::now() + chrono::Duration::seconds(90)).to_rfc3339()
}
pub(crate) fn guard(db: &Connection, claim: &SyncClaim) -> Result<()> {
    let current: Option<(i64, String, String, String)> = db.query_row(
        "SELECT a.sync_epoch,a.connection_state,l.owner,l.expires_at FROM accounts a JOIN account_leases l ON l.account_id=a.id WHERE a.id=?1",
        [&claim.account.id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if !current.is_some_and(|(epoch, state, owner, expiry)| {
        epoch == claim.epoch && state == "\"connected\"" && owner == claim.owner && expiry > stamp()
    }) {
        return Err(error(
            "syncSuperseded",
            "Account or calendar selection changed. Refresh again.",
        ));
    }
    db.execute(
        "UPDATE account_leases SET expires_at=?2 WHERE account_id=?1",
        params![claim.account.id, lease_end()],
    )?;
    Ok(())
}
pub(crate) fn pending(db: &Connection, id: &str) -> Result<bool> {
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM active_group_series WHERE event_id=?1) OR EXISTS(SELECT 1 FROM conflicts WHERE event_id=?1)", [id], |r| r.get(0))?)
}
pub(crate) fn mapping(
    db: &Connection,
    account: &str,
    calendar: &str,
    item: &IncomingItem,
    caps: Capabilities,
) -> Result<String> {
    let existing: Option<String> = db
        .query_row(
            "SELECT id FROM remote_events WHERE account_id=?1 AND calendar_id=?2 AND remote_id=?3",
            params![account, calendar, item.remote_id],
            |r| r.get(0),
        )
        .optional()?;
    let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    db.execute("INSERT INTO remote_events(id,account_id,calendar_id,remote_id,revision,shadow_payload,capabilities) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,shadow_payload=CASE WHEN ?8 THEN remote_events.shadow_payload ELSE excluded.shadow_payload END,capabilities=excluded.capabilities", params![id,account,calendar,item.remote_id,item.revision,item.shadow,serde_json::to_string(&caps)?,item.event.is_none()])?;
    // Refresh the retained remote side of an existing conflict, without touching local intent.
    let remote = item.event.as_ref().map(|event| {
        let mut event = event.clone();
        event.id = id.clone();
        event.calendar_id = calendar.into();
        EventSeries {
            event,
            exceptions: vec![],
        }
    });
    db.execute(
        "UPDATE conflicts SET remote_payload=?2,remote_revision=?3 WHERE event_id=?1",
        params![
            id,
            remote.as_ref().map(serde_json::to_string).transpose()?,
            if item.event.is_some() {
                Some(&item.revision)
            } else {
                None
            }
        ],
    )?;
    Ok(id)
}
impl CalendarService {
    pub fn check_sync(&mut self, claim: &SyncClaim) -> Result<()> {
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard(&tx, claim)?;
        tx.commit()?;
        Ok(())
    }
    pub fn begin_sync(&mut self, account_id: &str, range: EventRange) -> Result<Option<SyncClaim>> {
        crate::service::validate_range(&range)?;
        let account = self.account(account_id)?;
        if account.connection_state != ConnectionState::Connected {
            return Ok(None);
        }
        if !self.providers.repository.db.query_row(
            "SELECT selection_confirmed FROM accounts WHERE id=?1",
            [account_id],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(None);
        }
        let adapter = self.connection_adapter(&account.provider_id)?;
        // Baseline coverage plus the navigated range. Calendar month arithmetic preserves dates.
        let now = Utc::now();
        let start = (now
            .checked_sub_months(Months::new(12))
            .unwrap()
            .date_naive())
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
        let end = (now
            .checked_add_months(Months::new(24))
            .unwrap()
            .date_naive())
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc();
        let start = start.min(
            chrono::DateTime::parse_from_rfc3339(&range.start)
                .unwrap()
                .with_timezone(&Utc)
                - chrono::Duration::days(2),
        );
        let end = end.max(
            chrono::DateTime::parse_from_rfc3339(&range.end)
                .unwrap()
                .with_timezone(&Utc)
                + chrono::Duration::days(2),
        );
        let range = EventRange {
            start: aika_domain::iso(start),
            end: aika_domain::iso(end),
            start_date: start.date_naive().to_string(),
            end_date: end.date_naive().to_string(),
        };
        let repo = &mut self.providers.repository;
        let tx = repo
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner = uuid::Uuid::new_v4().to_string();
        let changed=tx.execute("INSERT INTO account_leases(account_id,owner,expires_at) VALUES(?1,?2,?3) ON CONFLICT(account_id) DO UPDATE SET owner=excluded.owner,expires_at=excluded.expires_at WHERE account_leases.expires_at<=?4",params![account_id,owner,lease_end(),stamp()])?;
        if changed == 0 {
            return Ok(None);
        }
        let epoch = tx.query_row(
            "SELECT sync_epoch FROM accounts WHERE id=?1",
            [account_id],
            |r| r.get(0),
        )?;
        // Recover abandoned staging; effective cache/cursors remain untouched.
        tx.execute("DELETE FROM sync_generations WHERE calendar_id IN (SELECT id FROM calendars WHERE account_id=?1)",[account_id])?;
        tx.execute(
            "UPDATE accounts SET sync_failure=NULL WHERE id=?1",
            [account_id],
        )?;
        tx.commit()?;
        Ok(Some(SyncClaim {
            account,
            adapter,
            owner,
            epoch,
            range,
        }))
    }
    /// A complete calendar-list refresh auto-adds eligible calendars and preserves explicit exclusions.
    pub fn reconcile_sync(
        &mut self,
        claim: &mut SyncClaim,
        calendars: Vec<DiscoveredCalendar>,
    ) -> Result<Vec<CalendarJob>> {
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard(&tx, claim)?;
        let mut ids = BTreeSet::new();
        for c in &calendars {
            if c.remote_id.is_empty() || !ids.insert(c.remote_id.clone()) {
                return Err(error(
                    "providerIdentity",
                    "Invalid calendar discovery identity.",
                ));
            }
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM calendars WHERE account_id=?1 AND remote_id=?2",
                    params![claim.account.id, c.remote_id],
                    |r| r.get(0),
                )
                .optional()?;
            if existing.is_none() && !c.eligible {
                continue;
            }
            let id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            tx.execute("INSERT INTO calendars(id,name,color,description,provider_id,account_id,remote_id,writable,capabilities,active,remote_metadata) VALUES(?1,?2,?3,?4,?5,?6,?7,?11,?8,?9,?10) ON CONFLICT(account_id,remote_id) DO UPDATE SET active=excluded.active,writable=excluded.writable,capabilities=excluded.capabilities,
                name=CASE WHEN calendars.name=json_extract(calendars.remote_metadata,'$.name') THEN excluded.name ELSE calendars.name END,
                color=CASE WHEN calendars.color=json_extract(calendars.remote_metadata,'$.color') THEN excluded.color ELSE calendars.color END,
                description=CASE WHEN calendars.description=json_extract(calendars.remote_metadata,'$.description') THEN excluded.description ELSE calendars.description END,
                remote_metadata=excluded.remote_metadata",params![id,c.name,c.color,c.description,claim.account.provider_id,claim.account.id,c.remote_id,serde_json::to_string(&claim.adapter.capabilities())?,c.eligible,serde_json::to_string(&serde_json::json!({"name":c.name,"color":c.color,"description":c.description}))?,c.eligible && matches!(c.access_role.as_str(),"owner"|"writer")])?;
        }
        let existing = {
            let mut q = tx.prepare("SELECT id,remote_id FROM calendars WHERE account_id=?1")?;
            let values = q
                .query_map([&claim.account.id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            values
        };
        for (id, remote) in existing {
            if !ids.contains(&remote) {
                tx.execute("UPDATE calendars SET active=0 WHERE id=?1", [id])?;
            }
        }
        Repository::repair_default(&tx)?;
        claim.epoch = tx.query_row(
            "SELECT sync_epoch FROM accounts WHERE id=?1",
            [&claim.account.id],
            |r| r.get(0),
        )?;
        let jobs = {
            let mut q=tx.prepare("SELECT c.id,c.remote_id,s.cursor FROM calendars c LEFT JOIN sync_cursors s ON s.calendar_id=c.id WHERE c.account_id=?1 AND c.selected=1 AND c.active=1")?;
            let values = q
                .query_map([&claim.account.id], |r| {
                    Ok(CalendarJob {
                        id: r.get(0)?,
                        remote_id: r.get(1)?,
                        cursor: r.get(2)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            values
        };
        tx.commit()?;
        Ok(jobs)
    }
    pub fn start_generation(
        &mut self,
        claim: &SyncClaim,
        job: &CalendarJob,
        full: bool,
    ) -> Result<String> {
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard(&tx, claim)?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calendars WHERE id=?1 AND account_id=?2 AND remote_id=?3 AND selected=1 AND active=1)",params![job.id,claim.account.id,job.remote_id],|r|r.get(0))?;
        if !valid {
            return Err(error("providerIdentity", "Calendar identity changed."));
        }
        tx.execute(
            "DELETE FROM sync_generations WHERE calendar_id=?1",
            [&job.id],
        )?;
        let id = uuid::Uuid::new_v4().to_string();
        let generation = Generation {
            owner: claim.owner.clone(),
            epoch: claim.epoch,
            full,
            range: claim.range.clone(),
            items: vec![],
            projections: vec![],
            feed_complete: false,
            projection_complete: false,
            cursor: None,
        };
        tx.execute(
            "INSERT INTO sync_generations(id,calendar_id,payload) VALUES(?1,?2,?3)",
            params![id, job.id, serde_json::to_string(&generation)?],
        )?;
        tx.commit()?;
        Ok(id)
    }
    pub fn stage_page(
        &mut self,
        claim: &SyncClaim,
        id: &str,
        page: IncomingPage,
        projection: bool,
    ) -> Result<()> {
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard(&tx, claim)?;
        let json: String = tx.query_row(
            "SELECT payload FROM sync_generations WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        let mut g: Generation = serde_json::from_str(&json)?;
        if g.owner != claim.owner
            || g.epoch != claim.epoch
            || (if projection {
                g.projection_complete
            } else {
                g.feed_complete
            })
        {
            return Err(error(
                "syncSuperseded",
                "Synchronization generation expired.",
            ));
        }
        let target = if projection {
            &mut g.projections
        } else {
            &mut g.items
        };
        let mut seen: BTreeSet<_> = target.iter().map(|i| i.remote_id.clone()).collect();
        for item in page.items {
            if item.remote_id.is_empty() || !seen.insert(item.remote_id.clone()) {
                return Err(error(
                    "providerIdentity",
                    "Invalid or duplicate incoming event identity.",
                ));
            }
            target.push(item);
        }
        if target.len() > 100_000 {
            return Err(error(
                "syncLimit",
                "Calendar import exceeds the supported size.",
            ));
        }
        if page.next_page.is_none() {
            if projection {
                g.projection_complete = true;
            } else {
                g.cursor = if claim.adapter.incremental_sync() {
                    Some(page.cursor.filter(|s| !s.is_empty()).ok_or_else(|| {
                        error(
                            "providerIdentity",
                            "A complete event feed must include a synchronization cursor.",
                        )
                    })?)
                } else {
                    None
                };
                g.feed_complete = true;
            }
        }
        let payload = serde_json::to_string(&g)?;
        if payload.len() > 64 * 1024 * 1024 {
            return Err(error(
                "syncLimit",
                "Calendar import exceeds the supported size.",
            ));
        }
        tx.execute(
            "UPDATE sync_generations SET payload=?2,final_cursor=?3,complete=?4 WHERE id=?1",
            params![
                id,
                payload,
                g.cursor,
                g.feed_complete && g.projection_complete
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn publish_generation(&mut self, claim: &SyncClaim, generation: &str) -> Result<()> {
        self.providers
            .repository
            .publish_incoming(claim, generation)
    }
    pub fn end_sync(&mut self, claim: &SyncClaim, failure: Option<ServiceError>) -> Result<()> {
        let tx = self
            .providers
            .repository
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let valid = guard(&tx, claim).is_ok();
        if valid {
            if let Some(failure) = &failure {
                tx.execute(
                    "UPDATE accounts SET sync_failure=?2 WHERE id=?1",
                    params![claim.account.id, serde_json::to_string(failure)?],
                )?;
                if failure.code == "authenticationRequired" {
                    tx.execute("UPDATE accounts SET connection_state='\"authenticationRequired\"' WHERE id=?1",[&claim.account.id])?;
                }
            } else {
                tx.execute(
                    "UPDATE accounts SET last_sync_success=?2,sync_failure=NULL WHERE id=?1",
                    params![claim.account.id, stamp()],
                )?;
            }
        }
        tx.execute("DELETE FROM sync_generations WHERE calendar_id IN (SELECT id FROM calendars WHERE account_id=?1) AND json_extract(payload,'$.owner')=?2",params![claim.account.id,claim.owner])?;
        tx.execute(
            "DELETE FROM account_leases WHERE account_id=?1 AND owner=?2",
            params![claim.account.id, claim.owner],
        )?;
        tx.commit()?;
        Ok(())
    }
}
impl Repository {
    fn publish_incoming(&mut self, claim: &SyncClaim, generation: &str) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        guard(&tx, claim)?;
        let (calendar, json): (String, String) = tx.query_row(
            "SELECT calendar_id,payload FROM sync_generations WHERE id=?1 AND complete=1",
            [generation],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let g: Generation = serde_json::from_str(&json)?;
        if g.owner != claim.owner || g.epoch != claim.epoch {
            return Err(error(
                "syncSuperseded",
                "Synchronization generation expired.",
            ));
        }
        let before = {
            let mut q=tx.prepare("SELECT r.remote_id,i.payload FROM remote_events r JOIN incoming_items i ON i.id=r.id WHERE r.calendar_id=?1")?;
            let values = q
                .query_map([&calendar], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<BTreeMap<_, _>, _>>()?;
            values
        };
        let mut items = if g.full {
            BTreeMap::new()
        } else {
            before.clone()
        };
        for i in &g.items {
            items.insert(i.remote_id.clone(), serde_json::to_string(i)?);
        }
        let changed = before != items;
        // Keep identity mappings even for tombstones/full-resync omissions.
        for (remote, payload) in &before {
            if !items.contains_key(remote) {
                let mut i: IncomingItem = serde_json::from_str(payload)?;
                i.event = None;
                i.unsupported = false;
                items.insert(remote.clone(), serde_json::to_string(&i)?);
            }
        }
        let mut mapped = BTreeMap::new();
        let mut normalized = BTreeMap::new();
        for (remote, payload) in &items {
            let i: IncomingItem = serde_json::from_str(payload)?;
            let id = mapping(
                &tx,
                &claim.account.id,
                &calendar,
                &i,
                claim.adapter.event_capabilities(&i),
            )?;
            tx.execute("INSERT INTO incoming_items(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload",params![id,payload])?;
            mapped.insert(remote.clone(), id.clone());
            normalized.insert(remote.clone(), i);
            if !pending(&tx, &id)? {
                tx.execute("DELETE FROM events WHERE id=?1", [id])?;
            }
        }
        // Masters/ordinary events first, then exceptions, regardless of provider page order.
        for (remote, i) in &normalized {
            if i.master.is_none() && !i.unsupported {
                if let Some(e) = &i.event {
                    let id = &mapped[remote];
                    if !pending(&tx, id)? {
                        let mut e = e.clone();
                        e.id = id.clone();
                        e.calendar_id = calendar.clone();
                        Self::write(&tx, &e)?;
                    }
                }
            }
        }
        for i in normalized.values() {
            if let Some(master) = &i.master {
                if let (Some(id), Some(original)) = (mapped.get(master), &i.original_start) {
                    if !pending(&tx, id)?
                        && normalized.get(master).is_some_and(|m| {
                            m.event.as_ref().is_some_and(|e| e.recurrence.is_some())
                                && !m.unsupported
                        })
                    {
                        let event = i.event.as_ref().map(|e| {
                            let mut e = e.clone();
                            e.id = id.clone();
                            e.calendar_id = calendar.clone();
                            e
                        });
                        tx.execute("INSERT INTO event_exceptions(series_id,original_start,calendar_id,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(series_id,original_start) DO UPDATE SET calendar_id=excluded.calendar_id,payload=excluded.payload",params![id,original,event.as_ref().map(|e|&e.calendar_id),event.as_ref().map(serde_json::to_string).transpose()?])?;
                    }
                }
            }
        }
        // A resource that becomes ordinary/compatible must no longer be removed as a projection.
        for (remote, item) in &normalized {
            if item
                .master
                .as_ref()
                .is_none_or(|m| normalized.get(m).is_some_and(|master| !master.unsupported))
            {
                tx.execute(
                    "DELETE FROM incoming_projections WHERE id=?1",
                    [&mapped[remote]],
                )?;
            }
        }
        // Any feed change invalidates old unsupported-series ranges. Publication is atomic.
        if changed {
            let ids = {
                let mut q=tx.prepare("SELECT p.id FROM incoming_projections p JOIN remote_events r ON r.id=p.id WHERE r.calendar_id=?1")?;
                let v = q
                    .query_map([&calendar], |r| r.get::<_, String>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                v
            };
            for id in ids {
                if !pending(&tx, &id)? {
                    tx.execute("DELETE FROM events WHERE id=?1", [&id])?;
                }
            }
            tx.execute("DELETE FROM coverage WHERE calendar_id=?1", [&calendar])?;
        }
        let old_projections = {
            let mut q=tx.prepare("SELECT p.id,e.payload FROM incoming_projections p JOIN events e ON e.id=p.id WHERE e.calendar_id=?1")?;
            let values = q
                .query_map([&calendar], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            values
        };
        for (id, payload) in old_projections {
            let event: CalendarEvent = serde_json::from_str(&payload)?;
            let overlaps = match event.time {
                EventTime::Timed { start, end, .. } => start < g.range.end && end > g.range.start,
                EventTime::AllDay {
                    start_date,
                    end_date,
                } => start_date < g.range.end_date && end_date > g.range.start_date,
            };
            if overlaps && !pending(&tx, &id)? {
                tx.execute("DELETE FROM events WHERE id=?1", [id])?;
            }
        }
        for i in &g.projections {
            let Some(master) = &i.master else {
                continue;
            };
            if !normalized
                .get(master)
                .is_some_and(|m| m.unsupported && m.event.is_some())
            {
                continue;
            }
            let id = mapping(
                &tx,
                &claim.account.id,
                &calendar,
                i,
                Capabilities::read_only(),
            )?;
            tx.execute("INSERT INTO incoming_projections(id,master_remote_id) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET master_remote_id=excluded.master_remote_id",params![id,master])?;
            if !pending(&tx, &id)? {
                if let Some(e) = &i.event {
                    let mut e = e.clone();
                    e.id = id;
                    e.calendar_id = calendar.clone();
                    e.recurrence = None;
                    e.occurrence = None;
                    Self::write(&tx, &e)?;
                } else {
                    tx.execute("DELETE FROM events WHERE id=?1", [&id])?;
                }
            }
        }
        tx.execute("INSERT INTO coverage(calendar_id,start,end,generation_id) VALUES(?1,?2,?3,?4) ON CONFLICT(calendar_id,start,end) DO UPDATE SET generation_id=excluded.generation_id",params![calendar,g.range.start,g.range.end,generation])?;
        tx.execute("INSERT INTO sync_cursors(calendar_id,cursor,last_success) VALUES(?1,?2,?3) ON CONFLICT(calendar_id) DO UPDATE SET cursor=excluded.cursor,last_success=excluded.last_success",params![calendar,g.cursor,stamp()])?;
        tx.execute("DELETE FROM sync_generations WHERE id=?1", [generation])?;
        tx.commit()?;
        Ok(())
    }
}

impl Repository {
    pub fn sync_status(&self, range: &EventRange) -> Result<Vec<AccountSyncStatus>> {
        let mut statuses = vec![];
        for a in self.accounts()? {
            let (last_success,failure,confirmed):(Option<String>,Option<String>,bool)=self.db.query_row("SELECT last_sync_success,sync_failure,selection_confirmed FROM accounts WHERE id=?1",[&a.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            if !confirmed {
                continue;
            }
            let failure = failure.or(self.db.query_row("SELECT acknowledgement FROM outbox WHERE account_id=?1 AND state IN ('pending','blocked') AND acknowledgement IS NOT NULL ORDER BY rowid LIMIT 1",[&a.id],|r|r.get::<_,Option<String>>(0)).optional()?.flatten());
            let failure = failure
                .map(|f| {
                    let value: serde_json::Value = serde_json::from_str(&f)?;
                    Ok::<_, ServiceError>(error(
                        value["code"].as_str().unwrap_or("sync"),
                        value["message"]
                            .as_str()
                            .unwrap_or("Synchronization failed. Try again."),
                    ))
                })
                .transpose()?;
            let syncing = self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM account_leases WHERE account_id=?1 AND expires_at>?2)",
                params![a.id, stamp()],
                |r| r.get(0),
            )?;
            let pending_count = self.db.query_row(
                "SELECT count(DISTINCT group_id) FROM active_group_series WHERE account_id=?1",
                [&a.id],
                |r| r.get(0),
            )?;
            let conflict_count = self.db.query_row(
                "SELECT count(*) FROM conflicts WHERE account_id=?1",
                [&a.id],
                |r| r.get(0),
            )?;
            let calendars = {
                let mut q=self.db.prepare("SELECT c.id,c.active,s.last_success FROM calendars c LEFT JOIN sync_cursors s ON s.calendar_id=c.id WHERE c.account_id=?1 AND c.selected=1")?;
                let v = q
                    .query_map([&a.id], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, bool>(1)?,
                            r.get::<_, Option<String>>(2)?,
                        ))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                v
            };
            let mut calendar_statuses = vec![];
            for (calendar_id, available, last_success) in calendars {
                let unsupported:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM incoming_items i JOIN remote_events r ON r.id=i.id WHERE r.calendar_id=?1 AND json_extract(i.payload,'$.unsupported')=1 AND json_extract(i.payload,'$.event') IS NOT NULL)",[&calendar_id],|r|r.get(0))?;
                let covered:bool=self.db.query_row("SELECT EXISTS(SELECT 1 FROM coverage WHERE calendar_id=?1 AND julianday(start)<=julianday(?2) AND julianday(end)>=julianday(?3))",params![calendar_id,range.start,range.end],|r|r.get(0))?;
                calendar_statuses.push(CalendarSyncStatus {
                    calendar_id,
                    available,
                    range_complete: last_success.is_some() && (!unsupported || covered),
                    last_success,
                });
            }
            statuses.push(AccountSyncStatus {
                account_id: a.id.clone(),
                syncing,
                last_success,
                failure,
                pending_count,
                conflict_count,
                calendars: calendar_statuses,
                recovery_groups: Some({
                    let mut q = self.db.prepare("SELECT g.id,coalesce(json_extract((SELECT coalesce(after_payload,before_payload) FROM outbox WHERE group_id=g.id ORDER BY step_index LIMIT 1),'$.event.title'),'Google action'),sum(o.state='acknowledged'),count(*),max(o.state='blocked') FROM operation_groups g JOIN outbox o ON o.group_id=g.id WHERE g.account_id=?1 AND coalesce(json_extract(o.acknowledgement,'$.superseded'),0)=0 AND EXISTS(SELECT 1 FROM outbox p WHERE p.group_id=g.id AND p.state!='acknowledged') GROUP BY g.id HAVING count(*)>1 ORDER BY g.rowid")?;
                    let groups = q.query_map([&a.id], |r| Ok(GroupProgress { id:r.get(0)?,title:r.get(1)?,completed_steps:r.get(2)?,total_steps:r.get(3)?,blocked:r.get(4)? }))?.collect::<std::result::Result<Vec<_>,_>>()?;
                    groups
                }),
            });
        }
        Ok(statuses)
    }
}

/// Narrow access boundary lets desktop asynchronous IPC and test fixtures share recovery logic.
pub trait SyncAccess: Send + Sync {
    fn start(&self, full: bool) -> impl std::future::Future<Output = Result<String>> + Send;
    fn fetch(
        &self,
        cursor: Option<String>,
        page: Option<String>,
        projection: bool,
    ) -> impl std::future::Future<Output = Result<IncomingPage>> + Send;
    fn stage(
        &self,
        generation: String,
        page: IncomingPage,
        projection: bool,
    ) -> impl std::future::Future<Output = Result<()>> + Send;
    fn publish(&self, generation: String) -> impl std::future::Future<Output = Result<()>> + Send;
}
/// HTTP 410 restarts staging once, retaining visible cache and queued intent throughout.
pub async fn receive_calendar(access: &impl SyncAccess, cursor: Option<String>) -> Result<()> {
    let mut full = cursor.is_none();
    for attempt in 0..2 {
        let generation = access.start(full).await?;
        let mut expired = false;
        for projection in [false, true] {
            let mut page = None;
            let mut seen = BTreeSet::new();
            loop {
                let result = access
                    .fetch(
                        if full || projection {
                            None
                        } else {
                            cursor.clone()
                        },
                        page.clone(),
                        projection,
                    )
                    .await;
                let incoming = match result {
                    Err(f) if f.code == "cursorExpired" && !full && !projection && attempt == 0 => {
                        expired = true;
                        break;
                    }
                    other => other?,
                };
                page = incoming.next_page.clone();
                if let Some(token) = &page {
                    if token.is_empty() || !seen.insert(token.clone()) || seen.len() > 1000 {
                        return Err(error(
                            "providerIdentity",
                            "Invalid event pagination. Refresh again.",
                        ));
                    }
                }
                access
                    .stage(generation.clone(), incoming, projection)
                    .await?;
                if page.is_none() {
                    break;
                }
            }
            if expired {
                break;
            }
        }
        if expired {
            full = true;
            continue;
        }
        return access.publish(generation).await;
    }
    Err(error(
        "cursorExpired",
        "Google could not complete a full refresh. Try again.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{LocalProvider, ProviderRegistry};
    use std::path::Path;
    struct TestProvider;
    impl Provider for TestProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            ProviderDescriptor {
                id: "test-sync".into(),
                name: "Fixture".into(),
                connection_available: true,
                unavailable_reason: None,
                capabilities: self.capabilities(),
            }
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::read_only()
        }
    }
    fn range() -> EventRange {
        EventRange {
            start: "2026-10-01T00:00:00.000Z".into(),
            end: "2026-11-01T00:00:00.000Z".into(),
            start_date: "2026-10-01".into(),
            end_date: "2026-11-01".into(),
        }
    }
    fn discovered(id: &str, eligible: bool) -> DiscoveredCalendar {
        DiscoveredCalendar {
            remote_id: id.into(),
            name: id.into(),
            color: "#718675".into(),
            description: "".into(),
            access_role: "reader".into(),
            eligible,
            disabled_reason: None,
            selected: true,
        }
    }
    fn service(path: &Path) -> (CalendarService, String) {
        service_with_provider(path, Box::new(TestProvider))
    }
    fn service_with_provider(
        path: &Path,
        provider: Box<dyn Provider>,
    ) -> (CalendarService, String) {
        let mut registry = ProviderRegistry::new(Repository::open(path).unwrap());
        registry.register(Box::new(LocalProvider)).unwrap();
        registry.register(provider).unwrap();
        let account = registry
            .repository
            .upsert_account(
                "test-sync",
                "fixture-subject",
                "Fixture",
                ConnectionState::Connected,
            )
            .unwrap();
        let mut service = CalendarService {
            lifecycle: None,
            providers: registry,
            discoveries: Default::default(),
            recorded_connections: Default::default(),
        };
        let a = service.account(&account).unwrap();
        let d = service
            .prepare_discovery(
                &a,
                vec![discovered("one", true), discovered("excluded", true)],
            )
            .unwrap();
        service.commit_selection(&d.id, vec!["one".into()]).unwrap();
        (service, account)
    }
    fn item(id: &str, title: &str) -> IncomingItem {
        IncomingItem {
            remote_id: id.into(),
            revision: title.into(),
            shadow: format!("{{\"providerOnly\":true,\"title\":\"{title}\"}}"),
            event: Some(CalendarEvent {
                id: "".into(),
                calendar_id: "".into(),
                title: title.into(),
                location: "".into(),
                notes: "".into(),
                recurrence: None,
                occurrence: None,
                time: EventTime::AllDay {
                    start_date: "2026-10-09".into(),
                    end_date: "2026-10-10".into(),
                },
            }),
            master: None,
            original_start: None,
            unsupported: false,
        }
    }
    fn begin(s: &mut CalendarService, a: &str) -> (SyncClaim, CalendarJob) {
        let mut c = s.begin_sync(a, range()).unwrap().unwrap();
        let mut j = s
            .reconcile_sync(
                &mut c,
                vec![discovered("one", true), discovered("excluded", true)],
            )
            .unwrap();
        assert_eq!(j.len(), 1);
        (c, j.remove(0))
    }
    fn stage(
        s: &mut CalendarService,
        c: &SyncClaim,
        j: &CalendarJob,
        items: Vec<IncomingItem>,
        projections: Vec<IncomingItem>,
        full: bool,
    ) -> String {
        let g = s.start_generation(c, j, full).unwrap();
        s.stage_page(
            c,
            &g,
            IncomingPage {
                items,
                next_page: None,
                cursor: Some("next-cursor".into()),
            },
            false,
        )
        .unwrap();
        s.stage_page(
            c,
            &g,
            IncomingPage {
                items: projections,
                next_page: None,
                cursor: None,
            },
            true,
        )
        .unwrap();
        g
    }
    fn events(s: &mut CalendarService) -> Vec<CalendarEvent> {
        s.collection_snapshot(range()).unwrap().events
    }
    struct FullProvider;
    impl Provider for FullProvider {
        fn descriptor(&self) -> ProviderDescriptor {
            TestProvider.descriptor()
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities::read_only()
        }
        fn incremental_sync(&self) -> bool {
            false
        }
    }
    #[test]
    fn full_only_provider_reconciles_without_an_incremental_cursor() {
        let (mut s, a) = service_with_provider(Path::new(":memory:"), Box::new(FullProvider));
        let (c, j) = begin(&mut s, &a);
        let g = s.start_generation(&c, &j, true).unwrap();
        s.stage_page(
            &c,
            &g,
            IncomingPage {
                items: vec![item("a", "Full only")],
                next_page: None,
                cursor: None,
            },
            false,
        )
        .unwrap();
        s.stage_page(
            &c,
            &g,
            IncomingPage {
                items: vec![],
                next_page: None,
                cursor: None,
            },
            true,
        )
        .unwrap();
        s.publish_generation(&c, &g).unwrap();
        s.end_sync(&c, None).unwrap();
        assert_eq!(events(&mut s)[0].title, "Full only");
        let (c, j) = begin(&mut s, &a);
        assert!(j.cursor.is_none());
        let g = stage(&mut s, &c, &j, vec![], vec![], true);
        s.publish_generation(&c, &g).unwrap();
        assert!(events(&mut s).is_empty());
    }
    #[test]
    fn pages_and_cursor_publish_atomically_and_restart_reads_cache() {
        let path = std::env::temp_dir().join(format!("aika-sync-{}.sqlite3", uuid::Uuid::new_v4()));
        let (mut s, a) = service(&path);
        let (c, j) = begin(&mut s, &a);
        let g = s.start_generation(&c, &j, true).unwrap();
        s.stage_page(
            &c,
            &g,
            IncomingPage {
                items: vec![item("a", "A")],
                next_page: Some("page2".into()),
                cursor: None,
            },
            false,
        )
        .unwrap();
        assert!(events(&mut s).is_empty());
        assert!(s.publish_generation(&c, &g).is_err());
        s.stage_page(
            &c,
            &g,
            IncomingPage {
                items: vec![item("b", "B")],
                next_page: None,
                cursor: Some("cursor".into()),
            },
            false,
        )
        .unwrap();
        assert!(s.publish_generation(&c, &g).is_err());
        s.stage_page(
            &c,
            &g,
            IncomingPage {
                items: vec![],
                next_page: None,
                cursor: None,
            },
            true,
        )
        .unwrap();
        s.publish_generation(&c, &g).unwrap();
        s.end_sync(&c, None).unwrap();
        assert_eq!(events(&mut s).len(), 2);
        let ids = events(&mut s)
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>();
        drop(s);
        let mut reopened = CalendarService::open(&path).unwrap();
        assert_eq!(
            events(&mut reopened)
                .iter()
                .map(|e| e.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert!(
            reopened.providers.repository.sync_status(&range()).unwrap()[0].calendars[0]
                .range_complete
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cursor_failure_rolls_back_shadow_effective_and_coverage() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        let g = stage(&mut s, &c, &j, vec![item("a", "Old")], vec![], true);
        s.publish_generation(&c, &g).unwrap();
        let original = events(&mut s);
        let g = stage(&mut s, &c, &j, vec![item("a", "New")], vec![], false);
        s.providers.repository.db.execute_batch("CREATE TRIGGER reject_cursor BEFORE UPDATE ON sync_cursors BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
        assert!(s.publish_generation(&c, &g).is_err());
        assert_eq!(events(&mut s), original);
        let shadow: String = s
            .providers
            .repository
            .db
            .query_row(
                "SELECT shadow_payload FROM remote_events WHERE remote_id='a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(shadow.contains("Old"));
        s.providers
            .repository
            .db
            .execute_batch("DROP TRIGGER reject_cursor")
            .unwrap();
        s.publish_generation(&c, &g).unwrap();
        assert_eq!(events(&mut s)[0].title, "New");
    }
    #[test]
    fn incremental_tombstones_and_failed_full_resync_preserve_pending_intent() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        let g = stage(
            &mut s,
            &c,
            &j,
            vec![item("a", "A"), item("b", "B")],
            vec![],
            true,
        );
        s.publish_generation(&c, &g).unwrap();
        let original = events(&mut s).into_iter().find(|e| e.title == "A").unwrap();
        // Simulate pre-existing durable intent without enabling phase 4 writes.
        let mut local = original.clone();
        local.title = "Pending local intent".into();
        Repository::write(&s.providers.repository.db, &local).unwrap();
        s.providers.repository.db.execute("INSERT INTO outbox(id,account_id,calendar_id,event_id,operation) VALUES('pending',?1,?2,?3,'update')",params![a,j.id,local.id]).unwrap();
        let mut deleted = item("a", "deleted");
        deleted.event = None;
        let g = stage(&mut s, &c, &j, vec![deleted], vec![], false);
        s.publish_generation(&c, &g).unwrap();
        assert_eq!(events(&mut s).len(), 2);
        assert!(events(&mut s).contains(&local));
        let abandoned = s.start_generation(&c, &j, true).unwrap();
        s.stage_page(
            &c,
            &abandoned,
            IncomingPage {
                items: vec![],
                next_page: Some("next".into()),
                cursor: None,
            },
            false,
        )
        .unwrap();
        assert!(s.publish_generation(&c, &abandoned).is_err());
        assert_eq!(events(&mut s).len(), 2);
        let g = stage(&mut s, &c, &j, vec![], vec![], true);
        s.publish_generation(&c, &g).unwrap();
        assert_eq!(events(&mut s), vec![local]);
    }
    #[test]
    fn lease_coalescing_expiry_recovery_and_late_publication() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        assert!(s.begin_sync(&a, range()).unwrap().is_none());
        let g = stage(&mut s, &c, &j, vec![item("a", "A")], vec![], true);
        s.providers
            .repository
            .db
            .execute(
                "UPDATE account_leases SET expires_at='2000-01-01T00:00:00Z'",
                [],
            )
            .unwrap();
        assert!(s.publish_generation(&c, &g).is_err());
        let (c2, j2) = begin(&mut s, &a);
        assert_ne!(c.owner, c2.owner);
        s.end_sync(&c, None).unwrap();
        assert!(s.begin_sync(&a, range()).unwrap().is_none());
        let g = stage(&mut s, &c2, &j2, vec![item("a", "Recovered")], vec![], true);
        s.publish_generation(&c2, &g).unwrap();
        s.end_sync(&c2, None).unwrap();
        assert_eq!(events(&mut s)[0].title, "Recovered");
    }
    #[test]
    fn selection_disconnect_and_reconnect_fence_inflight_results() {
        for disconnect in [false, true] {
            let (mut s, a) = service(Path::new(":memory:"));
            let (c, j) = begin(&mut s, &a);
            let g = stage(&mut s, &c, &j, vec![item("a", "A")], vec![], true);
            if disconnect {
                s.record_disconnect(&a).unwrap();
                s.providers
                    .repository
                    .upsert_account(
                        "test-sync",
                        "fixture-subject",
                        "Reconnect",
                        ConnectionState::Connected,
                    )
                    .unwrap();
            } else {
                s.providers.repository.delete_calendar(&j.id).unwrap();
            }
            assert_eq!(
                s.publish_generation(&c, &g).unwrap_err().code,
                "syncSuperseded"
            );
            assert!(events(&mut s).is_empty());
            s.end_sync(&c, None).unwrap();
        }
    }
    #[test]
    fn reconciliation_adds_new_preserves_exclusions_visibility_and_unavailable_cache() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        let g = stage(&mut s, &c, &j, vec![item("a", "Cached")], vec![], true);
        s.publish_generation(&c, &g).unwrap();
        s.end_sync(&c, None).unwrap();
        s.providers
            .repository
            .set_calendar_visibility(&j.id, false)
            .unwrap();
        let mut c = s.begin_sync(&a, range()).unwrap().unwrap();
        let jobs = s
            .reconcile_sync(
                &mut c,
                vec![
                    discovered("one", false),
                    discovered("excluded", true),
                    discovered("new", true),
                ],
            )
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].remote_id, "new");
        assert!(s
            .providers
            .repository
            .get(&events_payload_id(&s, &j.id))
            .unwrap()
            .is_some());
        assert!(
            !s.providers.repository.sync_status(&range()).unwrap()[0]
                .calendars
                .iter()
                .find(|c| c.calendar_id == j.id)
                .unwrap()
                .available
        );
        s.end_sync(&c, None).unwrap();
        let mut c = s.begin_sync(&a, range()).unwrap().unwrap();
        s.reconcile_sync(
            &mut c,
            vec![
                discovered("one", true),
                discovered("excluded", true),
                discovered("new", true),
            ],
        )
        .unwrap();
        assert!(s
            .collection_snapshot(range())
            .unwrap()
            .hidden_calendar_ids
            .contains(&j.id));
        assert_eq!(events(&mut s)[0].title, "Cached");
    }
    fn events_payload_id(s: &CalendarService, c: &str) -> String {
        s.providers
            .repository
            .db
            .query_row("SELECT id FROM events WHERE calendar_id=?1", [c], |r| {
                r.get(0)
            })
            .unwrap()
    }
    #[test]
    fn projections_coverage_and_invalidation_publish_together() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        let mut master = item("master", "Unsupported");
        master.unsupported = true;
        let mut occurrence = item("instance", "Projected");
        occurrence.master = Some("master".into());
        occurrence.original_start = Some("2026-10-09".into());
        let g = stage(
            &mut s,
            &c,
            &j,
            vec![master.clone()],
            vec![occurrence.clone()],
            true,
        );
        s.publish_generation(&c, &g).unwrap();
        assert_eq!(events(&mut s)[0].title, "Projected");
        let outside = EventRange {
            start: "2040-10-01T00:00:00Z".into(),
            end: "2040-11-01T00:00:00Z".into(),
            start_date: "2040-10-01".into(),
            end_date: "2040-11-01".into(),
        };
        assert!(
            !s.providers.repository.sync_status(&outside).unwrap()[0].calendars[0].range_complete
        );
        master.revision = "changed".into();
        let g = stage(&mut s, &c, &j, vec![master], vec![], false);
        s.publish_generation(&c, &g).unwrap();
        assert!(events(&mut s).is_empty());
        assert!(
            s.providers.repository.sync_status(&range()).unwrap()[0].calendars[0].range_complete
        );
        let mut dead = occurrence;
        dead.event = None;
        let g = stage(&mut s, &c, &j, vec![], vec![dead], false);
        s.publish_generation(&c, &g).unwrap();
        assert!(events(&mut s).is_empty());
    }
    #[test]
    fn compatible_series_moved_exception_and_sparse_cancellation_are_order_independent() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        let mut master = item("series", "Daily");
        master.event.as_mut().unwrap().recurrence = Some(Recurrence {
            frequency: RepeatFrequency::Daily,
            interval: 1,
            weekdays: vec![],
            monthly_on: MonthlyOn::Date,
            until: None,
            count: Some(3),
            segment: None,
        });
        let mut moved = item("moved", "Moved");
        moved.master = Some("series".into());
        moved.original_start = Some("2026-10-10".into());
        moved.event.as_mut().unwrap().time = EventTime::AllDay {
            start_date: "2026-10-15".into(),
            end_date: "2026-10-16".into(),
        };
        let mut cancelled = item("cancelled", "");
        cancelled.master = Some("series".into());
        cancelled.original_start = Some("2026-10-11".into());
        cancelled.event = None;
        let g = stage(&mut s, &c, &j, vec![cancelled, moved, master], vec![], true);
        s.publish_generation(&c, &g).unwrap();
        let events = events(&mut s);
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].title, "Moved");
        assert_eq!(
            events[1].occurrence.as_ref().unwrap().original_start,
            "2026-10-10"
        );
    }
    #[test]
    fn checked_calendar_removal_fences_staged_incoming_and_preserves_exclusion() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        let g = stage(&mut s, &c, &j, vec![item("a", "Late")], vec![], true);
        let review = s.review_calendar_removal(&j.id).unwrap();
        assert_eq!(review.pending_count, 0);
        s.discard_and_remove_calendar(review).unwrap();
        assert_eq!(
            s.publish_generation(&c, &g).unwrap_err().code,
            "syncSuperseded"
        );
        s.end_sync(&c, None).unwrap();
        let mut next = s.begin_sync(&a, range()).unwrap().unwrap();
        let jobs = s
            .reconcile_sync(
                &mut next,
                vec![discovered("one", true), discovered("excluded", true)],
            )
            .unwrap();
        assert!(!jobs.iter().any(|job| job.id == j.id));
        assert!(events(&mut s).is_empty());
    }

    #[test]
    fn leases_and_selection_fences_work_across_independent_database_connections() {
        let path =
            std::env::temp_dir().join(format!("aika-sync-race-{}.sqlite3", uuid::Uuid::new_v4()));
        let (mut first, a) = service(&path);
        let (c, j) = begin(&mut first, &a);
        let mut registry = ProviderRegistry::new(Repository::open(&path).unwrap());
        registry.register(Box::new(LocalProvider)).unwrap();
        registry.register(Box::new(TestProvider)).unwrap();
        let mut second = CalendarService {
            lifecycle: None,
            providers: registry,
            discoveries: Default::default(),
            recorded_connections: Default::default(),
        };
        assert!(second.begin_sync(&a, range()).unwrap().is_none());
        let g = stage(&mut first, &c, &j, vec![item("a", "Late")], vec![], true);
        second.providers.repository.delete_calendar(&j.id).unwrap();
        assert_eq!(
            first.publish_generation(&c, &g).unwrap_err().code,
            "syncSuperseded"
        );
        first.end_sync(&c, None).unwrap();
        assert!(second.begin_sync(&a, range()).unwrap().is_some());
        drop(first);
        drop(second);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn metadata_tracks_remote_changes_while_preserving_local_overrides() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        s.end_sync(&c, None).unwrap();
        let mut renamed = discovered("one", true);
        renamed.name = "Remote rename".into();
        renamed.color = "#123456".into();
        let mut c = s.begin_sync(&a, range()).unwrap().unwrap();
        s.reconcile_sync(&mut c, vec![renamed.clone(), discovered("excluded", true)])
            .unwrap();
        assert_eq!(
            s.providers
                .repository
                .calendars()
                .unwrap()
                .iter()
                .find(|c| c.id == j.id)
                .unwrap()
                .name,
            "Remote rename"
        );
        s.end_sync(&c, None).unwrap();
        let original = s
            .providers
            .repository
            .calendars()
            .unwrap()
            .into_iter()
            .find(|c| c.id == j.id)
            .unwrap();
        s.providers
            .repository
            .update_calendar(
                &j.id,
                CalendarDetails {
                    name: "My label".into(),
                    color: original.color.clone(),
                    description: original.description.clone(),
                },
                original,
            )
            .unwrap();
        renamed.name = "Another remote rename".into();
        let mut c = s.begin_sync(&a, range()).unwrap().unwrap();
        s.reconcile_sync(&mut c, vec![renamed, discovered("excluded", true)])
            .unwrap();
        assert_eq!(
            s.providers
                .repository
                .calendars()
                .unwrap()
                .iter()
                .find(|c| c.id == j.id)
                .unwrap()
                .name,
            "My label"
        );
    }
    struct MockAccess {
        service: std::sync::Mutex<CalendarService>,
        claim: SyncClaim,
        job: CalendarJob,
        fail_full: bool,
        repeated_page: bool,
        starts: std::sync::Mutex<Vec<bool>>,
    }
    impl SyncAccess for MockAccess {
        async fn start(&self, full: bool) -> Result<String> {
            self.starts.lock().unwrap().push(full);
            self.service
                .lock()
                .unwrap()
                .start_generation(&self.claim, &self.job, full)
        }
        async fn fetch(
            &self,
            cursor: Option<String>,
            page: Option<String>,
            projection: bool,
        ) -> Result<IncomingPage> {
            // Provider calls do not hold the service lock, and staged data is still invisible.
            assert_eq!(
                events(&mut self.service.try_lock().unwrap())[0].title,
                "Old"
            );
            if projection {
                return Ok(IncomingPage {
                    items: vec![],
                    next_page: None,
                    cursor: None,
                });
            }
            if cursor.is_some() && page.is_some() {
                return Err(error("cursorExpired", "Cursor expired."));
            }
            if cursor.is_none() && page.is_some() && self.fail_full {
                return Err(error("network", "Offline during resync."));
            }
            Ok(IncomingPage {
                items: vec![item(
                    if page.is_none() { "first" } else { "second" },
                    if cursor.is_some() {
                        "Unpublished"
                    } else {
                        "Recovered"
                    },
                )],
                next_page: if page.is_none() || self.repeated_page {
                    Some("page2".into())
                } else {
                    None
                },
                cursor: page.is_some().then(|| "recovered-cursor".into()),
            })
        }
        async fn stage(&self, g: String, p: IncomingPage, projection: bool) -> Result<()> {
            self.service
                .lock()
                .unwrap()
                .stage_page(&self.claim, &g, p, projection)
        }
        async fn publish(&self, g: String) -> Result<()> {
            self.service
                .lock()
                .unwrap()
                .publish_generation(&self.claim, &g)
        }
    }
    fn ready<T>(future: impl std::future::Future<Output = T>) -> T {
        let mut future = std::pin::pin!(future);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            std::task::Poll::Ready(v) => v,
            std::task::Poll::Pending => panic!("fixture unexpectedly blocked"),
        }
    }
    #[test]
    fn actual_coordinator_recovers_410_after_partial_page_and_preserves_cache_on_failed_resync() {
        for fail_full in [false, true] {
            let (mut s, a) = service(Path::new(":memory:"));
            let (c, j) = begin(&mut s, &a);
            let g = stage(&mut s, &c, &j, vec![item("old", "Old")], vec![], true);
            s.publish_generation(&c, &g).unwrap();
            let access = MockAccess {
                service: std::sync::Mutex::new(s),
                claim: c,
                job: j,
                fail_full,
                repeated_page: false,
                starts: Default::default(),
            };
            let result = ready(receive_calendar(&access, Some("expired-cursor".into())));
            assert_eq!(*access.starts.lock().unwrap(), vec![false, true]);
            let mut s = access.service.lock().unwrap();
            if fail_full {
                assert_eq!(result.unwrap_err().code, "network");
                assert_eq!(events(&mut s)[0].title, "Old");
            } else {
                result.unwrap();
                assert_eq!(events(&mut s).len(), 2);
                assert!(events(&mut s).iter().all(|e| e.title == "Recovered"));
            }
        }
    }
    #[test]
    fn actual_coordinator_rejects_repeated_page_tokens_without_publication() {
        let (mut s, a) = service(Path::new(":memory:"));
        let (c, j) = begin(&mut s, &a);
        let g = stage(&mut s, &c, &j, vec![item("old", "Old")], vec![], true);
        s.publish_generation(&c, &g).unwrap();
        let access = MockAccess {
            service: std::sync::Mutex::new(s),
            claim: c,
            job: j,
            fail_full: false,
            repeated_page: true,
            starts: Default::default(),
        };
        assert_eq!(
            ready(receive_calendar(&access, None)).unwrap_err().code,
            "providerIdentity"
        );
        assert_eq!(events(&mut access.service.lock().unwrap())[0].title, "Old");
    }
    #[test]
    fn unconfirmed_connection_never_auto_imports_and_failures_are_redacted_and_isolated() {
        let (mut s, a) = service(Path::new(":memory:"));
        let other = s
            .providers
            .repository
            .upsert_account("test-sync", "other", "Other", ConnectionState::Connected)
            .unwrap();
        assert!(s.begin_sync(&other, range()).unwrap().is_none());
        let (c, _) = begin(&mut s, &a);
        s.end_sync(
            &c,
            Some(error(
                "credentialUnavailable",
                "Unlock the credential store.",
            )),
        )
        .unwrap();
        let status = s.providers.repository.sync_status(&range()).unwrap();
        assert_eq!(
            status[0].failure.as_ref().unwrap().code,
            "credentialUnavailable"
        );
        assert_eq!(
            s.account(&a).unwrap().connection_state,
            ConnectionState::Connected
        );
        let (c, _) = begin(&mut s, &a);
        s.end_sync(
            &c,
            Some(error("authenticationRequired", "Reconnect this account.")),
        )
        .unwrap();
        assert_eq!(
            s.account(&a).unwrap().connection_state,
            ConnectionState::AuthenticationRequired
        );
        assert_eq!(
            s.account(&other).unwrap().connection_state,
            ConnectionState::Connected
        );
    }
}

#[cfg(test)]
mod migration_tests {
    use super::*;
    #[test]
    fn v4_upgrade_is_transactional_and_preserves_local_data() {
        let path = std::env::temp_dir().join(format!("aika-v5-{}.sqlite3", uuid::Uuid::new_v4()));
        let repo = Repository::open(&path).unwrap();
        repo.db.execute_batch("ALTER TABLE outbox DROP COLUMN meeting_intent; DROP TRIGGER fence_removing_account; ALTER TABLE accounts DROP COLUMN removal_in_progress; DROP TABLE identity_lifecycles; DROP TABLE lifecycle_clock; DROP VIEW active_group_series; DROP TRIGGER outbox_group_scope; DROP TRIGGER outbox_group_scope_update; DROP TRIGGER outbox_group_member; DROP INDEX outbox_group_order; ALTER TABLE outbox DROP COLUMN group_id; ALTER TABLE outbox DROP COLUMN step_index; ALTER TABLE outbox DROP COLUMN resource_id; ALTER TABLE outbox DROP COLUMN series_remote_id; ALTER TABLE outbox DROP COLUMN original_start; DROP TABLE operation_group_series; DROP TABLE operation_groups; DROP TRIGGER invalidate_new_calendar_sync; DROP TRIGGER invalidate_selection_sync; DROP TRIGGER invalidate_account_sync; DROP TABLE incoming_items; DROP TABLE incoming_projections; ALTER TABLE calendars DROP COLUMN remote_metadata; ALTER TABLE accounts DROP COLUMN selection_confirmed; ALTER TABLE accounts DROP COLUMN sync_epoch; ALTER TABLE accounts DROP COLUMN sync_failure; ALTER TABLE accounts DROP COLUMN last_sync_success; PRAGMA user_version=4; CREATE TABLE incoming_items(marker TEXT); INSERT INTO incoming_items VALUES('preserve');").unwrap();
        drop(repo);
        assert!(Repository::open(&path).is_err());
        let db = Connection::open(&path).unwrap();
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            4
        );
        assert_eq!(
            db.query_row("SELECT marker FROM incoming_items", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "preserve"
        );
        db.execute_batch("DROP TABLE incoming_items").unwrap();
        drop(db);
        let repo = Repository::open(&path).unwrap();
        assert_eq!(repo.calendars().unwrap()[0].id, "local-default");
        assert_eq!(
            repo.db
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))
                .unwrap(),
            9
        );
        drop(repo);
        std::fs::remove_file(path).unwrap();
    }
}
