//! Read-only Kubernetes access over kube-rs with rustls. The kubeconfig is read in place from
//! the path in settings; nothing is copied. This module lists and summarises; the live path in
//! `watch/` feeds the same `summarise` from reflector stores. Writes (M1) will be a separate
//! module with their own diff-and-confirm path.
//!
//! Workloads are found through Pods: every pod's owner chain is walked to its root
//! (Deployment, DaemonSet, StatefulSet, Job, VirtualMachine, a static pod's node, …), so
//! nothing running can hide. Deployments contribute their own status too. Each workload
//! carries a role (app, platform, system) from `roles.rs` so the canvas can show the user's
//! things and fold the machinery away.

pub mod kubevirt;
pub mod roles;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Container, Event, Namespace, Node, Pod, Service};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use k8s_openapi::jiff::Timestamp;
use kube::api::{DynamicObject, ListParams};
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{Api, Client, Config};
use serde::Serialize;

use crate::error::{Result, TerraceError};
pub use kubevirt::{KubeVirtApi, VmDetails};
pub use roles::{Classifier, WorkloadRole};

const TIMEOUT: Duration = Duration::from_secs(20);
/// Extended-resource prefixes that name an accelerator, whoever makes it.
const ACCELERATOR_VENDORS: &[&str] = &[
    "nvidia.com/",
    "amd.com/",
    "gpu.intel.com/",
    "habana.ai/",
    "xilinx.com/",
];

/// `nvidia.com/gpu`, `amd.com/gpu`, `gpu.intel.com/i915`, a KubeVirt host device's resource
/// name under a vendor's prefix, or anything else with "gpu" in it.
pub fn is_accelerator(resource: &str) -> bool {
    ACCELERATOR_VENDORS
        .iter()
        .any(|prefix| resource.starts_with(prefix))
        || resource.to_ascii_lowercase().contains("gpu")
}

