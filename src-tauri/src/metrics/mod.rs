//! Live metrics from what a cluster already serves, with nothing installed on it: the
//! kubelet's summary API through the API server's node proxy (node and pod CPU, memory,
//! filesystem, network), an NVIDIA DCGM exporter when one is running (GPU utilisation,
//! memory, temperature, power, clocks; read through the pod proxy, one exporter pod per node,
//! so every node's GPUs arrive every sample), and the hosts' temperature sensors through
//! Talos's file read API. Sampled on an interval; every sample says which
//! sources answered, so a cluster without a GPU exporter simply has no GPU dials. Read-only.

pub mod kubelet;
pub mod prometheus;
pub mod sensors;

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use k8s_openapi::api::core::v1::{Node, Service};
use k8s_openapi::api::discovery::v1::EndpointSlice;
use kube::api::ListParams;
use kube::{Api, Client};
use serde::Serialize;

use crate::cluster;
use crate::error::{Result, TerraceError};
use crate::talos::TalosCli;

/// The Service name the NVIDIA GPU Operator gives its DCGM exporter.
pub const DCGM_SERVICE_NAME: &str = "nvidia-dcgm-exporter";
const DCGM_DEFAULT_PORT: u16 = 9400;
const DEFAULT_INTERVAL: Duration = Duration::from_secs(5);
/// Sensors are read every this many samples: one process per file, and temperatures move
/// slowly.
const SENSOR_EVERY: u32 = 3;
/// The exporter Service is looked for again every this many samples; it may arrive later.
const DISCOVER_EVERY: u32 = 24;
/// How many `/sys/class/hwmon/hwmonN` slots are probed, once per node.
const HWMON_SLOTS: u32 = 8;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MetricsSource {
    Kubelet,
    Dcgm,
    Sensors,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSourceState {
    pub source: MetricsSource,
    pub ok: bool,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Temperature {
    /// The hwmon driver (`k10temp`, `nvme`).
    pub sensor: String,
    /// Plain words (`CPU`, `NVMe 1`).
    pub label: String,
    pub celsius: f64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NodeMetrics {
    pub node: String,
    pub cpu_cores: Option<f64>,
    pub cpu_capacity_cores: Option<f64>,
    /// Resident plus active page cache: what the kubelet counts against eviction.
    pub memory_working_set_bytes: Option<u64>,
    /// Resident memory alone; the difference from the working set is reclaimable cache.
    pub memory_rss_bytes: Option<u64>,
    pub memory_capacity_bytes: Option<u64>,
    pub fs_used_bytes: Option<u64>,
    pub fs_capacity_bytes: Option<u64>,
    /// Rates need two samples; `None` on the first and after a counter reset.
    pub network_receive_bytes_per_second: Option<f64>,
    pub network_transmit_bytes_per_second: Option<f64>,
    pub temperatures: Vec<Temperature>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GpuMetrics {
    /// The exporter's `Hostname` label, when it carries one.
    pub node: Option<String>,
    pub uuid: String,
    pub index: String,
    pub model: String,
    pub pci_bus_id: String,
    pub utilization_percent: Option<f64>,
    pub memory_used_bytes: Option<u64>,
    /// `FB_TOTAL` when the exporter serves it, otherwise used plus free.
    pub memory_total_bytes: Option<u64>,
    pub temperature_celsius: Option<f64>,
    pub power_watts: Option<f64>,
    pub sm_clock_mhz: Option<f64>,
    pub memory_copy_percent: Option<f64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PodMetrics {
    pub namespace: String,
    pub name: String,
    pub cpu_cores: Option<f64>,
    pub memory_working_set_bytes: Option<u64>,
    pub memory_rss_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSample {
    pub at: String,
    pub interval_seconds: u64,
    pub nodes: Vec<NodeMetrics>,
    pub gpus: Vec<GpuMetrics>,
    pub pods: Vec<PodMetrics>,
    pub sources: Vec<MetricsSourceState>,
}

/// Where GPU metrics come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DcgmSetting {
    /// Find a Service named [`DCGM_SERVICE_NAME`] in any namespace.
    Auto,
    Off,
    Service {
        namespace: String,
        name: String,
        port: u16,
    },
}

impl DcgmSetting {
    /// Empty finds the exporter by name, `off` disables GPU metrics, otherwise
    /// `namespace/name[:port]`.
    pub fn parse(text: &str) -> Result<Self> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Self::Auto);
        }
        if text.eq_ignore_ascii_case("off") {
            return Ok(Self::Off);
        }
        let invalid = || {
            TerraceError::Config(
                "metrics.dcgm.service must be namespace/name[:port], empty, or off".to_string(),
            )
        };
        let (path, port) = match text.rsplit_once(':') {
            Some((path, port)) => (path, port.parse::<u16>().map_err(|_| invalid())?),
            None => (text, DCGM_DEFAULT_PORT),
        };
        let (namespace, name) = path.split_once('/').ok_or_else(invalid)?;
        let valid_label = |value: &str| {
            !value.is_empty()
                && value.len() <= 63
                && value
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && value.as_bytes()[0].is_ascii_alphanumeric()
                && value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        };
        if !valid_label(namespace) || !valid_label(name) || port == 0 {
            return Err(invalid());
        }
        Ok(Self::Service {
            namespace: namespace.to_string(),
            name: name.to_string(),
            port,
        })
    }
}

#[derive(Debug, Clone)]
pub struct MetricsOptions {
    pub interval: Duration,
    pub dcgm: DcgmSetting,
    pub sensors: bool,
}

impl Default for MetricsOptions {
    fn default() -> Self {
        Self {
            interval: DEFAULT_INTERVAL,
            dcgm: DcgmSetting::Auto,
            sensors: true,
        }
    }
}

/// One pod behind the exporter Service, and the node it runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ExporterPod {
    name: String,
    node: Option<String>,
}

/// The exporter Service and, when its EndpointSlices could be read, the pods behind it. The
/// Service proxy reaches one pod at random per request, which on a multi-node cluster would
/// show a different node's GPUs every sample; the pods are read one by one instead, and each
/// pod's node names the GPUs it reports.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Endpoint {
    namespace: String,
    name: String,
    port: u16,
    pods: Vec<ExporterPod>,
}

