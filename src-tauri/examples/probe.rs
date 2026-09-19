//! Headless probe: print what Terrace reads from your cluster, as JSON, without launching the
//! desktop app. Read-only. Useful for checking a new Talos or Kubernetes version, a cluster
//! with several nodes, or a bug report.
//!
//! ```text
//! cargo run --example probe -- [--kubeconfig PATH] [--context NAME] [--talosconfig PATH]
//!                              [--talosctl BIN] [--watch SECONDS]
//! ```
//!
//! Defaults follow the app's settings defaults: `~/.kube/config` and its current context,
//! `$TALOSCONFIG` or `~/.talos/config`, `talosctl` on `PATH`. One metrics sample is taken
//! too (the kubelet summary, a DCGM exporter if one is found, the hosts' sensors). With
//! `--watch N` the live path runs for N seconds and prints every summary and sample it would
//! push to the canvas.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use terrace_lib::cluster::{self, Classifier, ClusterSummary};
use terrace_lib::metrics::{self, MetricsOptions, MetricsSample};
use terrace_lib::settings::{self, keys};
use terrace_lib::talos::{TalosCli, TalosInventory};
use terrace_lib::watch::{Sink, WatchHub, WatchSources, WatchState};

struct Options {
    kubeconfig: PathBuf,
    context: Option<String>,
    talosconfig: PathBuf,
    talosctl: String,
    watch: Option<u64>,
}

const USAGE: &str = "usage: probe [--kubeconfig PATH] [--context NAME] [--talosconfig PATH] [--talosctl BIN] [--watch SECONDS]";

fn parse_args() -> Result<Options, String> {
    let mut options = Options {
        kubeconfig: settings::default_for(keys::KUBECONFIG_PATH)
            .map(PathBuf::from)
            .unwrap_or_default(),
        context: None,
        talosconfig: settings::default_for(keys::TALOSCONFIG_PATH)
            .map(PathBuf::from)
            .unwrap_or_default(),
        talosctl: settings::default_for(keys::TALOSCTL_PATH).unwrap_or_default(),
        watch: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--kubeconfig" => options.kubeconfig = PathBuf::from(value()?),
            "--context" => options.context = Some(value()?),
            "--talosconfig" => options.talosconfig = PathBuf::from(value()?),
            "--talosctl" => options.talosctl = value()?,
            "--watch" => {
                options.watch = Some(
                    value()?
                        .parse()
                        .map_err(|_| "--watch needs a number of seconds".to_string())?,
                )
            }
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}\n{USAGE}")),
        }
    }
    Ok(options)
}

/// Prints every push as one JSON line so the output can be piped through `jq`.
struct PrintSink;

impl Sink for PrintSink {
    fn state(&self, state: WatchState) {
        eprintln!(
            "[watch] {:?} {:?}{}",
            state.source,
            state.phase,
            state
                .message
                .map(|message| format!(": {message}"))
                .unwrap_or_default()
        );
    }

    fn cluster(&self, summary: &ClusterSummary) {
        print_json("cluster", summary);
    }

    fn talos(&self, inventory: &TalosInventory) {
        print_json("talos", inventory);
    }

    fn metrics(&self, sample: &MetricsSample) {
        print_json("metrics", sample);
    }
}

fn print_json<T: serde::Serialize>(label: &str, value: &T) {
    match serde_json::to_string(&serde_json::json!({ label: value })) {
        Ok(json) => println!("{json}"),
        Err(error) => eprintln!("cannot serialise {label}: {error}"),
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    terrace_lib::init_crypto();
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let mut failures = 0u8;

    eprintln!(
        "[probe] Kubernetes: {} (context {})",
        options.kubeconfig.display(),
        options.context.as_deref().unwrap_or("current")
    );
    match cluster::summary(
        &options.kubeconfig,
        options.context.as_deref(),
        &Classifier::default(),
    )
    .await
    {
        Ok(summary) => print_json("cluster", &summary),
        Err(error) => {
            failures += 1;
            eprintln!("[probe] Kubernetes failed: {error}");
        }
    }

    eprintln!(
        "[probe] Talos: {} via {}",
        options.talosconfig.display(),
        options.talosctl
    );
    let cli = TalosCli::new(options.talosctl.clone(), options.talosconfig.clone());
    match cli.inventory().await {
        Ok(inventory) => print_json("talos", &inventory),
        Err(error) => {
            failures += 1;
            eprintln!("[probe] Talos failed: {error}");
        }
    }

    eprintln!("[probe] metrics: one sample (kubelet summary, DCGM exporter if found, sensors)");
    let talos = Arc::new(TalosCli::new(
        options.talosctl.clone(),
        options.talosconfig.clone(),
    ));
    match metrics::probe_once(
        &options.kubeconfig,
        options.context.as_deref(),
        talos,
        MetricsOptions::default(),
    )
    .await
    {
        Ok(sample) => print_json("metrics", &sample),
        Err(error) => {
            failures += 1;
            eprintln!("[probe] metrics failed: {error}");
        }
    }

    if let Some(seconds) = options.watch {
        eprintln!("[probe] watching for {seconds} s");
        let hub = WatchHub::default();
        hub.start(
            Arc::new(PrintSink),
            WatchSources {
                kubeconfig: options.kubeconfig,
                context: options.context,
                classifier: Classifier::default(),
                talos: TalosCli::new(options.talosctl, options.talosconfig),
                metrics: Some(MetricsOptions::default()),
            },
        );
        tokio::time::sleep(Duration::from_secs(seconds)).await;
        hub.stop();
        eprintln!("[probe] watch stopped");
    }

    if failures == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(failures)
    }
}
