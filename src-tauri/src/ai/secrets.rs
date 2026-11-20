//! Where the RemNote token lives: the OS keychain, never SQLite, logs or exports.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::{AppError, AppResult};

const SERVICE: &str = "gabaritei";
const REMNOTE_TOKEN: &str = "remnote-token";

/// A place to keep secrets. The app uses the OS keychain; tests use memory.
pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> AppResult<Option<String>>;
    fn set(&self, name: &str, value: &str) -> AppResult<()>;
    fn delete(&self, name: &str) -> AppResult<()>;
}

fn keychain_error(error: &keyring::Error) -> AppError {
    AppError::Invalid(format!(
        "Não foi possível acessar o chaveiro do sistema para guardar o token ({error})."
    ))
}

/// The operating system's credential store (Windows Credential Manager, macOS Keychain).
pub struct Keychain;

impl SecretStore for Keychain {
    fn get(&self, name: &str) -> AppResult<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, name).map_err(|e| keychain_error(&e))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keychain_error(&error)),
        }
    }

    fn set(&self, name: &str, value: &str) -> AppResult<()> {
        keyring::Entry::new(SERVICE, name)
            .and_then(|entry| entry.set_password(value))
            .map_err(|e| keychain_error(&e))
    }

    fn delete(&self, name: &str) -> AppResult<()> {
        let entry = keyring::Entry::new(SERVICE, name).map_err(|e| keychain_error(&e))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keychain_error(&error)),
        }
    }
}

/// An in-memory store for tests.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, name: &str) -> AppResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> AppResult<()> {
        self.0
            .lock()
            .unwrap()
            .insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, name: &str) -> AppResult<()> {
        self.0.lock().unwrap().remove(name);
        Ok(())
    }
}

/// Tauri-managed handle to the secret store.
pub struct Secrets(pub Box<dyn SecretStore>);

impl Secrets {
    pub fn keychain() -> Self {
        Self(Box::new(Keychain))
    }

    pub fn memory() -> Self {
        Self(Box::<MemorySecrets>::default())
    }

    pub fn remnote_token(&self) -> AppResult<Option<String>> {
        self.0.get(REMNOTE_TOKEN)
    }

    /// Saves the token, or removes it when `token` is `None` or blank.
    pub fn set_remnote_token(&self, token: Option<&str>) -> AppResult<()> {
        match token.map(str::trim).filter(|t| !t.is_empty()) {
            Some(token) => self.0.set(REMNOTE_TOKEN, token),
            None => self.0.delete(REMNOTE_TOKEN),
        }
    }
}
