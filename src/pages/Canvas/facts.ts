/**
 * Pure builders that turn API answers into canvas nodes. No React, no store, no IPC:
 * everything here is unit-testable with plain objects. Nothing here knows about any
 * particular cluster; every fact comes from the Talos or Kubernetes API.
 */
import { MarkerType, type Edge, type XYPosition } from '@xyflow/react';
import type {
  ClusterSummary,
  EventSummary,
  GpuMetrics,
  MetricsSample,
  NodeInventory,
  NodeMetrics,
  NodeSummary,
  OllamaStatus,
  PodSummary,
  ServiceSummary,
  TalosInventory,
  WorkloadSummary,
} from '@/services/api';
import { DOCS, type DocLink } from './docs';
import {
  LAYERS,
  LAYER_WIDTH,
  MAX_COLUMNS,
  ROW_HEIGHT,
  SINGLE_ROW_HEIGHTS,
  layerHeight,
  layerPosition,
  type LayerHeights,
  type LayerId,
} from './layers';
import {
  POWER_KEY,
  formatBytes,
  formatWatts,
  gpuGauges,
  gpuKey,
  nodeGauges,
  nodeKey,
  powerGauge,
  usageLabel,
  worstLevel,
} from './metrics';
import {
  layerNodeId,
  type Fact,
  type FactNode,
  type FactNodeData,
  type LayerNode,
} from './nodes/types';
import type { PaletteEntry } from './palette';

/** Card width; `.terrace-node` in styles/layouts/canvas.css must agree. */
export const FACT_WIDTH = 240;
const FACT_GAP = 16;
export const FACT_TOP = 48;
export const FACT_LEFT = 232;
const FACT_RIGHT_MARGIN = 24;
/** A Warning event this recent colours the Events fact. */
const EVENT_WINDOW_MS = 30 * 60 * 1000;
const MAX_EVENT_LINES = 6;
let plannedSequence = 0;

export type SourceResult<T> = { ok: true; value: T } | { ok: false; error: string };

/** The card every Service edge starts from: the cluster's network. */
export const CLUSTER_FACT_ID = 'k8s:cluster';

export function workloadFactId(
  workload: Pick<WorkloadSummary, 'kind' | 'namespace' | 'name'>
): string {
  return `k8s:${workload.kind}:${workload.namespace}/${workload.name}`;
}

export function buildLayerNodes(
  width = LAYER_WIDTH,
  heights: LayerHeights = SINGLE_ROW_HEIGHTS
): LayerNode[] {
  return LAYERS.map((layer) => ({
    id: layerNodeId(layer.id),
    type: 'layer',
    position: layerPosition(layer.id, heights),
    width,
    height: heights[layer.id],
    draggable: false,
    selectable: false,
    connectable: false,
    deletable: false,
    data: { layer: layer.id, label: layer.label, description: layer.description },
  }));
}

function countByLayer(facts: FactNode[]): Map<LayerId, number> {
  const counts = new Map<LayerId, number>();
  for (const node of facts) {
    counts.set(node.data.layer, (counts.get(node.data.layer) ?? 0) + 1);
  }
  return counts;
}

/** Wide enough for the busiest layer's row, never narrower than the default. */
export function requiredLayerWidth(facts: FactNode[]): number {
  const busiest = Math.min(MAX_COLUMNS, Math.max(0, ...countByLayer(facts).values()));
  return Math.max(LAYER_WIDTH, FACT_LEFT + busiest * (FACT_WIDTH + FACT_GAP) + FACT_RIGHT_MARGIN);
}

/** Each band's height for these cards: one row, or more once a row is full. */
export function layerHeights(facts: FactNode[]): LayerHeights {
  const heights = { ...SINGLE_ROW_HEIGHTS };
  for (const [layer, count] of countByLayer(facts)) {
    heights[layer] = layerHeight(Math.ceil(count / MAX_COLUMNS));
  }
  return heights;
}

/** A live card: it reflects the cluster, so it cannot be deleted or wired by hand. */
function fact(id: string, data: FactNodeData): FactNode {
  return {
    id,
    type: 'fact',
    parentId: layerNodeId(data.layer),
    extent: 'parent',
    position: { x: FACT_LEFT, y: FACT_TOP },
    deletable: false,
    data,
  };
}

/** Lay facts out left to right within each layer, in the order given, wrapping into rows. */
export function layoutFacts(facts: FactNode[]): FactNode[] {
  const slots = new Map<LayerId, number>();
  return facts.map((node) => {
    const slot = slots.get(node.data.layer) ?? 0;
    slots.set(node.data.layer, slot + 1);
    const column = slot % MAX_COLUMNS;
    const row = Math.floor(slot / MAX_COLUMNS);
    return {
      ...node,
      position: {
        x: FACT_LEFT + column * (FACT_WIDTH + FACT_GAP),
        y: FACT_TOP + row * ROW_HEIGHT,
      },
    };
  });
}

export function notConnectedFact(layer: LayerId, title: string, message: string): FactNode {
  return fact(`unavailable:${layer}:${title}`, {
    layer,
    title,
    subtitle: 'Not connected',
    status: 'unknown',
    source: 'live',
    facts: [{ label: 'Reason', value: message }],
  });
}

function shortImage(image: string): string {
  const withoutDigest = image.split('@')[0] ?? image;
  const segments = withoutDigest.split('/');
  return segments[segments.length - 1] ?? withoutDigest;
}

function shortId(value: string, length = 8): string {
  return value.length > length ? `${value.slice(0, length)}…` : value;
}

function gibibytes(mib: number): string {
  return `${(mib / 1024).toFixed(0)} GiB`;
}

function gigabytes(bytes: number): string {
  return `${(bytes / 1_000_000_000).toFixed(1)} GB`;
}

