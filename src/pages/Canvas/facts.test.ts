import { describe, expect, it } from 'vitest';
import type {
  ClusterSummary,
  GpuMetrics,
  MetricsSample,
  NodeInventory,
  OllamaStatus,
  TalosInventory,
  WorkloadSummary,
} from '@/services/api';
import {
  buildFacts,
  buildLayerNodes,
  CLUSTER_FACT_ID,
  devicesLabel,
  eventsFact,
  gpuFacts,
  hardwareFacts,
  trustFacts,
  holdersLabel,
  isAccelerator,
  powerFact,
  kindLabel,
  layerHeights,
  layoutFacts,
  notConnectedFact,
  plannedFact,
  platformVersion,
  podsLabel,
  reachableLabel,
  requiredLayerWidth,
  serviceEdges,
  servicePortsLabel,
  servicesCountLabel,
  workloadStatusText,
} from './facts';
import {
  LAYER_HEIGHT,
  LAYER_IDS,
  LAYER_WIDTH,
  MAX_COLUMNS,
  ROW_HEIGHT,
  layerHeight,
} from './layers';
import { layerNodeId } from './nodes/types';
import { PALETTE } from './palette';

const failed = { ok: false as const, error: 'nope' };

const cluster: ClusterSummary = {
  context: 'admin@lab',
  server: 'https://10.0.0.1:6443',
  version: 'v1.37.0',
  nodes: [
    {
      name: 'node-a',
      ready: true,
      kubeletVersion: 'v1.37.0',
      osImage: 'Talos (v1.14.0)',
      kernelVersion: '6.18.0-talos',
      containerRuntime: 'containerd://2.1',
      gpuCapacity: 1,
      gpuAllocatable: 1,
      devices: { 'nvidia.com/gpu': { capacity: 1, allocatable: 1 } },
      addresses: [{ kind: 'InternalIP', address: '10.0.0.1' }],
      taints: [],
      labels: { 'nvidia.com/cuda.driver-version.full': '1.0.0' },
    },
  ],
  namespaces: [
    { name: 'ai', psaEnforce: 'privileged' },
    { name: 'default', psaEnforce: null },
  ],
  workloads: [
    {
      namespace: 'ai',
      name: 'ollama',
      kind: 'Deployment',
      role: 'app',
      vm: null,
      image: 'ollama/ollama:0.34.0@sha256:abc',
      readyReplicas: 1,
      replicas: 1,
      strategy: 'Recreate',
      wantsGpu: true,
      devices: ['nvidia.com/gpu'],
      available: true,
      pods: [
        {
          name: 'ollama-a',
          phase: 'Running',
          ready: true,
          restarts: 0,
          node: 'node-a',
          startedAt: '2026-09-17T10:00:00Z',
        },
        {
          name: 'ollama-b',
          phase: 'Succeeded',
          ready: false,
          restarts: 0,
          node: null,
          startedAt: null,
        },
      ],
    },
    {
      namespace: 'default',
      name: 'web',
      kind: 'Deployment',
      role: 'app',
      vm: null,
      image: 'nginx:1.27',
      readyReplicas: 1,
      replicas: 1,
      strategy: 'RollingUpdate',
      wantsGpu: false,
      devices: [],
      available: true,
      pods: [],
    },
    {
      namespace: 'kube-system',
      name: 'kube-proxy',
      kind: 'DaemonSet',
      role: 'system',
      vm: null,
      image: 'registry.k8s.io/kube-proxy:v1.37.0',
      readyReplicas: 1,
      replicas: 1,
      strategy: '',
      wantsGpu: false,
      devices: [],
      available: true,
      pods: [
        {
          name: 'kube-proxy-a',
          phase: 'Running',
          ready: true,
          restarts: 0,
          node: 'node-a',
          startedAt: null,
        },
      ],
    },
    {
      namespace: 'kubevirt',
      name: 'virt-handler',
      kind: 'DaemonSet',
      role: 'platform',
      vm: null,
      image: 'quay.io/kubevirt/virt-handler:v1.9.0',
      readyReplicas: 0,
      replicas: 1,
      strategy: '',
      wantsGpu: false,
      devices: [],
      available: false,
      pods: [
        {
          name: 'virt-handler-a',
          phase: 'Running',
          ready: false,
          restarts: 3,
          node: 'node-a',
          startedAt: null,
        },
      ],
    },
    {
      namespace: 'kubevirt',
      name: 'virt-operator',
      kind: 'Deployment',
      role: 'platform',
      vm: null,
      image: 'quay.io/kubevirt/virt-operator:v1.9.0@sha256:def',
      readyReplicas: 1,
      replicas: 1,
      strategy: 'RollingUpdate',
      wantsGpu: false,
      devices: [],
      available: true,
      pods: [],
    },
    {
      namespace: 'vm',
      name: 'win11',
      kind: 'VirtualMachine',
      role: 'app',
      image: '',
      readyReplicas: 1,
      replicas: 1,
      strategy: '',
      wantsGpu: false,
      devices: [],
      available: true,
      pods: [
        {
          name: 'virt-launcher-win11-a',
          phase: 'Running',
          ready: true,
          restarts: 0,
          node: 'node-a',
          startedAt: null,
        },
      ],
      vm: {
        status: 'Running',
        ready: true,
        runStrategy: 'Always',
        cores: 8,
        memory: '16Gi',
        guestOs: null,
        addresses: ['10.244.0.66'],
        node: 'node-a',
        disks: ['win11-disk (disk)', 'win11-iso (cdrom)'],
        secureBoot: true,
        gpus: [],
      },
    },
  ],
  services: [
    {
      namespace: 'ai',
      name: 'ollama',
      type: 'NodePort',
      clusterIp: '10.96.0.10',
      ports: [{ name: 'http', port: 11434, targetPort: '11434', nodePort: 31434, protocol: 'TCP' }],
      externalIps: [],
      targets: [{ kind: 'Deployment', name: 'ollama' }],
    },
    {
      namespace: 'kube-system',
      name: 'kube-dns',
      type: 'ClusterIP',
      clusterIp: '10.96.0.10',
      ports: [{ name: 'dns', port: 53, targetPort: '53', nodePort: null, protocol: 'UDP' }],
      externalIps: [],
      targets: [{ kind: 'Deployment', name: 'coredns' }],
    },
  ],
  events: [
    {
      namespace: 'ai',
      kind: 'Pod',
      name: 'ollama-a',
      reason: 'Started',
      message: 'Container started',
      type: 'Normal',
      count: 1,
      lastSeen: new Date(Date.now() - 60_000).toISOString(),
    },
    {
      namespace: 'default',
      kind: 'Pod',
      name: 'web-1',
      reason: 'BackOff',
      message: 'Back-off restarting failed container',
      type: 'Warning',
      count: 4,
      lastSeen: new Date(Date.now() - 120_000).toISOString(),
    },
    {
      namespace: 'default',
      kind: 'Pod',
      name: 'web-1',
      reason: 'Pulled',
      message: 'Container image pulled',
      type: 'Normal',
      count: 1,
      lastSeen: new Date(Date.now() - 180_000).toISOString(),
    },
  ],
};

