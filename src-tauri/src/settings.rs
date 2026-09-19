//! Settings: the known keys, their defaults, and how a stored value or a default is chosen.
//! Values are stored as plain strings, never JSON-quoted (a Modz lesson: quoted values leaked into paths).
//! Paths are settings, secrets are not: anything secret goes through the vault.

use serde::Serialize;

use crate::db::Database;
use crate::error::{Result, TerraceError};

pub mod keys {
    pub const THEME: &str = "theme";
    pub const KUBECONFIG_PATH: &str = "kubeconfig.path";
    pub const KUBE_CONTEXT: &str = "kubeconfig.context";
    pub const TALOSCONFIG_PATH: &str = "talosconfig.path";
    pub const TALOSCTL_PATH: &str = "talosctl.path";
    pub const OLLAMA_URL: &str = "ollama.url";
    /// Namespaces forced to count as the user's apps (comma separated).
    pub const WORKLOADS_APPS: &str = "workloads.apps";
    /// Namespaces forced to count as platform machinery (comma separated).
    pub const WORKLOADS_PLATFORM: &str = "workloads.platform";
    /// Seconds between metric samples; `0` switches live metrics off.
    pub const METRICS_INTERVAL: &str = "metrics.interval";
    /// The DCGM exporter Service as `namespace/name[:port]`; empty finds one by name, `off`
    /// disables GPU metrics.
    pub const METRICS_DCGM: &str = "metrics.dcgm.service";
    /// `on` or `off`: read the hosts' temperature sensors through the Talos API.
    pub const METRICS_SENSORS: &str = "metrics.sensors";
    /// A power budget in watts the GPU power draw is shown against; empty shows draw alone.
    pub const POWER_BUDGET_WATTS: &str = "power.budget.watts";
}

/// Every key the UI knows about, in display order. Mirrored in stores/useSettingsStore.ts.
pub const KNOWN_KEYS: &[&str] = &[
    keys::THEME,
    keys::KUBECONFIG_PATH,
    keys::KUBE_CONTEXT,
    keys::TALOSCONFIG_PATH,
    keys::TALOSCTL_PATH,
    keys::OLLAMA_URL,
    keys::WORKLOADS_APPS,
    keys::WORKLOADS_PLATFORM,
    keys::METRICS_INTERVAL,
    keys::METRICS_DCGM,
    keys::METRICS_SENSORS,
    keys::POWER_BUDGET_WATTS,
];

/// Metric samples every this many seconds unless a setting says otherwise.
pub const DEFAULT_METRICS_INTERVAL_SECONDS: u64 = 5;

/// A placeholder for the UI only. Ollama is an optional integration, off until a URL is set.
pub const SUGGESTED_OLLAMA_URL: &str = "http://localhost:11434";

fn home_join(parts: &[&str]) -> Option<String> {
    let mut path = dirs::home_dir()?;
    for part in parts {
        path.push(part);
    }
    Some(path.display().to_string())
}

