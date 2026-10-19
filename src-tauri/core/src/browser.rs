//! System-browser launch stays in the backend; OAuth URLs and process diagnostics are not logged.
use crate::contracts::ServiceError;
use crate::service::error;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn launch_error() -> ServiceError {
    error(
        "browser",
        "Could not open the system browser. Check your default browser, then try signing in again.",
    )
}

#[cfg(target_os = "linux")]
fn linux_command(environment: &[(std::ffi::OsString, std::ffi::OsString)]) -> Command {
    let appdir = environment
        .iter()
        .find(|(key, value)| key == "APPDIR" && !value.is_empty())
        .map(|(_, value)| std::path::PathBuf::from(value));
    // AppImages can prepend a bundled xdg-open built for a different distribution.
    let mut command = Command::new(
        if appdir.is_some() && std::path::Path::new("/usr/bin/xdg-open").is_file() {
            "/usr/bin/xdg-open"
        } else {
            "xdg-open"
        },
    );
    if let Some(appdir) = appdir {
        filter_appimage_environment(&mut command, environment, &appdir);
    }
    command
}

#[cfg(target_os = "linux")]
fn filter_appimage_environment(
    command: &mut Command,
    environment: &[(std::ffi::OsString, std::ffi::OsString)],
    appdir: &std::path::Path,
) {
    // Keep host paths and session configuration, but don't inject bundled libraries,
    // Qt plugins, or desktop data into Fedora's launcher and browser processes.
    for (key, value) in environment {
        if matches!(
            key.to_str(),
            Some(
                "PATH"
                    | "LD_LIBRARY_PATH"
                    | "LD_PRELOAD"
                    | "QT_PLUGIN_PATH"
                    | "QT_QPA_PLATFORM_PLUGIN_PATH"
                    | "XDG_DATA_DIRS"
                    | "GIO_EXTRA_MODULES"
                    | "GSETTINGS_SCHEMA_DIR"
                    | "GTK_PATH"
                    | "GTK_EXE_PREFIX"
                    | "GTK_DATA_PREFIX"
            )
        ) {
            let paths: Vec<_> = std::env::split_paths(value)
                .filter(|path| !path.starts_with(appdir))
                .collect();
            if paths.is_empty() {
                command.env_remove(key);
            } else if let Ok(filtered) = std::env::join_paths(paths) {
                command.env(key, filtered);
            }
        }
    }
}

pub(crate) fn open(url: &url::Url) -> Result<(), ServiceError> {
    #[cfg(target_os = "linux")]
    let mut command = linux_command(&std::env::vars_os().collect::<Vec<_>>());
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    command.arg(url.as_str());
    launch(&mut command, Duration::from_secs(3))
}

fn launch(command: &mut Command, grace: Duration) -> Result<(), ServiceError> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| launch_error())?;
    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(launch_error())
                }
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            result => {
                // Some launchers stay attached to a newly opened browser. Don't block OAuth
                // on that browser closing; keep a reaper so it never becomes a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return result.map(|_| ()).map_err(|_| launch_error());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn launcher_exit_failure_and_missing_executable_are_reported_without_diagnostics() {
        let mut failed = Command::new("/bin/sh");
        failed.args(["-c", "echo private-oauth-url >&2; exit 4"]);
        let failure = launch(&mut failed, Duration::from_secs(1)).unwrap_err();
        assert_eq!(failure.code, "browser");
        assert!(!format!("{failure:?}").contains("private-oauth-url"));
        assert!(launch(
            &mut Command::new("/nonexistent/aika-browser"),
            Duration::ZERO
        )
        .is_err());
        assert!(launch(&mut Command::new("/bin/true"), Duration::from_secs(1)).is_ok());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn appimage_launch_filters_bundled_paths_and_preserves_host_session() {
        let environment: Vec<_> = [
            ("APPDIR", "/tmp/aika.AppDir"),
            ("PATH", "/tmp/aika.AppDir/usr/bin:/usr/bin"),
            ("LD_LIBRARY_PATH", "/tmp/aika.AppDir/usr/lib:/opt/host/lib"),
            ("QT_PLUGIN_PATH", "/tmp/aika.AppDir/usr/plugins"),
            ("XDG_DATA_DIRS", "/tmp/aika.AppDir/usr/share:/usr/share"),
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let mut command = Command::new("/bin/sh");
        command.env_clear().envs(environment.clone());
        filter_appimage_environment(
            &mut command,
            &environment,
            std::path::Path::new("/tmp/aika.AppDir"),
        );
        command.args([
            "-c",
            r#"
            test "$PATH" = /usr/bin &&
            test "$LD_LIBRARY_PATH" = /opt/host/lib &&
            test "${QT_PLUGIN_PATH-unset}" = unset &&
            test "$XDG_DATA_DIRS" = /usr/share &&
            test "$DBUS_SESSION_BUS_ADDRESS" = unix:path=/run/user/1000/bus
        "#,
        ]);
        assert!(command.status().unwrap().success());
        assert_eq!(linux_command(&[]).get_program(), "xdg-open");
    }
}
