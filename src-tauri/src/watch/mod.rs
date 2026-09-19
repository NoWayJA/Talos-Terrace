//! Live watches. Kubernetes objects arrive through kube-runtime reflectors (one watch stream
//! per kind, retried with backoff); Talos changes arrive through `talosctl get --watch` on a
//! few trigger resources and cause the whole inventory to be re-read; metrics are sampled on
//! an interval from the kubelet, a GPU exporter and the hosts' sensors. All push to a
//! [`Sink`], so the desktop app (Tauri events) and the headless probe (stdout) share one
//! path. Everything here is read-only.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::stream::BoxStream;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Event, Namespace, Node, Pod, Service};
use kube::api::DynamicObject;
use kube::discovery::ApiResource;
use kube::runtime::reflector::store::Writer;
use kube::runtime::reflector::{self, Store};
use kube::runtime::{WatchStreamExt, watcher};
use kube::{Api, Client};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::MissedTickBehavior;

use crate::cluster::{self, Classifier, ClusterObjects, ClusterSummary, Target};
use crate::metrics::{Collector, MetricsOptions, MetricsSample};
use crate::talos::{TalosCli, TalosInventory, drain_documents, inventory::affects_inventory};

/// Tauri event names. Mirrored in `services/api.ts`.
pub const EVENT_CLUSTER: &str = "terrace://cluster";
pub const EVENT_TALOS: &str = "terrace://talos";
pub const EVENT_METRICS: &str = "terrace://metrics";
pub const EVENT_STATE: &str = "terrace://watch";
/// Consecutive samples with every source failing before the metrics loop reconnects.
const METRICS_FAILURES_BEFORE_RECONNECT: u32 = 3;

