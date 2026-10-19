//! Desktop-only scheduler. The CLI only opens the shared service and never starts this worker.
use super::{provider_work, with_service, ServiceState};
use aika_core::{
    contracts::*,
    sync::{CalendarJob, SyncClaim},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tauri::Emitter;
type Result<T> = std::result::Result<T, ServiceError>;
pub type SyncQueue = Arc<Mutex<BTreeMap<String, Option<EventRange>>>>;

struct NativeSyncAccess {
    state: ServiceState,
    claim: SyncClaim,
    job: CalendarJob,
}
impl aika_core::sync::SyncAccess for NativeSyncAccess {
    async fn start(&self, full: bool) -> Result<String> {
        let c = self.claim.clone();
        let job = CalendarJob {
            id: self.job.id.clone(),
            remote_id: self.job.remote_id.clone(),
            cursor: self.job.cursor.clone(),
        };
        with_service(self.state.clone(), move |s| {
            s.start_generation(&c, &job, full)
        })
        .await
    }
    async fn fetch(
        &self,
        cursor: Option<String>,
        page: Option<String>,
        projection: bool,
    ) -> Result<aika_core::sync::IncomingPage> {
        let c = self.claim.clone();
        let remote = self.job.remote_id.clone();
        provider_work(move || {
            c.adapter.incoming_page(
                &c.account.remote_subject,
                &remote,
                cursor.as_deref(),
                page.as_deref(),
                projection.then_some(&c.range),
            )
        })
        .await
    }
    async fn stage(
        &self,
        generation: String,
        page: aika_core::sync::IncomingPage,
        projection: bool,
    ) -> Result<()> {
        let c = self.claim.clone();
        with_service(self.state.clone(), move |s| {
            s.stage_page(&c, &generation, page, projection)
        })
        .await
    }
    async fn publish(&self, generation: String) -> Result<()> {
        let c = self.claim.clone();
        with_service(self.state.clone(), move |s| {
            s.publish_generation(&c, &generation)
        })
        .await
    }
}
async fn calendar(state: ServiceState, claim: SyncClaim, job: CalendarJob) -> Result<()> {
    let cursor = job.cursor.clone();
    aika_core::sync::receive_calendar(&NativeSyncAccess { state, claim, job }, cursor).await
}
async fn cycle(
    state: ServiceState,
    account: String,
    range: EventRange,
    app: tauri::AppHandle,
) -> Result<()> {
    let claim = with_service(state.clone(), move |s| s.begin_sync(&account, range)).await?;
    let Some(mut claim) = claim else {
        return Ok(());
    };
    let _ = app.emit("calendar-sync-changed", ());
    let c = claim.clone();
    let checkpoint_state = state.clone();
    let discovery = provider_work(move || {
        c.adapter
            .discover_for_sync(&c.account.remote_subject, &mut || {
                let claim = c.clone();
                tauri::async_runtime::block_on(with_service(checkpoint_state.clone(), move |s| {
                    s.check_sync(&claim)
                }))
            })
    })
    .await;
    let result = match discovery {
        Ok(calendars) => {
            let c = claim.clone();
            match with_service(state.clone(), move |s| {
                let mut c = c;
                let jobs = s.reconcile_sync(&mut c, calendars)?;
                Ok((c, jobs))
            })
            .await
            {
                Ok((updated, jobs)) => {
                    claim = updated;
                    let mut failure = None;
                    for job in jobs {
                        if let Err(f) = calendar(state.clone(), claim.clone(), job).await {
                            let stop = matches!(
                                f.code.as_str(),
                                "authenticationRequired"
                                    | "credentialUnavailable"
                                    | "syncSuperseded"
                            );
                            failure = Some(f);
                            if stop {
                                break;
                            }
                        }
                    }
                    failure.map_or(Ok(()), Err)
                }
                Err(f) => Err(f),
            }
        }
        Err(f) => Err(f),
    };
    let failure = result.err();
    with_service(state, move |s| s.end_sync(&claim, failure)).await
}

#[tauri::command]
pub async fn request_sync(
    state: tauri::State<'_, ServiceState>,
    queue: tauri::State<'_, SyncQueue>,
    app: tauri::AppHandle,
    range: EventRange,
) -> Result<()> {
    aika_core::service::validate_range(&range)?;
    let state = state.inner().clone();
    let accounts = with_service(state.clone(), |s| s.accounts()).await?;
    for account in accounts
        .into_iter()
        .filter(|a| a.connection_state == ConnectionState::Connected)
    {
        let queue = queue.inner().clone();
        let launch = {
            let mut q = queue.lock().map_err(|_| {
                aika_core::service::error("service", "Synchronization scheduler unavailable.")
            })?;
            if let Some(pending) = q.get_mut(&account.id) {
                *pending = Some(range.clone());
                false
            } else {
                q.insert(account.id.clone(), None);
                true
            }
        };
        if !launch {
            continue;
        }
        let state = state.clone();
        let app = app.clone();
        let mut range = range.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                // Only redacted status persisted by the service reaches the renderer.
                let _ = app.emit("calendar-sync-changed", ());
                let _ = cycle(state.clone(), account.id.clone(), range, app.clone()).await;
                let _ = app.emit("calendar-sync-changed", ());
                let next = {
                    let mut q = queue.lock().unwrap();
                    let next = q.get_mut(&account.id).and_then(Option::take);
                    if next.is_none() {
                        q.remove(&account.id);
                    }
                    next
                };
                match next {
                    Some(next) => range = next,
                    None => break,
                }
            }
        });
    }
    Ok(())
}
