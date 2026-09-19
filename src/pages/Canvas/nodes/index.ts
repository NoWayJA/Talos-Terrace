// Module-level nodeTypes, the pattern lifted from Modz (NoWayJA/Modz, MIT)
// pages/Workflows/nodes/index.ts. Defining it once outside any component keeps
// React Flow from remounting every node on each render.
import type { NodeTypes } from '@xyflow/react';
import { FactNodeComponent } from './FactNode';
import { LayerNodeComponent } from './LayerNode';

export const nodeTypes = {
  fact: FactNodeComponent,
  layer: LayerNodeComponent,
} satisfies NodeTypes;
