/**
 * The services layer. Every Tauri command is called from here and nowhere else;
 * components never import `invoke`. Pattern lifted from Modz (NoWayJA/Modz, MIT)
 * services/api.ts: namespace objects, typed wrappers, camelCase payloads.
 */
import { invoke } from '@tauri-apps/api/core';
import { toConsoleError } from './errors';

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error: unknown) {
    throw toConsoleError(error, command);
  }
}

/** For commands that return unit on the Rust side (serialised as null). */
async function callVoid(command: string, args?: Record<string, unknown>): Promise<void> {
  await call<null>(command, args);
}

// --- App -----------------------------------------------------------------

export interface AppInfo {
  name: string;
  version: string;
  platform: string;
  arch: string;
  profile: 'debug' | 'release';
  dataDir: string;
  logDir: string;
}

// --- Settings ------------------------------------------------------------

export interface Setting {
  key: string;
  value: string;
  /** True when the value shown is the built-in default, not something stored. */
  isDefault: boolean;
}

// --- Kubernetes ----------------------------------------------------------

export interface NodeAddress {
  kind: string;
  address: string;
}

/** How many of an extended resource (`vendor.com/device`) a node has and can still hand out. */
export interface DeviceCount {
  capacity: number;
  allocatable: number;
}

export interface NodeSummary {
  name: string;
  ready: boolean;
  kubeletVersion: string;
  osImage: string;
  kernelVersion: string;
  containerRuntime: string;
  gpuCapacity: number;
  gpuAllocatable: number;
  /** Every extended resource the node advertises, by name. */
  devices: Record<string, DeviceCount>;
  addresses: NodeAddress[];
  taints: string[];
  labels: Record<string, string>;
}

export interface NamespaceSummary {
  name: string;
  psaEnforce: string | null;
}

export interface PodSummary {
  name: string;
  phase: string;
  ready: boolean;
  restarts: number;
  node: string | null;
  startedAt: string | null;
}

/** Who a workload belongs to: the user (app), the cluster's machinery (platform), Kubernetes itself. */
export type WorkloadRole = 'app' | 'platform' | 'system';

/** A KubeVirt VirtualMachine, combined with its running instance when there is one. */
export interface VmDetails {
  /** Running, Stopped, Starting, Stopping, Provisioning… */
  status: string;
  ready: boolean;
  runStrategy: string | null;
  cores: number | null;
  memory: string | null;
  guestOs: string | null;
  addresses: string[];
  node: string | null;
  disks: string[];
  secureBoot: boolean | null;
  /** Resource names of the GPUs and host devices the machine is given. */
  gpus: string[];
}

export interface WorkloadSummary {
  namespace: string;
  name: string;
  /** Deployment, DaemonSet, StatefulSet, Job, VirtualMachine, StaticPod, Pod, or a custom kind. */
  kind: string;
  role: WorkloadRole;
  image: string;
  readyReplicas: number;
  replicas: number;
  /** Deployments only; empty otherwise. */
  strategy: string;
  wantsGpu: boolean;
  /** Every extended resource its containers ask for (a stopped VM: what its spec declares). */
  devices: string[];
  available: boolean;
  /** The Pods this workload owns, by name. */
  pods: PodSummary[];
  vm: VmDetails | null;
}

export interface EventSummary {
  namespace: string;
  kind: string;
  name: string;
  reason: string;
  message: string;
  type: string;
  count: number;
  lastSeen: string | null;
}

export interface ServicePortSummary {
  name: string | null;
  port: number;
  targetPort: string;
  nodePort: number | null;
  protocol: string;
}

export interface WorkloadRef {
  kind: string;
  name: string;
}

/** A Service: the edge from the cluster's network to the workloads it selects. */
export interface ServiceSummary {
  namespace: string;
  name: string;
  /** ClusterIP, NodePort, LoadBalancer or ExternalName. */
  type: string;
  /** Null for headless Services. */
  clusterIp: string | null;
  ports: ServicePortSummary[];
  externalIps: string[];
  /** Workloads in the same namespace whose pods match the selector. */
  targets: WorkloadRef[];
}

export interface ClusterSummary {
  context: string;
  server: string;
  version: string;
  nodes: NodeSummary[];
  namespaces: NamespaceSummary[];
  workloads: WorkloadSummary[];
  services: ServiceSummary[];
  /** Newest first, at most forty. */
  events: EventSummary[];
}

// --- Talos ---------------------------------------------------------------

export interface NodeVersion {
  node: string;
  tag: string;
}

export interface TalosVersion {
  client: string;
  /** The first node's version; mid-upgrade the nodes differ, see `nodes`. */
  server: string | null;
  node: string | null;
  nodes: NodeVersion[];
}

export interface SecurityState {
  secureBoot?: boolean | null;
  bootedWithUki?: boolean | null;
  moduleSignatureEnforced?: boolean | null;
  selinux?: string | null;
  fips?: boolean | null;
}

export interface SystemInfo {
  manufacturer?: string | null;
  product?: string | null;
  version?: string | null;
  biosVersion?: string | null;
}

export interface Processor {
  product?: string | null;
  cores?: number | null;
  threads?: number | null;
}

export interface Disk {
  device: string;
  model?: string | null;
  serial?: string | null;
  sizeBytes?: number | null;
  prettySize?: string | null;
  transport?: string | null;
  rotational?: boolean | null;
}

export interface Link {
  name: string;
  up: boolean;
  kind?: string | null;
  driver?: string | null;
  speedMbit?: number | null;
  hardwareAddr?: string | null;
}

export interface Address {
  link: string;
  address: string;
}

export interface Extension {
  name: string;
  version?: string | null;
}