const talos: TalosInventory = {
  version: {
    client: 'v1.14.0',
    server: 'v1.14.0',
    node: '10.0.0.1',
    nodes: [{ node: '10.0.0.1', tag: 'v1.14.0' }],
  },
  nodes: [
    {
      node: '10.0.0.1',
      hostname: 'node-a',
      stage: 'running',
      ready: true,
      security: {
        secureBoot: false,
        bootedWithUki: true,
        moduleSignatureEnforced: true,
        selinux: 'enabled, permissive',
      },
      system: {
        manufacturer: 'Example Boards',
        product: 'Board X1',
        version: '1.0',
        biosVersion: '1.20',
      },
      processors: [{ product: 'Example CPU 12-Core', cores: 12, threads: 24 }],
      memoryMib: 32768,
      disks: [
        { device: '/dev/nvme0n1', model: 'Disk Maker A', prettySize: '1.0 TB', transport: 'nvme' },
        { device: '/dev/nvme1n1', model: 'Disk Maker B', prettySize: '2.0 TB', transport: 'nvme' },
      ],
      links: [
        { name: 'eth0', up: true, driver: 'nic', speedMbit: 2500 },
        { name: 'eth1', up: false, driver: 'nic' },
        { name: 'tailscale0', up: true, kind: 'tun' },
      ],
      addresses: [
        { link: 'eth0', address: '10.0.0.1/24' },
        { link: 'tailscale0', address: '100.64.0.1/32' },
      ],
      extensions: [
        { name: 'schematic', version: '0123456789abcdef' },
        { name: 'tailscale', version: '1.0.0' },
        { name: 'nvidia-open-gpu-kernel-modules-lts', version: '1.0.0-v1.14.0' },
      ],
      schematic: '0123456789abcdef',
      volumes: [
        { name: 'STATE', filesystem: 'xfs' },
        { name: 'EPHEMERAL', filesystem: 'xfs' },
        { name: 'u-data', filesystem: 'xfs' },
      ],
      gpus: [
        {
          vendor: 'Example Silicon',
          product: 'Example GPU [Card A]',
          class: 'VGA compatible controller',
          driver: 'nvidia',
        },
      ],
    },
  ],
  warnings: [],
};

