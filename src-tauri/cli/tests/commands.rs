use aika_core::{contracts::EventTime, service::CalendarService};
use std::{
    path::PathBuf,
    process::{Command, Output},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("aika-cli-{}.sqlite3", uuid::Uuid::new_v4())))
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_aika-cli"))
            .arg("--database")
            .arg(&self.0)
            .args(args)
            .env("TZ", "Europe/Zurich")
            .output()
            .unwrap()
    }
    fn success(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        String::from_utf8(output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn id(output: &str) -> &str {
    output.lines().next().unwrap().strip_prefix("ID: ").unwrap()
}

#[test]
fn crud_persists_and_shares_desktop_core() {
    let f = Fixture::new();
    let created = f.success(&[
        "events",
        "create",
        "--title",
        "Meeting",
        "--start",
        "2026-10-10T10:00",
        "--end",
        "2026-10-10T11:00",
        "--location",
        "Office",
        "--notes",
        "Bring agenda",
    ]);
    let id = id(&created);
    let mut desktop = CalendarService::open(&f.0).unwrap();
    let event = desktop.get(id).unwrap();
    if let EventTime::Timed {
        start, time_zone, ..
    } = &event.time
    {
        assert_eq!(start, "2026-10-10T08:00:00.000Z");
        assert_eq!(time_zone, "Europe/Zurich");
    } else {
        panic!("expected timed");
    }
    let changed = f.success(&[
        "events",
        "update",
        id,
        "--title",
        "Updated",
        "--location",
        "",
        "--notes",
        "",
    ]);
    assert!(changed.contains("Title: Updated\n"));
    let updated = desktop.get(id).unwrap();
    assert_eq!(updated.time, event.time);
    assert!(updated.location.is_empty());
    assert!(updated.notes.is_empty());
    assert_eq!(
        desktop.delete_checked(id, &event).unwrap_err().code,
        "conflict"
    );
    assert!(f
        .success(&["events", "get", id])
        .contains("Calendar: local-default"));
    assert!(f
        .success(&[
            "events",
            "list",
            "--from",
            "2026-10-10",
            "--to",
            "2026-10-11"
        ])
        .contains(id));
    assert_eq!(
        f.success(&["events", "delete", id]),
        format!("Deleted {id}\n")
    );
    assert_eq!(f.run(&["events", "get", id]).status.code(), Some(3));
    assert_eq!(desktop.get(id).unwrap_err().code, "notFound");
}

#[test]
fn all_day_conversion_and_exclusive_ranges() {
    let f = Fixture::new();
    let output = f.success(&[
        "events",
        "create",
        "--title",
        "Holiday",
        "--all-day",
        "--start",
        "2026-10-09",
        "--end",
        "2026-10-11",
    ]);
    let id = id(&output);
    assert!(f
        .success(&[
            "events",
            "list",
            "--from",
            "2026-10-10",
            "--to",
            "2026-10-11"
        ])
        .contains(id));
    assert!(f
        .success(&[
            "events",
            "list",
            "--from",
            "2026-10-11",
            "--to",
            "2026-10-12"
        ])
        .is_empty());
    assert!(f
        .success(&[
            "events",
            "list",
            "--from",
            "2026-10-08",
            "--to",
            "2026-10-09"
        ])
        .is_empty());
    assert_eq!(
        f.run(&[
            "events",
            "update",
            id,
            "--timed",
            "--start",
            "2026-10-10T10:00"
        ])
        .status
        .code(),
        Some(2)
    );
    f.success(&[
        "events",
        "update",
        id,
        "--timed",
        "--start",
        "2026-10-10T10:00",
        "--end",
        "2026-10-10T11:00",
        "--time-zone",
        "America/New_York",
    ]);
    f.success(&[
        "events",
        "update",
        id,
        "--start",
        "2026-10-10T12:00",
        "--end",
        "2026-10-10T13:00",
    ]);
    assert!(f
        .success(&["events", "get", id])
        .contains("2026-10-10T12:00:00-04:00"));
    f.success(&[
        "events",
        "update",
        id,
        "--all-day",
        "--start",
        "2026-10-10",
        "--end",
        "2026-10-12",
    ]);
    assert!(f.success(&["events", "get", id]).contains("all-day"));
}

#[test]
fn validation_dst_and_explicit_offsets() {
    let f = Fixture::new();
    for args in [
        vec!["events", "create", "--title", "Missing"],
        vec![
            "events",
            "create",
            "--title",
            "Gap",
            "--start",
            "2026-03-29T02:30",
            "--end",
            "2026-03-29T04:00",
        ],
        vec![
            "events",
            "create",
            "--title",
            "Fold",
            "--start",
            "2026-10-25T02:30",
            "--end",
            "2026-10-25T04:00",
        ],
        vec![
            "events",
            "create",
            "--title",
            "Bad",
            "--start",
            "2026-10-10T10:00",
            "--end",
            "2026-10-10T09:00",
        ],
        vec![
            "events",
            "create",
            "--title",
            "   ",
            "--all-day",
            "--start",
            "2026-10-10",
            "--end",
            "2026-10-11",
        ],
        vec![
            "events",
            "list",
            "--from",
            "2026-10-10",
            "--to",
            "2026-10-09",
        ],
        vec![
            "events",
            "list",
            "--from",
            "2026-01-01",
            "--to",
            "2028-01-01",
        ],
        vec![
            "events",
            "list",
            "--from",
            "2026-02-30",
            "--to",
            "2026-03-01",
        ],
        vec![
            "events",
            "list",
            "--from",
            "2026-10-10",
            "--to",
            "2026-10-11",
            "--time-zone",
            "Bogus/Zone",
        ],
        vec!["events", "get", "not-a-uuid"],
    ] {
        let output = f.run(&args);
        assert_eq!(
            output.status.code(),
            Some(2),
            "args: {args:?}, error: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
    }
    let first = f.success(&[
        "events",
        "create",
        "--title",
        "First fold",
        "--start",
        "2026-10-25T02:30:00+02:00",
        "--end",
        "2026-10-25T03:00:00+01:00",
    ]);
    let second = f.success(&[
        "events",
        "create",
        "--title",
        "Second fold",
        "--start",
        "2026-10-25T02:30:00+01:00",
        "--end",
        "2026-10-25T03:00:00+01:00",
    ]);
    let list = f.success(&[
        "events",
        "list",
        "--from",
        "2026-10-25",
        "--to",
        "2026-10-26",
        "--time-zone",
        "UTC",
    ]);
    assert!(list.find(id(&first)).unwrap() < list.find(id(&second)).unwrap());
    assert!(list.contains("2026-10-25T00:30:00+00:00"));
    let desktop = CalendarService::open(&f.0).unwrap();
    assert_eq!(desktop.get(id(&second)).unwrap().title, "Second fold");
}

#[test]
fn help_and_version_do_not_open_database() {
    let f = Fixture::new();
    assert!(f.success(&["--help"]).contains("events"));
    assert!(f.success(&["--version"]).starts_with("aika-cli "));
    assert!(!f.0.exists());
}

#[test]
fn preserves_newer_database() {
    let f = Fixture::new();
    let db = rusqlite::Connection::open(&f.0).unwrap();
    db.execute_batch("CREATE TABLE marker(value TEXT); INSERT INTO marker VALUES('preserve'); PRAGMA user_version=99;").unwrap();
    let output = f.run(&[
        "events",
        "list",
        "--from",
        "2026-10-10",
        "--to",
        "2026-10-11",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("newer version"));
    assert_eq!(
        db.query_row("SELECT value FROM marker", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "preserve"
    );
}

#[test]
fn desktop_created_event_is_readable_by_cli() {
    let f = Fixture::new();
    let mut desktop = CalendarService::open(&f.0).unwrap();
    let event = aika_core::contracts::CalendarEvent {
        id: uuid::Uuid::new_v4().to_string(),
        calendar_id: "local-default".into(),
        title: "From desktop".into(),
        location: "Home".into(),
        notes: "Saved through shared core".into(),
        recurrence: None,
        occurrence: None,
        time: EventTime::AllDay {
            start_date: "2026-10-10".into(),
            end_date: "2026-10-11".into(),
        },
    };
    desktop.save_checked(event.clone(), false, None).unwrap();
    let output = f.success(&["events", "get", &event.id]);
    assert!(output.contains("Title: From desktop"));
    assert!(output.contains("Location: Home"));
    assert!(output.contains("Notes: Saved through shared core"));
}

#[cfg(target_os = "linux")]
#[test]
fn default_database_uses_desktop_identifier_in_xdg_data_directory() {
    let directory = std::env::temp_dir().join(format!("aika-xdg-{}", uuid::Uuid::new_v4()));
    let output = Command::new(env!("CARGO_BIN_EXE_aika-cli"))
        .args([
            "events",
            "create",
            "--title",
            "Default location",
            "--all-day",
            "--start",
            "2026-10-10",
            "--end",
            "2026-10-11",
        ])
        .env("XDG_DATA_HOME", &directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let path = directory.join("dev.projectcal.desktop/calendar.sqlite3");
    assert!(path.exists());
    let desktop = CalendarService::open(&path).unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    assert_eq!(desktop.get(id(&output)).unwrap().title, "Default location");
    drop(desktop);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cli_uses_default_and_preserves_calendar_after_original_is_deleted() {
    let f = Fixture::new();
    let mut desktop = CalendarService::open(&f.0).unwrap();
    let calendar = desktop
        .create_calendar(aika_core::contracts::CalendarDetails {
            name: "Work".into(),
            description: "Office".into(),
            color: "#5B7FA3".into(),
        })
        .unwrap();
    desktop.set_default_calendar(&calendar.id).unwrap();
    desktop.delete_calendar("local-default").unwrap();
    let created = f.success(&[
        "events",
        "create",
        "--title",
        "Work event",
        "--all-day",
        "--start",
        "2026-10-10",
        "--end",
        "2026-10-11",
    ]);
    let id = id(&created);
    assert!(created.contains(&format!("Calendar: {}", calendar.id)));
    desktop
        .set_calendar_visibility(&calendar.id, false)
        .unwrap();
    let listed = f.success(&[
        "events",
        "list",
        "--from",
        "2026-10-10",
        "--to",
        "2026-10-11",
    ]);
    assert!(listed.contains("Work event"));
    let updated = f.success(&["events", "update", id, "--title", "Updated work event"]);
    assert!(updated.contains(&format!("Calendar: {}", calendar.id)));
    assert_eq!(desktop.get(id).unwrap().calendar_id, calendar.id);
    f.success(&["events", "delete", id]);
    desktop.delete_calendar(&calendar.id).unwrap();
    let empty = f.success(&[
        "events",
        "list",
        "--from",
        "2026-10-10",
        "--to",
        "2026-10-11",
    ]);
    assert!(empty.is_empty());
    let output = f.run(&[
        "events",
        "create",
        "--title",
        "No calendar",
        "--all-day",
        "--start",
        "2026-10-10",
        "--end",
        "2026-10-11",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Create a local calendar"));
    assert!(desktop
        .collection_snapshot(aika_core::contracts::EventRange {
            start: "2026-10-10T00:00:00Z".into(),
            end: "2026-10-11T00:00:00Z".into(),
            start_date: "2026-10-10".into(),
            end_date: "2026-10-11".into(),
        })
        .unwrap()
        .calendars
        .is_empty());
}

#[test]
fn calendar_crud_settings_and_desktop_persistence() {
    let f = Fixture::new();
    let personal = f.success(&["calendars", "get", "local-default"]);
    assert!(personal.contains("Name: Personal\n"));
    assert!(personal.contains("Default: true\n"));
    assert!(personal.contains("Visible: true\n"));
    let created = f.success(&[
        "calendars",
        "create",
        "--name",
        "  Work  ",
        "--description",
        "Office",
        "--color",
        "#5B7FA3",
    ]);
    let calendar_id = id(&created);
    assert!(uuid::Uuid::parse_str(calendar_id).is_ok());
    assert!(created.contains("Name: Work\n"));
    assert!(created.contains("Default: false\n"));
    let mut desktop = CalendarService::open(&f.0).unwrap();
    let original = desktop.calendar_management_snapshot().unwrap().calendars[1].clone();
    let updated = f.success(&[
        "calendars",
        "update",
        calendar_id,
        "--name",
        "Office calendar",
    ]);
    assert!(updated.contains("Description: Office\n"));
    assert!(updated.contains("Color: #5B7FA3\n"));
    assert_eq!(
        desktop
            .update_calendar(
                calendar_id,
                aika_core::contracts::CalendarDetails {
                    name: "Stale".into(),
                    description: "".into(),
                    color: "#718675".into(),
                },
                original
            )
            .unwrap_err()
            .code,
        "conflict"
    );
    let cleared = f.success(&["calendars", "update", calendar_id, "--description", ""]);
    assert!(cleared.contains("Description: \n"));
    f.success(&["calendars", "set-default", calendar_id]);
    f.success(&["calendars", "hide", calendar_id]);
    let snapshot = desktop.calendar_management_snapshot().unwrap();
    assert_eq!(snapshot.default_calendar_id.as_deref(), Some(calendar_id));
    assert_eq!(snapshot.hidden_calendar_ids, vec![calendar_id]);
    let listed = f.success(&["calendars", "list"]);
    assert!(listed.find("local-default").unwrap() < listed.find(calendar_id).unwrap());
    assert!(listed.contains(&format!(
        "{calendar_id}  Office calendar  #5B7FA3  default=true  visible=false"
    )));
    let event = f.success(&[
        "events",
        "create",
        "--title",
        "Uses default",
        "--all-day",
        "--start",
        "2026-10-10",
        "--end",
        "2026-10-11",
    ]);
    assert_eq!(desktop.get(id(&event)).unwrap().calendar_id, calendar_id);
    // Event creation reveals its calendar; hiding still does not affect CLI event listing.
    f.success(&["calendars", "hide", calendar_id]);
    assert!(f
        .success(&[
            "events",
            "list",
            "--from",
            "2026-10-10",
            "--to",
            "2026-10-11"
        ])
        .contains(id(&event)));
    f.success(&["calendars", "show", calendar_id]);
    assert!(desktop
        .calendar_management_snapshot()
        .unwrap()
        .hidden_calendar_ids
        .is_empty());
    drop(desktop);
    let reopened = CalendarService::open(&f.0).unwrap();
    let snapshot = reopened.calendar_management_snapshot().unwrap();
    assert_eq!(snapshot.calendars[1].name, "Office calendar");
    assert!(snapshot.calendars[1].description.is_empty());
    assert_eq!(snapshot.default_calendar_id.as_deref(), Some(calendar_id));
}

#[test]
fn calendar_deletion_requires_force_and_recovers_from_empty_database() {
    let f = Fixture::new();
    let first = f.success(&["calendars", "create", "--name", "Work"]);
    let first_id = id(&first);
    assert!(first.contains("Color: #718675\n"));
    let second = f.success(&["calendars", "create", "--name", "Other"]);
    let second_id = id(&second);
    f.success(&["calendars", "set-default", first_id]);
    let event = f.success(&[
        "events",
        "create",
        "--title",
        "Keep me",
        "--all-day",
        "--start",
        "2026-10-10",
        "--end",
        "2026-10-11",
    ]);
    let rejected = f.run(&["calendars", "delete", first_id]);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(rejected.stdout.is_empty());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("--force"));
    let desktop = CalendarService::open(&f.0).unwrap();
    assert_eq!(
        desktop.default_calendar_id().unwrap().as_deref(),
        Some(first_id)
    );
    assert_eq!(desktop.get(id(&event)).unwrap().title, "Keep me");
    assert_eq!(
        f.success(&["calendars", "delete", first_id, "--force"]),
        format!("Deleted {first_id}\n")
    );
    assert_eq!(desktop.get(id(&event)).unwrap_err().code, "notFound");
    assert_eq!(
        desktop.default_calendar_id().unwrap().as_deref(),
        Some("local-default")
    );
    f.success(&["calendars", "delete", "local-default"]);
    assert_eq!(
        desktop.default_calendar_id().unwrap().as_deref(),
        Some(second_id)
    );
    f.success(&["calendars", "delete", second_id]);
    assert!(f.success(&["calendars", "list"]).is_empty());
    assert!(desktop.default_calendar_id().unwrap().is_none());
    let output = f.run(&[
        "events",
        "create",
        "--title",
        "No calendar",
        "--all-day",
        "--start",
        "2026-10-10",
        "--end",
        "2026-10-11",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("calendars create"));
    let replacement = f.success(&["calendars", "create", "--name", "New first"]);
    assert!(replacement.contains("Default: true\n"));
    assert_eq!(
        desktop.default_calendar_id().unwrap().as_deref(),
        Some(id(&replacement))
    );
}

#[test]
fn calendar_validation_missing_ids_and_terminal_safe_output() {
    let f = Fixture::new();
    for args in [
        vec!["calendars", "create"],
        vec!["calendars", "create", "--name", "   "],
        vec![
            "calendars",
            "create",
            "--name",
            "Bad color",
            "--color",
            "#ffffff",
        ],
        vec!["calendars", "update", "local-default"],
    ] {
        let output = f.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!f.0.exists());
    }
    let created = f.success(&[
        "calendars",
        "create",
        "--name",
        "Safe\n\x1bName",
        "--description",
        "Line\nTab\tEnd",
    ]);
    assert!(created.contains("Name: Safe  Name\n"));
    assert!(created.contains("Description: Line Tab End\n"));
    assert!(!created.contains('\x1b'));
    let before = f.success(&["calendars", "get", id(&created)]);
    for args in [
        vec!["calendars", "update", id(&created), "--name", ""],
        vec!["calendars", "update", id(&created), "--color", "#ffffff"],
    ] {
        let output = f.run(&args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(f.success(&["calendars", "get", id(&created)]), before);
    }
    let long_name = "x".repeat(101);
    let long_description = "x".repeat(2001);
    for args in [
        vec!["calendars", "update", id(&created), "--name", &long_name],
        vec![
            "calendars",
            "update",
            id(&created),
            "--description",
            &long_description,
        ],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(2));
        assert_eq!(f.success(&["calendars", "get", id(&created)]), before);
    }
    for color in [
        "#718675", "#5B7FA3", "#8A6FA8", "#B56F86", "#B38B43", "#4F918A", "#B77B50", "#768390",
    ] {
        assert!(f
            .success(&["calendars", "update", id(&created), "--color", color])
            .contains(&format!("Color: {color}\n")));
    }
    for operation in ["get", "update", "delete", "set-default", "show", "hide"] {
        let missing = uuid::Uuid::new_v4().to_string();
        let mut args = vec!["calendars", operation, &missing];
        if operation == "update" {
            args.extend(["--name", "Missing"]);
        }
        let output = f.run(&args);
        assert_eq!(output.status.code(), Some(3), "{operation}");
        assert!(output.stdout.is_empty());
    }
    assert_eq!(
        f.run(&["calendars", "get", &id(&created)[..8]])
            .status
            .code(),
        Some(3)
    );
    assert!(!f.success(&["calendars", "list"]).contains('\x1b'));
}

#[test]
fn calendar_help_and_listing_do_not_require_time_zone() {
    let f = Fixture::new();
    assert!(f.success(&["--help"]).contains("calendars"));
    assert!(f.success(&["calendars", "--help"]).contains("set-default"));
    assert!(f
        .success(&["calendars", "delete", "--help"])
        .contains("--force"));
    assert!(!f.0.exists());
    let output = Command::new(env!("CARGO_BIN_EXE_aika-cli"))
        .args(["calendars", "list", "--database"])
        .arg(&f.0)
        .env("TZ", "Bogus/Zone")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("local-default"));
}

#[test]
fn recurring_cli_scopes_endings_and_exception_reset() {
    let f = Fixture::new();
    let created = f.success(&[
        "events",
        "create",
        "--title",
        "Weekly",
        "--all-day",
        "--start",
        "2026-10-05",
        "--end",
        "2026-10-06",
        "--repeat",
        "weekly",
        "--weekdays",
        "mon,wed",
        "--interval",
        "2",
        "--count",
        "5",
    ]);
    let master = id(&created);
    let list = f.success(&[
        "events",
        "list",
        "--from",
        "2026-10-01",
        "--to",
        "2026-12-01",
    ]);
    assert_eq!(list.lines().count(), 5);
    assert!(list.contains(&format!("{master}@2026-10-19")));
    assert_eq!(f.run(&["events", "delete", master]).status.code(), Some(2));
    let occurrence = format!("{master}@2026-10-19");
    f.success(&[
        "events",
        "update",
        &occurrence,
        "--scope",
        "occurrence",
        "--title",
        "Exception",
    ]);
    assert!(f
        .success(&["events", "get", &occurrence])
        .contains("Title: Exception"));
    let failed = f.run(&[
        "events", "update", master, "--scope", "series", "--title", "Reset",
    ]);
    assert_eq!(failed.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("exceptionsResetRequired"));
    f.success(&[
        "events",
        "update",
        master,
        "--scope",
        "series",
        "--title",
        "Reset",
        "--reset-exceptions",
    ]);
    assert!(f
        .success(&["events", "get", &occurrence])
        .contains("Title: Reset"));
    let updated = f.success(&[
        "events",
        "update",
        &occurrence,
        "--scope",
        "following",
        "--title",
        "Tail",
    ]);
    let tail = id(&updated);
    assert_ne!(tail, master);
    assert!(uuid::Uuid::parse_str(tail).is_ok());
    let listed = f.success(&[
        "events",
        "list",
        "--from",
        "2026-10-01",
        "--to",
        "2026-12-01",
    ]);
    assert_eq!(listed.lines().count(), 5);
    assert_eq!(listed.lines().filter(|l| l.contains("Tail")).count(), 3);
    f.success(&[
        "events",
        "delete",
        &format!("{tail}@2026-10-21"),
        "--scope",
        "occurrence",
    ]);
    assert_eq!(
        f.success(&[
            "events",
            "list",
            "--from",
            "2026-10-01",
            "--to",
            "2026-12-01"
        ])
        .lines()
        .count(),
        4
    );
    f.success(&[
        "events",
        "update",
        tail,
        "--scope",
        "series",
        "--no-repeat",
        "--reset-exceptions",
    ]);
    assert_eq!(
        f.success(&[
            "events",
            "list",
            "--from",
            "2026-10-01",
            "--to",
            "2026-12-01"
        ])
        .lines()
        .count(),
        3
    );
}

#[test]
fn recurring_cli_validation_monthly_yearly_and_timed_dst() {
    let f = Fixture::new();
    for args in [
        vec!["--repeat", "daily", "--interval", "0"],
        vec!["--repeat", "daily", "--count", "0"],
        vec!["--repeat", "weekly", "--weekdays", "wed"],
        vec!["--interval", "2"],
        vec!["--repeat", "daily", "--weekdays", "mon"],
        vec!["--repeat", "weekly", "--weekdays", "mon,mon"],
    ] {
        let mut input = vec![
            "events",
            "create",
            "--title",
            "Bad",
            "--all-day",
            "--start",
            "2026-10-05",
            "--end",
            "2026-10-06",
        ];
        input.extend(args);
        assert_eq!(f.run(&input).status.code(), Some(2));
        assert!(!f.0.exists());
    }
    let created = f.success(&[
        "events",
        "create",
        "--title",
        "Month end",
        "--all-day",
        "--start",
        "2026-01-31",
        "--end",
        "2026-02-01",
        "--repeat",
        "monthly",
        "--count",
        "3",
    ]);
    let listed = f.success(&[
        "events",
        "list",
        "--from",
        "2026-01-01",
        "--to",
        "2026-06-01",
    ]);
    assert_eq!(listed.lines().count(), 3);
    assert!(!listed.contains("2026-02-28"));
    assert!(listed.contains("2026-05-31"));
    f.success(&["events", "delete", id(&created), "--scope", "series"]);
    f.success(&[
        "events",
        "create",
        "--title",
        "DST",
        "--start",
        "2026-03-28T02:30",
        "--end",
        "2026-03-28T03:30",
        "--repeat",
        "daily",
        "--count",
        "3",
    ]);
    let list = f.success(&[
        "events",
        "list",
        "--from",
        "2026-03-28",
        "--to",
        "2026-04-03",
    ]);
    assert_eq!(list.lines().count(), 3);
    assert!(!list.contains("2026-03-29"));
    assert!(list.contains("2026-03-30T02:30:00+02:00"));
}
