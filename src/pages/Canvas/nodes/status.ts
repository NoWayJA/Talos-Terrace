import type { FactSource, FactStatus } from './types';

export type BadgeVariant = 'default' | 'accent' | 'success' | 'warning' | 'error' | 'info';

export function statusBadge(status: FactStatus): BadgeVariant {
  switch (status) {
    case 'ok':
      return 'success';
    case 'warn':
      return 'warning';
    case 'error':
      return 'error';
    case 'planned':
      return 'accent';
    case 'unknown':
      return 'default';
  }
}

/** The status in plain words, for the panel and for screen readers. */
export function statusLabel(status: FactStatus): string {
  switch (status) {
    case 'ok':
      return 'Healthy';
    case 'warn':
      return 'Needs attention';
    case 'error':
      return 'Problem';
    case 'planned':
      return 'Planned';
    case 'unknown':
      return 'Unknown';
  }
}

export function sourceLabel(source: FactSource): string {
  switch (source) {
    case 'live':
      return 'Live from the API';
    case 'planned':
      return 'Planned, not applied';
  }
}
