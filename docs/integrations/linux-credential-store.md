# Linux credential-store troubleshooting

aika stores Google account tokens through the desktop's D-Bus Secret Service using the Rust `keyring` crate's `sync-secret-service` backend. An AppImage still needs a working Secret Service in the host desktop session; the package does not supply a private password store. The OAuth client settings in `.env` configure the application, not a signed-in Google account.

## Failure before Google sign-in

Choosing Google Calendar first reads a nonexistent probe credential. This checks store access before opening the browser. In keyring 3.6.3, a failed search also checks the default collection for legacy entries. A missing default collection can therefore fail this check even when the Secret Service process is running. A locked collection, a dismissed unlock prompt, a missing provider, or an inaccessible session bus can also prevent connection.

Older aika builds flatten every store error into “Unlock or restore access to the system credential store before connecting an account.” That message does not establish that the store is locked. Current source distinguishes inaccessible storage, Linux platform/service failures, duplicate credentials, and unreadable credentials with static recovery guidance. It intentionally omits raw backend errors, which can contain private attributes or bytes. The error code remains `credentialUnavailable`; local and cached calendars remain available.

On the affected machine, run these commands as your normal desktop user, from a terminal in the same login session as aika:

```sh
busctl --user status org.freedesktop.secrets
gdbus call --session --dest org.freedesktop.secrets \
  --object-path /org/freedesktop/secrets \
  --method org.freedesktop.Secret.Service.ReadAlias default
```

These inspect service metadata without reading saved passwords. A missing service or failed bus connection points to session/provider availability. A returned object path of `/` means no default collection exists. With a real collection path, inspect its lock state by substituting that path:

```sh
gdbus call --session --dest org.freedesktop.secrets \
  --object-path /org/freedesktop/secrets/collection/REPLACE_WITH_RETURNED_PATH \
  --method org.freedesktop.DBus.Properties.Get \
  org.freedesktop.Secret.Collection Locked
```

Use the desktop password manager to unlock the existing keyring/wallet and approve access prompts. If no default collection exists, create or designate a password keyring with that manager. On GNOME, Passwords and Keys (Seahorse) manages keyrings; on KDE, inspect the Secret Service provider identified by `busctl` and its wallet configuration. If no provider is installed, configure one appropriate to that desktop. Launch aika as the same desktop user, without `sudo`. Retry after restoring access. Replacing or deleting existing keyring files is unnecessary for these checks.

GNOME login integration normally unlocks the login keyring using the login password. Automatic or fingerprint login can leave it locked; manually unlocking through the password manager distinguishes that case. See [GNOME's PAM documentation](https://wiki.gnome.org/Projects/GnomeKeyring/Pam) and [keyring 3.6.3's backend implementation](https://docs.rs/keyring/3.6.3/src/keyring/secret_service.rs.html). Fedora 44 ships [GNOME Keyring](https://packages.fedoraproject.org/pkgs/gnome-keyring/gnome-keyring/fedora-44.html), but identify the actual desktop/provider rather than assuming it is running.

The exact Fedora failure remains unconfirmed until the service, default collection, and lock state are checked on that machine. Switching package format alone does not establish a fix for a desktop credential-store failure.

## Investigation evidence, 2026-10-19

The investigation host runs Omarchy, not Fedora 44. Its session bus identifies GNOME Keyring as the Secret Service provider, with a default collection. The opt-in `real_os_store_roundtrip` test passed, using a disposable credential and deleting it afterward:

```sh
pnpm rust test --manifest-path src-tauri/Cargo.toml -p aika-core real_os_store_roundtrip -- --ignored
pnpm desktop:test:build
```

The debug native build succeeded. WebdriverIO automation then opened **+ → Google Calendar** in the real embedded webview with isolated app data and the locally configured OAuth client. The credential preflight and backend session startup succeeded. The authorization URL used Google Accounts, fresh state, S256 PKCE, and offline access. For browser automation only, the `xdg-open` launcher was redirected to a fresh Chromium profile; the Google page itself was live. Google presented “Sign in to continue to aika” with its account identifier field. Cancellation returned to the local calendar with event creation still available. A screenshot is saved locally in `artifacts/investigation/google-response.png`; no account was entered.

Core regression tests passed: 84 passed, 2 opt-in tests ignored. The real-store test above passed separately. The core suite initially had three callback-listener failures because sandbox restrictions denied loopback sockets; rerunning with socket access passed. Rust formatting, documentation formatting, and whitespace checks passed. The new credential error test checks recovery guidance and verifies that sensitive backend details do not enter the returned error.

This establishes native startup through the live Google sign-in page on this host. It does not establish installed AppImage behavior on Fedora, actual default-browser dispatch, successful account consent, token exchange/refresh, calendar discovery, or synchronization. The `.env` has application OAuth configuration, not a Google user login or refresh token. Those remaining live checks need a test account to complete authorization.