/// A whole GPU as a scheduler resource (`vendor/gpu`), as opposed to a slice, a sub-device or
/// a device passed through to a virtual machine.
pub fn is_whole_gpu(resource: &str) -> bool {
    resource.ends_with("/gpu")
}
const PSA_ENFORCE_LABEL: &str = "pod-security.kubernetes.io/enforce";
/// How many events a summary carries, newest first.
pub const MAX_EVENTS: usize = 40;
/// The list path reads at most this many events; the watch path sees them all.
const EVENT_LIST_LIMIT: u32 = 1000;
/// Node labels worth carrying to the canvas; everything else stays on the node.
const LABEL_PREFIXES: &[&str] = &[
    "nvidia.com/",
    "amd.com/",
    "gpu.intel.com/",
    "habana.ai/",
    "node-role.kubernetes.io/",
    "kubernetes.io/arch",
    "kubernetes.io/os",
    "feature.node.kubernetes.io/pci-",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeAddress {
    pub kind: String,
    pub address: String,
}

/// How many of an extended resource (`vendor.com/device`) a node has and exposes to scheduling (not its remaining free devices).
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCount {
    pub capacity: u32,
    pub allocatable: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub name: String,
    pub ready: bool,
    pub kubelet_version: String,
    pub os_image: String,
    pub kernel_version: String,
    pub container_runtime: String,
    /// Whole GPUs (`vendor/gpu` resources) the node has and can still hand out; the
    /// per-resource detail is in `devices`.
    pub gpu_capacity: u32,
    pub gpu_allocatable: u32,
    /// Every extended resource the node advertises (`nvidia.com/gpu`, a KubeVirt host
    /// device's resource name, …), by name.
    pub devices: BTreeMap<String, DeviceCount>,
    pub addresses: Vec<NodeAddress>,
    pub taints: Vec<String>,
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NamespaceSummary {
    pub name: String,
    pub psa_enforce: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PodSummary {
    pub name: String,
    pub phase: String,
    pub ready: bool,
    pub restarts: i32,
    pub node: Option<String>,
    pub started_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadSummary {
    pub namespace: String,
    pub name: String,
    /// `Deployment`, `DaemonSet`, `StatefulSet`, `Job`, `VirtualMachine`, `StaticPod`, `Pod`,
    /// or whatever custom kind owns the pods (`ClusterPolicy`).
    pub kind: String,
    pub role: WorkloadRole,
    pub image: String,
    pub ready_replicas: i32,
    pub replicas: i32,
    /// Deployments only; empty otherwise.
    pub strategy: String,
    pub wants_gpu: bool,
    /// Every extended resource its containers ask for, so the GPU layer can say who holds
    /// which device; a stopped virtual machine lists the devices its spec declares.
    pub devices: Vec<String>,
    pub available: bool,
    /// The Pods this workload owns, by name.
    pub pods: Vec<PodSummary>,
    /// For `VirtualMachine` workloads.
    pub vm: Option<VmDetails>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSummary {
    pub namespace: String,
    pub kind: String,
    pub name: String,
    pub reason: String,
    pub message: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub count: i32,
    pub last_seen: Option<String>,
}

/// A Service port: what the cluster answers on, where it lands in the pod, and the node
/// port when there is one.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServicePortSummary {
    pub name: Option<String>,
    pub port: i32,
    pub target_port: String,
    pub node_port: Option<i32>,
    pub protocol: String,
}

/// A workload a Service selects, named by the root a person would name (see [`root_of`]).
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadRef {
    pub kind: String,
    pub name: String,
}

/// A Service: the edge from the cluster's network to the workloads it selects.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    pub namespace: String,
    pub name: String,
    /// `ClusterIP`, `NodePort`, `LoadBalancer` or `ExternalName`.
    #[serde(rename = "type")]
    pub type_: String,
    /// None for headless Services.
    pub cluster_ip: Option<String>,
    pub ports: Vec<ServicePortSummary>,
    /// Addresses or hostnames a LoadBalancer was given.
    pub external_ips: Vec<String>,
    /// Workloads in the same namespace whose pods match the selector, sorted.
    pub targets: Vec<WorkloadRef>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterSummary {
    pub context: String,
    pub server: String,
    pub version: String,
    pub nodes: Vec<NodeSummary>,
    pub namespaces: Vec<NamespaceSummary>,
    pub workloads: Vec<WorkloadSummary>,
    pub services: Vec<ServiceSummary>,
    /// Newest first, at most `MAX_EVENTS`.
    pub events: Vec<EventSummary>,
}

/// What a summary says about the cluster it came from.
#[derive(Debug, Clone)]
pub struct Target {
    pub context: String,
    pub server: String,
    pub version: String,
}

/// A connected client and its target, shared by the list path and the watch path.
pub struct Connection {
    pub client: Client,
    pub target: Target,
    /// Present when the cluster serves `kubevirt.io`.
    pub kubevirt: Option<KubeVirtApi>,
}

/// The raw objects a summary is built from. `Arc` because reflector stores hand them out that way.
#[derive(Debug, Default)]
pub struct ClusterObjects {
    pub nodes: Vec<Arc<Node>>,
    pub namespaces: Vec<Arc<Namespace>>,
    pub deployments: Vec<Arc<Deployment>>,
    pub pods: Vec<Arc<Pod>>,
    pub services: Vec<Arc<Service>>,
    pub events: Vec<Arc<Event>>,
    /// KubeVirt VirtualMachines and VirtualMachineInstances; empty without KubeVirt.
    pub vms: Vec<Arc<DynamicObject>>,
    pub vmis: Vec<Arc<DynamicObject>>,
}

/// Connect to the cluster the kubeconfig (and optional context) points at and read its version.
pub async fn connect(kubeconfig_path: &Path, context: Option<&str>) -> Result<Connection> {
    tokio::time::timeout(TIMEOUT, connect_inner(kubeconfig_path, context))
        .await
        .map_err(|_| TerraceError::Timeout(TIMEOUT))?
}

async fn connect_inner(kubeconfig_path: &Path, context: Option<&str>) -> Result<Connection> {
    if !kubeconfig_path.exists() {
        return Err(TerraceError::NotConnected(format!(
            "kubeconfig not found at {}",
            kubeconfig_path.display()
        )));
    }

    let kubeconfig = Kubeconfig::read_from(kubeconfig_path)?;
    let context_name = context
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(String::from)
        .or_else(|| kubeconfig.current_context.clone())
        .ok_or_else(|| {
            TerraceError::Config("kubeconfig has no current-context; choose one in Settings".into())
        })?;

    let options = KubeConfigOptions {
        context: Some(context_name.clone()),
        ..KubeConfigOptions::default()
    };
    let config = Config::from_custom_kubeconfig(kubeconfig, &options).await?;
    let server = config.cluster_url.to_string();
    let client = Client::try_from(config)?;

    let version = client
        .apiserver_version()
        .await
        .map_err(|error| TerraceError::NotConnected(format!("{server}: {error}")))?
        .git_version;

    let kubevirt = kubevirt::discover(&client).await;

    Ok(Connection {
        client,
        target: Target {
            context: context_name,
            server,
            version,
        },
        kubevirt,
    })
}

/// Nodes, namespaces, every workload with its pods, VMs and recent events, in one read.
pub async fn summary(
    kubeconfig_path: &Path,
    context: Option<&str>,
    classifier: &Classifier,
) -> Result<ClusterSummary> {
    tokio::time::timeout(TIMEOUT, summary_inner(kubeconfig_path, context, classifier))
        .await
        .map_err(|_| TerraceError::Timeout(TIMEOUT))?
}

async fn summary_inner(
    kubeconfig_path: &Path,
    context: Option<&str>,
    classifier: &Classifier,
) -> Result<ClusterSummary> {
    let connection = connect_inner(kubeconfig_path, context).await?;
    let client = &connection.client;
    let everything = ListParams::default();

    let nodes = Api::<Node>::all(client.clone()).list(&everything).await?;
    let namespaces = Api::<Namespace>::all(client.clone())
        .list(&everything)
        .await?;
    let deployments = Api::<Deployment>::all(client.clone())
        .list(&everything)
        .await?;
    let pods = Api::<Pod>::all(client.clone()).list(&everything).await?;
    let services = Api::<Service>::all(client.clone())
        .list(&everything)
        .await?;
    let events = Api::<Event>::all(client.clone())
        .list(&ListParams::default().limit(EVENT_LIST_LIMIT))
        .await?;
    let (vms, vmis) = match &connection.kubevirt {
        Some(api) => (
            Api::<DynamicObject>::all_with(client.clone(), &api.vm)
                .list(&everything)
                .await?
                .items,
            Api::<DynamicObject>::all_with(client.clone(), &api.vmi)
                .list(&everything)
                .await?
                .items,
        ),
        None => (Vec::new(), Vec::new()),
    };

    let objects = ClusterObjects {
        nodes: arcs(nodes.items),
        namespaces: arcs(namespaces.items),
        deployments: arcs(deployments.items),
        pods: arcs(pods.items),
        services: arcs(services.items),
        events: arcs(events.items),
        vms: arcs(vms),
        vmis: arcs(vmis),
    };
    Ok(summarise(&connection.target, &objects, classifier))
}

fn arcs<K>(items: Vec<K>) -> Vec<Arc<K>> {
    items.into_iter().map(Arc::new).collect()
}

/// Pure: objects in, summary out, sorted so that the canvas is stable between rebuilds.
pub fn summarise(
    target: &Target,
    objects: &ClusterObjects,
    classifier: &Classifier,
) -> ClusterSummary {
    let mut nodes: Vec<NodeSummary> = objects.nodes.iter().map(|n| summarise_node(n)).collect();
    nodes.sort_by(|a, b| a.name.cmp(&b.name));

    let mut namespaces: Vec<NamespaceSummary> = objects
        .namespaces
        .iter()
        .map(|n| summarise_namespace(n))
        .collect();
    namespaces.sort_by(|a, b| a.name.cmp(&b.name));

    let workloads = summarise_workloads(objects, classifier);
    let services = summarise_services(objects);

    ClusterSummary {
        context: target.context.clone(),
        server: target.server.clone(),
        version: target.version.clone(),
        nodes,
        namespaces,
        workloads,
        services,
        events: recent_events(&objects.events),
    }
}

/// A counted quantity (`1`, `1k` as KubeVirt writes its device plugins' capacity).
fn quantity_to_u32(quantity: Option<&Quantity>) -> u32 {
    quantity
        .and_then(|value| crate::metrics::kubelet::bytes(&value.0))
        .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

/// Extended resources are the quantities whose name carries a vendor domain (`nvidia.com/gpu`);
/// `cpu`, `memory`, `pods` and `hugepages-*` stay behind.
fn extended_resources(
    capacity: Option<&BTreeMap<String, Quantity>>,
    allocatable: Option<&BTreeMap<String, Quantity>>,
) -> BTreeMap<String, DeviceCount> {
    let mut devices: BTreeMap<String, DeviceCount> = BTreeMap::new();
    for (key, quantity) in capacity.into_iter().flatten() {
        if key.contains('/') {
            devices.entry(key.clone()).or_default().capacity = quantity_to_u32(Some(quantity));
        }
    }
    for (key, quantity) in allocatable.into_iter().flatten() {
        if key.contains('/') {
            devices.entry(key.clone()).or_default().allocatable = quantity_to_u32(Some(quantity));
        }
    }
    devices
}

/// The extended resources a set of containers ask for in their limits.
fn container_devices(containers: &[Container]) -> BTreeSet<String> {
    containers
        .iter()
        .filter_map(|c| c.resources.as_ref().and_then(|r| r.limits.as_ref()))
        .flat_map(|limits| limits.keys().filter(|key| key.contains('/')).cloned())
        .collect()
}

fn pod_devices(pod: &Pod) -> BTreeSet<String> {
    pod.spec
        .as_ref()
        .map(|spec| container_devices(&spec.containers))
        .unwrap_or_default()
}

fn condition_true(conditions: Option<&[(String, String)]>, wanted: &str) -> bool {
    conditions
        .map(|list| {
            list.iter()
                .any(|(kind, status)| kind == wanted && status == "True")
        })
        .unwrap_or(false)
}

pub fn summarise_node(node: &Node) -> NodeSummary {
    let status = node.status.as_ref();
    let info = status.and_then(|status| status.node_info.as_ref());
    let conditions: Option<Vec<(String, String)>> = status
        .and_then(|status| status.conditions.as_ref())
        .map(|list| {
            list.iter()
                .map(|c| (c.type_.clone(), c.status.clone()))
                .collect()
        });

    let labels = node
        .metadata
        .labels
        .as_ref()
        .map(|labels| {
            labels
                .iter()
                .filter(|(key, _)| LABEL_PREFIXES.iter().any(|prefix| key.starts_with(prefix)))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    NodeSummary {
        name: node.metadata.name.clone().unwrap_or_default(),
        ready: condition_true(conditions.as_deref(), "Ready"),
        kubelet_version: info.map(|i| i.kubelet_version.clone()).unwrap_or_default(),
        os_image: info.map(|i| i.os_image.clone()).unwrap_or_default(),
        kernel_version: info.map(|i| i.kernel_version.clone()).unwrap_or_default(),
        container_runtime: info
            .map(|i| i.container_runtime_version.clone())
            .unwrap_or_default(),
        gpu_capacity: whole_gpus(status.and_then(|s| s.capacity.as_ref())),
        gpu_allocatable: whole_gpus(status.and_then(|s| s.allocatable.as_ref())),
        devices: extended_resources(
            status.and_then(|s| s.capacity.as_ref()),
            status.and_then(|s| s.allocatable.as_ref()),
        ),
        addresses: status
            .and_then(|s| s.addresses.as_ref())
            .map(|list| {
                list.iter()
                    .map(|a| NodeAddress {
                        kind: a.type_.clone(),
                        address: a.address.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        taints: node
            .spec
            .as_ref()
            .and_then(|spec| spec.taints.as_ref())
            .map(|taints| {
                taints
                    .iter()
                    .map(|t| {
                        format!(
                            "{}={}:{}",
                            t.key,
                            t.value.clone().unwrap_or_default(),
                            t.effect
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        labels,
    }
}

pub fn summarise_namespace(namespace: &Namespace) -> NamespaceSummary {
    NamespaceSummary {
        name: namespace.metadata.name.clone().unwrap_or_default(),
        psa_enforce: namespace
            .metadata
            .labels
            .as_ref()
            .and_then(|labels| labels.get(PSA_ENFORCE_LABEL).cloned()),
    }
}

/// Every workload: Deployments with their status, then one entry per owner root of the pods no
/// Deployment claimed (DaemonSets, StatefulSets, Jobs, VMs, static pods, custom kinds), then
/// VMs that have no pod (stopped). Roles come last, from the names seen in each namespace.
fn summarise_workloads(objects: &ClusterObjects, classifier: &Classifier) -> Vec<WorkloadSummary> {
    let mut workloads: Vec<WorkloadSummary> = objects
        .deployments
        .iter()
        .map(|d| summarise_deployment(d, &objects.pods))
        .collect();
    let mut claimed: BTreeSet<String> = workloads
        .iter()
        .flat_map(|w| w.pods.iter().map(|p| format!("{}/{}", w.namespace, p.name)))
        .collect();

    // Roots of the remaining pods, in first-seen order within a stable sort later.
    let mut roots: BTreeMap<(String, String, String), Vec<&Pod>> = BTreeMap::new();
    for pod in &objects.pods {
        let namespace = pod.metadata.namespace.clone().unwrap_or_default();
        let name = pod.metadata.name.clone().unwrap_or_default();
        if !claimed.insert(format!("{namespace}/{name}")) {
            continue;
        }
        let (kind, root) = root_of(pod);
        roots.entry((namespace, kind, root)).or_default().push(pod);
    }
    for ((namespace, kind, name), pods) in roots {
        workloads.push(summarise_root(&namespace, &kind, &name, &pods));
    }

    // Virtual machines: attach details to the root their pod produced, or add a stopped one.
    for vm in &objects.vms {
        let namespace = vm.metadata.namespace.clone().unwrap_or_default();
        let name = vm.metadata.name.clone().unwrap_or_default();
        let vmi = objects
            .vmis
            .iter()
            .find(|candidate| {
                candidate.metadata.namespace.as_deref() == Some(namespace.as_str())
                    && candidate.metadata.name.as_deref() == Some(name.as_str())
            })
            .map(Arc::as_ref);
        let details = kubevirt::details(vm, vmi);
        match workloads
            .iter_mut()
            .find(|w| w.kind == "VirtualMachine" && w.namespace == namespace && w.name == name)
        {
            Some(existing) => {
                existing.available = details.ready;
                existing.vm = Some(details);
            }
            None => workloads.push(WorkloadSummary {
                namespace,
                name,
                kind: "VirtualMachine".to_string(),
                role: WorkloadRole::App,
                image: String::new(),
                ready_replicas: 0,
                replicas: 0,
                strategy: String::new(),
                wants_gpu: false,
                devices: details.gpus.clone(),
                available: details.ready,
                pods: Vec::new(),
                vm: Some(details),
            }),
        }
    }

    // Roles: a namespace is classified once, from every workload name in it.
    let mut names_by_namespace: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for workload in &workloads {
        names_by_namespace
            .entry(workload.namespace.as_str())
            .or_default()
            .push(workload.name.as_str());
    }
    let roles: BTreeMap<String, WorkloadRole> = names_by_namespace
        .iter()
        .map(|(namespace, names)| (namespace.to_string(), classifier.classify(namespace, names)))
        .collect();
    for workload in &mut workloads {
        workload.role = roles
            .get(&workload.namespace)
            .copied()
            .unwrap_or(WorkloadRole::App);
    }

    workloads
        .sort_by(|a, b| (&a.namespace, &a.kind, &a.name).cmp(&(&b.namespace, &b.kind, &b.name)));
    workloads
}

/// Every Service, sorted by namespace and name, each with the workloads its selector reaches.
fn summarise_services(objects: &ClusterObjects) -> Vec<ServiceSummary> {
    let mut services: Vec<ServiceSummary> = objects
        .services
        .iter()
        .map(|service| summarise_service(service, &objects.pods))
        .collect();
    services.sort_by(|a, b| (&a.namespace, &a.name).cmp(&(&b.namespace, &b.name)));
    services
}

/// A Service and the workloads it selects: pods in its namespace whose labels carry every
/// selector pair, walked to their root. No selector (or a headless Service) reaches nothing.
pub fn summarise_service(service: &Service, pods: &[Arc<Pod>]) -> ServiceSummary {
    let namespace = service.metadata.namespace.clone().unwrap_or_default();
    let name = service.metadata.name.clone().unwrap_or_default();
    let spec = service.spec.as_ref();

    let mut targets: BTreeSet<WorkloadRef> = BTreeSet::new();
    if let Some(selector) = spec
        .and_then(|spec| spec.selector.as_ref())
        .filter(|selector| !selector.is_empty())
    {
        for pod in pods {
            if pod.metadata.namespace.as_deref() != Some(namespace.as_str()) {
                continue;
            }
            if !labels_match(selector, pod.metadata.labels.as_ref()) {
                continue;
            }
            let (kind, root) = root_of(pod);
            targets.insert(WorkloadRef { kind, name: root });
        }
    }

    let ports = spec
        .and_then(|spec| spec.ports.as_ref())
        .map(|ports| {
            ports
                .iter()
                .map(|port| ServicePortSummary {
                    name: port.name.clone(),
                    port: port.port,
                    target_port: match &port.target_port {
                        Some(IntOrString::Int(value)) => value.to_string(),
                        Some(IntOrString::String(value)) => value.clone(),
                        None => port.port.to_string(),
                    },
                    node_port: port.node_port,
                    protocol: port.protocol.clone().unwrap_or_else(|| "TCP".to_string()),
                })
                .collect()
        })
        .unwrap_or_default();

    let external_ips = service
        .status
        .as_ref()
        .and_then(|status| status.load_balancer.as_ref())
        .and_then(|balancer| balancer.ingress.as_ref())
        .map(|ingress| {
            ingress
                .iter()
                .filter_map(|entry| entry.ip.clone().or_else(|| entry.hostname.clone()))
                .collect()
        })
        .unwrap_or_default();

    ServiceSummary {
        namespace,
        name,
        type_: spec
            .and_then(|spec| spec.type_.clone())
            .unwrap_or_else(|| "ClusterIP".to_string()),
        cluster_ip: spec
            .and_then(|spec| spec.cluster_ip.clone())
            .filter(|ip| ip != "None" && !ip.is_empty()),
        ports,
        external_ips,
        targets: targets.into_iter().collect(),
    }
}

fn labels_match(
    selector: &BTreeMap<String, String>,
    labels: Option<&BTreeMap<String, String>>,
) -> bool {
    selector
        .iter()
        .all(|(key, value)| labels.and_then(|labels| labels.get(key)) == Some(value))
}

/// Walk a pod's first owner to the thing a person would name: the Deployment behind a
/// ReplicaSet (the `pod-template-hash` label pins the suffix), the VirtualMachine behind a
/// VirtualMachineInstance, the static pod behind a Node, or the owner itself.
pub fn root_of(pod: &Pod) -> (String, String) {
    let pod_name = pod.metadata.name.clone().unwrap_or_default();
    let Some(owner) = pod.metadata.owner_references.as_ref().and_then(|list| {
        list.iter()
            .find(|owner| owner.controller == Some(true))
            .or_else(|| list.first())
    }) else {
        return ("Pod".to_string(), pod_name);
    };
    match owner.kind.as_str() {
        "ReplicaSet" => {
            let hash = pod
                .metadata
                .labels
                .as_ref()
                .and_then(|labels| labels.get("pod-template-hash"));
            ("Deployment".to_string(), strip_suffix(&owner.name, hash))
        }
        "VirtualMachineInstance" => ("VirtualMachine".to_string(), owner.name.clone()),
        "Node" => (
            "StaticPod".to_string(),
            pod_name
                .strip_suffix(&format!("-{}", owner.name))
                .unwrap_or(&pod_name)
                .to_string(),
        ),
        other => (other.to_string(), owner.name.clone()),
    }
}

/// `ollama-64db946489` with hash `64db946489` is `ollama`; without a hash, drop the last segment.
fn strip_suffix(name: &str, hash: Option<&String>) -> String {
    if let Some(hash) = hash
        && let Some(stripped) = name.strip_suffix(&format!("-{hash}"))
    {
        return stripped.to_string();
    }
    match name.rfind('-') {
        Some(index) if index > 0 => name[..index].to_string(),
        _ => name.to_string(),
    }
}

/// The whole GPUs in a capacity or allocatable map, whoever makes them.
fn whole_gpus(quantities: Option<&BTreeMap<String, Quantity>>) -> u32 {
    quantities
        .map(|map| {
            map.iter()
                .filter(|(name, _)| is_whole_gpu(name))
                .map(|(_, quantity)| quantity_to_u32(Some(quantity)))
                .fold(0u32, u32::saturating_add)
        })
        .unwrap_or(0)
}

fn limits_want_accelerator(limits: Option<&BTreeMap<String, Quantity>>) -> bool {
    limits.is_some_and(|limits| limits.keys().any(|name| is_accelerator(name)))
}

fn pod_wants_gpu(pod: &Pod) -> bool {
    pod.spec
        .as_ref()
        .map(|spec| {
            spec.containers.iter().any(|c| {
                limits_want_accelerator(c.resources.as_ref().and_then(|r| r.limits.as_ref()))
            })
        })
        .unwrap_or(false)
}

/// A workload known only through its pods: counts from the pods, image from the first one.
fn summarise_root(namespace: &str, kind: &str, name: &str, pods: &[&Pod]) -> WorkloadSummary {
    let mut summaries: Vec<PodSummary> = pods.iter().map(|pod| summarise_pod(pod)).collect();
    summaries.sort_by(|a, b| a.name.cmp(&b.name));
    let ready = summaries.iter().filter(|p| p.ready).count();
    let finished = summaries.iter().filter(|p| p.phase == "Succeeded").count();
    let total = summaries.len();
    WorkloadSummary {
        namespace: namespace.to_string(),
        name: name.to_string(),
        kind: kind.to_string(),
        role: WorkloadRole::App,
        image: pods
            .first()
            .and_then(|pod| pod.spec.as_ref())
            .and_then(|spec| spec.containers.first())
            .and_then(|c| c.image.clone())
            .unwrap_or_default(),
        ready_replicas: ready as i32,
        replicas: total as i32,
        strategy: String::new(),
        wants_gpu: pods.iter().any(|pod| pod_wants_gpu(pod)),
        devices: pods
            .iter()
            .flat_map(|pod| pod_devices(pod))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        // A one-shot task whose pods all succeeded is fine (Jobs, validators); anything else
        // must have every pod ready.
        available: total > 0 && (ready == total || finished == total),
        pods: summaries,
        vm: None,
    }
}

/// A Pod belongs to a Deployment when it is in the same namespace and carries every label of
/// the Deployment's selector. Good enough without walking ReplicaSets.
fn selects(deployment: &Deployment, pod: &Pod) -> bool {
    if deployment.metadata.namespace != pod.metadata.namespace {
        return false;
    }
    let Some(selector) = deployment
        .spec
        .as_ref()
        .and_then(|spec| spec.selector.match_labels.as_ref())
    else {
        return false;
    };
    if selector.is_empty() {
        return false;
    }
    let labels = pod.metadata.labels.as_ref();
    selector.iter().all(|(key, value)| {
        labels
            .and_then(|labels| labels.get(key))
            .is_some_and(|found| found == value)
    })
}

pub fn summarise_pod(pod: &Pod) -> PodSummary {
    let status = pod.status.as_ref();
    let containers = status
        .and_then(|s| s.container_statuses.as_ref())
        .map(|list| list.as_slice())
        .unwrap_or(&[]);
    let phase = status
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let ready = phase == "Running"
        && !containers.is_empty()
        && containers.iter().all(|container| container.ready)
        && status
            .and_then(|s| s.conditions.as_ref())
            .and_then(|conditions| {
                conditions
                    .iter()
                    .find(|condition| condition.type_ == "Ready")
            })
            .is_some_and(|condition| condition.status == "True");

    PodSummary {
        name: pod.metadata.name.clone().unwrap_or_default(),
        phase,
        ready,
        restarts: containers.iter().map(|c| c.restart_count).sum(),
        node: pod.spec.as_ref().and_then(|spec| spec.node_name.clone()),
        started_at: status
            .and_then(|s| s.start_time.as_ref())
            .map(|time| time.0.to_string()),
    }
}

pub fn summarise_deployment(deployment: &Deployment, pods: &[Arc<Pod>]) -> WorkloadSummary {
    let spec = deployment.spec.as_ref();
    let containers = spec
        .and_then(|s| s.template.spec.as_ref())
        .map(|pod| pod.containers.as_slice())
        .unwrap_or(&[]);
    let status = deployment.status.as_ref();
    let conditions: Option<Vec<(String, String)>> =
        status.and_then(|s| s.conditions.as_ref()).map(|list| {
            list.iter()
                .map(|c| (c.type_.clone(), c.status.clone()))
                .collect()
        });

    let mut matched: Vec<PodSummary> = pods
        .iter()
        .filter(|pod| selects(deployment, pod))
        .map(|pod| summarise_pod(pod))
        .collect();
    matched.sort_by(|a, b| a.name.cmp(&b.name));

    WorkloadSummary {
        namespace: deployment.metadata.namespace.clone().unwrap_or_default(),
        name: deployment.metadata.name.clone().unwrap_or_default(),
        kind: "Deployment".to_string(),
        role: WorkloadRole::App,
        image: containers
            .first()
            .and_then(|c| c.image.clone())
            .unwrap_or_default(),
        ready_replicas: status.and_then(|s| s.ready_replicas).unwrap_or(0),
        replicas: spec.and_then(|s| s.replicas).unwrap_or(1),
        strategy: spec
            .and_then(|s| s.strategy.as_ref())
            .and_then(|s| s.type_.clone())
            .unwrap_or_else(|| "RollingUpdate".to_string()),
        wants_gpu: containers
            .iter()
            .any(|c| limits_want_accelerator(c.resources.as_ref().and_then(|r| r.limits.as_ref()))),
        devices: container_devices(containers).into_iter().collect(),
        available: condition_true(conditions.as_deref(), "Available"),
        pods: matched,
        vm: None,
    }
}

/// When an event was last seen: the series, the last timestamp, the event time, or creation.
fn event_time(event: &Event) -> Option<Timestamp> {
    event
        .series
        .as_ref()
        .and_then(|series| series.last_observed_time.as_ref().map(|time| time.0))
        .or_else(|| event.last_timestamp.as_ref().map(|time| time.0))
        .or_else(|| event.event_time.as_ref().map(|time| time.0))
        .or_else(|| {
            event
                .metadata
                .creation_timestamp
                .as_ref()
                .map(|time| time.0)
        })
}

pub fn summarise_event(event: &Event) -> EventSummary {
    EventSummary {
        namespace: event.metadata.namespace.clone().unwrap_or_default(),
        kind: event.involved_object.kind.clone().unwrap_or_default(),
        name: event.involved_object.name.clone().unwrap_or_default(),
        reason: event.reason.clone().unwrap_or_default(),
        message: event.message.clone().unwrap_or_default(),
        type_: event.type_.clone().unwrap_or_else(|| "Normal".to_string()),
        count: event
            .series
            .as_ref()
            .and_then(|series| series.count)
            .or(event.count)
            .unwrap_or(1),
        last_seen: event_time(event).map(|time| time.to_string()),
    }
}

/// The newest `MAX_EVENTS` events, newest first; events without any timestamp go last.
pub fn recent_events(events: &[Arc<Event>]) -> Vec<EventSummary> {
    let mut timed: Vec<(Option<Timestamp>, &Event)> = events
        .iter()
        .map(|event| (event_time(event), event.as_ref()))
        .collect();
    timed.sort_by(|a, b| b.0.cmp(&a.0));
    timed
        .into_iter()
        .take(MAX_EVENTS)
        .map(|(_, event)| summarise_event(event))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerators_are_recognised_whoever_makes_them() {
        assert!(is_accelerator("nvidia.com/gpu"));
        assert!(is_accelerator("amd.com/gpu"));
        assert!(is_accelerator("gpu.intel.com/i915"));
        assert!(is_accelerator("habana.ai/gaudi"));
        assert!(is_accelerator("nvidia.com/TU106_GeForce_RTX_2060"));
        assert!(!is_accelerator("devices.kubevirt.io/kvm"));
        assert!(!is_accelerator("hugepages-2Mi"));
        assert!(is_whole_gpu("amd.com/gpu"));
        assert!(!is_whole_gpu("nvidia.com/mig-1g.5gb"));
        let mut capacity = BTreeMap::new();
        capacity.insert("amd.com/gpu".to_string(), Quantity("2".into()));
        capacity.insert("nvidia.com/gpu".to_string(), Quantity("1".into()));
        capacity.insert("nvidia.com/TU106".to_string(), Quantity("1".into()));
        assert_eq!(whole_gpus(Some(&capacity)), 3);
    }
    use k8s_openapi::api::apps::v1::{DeploymentSpec, DeploymentStatus, DeploymentStrategy};
    use k8s_openapi::api::core::v1::{
        Container, ContainerStatus, NodeCondition, NodeStatus, NodeSystemInfo, ObjectReference,
        PodSpec, PodStatus, PodTemplateSpec, ResourceRequirements,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{
        LabelSelector, ObjectMeta, OwnerReference, Time,
    };
    use serde_json::json;

    fn quantity(value: &str) -> Quantity {
        Quantity(value.to_string())
    }

    fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn pod(namespace: &str, name: &str, app: &str, phase: &str, ready: bool) -> Pod {
        owned_pod(namespace, name, app, phase, ready, None)
    }

    fn owned_pod(
        namespace: &str,
        name: &str,
        app: &str,
        phase: &str,
        ready: bool,
        owner: Option<(&str, &str)>,
    ) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                labels: Some(labels(&[("app", app)])),
                owner_references: owner.map(|(kind, owner_name)| {
                    vec![OwnerReference {
                        kind: kind.to_string(),
                        name: owner_name.to_string(),
                        ..OwnerReference::default()
                    }]
                }),
                ..ObjectMeta::default()
            },
            spec: Some(PodSpec {
                node_name: Some("node-a".to_string()),
                ..PodSpec::default()
            }),
            status: Some(PodStatus {
                phase: Some(phase.to_string()),
                conditions: Some(vec![k8s_openapi::api::core::v1::PodCondition {
                    type_: "Ready".into(),
                    status: if ready { "True" } else { "False" }.into(),
                    ..Default::default()
                }]),
                container_statuses: Some(vec![ContainerStatus {
                    ready,
                    restart_count: 2,
                    ..ContainerStatus::default()
                }]),
                ..PodStatus::default()
            }),
        }
    }

    fn service(
        namespace: &str,
        name: &str,
        app: Option<&str>,
        type_: &str,
        node_port: Option<i32>,
    ) -> Service {
        use k8s_openapi::api::core::v1::{ServicePort, ServiceSpec};
        Service {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..ObjectMeta::default()
            },
            spec: Some(ServiceSpec {
                type_: Some(type_.to_string()),
                cluster_ip: Some("10.96.0.10".to_string()),
                selector: app.map(|app| labels(&[("app", app)])),
                ports: Some(vec![ServicePort {
                    name: Some("http".to_string()),
                    port: 11434,
                    target_port: Some(IntOrString::Int(11434)),
                    node_port,
                    protocol: Some("TCP".to_string()),
                    ..ServicePort::default()
                }]),
                ..ServiceSpec::default()
            }),
            status: None,
        }
    }

    #[test]
    fn a_service_reaches_the_workloads_its_selector_matches_by_root() {
        let pods: Vec<Arc<Pod>> = vec![
            Arc::new(owned_pod(
                "ai",
                "ollama-64db946489-hht8v",
                "ollama",
                "Running",
                true,
                Some(("ReplicaSet", "ollama-64db946489")),
            )),
            Arc::new(pod("ai", "other", "other", "Running", true)),
            Arc::new(pod(
                "default",
                "ollama-elsewhere",
                "ollama",
                "Running",
                true,
            )),
        ];
        let summary = summarise_service(
            &service("ai", "ollama", Some("ollama"), "NodePort", Some(31434)),
            &pods,
        );
        assert_eq!(summary.namespace, "ai");
        assert_eq!(summary.type_, "NodePort");
        assert_eq!(summary.cluster_ip.as_deref(), Some("10.96.0.10"));
        assert_eq!(
            summary.targets,
            vec![WorkloadRef {
                kind: "Deployment".to_string(),
                name: "ollama".to_string()
            }]
        );
        assert_eq!(summary.ports.len(), 1);
        assert_eq!(summary.ports[0].node_port, Some(31434));
        assert_eq!(summary.ports[0].target_port, "11434");
        assert_eq!(summary.ports[0].protocol, "TCP");
        assert!(summary.external_ips.is_empty());
    }

    #[test]
    fn a_service_without_a_selector_reaches_nothing_and_headless_has_no_cluster_ip() {
        let pods: Vec<Arc<Pod>> = vec![Arc::new(pod("ai", "ollama-a", "ollama", "Running", true))];
        let mut headless = service("ai", "manual", None, "ClusterIP", None);
        headless.spec.as_mut().expect("spec").cluster_ip = Some("None".to_string());
        let summary = summarise_service(&headless, &pods);
        assert!(summary.targets.is_empty());
        assert_eq!(summary.cluster_ip, None);
        assert_eq!(summary.type_, "ClusterIP");
    }

    #[test]
    fn summarise_lists_services_sorted_by_namespace_and_name() {
        let objects = ClusterObjects {
            pods: vec![Arc::new(pod("ai", "ollama-a", "ollama", "Running", true))],
            services: vec![
                Arc::new(service(
                    "vm",
                    "win11-rdp",
                    Some("win11"),
                    "NodePort",
                    Some(32389),
                )),
                Arc::new(service(
                    "ai",
                    "ollama",
                    Some("ollama"),
                    "NodePort",
                    Some(31434),
                )),
            ],
            ..ClusterObjects::default()
        };
        let summary = summarise(&target(), &objects, &Classifier::default());
        let names: Vec<String> = summary
            .services
            .iter()
            .map(|s| format!("{}/{}", s.namespace, s.name))
            .collect();
        assert_eq!(names, vec!["ai/ollama", "vm/win11-rdp"]);
        assert_eq!(summary.services[0].targets.len(), 1);
        assert!(summary.services[1].targets.is_empty());
    }

    fn gpu_deployment() -> Deployment {
        let mut limits = BTreeMap::new();
        limits.insert("nvidia.com/gpu".to_string(), quantity("1"));
        Deployment {
            metadata: ObjectMeta {
                name: Some("ollama".to_string()),
                namespace: Some("ai".to_string()),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                replicas: Some(1),
                selector: LabelSelector {
                    match_labels: Some(labels(&[("app", "ollama")])),
                    ..LabelSelector::default()
                },
                strategy: Some(DeploymentStrategy {
                    type_: Some("Recreate".to_string()),
                    ..DeploymentStrategy::default()
                }),
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            image: Some("ollama/ollama:0.34.0".to_string()),
                            resources: Some(ResourceRequirements {
                                limits: Some(limits),
                                ..ResourceRequirements::default()
                            }),
                            ..Container::default()
                        }],
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            status: Some(DeploymentStatus {
                ready_replicas: Some(1),
                ..DeploymentStatus::default()
            }),
        }
    }

    fn event(name: &str, type_: &str, seconds_ago: i64) -> Arc<Event> {
        Arc::new(Event {
            metadata: ObjectMeta {
                name: Some(format!("{name}.1")),
                namespace: Some("ai".to_string()),
                ..ObjectMeta::default()
            },
            involved_object: ObjectReference {
                kind: Some("Pod".to_string()),
                name: Some(name.to_string()),
                ..ObjectReference::default()
            },
            reason: Some("Started".to_string()),
            message: Some("Container started".to_string()),
            type_: Some(type_.to_string()),
            count: Some(3),
            last_timestamp: Some(Time(
                Timestamp::now() - k8s_openapi::jiff::SignedDuration::from_secs(seconds_ago),
            )),
            ..Event::default()
        })
    }

    #[test]
    fn parses_gpu_quantities_and_tolerates_junk() {
        assert_eq!(quantity_to_u32(Some(&quantity("1"))), 1);
        assert_eq!(quantity_to_u32(Some(&quantity(" 2 "))), 2);
        assert_eq!(quantity_to_u32(Some(&quantity("lots"))), 0);
        assert_eq!(quantity_to_u32(None), 0);
    }

    #[test]
    fn pod_readiness_honours_readiness_gates() {
        let mut pod = pod("apps", "web", "web", "Running", true);
        assert!(summarise_pod(&pod).ready);
        pod.status
            .as_mut()
            .expect("status")
            .conditions
            .as_mut()
            .expect("conditions")[0]
            .status = "False".into();
        assert!(
            !summarise_pod(&pod).ready,
            "ready containers are insufficient"
        );
        pod.status.as_mut().expect("status").conditions = None;
        assert!(
            !summarise_pod(&pod).ready,
            "missing condition is unknown, not ready"
        );
    }

    #[test]
    fn controller_owner_takes_precedence_over_other_references() {
        let mut pod = pod("apps", "web", "web", "Running", true);
        pod.metadata.owner_references = Some(vec![
            OwnerReference {
                kind: "Other".into(),
                name: "not-controller".into(),
                ..Default::default()
            },
            OwnerReference {
                kind: "StatefulSet".into(),
                name: "database".into(),
                controller: Some(true),
                ..Default::default()
            },
        ]);
        assert_eq!(root_of(&pod), ("StatefulSet".into(), "database".into()));
    }

    #[test]
    fn summarises_a_ready_gpu_node_and_keeps_only_interesting_labels() {
        let mut capacity = BTreeMap::new();
        capacity.insert("nvidia.com/gpu".to_string(), quantity("1"));

        let node = Node {
            metadata: ObjectMeta {
                name: Some("node-a".to_string()),
                labels: Some(labels(&[
                    ("nvidia.com/gpu.present", "true"),
                    ("beta.kubernetes.io/instance-type", "x"),
                ])),
                ..ObjectMeta::default()
            },
            spec: None,
            status: Some(NodeStatus {
                conditions: Some(vec![NodeCondition {
                    type_: "Ready".to_string(),
                    status: "True".to_string(),
                    ..NodeCondition::default()
                }]),
                capacity: Some(capacity.clone()),
                allocatable: Some(capacity),
                node_info: Some(NodeSystemInfo {
                    kubelet_version: "v1.37.0".to_string(),
                    ..NodeSystemInfo::default()
                }),
                ..NodeStatus::default()
            }),
        };

        let summary = summarise_node(&node);
        assert_eq!(summary.name, "node-a");
        assert!(summary.ready);
        assert_eq!(summary.gpu_capacity, 1);
        assert_eq!(summary.gpu_allocatable, 1);
        assert_eq!(summary.kubelet_version, "v1.37.0");
        assert_eq!(summary.labels.len(), 1);
        assert!(summary.labels.contains_key("nvidia.com/gpu.present"));
    }

    #[test]
    fn summarises_a_gpu_deployment_with_the_pods_its_selector_matches() {
        let pods = vec![
            Arc::new(pod("ai", "ollama-b", "ollama", "Running", true)),
            Arc::new(pod("ai", "ollama-a", "ollama", "Succeeded", false)),
            Arc::new(pod("ai", "webui-1", "webui", "Running", true)),
            Arc::new(pod("other", "ollama-c", "ollama", "Running", true)),
        ];
        let summary = summarise_deployment(&gpu_deployment(), &pods);
        assert_eq!(summary.namespace, "ai");
        assert_eq!(summary.image, "ollama/ollama:0.34.0");
        assert_eq!(summary.strategy, "Recreate");
        assert!(summary.wants_gpu);
        assert!(!summary.available);
        assert_eq!(summary.ready_replicas, 1);
        let names: Vec<&str> = summary.pods.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["ollama-a", "ollama-b"],
            "same namespace, sorted"
        );
        assert!(summary.pods[1].ready);
        assert!(!summary.pods[0].ready, "a Succeeded pod is not ready");
        assert_eq!(summary.pods[1].restarts, 2);
        assert_eq!(summary.pods[1].node.as_deref(), Some("node-a"));
    }

    #[test]
    fn a_deployment_without_a_selector_matches_nothing() {
        let mut deployment = gpu_deployment();
        deployment.spec.as_mut().expect("spec").selector = LabelSelector::default();
        let pods = vec![Arc::new(pod("ai", "ollama-a", "ollama", "Running", true))];
        assert!(summarise_deployment(&deployment, &pods).pods.is_empty());
    }

    #[test]
    fn namespace_psa_label_is_read() {
        let namespace = Namespace {
            metadata: ObjectMeta {
                name: Some("ai".to_string()),
                labels: Some(labels(&[(PSA_ENFORCE_LABEL, "privileged")])),
                ..ObjectMeta::default()
            },
            ..Namespace::default()
        };
        let summary = summarise_namespace(&namespace);
        assert_eq!(summary.psa_enforce.as_deref(), Some("privileged"));
    }

    #[test]
    fn recent_events_are_newest_first_and_capped() {
        let mut events: Vec<Arc<Event>> = (0..(MAX_EVENTS as i64 + 5))
            .map(|i| event(&format!("pod-{i}"), "Normal", (i + 1) * 10))
            .collect();
        events.push(event("late-warning", "Warning", 1));
        let recent = recent_events(&events);
        assert_eq!(recent.len(), MAX_EVENTS);
        assert_eq!(recent[0].name, "late-warning");
        assert_eq!(recent[0].type_, "Warning");
        assert_eq!(recent[0].count, 3);
        assert_eq!(recent[1].name, "pod-0");
        assert!(recent[0].last_seen.is_some());
    }

    fn target() -> Target {
        Target {
            context: "lab".into(),
            server: "https://10.0.0.1:6443".into(),
            version: "v1.37.0".into(),
        }
    }

    #[test]
    fn summarise_sorts_everything_by_name() {
        let mut b = gpu_deployment();
        b.metadata.name = Some("zeta".into());
        let mut a = gpu_deployment();
        a.metadata.name = Some("alpha".into());
        let objects = ClusterObjects {
            deployments: vec![Arc::new(b), Arc::new(a)],
            ..ClusterObjects::default()
        };
        let summary = summarise(&target(), &objects, &Classifier::default());
        assert_eq!(summary.context, "lab");
        let names: Vec<&str> = summary.workloads.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
        assert!(summary.events.is_empty());
    }

    #[test]
    fn roots_are_named_the_way_people_name_them() {
        let mut rs = owned_pod(
            "ai",
            "ollama-64db946489-hht8v",
            "ollama",
            "Running",
            true,
            Some(("ReplicaSet", "ollama-64db946489")),
        );
        rs.metadata
            .labels
            .get_or_insert_with(BTreeMap::new)
            .insert("pod-template-hash".into(), "64db946489".into());
        assert_eq!(root_of(&rs), ("Deployment".into(), "ollama".into()));

        let vmi = owned_pod(
            "vm",
            "virt-launcher-win11-znqd5",
            "win11",
            "Running",
            true,
            Some(("VirtualMachineInstance", "win11")),
        );
        assert_eq!(root_of(&vmi), ("VirtualMachine".into(), "win11".into()));

        let static_pod = owned_pod(
            "kube-system",
            "kube-apiserver-node-a",
            "apiserver",
            "Running",
            true,
            Some(("Node", "node-a")),
        );
        assert_eq!(
            root_of(&static_pod),
            ("StaticPod".into(), "kube-apiserver".into())
        );

        let ds = owned_pod(
            "kube-system",
            "kube-proxy-lhbjn",
            "proxy",
            "Running",
            true,
            Some(("DaemonSet", "kube-proxy")),
        );
        assert_eq!(root_of(&ds), ("DaemonSet".into(), "kube-proxy".into()));

        let bare = pod("default", "debug", "debug", "Running", true);
        assert_eq!(root_of(&bare), ("Pod".into(), "debug".into()));
    }

    #[test]
    fn every_pod_becomes_a_workload_and_roles_follow_the_namespace() {
        let ollama_pod = {
            let mut p = owned_pod(
                "ai",
                "ollama-64db946489-hht8v",
                "ollama",
                "Running",
                true,
                Some(("ReplicaSet", "ollama-64db946489")),
            );
            p.metadata
                .labels
                .get_or_insert_with(BTreeMap::new)
                .insert("pod-template-hash".into(), "64db946489".into());
            p
        };
        let objects = ClusterObjects {
            deployments: vec![Arc::new(gpu_deployment())],
            pods: vec![
                Arc::new(ollama_pod),
                Arc::new(owned_pod(
                    "vm",
                    "virt-launcher-win11-znqd5",
                    "win11",
                    "Running",
                    true,
                    Some(("VirtualMachineInstance", "win11")),
                )),
                Arc::new(owned_pod(
                    "kube-system",
                    "kube-proxy-lhbjn",
                    "proxy",
                    "Running",
                    true,
                    Some(("DaemonSet", "kube-proxy")),
                )),
                Arc::new(owned_pod(
                    "kube-system",
                    "kube-apiserver-node-a",
                    "apiserver",
                    "Running",
                    true,
                    Some(("Node", "node-a")),
                )),
                Arc::new(owned_pod(
                    "gpu-operator",
                    "nvidia-device-plugin-daemonset-q2zj4",
                    "plugin",
                    "Running",
                    true,
                    Some(("DaemonSet", "nvidia-device-plugin-daemonset")),
                )),
                Arc::new(owned_pod(
                    "gpu-operator",
                    "nvidia-cuda-validator-tk4zt",
                    "validator",
                    "Succeeded",
                    false,
                    Some(("ClusterPolicy", "cluster-policy")),
                )),
            ],
            vms: vec![Arc::new(kubevirt::tests::object(
                "vm",
                "win11",
                json!({"spec": {"runStrategy": "Always", "template": {"spec": {"domain": {"cpu": {"cores": 8}, "memory": {"guest": "16Gi"}}}}}, "status": {"printableStatus": "Running", "ready": true}}),
            ))],
            vmis: vec![Arc::new(kubevirt::tests::object(
                "vm",
                "win11",
                json!({"status": {"phase": "Running", "nodeName": "node-a", "interfaces": [{"ipAddress": "10.244.0.66"}]}}),
            ))],
            ..ClusterObjects::default()
        };
        let summary = summarise(&target(), &objects, &Classifier::default());
        let describe: Vec<(String, String, String, WorkloadRole)> = summary
            .workloads
            .iter()
            .map(|w| (w.namespace.clone(), w.kind.clone(), w.name.clone(), w.role))
            .collect();
        assert_eq!(
            describe,
            vec![
                (
                    "ai".into(),
                    "Deployment".into(),
                    "ollama".into(),
                    WorkloadRole::App
                ),
                (
                    "gpu-operator".into(),
                    "ClusterPolicy".into(),
                    "cluster-policy".into(),
                    WorkloadRole::Platform
                ),
                (
                    "gpu-operator".into(),
                    "DaemonSet".into(),
                    "nvidia-device-plugin-daemonset".into(),
                    WorkloadRole::Platform
                ),
                (
                    "kube-system".into(),
                    "DaemonSet".into(),
                    "kube-proxy".into(),
                    WorkloadRole::System
                ),
                (
                    "kube-system".into(),
                    "StaticPod".into(),
                    "kube-apiserver".into(),
                    WorkloadRole::System
                ),
                (
                    "vm".into(),
                    "VirtualMachine".into(),
                    "win11".into(),
                    WorkloadRole::App
                ),
            ]
        );
        let ollama = &summary.workloads[0];
        assert_eq!(ollama.pods.len(), 1, "claimed by the Deployment's selector");
        let win11 = summary.workloads.last().expect("vm");
        let vm = win11.vm.as_ref().expect("details");
        assert_eq!(vm.status, "Running");
        assert_eq!(vm.addresses, vec!["10.244.0.66"]);
        assert_eq!(win11.pods.len(), 1);
        assert!(win11.available);
    }

    #[test]
    fn a_stopped_vm_without_a_pod_still_appears() {
        let objects = ClusterObjects {
            vms: vec![Arc::new(kubevirt::tests::object(
                "vm",
                "win11",
                json!({"spec": {"runStrategy": "Halted", "template": {"spec": {"domain": {}}}}, "status": {"printableStatus": "Stopped", "ready": false}}),
            ))],
            ..ClusterObjects::default()
        };
        let summary = summarise(&target(), &objects, &Classifier::default());
        assert_eq!(summary.workloads.len(), 1);
        let vm = &summary.workloads[0];
        assert_eq!(vm.kind, "VirtualMachine");
        assert_eq!(vm.replicas, 0);
        assert!(!vm.available);
        assert_eq!(vm.vm.as_ref().map(|d| d.status.as_str()), Some("Stopped"));
    }

    #[tokio::test]
    async fn missing_kubeconfig_is_not_connected() {
        let error = summary(
            Path::new("/definitely/not/here"),
            None,
            &Classifier::default(),
        )
        .await
        .expect_err("missing");
        assert_eq!(error.code(), "NOT_CONNECTED");
    }
}