const ollama: OllamaStatus = {
  url: 'http://localhost:11434',
  version: '0.34.0',
  models: [
    {
      name: 'qwen3.8:27b',
      size: 17_000_000_000,
      digest: 'abc',
      family: 'qwen3',
      parameterSize: '27B',
      quantization: 'Q4_K_M',
    },
  ],
  loaded: [{ name: 'qwen3.8:27b', sizeVram: 18_500_000_000, expiresAt: null }],
};

function titles(facts: ReturnType<typeof buildFacts>): string[] {
  return facts.map((fact) => fact.data.title);
}

describe('buildLayerNodes', () => {
  it('creates one fixed band per layer, in canvas order, that cannot be removed', () => {
    const bands = buildLayerNodes();
    expect(bands.map((band) => band.data.layer)).toEqual([...LAYER_IDS]);
    for (const band of bands) {
      expect(band.type).toBe('layer');
      expect(band.deletable).toBe(false);
      expect(band.draggable).toBe(false);
      expect(band.id).toBe(layerNodeId(band.data.layer));
      expect(band.width).toBe(LAYER_WIDTH);
    }
    expect(buildLayerNodes(2000)[0]?.width).toBe(2000);
  });
});

describe('buildFacts', () => {
  it('shows every layer as not connected when nothing answers', () => {
    const facts = buildFacts(failed, failed, failed);
    const unavailable = facts.filter((fact) => fact.data.subtitle === 'Not connected');
    expect(unavailable.map((fact) => fact.data.layer).sort()).toEqual(
      ['gpu', 'hardware', 'kubernetes', 'talos', 'trust', 'workloads', 'workloads'].sort()
    );
    expect(facts.every((fact) => fact.data.source === 'live')).toBe(true);
  });

  it('turns a cluster summary into node, GPU, cluster, workload and trust facts', () => {
    const facts = buildFacts({ ok: true, value: cluster }, failed, { ok: true, value: null });
    const names = titles(facts);
    expect(names).toContain('node-a');
    expect(names).toContain('nvidia.com/gpu');
    expect(names).toContain('Cluster');
    expect(names).toContain('ollama');
    expect(names).toContain('Pod Security');
    expect(names).toContain('Primary image pinning');
    expect(names).not.toContain('Ollama');

    // Your things are cards in the Workloads layer; the machinery is folded away.
    const workloads = facts.filter((fact) => fact.data.layer === 'workloads');
    expect(workloads.map((fact) => fact.data.title)).toEqual(['ollama', 'web', 'win11']);
    const win11 = workloads[2];
    expect(win11?.data.subtitle).toBe('Virtual machine · vm');
    expect(win11?.data.status).toBe('ok');
    expect(win11?.data.node).toBe('node-a');
    expect(win11?.data.facts.find((entry) => entry.label === 'Size')?.value).toBe(
      '8 cores, 16Gi memory'
    );
    expect(win11?.data.facts.find((entry) => entry.label === 'Guest OS')?.value).toBe(
      'not reported yet'
    );
    expect(win11?.data.facts.find((entry) => entry.label === 'Address')?.value).toBe('10.244.0.66');
    expect(names).not.toContain('virt-operator');
    expect(names).not.toContain('kube-proxy');

    const kubevirt = facts.find((fact) => fact.data.title === 'KubeVirt');
    expect(kubevirt?.data.layer).toBe('kubernetes');
    expect(kubevirt?.data.status).toBe('warn');
    expect(kubevirt?.data.subtitle).toBe('Platform v1.9.0 · 2 components in kubevirt');
    expect(kubevirt?.data.facts[0]).toEqual({ label: 'virt-handler', value: '0 of 1 ready' });
    expect(kubevirt?.data.facts[1]).toEqual({ label: 'virt-operator', value: 'healthy' });

    const system = facts.find((fact) => fact.data.title === 'Kubernetes system');
    expect(system?.data.status).toBe('ok');
    expect(system?.data.subtitle).toBe('1 components healthy');

    const gpu = facts.find((fact) => fact.data.title === 'nvidia.com/gpu');
    expect(gpu?.data.facts.find((entry) => entry.label === 'Held by')?.value).toBe('ollama');
    expect(gpu?.data.node).toBe('node-a');

    const pinning = facts.find((fact) => fact.data.title === 'Primary image pinning');
    expect(pinning?.data.status).toBe('warn');
    expect(pinning?.data.subtitle).toBe('1 of 2 primary app images by digest');
    expect(pinning?.data.facts[0]).toEqual({ label: 'web', value: 'by tag' });
    expect(pinning?.data.facts.at(-1)).toEqual({
      label: 'Primary platform images',
      value: '1 of 2 by digest',
    });

    const psa = facts.find((fact) => fact.data.title === 'Pod Security');
    expect(psa?.data.status).toBe('warn');

    const ollama = facts.find((fact) => fact.data.title === 'ollama');
    expect(ollama?.data.subtitle).toBe('App · ai');
    expect(ollama?.data.facts.find((entry) => entry.label === 'Status')?.value).toBe(
      '1 of 2 ready'
    );
    expect(ollama?.data.facts.find((entry) => entry.label === 'Pods')?.value).toBe(
      '1 Running, 1 Succeeded'
    );
    expect(ollama?.data.facts.find((entry) => entry.label === 'GPU')?.value).toBe('requested');

    const events = facts.find((fact) => fact.data.title === 'Events');
    expect(events?.data.layer).toBe('kubernetes');
    expect(events?.data.status).toBe('warn');
    expect(events?.data.facts[0]).toEqual({ label: 'default/Pod/web-1', value: 'BackOff ×4' });
    expect(events?.data.facts).toHaveLength(2);
  });

  it('turns a Talos inventory into hardware, Talos and trust facts per node', () => {
    const facts = buildFacts(failed, { ok: true, value: talos }, { ok: true, value: null });
    const names = titles(facts);
    expect(names).toContain('node-a');
    expect(names).toContain('Disks');
    expect(names).toContain('Network');
    expect(names).toContain('Example GPU [Card A]');
    expect(names).toContain('Talos v1.14.0');
    expect(names).toContain('Extensions');
    expect(names).toContain('Boot');
    expect(names).toContain('Disk encryption');

    const system = facts.find(
      (fact) => fact.data.layer === 'hardware' && fact.data.title === 'node-a'
    );
    expect(system?.data.facts.find((entry) => entry.label === 'Memory')?.value).toBe('32 GiB');
    expect(system?.data.facts.find((entry) => entry.label === 'Firmware')?.value).toBe('1.20');
    expect(system?.data.status).toBe('ok');

    const boot = facts.find((fact) => fact.data.title === 'Boot');
    expect(boot?.data.status).toBe('warn');
    expect(boot?.data.facts.find((entry) => entry.label === 'Secure Boot')?.value).toBe('off');
    expect(boot?.data.facts.find((entry) => entry.label === 'Module signing')?.value).toBe(
      'enforced'
    );

    const encryption = facts.find((fact) => fact.data.title === 'Disk encryption');
    expect(encryption?.data.status).toBe('warn');
    expect(encryption?.data.facts).toHaveLength(3);

    const extensions = facts.find((fact) => fact.data.title === 'Extensions');
    expect(extensions?.data.facts.map((entry) => entry.label)).not.toContain('schematic');
    expect(extensions?.data.facts).toHaveLength(2);

    const network = facts.find((fact) => fact.data.title === 'Network');
    expect(network?.data.status).toBe('ok');
    expect(network?.data.subtitle).toBe('2 of 3 links up on node-a');

    const gpu = facts.find((fact) => fact.data.title === 'Example GPU [Card A]');
    expect(gpu?.data.status).toBe('ok');

    expect(
      facts.every((fact) => (fact.data.docs ?? []).every((doc) => doc.url.startsWith('https://')))
    ).toBe(true);
  });

  it('reports unread resources and marks encrypted system volumes ok', () => {
    const first = talos.nodes[0];
    if (!first) throw new Error('fixture has no node');
    const inventory: TalosInventory = {
      ...talos,
      warnings: ['pcidevices: unsupported', 'memorymodules: timeout'],
      nodes: [
        {
          ...first,
          volumes: [
            { name: 'STATE', encryption: 'luks2' },
            { name: 'EPHEMERAL', encryption: 'luks2' },
          ],
        },
      ],
    };
    const facts = buildFacts(failed, { ok: true, value: inventory }, { ok: true, value: null });
    expect(titles(facts)).toContain('Unread resources');
    expect(facts.find((fact) => fact.data.title === 'Disk encryption')?.data.status).toBe('ok');
  });

  it('adds Ollama only when configured and marks it unreachable when it fails', () => {
    const configured = buildFacts(failed, failed, { ok: true, value: ollama });
    expect(titles(configured)).toContain('Ollama');
    const broken = buildFacts(failed, failed, { ok: false, error: 'refused' });
    expect(
      broken.some((fact) => fact.data.title === 'Ollama' && fact.data.status === 'unknown')
    ).toBe(true);
  });

  it('parents every fact to its layer band and lays them out without overlap', () => {
    const facts = buildFacts({ ok: true, value: cluster }, { ok: true, value: talos }, failed);
    for (const fact of facts) {
      expect(fact.parentId).toBe(layerNodeId(fact.data.layer));
      expect(fact.extent).toBe('parent');
    }
    const hardware = facts.filter((fact) => fact.data.layer === 'hardware');
    const xs = hardware.map((fact) => fact.position.x);
    expect(new Set(xs).size).toBe(xs.length);
    expect(requiredLayerWidth(facts)).toBeGreaterThanOrEqual(LAYER_WIDTH);
  });
});

