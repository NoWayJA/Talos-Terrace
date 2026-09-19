import { useState } from 'react';
import { api } from '@/services/api';
import { describeError } from '@/services/errors';
import {
  THEMES,
  toTheme,
  useSettingsStore,
  type SettingKey,
  type Theme,
} from '@/stores/useSettingsStore';

interface FieldDefinition {
  key: SettingKey;
  label: string;
  help: string;
  /** Returns what is wrong with a value, or null when it can be saved. */
  validate?: (value: string) => string | null;
}

const THEME_LABELS: Record<Theme, string> = { system: 'System', dark: 'Dark', light: 'Light' };

function wholeSeconds(value: string): string | null {
  if (value === '') return null;
  const seconds = Number(value);
  return /^\d+$/.test(value) && (seconds === 0 || (seconds >= 2 && seconds <= 300))
    ? null
    : '2–300 whole seconds, or 0 to switch metrics off';
}

function positiveNumber(value: string): string | null {
  if (value === '') return null;
  const number = Number(value);
  return Number.isFinite(number) && number > 0 ? null : 'A number of watts above zero, or empty';
}

function onOrOff(value: string): string | null {
  return value === '' || value === 'on' || value === 'off' ? null : 'on or off';
}

function exporterSetting(value: string): string | null {
  if (value === '' || value === 'off') return null;
  return /^[a-z0-9-]+\/[a-z0-9-]+(:\d+)?$/.test(value)
    ? null
    : 'namespace/name, optionally :port; or off; or empty to look for nvidia-dcgm-exporter';
}

const SOURCE_FIELDS: readonly FieldDefinition[] = [
  {
    key: 'kubeconfig.path',
    label: 'Kubeconfig path',
    help: 'Read in place; only the path is stored. Default: ~/.kube/config',
  },
  {
    key: 'kubeconfig.context',
    label: 'Kubernetes context',
    help: "Empty uses the file's current-context",
  },
  {
    key: 'talosconfig.path',
    label: 'Talosconfig path',
    help: 'Default: $TALOSCONFIG, then ~/.talos/config',
  },
  {
    key: 'talosctl.path',
    label: 'talosctl binary',
    help: 'Default: talosctl on PATH',
  },
  {
    key: 'ollama.url',
    label: 'Ollama URL (optional)',
    help: 'Leave empty unless an Ollama server should appear on the canvas, for example http://localhost:11434 through a kubectl port-forward',
  },
];

const WORKLOAD_FIELDS: readonly FieldDefinition[] = [
  {
    key: 'workloads.apps',
    label: 'Namespaces that are your apps',
    help: 'Comma separated. Their workloads get their own cards in the Workloads layer even if they look like platform machinery',
  },
  {
    key: 'workloads.platform',
    label: 'Namespaces that are platform',
    help: 'Comma separated. Their workloads fold into one card per namespace in the Kubernetes layer. By default, kube-system is Kubernetes itself, namespaces that hold an operator (gpu-operator, kubevirt, cdi, *-system, *-storage) are platform, and everything else is yours',
  },
];

const METRICS_FIELDS: readonly FieldDefinition[] = [
  {
    key: 'metrics.interval',
    label: 'Sample interval (seconds)',
    help: 'How often node, pod and GPU numbers are read through the APIs you already use. 0 switches live metrics off. Default: 5',
    validate: wholeSeconds,
  },
  {
    key: 'metrics.dcgm.service',
    label: 'GPU metrics exporter (optional)',
    help: 'Empty finds a Service named nvidia-dcgm-exporter, which the NVIDIA GPU Operator installs. Otherwise namespace/name[:port], or off',
    validate: exporterSetting,
  },
  {
    key: 'metrics.sensors',
    label: 'Host temperature sensors',
    help: 'on reads the CPU and drive sensors through the Talos API every third sample; off skips them. Reading them needs an os:admin talosconfig',
    validate: onOrOff,
  },
  {
    key: 'power.budget.watts',
    label: 'Power budget (watts, optional)',
    help: "Shown against the GPUs' combined power draw, for example your power supply's rating",
    validate: positiveNumber,
  },
];

type TestId = 'cluster' | 'talos' | 'ollama';

type TestState =
  | { state: 'idle' }
  | { state: 'running' }
  | { state: 'ok'; detail: string }
  | { state: 'error'; detail: string };

const TESTS: Record<TestId, { label: string; run: () => Promise<string> }> = {
  cluster: {
    label: 'Kubernetes API',
    run: async () => {
      const summary = await api.cluster.summary();
      const apps = summary.workloads.filter((workload) => workload.role === 'app').length;
      const platform = new Set(
        summary.workloads
          .filter((workload) => workload.role === 'platform')
          .map((workload) => workload.namespace)
      ).size;
      return `${summary.context}: ${String(summary.nodes.length)} node(s), ${summary.version}, ${String(apps)} app(s), ${String(platform)} platform namespace(s)`;
    },
  },
  talos: {
    label: 'Talos API (talosctl)',
    run: async () => {
      const inventory = await api.talos.inventory();
      const names = inventory.nodes.map((node) => node.hostname ?? node.node).join(', ');
      return `server ${inventory.version.server ?? 'unreachable'}, ${String(inventory.nodes.length)} node(s): ${names}`;
    },
  },
  ollama: {
    label: 'Ollama',
    run: async () => {
      const status = await api.ollama.status();
      if (!status) return 'not configured (optional)';
      return `v${status.version}, ${String(status.models.length)} model(s), ${String(status.loaded.length)} loaded`;
    },
  },
};

const TEST_IDS: readonly TestId[] = ['cluster', 'talos', 'ollama'];

interface SettingFieldProps {
  definition: FieldDefinition;
}

