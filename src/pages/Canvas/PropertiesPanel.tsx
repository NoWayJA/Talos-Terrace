import { openUrl } from '@tauri-apps/plugin-opener';
import { ExternalLink, X } from 'lucide-react';
import { describeError } from '@/services/errors';
import { DOCS, type DocLink } from './docs';
import { exposureText, servicePortsLabel, type ServiceEdgeData } from './facts';
import { useCanvasStore } from './hooks/useCanvasStore';
import { layerById } from './layers';
import { formatValue, seriesFor, type SeriesUnit } from './metrics';
import { GaugeRing } from './nodes/Gauge';
import { Sparkline } from './nodes/Sparkline';
import { sourceLabel, statusBadge, statusLabel } from './nodes/status';
import type { FactNode } from './nodes/types';

/** The top of a sparkline for units with a natural ceiling; others scale to what was seen. */
function ceilingFor(unit: SeriesUnit): number | undefined {
  switch (unit) {
    case 'fraction':
      return 1;
    case 'percent':
    case 'celsius':
      return 100;
    default:
      return undefined;
  }
}

/** Ten minutes of every series behind a card, newest value beside each. */
function LiveSeries({ metricsKey }: { metricsKey: string }) {
  const history = useCanvasStore((state) => state.metrics);
  const series = seriesFor(history, metricsKey);
  if (series.length === 0) return null;
  return (
    <>
      <h3 className="properties__section-title">Live</h3>
      <div className="properties__series">
        {series.map((entry) => {
          const last = entry.points[entry.points.length - 1];
          return (
            <div key={entry.id}>
              <div className="properties__series-head">
                <span>{entry.label}</span>
                <span className="properties__series-value">
                  {last ? formatValue(entry.unit, last.value) : ''}
                </span>
              </div>
              <Sparkline points={entry.points} max={ceilingFor(entry.unit)} />
            </div>
          );
        })}
      </div>
    </>
  );
}

function useSelectedFact(): FactNode | null {
  return useCanvasStore((state) => {
    const node = state.nodes.find((candidate) => candidate.id === state.selectedNodeId);
    return node?.type === 'fact' ? node : null;
  });
}

function useSelectedService(): ServiceEdgeData | null {
  return useCanvasStore((state) => {
    const edge = state.edges.find((candidate) => candidate.id === state.selectedEdgeId);
    return (edge?.data as ServiceEdgeData | undefined) ?? null;
  });
}

function openDoc(doc: DocLink) {
  openUrl(doc.url).catch((error: unknown) => {
    console.error(describeError(error));
  });
}

