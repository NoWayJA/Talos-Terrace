import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@/services/api', () => ({
  api: {
    settings: { all: vi.fn(), set: vi.fn() },
  },
}));

import { api } from '@/services/api';
import { powerBudgetWatts, useSettingsStore } from './useSettingsStore';

describe('useSettingsStore', () => {
  beforeEach(() => {
    useSettingsStore.setState(useSettingsStore.getInitialState());
    vi.mocked(api.settings.all).mockReset();
    vi.mocked(api.settings.set).mockReset();
  });

  it('loads known rows and ignores unknown ones', async () => {
    vi.mocked(api.settings.all).mockResolvedValue([
      { key: 'metrics.interval', value: '10', isDefault: false },
      { key: 'something.else', value: 'x', isDefault: false },
    ]);
    await useSettingsStore.getState().load();
    const state = useSettingsStore.getState();
    expect(state.loaded).toBe(true);
    expect(state.values['metrics.interval']).toBe('10');
    expect(state.fromDefault['metrics.interval']).toBe(false);
    expect(state.fromDefault['kubeconfig.path']).toBe(true);
  });

  it('writes first and adopts the value only once the write succeeded', async () => {
    vi.mocked(api.settings.set).mockResolvedValue(undefined);
    vi.mocked(api.settings.all).mockResolvedValue([
      { key: 'power.budget.watts', value: '750', isDefault: false },
    ]);
    const failure = await useSettingsStore.getState().save('power.budget.watts', '750');
    expect(failure).toBeNull();
    expect(api.settings.set).toHaveBeenCalledWith('power.budget.watts', '750');
    expect(useSettingsStore.getState().values['power.budget.watts']).toBe('750');
  });

  it('reports a failed write and keeps the old value', async () => {
    vi.mocked(api.settings.set).mockRejectedValue({ code: 'DATABASE', message: 'disk full' });
    const failure = await useSettingsStore.getState().save('metrics.interval', '10');
    expect(failure).toBe('disk full');
    expect(useSettingsStore.getState().values['metrics.interval']).toBe('5');
    expect(api.settings.all).not.toHaveBeenCalled();
  });

  it('reads the power budget as a positive number or nothing', () => {
    expect(powerBudgetWatts({ 'power.budget.watts': '750' })).toBe(750);
    expect(powerBudgetWatts({ 'power.budget.watts': '' })).toBeNull();
    expect(powerBudgetWatts({ 'power.budget.watts': '-5' })).toBeNull();
    expect(powerBudgetWatts({ 'power.budget.watts': 'lots' })).toBeNull();
  });
});
