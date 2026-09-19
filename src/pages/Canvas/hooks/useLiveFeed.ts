import { useEffect } from 'react';
import { useTauriEvent } from '@/hooks/useTauriEvent';
import {
  WATCH_EVENTS,
  type ClusterSummary,
  type MetricsSample,
  type TalosInventory,
  type WatchState,
  type WatchMessage,
} from '@/services/api';
import { useCanvasStore } from './useCanvasStore';

/**
 * Subscribes to what the Rust watches push and keeps them running for as long as the app
 * is open, whichever page is showing. Mount it once, above the pages: a visit to Settings
 * must not abort the reflectors, and a Talos change while the canvas is not on screen must
 * still land in the store.
 */
export function useLiveFeed(): void {
  const applyCluster = useCanvasStore((state) => state.applyCluster);
  const applyTalos = useCanvasStore((state) => state.applyTalos);
  const applyMetrics = useCanvasStore((state) => state.applyMetrics);
  const setWatchState = useCanvasStore((state) => state.setWatchState);
  const refresh = useCanvasStore((state) => state.refresh);
  const startWatch = useCanvasStore((state) => state.startWatch);
  const stopWatch = useCanvasStore((state) => state.stopWatch);

  const clusterReady = useTauriEvent<WatchMessage<ClusterSummary>>(
    WATCH_EVENTS.cluster,
    (message) => {
      applyCluster(message.payload, message.generation);
    }
  );
  const talosReady = useTauriEvent<WatchMessage<TalosInventory>>(WATCH_EVENTS.talos, (message) => {
    applyTalos(message.payload, message.generation);
  });
  const metricsReady = useTauriEvent<WatchMessage<MetricsSample>>(
    WATCH_EVENTS.metrics,
    (message) => {
      applyMetrics(message.payload, message.generation);
    }
  );
  const stateReady = useTauriEvent<WatchMessage<WatchState>>(WATCH_EVENTS.state, (message) => {
    setWatchState(message.payload, message.generation);
  });

  const ready = clusterReady && talosReady && metricsReady && stateReady;
  useEffect(() => {
    if (!ready) return;
    void refresh();
    void startWatch();
    return () => {
      void stopWatch();
    };
  }, [ready, refresh, startWatch, stopWatch]);
}
