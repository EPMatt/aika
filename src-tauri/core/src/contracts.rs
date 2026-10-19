pub use aika_domain::contracts::*;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub read: bool,
    pub create: bool,
    pub edit: bool,
    pub delete: bool,
    pub recurrence: bool,
    pub customize_metadata: bool,
    pub move_event: bool,
}
impl Capabilities {
    pub fn local() -> Self {
        Self {
            read: true,
            create: true,
            edit: true,
            delete: true,
            recurrence: true,
            customize_metadata: true,
            move_event: true,
        }
    }
    pub fn google_recurring() -> Self {
        Self {
            recurrence: true,
            ..Self::ordinary()
        }
    }
    pub fn ordinary() -> Self {
        Self {
            recurrence: false,
            move_event: false,
            ..Self::local()
        }
    }
    pub fn read_only() -> Self {
        Self {
            read: true,
            create: false,
            edit: false,
            delete: false,
            recurrence: false,
            customize_metadata: true,
            move_event: false,
        }
    }
    pub fn intersect(&self, other: &Self) -> Self {
        Self {
            read: self.read && other.read,
            create: self.create && other.create,
            edit: self.edit && other.edit,
            delete: self.delete && other.delete,
            recurrence: self.recurrence && other.recurrence,
            customize_metadata: self.customize_metadata && other.customize_metadata,
            move_event: self.move_event && other.move_event,
        }
    }
}
/// Scope availability is backend-owned; absence of a reason means available.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScopeAvailability {
    pub scope: MutationScope,
    pub disabled_reason: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EventActions {
    pub edit: Vec<ScopeAvailability>,
    pub delete: Vec<ScopeAvailability>,
}
impl EventActions {
    pub fn for_event(caps: &Capabilities, recurring: bool, _remote: bool) -> Self {
        let scopes = if recurring {
            vec![
                MutationScope::Occurrence,
                MutationScope::Following,
                MutationScope::Series,
            ]
        } else {
            vec![MutationScope::Series]
        };
        let available = |allowed: bool| {
            scopes
                .iter()
                .map(|scope| ScopeAvailability {
                    scope: scope.clone(),
                    disabled_reason: if !allowed || (recurring && !caps.recurrence) {
                        Some("This calendar or event is read-only.".into())
                    } else {
                        None
                    },
                })
                .collect()
        };
        Self {
            edit: available(caps.edit),
            delete: available(caps.delete),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDescriptor {
    pub id: String,
    pub name: String,
    pub connection_available: bool,
    pub unavailable_reason: Option<String>,
    pub capabilities: Capabilities,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    Connected,
    AuthenticationRequired,
    Disconnected,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub provider_id: String,
    pub remote_subject: String,
    pub display_name: String,
    pub connection_state: ConnectionState,
    pub selected_calendar_ids: Vec<String>,
}
/// Backend collection enrichment stays out of the pure recurrence/WASM domain.
#[derive(Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarCollectionSnapshot {
    pub calendars: Vec<Calendar>,
    pub events: Vec<CalendarEvent>,
    pub default_calendar_id: Option<String>,
    pub hidden_calendar_ids: Vec<String>,
    pub can_undo: bool,
    pub accounts: Vec<AccountSummary>,
    pub calendar_capabilities: BTreeMap<String, Capabilities>,
    pub event_capabilities: BTreeMap<String, Capabilities>,
    pub sync_status: Vec<AccountSyncStatus>,
    pub event_actions: BTreeMap<String, EventActions>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionProgress {
    WaitingForBrowser,
    Connected,
    Cancelled,
    Failed,
    TimedOut,
}
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSession {
    pub id: String,
    pub progress: ConnectionProgress,
    pub account_id: Option<String>,
    pub failure: Option<ServiceError>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredCalendar {
    pub remote_id: String,
    pub name: String,
    pub color: String,
    pub description: String,
    pub access_role: String,
    pub eligible: bool,
    pub disabled_reason: Option<String>,
    pub selected: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarDiscovery {
    pub id: String,
    pub account: AccountSummary,
    pub calendars: Vec<DiscoveredCalendar>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSyncStatus {
    pub calendar_id: String,
    pub available: bool,
    pub range_complete: bool,
    pub last_success: Option<String>,
}
#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AccountSyncStatus {
    pub account_id: String,
    pub syncing: bool,
    pub last_success: Option<String>,
    pub failure: Option<ServiceError>,
    pub pending_count: u32,
    pub conflict_count: u32,
    pub calendars: Vec<CalendarSyncStatus>,
    #[ts(optional)]
    pub recovery_groups: Option<Vec<GroupProgress>>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GroupProgress {
    pub id: String,
    pub title: String,
    pub completed_steps: u32,
    pub total_steps: u32,
    pub blocked: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SyncConflict {
    pub id: String,
    pub account_id: String,
    pub calendar_id: String,
    pub event_id: String,
    pub local: Option<EventSeries>,
    pub remote: Option<EventSeries>,
    pub remote_revision: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub group_revision: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub recovery_steps: Option<Vec<GroupRecoveryStep>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GroupRecoveryStep {
    pub title: String,
    pub operation: String,
    pub state: String,
}

/// Checked review of account-wide local intent. The revision is opaque to clients.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectReview {
    pub account_id: String,
    pub pending_count: u32,
    pub conflict_count: u32,
    pub attempted_count: u32,
    pub revision: String,
}

/// Checked removal of one imported calendar; no provider resources are deleted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CalendarRemovalReview {
    pub calendar_id: String,
    pub account_id: String,
    pub pending_count: u32,
    pub conflict_count: u32,
    pub attempted_count: u32,
    pub revision: String,
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    #[test]
    fn remote_recurrence_enables_all_scopes() {
        let remote = EventActions::for_event(&Capabilities::local(), true, true);
        assert_eq!(remote.edit.len(), 3);
        for action in remote.edit.iter().chain(&remote.delete) {
            assert!(action.disabled_reason.is_none());
        }
        let local = EventActions::for_event(&Capabilities::local(), true, false);
        assert!(local
            .edit
            .iter()
            .chain(&local.delete)
            .all(|s| s.disabled_reason.is_none()));
        let ordinary = EventActions::for_event(&Capabilities::ordinary(), false, true);
        assert_eq!(ordinary.edit.len(), 1);
        assert_eq!(ordinary.edit[0].scope, MutationScope::Series);
        assert!(ordinary.edit[0].disabled_reason.is_none());
    }
}
