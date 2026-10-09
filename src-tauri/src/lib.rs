pub mod contracts;

use contracts::AppInfo;

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

pub fn run() {
    tauri::Builder::default()
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
        .invoke_handler(tauri::generate_handler![get_app_info])
        .run(tauri::generate_context!())
        .expect("could not start aika");
}