function deviceName(path: string): string {
  const segments = path.split('/');
  return segments[segments.length - 1] ?? path;
}

/** "1 Running" or "1 Running, 1 Succeeded"; a Running pod that is not ready is "Not ready". */
export function podsLabel(pods: PodSummary[]): string {
  if (pods.length === 0) return 'none';
  const counts = new Map<string, number>();
  for (const pod of pods) {
    const key = pod.phase === 'Running' && !pod.ready ? 'Not ready' : pod.phase;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts.entries()].map(([phase, count]) => `${String(count)} ${phase}`).join(', ');
}

function withinWindow(iso: string | null, now: number): boolean {
  if (!iso) return false;
  const time = new Date(iso).getTime();
  return !Number.isNaN(time) && now - time <= EVENT_WINDOW_MS;
}

// --- Workloads in plain words --------------------------------------------------

const KIND_LABELS: Record<string, string> = {
  Deployment: 'App',
  ReplicaSet: 'App',
  StatefulSet: 'Stateful app',
  DaemonSet: 'Node agent',
  Job: 'Job',
  CronJob: 'Scheduled job',
  VirtualMachine: 'Virtual machine',
  StaticPod: 'Control plane',
  Pod: 'Pod',
};

/** What a person would call the kind: "Virtual machine", "Node agent", "App". */
export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

function docsForKind(kind: string) {
  switch (kind) {
    case 'DaemonSet':
      return DOCS.k8sDaemonSets;
    case 'StatefulSet':
      return DOCS.k8sStatefulSets;
    case 'Job':
    case 'CronJob':
      return DOCS.k8sJobs;
    case 'StaticPod':
      return DOCS.k8sControlPlane;
    case 'Pod':
      return DOCS.k8sPods;
    default:
      return DOCS.k8sDeployments;
  }
}

/** "Running, 1 of 1 ready", "Completed", "Stopped", "Scaled to zero". */
export function workloadStatusText(workload: WorkloadSummary): string {
  if (workload.vm) return workload.vm.ready ? `${workload.vm.status}, ready` : workload.vm.status;
  const total = workload.pods.length;
  if (total === 0) return workload.replicas > 0 ? 'No pods yet' : 'Scaled to zero';
  const ready = workload.pods.filter((pod) => pod.ready).length;
  const finished = workload.pods.filter((pod) => pod.phase === 'Succeeded').length;
  if (finished === total) return 'Completed';
  if (ready === total) return `Running, ${String(ready)} of ${String(total)} ready`;
  return `${String(ready)} of ${String(total)} ready`;
}

function workloadStatus(workload: WorkloadSummary): FactNodeData['status'] {
  if (workload.available) return 'ok';
  // A machine somebody switched off is not a problem; anything else short of ready is.
  if (workload.vm?.status === 'Stopped') return 'unknown';
  return 'warn';
}

function componentStatusText(workload: WorkloadSummary): string {
  return workload.available ? 'healthy' : workloadStatusText(workload);
}

/** Display names for the platform namespaces people meet most; the namespace otherwise. */
const PLATFORM_NAMES: Record<string, string> = {
  argocd: 'Argo CD',
  'calico-system': 'Calico',
  cdi: 'Containerized Data Importer',
  'cert-manager': 'cert-manager',
  cilium: 'Cilium',
  'flux-system': 'Flux',
  'gpu-operator': 'NVIDIA GPU Operator',
  'ingress-nginx': 'NGINX Ingress',
  'kube-flannel': 'Flannel',
  kubevirt: 'KubeVirt',
  kyverno: 'Kyverno',
  'local-path-storage': 'Local Path Provisioner',
  'longhorn-system': 'Longhorn',
  'metallb-system': 'MetalLB',
  'metrics-server': 'Metrics Server',
  monitoring: 'Monitoring',
  'rook-ceph': 'Rook Ceph',
  tailscale: 'Tailscale',
  velero: 'Velero',
};

export function platformName(namespace: string): string {
  return PLATFORM_NAMES[namespace] ?? namespace;
}

function imageTag(image: string): string | null {
  const withoutDigest = image.split('@')[0] ?? image;
  const tail = withoutDigest.slice(withoutDigest.lastIndexOf('/') + 1);
  const colon = tail.lastIndexOf(':');
  return colon > 0 ? tail.slice(colon + 1) : null;
}

/** The operator's image tag, or the first component's: what "KubeVirt v1.9.0" is made of. */
export function platformVersion(components: WorkloadSummary[]): string | null {
  const operator =
    components.find((component) => component.name.includes('operator')) ??
    components.find((component) => component.kind === 'Deployment') ??
    components[0];
  return operator ? imageTag(operator.image) : null;
}

/** Strip the namespace prefix a component name often repeats, so labels fit on a card. */
function componentLabel(component: WorkloadSummary): string {
  const prefix = `${component.namespace}-`;
  return component.name.startsWith(prefix) && component.name.length > prefix.length
    ? component.name.slice(prefix.length)
    : component.name;
}

function unhealthyFirst(components: WorkloadSummary[]): WorkloadSummary[] {
  return [...components].sort((a, b) => Number(a.available) - Number(b.available));
}

/** The part of a resource name after the vendor: `nvidia.com/gpu` → `gpu`. */
function shortResource(name: string): string {
  const slash = name.indexOf('/');
  return slash >= 0 ? name.slice(slash + 1) : name;
}

/** What a workload holds: "requested" for the ordinary GPU resource, other devices by name. */
export function devicesLabel(workload: Pick<WorkloadSummary, 'devices' | 'wantsGpu'>): string {
  const generic = workload.devices.some(isWholeGpu);
  const others = workload.devices
    .filter((device) => isAccelerator(device) && !isWholeGpu(device))
    .map(shortResource);
  const parts = [...(generic ? ['requested'] : []), ...others];
  return parts.join(', ') || 'no';
}