/// Coalesce bursts of Kubernetes changes into one summary.
const CLUSTER_DEBOUNCE: Duration = Duration::from_millis(250);
/// Coalesce Talos triggers into one inventory read (twelve `talosctl get` calls).
const TALOS_DEBOUNCE: Duration = Duration::from_millis(750);
/// A watch starts by replaying `created` for every existing resource. The burst arrives as one
/// run of documents; the first quiet gap ends it, and never later than this.
const TALOS_REPLAY_GAP: Duration = Duration::from_millis(500);
const TALOS_REPLAY_MAX: Duration = Duration::from_secs(10);
/// How long the cluster watch waits for every kind's first list before summarising what it
/// has. A kind that never arrives (forbidden for these credentials, typically) is reported on
/// the badge and the rest of the canvas still fills.
const READY_TIMEOUT: Duration = Duration::from_secs(20);
/// The sample interval can come from any caller; never let it reach zero.
const MIN_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);
const BACKOFF_MIN: Duration = Duration::from_secs(3);
const BACKOFF_MAX: Duration = Duration::from_secs(60);
/// Talos resources whose change means the inventory should be re-read. Machine status covers
/// stage and readiness (an upgrade or reboot); the rest cover disks, volumes, links,
/// addresses and extensions. Hardware facts never change while a node is up.
const TALOS_TRIGGERS: &[&str] = &[
    "machinestatus",
    "volumestatus",
    "disks",
    "links",
    "addresses",
    "extensions",
];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WatchSource {
    Cluster,
    Talos,
    Metrics,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WatchPhase {
    Connecting,
    Live,
    Error,
    /// Deliberately not running (metrics switched off in Settings).
    Stopped,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WatchState {
    pub source: WatchSource,
    pub phase: WatchPhase,
    pub message: Option<String>,
}

impl WatchState {
    fn connecting(source: WatchSource) -> Self {
        Self {
            source,
            phase: WatchPhase::Connecting,
            message: None,
        }
    }

    fn live(source: WatchSource) -> Self {
        Self {
            source,
            phase: WatchPhase::Live,
            message: None,
        }
    }

    fn error(source: WatchSource, message: impl Into<String>) -> Self {
        Self {
            source,
            phase: WatchPhase::Error,
            message: Some(message.into()),
        }
    }

    fn stopped(source: WatchSource, message: impl Into<String>) -> Self {
        Self {
            source,
            phase: WatchPhase::Stopped,
            message: Some(message.into()),
        }
    }
}

/// Where watch output goes.
pub trait Sink: Send + Sync + 'static {
    fn state(&self, state: WatchState);
    fn cluster(&self, summary: &ClusterSummary);
    fn talos(&self, inventory: &TalosInventory);
    fn metrics(&self, sample: &MetricsSample);
}

/// Desktop events carry the connection generation so queued output from a previous
/// configuration cannot be mistaken for the newly selected cluster.
pub struct AppSink {
    pub app: tauri::AppHandle,
    pub generation: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WatchMessage<'a, T> {
    generation: u64,
    payload: &'a T,
}

impl AppSink {
    fn emit<T: Serialize + Clone>(&self, event: &str, payload: &T) -> tauri::Result<()> {
        self.app.emit(
            event,
            WatchMessage {
                generation: self.generation,
                payload,
            },
        )
    }
}

impl Sink for AppSink {
    fn state(&self, state: WatchState) {
        if let Err(error) = self.emit(EVENT_STATE, &state) {
            tracing::warn!(%error, "could not emit watch state");
        }
    }

    fn cluster(&self, summary: &ClusterSummary) {
        if let Err(error) = self.emit(EVENT_CLUSTER, summary) {
            tracing::warn!(%error, "could not emit cluster summary");
        }
    }

    fn talos(&self, inventory: &TalosInventory) {
        if let Err(error) = self.emit(EVENT_TALOS, inventory) {
            tracing::warn!(%error, "could not emit Talos inventory");
        }
    }

    fn metrics(&self, sample: &MetricsSample) {
        if let Err(error) = self.emit(EVENT_METRICS, sample) {
            tracing::warn!(%error, "could not emit metrics sample");
        }
    }
}

/// What to watch: the two config files as they are, nothing copied.
pub struct WatchSources {
    pub kubeconfig: PathBuf,
    pub context: Option<String>,
    pub classifier: Classifier,
    pub talos: TalosCli,
    /// `None` leaves live metrics off.
    pub metrics: Option<MetricsOptions>,
}

/// Owns the running watch tasks. Stopping aborts them; aborting drops the `talosctl`
/// children, which are spawned with `kill_on_drop`.
#[derive(Default)]
pub struct WatchHub {
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl WatchHub {
    /// Start (or restart) both watches. Must be called from within a Tokio runtime. The lock
    /// is held from the abort to the assignment, so two starts arriving together cannot leave
    /// one set of tasks running unowned.
    pub fn start(&self, sink: Arc<dyn Sink>, sources: WatchSources) {
        let mut tasks = self.tasks.lock();
        for task in tasks.drain(..) {
            task.abort();
        }
        let talos = Arc::new(sources.talos);
        tasks.push(tokio::spawn(run_cluster_watch(
            Arc::clone(&sink),
            sources.kubeconfig.clone(),
            sources.context.clone(),
            sources.classifier,
        )));
        tasks.push(tokio::spawn(run_talos_watch(
            Arc::clone(&sink),
            Arc::clone(&talos),
        )));
        match sources.metrics {
            Some(options) => tasks.push(tokio::spawn(run_metrics_watch(
                sink,
                sources.kubeconfig,
                sources.context,
                talos,
                options,
            ))),
            None => sink.state(WatchState::stopped(
                WatchSource::Metrics,
                "switched off in Settings (sample interval 0)",
            )),
        }
    }

    pub fn stop(&self) {
        for task in self.tasks.lock().drain(..) {
            task.abort();
        }
    }

    pub fn is_running(&self) -> bool {
        self.tasks.lock().iter().any(|task| !task.is_finished())
    }
}

impl Drop for WatchHub {
    fn drop(&mut self) {
        self.stop();
    }
}

fn next_backoff(current: Duration) -> Duration {
    (current * 2).min(BACKOFF_MAX)
}

// --- Kubernetes ----------------------------------------------------------------

async fn run_cluster_watch(
    sink: Arc<dyn Sink>,
    kubeconfig: PathBuf,
    context: Option<String>,
    classifier: Classifier,
) {
    let mut backoff = BACKOFF_MIN;
    loop {
        sink.state(WatchState::connecting(WatchSource::Cluster));
        let message = match cluster::connect(&kubeconfig, context.as_deref()).await {
            Ok(connection) => {
                backoff = BACKOFF_MIN;
                watch_cluster(&sink, connection, &classifier).await
            }
            Err(error) => error.to_string(),
        };
        tracing::warn!(message, retry_in = ?backoff, "cluster watch stopped");
        sink.state(WatchState::error(WatchSource::Cluster, message));
        tokio::time::sleep(backoff).await;
        backoff = next_backoff(backoff);
    }
}

/// Kinds whose stream is currently failing, shared by every driver so the Cluster badge
/// goes back to live only when the last of them recovers.
type Degraded = Arc<Mutex<BTreeSet<String>>>;

/// Wait for a store's first list until the shared deadline; a store still empty by then is
/// named and the watch goes on without it.
async fn await_ready<K>(
    kind: &str,
    store: &Store<K>,
    deadline: tokio::time::Instant,
    late: &mut Vec<String>,
) -> std::result::Result<(), String>
where
    K: kube::Resource + Clone + 'static,
    K::DynamicType: std::hash::Hash + Eq + Clone,
{
    match tokio::time::timeout_at(deadline, store.wait_until_ready()).await {
        Ok(result) => result.map_err(|error| format!("watch not ready: {error}")),
        Err(_) => {
            late.push(kind.to_string());
            Ok(())
        }
    }
}

/// Reflect the kinds the summary needs (plus VMs when KubeVirt is served) and re-summarise
/// on every change. Returns the reason once the watch cannot go on.
async fn watch_cluster(
    sink: &Arc<dyn Sink>,
    connection: cluster::Connection,
    classifier: &Classifier,
) -> String {
    match watch_cluster_inner(sink, connection, classifier).await {
        Ok(()) => "the cluster watch ended".to_string(),
        Err(message) => message,
    }
}

async fn watch_cluster_inner(
    sink: &Arc<dyn Sink>,
    connection: cluster::Connection,
    classifier: &Classifier,
) -> std::result::Result<(), String> {
    let notify = Arc::new(Notify::new());
    let degraded: Degraded = Arc::default();
    let mut drivers = JoinSet::new();
    let client = &connection.client;

    let nodes = reflect::<Node>(client, &notify, &degraded, &mut drivers, sink);
    let namespaces = reflect::<Namespace>(client, &notify, &degraded, &mut drivers, sink);
    let deployments = reflect::<Deployment>(client, &notify, &degraded, &mut drivers, sink);
    let pods = reflect::<Pod>(client, &notify, &degraded, &mut drivers, sink);
    let services = reflect::<Service>(client, &notify, &degraded, &mut drivers, sink);
    let events = reflect::<Event>(client, &notify, &degraded, &mut drivers, sink);
    let vms = connection
        .kubevirt
        .as_ref()
        .map(|api| reflect_dynamic(client, &api.vm, &notify, &degraded, &mut drivers, sink));
    let vmis = connection
        .kubevirt
        .as_ref()
        .map(|api| reflect_dynamic(client, &api.vmi, &notify, &degraded, &mut drivers, sink));

    let ready = async {
        let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
        let mut late = Vec::new();
        await_ready("nodes", &nodes, deadline, &mut late).await?;
        await_ready("namespaces", &namespaces, deadline, &mut late).await?;
        await_ready("deployments", &deployments, deadline, &mut late).await?;
        await_ready("pods", &pods, deadline, &mut late).await?;
        await_ready("services", &services, deadline, &mut late).await?;
        await_ready("events", &events, deadline, &mut late).await?;
        if let Some(store) = &vms {
            await_ready("virtual machines", store, deadline, &mut late).await?;
        }
        if let Some(store) = &vmis {
            await_ready("virtual machine instances", store, deadline, &mut late).await?;
        }
        Ok::<Vec<String>, String>(late)
    };
    let late = tokio::select! {
        result = ready => result?,
        Some(result) = drivers.join_next() => return Err(driver_ended(result)),
    };
    if !late.is_empty() {
        tracing::warn!(kinds = ?late, "some kinds did not list in time; showing the rest");
        sink.state(WatchState::error(
            WatchSource::Cluster,
            format!("waiting for initial lists: {}", late.join(", ")),
        ));
    }

    let target: Target = connection.target;
    let summarise = || {
        let objects = ClusterObjects {
            nodes: nodes.state(),
            namespaces: namespaces.state(),
            deployments: deployments.state(),
            pods: pods.state(),
            services: services.state(),
            events: events.state(),
            vms: vms.as_ref().map(Store::state).unwrap_or_default(),
            vmis: vmis.as_ref().map(Store::state).unwrap_or_default(),
        };
        cluster::summarise(&target, &objects, classifier)
    };

    if degraded.lock().is_empty() {
        sink.state(WatchState::live(WatchSource::Cluster));
    }
    sink.cluster(&summarise());

    loop {
        tokio::select! {
            () = notify.notified() => {
                tokio::time::sleep(CLUSTER_DEBOUNCE).await;
                sink.cluster(&summarise());
            }
            Some(result) = drivers.join_next() => return Err(driver_ended(result)),
        }
    }
}

fn driver_ended(result: std::result::Result<(), tokio::task::JoinError>) -> String {
    match result {
        Ok(()) => "a watch stream ended".to_string(),
        Err(error) => format!("a watch task failed: {error}"),
    }
}

/// One reflector for a typed kind: a watch stream with default backoff feeding a store.
fn reflect<K>(
    client: &Client,
    notify: &Arc<Notify>,
    degraded: &Degraded,
    drivers: &mut JoinSet<()>,
    sink: &Arc<dyn Sink>,
) -> Store<K>
where
    K: kube::Resource<DynamicType = ()>
        + Clone
        + serde::de::DeserializeOwned
        + Debug
        + Send
        + Sync
        + 'static,
{
    let kind = K::kind(&()).to_string();
    degraded.lock().insert(kind.clone());
    let api = Api::<K>::all(client.clone());
    let (store, writer) = reflector::store();
    let stream = reflector::reflector(
        writer,
        watcher(api, watcher::Config::default()).default_backoff(),
    )
    .boxed();
    drivers.spawn(drive(
        stream,
        kind,
        Arc::clone(notify),
        Arc::clone(degraded),
        Arc::clone(sink),
    ));
    store
}

/// The same for a custom resource known only by its `ApiResource` (KubeVirt's kinds).
fn reflect_dynamic(
    client: &Client,
    resource: &ApiResource,
    notify: &Arc<Notify>,
    degraded: &Degraded,
    drivers: &mut JoinSet<()>,
    sink: &Arc<dyn Sink>,
) -> Store<DynamicObject> {
    let kind = resource.kind.clone();
    degraded.lock().insert(kind.clone());
    let api = Api::<DynamicObject>::all_with(client.clone(), resource);
    let writer = Writer::<DynamicObject>::new(resource.clone());
    let store = writer.as_reader();
    let stream = reflector::reflector(
        writer,
        watcher(api, watcher::Config::default()).default_backoff(),
    )
    .boxed();
    drivers.spawn(drive(
        stream,
        kind,
        Arc::clone(notify),
        Arc::clone(degraded),
        Arc::clone(sink),
    ));
    store
}

/// Every applied or deleted object pokes `notify`; stream errors are reported and retried by
/// kube-runtime, and the source goes back to live once no kind is failing any more.
async fn drive<K: Send + 'static>(
    mut stream: BoxStream<'static, std::result::Result<watcher::Event<K>, watcher::Error>>,
    kind: String,
    notify: Arc<Notify>,
    degraded: Degraded,
    sink: Arc<dyn Sink>,
) {
    while let Some(item) = stream.next().await {
        match item {
            // Reflector stores swap their complete list on InitDone, including an empty
            // relist after objects disappeared during an outage. touched_objects drops that
            // event, leaving stale cards and a failed badge until another object changes.
            Ok(watcher::Event::Apply(_) | watcher::Event::Delete(_) | watcher::Event::InitDone) => {
                let recovered = {
                    let mut failing = degraded.lock();
                    failing.remove(&kind) && failing.is_empty()
                };
                if recovered {
                    sink.state(WatchState::live(WatchSource::Cluster));
                }
                notify.notify_one();
            }
            Ok(watcher::Event::Init | watcher::Event::InitApply(_)) => {}
            Err(error) => {
                degraded.lock().insert(kind.clone());
                tracing::warn!(kind, %error, "watch stream error; kube-runtime retries");
                sink.state(WatchState::error(
                    WatchSource::Cluster,
                    format!("{kind}: {error}"),
                ));
            }
        }
    }
}

// --- Talos ---------------------------------------------------------------------

async fn run_talos_watch(sink: Arc<dyn Sink>, cli: Arc<TalosCli>) {
    let mut backoff = BACKOFF_MIN;
    loop {
        sink.state(WatchState::connecting(WatchSource::Talos));
        let message = match cli.inventory().await {
            Ok(inventory) => {
                backoff = BACKOFF_MIN;
                sink.talos(&inventory);
                watch_talos(&sink, &cli).await
            }
            Err(error) => error.to_string(),
        };
        tracing::warn!(message, retry_in = ?backoff, "Talos watch stopped");
        sink.state(WatchState::error(WatchSource::Talos, message));
        tokio::time::sleep(backoff).await;
        backoff = next_backoff(backoff);
    }
}

/// Run one `talosctl get --watch` per trigger resource; any change re-reads the inventory.
/// Returns the reason when a watcher dies, so the caller can back off and start again.
async fn watch_talos(sink: &Arc<dyn Sink>, cli: &Arc<TalosCli>) -> String {
    let notify = Arc::new(Notify::new());
    let mut watchers = JoinSet::new();
    for resource in TALOS_TRIGGERS {
        watchers.spawn(watch_resource(
            Arc::clone(cli),
            resource,
            Arc::clone(&notify),
        ));
    }
    sink.state(WatchState::live(WatchSource::Talos));

    loop {
        tokio::select! {
            () = notify.notified() => {
                tokio::time::sleep(TALOS_DEBOUNCE).await;
                match cli.inventory().await {
                    Ok(inventory) => sink.talos(&inventory),
                    Err(error) => return error.to_string(),
                }
            }
            Some(result) = watchers.join_next() => {
                return match result {
                    Ok(Ok(())) => "talosctl watch ended".to_string(),
                    Ok(Err(message)) => message,
                    Err(error) => format!("watch task failed: {error}"),
                };
            }
        }
    }
}

async fn watch_resource(
    cli: Arc<TalosCli>,
    resource: &'static str,
    notify: Arc<Notify>,
) -> std::result::Result<(), String> {
    let mut child = cli
        .spawn_watch(resource)
        .map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "talosctl stdout not captured".to_string())?;
    // stderr is drained as it comes, keeping the last line for the exit message: a pipe
    // nobody reads fills up, and a talosctl blocked on it would stall stdout too.
    let last_stderr = Arc::new(Mutex::new(String::new()));
    let stderr_reader = child.stderr.take().map(|pipe| {
        let last = Arc::clone(&last_stderr);
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    *last.lock() = line;
                }
            }
        })
    });
    let started = Instant::now();
    let mut previous = started;
    let mut replaying = true;
    let mut lines = BufReader::new(stdout).lines();
    let mut buffer = String::new();

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|error| format!("reading talosctl get {resource} --watch: {error}"))?
    {
        buffer.push_str(&line);
        buffer.push('\n');
        for event in drain_documents(&mut buffer) {
            let now = Instant::now();
            if replaying
                && (now.duration_since(previous) > TALOS_REPLAY_GAP
                    || now.duration_since(started) > TALOS_REPLAY_MAX)
            {
                replaying = false;
            }
            previous = now;
            let replay = replaying && event.event == "created";
            if replay || !affects_inventory(&event.resource) {
                continue;
            }
            tracing::debug!(
                resource,
                event = event.event,
                id = event.resource.id,
                "Talos resource changed"
            );
            notify.notify_one();
        }
    }

    // stdout closed: the process is ending. Say why, from its last stderr line.
    if let Some(task) = stderr_reader {
        let _ = task.await;
    }
    let status = child
        .wait()
        .await
        .map_err(|error| format!("waiting for talosctl: {error}"))?;
    let reason = last_stderr.lock().trim().to_string();
    Err(format!(
        "talosctl get {resource} --watch exited ({status}): {reason}"
    ))
}

