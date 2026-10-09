#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK DMA-BUF rendering stalled editor animations on the Linux host.
    // Set the compatibility default before GTK/WebKit or worker threads start.
    // An explicit environment value lets users retain the accelerated path.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    cal_desktop_lib::run()
}
