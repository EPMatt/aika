use aika_core::{
    contracts::{
        Calendar, CalendarDetails, CalendarEvent, EventMutation, EventRange, EventTime, MonthlyOn,
        MutationScope, Recurrence, RepeatFrequency, ServiceError,
    },
    service::{
        error, resolve_local, validate_event, validate_range, CalendarManagementSnapshot,
        CalendarService,
    },
    storage,
};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use chrono_tz::Tz;
use clap::{Args, Parser, Subcommand};
use std::{fmt::Write, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(name = "aika-cli", version, about = "Manage aika's local calendars")]
struct Cli {
    /// Use an explicit SQLite database instead of the desktop application's database.
    #[arg(long, global = true, value_name = "PATH")]
    database: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List, inspect, create, update, delete, or configure local calendars.
    Calendars {
        #[command(subcommand)]
        command: Calendars,
    },
    /// List, inspect, create, update, or delete events.
    Events {
        #[command(subcommand)]
        command: Box<Events>,
    },
}

#[derive(Subcommand)]
enum Calendars {
    /// List all local calendars, including hidden calendars, in creation order.
    List,
    /// Show calendar metadata and settings using its exact ID.
    Get { id: String },
    /// Create a visible calendar. Supply --name; color defaults to sage.
    Create(CalendarArgs),
    /// Update supplied fields; an empty description clears it.
    Update {
        id: String,
        #[command(flatten)]
        fields: CalendarArgs,
    },
    /// Permanently delete a calendar and its events; populated calendars require --force.
    Delete {
        id: String,
        #[arg(long)]
        force: bool,
    },
    /// Select the calendar used for new events.
    SetDefault { id: String },
    /// Show a calendar's events in the desktop app.
    Show { id: String },
    /// Hide a calendar's events in the desktop app.
    Hide { id: String },
}

#[derive(Args, Clone)]
struct CalendarArgs {
    #[arg(long)]
    name: Option<String>,
    #[arg(long)]
    description: Option<String>,
    /// Palette hex: #718675, #5B7FA3, #8A6FA8, #B56F86, #B38B43, #4F918A, #B77B50, #768390.
    #[arg(long, value_name = "HEX")]
    color: Option<String>,
}

#[derive(Subcommand)]
enum Events {
    /// List overlapping events in [from, to), up to 366 days.
    List {
        #[arg(long, value_name = "YYYY-MM-DD")]
        from: String,
        #[arg(long, value_name = "YYYY-MM-DD")]
        to: String,
        /// IANA zone for date boundaries and displayed times (default: system zone).
        #[arg(long, value_name = "ZONE")]
        time_zone: Option<String>,
    },
    /// Show every field of an event, identified by its full UUID.
    Get { id: String },
    /// Create an event. Title, start, and end are required.
    Create(EventArgs),
    /// Update only supplied fields. Use an empty string to clear location or notes.
    Update {
        id: String,
        #[command(flatten)]
        fields: EventArgs,
    },
    /// Delete an event by its full UUID, without a confirmation prompt.
    Delete {
        id: String,
        #[arg(long, value_parser = ["occurrence", "following", "series"])]
        scope: Option<String>,
    },
}

