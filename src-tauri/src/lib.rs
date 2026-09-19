//! Talos Terrace: a live, layered console for Talos Linux clusters.
//!
//! Module map:
//! - `error`: the one error type every command returns.
//! - `db`: SQLite with `PRAGMA user_version` migrations; settings and vault blobs.
//! - `vault`: one OS keychain entry (master key) and AES-256-GCM blobs in SQLite.
//! - `settings`: known keys, defaults, resolution.
//! - `cluster`: read-only Kubernetes client (kube-rs over rustls).
//! - `talos`: read-only `talosctl` sidecar (`version`, `get`, `get --watch`, `read`).
//! - `watch`: the live path: kube-runtime reflectors and `talosctl` watch triggers, pushed
//!   to the front end as events.
//! - `ollama`: read-only Ollama HTTP client with a timeout and a configurable URL.
//! - `metrics`: live samples from the kubelet summary API, a DCGM exporter and the hosts'
//!   sensors, polled by the watch hub; nothing installed on the cluster.
//! - `commands`: the Tauri commands, all async, all returning `Result<T, TerraceError>`.
//! - `logging`: tracing to a daily file plus an in-memory tail.

pub mod cluster;
pub mod commands;
pub mod db;
pub mod error;
pub mod logging;
pub mod metrics;
pub mod ollama;
pub mod settings;
pub mod state;
pub mod talos;
pub mod vault;
pub mod watch;

use tauri::Manager;

/// Choose rustls's crypto provider once per process. Both `ring` (kube-rs) and `aws-lc-rs`
/// (reqwest's default) are compiled in, and rustls panics on first use rather than pick one.
/// Call this before any TLS connection; every entry point (the app, the probe) does.
pub fn init_crypto() {
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::debug!("rustls crypto provider was already installed");
    }
}

/// A desktop app launched from the Dock or a launcher inherits a bare `PATH`
/// (`/usr/bin:/bin:/usr/sbin:/sbin` on macOS), so `talosctl` and any kubeconfig credential
/// plugin installed by Homebrew, `cargo install` or `~/.local/bin` are "not found". Append the
/// usual places that exist. Call it first thing in `main`, before any thread exists: setting
/// an environment variable is only sound while the process is single-threaded.
pub fn augment_path() {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut candidates = vec![
        std::path::PathBuf::from("/usr/local/bin"),
        std::path::PathBuf::from("/opt/homebrew/bin"),
        std::path::PathBuf::from("/opt/homebrew/sbin"),
        std::path::PathBuf::from("/snap/bin"),
    ];
    if let Some(home) = home {
        candidates.push(home.join(".local").join("bin"));
        candidates.push(home.join(".cargo").join("bin"));
        candidates.push(home.join("bin"));
    }
    let current = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<std::path::PathBuf> = std::env::split_paths(&current).collect();
    let mut added = false;
    for candidate in candidates {
        if candidate.is_dir() && !paths.contains(&candidate) {
            paths.push(candidate);
            added = true;
        }
    }
    if added && let Ok(joined) = std::env::join_paths(paths) {
        // SAFETY: called from `main` before the Tokio runtime or any other thread starts.
        unsafe { std::env::set_var("PATH", joined) };
    }
}

/// Build and run the desktop application.
pub fn run() {
    init_crypto();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let log_dir = data_dir.join("logs");
            if let Some(guard) = logging::init(&log_dir) {
                app.manage(logging::LogGuard(guard));
            }
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                data_dir = %data_dir.display(),
                "Terrace starting"
            );

            let state = state::AppState::open(&data_dir, &log_dir)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_recent_logs,
            commands::get_all_settings,
            commands::get_setting,
            commands::set_setting,
            commands::delete_setting,
            commands::vault_put,
            commands::vault_exists,
            commands::vault_delete,
            commands::vault_list,
            commands::cluster_summary,
            commands::talos_version,
            commands::talos_inventory,
            commands::ollama_status,
            commands::watch_start,
            commands::watch_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Terrace");
}
