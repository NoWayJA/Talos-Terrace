// Lifted from Modz (NoWayJA/Modz, MIT) pages/Workflows/nodes/BaseNode.tsx.
// Changes: layer colour and status come from class names rather than inline styles;
// the execution-status plumbing is gone; one handle each side, plus a top source and a
// bottom target so Service edges can climb between layers.
import { Handle, Position } from '@xyflow/react';
import type { ReactNode } from 'react';
import type { LayerId } from '../layers';
import type { FactStatus } from './types';

interface BaseNodeProps {
  layer: LayerId;
  title: string;
  status: FactStatus;
  selected: boolean | undefined;
  planned?: boolean;
  children?: ReactNode;
}

export function BaseNode({ layer, title, status, selected, planned, children }: BaseNodeProps) {
  const classes = ['terrace-node', `terrace-node--${layer}`, `terrace-node--status-${status}`];
  if (selected) classes.push('terrace-node--selected');
  if (planned) classes.push('terrace-node--planned');

  return (
    <div className={classes.join(' ')}>
      <Handle
        type="target"
        position={Position.Left}
        className="terrace-node__handle terrace-node__handle--input"
      />
      <div className="terrace-node__header">
        <span className="terrace-node__dot" aria-hidden="true" />
        <span className="terrace-node__title">{title}</span>
      </div>
      {children ? <div className="terrace-node__content">{children}</div> : null}
      <Handle
        type="source"
        position={Position.Right}
        className="terrace-node__handle terrace-node__handle--output"
      />
      <Handle
        type="source"
        position={Position.Top}
        id="up"
        className="terrace-node__handle terrace-node__handle--up"
      />
      <Handle
        type="target"
        position={Position.Bottom}
        id="down"
        className="terrace-node__handle terrace-node__handle--down"
      />
    </div>
  );
}
