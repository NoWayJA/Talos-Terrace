//! Managed application state: the database, the vault and where things live on disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;

use crate::db::Database;
use crate::error::Result;
use crate::vault::{MasterKey, Vault};
use crate::watch::WatchHub;

pub struct AppState {
    pub db: Arc<Database>,
    /// Opened on first use: the vault holds nothing yet, and a desktop without a keychain
    /// service (a bare Linux session) must not keep Terrace from starting.
    vault: Mutex<Option<Arc<Vault>>>,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub started_at: Instant,
    /// The live watches, if any are running.
    pub watch: WatchHub,
}

impl AppState {
    pub fn open(data_dir: &Path, log_dir: &Path) -> Result<Self> {
        let db = Arc::new(Database::open(&data_dir.join("terrace.db"))?);
        tracing::info!(path = %db.path().display(), "database ready");

        Ok(Self {
            db,
            vault: Mutex::new(None),
            data_dir: data_dir.to_path_buf(),
            log_dir: log_dir.to_path_buf(),
            started_at: Instant::now(),
            watch: WatchHub::default(),
        })
    }

    /// The vault, unlocking the master key on the first call.
    pub fn vault(&self) -> Result<Arc<Vault>> {
        let mut slot = self.vault.lock();
        if let Some(vault) = slot.as_ref() {
            return Ok(Arc::clone(vault));
        }
        let master = MasterKey::load_or_create(
            &self.data_dir,
            !self
                .db
                .list_secret_names(crate::vault::VAULT_PROFILE)?
                .is_empty(),
        )?;
        let vault = Arc::new(Vault::new(Arc::clone(&self.db), master));
        tracing::info!(profile = crate::vault::VAULT_PROFILE, "vault ready");
        *slot = Some(Arc::clone(&vault));
        Ok(vault)
    }
}