export function PropertiesPanel() {
  const fact = useSelectedFact();
  const service = useSelectedService();
  const selectNode = useCanvasStore((state) => state.selectNode);

  if (service) {
    return (
      <ServiceProperties
        data={service}
        onClose={() => {
          selectNode(null);
        }}
      />
    );
  }

  if (!fact) {
    return (
      <aside className="properties properties--empty" aria-label="Properties">
        <p>Select a node to see its facts.</p>
      </aside>
    );
  }

  const layer = layerById(fact.data.layer);
  const docs = fact.data.docs ?? [layer.docs];

  return (
    <aside className="properties" aria-label="Properties">
      <div className="properties__header">
        <h2 className="properties__title">{fact.data.title}</h2>
        <button
          type="button"
          className="btn btn--ghost btn--icon btn--sm"
          onClick={() => {
            selectNode(null);
          }}
          aria-label="Close"
        >
          <X size={14} />
        </button>
      </div>

      <div className="properties__body">
        <dl className="properties__meta">
          <div>
            <dt>Layer</dt>
            <dd>{layer.label}</dd>
          </div>
          <div>
            <dt>Status</dt>
            <dd>
              <span className={`badge badge--${statusBadge(fact.data.status)}`}>
                {statusLabel(fact.data.status)}
              </span>
            </dd>
          </div>
          <div>
            <dt>Source</dt>
            <dd>{sourceLabel(fact.data.source)}</dd>
          </div>
          {fact.data.node ? (
            <div>
              <dt>Node</dt>
              <dd>{fact.data.node}</dd>
            </div>
          ) : null}
          {fact.data.subtitle ? (
            <div>
              <dt>Detail</dt>
              <dd>{fact.data.subtitle}</dd>
            </div>
          ) : null}
        </dl>

        {fact.data.gauges && fact.data.gauges.length > 0 ? (
          <div className="terrace-node__gauges">
            {fact.data.gauges.map((gauge) => (
              <GaugeRing key={gauge.id} gauge={gauge} />
            ))}
          </div>
        ) : null}

        {fact.data.facts.length > 0 ? (
          <>
            <h3 className="properties__section-title">Facts</h3>
            <dl className="properties__facts">
              {fact.data.facts.map((entry, index) => (
                <div key={`${entry.label}:${String(index)}`}>
                  <dt>{entry.label}</dt>
                  <dd>{entry.value}</dd>
                </div>
              ))}
            </dl>
          </>
        ) : null}

        {fact.data.metricsKey ? <LiveSeries metricsKey={fact.data.metricsKey} /> : null}

        <h3 className="properties__section-title">Documentation</h3>
        <div className="properties__docs">
          {docs.map((doc) => (
            <button
              key={doc.url}
              type="button"
              className="btn btn--secondary btn--sm"
              onClick={() => {
                openDoc(doc);
              }}
            >
              <ExternalLink size={14} />
              {doc.label}
            </button>
          ))}
        </div>
      </div>
    </aside>
  );
}

/** A Service edge: how the cluster reaches the workloads it selects. */
function ServiceProperties({ data, onClose }: { data: ServiceEdgeData; onClose: () => void }) {
  const { service, exposure } = data;
  return (
    <aside className="properties" aria-label="Properties">
      <div className="properties__header">
        <h2 className="properties__title">{service.name}</h2>
        <button
          type="button"
          className="btn btn--ghost btn--icon btn--sm"
          onClick={onClose}
          aria-label="Close"
        >
          <X size={14} />
        </button>
      </div>

      <div className="properties__body">
        <dl className="properties__meta">
          <div>
            <dt>Kind</dt>
            <dd>Service · {service.type}</dd>
          </div>
          <div>
            <dt>Namespace</dt>
            <dd>{service.namespace}</dd>
          </div>
          <div>
            <dt>Reachable</dt>
            <dd>{exposureText(exposure)}</dd>
          </div>
          <div>
            <dt>Cluster address</dt>
            <dd>{service.clusterIp ?? 'none (headless)'}</dd>
          </div>
          {service.externalIps.length > 0 ? (
            <div>
              <dt>External</dt>
              <dd>{service.externalIps.join(', ')}</dd>
            </div>
          ) : null}
        </dl>

        <h3 className="properties__section-title">Ports</h3>
        <dl className="properties__facts">
          {service.ports.map((port) => (
            <div key={`${String(port.port)}/${port.protocol}`}>
              <dt>{port.name ?? String(port.port)}</dt>
              <dd>{servicePortsLabel({ ...service, ports: [port] })}</dd>
            </div>
          ))}
        </dl>

        <h3 className="properties__section-title">Reaches</h3>
        <dl className="properties__facts">
          {service.targets.map((target) => (
            <div key={`${target.kind}/${target.name}`}>
              <dt>{target.kind}</dt>
              <dd>{target.name}</dd>
            </div>
          ))}
        </dl>

        <h3 className="properties__section-title">Documentation</h3>
        <ul className="properties__docs">
          <li>
            <button
              type="button"
              className="btn btn--ghost btn--sm"
              onClick={() => {
                openDoc(DOCS.k8sServices);
              }}
            >
              <ExternalLink size={12} /> {DOCS.k8sServices.label}
            </button>
          </li>
        </ul>
      </div>
    </aside>
  );
}