describe('eventsFact', () => {
  it('is absent without events and calm when the warnings are old', () => {
    expect(eventsFact({ ...cluster, events: [] })).toBeNull();
    const old = eventsFact(cluster, Date.now() + 60 * 60 * 1000);
    expect(old?.data.status).toBe('ok');
    expect(old?.data.subtitle).toContain('no fresh warnings');
  });
});

describe('workloads in plain words', () => {
  it('names kinds the way people do and reads status off the pods', () => {
    expect(kindLabel('VirtualMachine')).toBe('Virtual machine');
    expect(kindLabel('DaemonSet')).toBe('Node agent');
    expect(kindLabel('ClusterPolicy')).toBe('ClusterPolicy');
    const base = cluster.workloads[0];
    if (!base) throw new Error('fixture');
    expect(workloadStatusText({ ...base, pods: [], replicas: 0 })).toBe('Scaled to zero');
    expect(workloadStatusText({ ...base, pods: [], replicas: 1 })).toBe('No pods yet');
    expect(
      workloadStatusText({
        ...base,
        kind: 'ClusterPolicy',
        pods: [
          { name: 'v', phase: 'Succeeded', ready: false, restarts: 0, node: null, startedAt: null },
        ],
      })
    ).toBe('Completed');
    const win11 = cluster.workloads.find((workload) => workload.kind === 'VirtualMachine');
    if (!win11?.vm) throw new Error('fixture');
    expect(workloadStatusText(win11)).toBe('Running, ready');
    expect(
      workloadStatusText({ ...win11, vm: { ...win11.vm, ready: false, status: 'Stopped' } })
    ).toBe('Stopped');
  });

  it('reads a platform version off the operator image, digest ignored', () => {
    const components = cluster.workloads.filter((workload) => workload.namespace === 'kubevirt');
    expect(platformVersion(components)).toBe('v1.9.0');
    expect(platformVersion([])).toBeNull();
  });
});

