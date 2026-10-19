//! Google protocol boundary. Credentials and OAuth material never cross IPC or SQLite.
use crate::contracts::*;
use crate::credentials::CredentialService;
use crate::providers::{ConnectedIdentity, Provider};
use crate::service::error;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;
#[path = "google_events.rs"]
pub(crate) mod events;
#[path = "google_outgoing.rs"]
mod outgoing;
type Result<T> = std::result::Result<T, ServiceError>;
const SCOPES: &str = "openid email https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events";
const TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone)]
struct ClientConfig {
    id: String,
    secret: Option<String>,
}
impl ClientConfig {
    fn resolve(
        runtime_id: Option<&str>,
        runtime_secret: Option<&str>,
        embedded_id: Option<&str>,
        embedded_secret: Option<&str>,
    ) -> Option<Self> {
        let value = |v: Option<&str>| {
            v.map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let runtime_id = value(runtime_id);
        let (id, secret) = if let Some(id) = runtime_id {
            (id, value(runtime_secret))
        } else {
            (value(embedded_id)?, value(embedded_secret))
        };
        Some(Self { id, secret })
    }
    fn environment() -> Option<Self> {
        Self::resolve(
            std::env::var("AIKA_GOOGLE_CLIENT_ID").ok().as_deref(),
            std::env::var("AIKA_GOOGLE_CLIENT_SECRET").ok().as_deref(),
            option_env!("AIKA_GOOGLE_CLIENT_ID"),
            option_env!("AIKA_GOOGLE_CLIENT_SECRET"),
        )
    }
}

#[derive(Serialize, Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: i64,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    token_type: String,
    scope: Option<String>,
}
impl TokenResponse {
    fn merge(self, previous: Option<Tokens>) -> Result<Tokens> {
        if !self.token_type.eq_ignore_ascii_case("Bearer")
            || self.access_token.is_empty()
            || self.expires_in <= 0
        {
            return Err(error(
                "authenticationRequired",
                "Google returned invalid credentials. Reconnect the account.",
            ));
        }
        if let Some(scope) = self.scope.as_ref() {
            let granted: BTreeSet<_> = scope.split_whitespace().collect();
            if !SCOPES.split_whitespace().all(|s| {
                granted.contains(s)
                    || (s == "email"
                        && granted.contains("https://www.googleapis.com/auth/userinfo.email"))
            }) {
                return Err(error(
                    "permission",
                    "Grant all requested permissions to connect Google Calendar.",
                ));
            }
        }
        let refresh_token = self
            .refresh_token
            .or_else(|| previous.and_then(|p| p.refresh_token));
        if refresh_token.as_ref().is_none_or(|v| v.is_empty()) {
            return Err(error(
                "authenticationRequired",
                "Google did not grant offline access. Reconnect and approve access.",
            ));
        }
        Ok(Tokens {
            access_token: self.access_token,
            refresh_token,
            expires_at: chrono::Utc::now()
                .timestamp()
                .saturating_add(self.expires_in),
        })
    }
}
struct Session {
    epoch: Option<i64>,
    public: ConnectionSession,
    identity: Option<ConnectedIdentity>,
    started: Instant,
}
struct Inner {
    config: Option<ClientConfig>,
    sessions: Mutex<BTreeMap<String, Session>>,
    credentials: CredentialService,
    // Serializes refresh/credential replacement for this provider without locking the repository.
    credential_work: Mutex<()>,
    lifecycle: Option<crate::lifecycle::Lifecycle>,
    transport: Arc<dyn GoogleTransport>,
}
pub struct GoogleProvider(Arc<Inner>);
impl GoogleProvider {
    pub(crate) fn with_lifecycle(lifecycle: Option<crate::lifecycle::Lifecycle>) -> Self {
        let mut provider = Self::default();
        Arc::get_mut(&mut provider.0).unwrap().lifecycle = lifecycle;
        provider
    }
}
impl Default for GoogleProvider {
    fn default() -> Self {
        Self(Arc::new(Inner {
            config: ClientConfig::environment(),
            sessions: Mutex::new(BTreeMap::new()),
            credentials: CredentialService::default(),
            credential_work: Mutex::new(()),
            lifecycle: None,
            transport: Arc::new(HttpTransport),
        }))
    }
}
fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| {
            error(
                "network",
                "Could not initialize a secure Google connection.",
            )
        })
}
fn decode<T: serde::de::DeserializeOwned>(response: reqwest::blocking::Response) -> Result<T> {
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            400 | 401 => error(
                "authenticationRequired",
                "Google authorization expired or was rejected. Reconnect the account.",
            ),
            410 => error(
                "cursorExpired",
                "Google synchronization cursor expired. A full refresh is required.",
            ),
            403 => error(
                "permission",
                "Google denied access. Check the account permissions.",
            ),
            429 | 500..=599 => error("retryable", "Google is temporarily unavailable. Try again."),
            _ => error("google", "Google could not complete this request."),
        });
    }
    // Limit provider response size; never return parser/network diagnostics containing secrets.
    serde_json::from_reader(response.take(4 * 1024 * 1024))
        .map_err(|_| error("google", "Google returned an invalid response."))
}
fn network(_: reqwest::Error) -> ServiceError {
    error(
        "network",
        "Could not reach Google. Check your connection and try again.",
    )
}
fn token_request(config: &ClientConfig, mut form: Vec<(&str, String)>) -> Result<TokenResponse> {
    form.push(("client_id", config.id.clone()));
    if let Some(secret) = &config.secret {
        form.push(("client_secret", secret.clone()));
    }
    decode(
        client()?
            .post("https://oauth2.googleapis.com/token")
            .form(&form)
            .send()
            .map_err(network)?,
    )
}
#[derive(Deserialize)]
struct Identity {
    sub: String,
    email: String,
    email_verified: bool,
}
trait GoogleTransport: Send + Sync {
    fn browser(&self, url: &Url) -> Result<()>;
    fn token(&self, config: &ClientConfig, form: Vec<(&str, String)>) -> Result<TokenResponse>;
    fn identity(&self, access: &str) -> Result<Identity>;
    fn event_page(
        &self,
        _access: &str,
        _calendar: &str,
        _cursor: Option<&str>,
        _page: Option<&str>,
        _projection: Option<&EventRange>,
    ) -> Result<events::EventPage> {
        Err(error("unsupported", "Event transport unavailable."))
    }
    fn event_request(
        &self,
        _access: &str,
        _calendar: &str,
        _event: &str,
        _method: &str,
        _body: Option<&serde_json::Value>,
        _revision: Option<&str>,
    ) -> Result<Option<serde_json::Value>> {
        Err(error(
            "unsupported",
            "Outgoing event transport unavailable.",
        ))
    }
    #[allow(clippy::too_many_arguments)]
    fn event_request_notifying(
        &self,
        access: &str,
        calendar: &str,
        event: &str,
        method: &str,
        body: Option<&serde_json::Value>,
        revision: Option<&str>,
        _policy: &NotificationPolicy,
    ) -> Result<Option<serde_json::Value>> {
        self.event_request(access, calendar, event, method, body, revision)
    }
    fn instance_page(
        &self,
        _access: &str,
        _calendar: &str,
        _series: &str,
        _original: &str,
        _page: Option<&str>,
    ) -> Result<events::EventPage> {
        Err(error("unsupported", "Instance lookup unavailable."))
    }
    fn calendar_page(&self, access: &str, page: Option<&str>) -> Result<CalendarPage>;
}
struct HttpTransport;
impl GoogleTransport for HttpTransport {
    fn browser(&self, url: &Url) -> Result<()> {
        open_browser(url)
    }
    fn token(&self, config: &ClientConfig, form: Vec<(&str, String)>) -> Result<TokenResponse> {
        token_request(config, form)
    }
    fn identity(&self, access: &str) -> Result<Identity> {
        decode(
            client()?
                .get("https://openidconnect.googleapis.com/v1/userinfo")
                .bearer_auth(access)
                .send()
                .map_err(network)?,
        )
    }
    fn event_page(
        &self,
        access: &str,
        calendar: &str,
        cursor: Option<&str>,
        page: Option<&str>,
        projection: Option<&EventRange>,
    ) -> Result<events::EventPage> {
        let url = event_url(calendar)?;
        let response = client()?
            .get(url)
            .bearer_auth(access)
            .query(&event_query(cursor, page, projection))
            .send()
            .map_err(network)?;
        match response.status().as_u16() {
            400 => Err(error(
                "google",
                "Google rejected the event synchronization request.",
            )),
            404 => Err(error(
                "permission",
                "This calendar is no longer available. Refresh its account selection.",
            )),
            _ => decode(response),
        }
    }

