//! Tauri commands. Conventions lifted from Modz (NoWayJA/Modz, MIT) and tightened: every
//! command is async, snake_case, takes camelCase JSON, and returns `Result<T, TerraceError>`.
//! Everything here is read-only against the cluster. The only writes are to Terrace's own
//! settings database and vault.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::cluster::{self, Classifier, ClusterSummary};
use crate::error::Result;
use crate::metrics::{DcgmSetting, MetricsOptions};
use crate::ollama::{OllamaClient, OllamaStatus};
use crate::settings::{self, SettingRow, keys};
use crate::state::AppState;
use crate::talos::{TalosCli, TalosInventory, TalosVersion};
use crate::vault::SecretValue;
use crate::watch::{AppSink, WatchSources};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
    pub profile: String,
    pub data_dir: String,
    pub log_dir: String,
    pub uptime_seconds: u64,
}

// --- App ---------------------------------------------------------------------

#[tauri::command]
pub async fn get_app_info(state: State<'_, AppState>) -> Result<AppInfo> {
    Ok(AppInfo {
        name: "Talos Terrace".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
        .to_string(),
        data_dir: state.data_dir.display().to_string(),
        log_dir: state.log_dir.display().to_string(),
        uptime_seconds: state.started_at.elapsed().as_secs(),
    })
}

#[tauri::command]
pub async fn get_recent_logs(count: usize) -> Result<Vec<String>> {
    Ok(crate::logging::recent(count.clamp(1, 500)))
}

// --- Settings ----------------------------------------------------------------

#[tauri::command]
pub async fn get_all_settings(state: State<'_, AppState>) -> Result<Vec<SettingRow>> {
    settings::all(&state.db)
}

#[tauri::command]
pub async fn get_setting(key: String, state: State<'_, AppState>) -> Result<Option<String>> {
    Ok(settings::resolve(&state.db, &key)?.map(|row| row.value))
}

#[tauri::command]
pub async fn set_setting(key: String, value: String, state: State<'_, AppState>) -> Result<()> {
    settings::set(&state.db, &key, &value)?;
    tracing::info!(key, "setting updated");
    Ok(())
}

#[tauri::command]
pub async fn delete_setting(key: String, state: State<'_, AppState>) -> Result<()> {
    state.db.delete_setting(&key)?;
    tracing::info!(key, "setting deleted");
    Ok(())
}

// --- Vault -------------------------------------------------------------------

#[tauri::command]
pub async fn vault_put(name: String, value: String, state: State<'_, AppState>) -> Result<()> {
    state.vault()?.put(&name, SecretValue::new(value))
}

#[tauri::command]
pub async fn vault_exists(name: String, state: State<'_, AppState>) -> Result<bool> {
    state.vault()?.exists(&name)
}

#[tauri::command]
pub async fn vault_delete(name: String, state: State<'_, AppState>) -> Result<()> {
    state.vault()?.delete(&name)
}

#[tauri::command]
pub async fn vault_list(state: State<'_, AppState>) -> Result<Vec<String>> {
    state.vault()?.list()
}

// --- Kubernetes --------------------------------------------------------------

/// The user's namespace overrides for apps versus platform, from settings.
fn classifier(state: &AppState) -> Result<Classifier> {
    let value = |key: &str| -> Result<String> {
        Ok(settings::resolve(&state.db, key)?
            .map(|row| row.value)
            .unwrap_or_default())
    };
    Ok(Classifier::from_settings(
        &value(keys::WORKLOADS_APPS)?,
        &value(keys::WORKLOADS_PLATFORM)?,
    ))
}

#[tauri::command]
pub async fn cluster_summary(state: State<'_, AppState>) -> Result<ClusterSummary> {
    let path = settings::require_path(&state.db, keys::KUBECONFIG_PATH)?;
    let context = settings::resolve(&state.db, keys::KUBE_CONTEXT)?.map(|row| row.value);
    let classifier = classifier(&state)?;
    cluster::summary(Path::new(&path), context.as_deref(), &classifier).await
}

// --- Talos -------------------------------------------------------------------

