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

#[tauri::command]
async fn calendar_snapshot(
    state: tauri::State<'_, ServiceState>,
    range: EventRange,
) -> Result<CalendarSnapshot, ServiceError> {
    with_service(state.inner().clone(), move |s| s.snapshot(range)).await
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
        .invoke_handler(tauri::generate_handler![get_app_info, calendar_snapshot, save_event, delete_event, undo_event, get_preferences, set_preferences, resolve_local_time])
        .run(tauri::generate_context!())
        .expect("could not start aika");
}