    fn instance_page(
        &self,
        access: &str,
        calendar: &str,
        series: &str,
        original: &str,
        page: Option<&str>,
    ) -> Result<events::EventPage> {
        if series.is_empty() || matches!(series, "." | "..") {
            return Err(error("validation", "Invalid series identity."));
        }
        let mut url = event_url(calendar)?;
        url.path_segments_mut()
            .map_err(|_| error("validation", "Invalid instance URL."))?
            .push(series)
            .push("instances");
        let mut query = vec![
            ("originalStart", original),
            ("showDeleted", "true"),
            ("maxResults", "2500"),
        ];
        if let Some(page) = page {
            query.push(("pageToken", page));
        }
        decode(
            client()?
                .get(url)
                .bearer_auth(access)
                .query(&query)
                .send()
                .map_err(network)?,
        )
    }

    fn event_request(
        &self,
        access: &str,
        calendar: &str,
        event: &str,
        method: &str,
        body: Option<&serde_json::Value>,
        revision: Option<&str>,
    ) -> Result<Option<serde_json::Value>> {
        self.event_request_notifying(
            access,
            calendar,
            event,
            method,
            body,
            revision,
            &NotificationPolicy::All,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn event_request_notifying(
        &self,
        access: &str,
        calendar: &str,
        event: &str,
        method: &str,
        body: Option<&serde_json::Value>,
        revision: Option<&str>,
        policy: &NotificationPolicy,
    ) -> Result<Option<serde_json::Value>> {
        if event.is_empty() || matches!(event, "." | "..") {
            return Err(error("providerIdentity", "Invalid remote event identity."));
        }
        let mut url = event_url(calendar)?;
        if method != "POST" {
            url.path_segments_mut().expect("fixed URL").push(event);
        }
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| error("unsupported", "Invalid event operation."))?;
        let mut request = client()?.request(method.clone(), url).bearer_auth(access);
        if method != reqwest::Method::GET {
            request = request.query(&[("sendUpdates", policy.as_str())]);
        }
        if let Some(revision) = revision {
            request = request.header(reqwest::header::IF_MATCH, revision);
        }
        if method != reqwest::Method::GET && method != reqwest::Method::DELETE {
            request = request.query(&[
                ("supportsAttachments", "true"),
                ("conferenceDataVersion", "1"),
            ]);
            if let Some(body) = body {
                request = request.json(body);
            }
        }
        let response = request.send().map_err(network)?;
        match response.status().as_u16() {
            204 => Ok(None),
            404 | 410 if method == reqwest::Method::POST => Err(error(
                "permission",
                "This calendar is unavailable. Restore access, then retry pending changes.",
            )),
            404 | 410 => Ok(None),
            409 | 412 => Err(error(
                "remoteConflict",
                "The Google event changed. Review both versions.",
            )),
            400 | 422 => Err(error(
                "validation",
                "Google rejected this event. Review its pending changes.",
            )),
            403 => {
                let value: serde_json::Value =
                    serde_json::from_reader(response.take(4 * 1024 * 1024)).unwrap_or_default();
                let rate = value["error"]["errors"].as_array().is_some_and(|errors| {
                    errors.iter().any(|e| {
                        matches!(
                            e["reason"].as_str(),
                            Some("rateLimitExceeded" | "userRateLimitExceeded" | "quotaExceeded")
                        )
                    })
                });
                Err(if rate {
                    error(
                        "retryable",
                        "Google request limit reached. A retry is scheduled.",
                    )
                } else {
                    error(
                        "permission",
                        "Google denied this write. Check permissions, then retry pending changes.",
                    )
                })
            }
            _ => decode(response).map(Some),
        }
    }

    fn calendar_page(&self, access: &str, page: Option<&str>) -> Result<CalendarPage> {
        let mut request = client()?
            .get("https://www.googleapis.com/calendar/v3/users/me/calendarList")
            .bearer_auth(access)
            .query(&[("showHidden", "true"), ("maxResults", "250")]);
        if let Some(page) = page {
            request = request.query(&[("pageToken", page)]);
        }
        decode(request.send().map_err(network)?)
    }
}
fn read_tokens(inner: &Inner, subject: &str) -> Result<Option<Tokens>> {
    inner
        .credentials
        .get("google", subject)?
        .map(|v| {
            serde_json::from_str(&v).map_err(|_| {
                error(
                    "authenticationRequired",
                    "Stored Google credentials are invalid. Reconnect the account.",
                )
            })
        })
        .transpose()
}
fn store_tokens(inner: &Inner, subject: &str, tokens: &Tokens) -> Result<()> {
    let value = serde_json::to_string(tokens).map_err(|_| {
        error(
            "credentialUnavailable",
            "Could not save Google credentials.",
        )
    })?;
    inner.credentials.set("google", subject, &value)
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn authorization(config: &ClientConfig, redirect: &str, state: &str, verifier: &str) -> Url {
    let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("fixed URL");
    url.query_pairs_mut().extend_pairs([
        ("client_id", config.id.as_str()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("state", state),
        ("code_challenge_method", "S256"),
        (
            "code_challenge",
            &URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())),
        ),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ]);
    url
}
fn open_browser(url: &Url) -> Result<()> {
    crate::browser::open(url)
}
// Only a complete, matching callback consumes the listener. Malformed/wrong-state probes do not.
fn callback(request: &str, state: &str, port: u16) -> Result<Option<String>> {
    let mut line = request.lines().next().unwrap_or("").split_whitespace();
    if line.next() != Some("GET") {
        return Ok(None);
    }
    let Some(target) = line.next() else {
        return Ok(None);
    };
    if !target.starts_with("/oauth2callback?") {
        return Ok(None);
    }
    let Ok(url) = Url::parse(&format!("http://127.0.0.1:{port}{target}")) else {
        return Ok(None);
    };
    let pairs: Vec<_> = url.query_pairs().collect();
    if pairs.iter().filter(|(k, _)| k == "state").count() != 1
        || !pairs.iter().any(|(k, v)| k == "state" && v == state)
    {
        return Ok(None);
    }
    if pairs.iter().any(|(k, _)| k == "error") {
        return Err(error(
            "authorizationDenied",
            "Google authorization was denied. You can try again.",
        ));
    }
    if pairs.iter().filter(|(k, _)| k == "code").count() != 1 {
        return Ok(None);
    }
    Ok(pairs
        .iter()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .filter(|v| !v.is_empty()))
}
fn waiting(inner: &Inner, id: &str) -> bool {
    let mut sessions = inner.sessions.lock().unwrap();
    let Some(s) = sessions.get_mut(id) else {
        return false;
    };
    if s.public.progress == ConnectionProgress::WaitingForBrowser && s.started.elapsed() >= TIMEOUT
    {
        s.public.progress = ConnectionProgress::TimedOut;
        s.public.failure = Some(error("timeout", "Google connection timed out. Try again."));
    }
    s.public.progress == ConnectionProgress::WaitingForBrowser
}
fn run_connection(
    inner: &Inner,
    id: &str,
    listener: TcpListener,
    state: String,
    verifier: String,
) -> Result<ConnectedIdentity> {
    let config = inner
        .config
        .as_ref()
        .ok_or_else(|| error("configuration", "Google client configuration is missing."))?;
    let port = listener
        .local_addr()
        .map_err(|_| error("callback", "Could not initialize the callback."))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/oauth2callback");
    inner
        .transport
        .browser(&authorization(config, &redirect, &state, &verifier))?;
    let code = loop {
        if !waiting(inner, id) {
            return Err(error("cancelled", "Google connection stopped."));
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                stream
                    .set_read_timeout(Some(Duration::from_millis(250)))
                    .ok();
                stream
                    .set_write_timeout(Some(Duration::from_millis(250)))
                    .ok();
                let mut request = Vec::new();
                let mut chunk = [0; 1024];
                while request.len() < 8192 && !request.windows(4).any(|v| v == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => request.extend_from_slice(&chunk[..n]),
                    }
                }
                if !request.windows(4).any(|v| v == b"\r\n\r\n") {
                    continue;
                }
                let result = callback(&String::from_utf8_lossy(&request), &state, port);
                let message = if matches!(result, Ok(Some(_)) | Err(_)) {
                    "Authorization received. Return to aika."
                } else {
                    "Invalid callback. Return to your authorization browser tab."
                };
                let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}", message.len(), message);
                let _ = stream.write_all(reply.as_bytes());
                if let Some(code) = result? {
                    break code;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40))
            }
            Err(_) => return Err(error("callback", "Could not receive Google authorization.")),
        }
    };
    drop(listener); // one use, closed before exchanging the code
    let response = inner.transport.token(
        config,
        vec![
            ("grant_type", "authorization_code".into()),
            ("code", code),
            ("redirect_uri", redirect),
            ("code_verifier", verifier),
        ],
    )?;
    // Authenticated Google UserInfo over verified TLS, rather than trusting decoded JWT claims.
    if response.scope.is_none() {
        return Err(error(
            "permission",
            "Google did not confirm the requested permissions. Reconnect the account.",
        ));
    }
    let identity = inner.transport.identity(&response.access_token)?;
    if identity.sub.is_empty()
        || identity.sub.len() > 256
        || identity.email.is_empty()
        || !identity.email_verified
    {
        return Err(error(
            "identity",
            "Google did not provide a verified account identity.",
        ));
    }
    if !waiting(inner, id) {
        return Err(error("cancelled", "Google connection stopped."));
    }
    let _credentials = inner.credential_work.lock().unwrap();
    let _identity_lock = inner.lifecycle.as_ref().map(|l| l.lock()).transpose()?;
    let epoch = inner.sessions.lock().unwrap().get(id).and_then(|s| s.epoch);
    if let Some(l) = &inner.lifecycle {
        l.check(&identity.sub, epoch)?;
    }
    let tokens = response.merge(read_tokens(inner, &identity.sub)?)?;
    let mut sessions = inner.sessions.lock().unwrap();
    let session = sessions
        .get_mut(id)
        .ok_or_else(|| error("cancelled", "Google connection stopped."))?;
    if session.public.progress != ConnectionProgress::WaitingForBrowser
        || session.started.elapsed() >= TIMEOUT
    {
        return Err(error("cancelled", "Google connection stopped."));
    }
    store_tokens(inner, &identity.sub, &tokens)?;
    if let Some(l) = &inner.lifecycle {
        l.db()?.execute(
            "UPDATE identity_lifecycles SET state='active' WHERE identity_hash=?1",
            [crate::lifecycle::identity("google", &identity.sub)],
        )?;
    }
    let identity = ConnectedIdentity {
        subject: identity.sub,
        display_name: identity.email,
    };
    session.identity = Some(identity.clone());
    session.public.progress = ConnectionProgress::Connected;
    Ok(identity)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarPage {
    #[serde(default)]
    items: Vec<CalendarEntry>,
    next_page_token: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CalendarEntry {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    description: String,
    background_color: Option<String>,
    #[serde(default)]
    access_role: String,
    #[serde(default)]
    deleted: bool,
}
impl CalendarEntry {
    fn normalized(self) -> Result<DiscoveredCalendar> {
        if self.id.is_empty() {
            return Err(error(
                "google",
                "Google returned a calendar without an identity.",
            ));
        }
        let eligible = !self.deleted
            && matches!(
                self.access_role.as_str(),
                "reader" | "writer" | "writerWithoutPrivateAccess" | "owner"
            );
        let disabled_reason = if self.deleted {
            Some("This calendar is no longer available.".into())
        } else if self.access_role == "freeBusyReader" {
            Some("Only free/busy access is available; event details cannot be imported.".into())
        } else if !eligible {
            Some("Event read access is unavailable.".into())
        } else {
            None
        };
        let color = self
            .background_color
            .filter(|c| {
                c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
            })
            .unwrap_or("#718675".into());
        Ok(DiscoveredCalendar {
            name: if self.summary.is_empty() {
                self.id.clone()
            } else {
                self.summary
            },
            remote_id: self.id,
            color,
            description: self.description,
            access_role: self.access_role,
            eligible,
            disabled_reason,
            selected: eligible,
        })
    }
}
fn pages(
    mut fetch: impl FnMut(Option<&str>) -> Result<CalendarPage>,
) -> Result<Vec<DiscoveredCalendar>> {
    let mut token: Option<String> = None;
    let mut seen = BTreeSet::new();
    let mut calendars = BTreeMap::new();
    loop {
        let page = fetch(token.as_deref())?;
        for entry in page.items {
            let entry = entry.normalized()?;
            if calendars.insert(entry.remote_id.clone(), entry).is_some() {
                return Err(error(
                    "google",
                    "Google returned duplicate calendar identities. Retry discovery.",
                ));
            }
        }
        match page.next_page_token.filter(|t| !t.is_empty()) {
            None => return Ok(calendars.into_values().collect()),
            Some(next) if seen.insert(next.clone()) && seen.len() <= 1000 => token = Some(next),
            Some(_) => {
                return Err(error(
                    "google",
                    "Google returned invalid discovery pagination. Retry discovery.",
                ))
            }
        }
    }
}
impl Provider for GoogleProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            id: "google".into(),
            name: "Google Calendar".into(),
            connection_available: self.0.config.is_some(),
            unavailable_reason: self.0.config.is_none().then(|| {
                "Google connection requires AIKA_GOOGLE_CLIENT_ID desktop client configuration at build time or when launching aika."
                    .into()
            }),
            capabilities: self.capabilities(),
        }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::google_recurring()
    }
    fn start_connection(&self) -> Result<ConnectionSession> {
        if self.0.config.is_none() {
            return Err(error(
                "configuration",
                "Google desktop client configuration is missing.",
            ));
        }
        // Probe the store before launching; an unavailable store cannot fall back to memory/SQLite.
        self.0.credentials.get("google", "connection-store-probe")?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| error("callback", "Could not create a loopback callback listener."))?;
        listener
            .set_nonblocking(true)
            .map_err(|_| error("callback", "Could not initialize the callback listener."))?;
        let public = ConnectionSession {
            id: uuid::Uuid::new_v4().to_string(),
            progress: ConnectionProgress::WaitingForBrowser,
            account_id: None,
            failure: None,
        };
        let mut sessions = self.0.sessions.lock().unwrap();
        sessions.retain(|_, s| s.started.elapsed() < Duration::from_secs(600));
        if sessions.len() >= 32 {
            return Err(error(
                "connectionBusy",
                "Too many recent connection attempts. Wait a few minutes before retrying.",
            ));
        }
        if sessions
            .values()
            .any(|s| s.public.progress == ConnectionProgress::WaitingForBrowser)
        {
            return Err(error(
                "connectionBusy",
                "Finish or cancel the current Google connection first.",
            ));
        }
        sessions.insert(
            public.id.clone(),
            Session {
                epoch: self.0.lifecycle.as_ref().map(|l| l.epoch()).transpose()?,
                public: public.clone(),
                identity: None,
                started: Instant::now(),
            },
        );
        drop(sessions);
        let inner = self.0.clone();
        let id = public.id.clone();
        std::thread::spawn(move || {
            let result = run_connection(&inner, &id, listener, random(), random());
            waiting(&inner, &id);
            let mut sessions = inner.sessions.lock().unwrap();
            if let Some(session) = sessions.get_mut(&id) {
                if session.public.progress == ConnectionProgress::WaitingForBrowser {
                    match result {
                        Ok(identity) => {
                            session.identity = Some(identity);
                            session.public.progress = ConnectionProgress::Connected;
                        }
                        Err(e) => {
                            session.public.failure = Some(e);
                            session.public.progress = ConnectionProgress::Failed;
                        }
                    }
                }
            }
        });
        Ok(public)
    }
    fn connection(&self, id: &str) -> Result<(ConnectionSession, Option<ConnectedIdentity>)> {
        waiting(&self.0, id);
        let sessions = self.0.sessions.lock().unwrap();
        let s = sessions
            .get(id)
            .ok_or_else(|| error("notFound", "Connection session expired. Start again."))?;
        if s.started.elapsed() >= Duration::from_secs(600) {
            return Err(error(
                "notFound",
                "Connection session expired. Start again.",
            ));
        }
        Ok((s.public.clone(), s.identity.clone()))
    }
    fn cancel_connection(&self, id: &str) -> Result<()> {
        let mut sessions = self.0.sessions.lock().unwrap();
        let s = sessions
            .get_mut(id)
            .ok_or_else(|| error("notFound", "Connection session expired."))?;
        if s.public.progress == ConnectionProgress::WaitingForBrowser {
            s.public.progress = ConnectionProgress::Cancelled;
        }
        Ok(())
    }
    fn discover(&self, subject: &str) -> Result<Vec<DiscoveredCalendar>> {
        let access = self.access(subject)?;
        pages(|page| self.0.transport.calendar_page(&access, page))
    }
    fn discover_for_sync(
        &self,
        subject: &str,
        checkpoint: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<DiscoveredCalendar>> {
        checkpoint()?;
        let access = self.access(subject)?;
        pages(|page| {
            checkpoint()?;
            self.0.transport.calendar_page(&access, page)
        })
    }
    fn event_capabilities(&self, item: &crate::sync::IncomingItem) -> Capabilities {
        if outgoing::editable(item) {
            Capabilities::google_recurring()
        } else {
            Capabilities::read_only()
        }
    }
    fn resolve_outgoing_instance(
        &self,
        subject: &str,
        calendar: &str,
        series: &str,
        original: &str,
    ) -> Result<crate::sync::IncomingItem> {
        self.resolve_instance(subject, calendar, series, original)
    }
    fn outgoing(
        &self,
        subject: &str,
        operation: &crate::outgoing::OutgoingOperation,
    ) -> Result<crate::outgoing::OutgoingResult> {
        outgoing::deliver(self.0.transport.as_ref(), &self.access(subject)?, operation)
    }
    fn incoming_page(
        &self,
        subject: &str,
        calendar: &str,
        cursor: Option<&str>,
        page: Option<&str>,
        projection: Option<&EventRange>,
    ) -> Result<crate::sync::IncomingPage> {
        let access = self.access(subject)?;
        events::normalized(
            self.0
                .transport
                .event_page(&access, calendar, cursor, page, projection)?,
        )
    }
    fn remove_credentials(&self, subject: &str) -> Result<()> {
        self.0.credentials.delete("google", subject)
    }
    fn connection_epoch(&self, id: &str) -> Result<Option<i64>> {
        Ok(self
            .0
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .and_then(|s| s.epoch))
    }
    fn credential_epoch(&self) -> Result<Option<i64>> {
        self.0.lifecycle.as_ref().map(|l| l.epoch()).transpose()
    }
    fn disconnect_checked(&self, subject: &str, epoch: Option<i64>) -> Result<()> {
        let _guard = self.0.credential_work.lock().unwrap();
        let _identity_lock = self.0.lifecycle.as_ref().map(|l| l.lock()).transpose()?;
        if let Some(l) = &self.0.lifecycle {
            l.check(subject, epoch)?;
        }
        self.0.credentials.delete("google", subject)
    }
    fn disconnect(&self, subject: &str) -> Result<()> {
        let _guard = self.0.credential_work.lock().unwrap();
        let _identity_lock = self.0.lifecycle.as_ref().map(|l| l.lock()).transpose()?;
        self.0.credentials.delete("google", subject)
    }
}