/** One card per app: a Deployment, a virtual machine, a DaemonSet the user runs… */
export function appFact(
  workload: WorkloadSummary,
  services: ServiceSummary[] = [],
  usage: string | null = null
): FactNode {
  const id = workloadFactId(workload);
  const base = {
    layer: 'workloads' as const,
    title: workload.name,
    subtitle: `${kindLabel(workload.kind)} · ${workload.namespace}`,
    status: workloadStatus(workload),
    source: 'live' as const,
  };
  const { vm } = workload;
  if (vm) {
    const size = [
      vm.cores === null ? null : `${String(vm.cores)} cores`,
      vm.memory ? `${vm.memory} memory` : null,
    ]
      .filter(Boolean)
      .join(', ');
    return fact(id, {
      ...base,
      node: vm.node ?? undefined,
      facts: [
        { label: 'Status', value: workloadStatusText(workload) },
        ...(usage ? [{ label: 'Usage', value: usage }] : []),
        { label: 'Guest OS', value: vm.guestOs ?? 'not reported yet' },
        { label: 'Size', value: size || 'unknown' },
        {
          label: 'GPU',
          value: vm.gpus.length > 0 ? vm.gpus.map(shortResource).join(', ') : 'none',
        },
        { label: 'Address', value: vm.addresses.join(', ') || 'none yet' },
        { label: 'Reachable', value: reachableLabel(workload, services) },
        { label: 'Disks', value: vm.disks.join(', ') || 'none' },
        {
          label: 'Secure Boot',
          value: vm.secureBoot === null ? 'unknown' : vm.secureBoot ? 'on' : 'off',
        },
        { label: 'Run strategy', value: vm.runStrategy ?? 'unknown' },
      ],
      docs: [DOCS.kubevirtVms, DOCS.kubevirtAccess],
    });
  }
  return fact(id, {
    ...base,
    facts: [
      { label: 'Status', value: workloadStatusText(workload) },
      ...(usage ? [{ label: 'Usage', value: usage }] : []),
      { label: 'Image', value: shortImage(workload.image) || 'unknown' },
      { label: 'Pods', value: podsLabel(workload.pods) },
      { label: 'Reachable', value: reachableLabel(workload, services) },
      { label: 'GPU', value: devicesLabel(workload) },
      ...(workload.strategy ? [{ label: 'Update strategy', value: workload.strategy }] : []),
    ],
    docs: [docsForKind(workload.kind)],
  });
}

/** One card per platform namespace: "KubeVirt v1.9.0, 7 components", worst first. */
export function platformFacts(summary: ClusterSummary): FactNode[] {
  const groups = new Map<string, WorkloadSummary[]>();
  for (const workload of summary.workloads) {
    if (workload.role !== 'platform') continue;
    groups.set(workload.namespace, [...(groups.get(workload.namespace) ?? []), workload]);
  }
  return [...groups.entries()].map(([namespace, components]) => {
    const unhealthy = components.filter((component) => !component.available).length;
    const version = platformVersion(components);
    const layer: LayerId = /gpu|nvidia|habana|accelerat/.test(namespace) ? 'gpu' : 'kubernetes';
    return fact(`k8s:platform:${namespace}`, {
      layer,
      title: platformName(namespace),
      subtitle: `Platform${version ? ` ${version}` : ''} · ${String(components.length)} components in ${namespace}`,
      status: unhealthy === 0 ? 'ok' : 'warn',
      source: 'live',
      facts: unhealthyFirst(components).map((component) => ({
        label: componentLabel(component),
        value: componentStatusText(component),
      })),
      docs: [DOCS.k8sNamespaces],
    });
  });
}

/** Kubernetes' own components, one card: control plane, DNS, networking. */
export function systemFact(summary: ClusterSummary): FactNode | null {
  const components = summary.workloads.filter((workload) => workload.role === 'system');
  if (components.length === 0) return null;
  const unhealthy = components.filter((component) => !component.available).length;
  return fact('k8s:system', {
    layer: 'kubernetes',
    title: 'Kubernetes system',
    subtitle:
      unhealthy === 0
        ? `${String(components.length)} components healthy`
        : `${String(unhealthy)} of ${String(components.length)} components unhealthy`,
    status: unhealthy === 0 ? 'ok' : 'warn',
    source: 'live',
    facts: unhealthyFirst(components).map((component) => ({
      label: component.name,
      value: componentStatusText(component),
    })),
    docs: [DOCS.k8sControlPlane],
  });
}

// --- Services as edges ---------------------------------------------------------

export type ServiceExposure = 'internal' | 'node' | 'external';

export interface ServiceEdgeData extends Record<string, unknown> {
  service: ServiceSummary;
  exposure: ServiceExposure;
}

export type ServiceEdge = Edge<ServiceEdgeData>;

/** Where a Service can be reached from: inside the cluster, on every node, or beyond. */
export function serviceExposure(service: ServiceSummary): ServiceExposure {
  if (service.externalIps.length > 0) return 'external';
  if (service.type === 'NodePort' || service.type === 'LoadBalancer') return 'node';
  return 'internal';
}

export function exposureText(exposure: ServiceExposure): string {
  switch (exposure) {
    case 'external':
      return 'Through a load balancer';
    case 'node':
      return 'On configured node addresses (subject to network policy)';
    default:
      return 'Inside the cluster only';
  }
}

/** "11434 → 31434 on the node", "53/UDP", several joined with commas. */
export function servicePortsLabel(service: ServiceSummary): string {
  return service.ports
    .map((port) => {
      const protocol = port.protocol === 'TCP' ? '' : `/${port.protocol}`;
      return port.nodePort === null
        ? `${String(port.port)}${protocol}`
        : `${String(port.port)}${protocol} → ${String(port.nodePort)} on the node`;
    })
    .join(', ');
}

