// Store-backed React Flow change handlers, the pattern lifted from Modz (NoWayJA/Modz, MIT)
// pages/Workflows/hooks/useWorkflowStore.ts. The store owns nodes and edges; the canvas
// component is a view of it. Live summaries pushed by the Rust watches land here too.
import {
  applyEdgeChanges,
  applyNodeChanges,
  type Edge,
  type EdgeChange,
  type NodeChange,
  type XYPosition,
} from '@xyflow/react';
import { create } from 'zustand';
import {
  api,
  type ClusterSummary,
  type MetricsSample,
  type OllamaStatus,
  type TalosInventory,
  type WatchSource,
  type WatchState,
} from '@/services/api';
import { describeError, describeErrorForLog } from '@/services/errors';
import { powerBudgetWatts, useSettingsStore, type SettingKey } from '@/stores/useSettingsStore';
import {
  FACT_LEFT,
  FACT_TOP,
  buildFacts,
  buildLayerNodes,
  layerHeights,
  plannedFact,
  requiredLayerWidth,
  serviceEdges,
  type SourceResult,
} from '../facts';
import { LAYER_HEIGHT, layerPosition } from '../layers';
import { EMPTY_HISTORY, alertsFor, pushSample, type Alert, type MetricsHistory } from '../metrics';
import {
  isLayerNodeId,
  layerNodeId,
  type CanvasNode,
  type FactNode,
  type LayerNode,
} from '../nodes/types';
import type { PaletteEntry } from '../palette';

export interface Sources {
  cluster: SourceResult<ClusterSummary>;
  talos: SourceResult<TalosInventory>;
  ollama: SourceResult<OllamaStatus | null>;
}

export type WatchStates = Record<WatchSource, WatchState>;

interface CanvasState {
  watchGeneration: number;
  sourceRevision: number;
  resetSources: () => void;
  /** Kubernetes context name once a read has reached the cluster. */
  target: string | null;
  nodes: CanvasNode[];
  edges: Edge[];
  selectedNodeId: string | null;
  /** A selected Service edge; never set together with a node. */
  selectedEdgeId: string | null;
  refreshing: boolean;
  /** When the canvas last changed from any source, refresh or watch. */
  lastRefresh: string | null;
  refreshError: string | null;
  /** The last answer from each source; a watch error keeps the last good one on screen. */
  sources: Sources;
  watch: WatchStates;
  /** Live metrics: the latest sample and a short history per series. */
  metrics: MetricsHistory;
  /** What the latest sample says is worth attention, errors first. */
  alerts: Alert[];

  onNodesChange: (changes: NodeChange<CanvasNode>[]) => void;
  onEdgesChange: (changes: EdgeChange[]) => void;
  selectNode: (id: string | null) => void;
  selectEdge: (id: string | null) => void;
  /** Plan a workload where it was dropped, or at the next free slot when there was no drop. */
  addPlannedNode: (entry: PaletteEntry, flowPosition: XYPosition | null) => void;
  /** Pull everything once, including the optional integrations the watches do not cover. */
  refresh: () => Promise<void>;
  applyCluster: (summary: ClusterSummary, generation?: number) => void;
  applyTalos: (inventory: TalosInventory, generation?: number) => void;
  applyMetrics: (sample: MetricsSample, generation?: number) => void;
  setWatchState: (state: WatchState, generation?: number) => void;
  startWatch: () => Promise<void>;
  stopWatch: () => Promise<void>;
}

const NOT_REFRESHED: SourceResult<never> = { ok: false, error: 'Not refreshed yet' };
const INITIAL_SOURCES: Sources = {
  cluster: NOT_REFRESHED,
  talos: NOT_REFRESHED,
  ollama: NOT_REFRESHED,
};

function watchStates(phase: WatchState['phase'], message: string | null = null): WatchStates {
  return {
    cluster: { source: 'cluster', phase, message },
    talos: { source: 'talos', phase, message },
    metrics: { source: 'metrics', phase, message },
  };
}

/** The power budget from Settings, read when a sample arrives so a change shows on the next one. */
function powerBudget(): number | null {
  return powerBudgetWatts(useSettingsStore.getState().values);
}