impl Endpoint {
    fn metrics_path(&self) -> String {
        format!(
            "/api/v1/namespaces/{}/services/{}:{}/proxy/metrics",
            self.namespace, self.name, self.port
        )
    }

    fn pod_metrics_path(&self, pod: &str) -> String {
        format!(
            "/api/v1/namespaces/{}/pods/{}:{}/proxy/metrics",
            self.namespace, pod, self.port
        )
    }
}

/// The ready pods an EndpointSlice list names, with their nodes.
fn exporter_pods(slices: &[EndpointSlice]) -> Vec<ExporterPod> {
    let mut pods = Vec::new();
    for slice in slices {
        for endpoint in slice.endpoints.iter().flatten() {
            let ready = endpoint
                .conditions
                .as_ref()
                .and_then(|conditions| conditions.ready)
                .unwrap_or(true);
            let Some(target) = endpoint.target_ref.as_ref() else {
                continue;
            };
            if !ready || target.kind.as_deref() != Some("Pod") {
                continue;
            }
            if let Some(name) = target.name.clone() {
                pods.push(ExporterPod {
                    name,
                    node: endpoint.node_name.clone(),
                });
            }
        }
    }
    pods.sort_by(|a, b| a.name.cmp(&b.name));
    pods.dedup();
    pods
}

struct NetworkPoint {
    at: Instant,
    rx: u64,
    tx: u64,
}

struct NodeSensors {
    node: String,
    hostname: String,
    sensors: Vec<sensors::Sensor>,
}

/// Samples every source, remembering what it needs between samples: the exporter's address,
/// the last network counters for rates, the discovered sensors and their last readings.
pub struct Collector {
    client: Client,
    talos: Arc<TalosCli>,
    options: MetricsOptions,
    dcgm: Option<Endpoint>,
    network: HashMap<String, NetworkPoint>,
    sensors: Option<Vec<NodeSensors>>,
    temperatures: HashMap<String, Vec<Temperature>>,
    /// Why the sensors source stood down for the session (the talosconfig cannot read files).
    sensors_off: Option<String>,
    /// The last real sensors outcome, repeated on the ticks between reads so the source's
    /// state does not flap between error and live.
    sensors_last: Option<String>,
    tick: u32,
}

impl Collector {
    pub fn new(client: Client, talos: Arc<TalosCli>, options: MetricsOptions) -> Self {
        Self {
            client,
            talos,
            options,
            dcgm: None,
            network: HashMap::new(),
            sensors: None,
            temperatures: HashMap::new(),
            sensors_off: None,
            sensors_last: None,
            tick: 0,
        }
    }