describe('podsLabel', () => {
  it('counts pods by phase and singles out running pods that are not ready', () => {
    expect(podsLabel([])).toBe('none');
    expect(
      podsLabel([
        { name: 'a', phase: 'Running', ready: false, restarts: 1, node: null, startedAt: null },
        { name: 'b', phase: 'Running', ready: true, restarts: 0, node: null, startedAt: null },
        { name: 'c', phase: 'Pending', ready: false, restarts: 0, node: null, startedAt: null },
      ])
    ).toBe('1 Not ready, 1 Running, 1 Pending');
  });
});

describe('layoutFacts', () => {
  it('assigns slots per layer in order', () => {
    const laid = layoutFacts([
      notConnectedFact('gpu', 'A', 'x'),
      notConnectedFact('gpu', 'B', 'y'),
      notConnectedFact('talos', 'C', 'z'),
    ]);
    expect(laid[0]?.position.x).toBeLessThan(laid[1]?.position.x ?? 0);
    expect(laid[2]?.position.x).toBe(laid[0]?.position.x);
  });

  it('wraps a full row into the next one and makes the band taller', () => {
    const many = Array.from({ length: MAX_COLUMNS + 1 }, (_, index) =>
      notConnectedFact('hardware', `card-${String(index)}`, 'x')
    );
    const laid = layoutFacts(many);
    const last = laid[MAX_COLUMNS];
    const first = laid[0];
    expect(last?.position.x).toBe(first?.position.x);
    expect(last?.position.y).toBe((first?.position.y ?? 0) + ROW_HEIGHT);
    const heights = layerHeights(many);
    expect(heights.hardware).toBe(layerHeight(2));
    expect(heights.trust).toBe(LAYER_HEIGHT);
    expect(buildLayerNodes(LAYER_WIDTH, heights)[LAYER_IDS.length - 1]?.height).toBe(
      layerHeight(2)
    );
    // The width stops growing once a row is full; height takes over.
    expect(requiredLayerWidth(many)).toBe(requiredLayerWidth(many.slice(0, MAX_COLUMNS)));
  });
});

