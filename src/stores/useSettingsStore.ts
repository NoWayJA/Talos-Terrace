import { create } from 'zustand';
import { api } from '@/services/api';
import { describeError } from '@/services/errors';

/** Mirror of `settings::keys` in src-tauri/src/settings.rs. */
export const SETTING_KEYS = [
  'theme',
  'kubeconfig.path',
  'kubeconfig.context',
  'talosconfig.path',
  'talosctl.path',
  'ollama.url',
  'workloads.apps',
  'workloads.platform',
  'metrics.interval',
  'metrics.dcgm.service',
  'metrics.sensors',
  'power.budget.watts',
] as const;
export type SettingKey = (typeof SETTING_KEYS)[number];

export const THEMES = ['system', 'dark', 'light'] as const;
export type Theme = (typeof THEMES)[number];

export type SettingValues = Record<SettingKey, string>;
export type SettingFlags = Record<SettingKey, boolean>;

const EMPTY_VALUES: SettingValues = {
  theme: 'system',
  'kubeconfig.path': '',
  'kubeconfig.context': '',
  'talosconfig.path': '',
  'talosctl.path': '',
  'ollama.url': '',
  'workloads.apps': '',
  'workloads.platform': '',
  'metrics.interval': '5',
  'metrics.dcgm.service': '',
  'metrics.sensors': 'on',
  'power.budget.watts': '',
};

const ALL_DEFAULT: SettingFlags = {
  theme: true,
  'kubeconfig.path': true,
  'kubeconfig.context': true,
  'talosconfig.path': true,
  'talosctl.path': true,
  'ollama.url': true,
  'workloads.apps': true,
  'workloads.platform': true,
  'metrics.interval': true,
  'metrics.dcgm.service': true,
  'metrics.sensors': true,
  'power.budget.watts': true,
};

export function isSettingKey(key: string): key is SettingKey {
  return (SETTING_KEYS as readonly string[]).includes(key);
}

export function toTheme(value: string): Theme {
  return (THEMES as readonly string[]).includes(value) ? (value as Theme) : 'system';
}

interface SettingsState {
  values: SettingValues;
  fromDefault: SettingFlags;
  loaded: boolean;
  error: string | null;
  load: () => Promise<void>;
  /** Writes the value; resolves to the failure message when it could not be written. */
  save: (key: SettingKey, value: string) => Promise<string | null>;
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  values: { ...EMPTY_VALUES },
  fromDefault: { ...ALL_DEFAULT },
  loaded: false,
  error: null,

  load: async () => {
    try {
      const rows = await api.settings.all();
      const values: SettingValues = { ...EMPTY_VALUES };
      const fromDefault: SettingFlags = { ...ALL_DEFAULT };
      for (const row of rows) {
        if (isSettingKey(row.key)) {
          values[row.key] = row.value;
          fromDefault[row.key] = row.isDefault;
        }
      }
      set({ values, fromDefault, loaded: true, error: null });
    } catch (error: unknown) {
      set({ loaded: true, error: describeError(error) });
    }
  },

  save: async (key, value) => {
    try {
      await api.settings.set(key, value);
    } catch (error: unknown) {
      // Nothing was written: the store keeps the old value and the caller keeps the draft.
      return describeError(error);
    }
    set((state) => ({
      values: { ...state.values, [key]: value },
      fromDefault: { ...state.fromDefault, [key]: value === '' ? state.fromDefault[key] : false },
      error: null,
    }));
    // Re-read so an emptied field shows its default again; the write already succeeded, so
    // a failure here is reported without undoing it.
    await get().load();
    return null;
  },
}));

export function selectTheme(state: SettingsState): Theme {
  return toTheme(state.values.theme);
}

/** The power budget in watts, or null when unset or not a positive number. */
export function powerBudgetWatts(values: Pick<SettingValues, 'power.budget.watts'>): number | null {
  const budget = Number(values['power.budget.watts']);
  return Number.isFinite(budget) && budget > 0 ? budget : null;
}