    /// One sample. Never fails as a whole: each source reports its own state.
    pub async fn sample(&mut self) -> MetricsSample {
        let mut sources = Vec::new();
        let mut nodes = Vec::new();
        let mut pods = Vec::new();
        sources.push(state(
            MetricsSource::Kubelet,
            self.collect_kubelet(&mut nodes, &mut pods).await,
        ));

        let mut gpus = Vec::new();
        if self.options.dcgm != DcgmSetting::Off {
            let outcome = self.collect_dcgm().await.map(|found| gpus = found);
            sources.push(state(MetricsSource::Dcgm, outcome));
        }

        if self.options.sensors {
            let outcome = if let Some(reason) = &self.sensors_off {
                Err(TerraceError::Forbidden(reason.clone()))
            } else if self.tick.is_multiple_of(SENSOR_EVERY) {
                let outcome = self.collect_sensors().await;
                self.sensors_last = outcome.as_ref().err().map(ToString::to_string);
                outcome
            } else {
                match &self.sensors_last {
                    Some(message) => Err(TerraceError::Talos(message.clone())),
                    None => Ok(()),
                }
            };
            sources.push(state(MetricsSource::Sensors, outcome));
            for (hostname, temperatures) in &self.temperatures {
                match nodes.iter_mut().find(|node| &node.node == hostname) {
                    Some(node) => node.temperatures = temperatures.clone(),
                    None => nodes.push(NodeMetrics {
                        node: hostname.clone(),
                        temperatures: temperatures.clone(),
                        ..NodeMetrics::default()
                    }),
                }
            }
        }

        // An exporter without a Hostname label on a one-node cluster can only mean that node.
        if let [only] = nodes.as_slice() {
            let name = only.node.clone();
            for gpu in gpus.iter_mut().filter(|gpu| gpu.node.is_none()) {
                gpu.node = Some(name.clone());
            }
        }

        self.tick = self.tick.wrapping_add(1);
        nodes.sort_by(|a, b| a.node.cmp(&b.node));
        pods.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
        MetricsSample {
            at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            interval_seconds: self.options.interval.as_secs(),
            nodes,
            gpus,
            pods,
            sources,
        }
    }

    async fn get_text(&self, path: &str) -> Result<String> {
        let request = http::Request::get(path)
            .body(Vec::new())
            .map_err(|error| TerraceError::Kubernetes(error.to_string()))?;
        Ok(
            tokio::time::timeout(REQUEST_TIMEOUT, self.client.request_text(request))
                .await
                .map_err(|_| TerraceError::Timeout(REQUEST_TIMEOUT))??,
        )
    }