#[derive(Args, Clone, Default)]
struct EventArgs {
    #[arg(long)]
    title: Option<String>,
    /// Local YYYY-MM-DDTHH:MM or RFC3339 with offset; YYYY-MM-DD for all-day events.
    #[arg(long, value_name = "VALUE")]
    start: Option<String>,
    /// Exclusive end, using the same formats as --start.
    #[arg(long, value_name = "VALUE")]
    end: Option<String>,
    /// IANA event zone (default: system zone on create, existing zone on update).
    #[arg(long, value_name = "ZONE")]
    time_zone: Option<String>,
    #[arg(long, conflicts_with_all = ["timed", "time_zone"])]
    all_day: bool,
    #[arg(long, conflicts_with = "all_day")]
    timed: bool,
    #[arg(long)]
    location: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long, value_parser = ["daily", "weekly", "monthly", "yearly"], conflicts_with = "no_repeat")]
    repeat: Option<String>,
    #[arg(long, conflicts_with = "no_repeat")]
    interval: Option<u32>,
    /// Comma-separated weekdays: mon,tue,wed,thu,fri,sat,sun.
    #[arg(long, conflicts_with = "no_repeat")]
    weekdays: Option<String>,
    #[arg(long, value_parser = ["date", "weekday"], conflicts_with = "no_repeat")]
    monthly_on: Option<String>,
    /// Inclusive ending date in the event's zone.
    #[arg(long, conflicts_with_all = ["count", "forever", "no_repeat"])]
    until: Option<String>,
    #[arg(long, conflicts_with_all = ["until", "forever", "no_repeat"])]
    count: Option<u32>,
    #[arg(long, conflicts_with_all = ["until", "count", "no_repeat"])]
    forever: bool,
    #[arg(long)]
    no_repeat: bool,
    #[arg(long, value_parser = ["occurrence", "following", "series"])]
    scope: Option<String>,
    /// Explicitly discard affected individual edits/cancellations on series/following edits.
    #[arg(long)]
    reset_exceptions: bool,
}

type Result<T> = std::result::Result<T, ServiceError>;

fn system_zone() -> Result<String> {
    // Honor an explicit IANA TZ, including in portable/test environments.
    let value = std::env::var("TZ")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| iana_time_zone::get_timezone().ok())
        .ok_or_else(|| {
            error(
                "validation",
                "Could not resolve the system IANA zone. Supply --time-zone.",
            )
        })?;
    parse_zone(&value)?;
    Ok(value)
}

fn parse_zone(value: &str) -> Result<Tz> {
    value
        .parse()
        .map_err(|_| error("validation", "Use a valid IANA time zone with --time-zone."))
}

fn date(value: &str) -> Result<NaiveDate> {
    let parsed = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| error("validation", "Use an ISO date: YYYY-MM-DD."))?;
    if parsed.to_string() != value {
        return Err(error("validation", "Use an ISO date: YYYY-MM-DD."));
    }
    Ok(parsed)
}

fn timestamp(value: &str, zone: &str) -> Result<String> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return Ok(value
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true));
    }
    resolve_local(value, zone, None).map_err(|mut err| {
        if err.code == "ambiguousTime" {
            err.message
                .push_str(" Supply an RFC3339 timestamp with the intended UTC offset.");
        }
        err
    })
}

fn range(from: &str, to: &str, zone: &str) -> Result<EventRange> {
    date(from)?;
    date(to)?;
    let midnight = |date: &str| {
        resolve_local(&format!("{date}T00:00"), zone, None).map_err(|mut err| {
            if err.code == "ambiguousTime" || err.code == "nonexistentTime" {
                err.message = format!("Midnight on {date} is unavailable or ambiguous in {zone}. Choose another boundary date or --time-zone.");
            }
            err
        })
    };
    Ok(EventRange {
        start: midnight(from)?,
        end: midnight(to)?,
        start_date: from.into(),
        end_date: to.into(),
    })
}

fn required(value: &Option<String>, name: &str) -> Result<String> {
    value
        .clone()
        .ok_or_else(|| error("validation", &format!("Supply --{name}.")))
}