// Start and stop run strictly in call order. React's development double-mount fires
// start, stop, start within milliseconds; without a queue the stop could land last.
let watchChain: Promise<unknown> = Promise.resolve();

function queued<T>(task: () => Promise<T>): Promise<T> {
  const next = watchChain.then(task, task);
  watchChain = next.then(
    () => undefined,
    () => undefined
  );
  return next;
}

async function settle<T>(promise: Promise<T>): Promise<SourceResult<T>> {
  try {
    return { ok: true, value: await promise };
  } catch (error: unknown) {
    return { ok: false, error: describeError(error) };
  }
}

/** A fresh answer replaces the old one unless it failed while a good one is on screen. */
function keepGood<T>(previous: SourceResult<T>, fresh: SourceResult<T>): SourceResult<T> {
  return fresh.ok || !previous.ok ? fresh : previous;
}

/** Plain-data equality for what the builders produce: strings, numbers, arrays, objects. */
export function sameData(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  if (Array.isArray(a) || Array.isArray(b)) {
    return (
      Array.isArray(a) &&
      Array.isArray(b) &&
      a.length === b.length &&
      a.every((item, index) => sameData(item, b[index]))
    );
  }
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = Object.keys(left);
  return (
    keys.length === Object.keys(right).length &&
    keys.every((key) => key in right && sameData(left[key], right[key]))
  );
}

function withoutSelection(edge: Edge): Omit<Edge, 'selected'> {
  const copy: Edge = { ...edge };
  delete copy.selected;
  return copy;
}

/**
 * Rebuild the canvas from the sources, keeping planned nodes where they are. React Flow
 * re-measures any node whose object changes and forgets what it owns on it (measured size,
 * selection), so a card whose data has not changed keeps its previous object, and one that
 * has carries those fields over. While the set of live cards is unchanged (a metrics
 * sample, typically) their positions are kept too, so a card someone dragged does not jump
 * back every few seconds.
 */
function rebuilt(
  nodes: CanvasNode[],
  edges: Edge[],
  target: string | null,
  sources: Sources,
  metrics: MetricsSample | null,
  budgetWatts: number | null
): Pick<CanvasState, 'nodes' | 'edges' | 'target' | 'sources' | 'lastRefresh' | 'refreshError'> {
  const built = buildFacts(sources.cluster, sources.talos, sources.ollama, metrics, budgetWatts);
  const previousLive = new Map<string, FactNode>();
  const previousLayers = new Map<string, LayerNode>();
  const planned: FactNode[] = [];
  for (const node of nodes) {
    if (node.type === 'layer') previousLayers.set(node.id, node);
    else if (node.data.source === 'planned') planned.push(node);
    else previousLive.set(node.id, node);
  }
  const sameCards =
    previousLive.size === built.length && built.every((node) => previousLive.has(node.id));
  const live = built.map((node) => {
    const before = previousLive.get(node.id);
    if (!before) return node;
    const position = sameCards ? before.position : node.position;
    const data = sameData(before.data, node.data) ? before.data : node.data;
    if (data === before.data && position === before.position) return before;
    return { ...before, ...node, position, data };
  });
  const layers = buildLayerNodes(requiredLayerWidth(live), layerHeights(live)).map((layer) => {
    const before = previousLayers.get(layer.id);
    return before &&
      before.width === layer.width &&
      before.height === layer.height &&
      sameData(before.position, layer.position)
      ? before
      : layer;
  });
  const previousEdges = new Map(edges.map((edge) => [edge.id, edge]));
  const nextEdges = (sources.cluster.ok ? serviceEdges(sources.cluster.value) : []).map((edge) => {
    const before = previousEdges.get(edge.id);
    if (!before) return edge;
    return sameData(withoutSelection(before), edge) ? before : { ...before, ...edge };
  });
  // A source that has never answered is not "unavailable": the first refresh is still to come.
  const unavailable = [sources.cluster, sources.talos, sources.ollama].filter(
    (result) => !result.ok && result !== NOT_REFRESHED
  ).length;
  return {
    nodes: [...layers, ...planned, ...live],
    edges: nextEdges,
    target: sources.cluster.ok ? sources.cluster.value.context : target,
    sources,
    lastRefresh: new Date().toISOString(),
    refreshError: unavailable > 0 ? `${String(unavailable)} of 3 sources unavailable` : null,
  };
}

