//! Secrets are only available to backend adapters. No IPC, logging, or fallback storage.
use crate::contracts::ServiceError;
use crate::service::error;

type Result<T> = std::result::Result<T, ServiceError>;
pub trait CredentialStore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn set(&self, account: &str, secret: &str) -> Result<()>;
    fn delete(&self, account: &str) -> Result<()>;
}
struct OsStore;
fn entry(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new("dev.projectcal.desktop", account).map_err(store_error)
}
fn store_error(_: keyring::Error) -> ServiceError {
    error(
        "credentialUnavailable",
        "Unlock or restore access to the system credential store before connecting an account.",
    )
}
impl CredentialStore for OsStore {
    fn get(&self, account: &str) -> Result<Option<String>> {
        match entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(store_error(e)),
        }
    }
    fn set(&self, account: &str, secret: &str) -> Result<()> {
        entry(account)?.set_password(secret).map_err(store_error)
    }
    fn delete(&self, account: &str) -> Result<()> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(store_error(e)),
        }
    }
}
pub struct CredentialService {
    store: Box<dyn CredentialStore>,
}
impl Default for CredentialService {
    fn default() -> Self {
        Self {
            store: Box::new(OsStore),
        }
    }
}
impl CredentialService {
    #[cfg(test)]
    pub(crate) fn testing(store: Box<dyn CredentialStore>) -> Self {
        Self { store }
    }
    fn account(provider: &str, account: &str) -> Result<String> {
        if provider.is_empty() || account.is_empty() || provider.len() > 256 || account.len() > 256
        {
            return Err(error("validation", "Invalid connected-account identity."));
        }
        // Length prefix prevents delimiter collisions between provider/account pairs.
        Ok(format!("{}:{provider}{account}", provider.len()))
    }
    pub fn get(&self, provider: &str, account: &str) -> Result<Option<String>> {
        self.store.get(&Self::account(provider, account)?)
    }
    pub fn set(&self, provider: &str, account: &str, secret: &str) -> Result<()> {
        self.store.set(&Self::account(provider, account)?, secret)
    }
    pub fn delete(&self, provider: &str, account: &str) -> Result<()> {
        self.store.delete(&Self::account(provider, account)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Memory(Mutex<std::collections::BTreeMap<String, String>>);
    impl CredentialStore for Memory {
        fn get(&self, account: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, secret: &str) -> Result<()> {
            self.0.lock().unwrap().insert(account.into(), secret.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<()> {
            self.0.lock().unwrap().remove(account);
            Ok(())
        }
    }
    #[test]
    fn account_isolation_and_lifecycle() {
        let s = CredentialService {
            store: Box::<Memory>::default(),
        };
        s.set("a:b", "c", "one").unwrap();
        s.set("a", "b:c", "two").unwrap();
        assert_eq!(s.get("a:b", "c").unwrap().as_deref(), Some("one"));
        assert_eq!(s.get("a", "b:c").unwrap().as_deref(), Some("two"));
        s.delete("a:b", "c").unwrap();
        s.delete("a:b", "c").unwrap();
        assert!(s.get("a:b", "c").unwrap().is_none());
        assert!(s.get("", "c").is_err());
    }
    #[test]
    #[ignore = "requires an unlocked OS credential store; uses a disposable account"]
    fn real_os_store_roundtrip() {
        let s = CredentialService::default();
        let account = uuid::Uuid::new_v4().to_string();
        assert!(s.get("quality-check", &account).unwrap().is_none());
        s.set("quality-check", &account, "disposable-test-secret")
            .unwrap();
        let read = s.get("quality-check", &account);
        let deleted = s.delete("quality-check", &account);
        assert_eq!(read.unwrap().as_deref(), Some("disposable-test-secret"));
        deleted.unwrap();
        assert!(s.get("quality-check", &account).unwrap().is_none());
    }
    struct Unavailable;
    impl CredentialStore for Unavailable {
        fn get(&self, _: &str) -> Result<Option<String>> {
            Err(store_error(keyring::Error::NoEntry))
        }
        fn set(&self, _: &str, _: &str) -> Result<()> {
            self.get("").map(|_| ())
        }
        fn delete(&self, _: &str) -> Result<()> {
            self.get("").map(|_| ())
        }
    }
    #[test]
    fn unavailable_store_blocks_secrets_but_not_local_data() {
        let s = CredentialService {
            store: Box::new(Unavailable),
        };
        assert_eq!(
            s.get("test", "account").unwrap_err().code,
            "credentialUnavailable"
        );
        assert!(s.set("test", "account", "secret").is_err());
        assert!(s.delete("test", "account").is_err());
        assert!(!format!("{:?}", s.get("test", "account").unwrap_err()).contains("secret"));
        assert!(
            crate::service::CalendarService::open(std::path::Path::new(":memory:"))
                .unwrap()
                .preferences()
                .is_ok()
        );
    }
}