fn repeat_from_args(
    fields: &EventArgs,
    previous: Option<&CalendarEvent>,
) -> Result<Option<Recurrence>> {
    if fields.no_repeat {
        return Ok(None);
    }
    let mut rule = previous.and_then(|e| e.recurrence.clone());
    if let Some(frequency) = &fields.repeat {
        let frequency = match frequency.as_str() {
            "daily" => RepeatFrequency::Daily,
            "weekly" => RepeatFrequency::Weekly,
            "monthly" => RepeatFrequency::Monthly,
            _ => RepeatFrequency::Yearly,
        };
        if let Some(rule) = &mut rule {
            if rule.frequency != frequency {
                rule.weekdays.clear();
            }
            rule.frequency = frequency;
        } else {
            rule = Some(Recurrence {
                frequency,
                interval: 1,
                weekdays: vec![],
                monthly_on: MonthlyOn::Date,
                until: None,
                count: None,
                segment: None,
            });
        }
    }
    let changed = fields.interval.is_some()
        || fields.weekdays.is_some()
        || fields.monthly_on.is_some()
        || fields.until.is_some()
        || fields.count.is_some()
        || fields.forever;
    if changed && rule.is_none() {
        return Err(error(
            "validation",
            "Supply --repeat before repeat options.",
        ));
    }
    if let Some(rule) = &mut rule {
        if let Some(interval) = fields.interval {
            rule.interval = interval;
        }
        if let Some(value) = &fields.weekdays {
            if rule.frequency != RepeatFrequency::Weekly {
                return Err(error(
                    "validation",
                    "--weekdays requires weekly repetition.",
                ));
            }
            rule.weekdays = value
                .split(',')
                .map(|day| match day.trim().to_ascii_lowercase().as_str() {
                    "mon" => Ok(1),
                    "tue" => Ok(2),
                    "wed" => Ok(3),
                    "thu" => Ok(4),
                    "fri" => Ok(5),
                    "sat" => Ok(6),
                    "sun" => Ok(7),
                    _ => Err(error(
                        "validation",
                        "Use weekdays mon,tue,wed,thu,fri,sat,sun.",
                    )),
                })
                .collect::<Result<Vec<_>>>()?;
        }
        if let Some(value) = &fields.monthly_on {
            if rule.frequency != RepeatFrequency::Monthly {
                return Err(error(
                    "validation",
                    "--monthly-on requires monthly repetition.",
                ));
            }
            rule.monthly_on = if value == "weekday" {
                MonthlyOn::Weekday
            } else {
                MonthlyOn::Date
            };
        }
        if let Some(until) = &fields.until {
            date(until)?;
            rule.until = Some(until.clone());
            rule.count = None;
        }
        if let Some(count) = fields.count {
            rule.count = Some(count);
            rule.until = None;
        }
        if fields.forever {
            rule.count = None;
            rule.until = None;
        }
    }
    Ok(rule)
}
fn mutation_scope(value: Option<&str>, recurring: bool) -> Result<MutationScope> {
    match value {
        Some("occurrence") => Ok(MutationScope::Occurrence),
        Some("following") => Ok(MutationScope::Following),
        Some("series") => Ok(MutationScope::Series),
        None if !recurring => Ok(MutationScope::Series),
        _ => Err(error(
            "validation",
            "Recurring mutations require --scope occurrence|following|series.",
        )),
    }
}

fn event_from_args(fields: EventArgs, previous: Option<&CalendarEvent>) -> Result<CalendarEvent> {
    let recurrence = repeat_from_args(&fields, previous)?;
    let previous_all_day = previous.is_some_and(|e| matches!(e.time, EventTime::AllDay { .. }));
    let all_day = fields.all_day || (previous_all_day && !fields.timed);
    if previous.is_some()
        && all_day != previous_all_day
        && (fields.start.is_none() || fields.end.is_none())
    {
        return Err(error(
            "validation",
            "Changing event kind requires --start and --end.",
        ));
    }
    if all_day && fields.time_zone.is_some() {
        return Err(error(
            "validation",
            "All-day events use dates; omit --time-zone or select --timed.",
        ));
    }
    let time = if all_day {
        let (old_start, old_end) = match previous.map(|e| &e.time) {
            Some(EventTime::AllDay {
                start_date,
                end_date,
            }) => (Some(start_date.clone()), Some(end_date.clone())),
            _ => (None, None),
        };
        let start_date = required(&fields.start.or(old_start), "start")?;
        let end_date = required(&fields.end.or(old_end), "end")?;
        date(&start_date)?;
        date(&end_date)?;
        EventTime::AllDay {
            start_date,
            end_date,
        }
    } else {
        let (old_start, old_end, old_zone) = match previous.map(|e| &e.time) {
            Some(EventTime::Timed {
                start,
                end,
                time_zone,
            }) => (
                Some(start.clone()),
                Some(end.clone()),
                Some(time_zone.clone()),
            ),
            _ => (None, None, None),
        };
        let time_zone = match fields.time_zone.or(old_zone) {
            Some(zone) => zone,
            None => system_zone()?,
        };
        parse_zone(&time_zone)?;
        EventTime::Timed {
            start: timestamp(&required(&fields.start.or(old_start), "start")?, &time_zone)?,
            end: timestamp(&required(&fields.end.or(old_end), "end")?, &time_zone)?,
            time_zone,
        }
    };
    Ok(CalendarEvent {
        recurrence,
        occurrence: previous.and_then(|e| e.occurrence.clone()),
        id: previous
            .map(|e| e.id.clone())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        calendar_id: previous
            .map(|event| event.calendar_id.clone())
            .unwrap_or_else(|| "local-default".into()),
        title: required(
            &fields.title.or_else(|| previous.map(|e| e.title.clone())),
            "title",
        )?,
        time,
        location: fields
            .location
            .unwrap_or_else(|| previous.map(|e| e.location.clone()).unwrap_or_default()),
        notes: fields
            .notes
            .unwrap_or_else(|| previous.map(|e| e.notes.clone()).unwrap_or_default()),
    })
}

