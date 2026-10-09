//! Shared production database location; matches Tauri's data_dir/identifier convention.
use crate::{contracts::ServiceError, service::error};
use std::path::PathBuf;

pub const APP_IDENTIFIER: &str = "dev.projectcal.desktop";

pub fn app_data_dir() -> Result<PathBuf, ServiceError> {
    dirs::data_dir()
        .map(|directory| directory.join(APP_IDENTIFIER))
        .ok_or_else(|| error("storage", "Could not locate application data."))
}

pub fn database_path() -> Result<PathBuf, ServiceError> {
    Ok(app_data_dir()?.join("calendar.sqlite3"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_identifier_matches_desktop_config() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"].as_str(), Some(APP_IDENTIFIER));
    }
}