function initialNodes(): CanvasNode[] {
  return [
    ...buildLayerNodes(),
    ...buildFacts(INITIAL_SOURCES.cluster, INITIAL_SOURCES.talos, INITIAL_SOURCES.ollama),
  ];
}

export const useCanvasStore = create<CanvasState>((set, get) => ({
  watchGeneration: 0,
  sourceRevision: 0,
  resetSources: () =>
    set((state) => ({
      sourceRevision: state.sourceRevision + 1,
      watchGeneration: state.watchGeneration + 1,
      target: null,
      nodes: initialNodes(),
      edges: [],
      selectedNodeId: null,
      selectedEdgeId: null,
      sources: INITIAL_SOURCES,
      metrics: EMPTY_HISTORY,
      alerts: [],
      lastRefresh: null,
      refreshError: null,
      refreshing: false,
    })),
  target: null,
  nodes: initialNodes(),
  edges: [],
  selectedNodeId: null,
  selectedEdgeId: null,
  refreshing: false,
  lastRefresh: null,
  refreshError: null,
  sources: INITIAL_SOURCES,
  watch: watchStates('stopped'),
  metrics: EMPTY_HISTORY,
  alerts: [],

  onNodesChange: (changes) => {
    // Layer bands are structure, not content: they cannot be removed.
    const allowed = changes.filter(
      (change) =>
        change.type !== 'remove' ||
        (!isLayerNodeId(change.id) &&
          get().nodes.find((node) => node.id === change.id)?.data.source === 'planned')
    );
    set({ nodes: applyNodeChanges<CanvasNode>(allowed, get().nodes) });
  },

  onEdgesChange: (changes) => {
    set({
      edges: applyEdgeChanges(
        changes.filter((change) => change.type !== 'remove'),
        get().edges
      ),
    });
  },

  selectNode: (id) => {
    set({ selectedNodeId: id, selectedEdgeId: null });
  },

  selectEdge: (id) => {
    set({ selectedEdgeId: id, selectedNodeId: null });
  },

  addPlannedNode: (entry, flowPosition) => {
    // Planned workloads always land in the Workloads layer, wherever the drop happened; a
    // keyboard add takes the next slot along, stepping so several stay visible.
    const nodes = get().nodes;
    const band = nodes.find((node) => node.id === layerNodeId('workloads'));
    const origin = band?.position ?? layerPosition('workloads');
    const height = band?.height ?? LAYER_HEIGHT;
    const alreadyPlanned = nodes.filter(
      (node) => node.type === 'fact' && node.data.source === 'planned'
    ).length;
    const relative: XYPosition = flowPosition
      ? {
          x: Math.max(8, flowPosition.x - origin.x),
          y: Math.min(Math.max(8, flowPosition.y - origin.y), height - 80),
        }
      : {
          x: FACT_LEFT + alreadyPlanned * 24,
          y: Math.min(FACT_TOP + alreadyPlanned * 24, height - 80),
        };
    const node = plannedFact(entry, relative);
    set({ nodes: [...get().nodes, node], selectedNodeId: node.id });
  },

  refresh: async () => {
    if (get().refreshing) return;
    const revision = get().sourceRevision;
    const before = get().sources;
    set({ refreshing: true });

    const [cluster, talos, ollama] = await Promise.all([
      settle(api.cluster.summary()),
      settle(api.talos.inventory()),
      settle(api.ollama.status()),
    ]);

    // A one-shot that fails while a good answer is on screen keeps that answer: the watches
    // are still feeding it, and a timeout on the Refresh button should not blank three layers.
    const state = get();
    if (state.sourceRevision !== revision) return;
    const fresh = { cluster, talos, ollama };
    const kept: Sources = {
      cluster:
        state.sources.cluster !== before.cluster && state.sources.cluster.ok
          ? state.sources.cluster
          : keepGood(state.sources.cluster, cluster),
      talos:
        state.sources.talos !== before.talos && state.sources.talos.ok
          ? state.sources.talos
          : keepGood(state.sources.talos, talos),
      ollama:
        state.sources.ollama !== before.ollama && state.sources.ollama.ok
          ? state.sources.ollama
          : keepGood(state.sources.ollama, ollama),
    };
    const stale = (['cluster', 'talos', 'ollama'] as const).filter(
      (source) => !fresh[source].ok && kept[source].ok
    );
    const next = rebuilt(
      state.nodes,
      state.edges,
      state.target,
      kept,
      state.metrics.latest,
      powerBudget()
    );
    const first = stale[0] === undefined ? null : fresh[stale[0]];
    set({
      ...next,
      refreshError:
        first && !first.ok
          ? `${String(stale.length)} of 3 sources did not answer (${first.error}); showing the last good read`
          : next.refreshError,
      refreshing: false,
    });
  },

  applyCluster: (summary, generation) => {
    if (generation !== undefined && generation !== get().watchGeneration) return;
    const state = get();
    set(
      rebuilt(
        state.nodes,
        state.edges,
        state.target,
        { ...state.sources, cluster: { ok: true, value: summary } },
        state.metrics.latest,
        powerBudget()
      )
    );
  },

  applyTalos: (inventory, generation) => {
    if (generation !== undefined && generation !== get().watchGeneration) return;
    const state = get();
    set(
      rebuilt(
        state.nodes,
        state.edges,
        state.target,
        { ...state.sources, talos: { ok: true, value: inventory } },
        state.metrics.latest,
        powerBudget()
      )
    );
  },

  applyMetrics: (sample, generation) => {
    if (generation !== undefined && generation !== get().watchGeneration) return;
    const state = get();
    const budget = powerBudget();
    set({
      metrics: pushSample(state.metrics, sample),
      alerts: alertsFor(sample, budget),
      ...rebuilt(state.nodes, state.edges, state.target, state.sources, sample, budget),
    });
  },

  setWatchState: (watchState, generation) => {
    if (generation !== undefined && generation !== get().watchGeneration) return;
    set((state) => ({ watch: { ...state.watch, [watchState.source]: watchState } }));
  },

  startWatch: async () => {
    const generation = get().watchGeneration + 1;
    set({ watchGeneration: generation });
    // The badge changes inside the queue too, so a late stop cannot outrun a later start.
    await queued(async () => {
      if (get().watchGeneration === generation) set({ watch: watchStates('connecting') });
      try {
        await api.watch.start(generation);
      } catch (error: unknown) {
        // The command fails before either watch starts, so both sources carry the reason.
        if (get().watchGeneration === generation)
          set({ watch: watchStates('error', describeError(error)) });
      }
    });
  },

  stopWatch: async () => {
    const generation = get().watchGeneration + 1;
    set({ watchGeneration: generation });
    await queued(async () => {
      try {
        await api.watch.stop();
      } catch (error: unknown) {
        console.error(describeErrorForLog(error));
      }
      if (get().watchGeneration === generation) set({ watch: watchStates('stopped') });
    });
  },
}));

