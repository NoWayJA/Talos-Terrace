import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { Sidebar } from './Sidebar';

describe('Sidebar', () => {
  it('marks the current page and navigates on click', () => {
    const onNavigate = vi.fn();
    render(
      <Sidebar
        current="canvas"
        onNavigate={onNavigate}
        collapsed={false}
        onToggle={() => undefined}
        version="0.0.1"
      />
    );
    expect(screen.getByRole('button', { name: 'Canvas' }).getAttribute('aria-current')).toBe(
      'page'
    );
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(onNavigate).toHaveBeenCalledWith('settings');
    expect(screen.getByText('v0.0.1')).toBeTruthy();
  });

  it('collapses and toggles', () => {
    const onToggle = vi.fn();
    render(
      <Sidebar
        current="settings"
        onNavigate={() => undefined}
        collapsed
        onToggle={onToggle}
        version=""
      />
    );
    fireEvent.click(screen.getByRole('button', { name: 'Expand sidebar' }));
    expect(onToggle).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('navigation').className).toContain('sidebar--collapsed');
  });
});
