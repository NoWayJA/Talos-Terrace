import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ClusterSummary, MetricsSample } from '@/services/api';

vi.mock('@/services/api', () => ({
  api: {
    cluster: { summary: vi.fn() },
    talos: { inventory: vi.fn() },
    ollama: { status: vi.fn() },
    watch: { start: vi.fn(), stop: vi.fn() },
  },
}));

import { api } from '@/services/api';
import { LAYER_IDS, ROW_HEIGHT } from '../layers';
import { layerNodeId } from '../nodes/types';
import { PALETTE } from '../palette';
import { useCanvasStore } from './useCanvasStore';

const summary: ClusterSummary = {
  context: 'admin@lab',
  server: 'https://10.0.0.1:6443',
  version: 'v1.37.0',
  nodes: [],
  namespaces: [],
  workloads: [],
  services: [],
  events: [],
};

const sample: MetricsSample = {
  at: '2026-01-01T00:00:00.000Z',
  intervalSeconds: 5,
  nodes: [],
  gpus: [],
  pods: [],
  sources: [{ source: 'kubelet', ok: true, message: null }],
};

describe('useCanvasStore', () => {
  beforeEach(() => {
    useCanvasStore.setState(useCanvasStore.getInitialState());
    vi.mocked(api.cluster.summary).mockReset();
    vi.mocked(api.talos.inventory).mockReset();
    vi.mocked(api.ollama.status).mockReset();
    vi.mocked(api.watch.start).mockReset();
    vi.mocked(api.watch.stop).mockReset();
  });

  it('starts with the six layer bands first, then facts', () => {
    const { nodes } = useCanvasStore.getState();
    expect(nodes.slice(0, LAYER_IDS.length).every((node) => node.type === 'layer')).toBe(true);
    expect(nodes.slice(LAYER_IDS.length).every((node) => node.type === 'fact')).toBe(true);
  });

  it('refuses to remove layer bands and live facts', () => {
    const state = useCanvasStore.getState();
    const fact = state.nodes.find((node) => node.type === 'fact');
    if (!fact) throw new Error('no fact node');
    state.onNodesChange([
      { type: 'remove', id: layerNodeId('gpu') },
      { type: 'remove', id: fact.id },
    ]);
    const after = useCanvasStore.getState().nodes;
    expect(after.some((node) => node.id === layerNodeId('gpu'))).toBe(true);
    expect(after.some((node) => node.id === fact.id)).toBe(true);
  });

  it('allows deleting planned cards without hiding live objects', () => {
    const entry = PALETTE[0];
    if (!entry) throw new Error('empty palette');
    const state = useCanvasStore.getState();
    state.addPlannedNode(entry, null);
    const id = useCanvasStore.getState().selectedNodeId;
    if (!id) throw new Error('no planned card');
    state.onNodesChange([{ type: 'remove', id }]);
    expect(useCanvasStore.getState().nodes.some((node) => node.id === id)).toBe(false);
  });

  it('discards queued events from a previous connection, including its metrics and badge', () => {
    const state = useCanvasStore.getState();
    const oldGeneration = state.watchGeneration;
    state.applyCluster(summary, oldGeneration);
    state.applyMetrics(sample, oldGeneration);
    state.resetSources();
    state.applyCluster(summary, oldGeneration);
    state.applyMetrics(sample, oldGeneration);
    state.setWatchState({ source: 'cluster', phase: 'live', message: null }, oldGeneration);
    const after = useCanvasStore.getState();
    expect(after.target).toBeNull();
    expect(after.sources.cluster.ok).toBe(false);
    expect(after.metrics.latest).toBeNull();
    expect(after.watch.cluster.phase).toBe('stopped');
    state.applyCluster({ ...summary, context: 'new-context' }, after.watchGeneration);
    expect(useCanvasStore.getState().target).toBe('new-context');
  });

  it('discards a refresh that finishes after the connection changes', async () => {
    let finish: ((value: ClusterSummary) => void) | undefined;
    vi.mocked(api.cluster.summary).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    vi.mocked(api.talos.inventory).mockRejectedValue(new Error('unconfigured'));
    vi.mocked(api.ollama.status).mockResolvedValue(null);
    const pending = useCanvasStore.getState().refresh();
    useCanvasStore.getState().resetSources();
    useCanvasStore.getState().applyCluster({ ...summary, context: 'new-context' });
    finish?.(summary);
    await pending;
    expect(useCanvasStore.getState().target).toBe('new-context');
    expect(useCanvasStore.getState().sources.ollama.ok).toBe(false);
  });

  it('keeps newer watch data when an older one-shot refresh completes', async () => {
    let finish: ((value: ClusterSummary) => void) | undefined;
    vi.mocked(api.cluster.summary).mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        })
    );
    vi.mocked(api.talos.inventory).mockRejectedValue(new Error('unconfigured'));
    vi.mocked(api.ollama.status).mockResolvedValue(null);
    const pending = useCanvasStore.getState().refresh();
    useCanvasStore.getState().applyCluster({ ...summary, version: 'newer' });
    finish?.(summary);
    await pending;
    const cluster = useCanvasStore.getState().sources.cluster;
    expect(cluster.ok && cluster.value.version).toBe('newer');
  });

  it('plans a dropped palette entry in the workloads layer and selects it', () => {
    const entry = PALETTE[0];
    if (!entry) throw new Error('empty palette');
    useCanvasStore.getState().addPlannedNode(entry, { x: 500, y: 250 });
    const { nodes, selectedNodeId } = useCanvasStore.getState();
    const planned = nodes.find((node) => node.id === selectedNodeId);
    expect(planned?.type).toBe('fact');
    expect(planned?.parentId).toBe(layerNodeId('workloads'));
    expect(planned?.position.x).toBeGreaterThanOrEqual(8);
  });

  it('refreshes from the three sources, keeps planned nodes and records failures', async () => {
    vi.mocked(api.cluster.summary).mockResolvedValue(summary);
    vi.mocked(api.talos.inventory).mockRejectedValue({
      code: 'TALOS',
      message: 'talosctl missing',
    });
    vi.mocked(api.ollama.status).mockResolvedValue(null);

    const entry = PALETTE[0];
    if (!entry) throw new Error('empty palette');
    useCanvasStore.getState().addPlannedNode(entry, { x: 400, y: 240 });

    await useCanvasStore.getState().refresh();

    const state = useCanvasStore.getState();
    expect(state.target).toBe('admin@lab');
    expect(state.lastRefresh).not.toBeNull();
    expect(state.refreshError).toBe('1 of 3 sources unavailable');
    expect(state.nodes.filter((node) => node.type === 'layer')).toHaveLength(LAYER_IDS.length);
    expect(state.nodes.some((node) => node.type === 'fact' && node.data.source === 'planned')).toBe(
      true
    );
    expect(state.nodes.some((node) => node.data.title === 'Cluster')).toBe(true);
    // The planned Ollama node stays; no live Ollama fact appears while it is unconfigured.
    expect(
      state.nodes.some(
        (node) =>
          node.type === 'fact' && node.data.source === 'live' && node.data.title === 'Ollama'
      )
    ).toBe(false);
  });

  it('applies a pushed cluster summary without a refresh and keeps the other sources', () => {
    const entry = PALETTE[0];
    if (!entry) throw new Error('empty palette');
    useCanvasStore.getState().addPlannedNode(entry, { x: 400, y: 240 });

    useCanvasStore.getState().applyCluster(summary);

    const state = useCanvasStore.getState();
    expect(api.cluster.summary).not.toHaveBeenCalled();
    expect(state.target).toBe('admin@lab');
    expect(state.sources.cluster.ok).toBe(true);
    expect(state.sources.talos.ok).toBe(false);
    // The other two have not been asked yet; that is not a failure to warn about.
    expect(state.refreshError).toBeNull();
    expect(state.lastRefresh).not.toBeNull();
    expect(state.nodes.some((node) => node.data.title === 'Cluster')).toBe(true);
    expect(state.nodes.filter((node) => node.data.source === 'planned')).toHaveLength(1);
    expect(state.nodes.filter((node) => node.type === 'layer')).toHaveLength(LAYER_IDS.length);
  });

  it('tracks the watch state per source and reports a failed start on both', async () => {
    vi.mocked(api.watch.start).mockRejectedValue(new Error('setting talosconfig.path is not set'));
    await useCanvasStore.getState().startWatch();
    let { watch } = useCanvasStore.getState();
    expect(watch.cluster.phase).toBe('error');
    expect(watch.talos.message).toContain('talosconfig.path');

    useCanvasStore.getState().setWatchState({ source: 'cluster', phase: 'live', message: null });
    watch = useCanvasStore.getState().watch;
    expect(watch.cluster.phase).toBe('live');
    expect(watch.talos.phase).toBe('error');

    vi.mocked(api.watch.stop).mockResolvedValue(undefined);
    await useCanvasStore.getState().stopWatch();
    expect(useCanvasStore.getState().watch.cluster.phase).toBe('stopped');
    expect(useCanvasStore.getState().watch.talos.phase).toBe('stopped');
  });

  it('runs start and stop in call order even when they overlap', async () => {
    const order: string[] = [];
    vi.mocked(api.watch.start).mockImplementation(
      () =>
        new Promise((resolve) => {
          setTimeout(() => {
            order.push('start');
            resolve();
          }, 20);
        })
    );
    vi.mocked(api.watch.stop).mockImplementation(() => {
      order.push('stop');
      return Promise.resolve();
    });
    const store = useCanvasStore.getState();
    // The development double-mount: start, stop, start, all before the first start resolves.
    await Promise.all([store.startWatch(), store.stopWatch(), store.startWatch()]);
    expect(order).toEqual(['start', 'stop', 'start']);
    expect(useCanvasStore.getState().watch.cluster.phase).toBe('connecting');
  });

  it('keeps a dragged card where it is and reuses untouched cards across samples', () => {
    useCanvasStore.getState().applyCluster(summary);
    const before = useCanvasStore.getState().nodes;
    const card = before.find((node) => node.data.title === 'Cluster');
    if (!card) throw new Error('no Cluster card');
    useCanvasStore
      .getState()
      .onNodesChange([{ type: 'position', id: card.id, position: { x: 900, y: 60 } }]);

    useCanvasStore.getState().applyMetrics(sample);

    const after = useCanvasStore.getState().nodes;
    const moved = after.find((node) => node.id === card.id);
    expect(moved?.position).toEqual({ x: 900, y: 60 });
    // A card whose data did not change is the same object, so React Flow keeps its measurements.
    const untouched = before.find((node) => node.type === 'fact' && node.id !== card.id);
    if (!untouched) throw new Error('no second card');
    expect(after.find((node) => node.id === untouched.id)).toBe(untouched);
    expect(after.find((node) => node.type === 'layer')).toBe(
      before.find((node) => node.type === 'layer')
    );
  });

  it('moves lower bands when an upper band grows without changing their own size', () => {
    const workload = {
      namespace: 'apps',
      name: 'web',
      kind: 'Deployment',
      role: 'app' as const,
      vm: null,
      image: 'example/web:1',
      readyReplicas: 1,
      replicas: 1,
      strategy: 'RollingUpdate',
      wantsGpu: false,
      devices: [],
      available: true,
      pods: [],
    };
    useCanvasStore.getState().applyCluster({ ...summary, workloads: [workload] });
    const before = useCanvasStore.getState().nodes.find((node) => node.id === layerNodeId('gpu'));
    useCanvasStore.getState().applyCluster({
      ...summary,
      workloads: Array.from({ length: 7 }, (_, index) => ({
        ...workload,
        name: `web-${String(index)}`,
      })),
    });
    const after = useCanvasStore.getState().nodes.find((node) => node.id === layerNodeId('gpu'));
    expect(after?.height).toBe(before?.height);
    expect(after?.position.y).toBe((before?.position.y ?? 0) + ROW_HEIGHT);
  });

  it('keeps the last good read when a refresh fails and says so', async () => {
    useCanvasStore.getState().applyCluster(summary);
    vi.mocked(api.cluster.summary).mockRejectedValue({ code: 'TIMEOUT', message: 'timed out' });
    vi.mocked(api.talos.inventory).mockRejectedValue({ code: 'TALOS', message: 'gone' });
    vi.mocked(api.ollama.status).mockResolvedValue(null);

    await useCanvasStore.getState().refresh();

    const state = useCanvasStore.getState();
    expect(state.sources.cluster.ok).toBe(true);
    expect(state.target).toBe('admin@lab');
    expect(state.nodes.some((node) => node.data.title === 'Cluster')).toBe(true);
    expect(state.refreshError).toContain('did not answer');
    expect(state.refreshError).toContain('timed out');
  });
});