fn event_query(
    cursor: Option<&str>,
    page: Option<&str>,
    projection: Option<&EventRange>,
) -> Vec<(String, String)> {
    let mut query = vec![
        ("showDeleted".into(), "true".into()),
        ("showHiddenInvitations".into(), "true".into()),
        ("maxResults".into(), "2500".into()),
        (
            "singleEvents".into(),
            if projection.is_some() {
                "true"
            } else {
                "false"
            }
            .into(),
        ),
    ];
    if let Some(range) = projection {
        query.push(("timeMin".into(), range.start.clone()));
        query.push(("timeMax".into(), range.end.clone()));
    } else if let Some(cursor) = cursor {
        query.push(("syncToken".into(), cursor.into()));
    }
    if let Some(page) = page {
        query.push(("pageToken".into(), page.into()));
    }
    query
}

fn event_url(calendar: &str) -> Result<Url> {
    if calendar.is_empty() || matches!(calendar, "." | "..") {
        return Err(error(
            "providerIdentity",
            "Missing remote calendar identity.",
        ));
    }
    let mut url =
        Url::parse("https://www.googleapis.com/calendar/v3/calendars/").expect("fixed URL");
    url.path_segments_mut()
        .expect("fixed URL")
        .pop_if_empty()
        .push(calendar)
        .push("events");
    Ok(url)
}
fn resolve_instance(
    transport: &dyn GoogleTransport,
    access: &str,
    calendar: &str,
    series: &str,
    original: &str,
) -> Result<crate::sync::IncomingItem> {
    let canonical = if original.len() == 10 {
        let date = chrono::NaiveDate::parse_from_str(original, "%Y-%m-%d")
            .map_err(|_| error("validation", "Invalid original start."))?;
        if date.to_string() != original {
            return Err(error("validation", "Invalid original start."));
        }
        original.to_owned()
    } else {
        aika_domain::iso(
            chrono::DateTime::parse_from_rfc3339(original)
                .map_err(|_| error("validation", "Invalid original start."))?
                .with_timezone(&chrono::Utc),
        )
    };
    let original = canonical.as_str();
    let mut page = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut found = None;
    loop {
        let response =
            transport.instance_page(access, calendar, series, original, page.as_deref())?;
        let next_page = response.next_page_token.clone();
        for item in events::normalized(response)?.items {
            if item.master.as_deref() == Some(series)
                && item.original_start.as_deref() == Some(original)
            {
                if found.is_some() {
                    return Err(error(
                        "providerIdentity",
                        "Google returned ambiguous instance identities.",
                    ));
                }
                found = Some(item);
            }
        }
        match next_page {
            Some(next) if seen.len() < 1000 && seen.insert(next.clone()) => page = Some(next),
            Some(_) => return Err(error("google", "Google repeated an instance page.")),
            None => break,
        }
    }
    found.ok_or_else(|| {
        error(
            "notFound",
            "Google did not return this occurrence. Refresh the series before editing.",
        )
    })
}

