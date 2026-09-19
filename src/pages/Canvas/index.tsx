import { RefreshCw } from 'lucide-react';
import { useState } from 'react';
import type { WatchSource } from '@/services/api';
import { ClusterCanvas } from './ClusterCanvas';
import { useCanvasStore } from './hooks/useCanvasStore';
import { LiveBadge } from './LiveBadge';
import { NodePalette } from './NodePalette';
import { PropertiesPanel } from './PropertiesPanel';

const SOURCE_LABELS: Record<WatchSource, string> = {
  cluster: 'Kubernetes',
  talos: 'Talos',
  metrics: 'Metrics',
};

function formatTime(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : date.toLocaleTimeString();
}

interface CanvasPageProps {
  onOpenSettings: () => void;
}

export function CanvasPage({ onOpenSettings }: CanvasPageProps) {
  const target = useCanvasStore((state) => state.target);
  const refreshing = useCanvasStore((state) => state.refreshing);
  const lastRefresh = useCanvasStore((state) => state.lastRefresh);
  const refreshError = useCanvasStore((state) => state.refreshError);
  const watch = useCanvasStore((state) => state.watch);
  const refresh = useCanvasStore((state) => state.refresh);
  const alerts = useCanvasStore((state) => state.alerts);
  const sources = useCanvasStore((state) => state.sources);
  const [stripOpen, setStripOpen] = useState(false);

  const failures = (Object.keys(SOURCE_LABELS) as WatchSource[])
    .map((source) => watch[source])
    .filter((state) => state.phase === 'error' && state.message);
  const nothingYet = !sources.cluster.ok && !sources.talos.ok;
  const hasDetails = alerts.length > 0 || failures.length > 0;

  return (
    <section className="canvas-page">
      <header className="canvas-page__header">
        <div className="canvas-page__heading">
          <h1 className="canvas-page__title" title={target ?? undefined}>
            {target ?? 'Cluster'}
          </h1>
          <span className="canvas-page__meta">
            {lastRefresh ? `Updated ${formatTime(lastRefresh)}` : 'Waiting for the first read'}
          </span>
          {refreshError ? <span className="badge badge--warning">{refreshError}</span> : null}
          {alerts.length > 0 ? (
            <button
              type="button"
              className={`badge badge--button ${alerts.some((alert) => alert.level === 'error') ? 'badge--error' : 'badge--warning'}`}
              aria-expanded={stripOpen}
              aria-controls="canvas-details"
              onClick={() => {
                setStripOpen((open) => !open);
              }}
            >
              {String(alerts.length)} {alerts.length === 1 ? 'alert' : 'alerts'}
            </button>
          ) : null}
        </div>
        <div className="canvas-page__live">
          {(Object.keys(SOURCE_LABELS) as WatchSource[]).map((source) => (
            <LiveBadge
              key={source}
              label={SOURCE_LABELS[source]}
              state={watch[source]}
              onOpen={
                watch[source].message
                  ? () => {
                      setStripOpen((open) => !open);
                    }
                  : undefined
              }
              expanded={stripOpen}
            />
          ))}
        </div>
        <button
          type="button"
          className="btn btn--secondary btn--sm"
          onClick={() => {
            void refresh();
          }}
          disabled={refreshing}
        >
          <RefreshCw size={14} className={refreshing ? 'spin' : undefined} />
          {refreshing ? 'Refreshing' : 'Refresh'}
        </button>
      </header>
      {stripOpen && hasDetails ? (
        <div id="canvas-details" className="canvas-page__strip" role="region" aria-label="Details">
          <ul>
            {failures.map((state) => (
              <li
                key={state.source}
                className="canvas-page__strip-item canvas-page__strip-item--error"
              >
                <dl>
                  <dt>{SOURCE_LABELS[state.source]}: </dt>
                  <dd>{state.message}</dd>
                </dl>
              </li>
            ))}
            {alerts.map((alert) => (
              <li
                key={alert.id}
                className={`canvas-page__strip-item canvas-page__strip-item--${alert.level}`}
              >
                {alert.text}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {nothingYet && lastRefresh ? (
        <div className="canvas-page__notice" role="status">
          <p>
            Nothing is connected yet. Terrace reads your kubeconfig and talosconfig where they
            already live; tell it where they are in Settings and run the connection tests.
          </p>
          <button type="button" className="btn btn--primary btn--sm" onClick={onOpenSettings}>
            Open Settings
          </button>
        </div>
      ) : null}
      <div className="canvas-page__body">
        <NodePalette />
        <ClusterCanvas />
        <PropertiesPanel />
      </div>
    </section>
  );
}