    async fn collect_kubelet(
        &mut self,
        nodes: &mut Vec<NodeMetrics>,
        pods: &mut Vec<PodMetrics>,
    ) -> Result<()> {
        let list = Api::<Node>::all(self.client.clone())
            .list(&ListParams::default())
            .await?;
        let mut failures = Vec::new();
        for node in list.items {
            let name = node.metadata.name.clone().unwrap_or_default();
            // The node's whole machine, not the allocatable share: the working set the kubelet
            // reports includes the system daemons that allocatable leaves out, so a dial over
            // allocatable could read past 100% on a node with reserved memory.
            let capacity = node
                .status
                .as_ref()
                .and_then(|status| status.capacity.as_ref().or(status.allocatable.as_ref()));
            let cpu_capacity = capacity
                .and_then(|map| map.get("cpu"))
                .and_then(|quantity| kubelet::cpu_cores(&quantity.0));
            let memory_capacity = capacity
                .and_then(|map| map.get("memory"))
                .and_then(|quantity| kubelet::bytes(&quantity.0));

            let path = format!("/api/v1/nodes/{name}/proxy/stats/summary");
            let summary = match self.get_text(&path).await {
                Ok(text) => match kubelet::parse(&text) {
                    Ok(summary) => summary,
                    Err(error) => {
                        failures.push(format!("{name}: {error}"));
                        continue;
                    }
                },
                Err(error) => {
                    failures.push(format!("{name}: {error}"));
                    continue;
                }
            };

            let now = Instant::now();
            let (rx, tx) = summary
                .node
                .network
                .as_ref()
                .map(kubelet::NetworkStats::totals)
                .unwrap_or((None, None));
            // The kubelet caches its summary for a few seconds, so two samples can carry the
            // same counters; keep the older point until they move, so every rate spans real change.
            let (receive, transmit) = match (rx, tx) {
                (Some(rx), Some(tx)) => {
                    let unchanged = self
                        .network
                        .get(&name)
                        .is_some_and(|previous| previous.rx == rx && previous.tx == tx);
                    if unchanged {
                        (None, None)
                    } else {
                        let rates = rate(self.network.get(&name), now, rx, tx);
                        self.network
                            .insert(name.clone(), NetworkPoint { at: now, rx, tx });
                        rates
                    }
                }
                _ => (None, None),
            };

            nodes.push(NodeMetrics {
                node: name,
                cpu_cores: summary
                    .node
                    .cpu
                    .as_ref()
                    .and_then(|cpu| cpu.usage_nano_cores)
                    .map(|nano| nano as f64 / 1e9),
                cpu_capacity_cores: cpu_capacity,
                memory_working_set_bytes: summary
                    .node
                    .memory
                    .as_ref()
                    .and_then(|memory| memory.working_set_bytes),
                memory_rss_bytes: summary
                    .node
                    .memory
                    .as_ref()
                    .and_then(|memory| memory.rss_bytes),
                memory_capacity_bytes: memory_capacity,
                fs_used_bytes: summary.node.fs.as_ref().and_then(|fs| fs.used_bytes),
                fs_capacity_bytes: summary.node.fs.as_ref().and_then(|fs| fs.capacity_bytes),
                network_receive_bytes_per_second: receive,
                network_transmit_bytes_per_second: transmit,
                temperatures: Vec::new(),
            });
            for pod in summary.pods {
                pods.push(PodMetrics {
                    namespace: pod.pod_ref.namespace,
                    name: pod.pod_ref.name,
                    cpu_cores: pod
                        .cpu
                        .as_ref()
                        .and_then(|cpu| cpu.usage_nano_cores)
                        .map(|nano| nano as f64 / 1e9),
                    memory_working_set_bytes: pod
                        .memory
                        .as_ref()
                        .and_then(|memory| memory.working_set_bytes),
                    memory_rss_bytes: pod.memory.as_ref().and_then(|memory| memory.rss_bytes),
                });
            }
        }
        if nodes.is_empty() && !failures.is_empty() {
            return Err(TerraceError::Kubernetes(failures.join("; ")));
        }
        if !failures.is_empty() {
            tracing::warn!(failures = ?failures, "some node summaries could not be read");
        }
        Ok(())
    }

    async fn discover_dcgm(&self) -> Result<Option<Endpoint>> {
        let services = Api::<Service>::all(self.client.clone())
            .list(&ListParams::default())
            .await?;
        let Some(service) = services
            .items
            .into_iter()
            .find(|service| service.metadata.name.as_deref() == Some(DCGM_SERVICE_NAME))
        else {
            return Ok(None);
        };
        let namespace = service.metadata.namespace.clone().unwrap_or_default();
        let port = service
            .spec
            .as_ref()
            .and_then(|spec| spec.ports.as_ref())
            .and_then(|ports| ports.first())
            .and_then(|port| u16::try_from(port.port).ok())
            .unwrap_or(DCGM_DEFAULT_PORT);
        Ok(Some(
            self.with_pods(Endpoint {
                namespace,
                name: DCGM_SERVICE_NAME.to_string(),
                port,
                pods: Vec::new(),
            })
            .await,
        ))
    }

    /// Fill in the pods behind the exporter Service. Without permission to list
    /// EndpointSlices the Service proxy is used instead, which is fine on one node.
    async fn with_pods(&self, mut endpoint: Endpoint) -> Endpoint {
        let slices = Api::<EndpointSlice>::namespaced(self.client.clone(), &endpoint.namespace)
            .list(
                &ListParams::default()
                    .labels(&format!("kubernetes.io/service-name={}", endpoint.name)),
            )
            .await;
        match slices {
            Ok(list) => endpoint.pods = exporter_pods(&list.items),
            Err(error) => {
                tracing::debug!(%error, "EndpointSlices not readable; using the Service proxy")
            }
        }
        endpoint
    }