function SettingField({ definition }: SettingFieldProps) {
  const value = useSettingsStore((state) => state.values[definition.key]);
  const fromDefault = useSettingsStore((state) => state.fromDefault[definition.key]);
  const save = useSettingsStore((state) => state.save);
  const [draft, setDraft] = useState(value);
  const [lastSeen, setLastSeen] = useState(value);
  const [problem, setProblem] = useState<string | null>(null);

  // Adopt a value that changed underneath us (a reload, or a reset to the default).
  if (value !== lastSeen) {
    setLastSeen(value);
    setDraft(value);
  }

  const commit = async () => {
    if (draft === value) return;
    const invalid = definition.validate?.(draft) ?? null;
    if (invalid) {
      setProblem(invalid);
      return;
    }
    // A failed write keeps the draft in the field, with the reason under it.
    setProblem(await save(definition.key, draft));
  };

  const inputId = `setting-${definition.key}`;
  const problemId = `${inputId}-problem`;
  return (
    <div className="settings__field">
      <label className="settings__label" htmlFor={inputId}>
        {definition.label}
        {fromDefault ? <span className="badge badge--default">default</span> : null}
      </label>
      <input
        id={inputId}
        className={problem ? 'input input--invalid' : 'input'}
        type="text"
        value={draft}
        spellCheck={false}
        aria-invalid={problem ? true : undefined}
        aria-describedby={problem ? problemId : undefined}
        onChange={(event) => {
          setDraft(event.target.value);
          if (problem) setProblem(definition.validate?.(event.target.value) ?? null);
        }}
        onBlur={() => {
          void commit();
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') void commit();
        }}
      />
      {problem ? (
        <p id={problemId} className="settings__problem" role="alert">
          {problem}
        </p>
      ) : null}
      <p className="settings__help">{definition.help}</p>
    </div>
  );
}

export function SettingsPage() {
  const loaded = useSettingsStore((state) => state.loaded);
  const error = useSettingsStore((state) => state.error);
  const theme = useSettingsStore((state) => toTheme(state.values.theme));
  const save = useSettingsStore((state) => state.save);
  const [tests, setTests] = useState<Record<TestId, TestState>>({
    cluster: { state: 'idle' },
    talos: { state: 'idle' },
    ollama: { state: 'idle' },
  });

  const runTest = async (id: TestId) => {
    setTests((previous) => ({ ...previous, [id]: { state: 'running' } }));
    try {
      const detail = await TESTS[id].run();
      setTests((previous) => ({ ...previous, [id]: { state: 'ok', detail } }));
    } catch (failure: unknown) {
      setTests((previous) => ({
        ...previous,
        [id]: { state: 'error', detail: describeError(failure) },
      }));
    }
  };

  return (
    <section className="settings">
      <header className="settings__header">
        <h1 className="settings__title">Settings</h1>
        <p className="settings__intro">
          Terrace reads your kubeconfig and talosconfig where they already live and stores only
          their paths. Nothing is installed on the cluster and nothing is copied into the app.
        </p>
        {error ? <p className="badge badge--error">{error}</p> : null}
      </header>

      {loaded ? (
        <>
          <div className="settings__group">
            <h2 className="settings__group-title">Sources</h2>
            {SOURCE_FIELDS.map((definition) => (
              <SettingField key={definition.key} definition={definition} />
            ))}
          </div>

          <div className="settings__group">
            <h2 className="settings__group-title">Workloads on the canvas</h2>
            <p className="settings__help">
              Terrace shows what you run as cards and folds the cluster&apos;s machinery away.
              Override the guess per namespace here; the canvas follows on its next update.
            </p>
            {WORKLOAD_FIELDS.map((definition) => (
              <SettingField key={definition.key} definition={definition} />
            ))}
          </div>

          <div className="settings__group">
            <h2 className="settings__group-title">Live metrics</h2>
            <p className="settings__help">
              Dials on the canvas come from the kubelet, a GPU exporter if one is running, and the
              hosts&apos; sensors. Nothing is installed on the cluster.
            </p>
            {METRICS_FIELDS.map((definition) => (
              <SettingField key={definition.key} definition={definition} />
            ))}
          </div>

          <div className="settings__group">
            <h2 className="settings__group-title">Connection tests</h2>
            <p className="settings__help">Read-only calls. Nothing on the cluster changes.</p>
            <ul className="settings__tests">
              {TEST_IDS.map((id) => {
                const result = tests[id];
                return (
                  <li key={id} className="settings__test">
                    <span className="settings__test-label">{TESTS[id].label}</span>
                    <button
                      type="button"
                      className="btn btn--secondary btn--sm"
                      disabled={result.state === 'running'}
                      onClick={() => {
                        void runTest(id);
                      }}
                    >
                      {result.state === 'running' ? 'Testing' : 'Test'}
                    </button>
                    {result.state === 'ok' ? (
                      <p className="settings__test-result settings__test-result--ok">
                        {result.detail}
                      </p>
                    ) : null}
                    {result.state === 'error' ? (
                      <p className="settings__test-result settings__test-result--error">
                        {result.detail}
                      </p>
                    ) : null}
                  </li>
                );
              })}
            </ul>
          </div>

          <div className="settings__group">
            <h2 className="settings__group-title">Appearance</h2>
            <div className="settings__field">
              <label className="settings__label" htmlFor="setting-theme">
                Theme
              </label>
              <select
                id="setting-theme"
                className="input"
                value={theme}
                onChange={(event) => {
                  void save('theme', toTheme(event.target.value));
                }}
              >
                {THEMES.map((option: Theme) => (
                  <option key={option} value={option}>
                    {THEME_LABELS[option]}
                  </option>
                ))}
              </select>
            </div>
          </div>
        </>
      ) : (
        <p className="settings__help">Loading settings…</p>
      )}
    </section>
  );
}
