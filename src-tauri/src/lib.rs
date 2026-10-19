mod sync_runtime;
pub use aika_core::{contracts, credentials, providers, service};

use contracts::*;
use service::{error, CalendarService};
use std::sync::{Arc, Mutex};
use tauri::Manager;
struct LocalState {
    directory: Result<std::path::PathBuf, ServiceError>,
    service: Option<CalendarService>,
}
type ServiceState = Arc<Mutex<LocalState>>;

async fn with_service<T: Send + 'static>(
    state: ServiceState,
    work: impl FnOnce(&mut CalendarService) -> Result<T, ServiceError> + Send + 'static,
) -> Result<T, ServiceError> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = state
            .lock()
            .map_err(|_| error("service", "The calendar service is unavailable."))?;
        if guard.service.is_none() {
            let directory = guard.directory.as_ref().map_err(Clone::clone)?;
            std::fs::create_dir_all(directory)
                .map_err(|_| error("storage", "Could not create application data directory."))?;
            guard.service = Some(CalendarService::open(&directory.join("calendar.sqlite3"))?);
        }
        work(guard.service.as_mut().expect("service initialized"))
    })
    .await
    .map_err(|_| error("service", "The calendar operation could not finish."))?
}

// Adapter work is deliberately performed outside ServiceState / SQLite locks.
async fn provider_work<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ServiceError> + Send + 'static,
) -> Result<T, ServiceError> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|_| error("service", "The provider operation could not finish."))?
}
#[tauri::command]
async fn sync_conflicts(
    state: tauri::State<'_, ServiceState>,
) -> Result<Vec<SyncConflict>, ServiceError> {
    with_service(state.inner().clone(), |s| s.sync_conflicts()).await
}
#[tauri::command]
async fn resolve_sync_conflict(
    state: tauri::State<'_, ServiceState>,
    expected: SyncConflict,
    keep_local: bool,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.resolve_sync_conflict(expected, keep_local)
    })
    .await
}
#[tauri::command]
async fn retry_outgoing(
    state: tauri::State<'_, ServiceState>,
    account_id: String,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.retry_outgoing(&account_id)
    })
    .await
}
#[tauri::command]
async fn list_providers(
    state: tauri::State<'_, ServiceState>,
) -> Result<Vec<ProviderDescriptor>, ServiceError> {
    with_service(state.inner().clone(), |s| Ok(s.providers())).await
}
#[tauri::command]
async fn start_connection(
    state: tauri::State<'_, ServiceState>,
    provider: String,
) -> Result<ConnectionSession, ServiceError> {
    let adapter = with_service(state.inner().clone(), move |s| {
        s.connection_adapter(&provider)
    })
    .await?;
    provider_work(move || adapter.start_connection()).await
}
#[tauri::command]
async fn connection_status(
    state: tauri::State<'_, ServiceState>,
    provider: String,
    id: String,
) -> Result<ConnectionSession, ServiceError> {
    let key = provider.clone();
    let adapter = with_service(state.inner().clone(), move |s| s.connection_adapter(&key)).await?;
    let (session, identity) = provider_work(move || adapter.connection(&id)).await?;
    with_service(state.inner().clone(), move |s| {
        s.record_connection(&provider, session, identity)
    })
    .await
}
#[tauri::command]
async fn cancel_connection(
    state: tauri::State<'_, ServiceState>,
    provider: String,
    id: String,
) -> Result<(), ServiceError> {
    let adapter = with_service(state.inner().clone(), move |s| {
        s.connection_adapter(&provider)
    })
    .await?;
    provider_work(move || adapter.cancel_connection(&id)).await
}
#[tauri::command]
async fn discover_calendars(
    state: tauri::State<'_, ServiceState>,
    account_id: String,
) -> Result<CalendarDiscovery, ServiceError> {
    let (account, adapter) = with_service(state.inner().clone(), move |s| {
        let account = s.account(&account_id)?;
        let adapter = s.connection_adapter(&account.provider_id)?;
        Ok((account, adapter))
    })
    .await?;
    if account.connection_state != ConnectionState::Connected {
        return Err(error(
            "authenticationRequired",
            "Reconnect this account before discovering calendars.",
        ));
    }
    let subject = account.remote_subject.clone();
    let result = provider_work(move || adapter.discover(&subject)).await;
    with_service(state.inner().clone(), move |s| match result {
        Ok(calendars) => s.prepare_discovery(&account, calendars),
        Err(failure) => {
            s.account_failure(&account.id, &failure)?;
            Err(failure)
        }
    })
    .await
}
#[tauri::command]
async fn commit_calendar_selection(
    state: tauri::State<'_, ServiceState>,
    discovery_id: String,
    selected: Vec<String>,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.commit_selection(&discovery_id, selected)
    })
    .await
}
#[tauri::command]
async fn review_calendar_removal(
    state: tauri::State<'_, ServiceState>,
    calendar_id: String,
) -> Result<CalendarRemovalReview, ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.review_calendar_removal(&calendar_id)
    })
    .await
}
#[tauri::command]
async fn discard_and_remove_calendar(
    state: tauri::State<'_, ServiceState>,
    expected: CalendarRemovalReview,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.discard_and_remove_calendar(expected)
    })
    .await
}
#[tauri::command]
async fn review_account_removal(
    state: tauri::State<'_, ServiceState>,
    account_id: String,
) -> Result<AccountRemovalReview, ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.review_account_removal(&account_id)
    })
    .await
}
#[tauri::command]
async fn remove_account(
    state: tauri::State<'_, ServiceState>,
    expected: AccountRemovalReview,
    discard: bool,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.remove_account(expected, discard)
    })
    .await
}
#[tauri::command]
async fn review_disconnect(
    state: tauri::State<'_, ServiceState>,
    account_id: String,
) -> Result<DisconnectReview, ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.review_disconnect(&account_id)
    })
    .await
}
#[tauri::command]
async fn discard_and_disconnect(
    state: tauri::State<'_, ServiceState>,
    expected: DisconnectReview,
) -> Result<(), ServiceError> {
    let (account, adapter, epoch) = with_service(state.inner().clone(), move |s| {
        let account = s.account(&expected.account_id)?;
        let adapter = s.connection_adapter(&account.provider_id)?;
        let epoch = adapter.credential_epoch()?;
        s.discard_and_disconnect(expected)?;
        Ok((account, adapter, epoch))
    })
    .await?;
    provider_work(move || adapter.disconnect_checked(&account.remote_subject, epoch)).await
}
#[tauri::command]
async fn disconnect_account(
    state: tauri::State<'_, ServiceState>,
    account_id: String,
) -> Result<(), ServiceError> {
    let (account, adapter, epoch) = with_service(state.inner().clone(), move |s| {
        let account = s.account(&account_id)?;
        let adapter = s.connection_adapter(&account.provider_id)?;
        let epoch = adapter.credential_epoch()?;
        // Fence workers and concurrent CLI mutations before removing credentials.
        s.record_disconnect(&account.id)?;
        Ok((account, adapter, epoch))
    })
    .await?;
    let subject = account.remote_subject;
    provider_work(move || adapter.disconnect_checked(&subject, epoch)).await
}

