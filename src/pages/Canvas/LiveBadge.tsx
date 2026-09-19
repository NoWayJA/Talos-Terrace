import type { WatchPhase, WatchState } from '@/services/api';

const PHASE_CLASS: Record<WatchPhase, string> = {
  live: 'badge--success',
  connecting: 'badge--info',
  error: 'badge--error',
  stopped: 'badge--default',
};

const PHASE_TEXT: Record<WatchPhase, string> = {
  live: 'live',
  connecting: 'connecting',
  error: 'error',
  stopped: 'off',
};

interface LiveBadgeProps {
  label: string;
  state: WatchState;
  /** When set, the badge is a button that opens the details strip with the reason. */
  onOpen?: () => void;
  expanded?: boolean;
}

/** One badge per watched source: "Kubernetes live", "Talos error" (open it for the reason). */
export function LiveBadge({ label, state, onOpen, expanded }: LiveBadgeProps) {
  const text = `${label} ${PHASE_TEXT[state.phase]}`;
  if (onOpen) {
    return (
      <button
        type="button"
        className={`badge badge--button ${PHASE_CLASS[state.phase]}`}
        title={state.message ?? undefined}
        aria-expanded={expanded ?? false}
        aria-controls="canvas-details"
        onClick={onOpen}
      >
        {text}
      </button>
    );
  }
  return (
    <span className={`badge ${PHASE_CLASS[state.phase]}`} title={state.message ?? undefined}>
      {text}
    </span>
  );
}