fn talos_cli(state: &AppState) -> Result<TalosCli> {
    let binary = settings::require_path(&state.db, keys::TALOSCTL_PATH)?;
    let talosconfig = settings::require_path(&state.db, keys::TALOSCONFIG_PATH)?;
    Ok(TalosCli::new(binary, talosconfig))
}

#[tauri::command]
pub async fn talos_version(state: State<'_, AppState>) -> Result<TalosVersion> {
    talos_cli(&state)?.version().await
}

/// Everything the hardware, Talos and trust layers show, for every node in the talosconfig.
#[tauri::command]
pub async fn talos_inventory(state: State<'_, AppState>) -> Result<TalosInventory> {
    talos_cli(&state)?.inventory().await
}

// --- Live watches --------------------------------------------------------------

/// Metric sampling as the settings ask for it: `None` when the interval is `0`. A value the
/// Settings page would have refused (the database was edited by hand) falls back to the
/// default with a warning rather than keeping the Kubernetes and Talos watches from starting.
fn metrics_options(state: &AppState) -> Result<Option<MetricsOptions>> {
    let value = |key: &str| -> Result<String> {
        Ok(settings::resolve(&state.db, key)?
            .map(|row| row.value)
            .unwrap_or_default())
    };
    let interval: u64 = match value(keys::METRICS_INTERVAL)?.trim().parse() {
        Ok(seconds) => seconds,
        Err(_) => {
            tracing::warn!(
                key = keys::METRICS_INTERVAL,
                "not a number of seconds; using the default"
            );
            settings::DEFAULT_METRICS_INTERVAL_SECONDS
        }
    };
    if interval == 0 {
        return Ok(None);
    }
    let dcgm = DcgmSetting::parse(&value(keys::METRICS_DCGM)?).unwrap_or_else(|error| {
        tracing::warn!(key = keys::METRICS_DCGM, %error, "using automatic discovery");
        DcgmSetting::Auto
    });
    Ok(Some(MetricsOptions {
        interval: Duration::from_secs(interval.clamp(2, 300)),
        dcgm,
        sensors: !value(keys::METRICS_SENSORS)?
            .trim()
            .eq_ignore_ascii_case("off"),
    }))
}

/// Start (or restart) the Kubernetes and Talos watches and the metrics loop with the current
/// settings. Summaries arrive as `terrace://cluster`, `terrace://talos` and
/// `terrace://metrics` events, state as `terrace://watch`.
#[tauri::command]
pub async fn watch_start(
    app: tauri::AppHandle,
    generation: u64,
    state: State<'_, AppState>,
) -> Result<()> {
    let kubeconfig = settings::require_path(&state.db, keys::KUBECONFIG_PATH)?;
    let context = settings::resolve(&state.db, keys::KUBE_CONTEXT)?
        .map(|row| row.value)
        .filter(|value| !value.trim().is_empty());
    let talos = talos_cli(&state)?;
    let classifier = classifier(&state)?;
    let metrics = metrics_options(&state)?;
    state.watch.start(
        Arc::new(AppSink { app, generation }),
        WatchSources {
            kubeconfig: PathBuf::from(kubeconfig),
            context,
            classifier,
            talos,
            metrics,
        },
    );
    tracing::info!("watches started");
    Ok(())
}

#[tauri::command]
pub async fn watch_stop(state: State<'_, AppState>) -> Result<()> {
    state.watch.stop();
    tracing::info!("watches stopped");
    Ok(())
}

// --- Ollama ------------------------------------------------------------------

/// Ollama is an optional integration: `None` when no URL is configured, an error only when a
/// configured server cannot be reached.
#[tauri::command]
pub async fn ollama_status(state: State<'_, AppState>) -> Result<Option<OllamaStatus>> {
    let url = settings::resolve(&state.db, keys::OLLAMA_URL)?
        .map(|row| row.value)
        .filter(|value| !value.trim().is_empty());
    match url {
        None => Ok(None),
        Some(url) => OllamaClient::new(&url)?.status().await.map(Some),
    }
}
