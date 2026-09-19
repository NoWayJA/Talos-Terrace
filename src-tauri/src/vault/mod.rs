//! Secrets vault. Lifted from Modz (NoWayJA/Modz, MIT) secrets/mod.rs and made cross-platform.
//!
//! One OS keychain entry holds the 32-byte master key (release builds); every secret is
//! AES-256-GCM encrypted into the `vault_secret_blobs` table, so the keychain is touched once
//! per session. Debug builds keep the master key in a `chmod 600` file in the data directory:
//! an ad-hoc-signed binary changes identity on every rebuild, and the keychain ACL would
//! re-prompt or, worse, hand back `NoEntry` and mint a fresh key that orphans every blob.
//!
//! Dropped from Modz: the macOS-only Touch ID `SecItemAdd` FFI, workspaces, the async trait.
//! Terrace has no secrets yet: kubeconfig and talosconfig are read in place and only their
//! paths are stored. The vault is here so the first token has a home.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aes_gcm::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use zeroize::Zeroizing;

use crate::db::Database;
use crate::error::{Result, TerraceError};

const SERVICE_PREFIX: &str = "com.nowayja.talosterrace.vault.master_key";
const ACCOUNT: &str = "master";
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;
const DEV_KEY_FILE: &str = ".master_key.dev";

#[cfg(debug_assertions)]
pub const VAULT_PROFILE: &str = "dev";
#[cfg(not(debug_assertions))]
pub const VAULT_PROFILE: &str = "release";

fn service_name() -> String {
    format!("{SERVICE_PREFIX}.{VAULT_PROFILE}")
}

fn backend(error: impl fmt::Display) -> TerraceError {
    TerraceError::Vault(error.to_string())
}

/// A secret string that zeroes its allocation on drop and never prints itself.
#[derive(Clone)]
pub struct SecretValue(Zeroizing<String>);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(Zeroizing::new(value.into()))
    }

    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretValue(<redacted>)")
    }
}

impl fmt::Display for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The AES-256 key for this session. Zeroised on drop.
pub struct MasterKey {
    key: Zeroizing<[u8; KEY_BYTES]>,
}

impl MasterKey {
    /// Keychain in release builds, a `chmod 600` file in debug builds; generated on first use.
    pub fn load_or_create(data_dir: &Path, has_secrets: bool) -> Result<Self> {
        if cfg!(debug_assertions) {
            Self::load_or_create_dev_file(data_dir, has_secrets)
        } else {
            Self::load_or_create_keychain(has_secrets)
        }
    }

    fn generate() -> Self {
        let key = Aes256Gcm::generate_key(OsRng);
        let mut bytes = [0u8; KEY_BYTES];
        bytes.copy_from_slice(&key);
        Self {
            key: Zeroizing::new(bytes),
        }
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != KEY_BYTES {
            return Err(TerraceError::Vault(format!(
                "master key has the wrong length ({} bytes)",
                bytes.len()
            )));
        }
        let mut array = [0u8; KEY_BYTES];
        array.copy_from_slice(bytes);
        Ok(Self {
            key: Zeroizing::new(array),
        })
    }

    fn load_or_create_keychain(has_secrets: bool) -> Result<Self> {
        let entry = keyring::Entry::new(&service_name(), ACCOUNT).map_err(backend)?;
        match entry.get_password() {
            Ok(encoded) => {
                let encoded = Zeroizing::new(encoded);
                let bytes = Zeroizing::new(B64.decode(encoded.trim()).map_err(backend)?);
                tracing::info!(
                    profile = VAULT_PROFILE,
                    "master key loaded from the keychain"
                );
                Self::from_bytes(&bytes)
            }
            Err(keyring::Error::NoEntry) if has_secrets => Err(TerraceError::Vault(
                "master key is missing but encrypted secrets exist; restore the original key"
                    .into(),
            )),
            Err(keyring::Error::NoEntry) => {
                let key = Self::generate();
                entry
                    .set_password(&B64.encode(key.key.as_ref()))
                    .map_err(backend)?;
                tracing::info!(
                    profile = VAULT_PROFILE,
                    "master key generated (first launch)"
                );
                Ok(key)
            }
            Err(error) => Err(backend(error)),
        }
    }

    fn load_or_create_dev_file(data_dir: &Path, has_secrets: bool) -> Result<Self> {
        let path = dev_key_path(data_dir);
        match std::fs::read(&path) {
            Ok(bytes) => return Self::from_bytes(&Zeroizing::new(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && !has_secrets => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(TerraceError::Vault(
                    "master key is missing but encrypted secrets exist; restore the original key"
                        .into(),
                ));
            }
            Err(error) => return Err(error.into()),
        }

        let key = Self::generate();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_private_file(&path, key.key.as_ref())?;
        tracing::info!(path = %path.display(), "dev master key generated (first launch)");
        Ok(key)
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(self.key.as_ref()))
    }

    /// The secret's name is bound in as associated data, so a blob moved to another name in
    /// the database (by anyone who can write the file but does not hold the key) fails to open
    /// rather than reading as that name's secret.
    fn encrypt(&self, name: &str, plaintext: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let aad = associated_data(name);
        let ciphertext = self
            .cipher()
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|error| TerraceError::Vault(format!("encrypt: {error}")))?;
        Ok((nonce.to_vec(), ciphertext))
    }

    fn decrypt(&self, name: &str, nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
        if nonce.len() != NONCE_BYTES {
            return Err(TerraceError::Vault(format!(
                "bad nonce length {}",
                nonce.len()
            )));
        }
        let aad = associated_data(name);
        let plaintext = self
            .cipher()
            .decrypt(
                Nonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|error| TerraceError::Vault(format!("decrypt: {error}")))?;
        String::from_utf8(plaintext)
            .map_err(|error| TerraceError::Vault(format!("decrypted text is not UTF-8: {error}")))
    }
}