fn display_time(event: &CalendarEvent, zone: Option<Tz>) -> String {
    match &event.time {
        EventTime::AllDay {
            start_date,
            end_date,
        } => format!("{start_date} → {end_date} (all-day, exclusive end)"),
        EventTime::Timed {
            start,
            end,
            time_zone,
        } => {
            let zone = zone.or_else(|| time_zone.parse::<Tz>().ok());
            let format = |value: &str| {
                DateTime::parse_from_rfc3339(value)
                    .ok()
                    .zip(zone)
                    .map(|(value, zone)| value.with_timezone(&zone).to_rfc3339())
                    .unwrap_or_else(|| value.into())
            };
            format!(
                "{} → {} ({})",
                format(start),
                format(end),
                zone.map(|z| z.name()).unwrap_or(time_zone)
            )
        }
    }
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn describe(event: &CalendarEvent) -> String {
    let mut output = format!(
        "ID: {}\nCalendar: {}\nTitle: {}\nTime: {}\nLocation: {}\nNotes: {}\n",
        event.id,
        event.calendar_id,
        single_line(&event.title),
        display_time(event, None),
        single_line(&event.location),
        single_line(&event.notes)
    );
    if let Some(rule) = &event.recurrence {
        let frequency = match rule.frequency {
            RepeatFrequency::Daily => "daily",
            RepeatFrequency::Weekly => "weekly",
            RepeatFrequency::Monthly => "monthly",
            RepeatFrequency::Yearly => "yearly",
        };
        writeln!(output, "Repeat: {frequency}, interval {}", rule.interval).expect("write string");
        if !rule.weekdays.is_empty() {
            writeln!(output, "Weekdays: {:?}", rule.weekdays).expect("write string");
        }
        if rule.frequency == RepeatFrequency::Monthly {
            writeln!(
                output,
                "Monthly on: {}",
                if rule.monthly_on == MonthlyOn::Weekday {
                    "weekday"
                } else {
                    "date"
                }
            )
            .expect("write string");
        }
        if let Some(until) = &rule.until {
            writeln!(output, "Until: {until}").expect("write string");
        }
        if let Some(count) = rule.count {
            writeln!(output, "Count: {count}").expect("write string");
        }
    }
    output
}

fn calendar_details(fields: CalendarArgs, previous: Option<&Calendar>) -> Result<CalendarDetails> {
    aika_core::repository::Repository::validate_details(CalendarDetails {
        name: required(
            &fields.name.or_else(|| previous.map(|c| c.name.clone())),
            "name",
        )?,
        description: fields
            .description
            .unwrap_or_else(|| previous.map(|c| c.description.clone()).unwrap_or_default()),
        color: fields.color.unwrap_or_else(|| {
            previous
                .map(|c| c.color.clone())
                .unwrap_or_else(|| "#718675".into())
        }),
    })
}

fn find_calendar<'a>(snapshot: &'a CalendarManagementSnapshot, id: &str) -> Result<&'a Calendar> {
    snapshot
        .calendars
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| {
            error(
                "notFound",
                "This calendar no longer exists. Use calendars list to see exact IDs.",
            )
        })
}

