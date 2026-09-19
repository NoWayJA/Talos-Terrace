import type { Node } from '@xyflow/react';
import type { DocLink } from '../docs';
import type { LayerId } from '../layers';

export type FactStatus = 'ok' | 'warn' | 'error' | 'unknown' | 'planned';
export type FactSource = 'live' | 'planned';

export interface Fact {
  label: string;
  value: string;
}

export type GaugeLevel = 'ok' | 'warn' | 'error' | 'unknown';

/** A live number on a card: a ring when it is a fraction of something, a value tile otherwise. */
export interface Gauge {
  id: string;
  label: string;
  /** 0 to 1, or null for a value with no natural maximum (watts). */
  fraction: number | null;
  /**
   * A second, solid arc under `fraction`: the part that cannot be given back, such as
   * resident memory beneath a working set that includes cache. Absent for plain rings.
   */
  committed?: number;
  text: string;
  level: GaugeLevel;
  /** The long form for a tooltip: "38.0 of 62.1 GiB". */
  detail?: string;
}

export interface FactNodeData extends Record<string, unknown> {
  layer: LayerId;
  title: string;
  subtitle?: string;
  status: FactStatus;
  source: FactSource;
  /** The Talos node address or Kubernetes node name a node-scoped fact belongs to. */
  node?: string;
  facts: Fact[];
  docs?: DocLink[];
  /** Live dials, refreshed with every metrics sample. */
  gauges?: Gauge[];
  /** The metrics history key (`node:<name>`, `gpu:<uuid>`) the properties panel charts. */
  metricsKey?: string;
}

export interface LayerNodeData extends Record<string, unknown> {
  layer: LayerId;
  label: string;
  description: string;
}

export type FactNode = Node<FactNodeData, 'fact'>;
export type LayerNode = Node<LayerNodeData, 'layer'>;
export type CanvasNode = FactNode | LayerNode;

export function layerNodeId(layer: LayerId): string {
  return `layer:${layer}`;
}

export function isLayerNodeId(id: string): boolean {
  return id.startsWith('layer:');
}