/** The settings the Rust side captures when a watch starts; a change means a restart. */
const WATCH_SETTINGS: readonly SettingKey[] = [
  'kubeconfig.path',
  'kubeconfig.context',
  'talosconfig.path',
  'talosctl.path',
  'workloads.apps',
  'workloads.platform',
  'metrics.interval',
  'metrics.dcgm.service',
  'metrics.sensors',
];

useSettingsStore.subscribe((state, previous) => {
  if (!state.loaded || !previous.loaded) return;
  const changed = (key: SettingKey) => state.values[key] !== previous.values[key];
  const sourceChanged = (
    [
      'kubeconfig.path',
      'kubeconfig.context',
      'talosconfig.path',
      'talosctl.path',
      'ollama.url',
    ] as const
  ).some(changed);
  if (!sourceChanged && !WATCH_SETTINGS.some(changed)) return;
  const canvas = useCanvasStore.getState();
  const running = Object.values(canvas.watch).some((watch) => watch.phase !== 'stopped');
  if (sourceChanged) canvas.resetSources();
  if (changed('metrics.interval')) {
    useCanvasStore.setState({ metrics: EMPTY_HISTORY, alerts: [] });
  }
  if (running) void canvas.startWatch();
  if (sourceChanged) void useCanvasStore.getState().refresh();
});