fn describe_calendar(calendar: &Calendar, snapshot: &CalendarManagementSnapshot) -> String {
    format!("ID: {}\nName: {}\nDescription: {}\nColor: {}\nProvider: {}\nWritable: {}\nDefault: {}\nVisible: {}\n",
        single_line(&calendar.id), single_line(&calendar.name), single_line(&calendar.description),
        single_line(&calendar.color), single_line(&calendar.provider_id), calendar.writable,
        snapshot.default_calendar_id.as_deref() == Some(calendar.id.as_str()),
        !snapshot.hidden_calendar_ids.contains(&calendar.id))
}

fn open_service(path: &std::path::Path) -> Result<CalendarService> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|_| error("storage", "Could not create database directory."))?;
    }
    CalendarService::open(path)
}

fn run_calendars(path: &std::path::Path, command: Calendars) -> Result<String> {
    let created = match &command {
        Calendars::Create(fields) => Some(calendar_details(fields.clone(), None)?),
        Calendars::Update { fields, .. }
            if fields.name.is_none() && fields.description.is_none() && fields.color.is_none() =>
        {
            return Err(error(
                "validation",
                "Supply --name, --description, or --color to update a calendar.",
            ));
        }
        _ => None,
    };
    let mut service = open_service(path)?;
    match command {
        Calendars::List => {
            let snapshot = service.calendar_management_snapshot()?;
            let mut output = String::new();
            for calendar in &snapshot.calendars {
                writeln!(
                    output,
                    "{}  {}  {}  default={}  visible={}",
                    single_line(&calendar.id),
                    single_line(&calendar.name),
                    single_line(&calendar.color),
                    snapshot.default_calendar_id.as_deref() == Some(calendar.id.as_str()),
                    !snapshot.hidden_calendar_ids.contains(&calendar.id)
                )
                .expect("write string");
            }
            Ok(output)
        }
        Calendars::Get { id } => {
            let snapshot = service.calendar_management_snapshot()?;
            Ok(describe_calendar(find_calendar(&snapshot, &id)?, &snapshot))
        }
        Calendars::Create(_) => {
            let calendar = service.create_calendar(created.expect("create input parsed"))?;
            Ok(describe_calendar(
                &calendar,
                &service.calendar_management_snapshot()?,
            ))
        }
        Calendars::Update { id, fields } => {
            let snapshot = service.calendar_management_snapshot()?;
            let previous = find_calendar(&snapshot, &id)?.clone();
            let details = calendar_details(fields, Some(&previous))?;
            let updated = Calendar {
                name: details.name.clone(),
                description: details.description.clone(),
                color: details.color.clone(),
                ..previous.clone()
            };
            service.update_calendar(&id, details, previous)?;
            Ok(describe_calendar(
                &updated,
                &service.calendar_management_snapshot()?,
            ))
        }
        Calendars::Delete { id, force } => {
            service.delete_calendar_guarded(&id, force)?;
            Ok(format!("Deleted {}\n", single_line(&id)))
        }
        Calendars::SetDefault { id } => {
            service.set_default_calendar(&id)?;
            Ok(format!("Default calendar: {}\n", single_line(&id)))
        }
        Calendars::Show { id } => {
            service.set_calendar_visibility(&id, true)?;
            Ok(format!("Shown {}\n", single_line(&id)))
        }
        Calendars::Hide { id } => {
            service.set_calendar_visibility(&id, false)?;
            Ok(format!("Hidden {}\n", single_line(&id)))
        }
    }
}