/// The built-in default for a key, if it has one. Empty string means "unset, and that is fine".
pub fn default_for(key: &str) -> Option<String> {
    match key {
        keys::THEME => Some("system".to_string()),
        keys::KUBECONFIG_PATH => home_join(&[".kube", "config"]),
        keys::KUBE_CONTEXT => Some(String::new()),
        keys::TALOSCONFIG_PATH => std::env::var("TALOSCONFIG")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| home_join(&[".talos", "config"])),
        keys::TALOSCTL_PATH => Some("talosctl".to_string()),
        keys::OLLAMA_URL => Some(String::new()),
        keys::WORKLOADS_APPS | keys::WORKLOADS_PLATFORM => Some(String::new()),
        keys::METRICS_INTERVAL => Some(DEFAULT_METRICS_INTERVAL_SECONDS.to_string()),
        keys::METRICS_DCGM | keys::POWER_BUDGET_WATTS => Some(String::new()),
        keys::METRICS_SENSORS => Some("on".to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub is_default: bool,
}

/// A stored, non-empty value wins; otherwise the default.
pub fn resolve(db: &Database, key: &str) -> Result<Option<SettingRow>> {
    match db.get_setting(key)? {
        Some(value) if !value.trim().is_empty() => Ok(Some(SettingRow {
            key: key.to_string(),
            value,
            is_default: false,
        })),
        _ => Ok(default_for(key).map(|value| SettingRow {
            key: key.to_string(),
            value,
            is_default: true,
        })),
    }
}

/// `~` and `~/...` as the shell would read them; anything else unchanged.
pub fn expand_home(value: &str) -> String {
    let trimmed = value.trim();
    if (trimmed == "~" || trimmed.starts_with("~/"))
        && let Some(home) = dirs::home_dir()
    {
        return match trimmed.strip_prefix("~/") {
            Some(rest) => home.join(rest).display().to_string(),
            None => home.display().to_string(),
        };
    }
    trimmed.to_string()
}

/// A path setting: required, with `~` expanded, because a desktop app has no shell to do it.
pub fn require_path(db: &Database, key: &str) -> Result<String> {
    require(db, key).map(|value| expand_home(&value))
}

/// Like `resolve`, but a missing or empty value is a configuration error naming the key.
pub fn require(db: &Database, key: &str) -> Result<String> {
    match resolve(db, key)? {
        Some(row) if !row.value.trim().is_empty() => Ok(row.value),
        _ => Err(TerraceError::Config(format!("setting {key} is not set"))),
    }
}

/// Every known key resolved, then any stored key the UI does not know about.
pub fn all(db: &Database) -> Result<Vec<SettingRow>> {
    let mut rows = Vec::with_capacity(KNOWN_KEYS.len());
    for key in KNOWN_KEYS {
        if let Some(row) = resolve(db, key)? {
            rows.push(row);
        }
    }
    for (key, value) in db.all_settings()? {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            rows.push(SettingRow {
                key,
                value,
                is_default: false,
            });
        }
    }
    Ok(rows)
}