#[tauri::command]
async fn calendar_snapshot(
    state: tauri::State<'_, ServiceState>,
    range: EventRange,
) -> Result<CalendarCollectionSnapshot, ServiceError> {
    with_service(state.inner().clone(), move |s| s.collection_snapshot(range)).await
}

#[tauri::command]
async fn create_calendar(
    state: tauri::State<'_, ServiceState>,
    details: CalendarDetails,
) -> Result<Calendar, ServiceError> {
    with_service(state.inner().clone(), move |s| s.create_calendar(details)).await
}
#[tauri::command]
async fn update_calendar(
    state: tauri::State<'_, ServiceState>,
    id: String,
    details: CalendarDetails,
    expected: Calendar,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.update_calendar(&id, details, expected)
    })
    .await
}
#[tauri::command]
async fn delete_calendar(
    state: tauri::State<'_, ServiceState>,
    id: String,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| s.delete_calendar(&id)).await
}
#[tauri::command]
async fn calendar_event_count(
    state: tauri::State<'_, ServiceState>,
    id: String,
) -> Result<usize, ServiceError> {
    with_service(state.inner().clone(), move |s| s.calendar_event_count(&id)).await
}
#[tauri::command]
async fn set_default_calendar(
    state: tauri::State<'_, ServiceState>,
    id: String,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| s.set_default_calendar(&id)).await
}
#[tauri::command]
async fn set_calendar_visibility(
    state: tauri::State<'_, ServiceState>,
    id: String,
    visible: bool,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.set_calendar_visibility(&id, visible)
    })
    .await
}