fn run(cli: Cli) -> Result<String> {
    let path = match cli.database {
        Some(path) => path,
        None => storage::database_path()?,
    };
    // Parse inputs before opening (or initializing) a database.
    let command = match cli.command {
        Command::Calendars { command } => return run_calendars(&path, command),
        Command::Events { command } => *command,
    };
    let listing = if let Events::List {
        from,
        to,
        time_zone,
    } = &command
    {
        let zone = match time_zone {
            Some(zone) => zone.clone(),
            None => system_zone()?,
        };
        let tz = parse_zone(&zone)?;
        let range = range(from, to, &zone)?;
        validate_range(&range)?;
        Some((range, tz))
    } else {
        None
    };
    let created = if let Events::Create(fields) = &command {
        Some(validate_event(event_from_args(fields.clone(), None)?)?)
    } else {
        None
    };
    if let Events::Get { id } | Events::Update { id, .. } | Events::Delete { id, .. } = &command {
        if uuid::Uuid::parse_str(id.split_once('@').map_or(id.as_str(), |(id, _)| id)).is_err() {
            return Err(error(
                "validation",
                "Use the full event UUID or occurrence selector.",
            ));
        }
    }
    let mut service = open_service(&path)?;
    match command {
        Events::List { .. } => {
            let (range, zone) = listing.expect("list input parsed");
            let snapshot = service.collection_snapshot(range)?;
            let mut output = String::new();
            for event in snapshot.events {
                writeln!(
                    output,
                    "{}  {}  {}",
                    event.id,
                    display_time(&event, Some(zone)),
                    single_line(&event.title)
                )
                .expect("write string");
            }
            Ok(output)
        }
        Events::Get { id } => {
            if let Some((_, key)) = id.split_once('@') {
                let series = service.get_series(&id)?;
                Ok(describe(&aika_core::recurrence::get_occurrence(
                    &series, key,
                )?))
            } else {
                Ok(describe(&service.get(&id)?))
            }
        }
        Events::Create(_) => {
            let mut event = created.expect("create input parsed");
            event.calendar_id = service.default_calendar_id()?.ok_or_else(|| {
                error(
                    "notFound",
                    "Create a local calendar with calendars create --name NAME first.",
                )
            })?;
            Ok(describe(&service.save_checked(event, false, None)?))
        }
        Events::Update { id, fields } => {
            let series = service.get_series(&id)?;
            let scope = mutation_scope(fields.scope.as_deref(), series.event.recurrence.is_some())?;
            if scope == MutationScope::Occurrence
                && (fields.repeat.is_some()
                    || fields.no_repeat
                    || fields.interval.is_some()
                    || fields.weekdays.is_some()
                    || fields.monthly_on.is_some()
                    || fields.until.is_some()
                    || fields.count.is_some()
                    || fields.forever)
            {
                return Err(error(
                    "validation",
                    "Changing a repeat pattern requires series or following scope.",
                ));
            }
            let reset_exceptions = fields.reset_exceptions;
            let count_changed = fields.count.is_some();
            let previous = if scope == MutationScope::Series {
                series.event.clone()
            } else {
                let (_, key) = id.split_once('@').ok_or_else(|| {
                    error("validation", "This scope requires an occurrence selector.")
                })?;
                aika_core::recurrence::get_occurrence(&series, key)?
            };
            let event = event_from_args(fields, Some(&previous))?;
            if series.event.recurrence.is_none() {
                return Ok(describe(&service.save_checked(
                    event,
                    true,
                    Some(previous),
                )?));
            }
            let output = describe(&event);
            let target = if scope == MutationScope::Series {
                series.event.id.clone()
            } else {
                id
            };
            let saved = service.mutate_event(EventMutation {
                target,
                scope: scope.clone(),
                event: Some(event),
                expected: aika_core::recurrence::expectation(&series)?,
                reset_exceptions,
                count_changed,
            })?;
            Ok(if scope == MutationScope::Following {
                saved.last().map(describe).unwrap_or(output)
            } else {
                output
            })
        }
        Events::Delete { id, scope } => {
            let series = service.get_series(&id)?;
            let scope = mutation_scope(scope.as_deref(), series.event.recurrence.is_some())?;
            if series.event.recurrence.is_none() {
                service.delete_checked(&id, &series.event)?;
            } else {
                service.mutate_event(EventMutation {
                    target: id.clone(),
                    scope,
                    event: None,
                    expected: aika_core::recurrence::expectation(&series)?,
                    reset_exceptions: false,
                    count_changed: false,
                })?;
            }
            Ok(format!("Deleted {id}\n"))
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!(
                "{}: {}{}",
                err.code,
                err.message,
                if err.code == "exceptionsResetRequired" {
                    " Supply --reset-exceptions to confirm."
                } else {
                    ""
                }
            );
            ExitCode::from(match err.code.as_str() {
                "validation" | "ambiguousTime" | "nonexistentTime" | "exceptionsResetRequired" => 2,
                "notFound" => 3,
                "conflict" => 4,
                _ => 1,
            })
        }
    }
}