/// Store a value; an empty value deletes the row so the default shows again.
pub fn set(db: &Database, key: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        db.delete_setting(key)
    } else {
        match key {
            keys::OLLAMA_URL => crate::ollama::validate_url(value)?,
            keys::METRICS_DCGM => {
                crate::metrics::DcgmSetting::parse(value)?;
            }
            keys::METRICS_INTERVAL => {
                let seconds = value.trim().parse::<u64>().map_err(|_| {
                    TerraceError::Config("metrics.interval must be 0 or 2–300 whole seconds".into())
                })?;
                if seconds != 0 && !(2..=300).contains(&seconds) {
                    return Err(TerraceError::Config(
                        "metrics.interval must be 0 or 2–300 whole seconds".into(),
                    ));
                }
            }
            keys::METRICS_SENSORS if !matches!(value.trim(), "on" | "off") => {
                return Err(TerraceError::Config(
                    "metrics.sensors must be on or off".into(),
                ));
            }
            keys::POWER_BUDGET_WATTS => {
                let watts = value.trim().parse::<f64>().map_err(|_| {
                    TerraceError::Config("power.budget.watts must be a positive number".into())
                })?;
                if !watts.is_finite() || watts <= 0.0 {
                    return Err(TerraceError::Config(
                        "power.budget.watts must be a positive number".into(),
                    ));
                }
            }
            _ => {}
        }
        db.set_setting(key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_a_leading_tilde_only() {
        let home = dirs::home_dir().expect("home directory");
        assert_eq!(
            expand_home("~/.kube/config"),
            home.join(".kube/config").display().to_string()
        );
        assert_eq!(expand_home("~"), home.display().to_string());
        assert_eq!(expand_home("/etc/talos"), "/etc/talos");
        assert_eq!(expand_home("talosctl"), "talosctl");
        assert_eq!(expand_home("a~/b"), "a~/b");
    }

    #[test]
    fn stored_value_beats_default_and_empty_falls_back() {
        let db = Database::open_in_memory().expect("db");
        let row = resolve(&db, keys::TALOSCTL_PATH)
            .expect("resolve")
            .expect("row");
        assert!(row.is_default);
        assert_eq!(row.value, "talosctl");

        set(&db, keys::TALOSCTL_PATH, "/opt/bin/talosctl").expect("set");
        let row = resolve(&db, keys::TALOSCTL_PATH)
            .expect("resolve")
            .expect("row");
        assert!(!row.is_default);
        assert_eq!(row.value, "/opt/bin/talosctl");

        set(&db, keys::TALOSCTL_PATH, "   ").expect("clear");
        let row = resolve(&db, keys::TALOSCTL_PATH)
            .expect("resolve")
            .expect("row");
        assert!(row.is_default);
    }

    #[test]
    fn optional_integrations_default_to_unset() {
        let db = Database::open_in_memory().expect("db");
        let row = resolve(&db, keys::OLLAMA_URL)
            .expect("resolve")
            .expect("row");
        assert!(row.is_default);
        assert!(row.value.is_empty());
        assert!(require(&db, keys::OLLAMA_URL).is_err());
        assert!(!SUGGESTED_OLLAMA_URL.is_empty());
    }

    #[test]
    fn metrics_defaults_are_on_and_the_budget_is_unset() {
        let db = Database::open_in_memory().expect("db");
        let interval = resolve(&db, keys::METRICS_INTERVAL)
            .expect("resolve")
            .expect("row");
        assert_eq!(interval.value, "5");
        let sensors = resolve(&db, keys::METRICS_SENSORS)
            .expect("resolve")
            .expect("row");
        assert_eq!(sensors.value, "on");
        let dcgm = resolve(&db, keys::METRICS_DCGM)
            .expect("resolve")
            .expect("row");
        assert!(dcgm.value.is_empty() && dcgm.is_default);
        assert!(require(&db, keys::POWER_BUDGET_WATTS).is_err());
    }

    #[test]
    fn invalid_metrics_settings_are_rejected_before_storage() {
        let db = Database::open_in_memory().expect("db");
        for (key, value) in [
            (keys::METRICS_INTERVAL, "1"),
            (keys::METRICS_INTERVAL, "301"),
            (keys::METRICS_DCGM, "a/b:0"),
            (keys::METRICS_SENSORS, "yes"),
            (keys::POWER_BUDGET_WATTS, "100W"),
            (keys::POWER_BUDGET_WATTS, "NaN"),
        ] {
            assert!(set(&db, key, value).is_err());
            assert!(db.get_setting(key).expect("read").is_none());
        }
        for interval in ["0", "2", "300"] {
            set(&db, keys::METRICS_INTERVAL, interval).expect("valid interval");
        }
    }

    #[test]
    fn credentials_in_an_ollama_url_never_reach_the_database() {
        let db = Database::open_in_memory().expect("db");
        for value in [
            "http://user:fixture-secret@localhost:11434",
            "http://localhost:11434?token=fixture-secret",
        ] {
            let error = set(&db, keys::OLLAMA_URL, value).expect_err("unsafe URL");
            assert!(!error.to_string().contains("fixture-secret"));
            assert!(db.get_setting(keys::OLLAMA_URL).expect("read").is_none());
        }
        set(&db, keys::OLLAMA_URL, "http://localhost:11434").expect("valid URL");
    }

    #[test]
    fn require_names_the_missing_key() {
        let db = Database::open_in_memory().expect("db");
        let error = require(&db, "no.such.key").expect_err("missing");
        assert!(error.to_string().contains("no.such.key"));
    }

    #[test]
    fn all_lists_known_keys_first_then_extras() {
        let db = Database::open_in_memory().expect("db");
        set(&db, "experimental.flag", "on").expect("set");
        let rows = all(&db).expect("all");
        assert_eq!(rows[0].key, keys::THEME);
        assert_eq!(
            rows.last().map(|row| row.key.as_str()),
            Some("experimental.flag")
        );
    }
}
