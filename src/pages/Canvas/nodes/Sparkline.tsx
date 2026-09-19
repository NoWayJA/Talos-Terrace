import type { Point } from '../metrics';

const WIDTH = 240;
const HEIGHT = 36;

interface SparklineProps {
  points: Point[];
  /** The top of the chart; the largest value seen when omitted. Fractions pass 1. */
  max?: number;
}

/** A tiny line chart of one series; the newest point is on the right. */
export function Sparkline({ points, max }: SparklineProps) {
  if (points.length < 2) {
    return <div className="sparkline sparkline--empty">collecting…</div>;
  }
  const top = max ?? Math.max(Number.EPSILON, ...points.map((point) => point.value));
  const step = WIDTH / (points.length - 1);
  const path = points
    .map((point, index) => {
      const x = index * step;
      const y = HEIGHT - Math.min(1, Math.max(0, point.value / top)) * (HEIGHT - 2) - 1;
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(' ');
  return (
    <svg
      className="sparkline"
      viewBox={`0 0 ${String(WIDTH)} ${String(HEIGHT)}`}
      preserveAspectRatio="none"
      aria-hidden="true"
    >
      <polyline className="sparkline__line" points={path} />
    </svg>
  );
}