describe('requiredLayerWidth', () => {
  it('grows with the busiest layer', () => {
    const many = Array.from({ length: 8 }, (_, index) =>
      notConnectedFact('hardware', `H${String(index)}`, 'x')
    );
    expect(requiredLayerWidth(many)).toBeGreaterThan(LAYER_WIDTH);
    expect(requiredLayerWidth([])).toBe(LAYER_WIDTH);
  });
});

describe('plannedFact', () => {
  it('lands in the workloads layer marked as planned', () => {
    const entry = PALETTE.find((candidate) => candidate.id === 'open-webui');
    if (!entry) throw new Error('palette entry missing');
    const node = plannedFact(entry, { x: 300, y: 40 });
    expect(node.data.layer).toBe('workloads');
    expect(node.data.source).toBe('planned');
    expect(node.data.status).toBe('planned');
    expect(node.parentId).toBe(layerNodeId('workloads'));
  });
});

describe('serviceEdges', () => {
  it('draws each app Service as an edge from the cluster card, labelled with its ports', () => {
    const edges = serviceEdges(cluster);
    expect(edges).toHaveLength(1);
    const edge = edges[0];
    expect(edge?.source).toBe(CLUSTER_FACT_ID);
    expect(edge?.sourceHandle).toBe('up');
    expect(edge?.target).toBe('k8s:Deployment:ai/ollama');
    expect(edge?.targetHandle).toBe('down');
    expect(edge?.label).toBe('ollama 11434 → 31434 on the node');
    expect(edge?.className).toContain('terrace-edge--node');
    expect(edge?.data?.exposure).toBe('node');
  });

  it('draws nothing for a Service that reaches nothing or only the platform', () => {
    const ollama = cluster.services[0];
    if (!ollama) throw new Error('fixture');
    const unreached = { ...cluster, services: [{ ...ollama, targets: [] }] };
    expect(serviceEdges(unreached)).toHaveLength(0);
    const platformOnly = { ...cluster, services: cluster.services.slice(1) };
    expect(serviceEdges(platformOnly)).toHaveLength(0);
  });

  it('says in plain words where a workload can be reached', () => {
    expect(
      reachableLabel({ kind: 'Deployment', namespace: 'ai', name: 'ollama' }, cluster.services)
    ).toBe('ollama on node port 31434');
    expect(
      reachableLabel({ kind: 'Deployment', namespace: 'default', name: 'web' }, cluster.services)
    ).toBe('not exposed');
    const dns = cluster.services[1];
    if (!dns) throw new Error('fixture');
    expect(servicePortsLabel(dns)).toBe('53/UDP');
    expect(servicesCountLabel(cluster.services)).toBe('2, 1 configured for outside access');
    expect(servicesCountLabel([])).toBe('none');
  });
});

// --- Live metrics on the canvas ------------------------------------------------------

const vmWorkload: WorkloadSummary = {
  namespace: 'vm',
  name: 'desk',
  kind: 'VirtualMachine',
  role: 'app',
  image: '',
  readyReplicas: 1,
  replicas: 1,
  strategy: '',
  wantsGpu: false,
  devices: ['devices.kubevirt.io/kvm', 'vendor.com/CARD_B'],
  available: true,
  pods: [
    {
      name: 'virt-launcher-desk-x',
      phase: 'Running',
      ready: true,
      restarts: 0,
      node: 'node-a',
      startedAt: null,
    },
  ],
  vm: {
    status: 'Running',
    ready: true,
    runStrategy: 'Always',
    cores: 4,
    memory: '8Gi',
    guestOs: null,
    addresses: [],
    node: 'node-a',
    disks: [],
    secureBoot: true,
    gpus: ['vendor.com/CARD_B'],
  },
};

function withDevices(): ClusterSummary {
  const node = cluster.nodes[0];
  if (!node) throw new Error('fixture');
  return {
    ...cluster,
    nodes: [
      {
        ...node,
        devices: {
          'nvidia.com/gpu': { capacity: 1, allocatable: 1 },
          'vendor.com/CARD_B': { capacity: 1, allocatable: 1 },
          'devices.kubevirt.io/kvm': { capacity: 1000, allocatable: 1000 },
        },
      },
    ],
    workloads: [...cluster.workloads, vmWorkload],
  };
}

const cardA: GpuMetrics = {
  node: 'node-a',
  uuid: 'GPU-a',
  index: '0',
  model: 'Card A',
  pciBusId: '00000000:0A:00.0',
  utilizationPercent: 12,
  memoryUsedBytes: 6 * 1024 ** 3,
  memoryTotalBytes: 24 * 1024 ** 3,
  temperatureCelsius: 43,
  powerWatts: 28,
  smClockMhz: 210,
  memoryCopyPercent: 0,
};