    async fn collect_dcgm(&mut self) -> Result<Vec<GpuMetrics>> {
        let refresh = self.dcgm.is_none() || self.tick.is_multiple_of(DISCOVER_EVERY);
        if let DcgmSetting::Service {
            namespace,
            name,
            port,
        } = &self.options.dcgm
        {
            if refresh {
                self.dcgm = Some(
                    self.with_pods(Endpoint {
                        namespace: namespace.clone(),
                        name: name.clone(),
                        port: *port,
                        pods: Vec::new(),
                    })
                    .await,
                );
            }
        } else if refresh {
            self.dcgm = self.discover_dcgm().await?;
        }
        let Some(endpoint) = self.dcgm.clone() else {
            return Err(TerraceError::NotConnected(format!(
                "no Service named {DCGM_SERVICE_NAME}; GPU dials need a DCGM exporter"
            )));
        };

        let outcome = if endpoint.pods.is_empty() {
            self.get_text(&endpoint.metrics_path())
                .await
                .map(|text| gpus_from_samples(&prometheus::parse(&text)))
        } else {
            let mut gpus = Vec::new();
            let mut failures = Vec::new();
            for pod in &endpoint.pods {
                match self.get_text(&endpoint.pod_metrics_path(&pod.name)).await {
                    Ok(text) => {
                        for mut gpu in gpus_from_samples(&prometheus::parse(&text)) {
                            // The pod's node is authoritative; the exporter's own label may be
                            // the pod name.
                            gpu.node = pod.node.clone().or(gpu.node);
                            gpus.push(gpu);
                        }
                    }
                    Err(error) => failures.push(format!("{}: {error}", pod.name)),
                }
            }
            if gpus.is_empty() && !failures.is_empty() {
                Err(TerraceError::Kubernetes(failures.join("; ")))
            } else {
                if !failures.is_empty() {
                    tracing::warn!(failures = ?failures, "some exporter pods could not be read");
                }
                gpus.sort_by(|a, b| (&a.node, &a.index).cmp(&(&b.node, &b.index)));
                Ok(gpus)
            }
        };
        if outcome.is_err() {
            // Let the next sample look for the exporter (and its pods) again.
            self.dcgm = None;
        }
        outcome
    }

    async fn collect_sensors(&mut self) -> Result<()> {
        if self.sensors.is_none() {
            let (nodes, denied) = self.discover_sensors().await?;
            if denied {
                // Talos reserves the file read API for os:admin. Say so once and stand down:
                // retrying every third sample would only fill the log.
                let reason = "the talosconfig cannot read files on the nodes (talosctl read \
                              needs the os:admin role); sensors stay off this session"
                    .to_string();
                self.sensors_off = Some(reason.clone());
                return Err(TerraceError::Forbidden(reason));
            }
            self.sensors = Some(nodes);
        }
        let Some(nodes) = &self.sensors else {
            return Ok(());
        };
        let mut readings: HashMap<String, Vec<Temperature>> = HashMap::new();
        for node in nodes {
            let mut temperatures = Vec::new();
            for sensor in &node.sensors {
                let path = sensors::temperature_path(sensor.index);
                match self.talos.read(&node.node, &path).await {
                    Ok(text) => {
                        if let Some(celsius) = sensors::parse_millidegrees(&text) {
                            temperatures.push(Temperature {
                                sensor: sensor.name.clone(),
                                label: sensor.label.clone(),
                                celsius,
                            });
                        }
                    }
                    Err(error) => {
                        tracing::debug!(node = node.node, path, %error, "sensor not read")
                    }
                }
            }
            readings.insert(node.hostname.clone(), temperatures);
        }
        self.temperatures = readings;
        Ok(())
    }

    /// Once per node: which hwmon slots hold a sensor Terrace can name. The flag says whether
    /// a node refused the read outright (a permission problem, not a missing sensor).
    async fn discover_sensors(&self) -> Result<(Vec<NodeSensors>, bool)> {
        let hostnames = self.talos.get("hostname", None).await?;
        let mut nodes = Vec::new();
        let mut denied = false;
        for resource in hostnames {
            let Some(node) = resource.node.clone() else {
                continue;
            };
            let hostname = resource
                .spec
                .get("hostname")
                .and_then(|value| value.as_str())
                .unwrap_or(&node)
                .to_string();
            let mut found = Vec::new();
            for index in 0..HWMON_SLOTS {
                let name = match self.talos.read(&node, &sensors::name_path(index)).await {
                    Ok(name) => name,
                    Err(error) if is_permission_denied(&error) => {
                        denied = true;
                        break;
                    }
                    Err(_) => continue,
                };
                let name = name.trim().to_string();
                if let Some(label) = sensors::label_for(&name) {
                    found.push(sensors::Sensor {
                        index,
                        name,
                        label: label.to_string(),
                    });
                }
            }
            sensors::number_duplicates(&mut found);
            tracing::info!(
                node,
                sensors = found.len(),
                "temperature sensors discovered"
            );
            nodes.push(NodeSensors {
                node,
                hostname,
                sensors: found,
            });
        }
        Ok((nodes, denied))
    }
}

