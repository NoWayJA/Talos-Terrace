import type { Gauge } from './types';

const RADIUS = 14;
const STROKE = 3.5;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

/** A ring for a fraction, a value tile for a number with no maximum; colour says the level. */
export function GaugeRing({ gauge }: { gauge: Gauge }) {
  const classes = ['gauge', `gauge--${gauge.level}`];
  const title = gauge.detail ? `${gauge.label}: ${gauge.detail}` : `${gauge.label}: ${gauge.text}`;

  if (gauge.fraction === null) {
    classes.push('gauge--value');
    return (
      <div
        className={classes.join(' ')}
        title={title}
        role="img"
        aria-label={`${gauge.label} ${gauge.text}`}
      >
        <span className="gauge__dial">
          <span className="gauge__text">{gauge.text}</span>
        </span>
        <span className="gauge__label">{gauge.label}</span>
      </div>
    );
  }

  const offset = CIRCUMFERENCE * (1 - Math.min(1, Math.max(0, gauge.fraction)));
  const committed =
    gauge.committed === undefined
      ? null
      : CIRCUMFERENCE * (1 - Math.min(1, Math.max(0, gauge.committed)));
  return (
    <div
      className={classes.join(' ')}
      title={title}
      role="img"
      aria-label={`${gauge.label} ${gauge.text}`}
    >
      <span className="gauge__dial">
        <svg className="gauge__ring" viewBox="0 0 36 36" aria-hidden="true">
          <circle className="gauge__track" cx="18" cy="18" r={RADIUS} strokeWidth={STROKE} />
          <circle
            className={committed === null ? 'gauge__fill' : 'gauge__fill gauge__fill--soft'}
            cx="18"
            cy="18"
            r={RADIUS}
            strokeWidth={STROKE}
            strokeDasharray={CIRCUMFERENCE}
            strokeDashoffset={offset}
          />
          {committed === null ? null : (
            <circle
              className="gauge__fill"
              cx="18"
              cy="18"
              r={RADIUS}
              strokeWidth={STROKE}
              strokeDasharray={CIRCUMFERENCE}
              strokeDashoffset={committed}
            />
          )}
        </svg>
        <span className="gauge__text">{gauge.text}</span>
      </span>
      <span className="gauge__label">{gauge.label}</span>
    </div>
  );
}