export function servicesFor(
  workload: Pick<WorkloadSummary, 'kind' | 'namespace' | 'name'>,
  services: ServiceSummary[]
): ServiceSummary[] {
  return services.filter(
    (service) =>
      service.namespace === workload.namespace &&
      service.targets.some(
        (target) => target.kind === workload.kind && target.name === workload.name
      )
  );
}

/** "ollama on node port 31434", "inside the cluster as web", or "not exposed". */
export function reachableLabel(
  workload: Pick<WorkloadSummary, 'kind' | 'namespace' | 'name'>,
  services: ServiceSummary[]
): string {
  const mine = servicesFor(workload, services);
  if (mine.length === 0) return 'not exposed';
  return mine
    .map((service) => {
      switch (serviceExposure(service)) {
        case 'external':
          return `${service.name} at ${service.externalIps.join(', ')}`;
        case 'node': {
          const nodePorts = service.ports
            .filter((port) => port.nodePort !== null)
            .map((port) => String(port.nodePort));
          return `${service.name} on node port ${nodePorts.join(', ')}`;
        }
        default:
          return `inside the cluster as ${service.name}`;
      }
    })
    .join('; ');
}

/** "12, 2 reachable from outside" for the Cluster card. */
export function servicesCountLabel(services: ServiceSummary[]): string {
  if (services.length === 0) return 'none';
  const outside = services.filter((service) => serviceExposure(service) !== 'internal').length;
  return outside === 0
    ? `${String(services.length)}, none configured for outside access`
    : `${String(services.length)}, ${String(outside)} configured for outside access`;
}

/**
 * One edge per Service and app it selects, from the Cluster card up to the app's card,
 * labelled with the Service and its ports. Platform and system Services stay folded into
 * their cards; a Service that reaches nothing draws nothing.
 */
export function serviceEdges(summary: ClusterSummary): ServiceEdge[] {
  const apps = new Map<string, WorkloadSummary>();
  for (const workload of summary.workloads) {
    if (workload.role === 'app') apps.set(workloadFactId(workload), workload);
  }
  const edges: ServiceEdge[] = [];
  for (const service of summary.services) {
    const exposure = serviceExposure(service);
    for (const target of service.targets) {
      const targetId = workloadFactId({ ...target, namespace: service.namespace });
      if (!apps.has(targetId)) continue;
      edges.push({
        id: `svc:${service.namespace}/${service.name}->${targetId}`,
        source: CLUSTER_FACT_ID,
        sourceHandle: 'up',
        target: targetId,
        targetHandle: 'down',
        deletable: false,
        type: 'smoothstep',
        label: `${service.name} ${servicePortsLabel(service)}`.trim(),
        className: `terrace-edge terrace-edge--${exposure}`,
        markerEnd: { type: MarkerType.ArrowClosed },
        data: { service, exposure },
      });
    }
  }
  return edges;
}

// --- Kubernetes ---------------------------------------------------------------

export function clusterFacts(
  summary: ClusterSummary,
  metrics: MetricsSample | null = null
): FactNode[] {
  const nodes: FactNode[] = [];

  for (const node of summary.nodes) {
    const internal = node.addresses.find((address) => address.kind === 'InternalIP');
    nodes.push(
      fact(`k8s:node:${node.name}`, {
        layer: 'kubernetes',
        title: node.name,
        subtitle: `kubelet ${node.kubeletVersion}`,
        status: node.ready ? 'ok' : 'error',
        source: 'live',
        node: node.name,
        facts: [
          { label: 'Ready', value: node.ready ? 'yes' : 'no' },
          { label: 'OS', value: node.osImage },
          { label: 'Runtime', value: node.containerRuntime },
          { label: 'Address', value: internal?.address ?? 'unknown' },
          { label: 'Taints', value: node.taints.length > 0 ? node.taints.join(', ') : 'none' },
        ],
        docs: [DOCS.k8sNodes],
      })
    );
  }

  nodes.push(...gpuFacts(summary, metrics));

  const privileged = summary.namespaces
    .filter((namespace) => namespace.psaEnforce === 'privileged')
    .map((namespace) => namespace.name);
  nodes.push(
    fact(CLUSTER_FACT_ID, {
      layer: 'kubernetes',
      title: 'Cluster',
      subtitle: `${summary.version} · ${String(summary.namespaces.length)} namespaces`,
      status: 'ok',
      source: 'live',
      facts: [
        { label: 'API server', value: summary.version },
        { label: 'Endpoint', value: summary.server },
        { label: 'Namespaces', value: String(summary.namespaces.length) },
        { label: 'Services', value: servicesCountLabel(summary.services) },
        { label: 'Privileged', value: privileged.join(', ') || 'none' },
      ],
      docs: [DOCS.k8sNamespaces],
    })
  );

  // Your things get cards; the cluster's machinery folds into one card per namespace.
  for (const workload of summary.workloads) {
    if (workload.role !== 'app') continue;
    const usage = metrics
      ? usageLabel(
          metrics.pods,
          workload.namespace,
          workload.pods.map((pod) => pod.name)
        )
      : null;
    nodes.push(appFact(workload, summary.services, usage));
  }
  nodes.push(...platformFacts(summary));
  const system = systemFact(summary);
  if (system) nodes.push(system);

  const events = eventsFact(summary);
  if (events) nodes.push(events);
  nodes.push(podSecurityFact(summary));
  const pinning = imagePinningFact(summary);
  if (pinning) nodes.push(pinning);

  return nodes;
}

// --- Accelerators ------------------------------------------------------------------

