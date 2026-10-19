//! Participant-only intent. Organizer capabilities never authorize an RSVP.
use crate::{
    contracts::*,
    repository::Repository,
    service::{error, CalendarService},
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;
type Result<T> = std::result::Result<T, ServiceError>;

/// `self` identifies a calendar copy, not necessarily the authenticated user.
/// Require the verified OAuth email AND its own calendar copy, refusing delegates.
pub(crate) fn attendee<'a>(value: &'a Value, email: &str) -> Option<&'a Value> {
    if value["status"] == "cancelled"
        || value["locked"] == true
        || value["eventType"].as_str().unwrap_or("default") != "default"
        || value["organizer"]["self"] == true
    {
        return None;
    }
    let mut entries = value["attendees"]
        .as_array()?
        .iter()
        .filter(|a| a["self"] == true);
    let found = entries.next()?;
    if entries.next().is_some()
        || found["organizer"] == true
        || found["resource"] == true
        || !found["email"]
            .as_str()
            .is_some_and(|e| e.eq_ignore_ascii_case(email))
    {
        None
    } else {
        Some(found)
    }
}
pub(crate) fn response_status<'a>(value: &'a Value, email: &str) -> Option<&'a str> {
    attendee(value, email).map(|a| a["responseStatus"].as_str().unwrap_or("needsAction"))
}
pub(crate) fn available(db: &Connection, target: &str, value: &Value) -> Result<Option<String>> {
    let id = target.split_once('@').map_or(target, |(id, _)| id);
    let projection: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM incoming_projections WHERE id=?1)",
        [id],
        |r| r.get(0),
    )?;
    if projection {
        return Ok(None);
    }
    if Repository::read_series(db, id)?
        .is_none_or(|s| crate::google_recurrence::compile(&s.event).is_err())
    {
        return Ok(None);
    }
    let email: Option<String> = db.query_row("SELECT a.display_name FROM remote_events r JOIN calendars c ON c.id=r.calendar_id JOIN accounts a ON a.id=c.account_id WHERE r.id=?1 AND a.provider_id='google' AND c.provider_id='google' AND c.selected=1 AND c.active=1 AND c.writable=1 AND a.removal_in_progress=0 AND lower(c.remote_id)=lower(a.display_name)", [id], |r| r.get(0)).optional()?;
    Ok(email.filter(|email| attendee(value, email).is_some()))
}
fn queue(
    db: &Connection,
    target: &str,
    scope: &MutationScope,
    response: InvitationResponseIntent,
) -> Result<(Vec<SeriesChange>, MeetingIntent)> {
    let id = target.split_once('@').map_or(target, |(id, _)| id);
    let series = Repository::read_series(db, id)?
        .ok_or_else(|| error("notFound", "Invitation no longer exists."))?;
    crate::google_recurrence::compile(&series.event)?;
    let original = if *scope == MutationScope::Occurrence {
        if series.event.recurrence.is_none() {
            return Err(error(
                "unsupported",
                "Select an occurrence of a supported recurring invitation.",
            ));
        }
        let (_, key) = target
            .split_once('@')
            .ok_or_else(|| error("validation", "Select an occurrence."))?;
        if crate::outgoing::occurrence_event(&series, key)?.is_none() {
            return Err(error(
                "readOnly",
                "Cancelled invitations cannot receive responses.",
            ));
        }
        Some(key.to_string())
    } else {
        None
    };
    let changes = vec![SeriesChange {
        id: id.into(),
        before: Some(series.clone()),
        after: Some(series),
    }];
    let group = uuid::Uuid::new_v4().to_string();
    Repository::queue_change(db, &changes[0], false, &group)?;
    let intent = MeetingIntent {
        response: Some(response),
        original_start: original,
        before_guests: None,
        guests: None,
        notifications: NotificationPolicy::All,
    };
    let operation: String =
        db.query_row("SELECT id FROM outbox WHERE group_id=?1", [&group], |r| {
            r.get(0)
        })?;
    if let Some(original) = &intent.original_start {
        crate::meetings::bind_occurrence(db, &operation, id, original)?;
    }
    db.execute(
        "UPDATE outbox SET meeting_intent=?2 WHERE id=?1",
        params![operation, serde_json::to_string(&intent)?],
    )?;
    Ok((changes, intent))
}
impl CalendarService {
    pub fn respond_to_invitation(&mut self, request: InvitationResponseMutation) -> Result<()> {
        if request.scope == MutationScope::Following {
            return Err(error(
                "unsupported",
                "Invitation responses support occurrence or whole series only.",
            ));
        }
        let repo = &mut self.providers.repository;
        let tx = repo
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Whole-series review always uses master attendee state, even from an occurrence.
        let target = if request.scope == MutationScope::Series {
            request
                .target
                .split_once('@')
                .map_or(request.target.as_str(), |(id, _)| id)
        } else {
            &request.target
        };
        let details = crate::meetings::details(&tx, target)?;
        if details.revision != request.expected_revision {
            return Err(error(
                "conflict",
                "Invitation changed. Reload before responding.",
            ));
        }
        let value = crate::meetings::shadow(&tx, target)?;
        let email = available(&tx, target, &value)?.ok_or_else(|| {
            error(
                "readOnly",
                "Respond from the authenticated attendee's own writable calendar copy.",
            )
        })?;
        let before = details
            .response
            .ok_or_else(|| error("readOnly", "Invitation response is unavailable."))?
            .current;
        let (changes, intent) = queue(
            &tx,
            target,
            &request.scope,
            InvitationResponseIntent {
                action_id: uuid::Uuid::new_v4().to_string(),
                email,
                before,
                desired: request.response.as_str().into(),
            },
        )?;
        tx.commit()?;
        repo.remember_meeting(changes, target.into(), request.scope, intent);
        Ok(())
    }
}
pub(crate) fn undo(
    db: &Connection,
    target: &str,
    scope: &MutationScope,
    intent: &MeetingIntent,
    _: &[SeriesChange],
) -> Result<()> {
    let response = intent.response.as_ref().unwrap();
    let details = crate::meetings::details(db, target)?;
    let value = crate::meetings::shadow(db, target)?;
    if available(db, target, &value)?.as_deref() != Some(&response.email)
        || details.response.as_ref().map(|r| r.current.as_str()) != Some(&response.desired)
    {
        return Err(error(
            "conflict",
            "The invitation response changed. Undo cannot overwrite it.",
        ));
    }
    let id = target.split_once('@').map_or(target, |(id, _)| id);
    let last: Option<(String, String, u32, String)> = db.query_row("SELECT group_id,state,attempts,meeting_intent FROM outbox WHERE event_id=?1 ORDER BY rowid DESC LIMIT 1", [id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if let Some((group, state, attempts, raw)) = last {
        let latest: MeetingIntent = serde_json::from_str(&raw)?;
        if latest.original_start != intent.original_start
            || latest.response.as_ref().is_none_or(|r| {
                r.action_id != response.action_id
                    || r.email != response.email
                    || r.desired != response.desired
            })
        {
            return Err(error(
                "conflict",
                "Another meeting action superseded this response.",
            ));
        }
        if state == "pending" && attempts == 0 {
            db.execute("DELETE FROM outbox WHERE group_id=?1", [&group])?;
            db.execute("DELETE FROM operation_groups WHERE id=?1", [&group])?;
            return Ok(());
        }
        if state != "acknowledged" {
            return Err(error(
                "groupRecoveryRequired",
                "Reconcile the attempted response before undoing it.",
            ));
        }
    }
    queue(
        db,
        target,
        scope,
        InvitationResponseIntent {
            action_id: uuid::Uuid::new_v4().to_string(),
            email: response.email.clone(),
            before: response.desired.clone(),
            desired: response.before.clone(),
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_input_rejects_attendee_identity_and_unknown_response_choices() {
        for extra in [
            serde_json::json!({"email":"someone@example.invalid"}),
            serde_json::json!({"attendees":[]}),
        ] {
            let mut value = serde_json::json!({"target":"event","scope":"series","expectedRevision":"revision","response":"accepted"});
            value
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            assert!(serde_json::from_value::<InvitationResponseMutation>(value).is_err());
        }
        assert!(serde_json::from_value::<InvitationResponseMutation>(serde_json::json!({"target":"event","scope":"series","expectedRevision":"revision","response":"needsAction"})).is_err());
    }
}
