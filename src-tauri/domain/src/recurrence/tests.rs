use super::*;
fn event(start: &str, frequency: RepeatFrequency) -> CalendarEvent {
    CalendarEvent {
        id: "7e89b8f8-16f5-4bf1-bf66-33b3963f70c4".into(),
        calendar_id: "local-default".into(),
        title: "Repeat".into(),
        location: "".into(),
        notes: "".into(),
        time: EventTime::AllDay {
            start_date: start.into(),
            end_date: (date(start).unwrap() + Duration::days(1)).to_string(),
        },
        recurrence: Some(Recurrence {
            frequency,
            interval: 1,
            weekdays: vec![],
            monthly_on: MonthlyOn::Date,
            until: None,
            count: Some(5),
            segment: None,
        }),
        occurrence: None,
    }
}
#[test]
fn common_patterns_and_calendar_skips() {
    let starts = |event| {
        preview(event)
            .unwrap()
            .iter()
            .map(|e| boundary(&e.time).to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        starts(event("2026-01-31", RepeatFrequency::Monthly)),
        [
            "2026-01-31",
            "2026-03-31",
            "2026-05-31",
            "2026-07-31",
            "2026-08-31"
        ]
    );
    assert_eq!(
        starts(event("2024-02-29", RepeatFrequency::Yearly)),
        ["2024-02-29", "2028-02-29", "2032-02-29"]
    );
    let mut weekly = event("2026-10-05", RepeatFrequency::Weekly);
    let rule = weekly.recurrence.as_mut().unwrap();
    rule.weekdays = vec![1, 3];
    rule.interval = 2;
    assert_eq!(
        starts(weekly),
        [
            "2026-10-05",
            "2026-10-07",
            "2026-10-19",
            "2026-10-21",
            "2026-11-02"
        ]
    );
    let mut monthly = event("2026-01-26", RepeatFrequency::Monthly);
    monthly.recurrence.as_mut().unwrap().monthly_on = MonthlyOn::Weekday;
    assert_eq!(
        starts(monthly),
        [
            "2026-01-26",
            "2026-02-23",
            "2026-03-23",
            "2026-04-27",
            "2026-05-25"
        ]
    );
}
#[test]
fn split_keeps_phase_count_and_requires_exception_reset() {
    let master = event("2026-10-05", RepeatFrequency::Weekly);
    let series = EventSeries {
        event: master.clone(),
        exceptions: vec![],
    };
    let selected = get_occurrence(&series, "2026-10-19").unwrap();
    let request = EventMutation {
        target: selected.id.clone(),
        scope: MutationScope::Following,
        event: Some(CalendarEvent {
            title: "Tail".into(),
            ..selected
        }),
        expected: expectation(&series).unwrap(),
        reset_exceptions: false,
        count_changed: false,
    };
    let changes = plan_mutation(&series, &request, "ab8bb795-b9c7-47c6-98da-3547021948c5").unwrap();
    assert_eq!(
        preview(changes[0].after.as_ref().unwrap().event.clone())
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        preview(changes[1].after.as_ref().unwrap().event.clone())
            .unwrap()
            .len(),
        3
    );
    let changed = EventSeries {
        exceptions: vec![EventException {
            original_start: "2026-10-26".into(),
            event: None,
        }],
        ..series
    };
    let request = EventMutation {
        expected: expectation(&changed).unwrap(),
        ..request
    };
    assert_eq!(
        plan_mutation(&changed, &request, "ab8bb795-b9c7-47c6-98da-3547021948c5")
            .unwrap_err()
            .code,
        "exceptionsResetRequired"
    );
}
#[test]
fn dst_skips_gaps_counts_valid_starts_and_keeps_duration() {
    let mut master = event("2026-03-28", RepeatFrequency::Daily);
    master.time = EventTime::Timed {
        start: "2026-03-28T01:30:00.000Z".into(),
        end: "2026-03-28T02:30:00.000Z".into(),
        time_zone: "Europe/Zurich".into(),
    };
    let values = preview(master).unwrap();
    assert_eq!(boundary(&values[1].time), "2026-03-30T00:30:00.000Z");
    assert_eq!(values.len(), 5);
    let EventTime::Timed { start, end, .. } = &values[1].time else {
        panic!()
    };
    assert_eq!(
        instant(end).unwrap() - instant(start).unwrap(),
        Duration::hours(1)
    );
}
#[test]
fn count_and_until_are_mutually_exclusive() {
    let mut master = event("2026-01-01", RepeatFrequency::Daily);
    master.recurrence.as_mut().unwrap().until = Some("2026-01-03".into());
    assert!(preview(master.clone()).is_err());
    master.recurrence.as_mut().unwrap().count = None;
    assert_eq!(preview(master).unwrap().len(), 3);
}

#[test]
fn explicit_seed_fold_and_generated_fold_have_stable_identities() {
    let mut master = event("2026-10-25", RepeatFrequency::Daily);
    master.time = EventTime::Timed {
        start: "2026-10-25T01:30:00.000Z".into(),
        end: "2026-10-25T02:30:00.000Z".into(),
        time_zone: "Europe/Zurich".into(),
    };
    assert_eq!(
        boundary(&preview(master).unwrap()[0].time),
        "2026-10-25T01:30:00.000Z"
    );
    let mut master = event("2026-10-24", RepeatFrequency::Daily);
    master.time = EventTime::Timed {
        start: "2026-10-24T00:30:00.000Z".into(),
        end: "2026-10-24T01:30:00.000Z".into(),
        time_zone: "Europe/Zurich".into(),
    };
    assert_eq!(
        boundary(&preview(master).unwrap()[1].time),
        "2026-10-25T00:30:00.000Z"
    );
}
#[test]
fn shifted_split_and_whole_tail_edit_do_not_restart_count() {
    let series = EventSeries {
        event: event("2026-10-05", RepeatFrequency::Weekly),
        exceptions: vec![],
    };
    let mut selected = get_occurrence(&series, "2026-10-19").unwrap();
    selected.time = EventTime::AllDay {
        start_date: "2026-10-20".into(),
        end_date: "2026-10-22".into(),
    };
    let request = EventMutation {
        target: selected.id.clone(),
        scope: MutationScope::Following,
        event: Some(selected),
        expected: expectation(&series).unwrap(),
        reset_exceptions: false,
        count_changed: false,
    };
    let tail = plan_mutation(&series, &request, "ab8bb795-b9c7-47c6-98da-3547021948c5")
        .unwrap()
        .pop()
        .unwrap()
        .after
        .unwrap();
    let values = preview(tail.event.clone()).unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!(boundary(&values[0].time), "2026-10-20");
    let request = EventMutation {
        target: tail.event.id.clone(),
        scope: MutationScope::Series,
        event: Some(CalendarEvent {
            time: EventTime::AllDay {
                start_date: "2026-10-21".into(),
                end_date: "2026-10-23".into(),
            },
            ..tail.event.clone()
        }),
        expected: expectation(&tail).unwrap(),
        reset_exceptions: false,
        count_changed: false,
    };
    let changed = plan_mutation(&tail, &request, "unused")
        .unwrap()
        .pop()
        .unwrap()
        .after
        .unwrap();
    let values = preview(changed.event).unwrap();
    assert_eq!(values.len(), 3);
    assert_eq!(boundary(&values[0].time), "2026-10-21");
}
#[test]
fn limits_and_invalid_weekdays_fail_without_partial_results() {
    let mut master = event("1600-01-01", RepeatFrequency::Daily);
    master.recurrence.as_mut().unwrap().count = None;
    let series = EventSeries {
        event: master,
        exceptions: vec![],
    };
    let range = EventRange {
        start: "2026-10-01T00:00:00Z".into(),
        end: "2026-11-01T00:00:00Z".into(),
        start_date: "2026-10-01".into(),
        end_date: "2026-11-01".into(),
    };
    assert_eq!(expand(&series, &range).unwrap_err().code, "recurrenceLimit");
    let mut master = event("2026-10-05", RepeatFrequency::Weekly);
    master.recurrence.as_mut().unwrap().weekdays = vec![3];
    assert!(preview(master).is_err());
}

#[test]
fn repeated_splits_expose_remaining_count_and_shifted_dst_gaps_do_not_consume_it() {
    let series = EventSeries {
        event: event("2026-10-05", RepeatFrequency::Weekly),
        exceptions: vec![],
    };
    let selected = get_occurrence(&series, "2026-10-19").unwrap();
    let request = EventMutation {
        target: selected.id.clone(),
        scope: MutationScope::Following,
        event: Some(selected),
        expected: expectation(&series).unwrap(),
        reset_exceptions: false,
        count_changed: false,
    };
    let tail = plan_mutation(&series, &request, "ab8bb795-b9c7-47c6-98da-3547021948c5")
        .unwrap()
        .pop()
        .unwrap()
        .after
        .unwrap();
    assert_eq!(tail.event.recurrence.as_ref().unwrap().count, Some(3));
    let selected = get_occurrence(&tail, "2026-10-26").unwrap();
    let request = EventMutation {
        target: selected.id.clone(),
        scope: MutationScope::Following,
        event: Some(selected),
        expected: expectation(&tail).unwrap(),
        reset_exceptions: false,
        count_changed: false,
    };
    let tail = plan_mutation(&tail, &request, "11fd0358-0197-4e97-bce7-7696de05a5dc")
        .unwrap()
        .pop()
        .unwrap()
        .after
        .unwrap();
    assert_eq!(tail.event.recurrence.as_ref().unwrap().count, Some(2));
    assert_eq!(preview(tail.event).unwrap().len(), 2);
    let mut master = event("2026-03-27", RepeatFrequency::Daily);
    master.time = EventTime::Timed {
        start: "2026-03-27T00:30:00.000Z".into(),
        end: "2026-03-27T01:30:00.000Z".into(),
        time_zone: "Europe/Zurich".into(),
    };
    let series = EventSeries {
        event: master,
        exceptions: vec![],
    };
    let mut selected = get_occurrence(&series, "2026-03-28T00:30:00.000Z").unwrap();
    selected.time = EventTime::Timed {
        start: "2026-03-28T01:30:00.000Z".into(),
        end: "2026-03-28T02:30:00.000Z".into(),
        time_zone: "Europe/Zurich".into(),
    };
    let request = EventMutation {
        target: selected.id.clone(),
        scope: MutationScope::Following,
        event: Some(selected),
        expected: expectation(&series).unwrap(),
        reset_exceptions: false,
        count_changed: false,
    };
    let tail = plan_mutation(&series, &request, "ab8bb795-b9c7-47c6-98da-3547021948c5")
        .unwrap()
        .pop()
        .unwrap()
        .after
        .unwrap();
    assert_eq!(tail.event.recurrence.as_ref().unwrap().count, Some(4));
    let values = preview(tail.event).unwrap();
    assert_eq!(values.len(), 4);
    assert_eq!(boundary(&values[1].time), "2026-03-30T00:30:00.000Z");
}

#[test]
fn shared_native_and_wasm_fixtures() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/recurrence.json")).unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let event: CalendarEvent = serde_json::from_value(fixture["event"].clone()).unwrap();
        let starts = preview(event)
            .unwrap()
            .iter()
            .map(|event| boundary(&event.time).to_string())
            .collect::<Vec<_>>();
        let expected: Vec<String> = serde_json::from_value(fixture["starts"].clone()).unwrap();
        assert_eq!(starts, expected, "{}", fixture["name"]);
    }
}