fn associated_data(name: &str) -> String {
    format!("{VAULT_PROFILE}/{name}")
}

fn dev_key_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DEV_KEY_FILE)
}

/// Create the file owner-only from the first byte: never a moment at the umask's mode, and
/// never through a symlink somebody left at that path.
fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Named secrets, encrypted with the master key, stored in SQLite.
pub struct Vault {
    db: Arc<Database>,
    master: MasterKey,
}

impl Vault {
    pub fn new(db: Arc<Database>, master: MasterKey) -> Self {
        Self { db, master }
    }

    fn check_name(name: &str) -> Result<()> {
        if name.trim().is_empty() {
            return Err(TerraceError::Vault("secret name is empty".into()));
        }
        Ok(())
    }

    pub fn put(&self, name: &str, value: SecretValue) -> Result<()> {
        Self::check_name(name)?;
        let (nonce, ciphertext) = self.master.encrypt(name, value.expose())?;
        self.db
            .upsert_secret_blob(VAULT_PROFILE, name, &nonce, &ciphertext)?;
        tracing::info!(name, profile = VAULT_PROFILE, "vault put");
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Option<SecretValue>> {
        Self::check_name(name)?;
        match self.db.get_secret_blob(VAULT_PROFILE, name)? {
            Some((nonce, ciphertext)) => Ok(Some(SecretValue::new(self.master.decrypt(
                name,
                &nonce,
                &ciphertext,
            )?))),
            None => Ok(None),
        }
    }

    pub fn exists(&self, name: &str) -> Result<bool> {
        Self::check_name(name)?;
        Ok(self.db.get_secret_blob(VAULT_PROFILE, name)?.is_some())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        Self::check_name(name)?;
        self.db.delete_secret_blob(VAULT_PROFILE, name)?;
        tracing::info!(name, profile = VAULT_PROFILE, "vault delete");
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<String>> {
        self.db.list_secret_names(VAULT_PROFILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> Vault {
        Vault::new(
            Arc::new(Database::open_in_memory().expect("db")),
            MasterKey::generate(),
        )
    }

    #[test]
    fn secret_value_redacts_debug_and_display() {
        let secret = SecretValue::new("super-secret-token");
        assert!(!format!("{secret:?}").contains("super-secret"));
        assert!(!format!("{secret}").contains("super-secret"));
        assert_eq!(secret.expose(), "super-secret-token");
    }

    #[test]
    fn round_trip_exists_list_delete() {
        let vault = vault();
        assert!(!vault.exists("api.example").expect("exists"));
        vault
            .put("api.example", SecretValue::new("value-1"))
            .expect("put");
        assert!(vault.exists("api.example").expect("exists"));
        assert_eq!(
            vault
                .get("api.example")
                .expect("get")
                .expect("some")
                .expose(),
            "value-1"
        );
        assert_eq!(vault.list().expect("list"), vec!["api.example".to_string()]);
        vault.delete("api.example").expect("delete");
        assert!(vault.get("api.example").expect("get").is_none());
    }

    #[test]
    fn a_different_master_key_cannot_decrypt() {
        let db = Arc::new(Database::open_in_memory().expect("db"));
        let first = Vault::new(Arc::clone(&db), MasterKey::generate());
        first.put("t", SecretValue::new("hidden")).expect("put");
        let second = Vault::new(db, MasterKey::generate());
        assert!(second.get("t").is_err());
    }

    #[test]
    fn a_blob_moved_to_another_name_does_not_open() {
        let db = Arc::new(Database::open_in_memory().expect("db"));
        let vault = Vault::new(Arc::clone(&db), MasterKey::generate());
        vault.put("a", SecretValue::new("for a")).expect("put");
        let (nonce, ciphertext) = db
            .get_secret_blob(VAULT_PROFILE, "a")
            .expect("get")
            .expect("some");
        db.upsert_secret_blob(VAULT_PROFILE, "b", &nonce, &ciphertext)
            .expect("move");
        assert!(vault.get("b").is_err());
        assert_eq!(
            vault.get("a").expect("get").expect("some").expose(),
            "for a"
        );
    }

    #[test]
    fn empty_names_are_rejected() {
        let vault = vault();
        assert!(vault.put("  ", SecretValue::new("x")).is_err());
    }

    #[test]
    fn dev_key_file_is_created_restricted_and_reloaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = MasterKey::load_or_create_dev_file(dir.path(), false).expect("create");
        let second = MasterKey::load_or_create_dev_file(dir.path(), false).expect("reload");
        assert_eq!(first.key.as_ref(), second.key.as_ref());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dev_key_path(dir.path()))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn corrupt_dev_key_is_not_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dev_key_path(dir.path());
        std::fs::write(&path, b"incomplete key").expect("fixture");
        assert!(MasterKey::load_or_create_dev_file(dir.path(), false).is_err());
        assert_eq!(std::fs::read(path).expect("read"), b"incomplete key");
    }

    #[test]
    fn missing_key_with_existing_secrets_is_not_regenerated() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(MasterKey::load_or_create_dev_file(dir.path(), true).is_err());
        assert!(!dev_key_path(dir.path()).exists());
    }

    #[test]
    fn key_creation_cannot_overwrite_an_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dev_key_path(dir.path());
        std::fs::write(&path, b"original").expect("fixture");
        assert!(write_private_file(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(path).expect("read"), b"original");
    }

    #[test]
    fn service_name_is_profile_scoped() {
        let name = service_name();
        assert!(name.starts_with(SERVICE_PREFIX));
        assert!(name.ends_with(".dev") || name.ends_with(".release"));
    }
}