impl GoogleProvider {
    /// Backend-only preflight. Callers must persist the returned provider identity
    /// before attempting a write; IDs are never constructed from occurrence strings.
    pub fn resolve_instance(
        &self,
        subject: &str,
        calendar: &str,
        series: &str,
        original: &str,
    ) -> Result<crate::sync::IncomingItem> {
        resolve_instance(
            self.0.transport.as_ref(),
            &self.access(subject)?,
            calendar,
            series,
            original,
        )
    }
    fn access(&self, subject: &str) -> Result<String> {
        let _guard = self.0.credential_work.lock().unwrap();
        let config = self.0.config.as_ref().ok_or_else(|| {
            error(
                "configuration",
                "Google desktop client configuration is missing.",
            )
        })?;
        let identity_lock = self.0.lifecycle.as_ref().map(|l| l.lock()).transpose()?;
        if let Some(l) = &self.0.lifecycle {
            l.check(subject, None)?;
        }
        let epoch = self.0.lifecycle.as_ref().map(|l| l.epoch()).transpose()?;
        let mut tokens = read_tokens(&self.0, subject)?
            .ok_or_else(|| error("authenticationRequired", "Reconnect this Google account."))?;
        drop(identity_lock);
        if tokens.expires_at <= chrono::Utc::now().timestamp() + 60 {
            let response = self.0.transport.token(
                config,
                vec![
                    ("grant_type", "refresh_token".into()),
                    (
                        "refresh_token",
                        tokens.refresh_token.clone().ok_or_else(|| {
                            error("authenticationRequired", "Reconnect this Google account.")
                        })?,
                    ),
                ],
            )?;
            tokens = response.merge(Some(tokens))?;
            let _identity_lock = self.0.lifecycle.as_ref().map(|l| l.lock()).transpose()?;
            if let Some(l) = &self.0.lifecycle {
                l.check(subject, epoch)?;
            }
            store_tokens(&self.0, subject, &tokens)?;
        }
        Ok(tokens.access_token)
    }
}