const KUBEVIRT_DEVICE_PREFIX = 'devices.kubevirt.io/';

/** An extended resource worth a card: a vendor device, not KubeVirt's own plugins. */
export function isAccelerator(resource: string): boolean {
  return resource.includes('/') && !resource.startsWith(KUBEVIRT_DEVICE_PREFIX);
}

/** A whole GPU as the scheduler names it (`nvidia.com/gpu`, `amd.com/gpu`), not a slice. */
export function isWholeGpu(resource: string): boolean {
  return resource.endsWith('/gpu');
}

/** The workloads asking for a resource by name. */
export function holdersOf(
  summary: ClusterSummary,
  resource: string,
  node: string
): WorkloadSummary[] {
  return summary.workloads.filter(
    (workload) =>
      workload.devices.includes(resource) &&
      workload.pods.some(
        (pod) => pod.node === node && pod.phase !== 'Succeeded' && pod.phase !== 'Failed'
      )
  );
}

/** Where to read more about a device, by who makes it. */
function acceleratorDocs(resource: string): DocLink[] {
  return resource.startsWith('nvidia.com/') ? [DOCS.gpuOperator, DOCS.talosNvidia] : [DOCS.k8sGpus];
}

export function holdersLabel(holders: WorkloadSummary[]): string {
  return (
    holders.map((workload) => (workload.vm ? `${workload.name} (VM)` : workload.name)).join(', ') ||
    'nobody'
  );
}

function vramLabel(gpu: GpuMetrics): string {
  if (gpu.memoryUsedBytes === null)
    return gpu.memoryTotalBytes === null ? 'unknown' : formatBytes(gpu.memoryTotalBytes);
  return `${formatBytes(gpu.memoryUsedBytes)} of ${gpu.memoryTotalBytes === null ? '?' : formatBytes(gpu.memoryTotalBytes)}`;
}

/** Driver facts from the labels a vendor's operator sets; nothing when there are none. */
function driverFacts(node: NodeSummary, resource: string): Fact[] {
  if (!resource.startsWith('nvidia.com/')) return [];
  const driver = node.labels['nvidia.com/cuda.driver-version.full'];
  const cuda = node.labels['nvidia.com/cuda.runtime-version.full'];
  return [
    ...(driver ? [{ label: 'Driver', value: driver }] : []),
    ...(cuda ? [{ label: 'CUDA', value: cuda }] : []),
  ];
}

/**
 * The GPU layer: one card per physical GPU an exporter reports, with dials, and one card per
 * extended resource a node advertises, with who holds it. When a node has exactly one reported
 * GPU and exactly one schedulable whole GPU (`nvidia.com/gpu`, `amd.com/gpu`), they are the same card. A device a
 * virtual machine holds through vfio is invisible to the exporter and appears as its resource.
 */
export function gpuFacts(summary: ClusterSummary, metrics: MetricsSample | null): FactNode[] {
  const facts: FactNode[] = [];
  const placed = new Set<string>();
  const reported = metrics?.gpus ?? [];

  const deviceCard = (
    gpu: GpuMetrics,
    node: NodeSummary | null,
    resource: string | null,
    holders: WorkloadSummary[] | null
  ): FactNode => {
    const gauges = gpuGauges(gpu);
    const where = node ? ` on ${node.name}` : '';
    const subtitle =
      holders && resource
        ? `${resource}${where} · held by ${holdersLabel(holders)}`
        : `GPU ${gpu.index}${where}`;
    return fact(`gpu:device:${gpu.uuid}`, {
      layer: 'gpu',
      title: gpu.model,
      subtitle,
      status: worstLevel(gauges),
      source: 'live',
      node: node?.name,
      gauges,
      metricsKey: gpuKey(gpu.uuid),
      facts: [
        ...(holders ? [{ label: 'Held by', value: holdersLabel(holders) }] : []),
        { label: 'VRAM', value: vramLabel(gpu) },
        ...(node && resource ? driverFacts(node, resource) : []),
        { label: 'PCI', value: gpu.pciBusId || 'unknown' },
        ...(gpu.smClockMhz === null
          ? []
          : [{ label: 'Clock', value: `${String(Math.round(gpu.smClockMhz))} MHz` }]),
      ],
      docs: acceleratorDocs(resource ?? 'nvidia.com/gpu'),
    });
  };

  for (const node of summary.nodes) {
    const onNode = reported.filter((gpu) => gpu.node === node.name);
    const accelerators = Object.entries(node.devices).filter(([name]) => isAccelerator(name));
    const generic = accelerators.find(([name]) => isWholeGpu(name));
    let merged: string | null = null;
    if (onNode.length === 1 && generic?.[1].capacity === 1) {
      const gpu = onNode[0];
      if (gpu) {
        placed.add(gpu.uuid);
        facts.push(deviceCard(gpu, node, generic[0], holdersOf(summary, generic[0], node.name)));
        merged = generic[0];
      }
    }
    for (const gpu of onNode) {
      if (placed.has(gpu.uuid)) continue;
      placed.add(gpu.uuid);
      facts.push(deviceCard(gpu, node, null, null));
    }
    for (const [name, count] of accelerators) {
      if (name === merged) continue;
      const holders = holdersOf(summary, name, node.name);
      const vmOnly = holders.length > 0 && holders.every((workload) => workload.vm !== null);
      facts.push(
        fact(`gpu:${node.name}:${name}`, {
          layer: 'gpu',
          title: name,
          subtitle: `${String(count.allocatable)} allocatable of ${String(count.capacity)} on ${node.name}`,
          status: count.capacity > 0 ? 'ok' : 'warn',
          source: 'live',
          node: node.name,
          facts: [
            { label: 'Held by', value: holdersLabel(holders) },
            ...(vmOnly ? [{ label: 'Access', value: 'whole device, passed through' }] : []),
            ...(isWholeGpu(name) ? driverFacts(node, name) : []),
          ],
          docs: acceleratorDocs(name),
        })
      );
    }
  }

  // GPUs the exporter reports without saying which node they are on.
  for (const gpu of reported) {
    if (placed.has(gpu.uuid)) continue;
    placed.add(gpu.uuid);
    facts.push(deviceCard(gpu, null, null, null));
  }
  return facts;
}

