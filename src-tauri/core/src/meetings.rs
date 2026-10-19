//! Native meeting contracts and checked durable organizer intent. No HTTP in mutations.
use crate::{
    contracts::*,
    repository::Repository,
    service::{error, CalendarService},
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;
use sha2::{Digest, Sha256};
type Result<T> = std::result::Result<T, ServiceError>;

pub(crate) fn shadow(db: &Connection, target: &str) -> Result<Value> {
    let id = target.split_once('@').map_or(target, |(id, _)| id);
    let raw: Option<String> = if let Some((_, original)) = target.split_once('@') {
        db.query_row("SELECT shadow_payload FROM remote_events WHERE id=(SELECT id FROM incoming_items WHERE json_extract(payload,'$.master')=(SELECT remote_id FROM remote_events WHERE id=?1) AND json_extract(payload,'$.original_start')=?2 LIMIT 1)", params![id,original], |r| r.get(0)).optional()?
    } else {
        None
    };
    let raw = raw.or(db
        .query_row(
            "SELECT shadow_payload FROM remote_events WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?);
    Ok(raw
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or(Value::Null))
}
pub(crate) fn has_conference(value: &Value) -> bool {
    value.get("conferenceData").is_some() || value.get("hangoutLink").is_some()
}
pub(crate) fn guests(value: &Value) -> Vec<MeetingGuest> {
    value["attendees"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| {
            Some(MeetingGuest {
                email: a["email"].as_str()?.into(),
                optional: a["optional"].as_bool().unwrap_or(false),
            })
        })
        .collect()
}
pub(crate) fn details(db: &Connection, target: &str) -> Result<MeetingDetails> {
    let value = shadow(db, target)?;
    let id = target.split_once('@').map_or(target, |(id, _)| id);
    let mut digest = Sha256::new();
    digest.update(value.to_string());
    let mut attendees: Vec<_> = guests(&value)
        .into_iter()
        .map(|g| MeetingAttendee {
            response: value["attendees"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|a| a["email"].as_str() == Some(&g.email))
                .and_then(|a| a["responseStatus"].as_str())
                .unwrap_or("needsAction")
                .into(),
            email: g.email,
            optional: g.optional,
        })
        .collect();
    let mut q = db.prepare("SELECT o.id,o.meeting_intent FROM outbox o JOIN active_group_series a ON a.group_id=o.group_id AND a.event_id=o.event_id WHERE o.event_id=?1 AND (o.original_start IS NULL OR o.original_start=?2) ORDER BY o.rowid")?;
    let rows = q
        .query_map(
            params![id, target.split_once('@').map(|(_, key)| key)],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (op, raw) in rows {
        digest.update(op);
        if let Some(raw) = raw {
            digest.update(&raw);
            let intent: MeetingIntent = serde_json::from_str(&raw)?;
            if let Some(list) = intent.guests {
                attendees = list
                    .into_iter()
                    .map(|g| MeetingAttendee {
                        response: attendees
                            .iter()
                            .find(|a| a.email.eq_ignore_ascii_case(&g.email))
                            .map(|a| a.response.clone())
                            .unwrap_or("needsAction".into()),
                        email: g.email,
                        optional: g.optional,
                    })
                    .collect();
            }
        }
    }
    let mut links = vec![];
    if let Some(link) = value["hangoutLink"]
        .as_str()
        .filter(|s| s.starts_with("https://"))
    {
        links.push(link.into());
    }
    for entry in value["conferenceData"]["entryPoints"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(link) = entry["uri"].as_str().filter(|s| s.starts_with("https://")) {
            if !links.iter().any(|v| v == link) {
                links.push(link.into());
            }
        }
    }
    Ok(MeetingDetails {
        organizer: value["organizer"]["email"].as_str().map(str::to_owned),
        organizer_owned: value["organizer"]["self"].as_bool() == Some(true)
            || value.is_null()
            || value.as_object().is_some_and(|o| o.is_empty()),
        attendees,
        attendees_complete: value["attendeesOmitted"].as_bool() != Some(true),
        conference_links: links,
        revision: format!("{:x}", digest.finalize()),
    })
}
impl CalendarService {
    pub fn meeting_details(&self, target: &str) -> Result<MeetingDetails> {
        let db = &self.providers.repository.db;
        let tx = db.unchecked_transaction()?;
        let result = details(&tx, target)?;
        tx.commit()?;
        Ok(result)
    }
    pub fn mutate_meeting(&mut self, mut request: MeetingMutation) -> Result<Vec<CalendarEvent>> {
        if request.notifications == NotificationPolicy::None && !request.acknowledge_silent {
            return Err(error("notificationWarning", "Silent updates can prevent calendar propagation or lose invitations. Acknowledge this before choosing none."));
        }
        if let Some(list) = &request.guests {
            let mut emails = std::collections::HashSet::new();
            for guest in list {
                let email = guest.email.trim();
                if email != guest.email
                    || email.len() > 254
                    || email.chars().any(char::is_whitespace)
                    || email.split('@').count() != 2
                    || email.starts_with('@')
                    || email.ends_with('@')
                    || !emails.insert(email.to_ascii_lowercase())
                {
                    return Err(error(
                        "validation",
                        "Use a unique valid email address for each guest.",
                    ));
                }
            }
        }
        let repo = &mut self.providers.repository;
        let tx = repo
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = details(&tx, &request.target)?;
        if current.revision != request.expected_revision {
            return Err(error(
                "conflict",
                "Meeting details changed. Reload before editing.",
            ));
        }
        let id = request
            .target
            .split_once('@')
            .map_or(request.target.as_str(), |(id, _)| id);
        let before = Repository::read_series(&tx, id)?;
        if request.expected != before {
            return Err(error(
                "conflict",
                "The event changed. Reload before editing.",
            ));
        }
        if before.is_none() {
            request.event = request
                .event
                .map(crate::repository::validate_event)
                .transpose()?;
        }
        let source = before
            .as_ref()
            .map(|s| &s.event)
            .or(request.event.as_ref())
            .ok_or_else(|| error("notFound", "No meeting to change."))?;
        let google: bool = tx.query_row(
            "SELECT provider_id='google' AND account_id IS NOT NULL FROM calendars WHERE id=?1",
            [&source.calendar_id],
            |r| r.get(0),
        )?;
        if !google {
            return Err(error("unsupported", "Meetings require a Google calendar."));
        }
        if before.is_some() && !current.organizer_owned {
            return Err(error(
                "readOnly",
                "Only the organizer can edit this meeting.",
            ));
        }
        let provider = shadow(&tx, &request.target)?;
        if request.guests.is_some() && !current.attendees_complete {
            return Err(error(
                "readOnly",
                "Sync complete Google attendee details before changing guests.",
            ));
        }
        if (request.scope == MutationScope::Following && request.event.is_some())
            && (has_conference(&provider) || has_conference(&shadow(&tx, id)?))
        {
            return Err(error("unsupported", "This action would copy a conference to another event. Create a new event without a conference instead."));
        }
        if let Some(event) = &request.event {
            if event.calendar_id != source.calendar_id || (before.is_none() && event.id != id) {
                return Err(error(
                    "unsupported",
                    "Meeting identity and calendar must remain unchanged.",
                ));
            }
        }
        if request.scope == MutationScope::Occurrence {
            let (_, original) = request
                .target
                .split_once('@')
                .ok_or_else(|| error("validation", "Select a recurring occurrence."))?;
            Repository::require_cached_occurrence(&tx, id, original)?;
        }
        let mut changes = if let Some(before) = &before {
            if request.scope == MutationScope::Following && !request.reset_exceptions {
                return Err(error(
                    "exceptionsResetRequired",
                    "Confirm resetting future Google exceptions.",
                ));
            }
            let mutation = EventMutation {
                target: request.target.clone(),
                scope: request.scope.clone(),
                event: request.event.clone(),
                expected: aika_domain::recurrence::expectation(before)?,
                reset_exceptions: request.reset_exceptions,
                count_changed: request.count_changed,
                exception_policy: Some(if request.scope == MutationScope::Following {
                    ExceptionPolicy::Reset
                } else {
                    ExceptionPolicy::Preserve
                }),
            };
            aika_domain::recurrence::plan_mutation(
                before,
                &mutation,
                &uuid::Uuid::new_v4().to_string(),
            )?
        } else {
            vec![SeriesChange {
                id: id.into(),
                before: None,
                after: request.event.clone().map(|event| EventSeries {
                    event,
                    exceptions: vec![],
                }),
            }]
        };
        if request.scope == MutationScope::Series {
            if let (Some(b), Some(a)) = (&before, &changes[0].after) {
                if b.event.recurrence.is_some()
                    && (b.event.time != a.event.time || b.event.recurrence != a.event.recurrence)
                {
                    return Err(error(
                        "unsupported",
                        "Recurring master schedule changes cannot preserve all Google exceptions.",
                    ));
                }
            }
        }
        if request.scope == MutationScope::Following {
            crate::google_recurrence::canonicalize_split(&mut changes)?;
        }
        let watermark: i64 =
            tx.query_row("SELECT coalesce(max(rowid),0) FROM outbox", [], |r| {
                r.get(0)
            })?;
        Repository::apply_changes_with_schedule(
            &tx,
            &changes,
            false,
            request.scope == MutationScope::Following,
        )?;
        let intent = MeetingIntent {
            original_start: if request.scope == MutationScope::Occurrence {
                request.target.split_once('@').map(|(_, key)| key.into())
            } else {
                None
            },
            before_guests: Some(
                current
                    .attendees
                    .iter()
                    .map(|a| MeetingGuest {
                        email: a.email.clone(),
                        optional: a.optional,
                    })
                    .collect(),
            ),
            guests: request.guests,
            notifications: request.notifications,
        };
        if let Some(original) = &intent.original_start {
            let mut q = tx.prepare("SELECT id FROM outbox WHERE rowid>?1 AND event_id=?2")?;
            let operations = q
                .query_map(params![watermark, id], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for operation in operations {
                bind_occurrence(&tx, &operation, id, original)?;
            }
        }
        tx.execute(
            "UPDATE outbox SET meeting_intent=?2 WHERE rowid>?1",
            params![watermark, serde_json::to_string(&intent)?],
        )?;
        if request.scope == MutationScope::Following {
            let head_intent = MeetingIntent {
                original_start: None,
                guests: None,
                ..intent.clone()
            };
            tx.execute("UPDATE outbox SET meeting_intent=?3 WHERE rowid>?1 AND event_id=?2 AND operation!='create'", params![watermark,id,serde_json::to_string(&head_intent)?])?;
        }
        for tail in changes
            .iter()
            .filter(|c| c.before.is_none() && before.is_some())
        {
            tx.execute("INSERT INTO remote_events(id,account_id,calendar_id,remote_id,revision,shadow_payload,capabilities) SELECT ?1,r.account_id,r.calendar_id,(SELECT resource_id FROM outbox WHERE event_id=?1 AND operation='create' ORDER BY rowid DESC LIMIT 1),NULL,r.shadow_payload,r.capabilities FROM remote_events r WHERE r.id=?2", params![tail.id,id])?;
        }
        tx.commit()?;
        let result = changes
            .iter()
            .filter_map(|c| c.after.as_ref().map(|s| s.event.clone()))
            .collect();
        repo.remember_meeting(changes, request.target, request.scope, intent);
        Ok(result)
    }
}

/// Fold intent separately for master and each actual occurrence resource.
pub(crate) fn pending_intents(db: &Connection, event: &str) -> Result<Vec<MeetingIntent>> {
    let mut q = db.prepare("SELECT original_start,meeting_intent FROM outbox WHERE event_id=?1 AND state!='acknowledged' AND meeting_intent IS NOT NULL ORDER BY rowid")?;
    let rows = q
        .query_map([event], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut result: Vec<MeetingIntent> = vec![];
    for (original, raw) in rows {
        let mut next: MeetingIntent = serde_json::from_str(&raw)?;
        next.original_start = original;
        if let Some(existing) = result
            .iter_mut()
            .find(|m| m.original_start == next.original_start)
        {
            if next.guests.is_none() {
                next.guests = existing.guests.clone();
            }
            *existing = next;
        } else {
            result.push(next);
        }
    }
    Ok(result)
}
/// Guest-only occurrence edits have no recurrence-domain difference to replan.
/// Preserve their resource scope explicitly when replacing conflict operations.
pub(crate) fn replan_intents(
    db: &Connection,
    event: &str,
    watermark: i64,
    intents: &[MeetingIntent],
) -> Result<()> {
    let mut q = db.prepare(
        "SELECT id,original_start FROM outbox WHERE event_id=?1 AND rowid>?2 ORDER BY step_index",
    )?;
    let mut rows = q
        .query_map(params![event, watermark], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if rows.is_empty() {
        return Ok(());
    }
    let template = rows[0].0.clone();
    for intent in intents {
        let mut intent = intent.clone();
        // Keep-local is a checked review of the newly observed remote guest list.
        let selector = intent
            .original_start
            .as_ref()
            .map(|o| format!("{event}@{o}"))
            .unwrap_or_else(|| event.into());
        intent.before_guests = Some(guests(&shadow(db, &selector)?));
        let found = rows
            .iter()
            .find(|(_, original)| *original == intent.original_start)
            .map(|(id, _)| id.clone());
        let id = if let Some(id) = found {
            id
        } else if intent.guests.is_some() {
            // Repurpose an otherwise empty placeholder created for guest-only intent.
            let empty_master: Option<String> = if !intents
                .iter()
                .any(|i| i.original_start.is_none())
            {
                db.query_row("SELECT id FROM outbox WHERE event_id=?1 AND rowid>?2 AND original_start IS NULL AND before_payload=after_payload LIMIT 1", params![event,watermark],|r|r.get(0)).optional()?
            } else {
                None
            };
            if let Some(id) = empty_master {
                id
            } else {
                let id = uuid::Uuid::new_v4().to_string();
                db.execute("INSERT INTO outbox(id,account_id,calendar_id,event_id,operation,before_payload,after_payload,group_id,step_index) SELECT ?1,account_id,calendar_id,event_id,'update',before_payload,after_payload,group_id,(SELECT max(step_index)+1 FROM outbox p WHERE p.group_id=outbox.group_id) FROM outbox WHERE id=?2",params![id,template])?;
                rows.push((id.clone(), intent.original_start.clone()));
                id
            }
        } else {
            continue;
        };
        if let Some(original) = &intent.original_start {
            bind_occurrence(db, &id, event, original)?;
            if let Some(row) = rows.iter_mut().find(|(r, _)| r == &id) {
                row.1 = Some(original.clone());
            }
        }
        if intent.original_start.is_none() {
            db.execute("UPDATE outbox SET expected_revision=(SELECT revision FROM remote_events WHERE id=?2) WHERE id=?1 AND operation!='create'",params![id,event])?;
        }
        db.execute(
            "UPDATE outbox SET meeting_intent=?2 WHERE id=?1",
            params![id, serde_json::to_string(&intent)?],
        )?;
    }
    Ok(())
}

/// Keep the persisted create identity when an occurrence depends on a recreated master.
pub(crate) fn bind_occurrence(
    db: &Connection,
    operation: &str,
    event: &str,
    original: &str,
) -> Result<()> {
    db.execute("UPDATE outbox SET original_start=?3,series_remote_id=coalesce((SELECT resource_id FROM outbox p WHERE p.event_id=?2 AND p.operation='create' AND p.state!='acknowledged' ORDER BY p.rowid DESC LIMIT 1),(SELECT remote_id FROM remote_events WHERE id=?2)),resource_id=NULL,expected_revision=NULL WHERE id=?1",params![operation,event,original])?;
    Ok(())
}
