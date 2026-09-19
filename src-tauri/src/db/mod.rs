//! SQLite storage for Terrace's own state: settings and encrypted vault blobs.
//!
//! Migrations run from the first table through `PRAGMA user_version` (a Modz lesson: ad hoc schema checks drifted).
//! The connection sits behind a `parking_lot::Mutex`, which cannot poison, so there is no
//! `lock().unwrap()` anywhere (a Modz lesson: a poisoned mutex took the app down).

pub mod migrations;

use std::path::{Path, PathBuf};
use std::time::Duration;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{Result, TerraceError};

pub struct Database {
    conn: Mutex<Connection>,
    path: PathBuf,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

impl Database {
    /// Open (or create) the database file and bring the schema up to date.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(()))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let db = Self {
            conn: Mutex::new(conn),
            path: path.to_path_buf(),
        };
        db.migrate()?;
        Ok(db)
    }

    /// An in-memory database with the full schema, for tests.
    pub fn open_in_memory() -> Result<Self> {
        let db = Self {
            conn: Mutex::new(Connection::open_in_memory()?),
            path: PathBuf::from(":memory:"),
        };
        db.migrate()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn user_version(&self) -> Result<u32> {
        let conn = self.conn.lock();
        Ok(conn.query_row("PRAGMA user_version", [], |row| row.get(0))?)
    }

    fn migrate(&self) -> Result<()> {
        let mut conn = self.conn.lock();
        let current: u32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let target = u32::try_from(migrations::MIGRATIONS.len())
            .map_err(|_| TerraceError::Config("too many migrations".into()))?;
        if current > target {
            return Err(TerraceError::Config(format!(
                "database schema {current} is newer than this build supports ({target})"
            )));
        }
        for (index, sql) in migrations::MIGRATIONS
            .iter()
            .enumerate()
            .skip(current as usize)
        {
            let version = u32::try_from(index + 1)
                .map_err(|_| TerraceError::Config("too many migrations".into()))?;
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
            tracing::info!(version, "applied database migration");
        }
        Ok(())
    }

    // --- Settings ---------------------------------------------------------

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, now()],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<Vec<(String, String)>> {
        let conn = self.conn.lock();
        let mut statement = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    // --- Vault blobs ------------------------------------------------------

    pub fn upsert_secret_blob(
        &self,
        profile: &str,
        name: &str,
        nonce: &[u8],
        ciphertext: &[u8],
    ) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO vault_secret_blobs (profile, name, nonce, ciphertext, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(profile, name) DO UPDATE SET
               nonce = excluded.nonce, ciphertext = excluded.ciphertext,
               updated_at = excluded.updated_at",
            params![profile, name, nonce, ciphertext, now()],
        )?;
        Ok(())
    }

    pub fn get_secret_blob(&self, profile: &str, name: &str) -> Result<Option<(Vec<u8>, Vec<u8>)>> {
        let conn = self.conn.lock();
        Ok(conn
            .query_row(
                "SELECT nonce, ciphertext FROM vault_secret_blobs WHERE profile = ?1 AND name = ?2",
                params![profile, name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn delete_secret_blob(&self, profile: &str, name: &str) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute(
            "DELETE FROM vault_secret_blobs WHERE profile = ?1 AND name = ?2",
            params![profile, name],
        )?;
        Ok(())
    }

    pub fn list_secret_names(&self, profile: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock();
        let mut statement =
            conn.prepare("SELECT name FROM vault_secret_blobs WHERE profile = ?1 ORDER BY name")?;
        let rows = statement.query_map(params![profile], |row| row.get(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_bring_user_version_to_the_latest() {
        let db = Database::open_in_memory().expect("db");
        assert_eq!(
            db.user_version().expect("version") as usize,
            migrations::MIGRATIONS.len()
        );
    }

    #[test]
    fn reopening_a_file_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("terrace.db");
        {
            let db = Database::open(&path).expect("first open");
            db.set_setting("a", "1").expect("set");
        }
        let db = Database::open(&path).expect("second open");
        assert_eq!(db.get_setting("a").expect("get"), Some("1".to_string()));
        assert_eq!(
            db.user_version().expect("version") as usize,
            migrations::MIGRATIONS.len()
        );
    }

    #[test]
    fn settings_round_trip_and_delete() {
        let db = Database::open_in_memory().expect("db");
        assert_eq!(db.get_setting("theme").expect("get"), None);
        db.set_setting("theme", "dark").expect("set");
        db.set_setting("theme", "light").expect("update");
        assert_eq!(
            db.get_setting("theme").expect("get"),
            Some("light".to_string())
        );
        assert_eq!(
            db.all_settings().expect("all"),
            vec![("theme".to_string(), "light".to_string())]
        );
        db.delete_setting("theme").expect("delete");
        assert_eq!(db.get_setting("theme").expect("get"), None);
    }

    #[test]
    fn secret_blobs_round_trip_per_profile() {
        let db = Database::open_in_memory().expect("db");
        db.upsert_secret_blob("dev", "token", b"nonce", b"cipher")
            .expect("put");
        assert_eq!(
            db.get_secret_blob("dev", "token").expect("get"),
            Some((b"nonce".to_vec(), b"cipher".to_vec()))
        );
        assert_eq!(db.get_secret_blob("release", "token").expect("get"), None);
        assert_eq!(
            db.list_secret_names("dev").expect("list"),
            vec!["token".to_string()]
        );
        db.delete_secret_blob("dev", "token").expect("delete");
        assert!(db.list_secret_names("dev").expect("list").is_empty());
    }
}