/// talosctl reports an authorisation refusal as a gRPC `PermissionDenied` status.
fn is_permission_denied(error: &TerraceError) -> bool {
    let text = error.to_string();
    text.contains("PermissionDenied") || text.to_ascii_lowercase().contains("permission denied")
}

fn state(source: MetricsSource, outcome: Result<()>) -> MetricsSourceState {
    match outcome {
        Ok(()) => MetricsSourceState {
            source,
            ok: true,
            message: None,
        },
        Err(error) => MetricsSourceState {
            source,
            ok: false,
            message: Some(error.to_string()),
        },
    }
}

/// Bytes per second since the previous point; `None` on the first sample or a counter reset.
fn rate(
    previous: Option<&NetworkPoint>,
    now: Instant,
    rx: u64,
    tx: u64,
) -> (Option<f64>, Option<f64>) {
    let Some(previous) = previous else {
        return (None, None);
    };
    let seconds = now.duration_since(previous.at).as_secs_f64();
    if seconds <= 0.0 || rx < previous.rx || tx < previous.tx {
        return (None, None);
    }
    (
        Some((rx - previous.rx) as f64 / seconds),
        Some((tx - previous.tx) as f64 / seconds),
    )
}

/// The exporter's page as one entry per GPU, keyed by UUID (or index when there is none),
/// in index order.
pub fn gpus_from_samples(samples: &[prometheus::Sample]) -> Vec<GpuMetrics> {
    let mut gpus: BTreeMap<String, GpuMetrics> = BTreeMap::new();
    let mut free: BTreeMap<String, u64> = BTreeMap::new();
    let mut explicit_total: BTreeMap<String, u64> = BTreeMap::new();
    let mebibytes = |mib: f64| (mib * 1024.0 * 1024.0).round() as u64;
    for sample in samples {
        let Some(field) = sample.name.strip_prefix("DCGM_FI_DEV_") else {
            continue;
        };
        let key = sample
            .label("UUID")
            .or_else(|| sample.label("gpu"))
            .unwrap_or("0")
            .to_string();
        let gpu = gpus.entry(key.clone()).or_insert_with(|| GpuMetrics {
            node: sample.label("Hostname").map(str::to_string),
            uuid: sample.label("UUID").unwrap_or(&key).to_string(),
            index: sample.label("gpu").unwrap_or("0").to_string(),
            model: sample.label("modelName").unwrap_or("GPU").to_string(),
            pci_bus_id: sample.label("pci_bus_id").unwrap_or_default().to_string(),
            ..GpuMetrics::default()
        });
        let value = sample.value;
        if !value.is_finite() || value < 0.0 {
            continue;
        }
        match field {
            "GPU_UTIL" => gpu.utilization_percent = Some(value),
            "FB_USED" => gpu.memory_used_bytes = Some(mebibytes(value)),
            "FB_FREE" => {
                free.insert(key, mebibytes(value));
            }
            "FB_TOTAL" => {
                explicit_total.insert(key, mebibytes(value));
            }
            "GPU_TEMP" => gpu.temperature_celsius = Some(value),
            "POWER_USAGE" => gpu.power_watts = Some(value),
            "SM_CLOCK" => gpu.sm_clock_mhz = Some(value),
            "MEM_COPY_UTIL" => gpu.memory_copy_percent = Some(value),
            _ => {}
        }
    }
    for (key, gpu) in gpus.iter_mut() {
        gpu.memory_total_bytes = explicit_total.get(key).copied().or_else(|| {
            match (gpu.memory_used_bytes, free.get(key)) {
                (Some(used), Some(free)) => used.checked_add(*free),
                _ => None,
            }
        });
    }
    let mut list: Vec<GpuMetrics> = gpus.into_values().collect();
    list.sort_by(|a, b| {
        a.index
            .parse::<u32>()
            .unwrap_or(0)
            .cmp(&b.index.parse::<u32>().unwrap_or(0))
            .then_with(|| a.uuid.cmp(&b.uuid))
    });
    list
}

