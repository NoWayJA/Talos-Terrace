/**
 * The six layers of a Talos node, top to bottom on the canvas. The trust boundary wraps the
 * workloads; hardware is the ground everything stands on. This order is the product.
 */
import { DOCS, type DocLink } from './docs';

export const LAYER_IDS = ['trust', 'workloads', 'gpu', 'kubernetes', 'talos', 'hardware'] as const;
export type LayerId = (typeof LAYER_IDS)[number];

export interface LayerDefinition {
  id: LayerId;
  label: string;
  description: string;
  /** Upstream documentation for the layer as a whole. */
  docs: DocLink;
}

export const LAYERS: readonly LayerDefinition[] = [
  {
    id: 'trust',
    label: 'Trust boundary',
    description: 'Secure Boot, UKI, disk encryption, SELinux, Pod Security, image digests',
    docs: DOCS.talosSecureBoot,
  },
  {
    id: 'workloads',
    label: 'Workloads',
    description: 'Your apps and virtual machines, and the Services that reach them',
    docs: DOCS.k8sDeployments,
  },
  {
    id: 'gpu',
    label: 'GPU',
    description: 'Accelerators the scheduler can hand out, and who holds them',
    docs: DOCS.talosNvidia,
  },
  {
    id: 'kubernetes',
    label: 'Kubernetes',
    description: 'Nodes, namespaces and their Pod Security level, platform add-ons, events',
    docs: DOCS.k8sNodes,
  },
  {
    id: 'talos',
    label: 'Talos',
    description: 'Version, stage, schematic and extensions, machine config, services',
    docs: DOCS.talosArchitecture,
  },
  {
    id: 'hardware',
    label: 'Hardware and firmware',
    description: 'System, processors, memory, disks by serial, links and addresses, PCI devices',
    docs: DOCS.talosDisks,
  },
];

/** Hex mirrors of the `--layer-*` tokens in styles/tokens.css, for the MiniMap. */
export const LAYER_COLORS: Record<LayerId, string> = {
  trust: '#eab308',
  workloads: '#8b5cf6',
  gpu: '#22c55e',
  kubernetes: '#3b82f6',
  talos: '#f97316',
  hardware: '#64748b',
};

export const LAYER_WIDTH = 1480;
/** Tall enough for one row of cards, each with a row of gauges and three facts. */
export const LAYER_HEIGHT = 232;
/** Every further row of cards adds this much to a band. */
export const ROW_HEIGHT = 184;
/** Cards per row: a band grows wider up to this, then taller, so thirty nodes stay readable. */
export const MAX_COLUMNS = 6;
export const LAYER_GAP = 14;

/** The height of each band, which depends on how many rows of cards it holds. */
export type LayerHeights = Record<LayerId, number>;

export const SINGLE_ROW_HEIGHTS: LayerHeights = {
  trust: LAYER_HEIGHT,
  workloads: LAYER_HEIGHT,
  gpu: LAYER_HEIGHT,
  kubernetes: LAYER_HEIGHT,
  talos: LAYER_HEIGHT,
  hardware: LAYER_HEIGHT,
};

export function layerHeight(rows: number): number {
  return LAYER_HEIGHT + Math.max(0, rows - 1) * ROW_HEIGHT;
}

export function isLayerId(value: unknown): value is LayerId {
  return typeof value === 'string' && (LAYER_IDS as readonly string[]).includes(value);
}

export function layerIndex(id: LayerId): number {
  return LAYER_IDS.indexOf(id);
}

export function layerById(id: LayerId): LayerDefinition {
  const layer = LAYERS.find((candidate) => candidate.id === id);
  if (!layer) throw new Error(`Unknown layer ${id}`);
  return layer;
}

/** Absolute canvas position of a layer band: below every band above it. */
export function layerPosition(
  id: LayerId,
  heights: LayerHeights = SINGLE_ROW_HEIGHTS
): { x: number; y: number } {
  let y = 0;
  for (const each of LAYER_IDS) {
    if (each === id) break;
    y += heights[each] + LAYER_GAP;
  }
  return { x: 0, y };
}

/** Which layer band a canvas y coordinate falls in, or null when it is in a gap or outside. */
export function layerAtY(y: number, heights: LayerHeights = SINGLE_ROW_HEIGHTS): LayerId | null {
  for (const id of LAYER_IDS) {
    const top = layerPosition(id, heights).y;
    if (y >= top && y < top + heights[id]) return id;
  }
  return null;
}