/** Every GPU's power draw on one card, against the budget from Settings when one is set. */
export function powerFact(metrics: MetricsSample, budgetWatts: number | null): FactNode | null {
  const gauge = powerGauge(metrics, budgetWatts);
  if (!gauge) return null;
  return fact('power:gpus', {
    layer: 'hardware',
    title: 'Power',
    subtitle:
      budgetWatts === null
        ? 'GPU draw; set a budget in Settings to see headroom'
        : `GPU draw against a ${formatWatts(budgetWatts)} budget`,
    status: gauge.level,
    source: 'live',
    gauges: [gauge],
    metricsKey: POWER_KEY,
    facts: metrics.gpus
      .filter((gpu) => gpu.powerWatts !== null)
      .map((gpu) => ({ label: gpu.model, value: formatWatts(gpu.powerWatts ?? 0) })),
    docs: [DOCS.talosNvidia],
  });
}

/** Recent events, warnings first, one line per object; warns when a Warning is fresh. */
export function eventsFact(summary: ClusterSummary, now = Date.now()): FactNode | null {
  if (summary.events.length === 0) return null;
  const freshWarnings = summary.events.filter(
    (event) => event.type === 'Warning' && withinWindow(event.lastSeen, now)
  ).length;
  const byObject = new Map<string, EventSummary>();
  const ordered = [...summary.events].sort(
    (a, b) => Number(a.type !== 'Warning') - Number(b.type !== 'Warning')
  );
  for (const event of ordered) {
    const key = `${event.namespace}/${event.kind}/${event.name}`;
    if (!byObject.has(key)) byObject.set(key, event);
  }
  return fact('k8s:events', {
    layer: 'kubernetes',
    title: 'Events',
    subtitle:
      freshWarnings > 0
        ? `${String(freshWarnings)} warning${freshWarnings === 1 ? '' : 's'} in the last 30 min`
        : `${String(summary.events.length)} recent, no fresh warnings`,
    status: freshWarnings > 0 ? 'warn' : 'ok',
    source: 'live',
    facts: [...byObject.entries()].slice(0, MAX_EVENT_LINES).map(([label, event]) => ({
      label,
      value: event.count > 1 ? `${event.reason} ×${String(event.count)}` : event.reason,
    })),
    docs: [DOCS.k8sEvents],
  });
}

export function podSecurityFact(summary: ClusterSummary): FactNode {
  const levels = new Map<string, number>();
  let labelled = 0;
  for (const namespace of summary.namespaces) {
    const level = namespace.psaEnforce ?? 'unlabelled';
    if (namespace.psaEnforce) labelled += 1;
    levels.set(level, (levels.get(level) ?? 0) + 1);
  }
  const total = summary.namespaces.length;
  return fact('trust:psa', {
    layer: 'trust',
    title: 'Pod Security',
    subtitle: `${String(labelled)} of ${String(total)} namespaces labelled`,
    status: total === 0 ? 'unknown' : labelled === total ? 'ok' : labelled > 0 ? 'warn' : 'unknown',
    source: 'live',
    facts: [...levels.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([level, count]) => ({ label: level, value: String(count) })),
    docs: [DOCS.k8sPodSecurity],
  });
}

/** Only the primary container image is carried by the summary; sidecars and init containers are not assessed. */
export function imagePinningFact(summary: ClusterSummary): FactNode | null {
  const withImage = (workload: WorkloadSummary) =>
    workload.image !== '' && workload.kind !== 'VirtualMachine';
  const apps = summary.workloads.filter(
    (workload) => workload.role === 'app' && withImage(workload)
  );
  if (apps.length === 0) return null;
  const pinned = (list: WorkloadSummary[]) =>
    list.filter((workload) => workload.image.includes('@sha256:')).length;
  const unpinned = apps.filter((workload) => !workload.image.includes('@sha256:'));
  const platform = summary.workloads.filter(
    (workload) => workload.role === 'platform' && withImage(workload)
  );
  return fact('trust:images', {
    layer: 'trust',
    title: 'Primary image pinning',
    subtitle: `${String(pinned(apps))} of ${String(apps.length)} primary app images by digest`,
    status: unpinned.length === 0 ? 'ok' : 'warn',
    source: 'live',
    facts: [
      ...(unpinned.length > 0
        ? unpinned.slice(0, 4).map((workload) => ({ label: workload.name, value: 'by tag' }))
        : [{ label: 'All primary app images', value: 'pinned by digest' }]),
      ...(platform.length > 0
        ? [
            {
              label: 'Primary platform images',
              value: `${String(pinned(platform))} of ${String(platform.length)} by digest`,
            },
          ]
        : []),
    ],
    docs: [DOCS.k8sImages],
  });
}

// --- Talos ---------------------------------------------------------------------

function nodeLabel(node: NodeInventory): string {
  return node.hostname ?? node.node;
}

function machineStatus(node: NodeInventory): FactNodeData['status'] {
  if (node.ready === true && node.stage === 'running') return 'ok';
  if (node.ready === false || (typeof node.stage === 'string' && node.stage !== 'running')) {
    return 'warn';
  }
  return 'unknown';
}