// --- Metrics -------------------------------------------------------------------

/// Sample on the interval for as long as the connection serves; reconnect with backoff when
/// it stops (or when every source fails several times in a row).
async fn run_metrics_watch(
    sink: Arc<dyn Sink>,
    kubeconfig: PathBuf,
    context: Option<String>,
    talos: Arc<TalosCli>,
    options: MetricsOptions,
) {
    let mut backoff = BACKOFF_MIN;
    loop {
        sink.state(WatchState::connecting(WatchSource::Metrics));
        let message = match cluster::connect(&kubeconfig, context.as_deref()).await {
            Ok(connection) => {
                backoff = BACKOFF_MIN;
                let mut collector =
                    Collector::new(connection.client, Arc::clone(&talos), options.clone());
                sample_loop(&sink, &mut collector, options.interval).await
            }
            Err(error) => error.to_string(),
        };
        tracing::warn!(message, retry_in = ?backoff, "metrics loop stopped");
        sink.state(WatchState::error(WatchSource::Metrics, message));
        tokio::time::sleep(backoff).await;
        backoff = next_backoff(backoff);
    }
}

/// Returns the reason once every source has failed [`METRICS_FAILURES_BEFORE_RECONNECT`]
/// samples running; a partial answer keeps going and is pushed as it is.
async fn sample_loop(
    sink: &Arc<dyn Sink>,
    collector: &mut Collector,
    interval: Duration,
) -> String {
    let mut ticker = tokio::time::interval(interval.max(MIN_SAMPLE_INTERVAL));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut failures = 0u32;
    let mut live = false;
    loop {
        ticker.tick().await;
        let sample = collector.sample().await;
        let all_failed = !sample.sources.is_empty() && sample.sources.iter().all(|s| !s.ok);
        if all_failed {
            failures += 1;
            live = false;
            let reason = sample
                .sources
                .iter()
                .filter_map(|s| s.message.clone())
                .next()
                .unwrap_or_else(|| "no source answered".to_string());
            sink.state(WatchState::error(WatchSource::Metrics, reason.clone()));
            if failures >= METRICS_FAILURES_BEFORE_RECONNECT {
                return reason;
            }
        } else {
            failures = 0;
            if !live {
                live = true;
                sink.state(WatchState::live(WatchSource::Metrics));
            }
        }
        sink.metrics(&sample);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_state_serialises_in_lowercase_for_the_front_end() {
        let state = WatchState::error(WatchSource::Talos, "gone");
        let json = serde_json::to_value(&state).expect("serialise");
        assert_eq!(json["source"], "talos");
        assert_eq!(json["phase"], "error");
        assert_eq!(json["message"], "gone");
        let live = serde_json::to_value(WatchState::live(WatchSource::Cluster)).expect("json");
        assert_eq!(live["phase"], "live");
        assert!(live["message"].is_null());
        let metrics =
            serde_json::to_value(WatchState::connecting(WatchSource::Metrics)).expect("json");
        assert_eq!(metrics["source"], "metrics");
    }

    #[derive(Default)]
    struct RecordingSink(Mutex<Vec<WatchState>>);

    impl Sink for RecordingSink {
        fn state(&self, state: WatchState) {
            self.0.lock().push(state);
        }
        fn cluster(&self, _: &ClusterSummary) {}
        fn talos(&self, _: &TalosInventory) {}
        fn metrics(&self, _: &MetricsSample) {}
    }

    #[tokio::test]
    async fn an_empty_relist_clears_the_store_and_recovers_the_badge() {
        use futures::FutureExt;
        let (store, writer) = reflector::store::<Pod>();
        let notify = Arc::new(Notify::new());
        let degraded = Arc::new(Mutex::new(BTreeSet::from(["Pod".into()])));
        let sink = Arc::new(RecordingSink::default());
        let events = vec![
            Ok(watcher::Event::Init),
            Ok(watcher::Event::InitApply(Pod {
                metadata: kube::core::ObjectMeta {
                    name: Some("old-pod".into()),
                    namespace: Some("apps".into()),
                    ..Default::default()
                },
                ..Default::default()
            })),
            Ok(watcher::Event::InitDone),
            Ok(watcher::Event::Init),
            Ok(watcher::Event::InitDone),
        ];
        drive(
            reflector::reflector(writer, futures::stream::iter(events)).boxed(),
            "Pod".into(),
            Arc::clone(&notify),
            Arc::clone(&degraded),
            sink.clone(),
        )
        .await;
        assert!(store.state().is_empty());
        assert!(degraded.lock().is_empty());
        assert!(notify.notified().now_or_never().is_some());
        assert_eq!(sink.0.lock().last().expect("state").phase, WatchPhase::Live);
    }

    #[test]
    fn backoff_doubles_and_caps() {
        assert_eq!(next_backoff(BACKOFF_MIN), BACKOFF_MIN * 2);
        assert_eq!(next_backoff(BACKOFF_MAX), BACKOFF_MAX);
        assert_eq!(next_backoff(Duration::from_secs(48)), BACKOFF_MAX);
    }

    #[tokio::test]
    async fn a_hub_without_tasks_is_not_running_and_stop_is_idempotent() {
        let hub = WatchHub::default();
        assert!(!hub.is_running());
        hub.stop();
        hub.stop();
        assert!(!hub.is_running());
    }
}
