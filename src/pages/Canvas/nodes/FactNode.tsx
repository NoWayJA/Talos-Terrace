import type { NodeProps } from '@xyflow/react';
import { BaseNode } from './BaseNode';
import { GaugeRing } from './Gauge';
import type { FactNode } from './types';

const MAX_FACTS_ON_NODE = 4;
const MAX_FACTS_WITH_GAUGES = 3;
const MAX_GAUGES_ON_NODE = 4;

export function FactNodeComponent({ data, selected }: NodeProps<FactNode>) {
  const gauges = data.gauges ?? [];
  const shown = data.facts.slice(0, gauges.length > 0 ? MAX_FACTS_WITH_GAUGES : MAX_FACTS_ON_NODE);
  return (
    <BaseNode
      layer={data.layer}
      title={data.title}
      status={data.status}
      selected={selected}
      planned={data.source === 'planned'}
    >
      {data.subtitle ? <div className="terrace-node__subtitle">{data.subtitle}</div> : null}
      {gauges.length > 0 ? (
        <div className="terrace-node__gauges">
          {gauges.slice(0, MAX_GAUGES_ON_NODE).map((gauge) => (
            <GaugeRing key={gauge.id} gauge={gauge} />
          ))}
        </div>
      ) : null}
      {shown.length > 0 ? (
        <dl className="terrace-node__facts">
          {shown.map((fact, index) => (
            <div key={`${fact.label}:${String(index)}`} className="terrace-node__fact">
              <dt>{fact.label}</dt>
              <dd>{fact.value}</dd>
            </div>
          ))}
        </dl>
      ) : null}
      {data.facts.length > shown.length ? (
        <div className="terrace-node__more">
          {String(data.facts.length - shown.length)} more in the panel
        </div>
      ) : null}
    </BaseNode>
  );
}