export function hardwareFacts(node: NodeInventory, live: NodeMetrics | null = null): FactNode[] {
  const label = nodeLabel(node);
  const facts: FactNode[] = [];
  const cpu = node.processors[0];
  const gauges = live ? nodeGauges(live) : [];

  facts.push(
    fact(`hw:system:${node.node}`, {
      layer: 'hardware',
      title: label,
      subtitle:
        [node.system?.manufacturer, node.system?.product].filter(Boolean).join(' ') || 'System',
      status:
        machineStatus(node) === 'ok' && gauges.length > 0
          ? worstLevel(gauges)
          : machineStatus(node),
      source: 'live',
      node: node.node,
      gauges: gauges.length > 0 ? gauges : undefined,
      metricsKey: nodeKey(live?.node ?? label),
      facts: [
        {
          label: 'CPU',
          value: cpu
            ? `${cpu.product ?? 'unknown'}${cpu.cores ? `, ${String(cpu.cores)} cores` : ''}`
            : 'unknown',
        },
        { label: 'Memory', value: node.memoryMib > 0 ? gibibytes(node.memoryMib) : 'unknown' },
        { label: 'Firmware', value: node.system?.biosVersion ?? node.system?.version ?? 'unknown' },
        { label: 'Address', value: node.node },
      ],
      docs: [DOCS.talosArchitecture],
    })
  );

  if (node.disks.length > 0) {
    facts.push(
      fact(`hw:disks:${node.node}`, {
        layer: 'hardware',
        title: 'Disks',
        subtitle: `${String(node.disks.length)} on ${label}`,
        status: 'ok',
        source: 'live',
        node: node.node,
        facts: node.disks.map((disk) => ({
          label: deviceName(disk.device),
          value:
            [disk.model ?? disk.transport, disk.prettySize].filter(Boolean).join(', ') || 'unknown',
        })),
        docs: [DOCS.talosDisks],
      })
    );
  }

  if (node.links.length > 0 || node.addresses.length > 0) {
    const up = node.links.filter((link) => link.up).length;
    facts.push(
      fact(`hw:network:${node.node}`, {
        layer: 'hardware',
        title: 'Network',
        subtitle: `${String(up)} of ${String(node.links.length)} links up on ${label}`,
        // An unplugged second port or an idle bond is normal; no link up at all is not.
        status: node.links.length === 0 ? 'unknown' : up > 0 ? 'ok' : 'warn',
        source: 'live',
        node: node.node,
        facts: [
          ...node.links.map((link) => ({
            label: link.name,
            value: `${link.up ? 'up' : 'down'}${link.speedMbit ? `, ${String(link.speedMbit)} Mbit` : ''}${link.driver ? ` (${link.driver})` : ''}`,
          })),
          ...node.addresses.map((address) => ({ label: address.link, value: address.address })),
        ],
        docs: [DOCS.talosNetwork],
      })
    );
  }

  if (node.gpus.length > 0) {
    const first = node.gpus[0];
    facts.push(
      fact(`hw:gpu:${node.node}`, {
        layer: 'hardware',
        title: first?.product ?? 'GPU',
        subtitle: `${first?.vendor ?? 'PCI'} on ${label}`,
        // Talos enumerates PCI devices before the driver modules load, so the bound driver
        // is often absent; the Kubernetes GPU fact carries the driver version instead.
        status: 'ok',
        source: 'live',
        node: node.node,
        facts: [
          { label: 'Driver', value: first?.driver ?? 'not reported' },
          { label: 'Class', value: first?.class ?? 'unknown' },
          { label: 'Devices', value: String(node.gpus.length) },
        ],
        docs: [DOCS.talosNvidia],
      })
    );
  }

  return facts;
}

export function talosLayerFacts(node: NodeInventory, serverVersion: string | null): FactNode[] {
  const label = nodeLabel(node);
  const extensions = node.extensions.filter((extension) => extension.name !== 'schematic');
  const facts: FactNode[] = [
    fact(`talos:node:${node.node}`, {
      layer: 'talos',
      title: `Talos ${node.version ?? serverVersion ?? ''}`.trim(),
      subtitle: label,
      status: machineStatus(node),
      source: 'live',
      node: node.node,
      facts: [
        { label: 'Stage', value: node.stage ?? 'unknown' },
        {
          label: 'Ready',
          value: typeof node.ready !== 'boolean' ? 'unknown' : node.ready ? 'yes' : 'no',
        },
        { label: 'Schematic', value: node.schematic ? shortId(node.schematic) : 'none' },
        { label: 'Extensions', value: String(extensions.length) },
      ],
      docs: [DOCS.talosUpgrade, DOCS.imageFactory],
    }),
  ];

  if (extensions.length > 0) {
    facts.push(
      fact(`talos:extensions:${node.node}`, {
        layer: 'talos',
        title: 'Extensions',
        subtitle: `${String(extensions.length)} on ${label}`,
        status: 'ok',
        source: 'live',
        node: node.node,
        facts: extensions.map((extension) => ({
          label: extension.name,
          value: extension.version ?? 'unknown',
        })),
        docs: [DOCS.talosExtensions],
      })
    );
  }

  return facts;
}

const SYSTEM_VOLUMES = ['STATE', 'EPHEMERAL'];

