import type { NodeProps } from '@xyflow/react';
import type { LayerNode } from './types';

/** A layer band. Not draggable, not selectable, not deletable; fact nodes live inside it. */
export function LayerNodeComponent({ data }: NodeProps<LayerNode>) {
  return (
    <div className={`terrace-layer terrace-layer--${data.layer}`}>
      <div className="terrace-layer__label">
        <span className="terrace-layer__name">{data.label}</span>
        <span className="terrace-layer__description">{data.description}</span>
      </div>
    </div>
  );
}