const metrics: MetricsSample = {
  at: '2026-01-01T00:00:00.000Z',
  intervalSeconds: 5,
  nodes: [
    {
      node: 'node-a',
      cpuCores: 3.2,
      cpuCapacityCores: 32,
      memoryWorkingSetBytes: 40 * 1024 ** 3,
      memoryRssBytes: 21 * 1024 ** 3,
      memoryCapacityBytes: 62 * 1024 ** 3,
      fsUsedBytes: 50 * 1024 ** 3,
      fsCapacityBytes: 1800 * 1024 ** 3,
      networkReceiveBytesPerSecond: null,
      networkTransmitBytesPerSecond: null,
      temperatures: [{ sensor: 'k10temp', label: 'CPU', celsius: 42 }],
    },
  ],
  gpus: [cardA],
  pods: [
    {
      namespace: 'ai',
      name: 'ollama-1',
      cpuCores: 0.3,
      memoryWorkingSetBytes: 2 * 1024 ** 3,
      memoryRssBytes: 2 * 1024 ** 3,
    },
  ],
  sources: [{ source: 'kubelet', ok: true, message: null }],
};

const inventoryNode: NodeInventory = {
  node: '10.0.0.1',
  hostname: 'node-a',
  stage: 'running',
  ready: true,
  security: {},
  processors: [],
  memoryMib: 32768,
  disks: [],
  links: [],
  addresses: [],
  extensions: [],
  volumes: [],
  gpus: [],
};

describe('uncertain and multi-node data', () => {
  it('shows null Talos security flags as unknown', () => {
    const facts = trustFacts({
      ...inventoryNode,
      security: {
        secureBoot: null,
        bootedWithUki: null,
        moduleSignatureEnforced: null,
        fips: null,
      },
    });
    expect(facts[0]?.data.status).toBe('unknown');
    expect(facts[0]?.data.facts.every((entry) => entry.value === 'unknown')).toBe(true);
  });

  it('does not list a workload as a GPU holder on another node', () => {
    const first = cluster.nodes[0];
    if (!first) throw new Error('fixture');
    const facts = gpuFacts({ ...cluster, nodes: [first, { ...first, name: 'node-b' }] }, null);
    const second = facts.find((card) => card.data.node === 'node-b');
    expect(second?.data.facts.find((entry) => entry.label === 'Held by')?.value).toBe('nobody');
    const completed = gpuFacts(
      {
        ...cluster,
        workloads: cluster.workloads.map((workload) => ({
          ...workload,
          pods: workload.pods.map((pod) => ({ ...pod, phase: 'Succeeded' })),
        })),
      },
      null
    );
    expect(completed[0]?.data.facts.find((entry) => entry.label === 'Held by')?.value).toBe(
      'nobody'
    );
  });

  it('keeps events for identically named objects in different namespaces separate', () => {
    const event = cluster.events[0];
    if (!event) throw new Error('fixture');
    const facts = eventsFact({ ...cluster, events: [event, { ...event, namespace: 'another' }] });
    expect(facts?.data.facts).toHaveLength(2);
  });
});

