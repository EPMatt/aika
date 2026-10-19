//! Google conditional event/recurrence patching and recovery; unknown provider fields never enter a PATCH.
use super::*;
use crate::{
    outgoing::{OutgoingOperation, OutgoingResult},
    sync::IncomingItem,
};
use serde_json::{json, Map, Value};

pub(super) fn editable(item: &IncomingItem) -> bool {
    item.master.is_none()
        && item.original_start.is_none()
        && item.event.is_some()
        && editable_fields(item)
}
pub(super) fn editable_fields(item: &IncomingItem) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(&item.shadow) else {
        return false;
    };
    !item.unsupported
        && item
            .event
            .as_ref()
            .is_none_or(|e| crate::google_recurrence::compile(e).is_ok())
        && v.get("attendees").is_none()
        && v.get("attendeesOmitted").and_then(Value::as_bool) != Some(true)
        && v.get("locked").and_then(Value::as_bool) != Some(true)
        && v.get("eventType")
            .and_then(Value::as_str)
            .unwrap_or("default")
            == "default"
        && v.get("conferenceData").is_none()
        && v.get("organizer")
            .and_then(|o| o.get("self"))
            .and_then(Value::as_bool)
            != Some(false)
}
fn fields(e: &CalendarEvent) -> Value {
    let (start, end) = match &e.time {
        EventTime::AllDay {
            start_date,
            end_date,
        } => (json!({"date":start_date}), json!({"date":end_date})),
        EventTime::Timed {
            start,
            end,
            time_zone,
        } => (
            json!({"dateTime":start,"timeZone":time_zone}),
            json!({"dateTime":end,"timeZone":time_zone}),
        ),
    };
    let mut value = json!({"summary":e.title,"location":e.location,"description":e.notes,"start":start,"end":end});
    if let Ok(Some(rule)) = crate::google_recurrence::compile(e) {
        value["recurrence"] = json!([rule]);
    }
    value
}
fn patch(op: &OutgoingOperation) -> Value {
    let Some(after) = &op.after else {
        return json!({});
    };
    let all = fields(&after.event);
    let Some(before) = &op.before else {
        return all;
    };
    let old = fields(&before.event);
    let mut patch = Map::new();
    for key in ["summary", "location", "description"] {
        if old[key] != all[key] {
            patch.insert(key.into(), all[key].clone());
        }
    }
    if before.event.recurrence != after.event.recurrence {
        patch.insert(
            "recurrence".into(),
            all.get("recurrence").cloned().unwrap_or(json!([])),
        );
    }
    if before.event.time != after.event.time {
        // Explicit null clears the other time representation on all-day/timed conversion.
        for key in ["start", "end"] {
            let mut value = all[key].clone();
            if value.get("date").is_some() {
                value["dateTime"] = Value::Null;
                value["timeZone"] = Value::Null;
            } else {
                value["date"] = Value::Null;
            }
            patch.insert(key.into(), value);
        }
    }
    Value::Object(patch)
}
fn normalize(value: Value) -> Result<Option<IncomingItem>> {
    if value["status"] == "cancelled" {
        return Ok(None);
    }
    let mut page = events::normalized(events::EventPage {
        items: vec![value],
        next_page_token: None,
        next_sync_token: None,
        time_zone: "UTC".into(),
    })?;
    Ok(page.items.pop())
}
fn achieved(op: &OutgoingOperation, item: &IncomingItem) -> bool {
    let Some(after) = &op.after else {
        return false;
    };
    let Some(remote) = &item.event else {
        return false;
    };
    let before = op.before.as_ref().map(|s| &s.event);
    let after = &after.event;
    (before.is_some_and(|b| b.title == after.title) || remote.title == after.title)
        && (before.is_some_and(|b| b.location == after.location)
            || remote.location == after.location)
        && (before.is_some_and(|b| b.notes == after.notes) || remote.notes == after.notes)
        && (before.is_some_and(|b| b.time == after.time) || remote.time == after.time)
        && (before.is_some_and(|b| b.recurrence == after.recurrence)
            || remote.recurrence == after.recurrence)
}
pub(super) fn deliver(
    transport: &dyn GoogleTransport,
    access: &str,
    op: &OutgoingOperation,
) -> Result<OutgoingResult> {
    if let Some(after) = &op.after {
        crate::google_recurrence::compile(&after.event)?;
    }
    let instance = op.original_start.is_some();
    let mut delivery = op.clone();
    if let Some(original) = &op.original_start {
        let master = op
            .series_remote_id
            .as_ref()
            .ok_or_else(|| error("providerIdentity", "Missing recurring master identity."))?;
        let current_master = transport
            .event_request(access, &op.remote_calendar, master, "GET", None, None)?
            .map(normalize)
            .transpose()?
            .flatten();
        if !current_master.as_ref().is_some_and(editable) {
            return Err(error(
                "readOnly",
                "The Google recurring master is read-only or no longer available.",
            ));
        }
        for (source, destination) in [
            (&op.before, &mut delivery.before),
            (&op.after, &mut delivery.after),
        ] {
            *destination = source
                .as_ref()
                .map(|s| crate::outgoing::occurrence_event(s, original))
                .transpose()?
                .flatten()
                .map(|event| EventSeries {
                    event,
                    exceptions: vec![],
                });
        }
        delivery.operation = "update".into();
    }
    let op = &delivery;
    let normalize_response = |value| {
        if instance {
            let page = events::normalized(events::EventPage {
                items: vec![value],
                next_page_token: None,
                next_sync_token: None,
                time_zone: "UTC".into(),
            })?;
            Ok(page.items.into_iter().next())
        } else {
            normalize(value)
        }
    };
    let get = || {
        transport
            .event_request(
                access,
                &op.remote_calendar,
                &op.remote_id,
                "GET",
                None,
                None,
            )
            .and_then(|v| v.map(normalize_response).transpose().map(Option::flatten))
    };
    let current = get()?;
    if let Some(item) = &current {
        if instance
            && (item.master != op.series_remote_id || item.original_start != op.original_start)
        {
            return Err(error(
                "providerIdentity",
                "Google returned a different occurrence.",
            ));
        }
        if item.remote_id != op.remote_id {
            return Err(error(
                "providerIdentity",
                "Google returned a different resource.",
            ));
        }
        if instance && op.after.is_none() && item.event.is_none() {
            return Ok(OutgoingResult::Applied(current));
        }
        if !(if instance {
            editable_fields(item)
        } else {
            editable(item)
        }) || item.revision.is_empty()
        {
            return Err(error(
                "readOnly",
                "This Google event is read-only. Review its pending changes.",
            ));
        }
    }
    if op.recovering
        && current.as_ref().is_some_and(|i| {
            let shadow: Value = serde_json::from_str(&i.shadow).unwrap_or_default();
            shadow["extendedProperties"]["private"]["aikaOperation"] == op.id
                && (achieved(op, i) || (instance && op.after.is_none() && i.event.is_none()))
        })
    {
        return Ok(OutgoingResult::Applied(current));
    }
    if op.operation == "delete" && current.is_none() {
        return Ok(OutgoingResult::Applied(None));
    }
    if op.operation == "create" {
        if current.is_some() {
            return Ok(OutgoingResult::Conflict(current));
        }
        // Recreating a deleted resource retains its last live provider-only fields.
        // Raw tombstones live in incoming_items; the mapping keeps the last live shadow.
        let shadow: Value = serde_json::from_str(&op.shadow).unwrap_or_default();
        let mut body = patch(op);
        for key in [
            "reminders",
            "colorId",
            "transparency",
            "visibility",
            "extendedProperties",
            "source",
            "attachments",
            "guestsCanInviteOthers",
            "guestsCanModify",
            "guestsCanSeeOtherGuests",
            "anyoneCanAddSelf",
        ] {
            if let Some(value) = shadow.get(key) {
                body[key] = value.clone();
            }
        }
        if !body["extendedProperties"].is_object() {
            body["extendedProperties"] = json!({});
        }
        if !body["extendedProperties"]["private"].is_object() {
            body["extendedProperties"]["private"] = json!({});
        }
        body["extendedProperties"]["private"]["aikaOperation"] = json!(op.id);
        body["id"] = Value::String(op.remote_id.clone());
        return match transport.event_request(
            access,
            &op.remote_calendar,
            &op.remote_id,
            "POST",
            Some(&body),
            None,
        ) {
            Ok(Some(v)) => Ok(OutgoingResult::Applied(normalize(v)?)),
            Ok(None) => Err(error(
                "retryable",
                "Google did not acknowledge the event. Retry to recover it.",
            )),
            Err(f) if f.code == "remoteConflict" => Ok(OutgoingResult::Conflict(get()?)),
            Err(f) => Err(f),
        };
    }
    if current.as_ref().map(|i| i.revision.as_str()) != op.revision.as_deref() || current.is_none()
    {
        return Ok(OutgoingResult::Conflict(current));
    }
    let mut body = patch(op);
    if instance {
        body["status"] = json!(if op.after.is_some() {
            "confirmed"
        } else {
            "cancelled"
        });
    }
    let shadow: Value = current
        .as_ref()
        .and_then(|i| serde_json::from_str(&i.shadow).ok())
        .unwrap_or_default();
    let mut private = shadow["extendedProperties"]["private"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    private.insert("aikaOperation".into(), Value::String(op.id.clone()));
    body["extendedProperties"] = json!({"private":private});
    match transport.event_request(
        access,
        &op.remote_calendar,
        &op.remote_id,
        if op.operation == "delete" {
            "DELETE"
        } else {
            "PATCH"
        },
        Some(&body),
        op.revision.as_deref(),
    ) {
        Ok(value) => {
            if op.operation != "delete" && value.is_none() {
                return Ok(OutgoingResult::Conflict(None));
            }
            Ok(OutgoingResult::Applied(
                value.map(normalize_response).transpose()?.flatten(),
            ))
        }
        Err(f) if f.code == "remoteConflict" => Ok(OutgoingResult::Conflict(get()?)),
        Err(f) => Err(f),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    type RecordedRequest = (String, Option<String>, Option<Value>, String);
    struct Transport {
        responses: Mutex<std::collections::VecDeque<Result<Option<Value>>>>,
        requests: Mutex<Vec<RecordedRequest>>,
    }
    impl Transport {
        fn new(responses: Vec<Result<Option<Value>>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                requests: Mutex::new(vec![]),
            }
        }
    }
    impl GoogleTransport for Transport {
        fn browser(&self, _: &Url) -> Result<()> {
            unreachable!()
        }
        fn token(&self, _: &ClientConfig, _: Vec<(&str, String)>) -> Result<TokenResponse> {
            unreachable!()
        }
        fn identity(&self, _: &str) -> Result<Identity> {
            unreachable!()
        }
        fn calendar_page(&self, _: &str, _: Option<&str>) -> Result<CalendarPage> {
            unreachable!()
        }
        fn event_request(
            &self,
            _: &str,
            _: &str,
            resource: &str,
            method: &str,
            body: Option<&Value>,
            revision: Option<&str>,
        ) -> Result<Option<Value>> {
            self.requests.lock().unwrap().push((
                method.into(),
                revision.map(str::to_owned),
                body.cloned(),
                resource.into(),
            ));
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request")
        }
    }
    fn resource(title: &str, revision: &str) -> Value {
        json!({"id":"remote","etag":revision,"summary":title,"start":{"date":"2026-10-01"},"end":{"date":"2026-10-02"},"extendedProperties":{"private":{"anotherApp":"preserve"}},"reminders":{"useDefault":true}})
    }
    fn operation(kind: &str, recovering: bool) -> OutgoingOperation {
        let mut before = normalize(resource("Before", "r1"))
            .unwrap()
            .unwrap()
            .event
            .unwrap();
        before.id = uuid::Uuid::new_v4().to_string();
        before.calendar_id = "calendar".into();
        let mut after = before.clone();
        after.title = "After".into();
        OutgoingOperation {
            id: "durable-op".into(),
            event_id: before.id.clone(),
            calendar_id: before.calendar_id.clone(),
            remote_calendar: "calendar".into(),
            remote_id: "remote".into(),
            operation: kind.into(),
            before: if kind == "create" {
                None
            } else {
                Some(EventSeries {
                    event: before,
                    exceptions: vec![],
                })
            },
            after: if kind == "delete" {
                None
            } else {
                Some(EventSeries {
                    event: after,
                    exceptions: vec![],
                })
            },
            revision: if kind == "create" {
                None
            } else {
                Some("r1".into())
            },
            recovering,
            shadow: "{}".into(),
            original_start: None,
            series_remote_id: None,
        }
    }
    #[test]
    fn successful_create_lost_response_is_recovered_without_duplicate() {
        let op = operation("create", true);
        let mut remote = resource("After", "r2");
        remote["extendedProperties"]["private"]["aikaOperation"] = json!(op.id);
        let transport = Transport::new(vec![Ok(Some(remote))]);
        assert!(matches!(
            deliver(&transport, "access", &op).unwrap(),
            OutgoingResult::Applied(Some(_))
        ));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }
    #[test]
    fn delete_compensation_restores_provider_only_fields_without_read_only_metadata() {
        let mut op = operation("create", false);
        let mut original = resource("Before", "r1");
        original["visibility"] = json!("private");
        op.shadow = original.to_string();
        let transport = Transport::new(vec![Ok(None), Ok(Some(resource("After", "r2")))]);
        deliver(&transport, "access", &op).unwrap();
        let requests = transport.requests.lock().unwrap();
        let body = requests[1].2.as_ref().unwrap();
        assert_eq!(body["reminders"]["useDefault"], true);
        assert_eq!(body["visibility"], "private");
        assert_eq!(
            body["extendedProperties"]["private"]["anotherApp"],
            "preserve"
        );
        assert_eq!(
            body["extendedProperties"]["private"]["aikaOperation"],
            op.id
        );
        assert_eq!(body["summary"], "After");
        assert!(body.get("etag").is_none());
    }
    #[test]
    fn update_is_conditional_and_preserves_unedited_and_private_fields() {
        let op = operation("update", false);
        let transport = Transport::new(vec![
            Ok(Some(resource("Before", "r1"))),
            Ok(Some(resource("After", "r2"))),
        ]);
        assert!(matches!(
            deliver(&transport, "access", &op).unwrap(),
            OutgoingResult::Applied(Some(_))
        ));
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[1].0, "PATCH");
        assert_eq!(requests[1].1.as_deref(), Some("r1"));
        let body = requests[1].2.as_ref().unwrap();
        assert_eq!(body["summary"], "After");
        assert!(body.get("description").is_none());
        assert!(body.get("reminders").is_none());
        assert_eq!(
            body["extendedProperties"]["private"]["anotherApp"],
            "preserve"
        );
        assert_eq!(
            body["extendedProperties"]["private"]["aikaOperation"],
            op.id
        );
    }
    #[test]
    fn revision_mismatch_and_racing_412_keep_remote_version() {
        for racing in [false, true] {
            let op = operation("update", false);
            let responses = if racing {
                vec![
                    Ok(Some(resource("Before", "r1"))),
                    Err(error("remoteConflict", "Changed")),
                    Ok(Some(resource("External", "r3"))),
                ]
            } else {
                vec![Ok(Some(resource("External", "r3")))]
            };
            let transport = Transport::new(responses);
            let OutgoingResult::Conflict(Some(item)) = deliver(&transport, "access", &op).unwrap()
            else {
                panic!("expected conflict")
            };
            assert_eq!(item.event.unwrap().title, "External");
            assert_eq!(item.revision, "r3");
        }
    }
    #[test]
    fn lost_update_requires_persisted_operation_marker() {
        for marker in [false, true] {
            let op = operation("update", true);
            let mut remote = resource("After", "r2");
            if marker {
                remote["extendedProperties"]["private"]["aikaOperation"] = json!(op.id);
            }
            let transport = Transport::new(vec![Ok(Some(remote))]);
            let result = deliver(&transport, "access", &op).unwrap();
            assert_eq!(matches!(result, OutgoingResult::Applied(_)), marker);
            assert_eq!(transport.requests.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn insert_persists_requested_identity_and_delete_is_conditional() {
        let create = operation("create", false);
        let transport = Transport::new(vec![Ok(None), Ok(Some(resource("After", "r1")))]);
        deliver(&transport, "access", &create).unwrap();
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[1].0, "POST");
        assert_eq!(requests[1].2.as_ref().unwrap()["id"], "remote");
        drop(requests);
        let delete = operation("delete", false);
        let transport = Transport::new(vec![Ok(Some(resource("Before", "r1"))), Ok(None)]);
        assert!(matches!(
            deliver(&transport, "access", &delete).unwrap(),
            OutgoingResult::Applied(None)
        ));
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[1].0, "DELETE");
        assert_eq!(requests[1].1.as_deref(), Some("r1"));
        drop(requests);
        let transport = Transport::new(vec![Ok(None)]);
        assert!(matches!(
            deliver(&transport, "access", &delete).unwrap(),
            OutgoingResult::Applied(None)
        ));
    }
    #[test]
    fn meetings_special_types_and_unsupported_recurrence_never_reach_write_transport() {
        for (key, value) in [
            ("attendees", json!([{ "email":"fixture@example.invalid" }])),
            ("eventType", json!("outOfOffice")),
            ("recurrence", json!(["RRULE:FREQ=HOURLY"])),
            ("locked", json!(true)),
            ("conferenceData", json!({"conferenceId":"fixture"})),
        ] {
            let mut remote = resource("Before", "r1");
            remote[key] = value;
            let transport = Transport::new(vec![Ok(Some(remote))]);
            let result = deliver(&transport, "access", &operation("update", false));
            assert_eq!(result.err().unwrap().code, "readOnly");
            assert_eq!(transport.requests.lock().unwrap().len(), 1);
        }
    }
    #[test]
    fn patch_keeps_unknown_fields_and_clears_old_time_kind() {
        let before = CalendarEvent {
            id: uuid::Uuid::new_v4().to_string(),
            calendar_id: "local-default".into(),
            title: "before".into(),
            location: "Room".into(),
            notes: "Notes".into(),
            time: EventTime::AllDay {
                start_date: "2026-10-01".into(),
                end_date: "2026-10-02".into(),
            },
            recurrence: None,
            occurrence: None,
        };
        let mut after = before.clone();
        after.title = "after".into();
        after.time = EventTime::Timed {
            start: "2026-10-01T10:00:00Z".into(),
            end: "2026-10-01T11:00:00Z".into(),
            time_zone: "UTC".into(),
        };
        let op = OutgoingOperation {
            id: "op".into(),
            event_id: before.id.clone(),
            calendar_id: before.calendar_id.clone(),
            remote_calendar: "remote".into(),
            remote_id: "id".into(),
            operation: "update".into(),
            before: Some(EventSeries {
                event: before,
                exceptions: vec![],
            }),
            after: Some(EventSeries {
                event: after,
                exceptions: vec![],
            }),
            revision: Some("etag".into()),
            recovering: false,
            shadow: "{}".into(),
            original_start: None,
            series_remote_id: None,
        };
        let p = patch(&op);
        assert_eq!(p["summary"], "after");
        assert!(p.get("location").is_none());
        assert!(p.get("attendees").is_none());
        assert!(p["start"]["date"].is_null());
    }
    fn daily() -> Recurrence {
        Recurrence {
            frequency: RepeatFrequency::Daily,
            interval: 2,
            weekdays: vec![],
            monthly_on: MonthlyOn::Date,
            until: None,
            count: Some(6),
            segment: None,
        }
    }
    fn recurring_resource(title: &str, revision: &str) -> Value {
        let mut v = resource(title, revision);
        v["recurrence"] = json!(["RRULE:FREQ=DAILY;INTERVAL=2;COUNT=6"]);
        v
    }
    fn instance_resource(title: &str, revision: &str, cancelled: bool) -> Value {
        let mut v = resource(title, revision);
        v["id"] = json!("actual-moved-instance");
        v["recurringEventId"] = json!("remote");
        v["originalStartTime"] = json!({"date":"2026-10-03"});
        v["status"] = json!(if cancelled { "cancelled" } else { "confirmed" });
        v["start"] = json!({"date":"2026-10-07"});
        v["end"] = json!({"date":"2026-10-08"});
        v
    }
    fn occurrence_operation(cancelled_before: bool, cancelled_after: bool) -> OutgoingOperation {
        let mut op = operation("update", false);
        let mut before = op.before.take().unwrap();
        before.event.recurrence = Some(daily());
        let mut replacement = before.event.clone();
        replacement.recurrence = None;
        replacement.time = EventTime::AllDay {
            start_date: "2026-10-07".into(),
            end_date: "2026-10-08".into(),
        };
        before.exceptions = vec![EventException {
            original_start: "2026-10-03".into(),
            event: if cancelled_before {
                None
            } else {
                Some(replacement.clone())
            },
        }];
        let mut after = before.clone();
        replacement.title = "After".into();
        after.exceptions[0].event = if cancelled_after {
            None
        } else {
            Some(replacement)
        };
        op.before = Some(before);
        op.after = Some(after);
        op.remote_id = "actual-moved-instance".into();
        op.original_start = Some("2026-10-03".into());
        op.series_remote_id = Some("remote".into());
        op
    }
    #[test]
    fn recurring_create_compiles_rule_and_metadata_patch_preserves_remote_fields() {
        let mut create = operation("create", false);
        create.after.as_mut().unwrap().event.recurrence = Some(daily());
        let t = Transport::new(vec![Ok(None), Ok(Some(recurring_resource("After", "r2")))]);
        assert!(matches!(
            deliver(&t, "access", &create).unwrap(),
            OutgoingResult::Applied(Some(_))
        ));
        assert_eq!(
            t.requests.lock().unwrap()[1].2.as_ref().unwrap()["recurrence"],
            json!(["RRULE:FREQ=DAILY;INTERVAL=2;COUNT=6"])
        );
        let mut op = operation("update", false);
        op.before.as_mut().unwrap().event.recurrence = Some(daily());
        op.after.as_mut().unwrap().event.recurrence = Some(daily());
        let t = Transport::new(vec![
            Ok(Some(recurring_resource("Before", "r1"))),
            Ok(Some(recurring_resource("After", "r2"))),
        ]);
        deliver(&t, "access", &op).unwrap();
        let requests = t.requests.lock().unwrap();
        let patch = requests[1].2.as_ref().unwrap();
        assert_eq!(patch["summary"], "After");
        for key in ["recurrence", "start", "end", "reminders", "attendees"] {
            assert!(patch.get(key).is_none(), "{key}");
        }
    }
    #[test]
    fn moved_occurrence_cancel_restore_and_lost_responses_use_actual_identity() {
        for (before, after) in [(false, false), (false, true), (true, false)] {
            let op = occurrence_operation(before, after);
            let t = Transport::new(vec![
                Ok(Some(recurring_resource("Before", "master-etag"))),
                Ok(Some(instance_resource("Before", "r1", before))),
                Ok(Some(instance_resource("After", "r2", after))),
            ]);
            let OutgoingResult::Applied(Some(item)) = deliver(&t, "access", &op).unwrap() else {
                panic!("expected applied")
            };
            assert_eq!(item.remote_id, "actual-moved-instance");
            assert_eq!(item.event.is_none(), after);
            let requests = t.requests.lock().unwrap();
            assert_eq!(requests[0].3, "remote");
            assert_eq!(requests[1].3, "actual-moved-instance");
            assert_eq!(requests[2].3, "actual-moved-instance");
            assert_eq!(requests[2].0, "PATCH");
            assert_eq!(requests[2].1.as_deref(), Some("r1"));
            assert_eq!(
                requests[2].2.as_ref().unwrap()["status"],
                if after { "cancelled" } else { "confirmed" }
            );
            drop(requests);
            let mut recovered = op.clone();
            recovered.recovering = true;
            let mut remote = instance_resource("After", "r2", after);
            remote["extendedProperties"]["private"]["aikaOperation"] = json!(op.id);
            let t = Transport::new(vec![
                Ok(Some(recurring_resource("Before", "master-etag"))),
                Ok(Some(remote)),
            ]);
            assert!(matches!(
                deliver(&t, "access", &recovered).unwrap(),
                OutgoingResult::Applied(Some(_))
            ));
            assert_eq!(t.requests.lock().unwrap().len(), 2);
        }
    }
    #[test]
    fn split_trim_and_tail_recover_lost_http_responses_without_repeating_writes() {
        let mut trim = operation("update", false);
        let rule = Recurrence {
            frequency: RepeatFrequency::Daily,
            interval: 2,
            weekdays: vec![],
            monthly_on: MonthlyOn::Date,
            until: None,
            count: Some(6),
            segment: None,
        };
        trim.before.as_mut().unwrap().event.recurrence = Some(rule.clone());
        trim.after.as_mut().unwrap().event.recurrence = Some(Recurrence {
            count: Some(2),
            ..rule
        });
        let mut original = resource("Before", "r1");
        original["recurrence"] = json!(["RRULE:FREQ=DAILY;INTERVAL=2;COUNT=6"]);
        let mut applied = resource("After", "r2");
        applied["recurrence"] = json!(["RRULE:FREQ=DAILY;INTERVAL=2;COUNT=2"]);
        applied["extendedProperties"]["private"]["aikaOperation"] = json!(trim.id);
        let transport = Transport::new(vec![
            Ok(Some(original)),
            Err(error("network", "Lost trim response")),
        ]);
        assert_eq!(
            deliver(&transport, "access", &trim).err().unwrap().code,
            "network"
        );
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[1].0, "PATCH");
        assert_eq!(requests[1].1.as_deref(), Some("r1"));
        assert_eq!(
            requests[1].2.as_ref().unwrap()["recurrence"],
            applied["recurrence"]
        );
        assert!(requests[1].2.as_ref().unwrap().get("start").is_none());
        drop(requests);
        trim.recovering = true;
        let transport = Transport::new(vec![Ok(Some(applied.clone()))]);
        assert!(matches!(
            deliver(&transport, "access", &trim).unwrap(),
            OutgoingResult::Applied(Some(_))
        ));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
        let mut tail = trim.clone();
        tail.id = "tail-operation".into();
        tail.operation = "create".into();
        tail.before = None;
        tail.revision = None;
        tail.recovering = false;
        applied["extendedProperties"]["private"]["aikaOperation"] = json!(tail.id);
        let transport = Transport::new(vec![Ok(None), Err(error("network", "Lost tail response"))]);
        assert_eq!(
            deliver(&transport, "access", &tail).err().unwrap().code,
            "network"
        );
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests[1].0, "POST");
        assert_eq!(requests[1].2.as_ref().unwrap()["id"], tail.remote_id);
        drop(requests);
        tail.recovering = true;
        let transport = Transport::new(vec![Ok(Some(applied))]);
        assert!(matches!(
            deliver(&transport, "access", &tail).unwrap(),
            OutgoingResult::Applied(Some(_))
        ));
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }
}
