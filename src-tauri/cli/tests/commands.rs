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