#[test]
fn monthly_date_edits_reanchor_and_keep_each_segment_count() {
    let series = EventSeries {
        event: event("2026-01-31", RepeatFrequency::Monthly),
        exceptions: vec![],
    };
    let mut selected = get_occurrence(&series, "2026-03-31").unwrap();
    selected.time = EventTime::AllDay {
        start_date: "2026-03-30".into(),
        end_date: "2026-03-31".into(),
    };
    let request = EventMutation {
        target: selected.id.clone(),
        scope: MutationScope::Following,
        event: Some(selected),
        expected: expectation(&series).unwrap(),
        reset_exceptions: false,
        count_changed: false,
    };
    let changes = plan_mutation(&series, &request, "ab8bb795-b9c7-47c6-98da-3547021948c5").unwrap();
    let tail = &changes[1].after.as_ref().unwrap().event;
    assert_eq!(
        preview(tail.clone())
            .unwrap()
            .iter()
            .map(|e| boundary(&e.time).to_string())
            .collect::<Vec<_>>(),
        ["2026-03-30", "2026-04-30", "2026-05-30", "2026-06-30"]
    );
    let head = changes[0].after.as_ref().unwrap();
    let mut edited_head = head.event.clone();
    edited_head.time = EventTime::AllDay {
        start_date: "2026-01-30".into(),
        end_date: "2026-01-31".into(),
    };
    let edited = plan_mutation(
        head,
        &EventMutation {
            target: head.event.id.clone(),
            scope: MutationScope::Series,
            event: Some(edited_head),
            expected: expectation(head).unwrap(),
            reset_exceptions: false,
            count_changed: false,
        },
        "unused",
    )
    .unwrap();
    assert_eq!(
        preview(edited[0].after.as_ref().unwrap().event.clone())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn explicit_count_and_weekday_date_changes_apply_to_the_new_tail() {
    let mut master = event("2026-10-05", RepeatFrequency::Weekly);
    master.recurrence.as_mut().unwrap().weekdays = vec![1, 3];
    let series = EventSeries {
        event: master,
        exceptions: vec![],
    };
    let mut selected = get_occurrence(&series, "2026-10-12").unwrap();
    selected.time = EventTime::AllDay {
        start_date: "2026-10-13".into(),
        end_date: "2026-10-14".into(),
    };
    let request = EventMutation {
        target: selected.id.clone(),
        scope: MutationScope::Following,
        event: Some(selected),
        expected: expectation(&series).unwrap(),
        reset_exceptions: false,
        count_changed: true,
    };
    let changes = plan_mutation(&series, &request, "ab8bb795-b9c7-47c6-98da-3547021948c5").unwrap();
    let tail = &changes[1].after.as_ref().unwrap().event;
    assert_eq!(tail.recurrence.as_ref().unwrap().weekdays, [2, 4]);
    assert_eq!(tail.recurrence.as_ref().unwrap().count, Some(5));
    assert_eq!(preview(tail.clone()).unwrap().len(), 5);
    let invalid = EventMutation {
        scope: MutationScope::Occurrence,
        ..request
    };
    assert!(plan_mutation(
        &series,
        &EventMutation {
            event: invalid.event.map(|mut e| {
                e.recurrence.as_mut().unwrap().count = Some(2);
                e
            }),
            ..invalid
        },
        "unused"
    )
    .is_err());
}
