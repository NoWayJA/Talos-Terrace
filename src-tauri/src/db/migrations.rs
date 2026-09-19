//! Schema migrations, applied in order and recorded in `PRAGMA user_version`.
//! Append a new entry for every schema change; never edit one that has shipped.

pub const MIGRATIONS: &[&str] = &[
    // 1. Settings and vault blobs.
    "CREATE TABLE settings (
        key TEXT PRIMARY KEY NOT NULL,
        value TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE vault_secret_blobs (
        profile TEXT NOT NULL,
        name TEXT NOT NULL,
        nonce BLOB NOT NULL,
        ciphertext BLOB NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (profile, name)
    );",
];