pub(crate) fn cached_instance_editable(shadow: &str) -> bool {
    serde_json::from_str(shadow)
        .ok()
        .and_then(|v| {
            events::normalized(events::EventPage {
                items: vec![v],
                next_page_token: None,
                next_sync_token: None,
                time_zone: "UTC".into(),
            })
            .ok()
        })
        .and_then(|p| p.items.into_iter().next())
        .as_ref()
        .is_some_and(outgoing::editable_fields)
}

/// Re-evaluate persisted pre-phase-02 capability JSON without changing provider data.
pub(crate) fn cached_capabilities(shadow: &str) -> Capabilities {
    let item = serde_json::from_str(shadow)
        .ok()
        .and_then(|v| {
            events::normalized(events::EventPage {
                items: vec![v],
                next_page_token: None,
                next_sync_token: None,
                time_zone: "UTC".into(),
            })
            .ok()
        })
        .and_then(|p| p.items.into_iter().next());
    if item.as_ref().is_some_and(outgoing::editable) {
        Capabilities::google_recurring()
    } else {
        Capabilities::read_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::CredentialStore;
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    #[test]
    fn client_configuration_resolves_complete_pairs_without_mixing_sources() {
        type Pair<'a> = (Option<&'a str>, Option<&'a str>);
        let cases: [(Pair<'_>, Pair<'_>, Option<Pair<'_>>); 9] = [
            ((None, None), (None, None), None),
            (
                (None, None),
                (Some("embedded"), Some("embedded-secret")),
                Some((Some("embedded"), Some("embedded-secret"))),
            ),
            (
                (Some("runtime"), Some("runtime-secret")),
                (None, None),
                Some((Some("runtime"), Some("runtime-secret"))),
            ),
            (
                (Some("runtime"), Some("runtime-secret")),
                (Some("embedded"), Some("embedded-secret")),
                Some((Some("runtime"), Some("runtime-secret"))),
            ),
            (
                (Some("runtime"), None),
                (Some("embedded"), Some("embedded-secret")),
                Some((Some("runtime"), None)),
            ),
            (
                (None, Some("orphan")),
                (Some("embedded"), Some("embedded-secret")),
                Some((Some("embedded"), Some("embedded-secret"))),
            ),
            (
                (Some("  "), Some("orphan")),
                (Some(" embedded "), Some("  ")),
                Some((Some("embedded"), None)),
            ),
            ((None, None), (Some("  "), Some("orphan")), None),
            (
                (Some(" runtime "), Some("  ")),
                (Some("embedded"), Some("embedded-secret")),
                Some((Some("runtime"), None)),
            ),
        ];
        for ((runtime_id, runtime_secret), (embedded_id, embedded_secret), expected) in cases {
            let actual =
                ClientConfig::resolve(runtime_id, runtime_secret, embedded_id, embedded_secret);
            assert_eq!(
                actual
                    .as_ref()
                    .map(|c| (Some(c.id.as_str()), c.secret.as_deref())),
                expected
            );
        }
    }
    #[test]
    #[ignore = "run via pnpm test:google-config with fictional build settings"]
    fn embedded_google_configuration_smoke() {
        let expected_id =
            std::env::var("AIKA_TEST_EXPECT_GOOGLE_ID").expect("smoke runner required");
        let expected_secret =
            std::env::var("AIKA_TEST_EXPECT_GOOGLE_SECRET").expect("smoke runner required");
        let provider = GoogleProvider::default();
        assert_eq!(
            provider.descriptor().connection_available,
            !expected_id.is_empty()
        );
        match &provider.0.config {
            Some(config) => {
                assert_eq!(config.id, expected_id);
                assert_eq!(config.secret.as_deref().unwrap_or(""), expected_secret);
            }
            None => assert!(expected_id.is_empty()),
        }
    }
    #[derive(Default)]
    struct Store {
        values: Mutex<BTreeMap<String, String>>,
        locked: AtomicBool,
        fail_write: AtomicBool,
    }
    impl CredentialStore for Arc<Store> {
        fn get(&self, key: &str) -> Result<Option<String>> {
            if self.locked.load(Ordering::SeqCst) {
                return Err(error(
                    "credentialUnavailable",
                    "Unlock the credential store.",
                ));
            }
            Ok(self.values.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<()> {
            if self.fail_write.load(Ordering::SeqCst) {
                return Err(error(
                    "credentialUnavailable",
                    "Unlock the credential store.",
                ));
            }
            self.values.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<()> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
    #[derive(Default)]
    struct Transport {
        url: Mutex<Option<Url>>,
        exchanges: AtomicUsize,
        refreshes: AtomicUsize,
        fail_refresh: AtomicBool,
        refresh_hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
        fail_page: AtomicBool,
        fail_browser: AtomicBool,
        instance_pages: Mutex<std::collections::VecDeque<events::EventPage>>,
        instance_queries: Mutex<Vec<(String, String, Option<String>)>>,
    }
    impl GoogleTransport for Transport {
        fn browser(&self, url: &Url) -> Result<()> {
            *self.url.lock().unwrap() = Some(url.clone());
            if self.fail_browser.load(Ordering::SeqCst) {
                return Err(error("browser", "Could not open the system browser."));
            }
            Ok(())
        }
        fn token(&self, _: &ClientConfig, form: Vec<(&str, String)>) -> Result<TokenResponse> {
            if form
                .iter()
                .any(|(k, v)| *k == "grant_type" && v == "refresh_token")
            {
                self.refreshes.fetch_add(1, Ordering::SeqCst);
                if let Some(hook) = self.refresh_hook.lock().unwrap().take() {
                    hook();
                }
                if self.fail_refresh.load(Ordering::SeqCst) {
                    return Err(error("authenticationRequired", "Reconnect the account."));
                }
                return Ok(TokenResponse {
                    access_token: "renewed-disposable-access".into(),
                    refresh_token: None,
                    expires_in: 3600,
                    token_type: "Bearer".into(),
                    scope: None,
                });
            }
            self.exchanges.fetch_add(1, Ordering::SeqCst);
            let url = self.url.lock().unwrap().clone().unwrap();
            let query: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
            let verifier = &form.iter().find(|(k, _)| *k == "code_verifier").unwrap().1;
            assert_eq!(
                query["code_challenge"],
                URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
            );
            assert_eq!(
                query["redirect_uri"],
                form.iter().find(|(k, _)| *k == "redirect_uri").unwrap().1
            );
            assert_eq!(query["code_challenge_method"], "S256");
            assert_eq!(query["access_type"], "offline");
            assert_eq!(query["scope"], SCOPES);
            Ok(TokenResponse {
                access_token: "disposable-access".into(),
                refresh_token: Some("disposable-refresh".into()),
                expires_in: 3600,
                token_type: "Bearer".into(),
                scope: Some(SCOPES.into()),
            })
        }
        fn identity(&self, _: &str) -> Result<Identity> {
            Ok(Identity {
                sub: "fictional-subject".into(),
                email: "test@example.invalid".into(),
                email_verified: true,
            })
        }
        fn instance_page(
            &self,
            _: &str,
            _: &str,
            series: &str,
            original: &str,
            page: Option<&str>,
        ) -> Result<events::EventPage> {
            self.instance_queries.lock().unwrap().push((
                series.into(),
                original.into(),
                page.map(str::to_owned),
            ));
            self.instance_pages
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| error("network", "No fixture page."))
        }
        fn calendar_page(&self, _: &str, page: Option<&str>) -> Result<CalendarPage> {
            if self.fail_page.load(Ordering::SeqCst) {
                return Err(error("network", "Try discovery again."));
            }
            Ok(CalendarPage {
                items: vec![entry(if page.is_none() { "one" } else { "two" }, "reader")],
                next_page_token: page.is_none().then(|| "next".into()),
            })
        }
    }
    fn entry(id: &str, role: &str) -> CalendarEntry {
        CalendarEntry {
            id: id.into(),
            summary: id.into(),
            description: "".into(),
            background_color: Some("#123456".into()),
            access_role: role.into(),
            deleted: false,
        }
    }
    #[test]
    fn calendar_reconciliation_checkpoints_each_page_and_stops_superseded_work() {
        let (p, _, _) = fixture();
        store_tokens(
            &p.0,
            "fictional-subject",
            &Tokens {
                access_token: "disposable".into(),
                refresh_token: Some("disposable".into()),
                expires_at: chrono::Utc::now().timestamp() + 3600,
            },
        )
        .unwrap();
        let mut checkpoints = 0;
        assert_eq!(
            p.discover_for_sync("fictional-subject", &mut || {
                checkpoints += 1;
                Ok(())
            })
            .unwrap()
            .len(),
            2
        );
        assert_eq!(checkpoints, 3);
        let mut checkpoints = 0;
        assert_eq!(
            p.discover_for_sync("fictional-subject", &mut || {
                checkpoints += 1;
                if checkpoints == 3 {
                    Err(error("syncSuperseded", "Selection changed."))
                } else {
                    Ok(())
                }
            })
            .unwrap_err()
            .code,
            "syncSuperseded"
        );
    }
    #[test]
    fn event_feed_parameters_stay_stable_and_projection_has_no_cursor() {
        let initial = event_query(None, None, None)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let mut incremental = event_query(Some("cursor"), Some("page"), None)
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(incremental.remove("syncToken").as_deref(), Some("cursor"));
        assert_eq!(incremental.remove("pageToken").as_deref(), Some("page"));
        assert_eq!(initial, incremental);
        assert_eq!(initial["singleEvents"], "false");
        assert_eq!(initial["showDeleted"], "true");
        assert!(!initial.contains_key("timeMin"));
        let range = EventRange {
            start: "2026-01-01T00:00:00Z".into(),
            end: "2029-01-01T00:00:00Z".into(),
            start_date: "2026-01-01".into(),
            end_date: "2029-01-01".into(),
        };
        let projection = event_query(Some("cursor"), None, Some(&range))
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert!(!projection.contains_key("syncToken"));
        assert_eq!(projection["singleEvents"], "true");
        assert_eq!(projection["timeMin"], range.start);
        let url = event_url("account/#?@example.invalid").unwrap();
        assert_eq!(url.host_str(), Some("www.googleapis.com"));
        assert!(url.query().is_none());
        assert!(url.fragment().is_none());
        assert_eq!(
            url.path(),
            "/calendar/v3/calendars/account%2F%23%3F@example.invalid/events"
        );
        assert!(event_url("..").is_err());
    }
    fn fixture() -> (GoogleProvider, Arc<Transport>, Arc<Store>) {
        let transport = Arc::<Transport>::default();
        let store = Arc::<Store>::default();
        let provider = GoogleProvider(Arc::new(Inner {
            config: Some(ClientConfig {
                id: "fictional-client".into(),
                secret: None,
            }),
            sessions: Mutex::new(BTreeMap::new()),
            credentials: CredentialService::testing(Box::new(store.clone())),
            credential_work: Mutex::new(()),
            lifecycle: None,
            transport: transport.clone(),
        }));
        (provider, transport, store)
    }
    fn browser_url(t: &Transport) -> Url {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(url) = t.url.lock().unwrap().clone() {
                return url;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn send(t: &Transport, query: &str) -> u16 {
        let url = browser_url(t);
        let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        let redirect = Url::parse(&pairs["redirect_uri"]).unwrap();
        let port = redirect.port().unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let request = format!(
            "GET {}?state={}&{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            redirect.path(),
            pairs["state"],
            query,
            port
        );
        stream.write_all(request.as_bytes()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(!response.contains("disposable"));
        port
    }
    fn finished(p: &GoogleProvider, id: &str) -> (ConnectionSession, Option<ConnectedIdentity>) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let value = p.connection(id).unwrap();
            if value.0.progress != ConnectionProgress::WaitingForBrowser {
                return value;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn closed(port: u16) {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if TcpListener::bind(("127.0.0.1", port)).is_ok() {
                return;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn lifecycle_fixture() -> (
        GoogleProvider,
        Arc<Transport>,
        Arc<Store>,
        crate::lifecycle::Lifecycle,
    ) {
        let (mut p, t, store) = fixture();
        let path = std::env::temp_dir().join(format!(
            "aika-credential-fence-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        crate::repository::Repository::open(&path).unwrap();
        let lifecycle = crate::lifecycle::Lifecycle::new(&path);
        Arc::get_mut(&mut p.0).unwrap().lifecycle = Some(lifecycle.clone());
        (p, t, store, lifecycle)
    }
    fn removed(l: &crate::lifecycle::Lifecycle) {
        let _lock = l.lock().unwrap();
        let db = l.db().unwrap();
        db.execute_batch("BEGIN IMMEDIATE; UPDATE lifecycle_clock SET epoch=epoch+1;")
            .unwrap();
        db.execute("INSERT INTO identity_lifecycles VALUES(?1,(SELECT epoch FROM lifecycle_clock),'removed') ON CONFLICT(identity_hash) DO UPDATE SET epoch=excluded.epoch,state=excluded.state",[crate::lifecycle::identity("google","fictional-subject")]).unwrap();
        db.execute_batch("COMMIT").unwrap();
    }
    #[test]
    fn refresh_after_removal_and_fresh_signin_cannot_restore_old_credentials() {
        let (p, t, store, l) = lifecycle_fixture();
        store_tokens(
            &p.0,
            "fictional-subject",
            &Tokens {
                access_token: "old".into(),
                refresh_token: Some("old-refresh".into()),
                expires_at: 0,
            },
        )
        .unwrap();
        let disconnect_epoch = p.credential_epoch().unwrap();
        let lifecycle = l.clone();
        let values = store.clone();
        *t.refresh_hook.lock().unwrap() = Some(Box::new(move || {
            // A second process can acquire the identity lock while HTTP is in progress.
            removed(&lifecycle);
            let _lock = lifecycle.lock().unwrap();
            values.values.lock().unwrap().clear();
            lifecycle
                .db()
                .unwrap()
                .execute("UPDATE identity_lifecycles SET state='active'", [])
                .unwrap();
        }));
        assert_eq!(
            p.access("fictional-subject").unwrap_err().code,
            "accountRemoved"
        );
        assert!(store.values.lock().unwrap().is_empty());
        store_tokens(
            &p.0,
            "fictional-subject",
            &Tokens {
                access_token: "fresh".into(),
                refresh_token: Some("fresh-refresh".into()),
                expires_at: i64::MAX,
            },
        )
        .unwrap();
        assert_eq!(
            p.disconnect_checked("fictional-subject", disconnect_epoch)
                .unwrap_err()
                .code,
            "accountRemoved"
        );
        assert_eq!(p.access("fictional-subject").unwrap(), "fresh");
        std::fs::remove_file(l.0).unwrap();
    }
    #[test]
    fn late_oauth_completion_is_fenced_and_explicit_new_signin_renews_lifecycle() {
        let (p, t, store, l) = lifecycle_fixture();
        let session = p.start_connection().unwrap();
        browser_url(&t);
        removed(&l);
        let port = send(&t, "code=disposable-code");
        let (done, _) = finished(&p, &session.id);
        closed(port);
        assert_eq!(done.progress, ConnectionProgress::Failed);
        assert_eq!(done.failure.unwrap().code, "accountRemoved");
        assert!(store.values.lock().unwrap().is_empty());
        *t.url.lock().unwrap() = None;
        let fresh = p.start_connection().unwrap();
        let port = send(&t, "code=disposable-code");
        assert_eq!(
            finished(&p, &fresh.id).0.progress,
            ConnectionProgress::Connected
        );
        closed(port);
        assert!(l
            .check(
                "fictional-subject",
                p.connection_epoch(&session.id).unwrap()
            )
            .is_err());
        assert!(l
            .check("fictional-subject", p.connection_epoch(&fresh.id).unwrap())
            .is_ok());
        assert!(p.access("fictional-subject").is_ok());
        std::fs::remove_file(l.0).unwrap();
    }
    #[test]
    fn native_loopback_pkce_identity_secure_store_discovery_and_refresh() {
        let (p, t, store) = fixture();
        let s = p.start_connection().unwrap();
        assert!(p.start_connection().is_err());
        let port = send(&t, "code=disposable-code");
        let (session, identity) = finished(&p, &s.id);
        closed(port);
        assert_eq!(session.progress, ConnectionProgress::Connected);
        assert_eq!(identity.unwrap().subject, "fictional-subject");
        assert_eq!(p.discover("fictional-subject").unwrap().len(), 2);
        let mut tokens = read_tokens(&p.0, "fictional-subject").unwrap().unwrap();
        tokens.expires_at = 0;
        store_tokens(&p.0, "fictional-subject", &tokens).unwrap();
        assert_eq!(p.discover("fictional-subject").unwrap().len(), 2);
        assert_eq!(
            read_tokens(&p.0, "fictional-subject")
                .unwrap()
                .unwrap()
                .refresh_token
                .as_deref(),
            Some("disposable-refresh")
        );
        assert_eq!(t.refreshes.load(Ordering::SeqCst), 1);
        t.fail_page.store(true, Ordering::SeqCst);
        assert_eq!(p.discover("fictional-subject").unwrap_err().code, "network");
        t.fail_page.store(false, Ordering::SeqCst);
        let mut tokens = read_tokens(&p.0, "fictional-subject").unwrap().unwrap();
        tokens.expires_at = 0;
        store_tokens(&p.0, "fictional-subject", &tokens).unwrap();
        t.fail_refresh.store(true, Ordering::SeqCst);
        assert_eq!(
            p.discover("fictional-subject").unwrap_err().code,
            "authenticationRequired"
        );
        assert!(read_tokens(&p.0, "fictional-subject").unwrap().is_some());
        p.disconnect("fictional-subject").unwrap();
        assert!(store.values.lock().unwrap().is_empty());
        let serialized = serde_json::to_string(&session).unwrap();
        for secret in [
            "disposable-access",
            "disposable-refresh",
            "disposable-code",
            "code_verifier",
        ] {
            assert!(!serialized.contains(secret));
        }
    }
    #[test]
    fn denial_cancellation_timeout_and_store_failures_close_listener_without_identity() {
        for outcome in ["denied", "cancelled", "timeout", "store", "browser"] {
            let (p, t, store) = fixture();
            t.fail_browser.store(outcome == "browser", Ordering::SeqCst);
            let s = p.start_connection().unwrap();
            let url = browser_url(&t);
            let pairs: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
            let port = Url::parse(&pairs["redirect_uri"]).unwrap().port().unwrap();
            match outcome {
                "denied" => {
                    send(&t, "error=access_denied");
                }
                "cancelled" => p.cancel_connection(&s.id).unwrap(),
                "timeout" => {
                    p.0.sessions.lock().unwrap().get_mut(&s.id).unwrap().started =
                        Instant::now() - TIMEOUT;
                }
                "store" => {
                    store.fail_write.store(true, Ordering::SeqCst);
                    send(&t, "code=disposable-code");
                }
                "browser" => {}
                _ => unreachable!(),
            }
            let (session, identity) = finished(&p, &s.id);
            closed(port);
            assert!(identity.is_none());
            if outcome == "browser" {
                assert_eq!(session.failure.as_ref().unwrap().code, "browser");
                assert_eq!(t.exchanges.load(Ordering::SeqCst), 0);
            }
            assert!(store.values.lock().unwrap().is_empty());
            assert_eq!(
                session.progress,
                match outcome {
                    "cancelled" => ConnectionProgress::Cancelled,
                    "timeout" => ConnectionProgress::TimedOut,
                    _ => ConnectionProgress::Failed,
                }
            );
        }
        let (p, t, store) = fixture();
        store.locked.store(true, Ordering::SeqCst);
        assert_eq!(
            p.start_connection().unwrap_err().code,
            "credentialUnavailable"
        );
        assert!(t.url.lock().unwrap().is_none());
        store.locked.store(false, Ordering::SeqCst);
        let retry = p.start_connection().unwrap();
        let port = send(&t, "code=disposable-code");
        assert_eq!(
            finished(&p, &retry.id).0.progress,
            ConnectionProgress::Connected
        );
        closed(port);
        assert!(read_tokens(&p.0, "fictional-subject").unwrap().is_some());
        p.0.sessions
            .lock()
            .unwrap()
            .get_mut(&retry.id)
            .unwrap()
            .started = Instant::now() - Duration::from_secs(601);
        assert!(p.connection(&retry.id).is_err());
    }
    #[test]
    fn invalid_state_does_not_consume_callback_and_fresh_sessions_have_fresh_pkce() {
        let (p, t, _) = fixture();
        let s = p.start_connection().unwrap();
        let first = browser_url(&t);
        let query: BTreeMap<_, _> = first.query_pairs().into_owned().collect();
        let redirect = Url::parse(&query["redirect_uri"]).unwrap();
        let mut socket = TcpStream::connect(("127.0.0.1", redirect.port().unwrap())).unwrap();
        socket
            .write_all(
                b"GET /oauth2callback?state=invalid&code=bad HTTP/1.1\r\nHost: localhost\r\n\r\n",
            )
            .unwrap();
        let mut reply = String::new();
        socket.read_to_string(&mut reply).unwrap();
        assert_eq!(
            p.connection(&s.id).unwrap().0.progress,
            ConnectionProgress::WaitingForBrowser
        );
        assert_eq!(t.exchanges.load(Ordering::SeqCst), 0);
        let port = send(&t, "code=disposable-code");
        finished(&p, &s.id);
        closed(port);
        *t.url.lock().unwrap() = None;
        let next = p.start_connection().unwrap();
        let next_url = browser_url(&t);
        let next_query: BTreeMap<_, _> = next_url.query_pairs().into_owned().collect();
        assert_ne!(query["state"], next_query["state"]);
        assert_ne!(query["code_challenge"], next_query["code_challenge"]);
        p.cancel_connection(&next.id).unwrap();
    }
    #[test]
    fn grant_validation_accepts_email_alias_and_rejects_missing_calendar_permission() {
        let response = |scope: String| TokenResponse {
            access_token: "disposable".into(),
            refresh_token: Some("disposable".into()),
            expires_in: 3600,
            token_type: "Bearer".into(),
            scope: Some(scope),
        };
        assert!(response(SCOPES.replace(
            "openid email",
            "openid https://www.googleapis.com/auth/userinfo.email"
        ))
        .merge(None)
        .is_ok());
        assert_eq!(
            response("openid email".into())
                .merge(None)
                .err()
                .unwrap()
                .code,
            "permission"
        );
    }
    #[test]
    fn pagination_hidden_ineligible_entries_repeated_pages_and_partial_failure() {
        let mut calls = 0;
        let result = pages(|token| {
            calls += 1;
            if token.is_none() {
                Ok(CalendarPage {
                    items: vec![entry("hidden", "owner"), entry("busy", "freeBusyReader")],
                    next_page_token: Some("page2".into()),
                })
            } else {
                Ok(CalendarPage {
                    items: vec![entry("no-access", "none")],
                    next_page_token: None,
                })
            }
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(result.len(), 3);
        assert!(
            result
                .iter()
                .find(|c| c.remote_id == "hidden")
                .unwrap()
                .selected
        );
        assert!(result
            .iter()
            .filter(|c| !c.eligible)
            .all(|c| !c.selected && c.disabled_reason.is_some()));
        assert!(pages(|_| Ok(CalendarPage {
            items: vec![],
            next_page_token: Some("same".into())
        }))
        .is_err());
        assert!(pages(|page| if page.is_some() {
            Err(error("network", "Try again."))
        } else {
            Ok(CalendarPage {
                items: vec![entry("one", "reader")],
                next_page_token: Some("next".into()),
            })
        })
        .is_err());
        assert!(callback(
            "GET /oauth2callback?state=s&state=s&code=c HTTP/1.1\r\n",
            "s",
            1
        )
        .unwrap()
        .is_none());
        assert!(callback("GET /wrong?state=s&code=c HTTP/1.1\r\n", "s", 1)
            .unwrap()
            .is_none());
    }
    #[test]
    fn instance_lookup_uses_original_start_and_provider_ids_for_moved_and_cancelled_instances() {
        for cancelled in [false, true] {
            let t = Transport::default();
            let mut item = serde_json::json!({"id":"opaque-provider-instance", "etag":"r1", "recurringEventId":"series", "originalStartTime":{"date":"2026-10-01"}, "summary":"Moved", "start":{"date":"2026-12-01"}, "end":{"date":"2026-12-02"}});
            if cancelled {
                item["status"] = serde_json::json!("cancelled");
                item.as_object_mut().unwrap().remove("start");
                item.as_object_mut().unwrap().remove("end");
            }
            for (items, next) in [(vec![], Some("second".into())), (vec![item], None)] {
                t.instance_pages
                    .lock()
                    .unwrap()
                    .push_back(events::EventPage {
                        items,
                        next_page_token: next,
                        next_sync_token: None,
                        time_zone: "UTC".into(),
                    });
            }
            let resolved =
                resolve_instance(&t, "access", "calendar", "series", "2026-10-01").unwrap();
            assert_eq!(resolved.remote_id, "opaque-provider-instance");
            assert_eq!(resolved.event.is_none(), cancelled);
            assert_eq!(
                t.instance_queries.lock().unwrap()[1],
                ("series".into(), "2026-10-01".into(), Some("second".into()))
            );
        }
    }
    #[test]
    fn instance_lookup_rejects_missing_ambiguous_and_repeated_pages() {
        for mode in ["missing", "ambiguous", "repeat"] {
            let t = Transport::default();
            let item = serde_json::json!({"id":"cancelled", "status":"cancelled", "recurringEventId":"series", "originalStartTime":{"date":"2026-10-01"}});
            let items = if mode == "ambiguous" {
                vec![item.clone(), item]
            } else {
                vec![]
            };
            let page = || events::EventPage {
                items: items.clone(),
                next_page_token: if mode == "repeat" {
                    Some("same".into())
                } else {
                    None
                },
                next_sync_token: None,
                time_zone: "UTC".into(),
            };
            t.instance_pages.lock().unwrap().extend([page(), page()]);
            let failure = resolve_instance(&t, "access", "calendar", "series", "2026-10-01")
                .err()
                .unwrap();
            assert_eq!(
                failure.code,
                match mode {
                    "missing" => "notFound",
                    "ambiguous" => "providerIdentity",
                    _ => "google",
                }
            );
        }
    }
}