export function trustFacts(node: NodeInventory): FactNode[] {
  const label = nodeLabel(node);
  const security = node.security;
  const facts: FactNode[] = [
    fact(`trust:boot:${node.node}`, {
      layer: 'trust',
      title: 'Boot',
      subtitle: label,
      status:
        security.secureBoot === true ? 'ok' : security.secureBoot === false ? 'warn' : 'unknown',
      source: 'live',
      node: node.node,
      facts: [
        {
          label: 'Secure Boot',
          value:
            typeof security.secureBoot !== 'boolean'
              ? 'unknown'
              : security.secureBoot
                ? 'on'
                : 'off',
        },
        {
          label: 'UKI',
          value:
            typeof security.bootedWithUki !== 'boolean'
              ? 'unknown'
              : security.bootedWithUki
                ? 'yes'
                : 'no',
        },
        {
          label: 'Module signing',
          value:
            typeof security.moduleSignatureEnforced !== 'boolean'
              ? 'unknown'
              : security.moduleSignatureEnforced
                ? 'enforced'
                : 'off',
        },
        { label: 'SELinux', value: security.selinux ?? 'unknown' },
        {
          label: 'FIPS',
          value: typeof security.fips !== 'boolean' ? 'unknown' : security.fips ? 'on' : 'off',
        },
      ],
      docs: [DOCS.talosSecureBoot],
    }),
  ];

  if (node.volumes.length > 0) {
    const system = node.volumes.filter((volume) => SYSTEM_VOLUMES.includes(volume.name));
    const encrypted = system.filter((volume) => Boolean(volume.encryption));
    facts.push(
      fact(`trust:encryption:${node.node}`, {
        layer: 'trust',
        title: 'Disk encryption',
        subtitle: `${String(encrypted.length)} of ${String(system.length)} system volumes on ${label}`,
        status:
          system.length === 0 ? 'unknown' : encrypted.length === system.length ? 'ok' : 'warn',
        source: 'live',
        node: node.node,
        facts: node.volumes.map((volume) => ({
          label: volume.name,
          value: `${volume.encryption ?? 'none'}${volume.filesystem ? `, ${volume.filesystem}` : ''}`,
        })),
        docs: [DOCS.talosDiskEncryption],
      })
    );
  }

  return facts;
}

/** The metrics entry for a Talos node: by hostname (the Kubernetes node name) or address. */
export function liveFor(node: NodeInventory, metrics: MetricsSample | null): NodeMetrics | null {
  if (!metrics) return null;
  return (
    metrics.nodes.find((entry) => entry.node === node.hostname || entry.node === node.node) ?? null
  );
}

export function talosFacts(
  inventory: TalosInventory,
  metrics: MetricsSample | null = null
): FactNode[] {
  const facts: FactNode[] = [];
  for (const node of inventory.nodes) {
    facts.push(
      ...hardwareFacts(node, liveFor(node, metrics)),
      ...talosLayerFacts(node, inventory.version.server),
      ...trustFacts(node)
    );
  }
  if (inventory.warnings.length > 0) {
    facts.push(
      fact('talos:warnings', {
        layer: 'talos',
        title: 'Unread resources',
        subtitle: `${String(inventory.warnings.length)} could not be read`,
        status: 'warn',
        source: 'live',
        facts: inventory.warnings.slice(0, 6).map((warning) => {
          const [name, ...rest] = warning.split(': ');
          return { label: name ?? 'resource', value: rest.join(': ') || warning };
        }),
        docs: [DOCS.talosArchitecture],
      })
    );
  }
  return facts;
}

// --- Optional integrations ---------------------------------------------------

export function ollamaFacts(status: OllamaStatus): FactNode[] {
  const loaded = status.loaded[0];
  return [
    fact('ollama:server', {
      layer: 'workloads',
      title: 'Ollama',
      subtitle: `v${status.version} at ${status.url}`,
      status: 'ok',
      source: 'live',
      facts: [
        { label: 'Models', value: String(status.models.length) },
        {
          label: 'Loaded',
          value: loaded ? `${loaded.name} (${gigabytes(loaded.sizeVram)} VRAM)` : 'none',
        },
      ],
      docs: [DOCS.ollamaApi],
    }),
  ];
}

export function plannedFact(entry: PaletteEntry, position: XYPosition): FactNode {
  return {
    id: `planned:${entry.id}:${String(++plannedSequence)}`,
    type: 'fact',
    parentId: layerNodeId('workloads'),
    extent: 'parent',
    position,
    data: {
      layer: 'workloads',
      title: entry.label,
      subtitle: 'planned',
      status: 'planned',
      source: 'planned',
      facts: [
        { label: 'Image', value: entry.image || 'to choose' },
        { label: 'GPU', value: entry.wantsGpu ? 'wants one' : 'no' },
      ],
      docs: [DOCS.k8sDeployments],
    },
  };
}

/** Everything the canvas shows apart from the layer bands and the planned nodes. */
export function buildFacts(
  cluster: SourceResult<ClusterSummary>,
  talos: SourceResult<TalosInventory>,
  ollama: SourceResult<OllamaStatus | null>,
  metrics: MetricsSample | null = null,
  powerBudgetWatts: number | null = null
): FactNode[] {
  const facts: FactNode[] = [];

  if (cluster.ok) {
    facts.push(...clusterFacts(cluster.value, metrics));
  } else {
    facts.push(
      notConnectedFact('kubernetes', 'Kubernetes API', cluster.error),
      notConnectedFact('gpu', 'GPU', 'Needs the Kubernetes API'),
      notConnectedFact('workloads', 'Workloads', 'Needs the Kubernetes API')
    );
  }

  if (talos.ok) {
    facts.push(...talosFacts(talos.value, metrics));
  } else {
    facts.push(
      notConnectedFact('hardware', 'Hardware', talos.error),
      notConnectedFact('talos', 'Talos API', talos.error),
      notConnectedFact('trust', 'Boot and encryption', 'Needs the Talos API')
    );
  }

  if (ollama.ok) {
    if (ollama.value) facts.push(...ollamaFacts(ollama.value));
  } else {
    facts.push(notConnectedFact('workloads', 'Ollama', ollama.error));
  }

  if (metrics) {
    const power = powerFact(metrics, powerBudgetWatts);
    if (power) facts.push(power);
  }

  return layoutFacts(facts);
}