export interface Volume {
  name: string;
  phase?: string | null;
  /** `partition` or `disk`; directories and overlays are not listed. */
  volumeType?: string | null;
  filesystem?: string | null;
  encryption?: string | null;
  device?: string | null;
  mount?: string | null;
  prettySize?: string | null;
}

export interface PciDevice {
  vendor?: string | null;
  product?: string | null;
  class?: string | null;
  driver?: string | null;
}

/** Everything the hardware, Talos and trust layers show for one node. Mirrors talos/inventory.rs. */
export interface NodeInventory {
  node: string;
  hostname?: string | null;
  /** This node's Talos version. */
  version?: string | null;
  stage?: string | null;
  ready?: boolean | null;
  security: SecurityState;
  system?: SystemInfo | null;
  processors: Processor[];
  memoryMib: number;
  disks: Disk[];
  links: Link[];
  addresses: Address[];
  extensions: Extension[];
  schematic?: string | null;
  volumes: Volume[];
  gpus: PciDevice[];
}

export interface TalosInventory {
  version: TalosVersion;
  nodes: NodeInventory[];
  warnings: string[];
}

export interface WatchMessage<T> {
  generation: number;
  payload: T;
}

// --- Live metrics --------------------------------------------------------

export type MetricsSource = 'kubelet' | 'dcgm' | 'sensors';

export interface MetricsSourceState {
  source: MetricsSource;
  ok: boolean;
  message: string | null;
}

export interface Temperature {
  /** The hwmon driver (`k10temp`, `nvme`). */
  sensor: string;
  /** Plain words (`CPU`, `NVMe 1`). */
  label: string;
  celsius: number;
}

export interface NodeMetrics {
  node: string;
  cpuCores: number | null;
  cpuCapacityCores: number | null;
  /** Resident plus active page cache: what the kubelet counts against eviction. */
  memoryWorkingSetBytes: number | null;
  /** Resident memory alone; the difference from the working set is reclaimable cache. */
  memoryRssBytes: number | null;
  memoryCapacityBytes: number | null;
  fsUsedBytes: number | null;
  fsCapacityBytes: number | null;
  /** Rates need two samples; null on the first and after a counter reset. */
  networkReceiveBytesPerSecond: number | null;
  networkTransmitBytesPerSecond: number | null;
  temperatures: Temperature[];
}

export interface GpuMetrics {
  node: string | null;
  uuid: string;
  index: string;
  model: string;
  pciBusId: string;
  utilizationPercent: number | null;
  memoryUsedBytes: number | null;
  memoryTotalBytes: number | null;
  temperatureCelsius: number | null;
  powerWatts: number | null;
  smClockMhz: number | null;
  memoryCopyPercent: number | null;
}

export interface PodMetrics {
  namespace: string;
  name: string;
  cpuCores: number | null;
  memoryWorkingSetBytes: number | null;
  memoryRssBytes: number | null;
}

/** One sample from every metrics source; `sources` says which of them answered. */
export interface MetricsSample {
  at: string;
  intervalSeconds: number;
  nodes: NodeMetrics[];
  gpus: GpuMetrics[];
  pods: PodMetrics[];
  sources: MetricsSourceState[];
}

// --- Live watches --------------------------------------------------------

export type WatchSource = 'cluster' | 'talos' | 'metrics';
/** `stopped` is the front end's own state; the Rust side reports the other three. */
export type WatchPhase = 'connecting' | 'live' | 'error' | 'stopped';

export interface WatchState {
  source: WatchSource;
  phase: WatchPhase;
  message: string | null;
}

/** Event names mirror src-tauri/src/watch/mod.rs. */
export const WATCH_EVENTS = {
  cluster: 'terrace://cluster',
  talos: 'terrace://talos',
  metrics: 'terrace://metrics',
  state: 'terrace://watch',
} as const;

// --- Ollama --------------------------------------------------------------

export interface OllamaModel {
  name: string;
  size: number;
  digest: string;
  family: string | null;
  parameterSize: string | null;
  quantization: string | null;
}

export interface OllamaLoadedModel {
  name: string;
  sizeVram: number;
  expiresAt: string | null;
}

export interface OllamaStatus {
  url: string;
  version: string;
  models: OllamaModel[];
  loaded: OllamaLoadedModel[];
}

// --- The API -------------------------------------------------------------

export const api = {
  app: {
    info: () => call<AppInfo>('get_app_info'),
    recentLogs: (count = 200) => call<string[]>('get_recent_logs', { count }),
  },
  settings: {
    all: () => call<Setting[]>('get_all_settings'),
    get: (key: string) => call<string | null>('get_setting', { key }),
    set: (key: string, value: string) => callVoid('set_setting', { key, value }),
    remove: (key: string) => callVoid('delete_setting', { key }),
  },
  vault: {
    put: (name: string, value: string) => callVoid('vault_put', { name, value }),
    exists: (name: string) => call<boolean>('vault_exists', { name }),
    remove: (name: string) => callVoid('vault_delete', { name }),
    list: () => call<string[]>('vault_list'),
  },
  cluster: {
    summary: () => call<ClusterSummary>('cluster_summary'),
  },
  talos: {
    version: () => call<TalosVersion>('talos_version'),
    inventory: () => call<TalosInventory>('talos_inventory'),
  },
  ollama: {
    /** Null when no Ollama URL is configured; Ollama is an optional integration. */
    status: () => call<OllamaStatus | null>('ollama_status'),
  },
  watch: {
    /** Start or restart the live watches; summaries then arrive as WATCH_EVENTS. */
    start: (generation: number) => callVoid('watch_start', { generation }),
    stop: () => callVoid('watch_stop'),
  },
};
