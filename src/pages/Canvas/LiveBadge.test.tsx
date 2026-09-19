import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { LiveBadge } from './LiveBadge';

describe('LiveBadge', () => {
  it('shows the source and phase, and the reason on hover when there is one', () => {
    render(<LiveBadge label="Talos" state={{ source: 'talos', phase: 'live', message: null }} />);
    const live = screen.getByText('Talos live');
    expect(live.className).toContain('badge--success');
    expect(live.getAttribute('title')).toBeNull();

    render(
      <LiveBadge
        label="Kubernetes"
        state={{ source: 'cluster', phase: 'error', message: 'connection refused' }}
      />
    );
    const failed = screen.getByText('Kubernetes error');
    expect(failed.className).toContain('badge--error');
    expect(failed.getAttribute('title')).toBe('connection refused');
  });
});
