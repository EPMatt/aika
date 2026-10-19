# Google sign-in button and Linux browser launch

The connection popup now waits for an explicit **Sign in with Google** click before probing credentials or starting OAuth. Existing connected accounts still load their calendar checklist when managed. Startup cancellation and late-session cleanup remain in place.

The previous browser launcher reported success after spawning a process and discarded its exit status. The new backend launcher reports missing executables and nonzero exits detected during a bounded three-second startup window. The existing connection polling surfaces that failure and restores the sign-in button for retry. Launch output and OAuth URLs are never included in errors or logs. Launchers that remain attached to a browser after the startup window are reaped asynchronously; a successful launcher invocation cannot prove that a browser window is visible.

On Linux AppImages, the launcher prefers the host `/usr/bin/xdg-open` when present, with a PATH-based fallback for other layouts. It removes paths inside `APPDIR` from the child process's executable, library, plugin, and desktop-data search variables while preserving host paths and the desktop session environment. Other package formats retain ordinary PATH lookup. This addresses a known class of AppImage browser launch failures; the precise failing launcher on the reported Fedora machine was not directly observed. See the [upstream Tauri report](https://github.com/tauri-apps/tauri/issues/10617).

Regression coverage checks explicit sign-in, immediate and polled launcher failures, retry, cancellation during startup, real process exit handling, OAuth listener cleanup on launch failure, and child-process isolation from simulated AppImage paths. No account tokens or OAuth contract fields were added to the renderer.

The fix requires rebuilding the installed AppImage. Fedora 44/KDE default-browser behavior and completed live Google authorization/discovery/synchronization remain platform checks; successful browser previews or local native tests do not establish them.

Verification on the Omarchy development host:

- Core suite: 88 passed, 2 opt-in tests ignored. Loopback socket access was enabled for OAuth callback tests.
- Connection UI suite: 4 passed, including both themes and accessibility checks. An initial dark-theme run timed out during a click; the updated suite passed on rerun.
- Frontend build, generated types, and debug native build succeeded. Documentation/renderer formatting and whitespace checks passed.
- The rebuilt native webview showed the sign-in button before any browser launch. A controlled `xdg-open` process exiting with code 4 produced the recovery error and restored the button. Retrying through an automated browser launcher reached Google's live account identifier page using the local OAuth configuration. Cancellation returned to the usable local calendar. This exercised the real native backend and keyring, but substituted the browser launcher; it does not verify Fedora's default browser dispatch.
- Local screenshots are under `artifacts/investigation/`: `google-sign-in-popup.png` and `google-response.png`. No Google user account was entered.
