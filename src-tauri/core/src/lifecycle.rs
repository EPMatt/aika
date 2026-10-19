//! Cross-process credential fence. Locks cover local checks/store access only, never HTTP.
use crate::{contracts::ServiceError, service::error};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, ServiceError>;
#[derive(Clone)]
pub(crate) struct Lifecycle(pub PathBuf);
pub(crate) fn identity(provider: &str, subject: &str) -> String {
    let mut hash = Sha256::new();
    hash.update((provider.len() as u64).to_le_bytes());
    hash.update(provider.as_bytes());
    hash.update(subject.as_bytes());
    format!("{:x}", hash.finalize())
}
impl Lifecycle {
    pub fn new(path: &Path) -> Self {
        Self(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
    }
    pub fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.0.with_extension("identity.lock"))
            .map_err(|_| error("storage", "Could not open account identity lock."))?;
        file.lock()
            .map_err(|_| error("storage", "Could not lock account identity."))?;
        Ok(file)
    }
    pub fn db(&self) -> Result<Connection> {
        let db = Connection::open(&self.0)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(db)
    }
    pub fn epoch(&self) -> Result<i64> {
        Ok(self
            .db()?
            .query_row("SELECT epoch FROM lifecycle_clock", [], |r| r.get(0))?)
    }
    pub fn check(&self, subject: &str, started: Option<i64>) -> Result<()> {
        check(&self.db()?, "google", subject, started)
    }
}
pub(crate) fn check(
    db: &Connection,
    provider: &str,
    subject: &str,
    started: Option<i64>,
) -> Result<()> {
    let state: Option<(i64, String)> = db
        .query_row(
            "SELECT epoch,state FROM identity_lifecycles WHERE identity_hash=?1",
            [identity(provider, subject)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((epoch, state)) = state {
        if state == "removing"
            || started.is_some_and(|s| s < epoch)
            || (state == "removed" && started.is_none())
        {
            return Err(error(
                "accountRemoved",
                "This account was removed. Start a new sign-in after cleanup finishes.",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_lock_is_shared_with_an_independent_process() {
        let path = std::env::temp_dir().join(format!("aika-lock-{}.sqlite3", uuid::Uuid::new_v4()));
        let lifecycle = Lifecycle::new(&path);
        let lock = lifecycle.lock().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lifecycle::tests::child_lock_probe", "--ignored"])
            .env("AIKA_LOCK_TEST_PATH", &path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        drop(lock);
        lifecycle.lock().unwrap();
        std::fs::remove_file(path.with_extension("identity.lock")).unwrap();
    }
    #[test]
    #[ignore = "subprocess helper for identity_lock_is_shared_with_an_independent_process"]
    fn child_lock_probe() {
        let path = std::env::var_os("AIKA_LOCK_TEST_PATH").expect("parent lock test required");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(PathBuf::from(path).with_extension("identity.lock"))
            .unwrap();
        assert!(file.try_lock().is_err());
    }
}