/// One sample without the live loop, for the headless probe.
pub async fn probe_once(
    kubeconfig: &Path,
    context: Option<&str>,
    talos: Arc<TalosCli>,
    options: MetricsOptions,
) -> Result<MetricsSample> {
    let connection = cluster::connect(kubeconfig, context).await?;
    let mut collector = Collector::new(connection.client, talos, options);
    Ok(collector.sample().await)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exporter_pods_come_from_ready_pod_endpoints_with_their_nodes() {
        let slice: EndpointSlice = serde_json::from_value(serde_json::json!({
            "metadata": {"name": "nvidia-dcgm-exporter-abc", "namespace": "gpu-operator"},
            "addressType": "IPv4",
            "endpoints": [
                {"addresses": ["10.244.0.5"], "conditions": {"ready": true}, "nodeName": "node-a",
                 "targetRef": {"kind": "Pod", "name": "nvidia-dcgm-exporter-x1"}},
                {"addresses": ["10.244.1.5"], "conditions": {"ready": false}, "nodeName": "node-b",
                 "targetRef": {"kind": "Pod", "name": "nvidia-dcgm-exporter-x2"}},
                {"addresses": ["10.244.2.5"], "nodeName": "node-c",
                 "targetRef": {"kind": "Pod", "name": "nvidia-dcgm-exporter-x3"}}
            ]
        }))
        .expect("slice");
        let pods = exporter_pods(&[slice]);
        assert_eq!(
            pods,
            vec![
                ExporterPod {
                    name: "nvidia-dcgm-exporter-x1".into(),
                    node: Some("node-a".into())
                },
                ExporterPod {
                    name: "nvidia-dcgm-exporter-x3".into(),
                    node: Some("node-c".into())
                },
            ]
        );
        let endpoint = Endpoint {
            namespace: "gpu-operator".into(),
            name: "nvidia-dcgm-exporter".into(),
            port: 9400,
            pods,
        };
        assert_eq!(
            endpoint.pod_metrics_path("nvidia-dcgm-exporter-x1"),
            "/api/v1/namespaces/gpu-operator/pods/nvidia-dcgm-exporter-x1:9400/proxy/metrics"
        );
    }

    #[test]
    fn a_talosctl_permission_refusal_is_recognised() {
        assert!(is_permission_denied(&TerraceError::Talos(
            "talosctl read /sys/class/hwmon/hwmon0/name failed: rpc error: code = PermissionDenied desc = not authorized".into()
        )));
        assert!(!is_permission_denied(&TerraceError::Talos(
            "talosctl read /x failed: file does not exist".into()
        )));
    }

    #[test]
    fn dcgm_setting_forms() {
        assert_eq!(DcgmSetting::parse("").expect("auto"), DcgmSetting::Auto);
        assert_eq!(DcgmSetting::parse(" OFF ").expect("off"), DcgmSetting::Off);
        assert_eq!(
            DcgmSetting::parse("monitoring/dcgm").expect("service"),
            DcgmSetting::Service {
                namespace: "monitoring".into(),
                name: "dcgm".into(),
                port: DCGM_DEFAULT_PORT
            }
        );
        assert_eq!(
            DcgmSetting::parse("gpu/exporter:9401").expect("service with port"),
            DcgmSetting::Service {
                namespace: "gpu".into(),
                name: "exporter".into(),
                port: 9401
            }
        );
        assert!(DcgmSetting::parse("no-slash").is_err());
        assert!(DcgmSetting::parse("a/b:port").is_err());
        assert!(DcgmSetting::parse("/b").is_err());
        for invalid in [
            "a/b:0",
            "a/b:65536",
            "a/../b",
            "a/b?token=x",
            "-a/b",
            "a/b-",
        ] {
            assert!(DcgmSetting::parse(invalid).is_err());
        }
    }

    #[test]
    fn proxy_path_for_the_exporter() {
        let endpoint = Endpoint {
            pods: Vec::new(),
            namespace: "gpu-operator".into(),
            name: DCGM_SERVICE_NAME.into(),
            port: 9400,
        };
        assert_eq!(
            endpoint.metrics_path(),
            "/api/v1/namespaces/gpu-operator/services/nvidia-dcgm-exporter:9400/proxy/metrics"
        );
    }

    #[test]
    fn rates_need_two_points_and_survive_resets() {
        let start = Instant::now();
        assert_eq!(rate(None, start, 10, 20), (None, None));
        let previous = NetworkPoint {
            at: start,
            rx: 1000,
            tx: 500,
        };
        let later = start + Duration::from_secs(2);
        let (rx, tx) = rate(Some(&previous), later, 3000, 900);
        assert_eq!(rx, Some(1000.0));
        assert_eq!(tx, Some(200.0));
        assert_eq!(rate(Some(&previous), later, 10, 900), (None, None), "reset");
        assert_eq!(
            rate(Some(&previous), start, 3000, 900),
            (None, None),
            "no time"
        );
    }

    #[test]
    fn malformed_gpu_values_do_not_wrap_or_overflow() {
        let page = "DCGM_FI_DEV_FB_USED{UUID=\"GPU-a\"} 1e30\nDCGM_FI_DEV_FB_FREE{UUID=\"GPU-a\"} 1e30\nDCGM_FI_DEV_POWER_USAGE{UUID=\"GPU-a\"} -1\n";
        let gpus = gpus_from_samples(&prometheus::parse(page));
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].memory_total_bytes, None);
        assert_eq!(gpus[0].power_watts, None);
    }

    #[test]
    fn gpus_are_built_from_dcgm_fields_in_any_order() {
        let page = "DCGM_FI_DEV_FB_FREE{gpu=\"1\",UUID=\"GPU-b\",modelName=\"Card B\",pci_bus_id=\"00000000:0B:00.0\",Hostname=\"node-a\"} 5000\n\
                    DCGM_FI_DEV_FB_USED{gpu=\"1\",UUID=\"GPU-b\"} 1000\n\
                    DCGM_FI_DEV_GPU_UTIL{gpu=\"0\",UUID=\"GPU-a\",modelName=\"Card A\",Hostname=\"node-a\"} 37\n\
                    DCGM_FI_DEV_FB_USED{gpu=\"0\",UUID=\"GPU-a\"} 2048\n\
                    DCGM_FI_DEV_FB_TOTAL{gpu=\"0\",UUID=\"GPU-a\"} 24576\n\
                    DCGM_FI_DEV_GPU_TEMP{gpu=\"0\",UUID=\"GPU-a\"} 41\n\
                    DCGM_FI_DEV_POWER_USAGE{gpu=\"0\",UUID=\"GPU-a\"} 15.5\n\
                    DCGM_FI_DEV_SM_CLOCK{gpu=\"0\",UUID=\"GPU-a\"} 210\n\
                    DCGM_FI_DEV_XID_ERRORS{gpu=\"0\",UUID=\"GPU-a\"} 0\n\
                    DCGM_FI_DRIVER_VERSION{gpu=\"0\"} 580\n";
        let gpus = gpus_from_samples(&prometheus::parse(page));
        assert_eq!(gpus.len(), 2);
        let a = &gpus[0];
        assert_eq!(a.uuid, "GPU-a");
        assert_eq!(a.index, "0");
        assert_eq!(a.model, "Card A");
        assert_eq!(a.node.as_deref(), Some("node-a"));
        assert_eq!(a.utilization_percent, Some(37.0));
        assert_eq!(a.memory_used_bytes, Some(2048 * 1024 * 1024));
        assert_eq!(a.memory_total_bytes, Some(24576 * 1024 * 1024));
        assert_eq!(a.temperature_celsius, Some(41.0));
        assert_eq!(a.power_watts, Some(15.5));
        assert_eq!(a.sm_clock_mhz, Some(210.0));
        let b = &gpus[1];
        assert_eq!(b.pci_bus_id, "00000000:0B:00.0");
        assert_eq!(b.memory_used_bytes, Some(1000 * 1024 * 1024));
        assert_eq!(
            b.memory_total_bytes,
            Some(6000 * 1024 * 1024),
            "used plus free when no total is served"
        );
    }

    #[test]
    fn a_sample_serialises_in_camel_case() {
        let sample = MetricsSample {
            at: "2026-01-01T00:00:00.000Z".into(),
            interval_seconds: 5,
            nodes: vec![NodeMetrics {
                node: "node-a".into(),
                cpu_cores: Some(0.3),
                ..NodeMetrics::default()
            }],
            gpus: vec![],
            pods: vec![],
            sources: vec![state(MetricsSource::Kubelet, Ok(()))],
        };
        let json = serde_json::to_value(&sample).expect("json");
        assert_eq!(json["intervalSeconds"], 5);
        assert_eq!(json["nodes"][0]["cpuCores"], 0.3);
        assert!(json["nodes"][0]["memoryWorkingSetBytes"].is_null());
        assert_eq!(json["sources"][0]["source"], "kubelet");
        assert_eq!(json["sources"][0]["ok"], true);
    }
}