#[tauri::command]
async fn get_event_series(
    state: tauri::State<'_, ServiceState>,
    target: String,
) -> Result<EventSeries, ServiceError> {
    with_service(state.inner().clone(), move |s| s.get_series(&target)).await
}
#[tauri::command]
async fn mutate_recurring_event(
    state: tauri::State<'_, ServiceState>,
    request: EventMutation,
) -> Result<Vec<CalendarEvent>, ServiceError> {
    with_service(state.inner().clone(), move |s| s.mutate_event(request)).await
}
#[tauri::command]
async fn meeting_details(
    state: tauri::State<'_, ServiceState>,
    target: String,
) -> Result<MeetingDetails, ServiceError> {
    with_service(state.inner().clone(), move |s| s.meeting_details(&target)).await
}
#[tauri::command]
async fn mutate_meeting(
    state: tauri::State<'_, ServiceState>,
    request: MeetingMutation,
) -> Result<Vec<CalendarEvent>, ServiceError> {
    with_service(state.inner().clone(), move |s| s.mutate_meeting(request)).await
}
#[tauri::command]
async fn preview_recurrence(
    event: CalendarEvent,
    series: Option<EventSeries>,
    request: Option<EventMutation>,
) -> Result<Vec<CalendarEvent>, ServiceError> {
    tauri::async_runtime::spawn_blocking(move || {
        if let (Some(series), Some(request)) = (series, request) {
            let changes = aika_core::recurrence::plan_mutation(
                &series,
                &request,
                "a86f6db8-7f44-47c5-a5ca-25d32f9bed3f",
            )?;
            return changes
                .last()
                .and_then(|c| c.after.as_ref())
                .map(|s| aika_core::recurrence::preview(s.event.clone()))
                .unwrap_or(Ok(vec![]));
        }
        aika_core::recurrence::preview(event)
    })
    .await
    .map_err(|_| error("service", "Could not preview repetition."))?
}

#[tauri::command]
async fn save_event(
    state: tauri::State<'_, ServiceState>,
    event: CalendarEvent,
    existing: bool,
    expected: Option<CalendarEvent>,
) -> Result<CalendarEvent, ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.save_checked(event, existing, expected)
    })
    .await
}
#[tauri::command]
async fn delete_event(
    state: tauri::State<'_, ServiceState>,
    id: String,
    expected: CalendarEvent,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.delete_checked(&id, &expected)
    })
    .await
}
#[tauri::command]
async fn undo_event(state: tauri::State<'_, ServiceState>) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), |s| s.undo()).await
}
#[tauri::command]
async fn get_preferences(
    state: tauri::State<'_, ServiceState>,
) -> Result<Preferences, ServiceError> {
    with_service(state.inner().clone(), |s| s.preferences()).await
}
#[tauri::command]
async fn set_preferences(
    state: tauri::State<'_, ServiceState>,
    preferences: Preferences,
) -> Result<(), ServiceError> {
    with_service(state.inner().clone(), move |s| {
        s.set_preferences(preferences)
    })
    .await
}
#[tauri::command]
fn resolve_local_time(
    local: String,
    time_zone: String,
    offset: Option<i32>,
) -> Result<String, ServiceError> {
    service::resolve_local(&local, &time_zone, offset)
}

#[tauri::command]
fn get_app_info() -> AppInfo {
    #[cfg(debug_assertions)]
    eprintln!("aika: native IPC ready");
    AppInfo {
        name: "aika".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        provider: "local".into(),
    }
}

#[cfg(all(feature = "native-test", not(debug_assertions)))]
compile_error!("native-test is restricted to debug test builds");

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(feature = "native-test")]
    let builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .setup(|app| {
            #[cfg(not(feature = "native-test"))]
            let directory = aika_core::storage::app_data_dir();
            #[cfg(feature = "native-test")]
            let directory = std::env::var_os("AIKA_TEST_DATA_DIR")
                .map(std::path::PathBuf::from)
                .ok_or_else(|| error("storage", "Native tests require AIKA_TEST_DATA_DIR."));
            app.manage(sync_runtime::SyncQueue::default());
            app.manage(Arc::new(Mutex::new(LocalState { directory, service: None })) as ServiceState);
            Ok(())
        })
        .on_page_load(|_window, _payload| {
            #[cfg(debug_assertions)]
            if _payload.event() == tauri::webview::PageLoadEvent::Finished {
                let window = _window.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let _ = window.eval_with_callback(
                        "({ rootChildren: document.getElementById('root')?.childElementCount ?? 0, runtime: document.querySelector('[data-runtime]')?.getAttribute('data-runtime') ?? 'unmounted' })",
                        |result| eprintln!("aika: startup check {result}"),
                    );
                });
            }
        })
        .invoke_handler(tauri::generate_handler![get_app_info, meeting_details, mutate_meeting, sync_conflicts, resolve_sync_conflict, retry_outgoing, sync_runtime::request_sync, list_providers, start_connection, connection_status, cancel_connection, discover_calendars, commit_calendar_selection, review_calendar_removal, discard_and_remove_calendar, review_account_removal, remove_account, review_disconnect, discard_and_disconnect, disconnect_account, calendar_snapshot, create_calendar, update_calendar, delete_calendar, calendar_event_count, set_default_calendar, set_calendar_visibility, save_event, get_event_series, mutate_recurring_event, preview_recurrence, delete_event, undo_event, get_preferences, set_preferences, resolve_local_time])
        .run(tauri::generate_context!())
        .expect("could not start aika");
}