describe('the GPU layer', () => {
  it("lists vendor devices and not KubeVirt's own plugins", () => {
    expect(isAccelerator('nvidia.com/gpu')).toBe(true);
    expect(isAccelerator('vendor.com/CARD_B')).toBe(true);
    expect(isAccelerator('devices.kubevirt.io/kvm')).toBe(false);
    expect(isAccelerator('cpu')).toBe(false);
    expect(devicesLabel({ devices: ['nvidia.com/gpu'], wantsGpu: true })).toBe('requested');
    expect(
      devicesLabel({ devices: ['nvidia.com/gpu', 'vendor.com/CARD_B'], wantsGpu: false })
    ).toBe('requested, CARD_B');
    // What a workload holds is read from its device names, whoever makes them.
    expect(devicesLabel({ devices: ['amd.com/gpu'], wantsGpu: true })).toBe('requested');
    expect(devicesLabel({ devices: [], wantsGpu: true })).toBe('no');
    expect(devicesLabel({ devices: [], wantsGpu: false })).toBe('no');
    expect(holdersLabel([vmWorkload])).toBe('desk (VM)');
  });

  it('without metrics shows one card per resource with who holds it', () => {
    const facts = gpuFacts(withDevices(), null);
    expect(facts.map((fact) => fact.data.title)).toEqual(['nvidia.com/gpu', 'vendor.com/CARD_B']);
    const generic = facts[0];
    expect(generic?.data.facts.find((entry) => entry.label === 'Held by')?.value).toBe('ollama');
    expect(generic?.data.gauges).toBeUndefined();
    const passed = facts[1];
    expect(passed?.data.facts.find((entry) => entry.label === 'Held by')?.value).toBe('desk (VM)');
    expect(passed?.data.facts.find((entry) => entry.label === 'Access')?.value).toBe(
      'whole device, passed through'
    );
  });

  it('with metrics merges the one reported GPU into the one schedulable resource, with dials', () => {
    const facts = gpuFacts(withDevices(), metrics);
    expect(facts.map((fact) => fact.data.title)).toEqual(['Card A', 'vendor.com/CARD_B']);
    const card = facts[0];
    expect(card?.id).toBe('gpu:device:GPU-a');
    expect(card?.data.subtitle).toBe('nvidia.com/gpu on node-a · held by ollama');
    expect(card?.data.metricsKey).toBe('gpu:GPU-a');
    expect(card?.data.gauges?.map((gauge) => gauge.id)).toEqual(['util', 'vram', 'temp', 'power']);
    expect(card?.data.status).toBe('ok');
    expect(card?.data.facts.find((entry) => entry.label === 'VRAM')?.value).toBe(
      '6.0 GiB of 24 GiB'
    );
  });

  it('keeps a reported GPU apart from the resource when the counts do not match', () => {
    const two: MetricsSample = {
      ...metrics,
      gpus: [cardA, { ...cardA, uuid: 'GPU-b', index: '1', model: 'Card B' }],
    };
    const facts = gpuFacts(withDevices(), two);
    expect(facts.map((fact) => fact.data.title)).toEqual([
      'Card A',
      'Card B',
      'nvidia.com/gpu',
      'vendor.com/CARD_B',
    ]);
    expect(facts[0]?.data.subtitle).toBe('GPU 0 on node-a');
  });

  it('places a GPU with no node on its own card', () => {
    const homeless: MetricsSample = { ...metrics, gpus: [{ ...cardA, node: null }] };
    const facts = gpuFacts(withDevices(), homeless);
    expect(facts.map((fact) => fact.data.title)).toEqual([
      'nvidia.com/gpu',
      'vendor.com/CARD_B',
      'Card A',
    ]);
    expect(facts[2]?.data.node).toBeUndefined();
  });
});

describe('node vitals', () => {
  it('put gauges and a history key on the system card', () => {
    const live = metrics.nodes[0] ?? null;
    const [system] = hardwareFacts(inventoryNode, live);
    expect(system?.data.gauges?.map((gauge) => gauge.id)).toEqual([
      'cpu',
      'memory',
      'disk',
      'temp:CPU',
    ]);
    expect(system?.data.metricsKey).toBe('node:node-a');
    expect(system?.data.status).toBe('ok');
    const [bare] = hardwareFacts(inventoryNode, null);
    expect(bare?.data.gauges).toBeUndefined();
    expect(bare?.data.metricsKey).toBe('node:node-a');
  });
});

describe('usage and power', () => {
  it("an app card carries its pods' usage when a sample has them", () => {
    const names =
      cluster.workloads
        .find((workload) => workload.name === 'ollama')
        ?.pods.map((pod) => pod.name) ?? [];
    const sampled: MetricsSample = {
      ...metrics,
      pods: names.map((name) => ({
        namespace: 'ai',
        name,
        cpuCores: 0.3 / names.length,
        memoryWorkingSetBytes: (2 * 1024 ** 3) / names.length,
        memoryRssBytes: (2 * 1024 ** 3) / names.length,
      })),
    };
    const facts = buildFacts({ ok: true, value: cluster }, failed, failed, sampled);
    const ollama = facts.find((fact) => fact.data.title === 'ollama');
    expect(ollama?.data.facts.find((entry) => entry.label === 'Usage')?.value).toBe(
      '0.30 cores, 2.0 GiB'
    );
  });

  it('a power card sums the GPUs and measures against the budget', () => {
    expect(powerFact({ ...metrics, gpus: [] }, 750)).toBeNull();
    const card = powerFact(metrics, 750);
    expect(card?.data.layer).toBe('hardware');
    expect(card?.data.gauges?.[0]?.text).toBe('28 W');
    expect(card?.data.gauges?.[0]?.fraction).toBeCloseTo(28 / 750);
    expect(card?.data.subtitle).toBe('GPU draw against a 750 W budget');
    const unbudgeted = powerFact(metrics, null);
    expect(unbudgeted?.data.gauges?.[0]?.fraction).toBeNull();
    const facts = buildFacts({ ok: true, value: cluster }, failed, failed, metrics, 750);
    expect(facts.some((fact) => fact.id === 'power:gpus')).toBe(true);
  });
});
