import { describe, expect, it } from 'vitest';
import type { GpuMetrics, MetricsSample, NodeMetrics } from '@/services/api';
import {
  EMPTY_HISTORY,
  HISTORY_LENGTH,
  alertsFor,
  formatBytes,
  formatPercent,
  formatRate,
  formatValue,
  gpuGauges,
  gpuKey,
  levelFor,
  nodeGauges,
  nodeKey,
  powerGauge,
  pushSample,
  seriesFor,
  usageLabel,
  worstLevel,
} from './metrics';

const node: NodeMetrics = {
  node: 'node-a',
  cpuCores: 3.2,
  cpuCapacityCores: 32,
  memoryWorkingSetBytes: 40 * 1024 ** 3,
  memoryRssBytes: 21 * 1024 ** 3,
  memoryCapacityBytes: 62 * 1024 ** 3,
  fsUsedBytes: 50 * 1024 ** 3,
  fsCapacityBytes: 1800 * 1024 ** 3,
  networkReceiveBytesPerSecond: 1200,
  networkTransmitBytesPerSecond: null,
  temperatures: [
    { sensor: 'k10temp', label: 'CPU', celsius: 42 },
    { sensor: 'nvme', label: 'NVMe 1', celsius: 31 },
  ],
};

const gpu: GpuMetrics = {
  node: 'node-a',
  uuid: 'GPU-a',
  index: '0',
  model: 'Card A',
  pciBusId: '00000000:0A:00.0',
  utilizationPercent: 37,
  memoryUsedBytes: 6 * 1024 ** 3,
  memoryTotalBytes: 24 * 1024 ** 3,
  temperatureCelsius: 43,
  powerWatts: 28.4,
  smClockMhz: 210,
  memoryCopyPercent: 0,
};

function sample(overrides: Partial<MetricsSample> = {}): MetricsSample {
  return {
    at: '2026-01-01T00:00:00.000Z',
    intervalSeconds: 5,
    nodes: [node],
    gpus: [gpu],
    pods: [
      {
        namespace: 'apps',
        name: 'web-1',
        cpuCores: 0.05,
        memoryWorkingSetBytes: 1024 ** 3,
        memoryRssBytes: 512 * 1024 ** 2,
      },
      {
        namespace: 'apps',
        name: 'web-2',
        cpuCores: 0.02,
        memoryWorkingSetBytes: 512 * 1024 ** 2,
        memoryRssBytes: 512 * 1024 ** 2,
      },
      {
        namespace: 'other',
        name: 'web-1',
        cpuCores: 9,
        memoryWorkingSetBytes: 9,
        memoryRssBytes: null,
      },
    ],
    sources: [{ source: 'kubelet', ok: true, message: null }],
    ...overrides,
  };
}

describe('formatting', () => {
  it('scales bytes in binary units and rates in decimal ones', () => {
    expect(formatBytes(40 * 1024 ** 3)).toBe('40 GiB');
    expect(formatBytes(1.5 * 1024 ** 3)).toBe('1.5 GiB');
    expect(formatBytes(512)).toBe('512 B');
    expect(formatRate(1200)).toBe('1.2 kB/s');
    expect(formatRate(0)).toBe('0 B/s');
    expect(formatPercent(0.123)).toBe('12%');
    expect(formatPercent(null)).toBe('n/a');
    expect(formatValue('celsius', 42.4)).toBe('42 °C');
    expect(formatValue('watts', 28.4)).toBe('28 W');
    expect(formatValue('megahertz', 210)).toBe('210 MHz');
    expect(formatValue('percent', 37)).toBe('37%');
  });
});

describe('levels', () => {
  it('compares against warn then error, unknown when there is nothing to compare', () => {
    const threshold = { warn: 0.8, error: 0.9 };
    expect(levelFor(0.5, threshold)).toBe('ok');
    expect(levelFor(0.85, threshold)).toBe('warn');
    expect(levelFor(0.95, threshold)).toBe('error');
    expect(levelFor(null, threshold)).toBe('unknown');
  });

  it('a card takes the worst of its gauges', () => {
    expect(worstLevel([])).toBe('unknown');
    expect(
      worstLevel([
        { id: 'a', label: 'a', fraction: 0.1, text: '', level: 'ok' },
        { id: 'b', label: 'b', fraction: 0.9, text: '', level: 'warn' },
      ])
    ).toBe('warn');
  });
});

describe('node gauges', () => {
  it('give CPU, memory, disk and every temperature with plain text', () => {
    const gauges = nodeGauges(node);
    expect(gauges.map((gauge) => gauge.id)).toEqual([
      'cpu',
      'memory',
      'disk',
      'temp:CPU',
      'temp:NVMe 1',
    ]);
    const cpu = gauges[0];
    expect(cpu?.fraction).toBeCloseTo(0.1);
    expect(cpu?.text).toBe('10%');
    expect(cpu?.detail).toBe('3.20 of 32.0 cores');
    expect(cpu?.level).toBe('ok');
    expect(gauges[1]?.text).toBe('65%');
    expect(gauges[1]?.committed).toBeCloseTo(21 / 62);
    expect(gauges[1]?.detail).toBe('21 GiB resident, 19 GiB cache, of 62 GiB');
    expect(gauges[3]?.text).toBe('42 °C');
    expect(gauges[3]?.fraction).toBeCloseTo(0.42);
  });

  it('show cores when the capacity is unknown, and n/a when nothing is', () => {
    const partial: NodeMetrics = {
      ...node,
      cpuCapacityCores: null,
      memoryWorkingSetBytes: null,
      memoryRssBytes: null,
      memoryCapacityBytes: null,
      temperatures: [],
    };
    const gauges = nodeGauges(partial);
    expect(gauges[0]?.text).toBe('3.20');
    expect(gauges[0]?.level).toBe('unknown');
    expect(gauges[1]?.text).toBe('n/a');
    expect(gauges[1]?.committed).toBeUndefined();
    const noRss = nodeGauges({ ...node, memoryRssBytes: null });
    expect(noRss[1]?.committed).toBeUndefined();
    expect(noRss[1]?.detail).toBe('40 GiB of 62 GiB');
  });

  it('warn and error on hot and full', () => {
    const hot: NodeMetrics = {
      ...node,
      memoryWorkingSetBytes: 61 * 1024 ** 3,
      temperatures: [{ sensor: 'k10temp', label: 'CPU', celsius: 96 }],
    };
    const gauges = nodeGauges(hot);
    expect(gauges[1]?.level).toBe('error');
    expect(gauges[3]?.level).toBe('error');
    expect(worstLevel(gauges)).toBe('error');
  });
});

describe('gpu gauges', () => {
  it('give busy, VRAM, temperature and power', () => {
    const gauges = gpuGauges(gpu);
    expect(gauges.map((gauge) => gauge.id)).toEqual(['util', 'vram', 'temp', 'power']);
    expect(gauges[0]?.text).toBe('37%');
    expect(gauges[0]?.level).toBe('ok');
    expect(gauges[1]?.text).toBe('25%');
    expect(gauges[1]?.detail).toBe('6.0 GiB of 24 GiB');
    expect(gauges[3]?.fraction).toBeNull();
    expect(gauges[3]?.text).toBe('28 W');
  });

  it('leave out what the exporter did not send', () => {
    const bare: GpuMetrics = {
      ...gpu,
      utilizationPercent: null,
      temperatureCelsius: null,
      powerWatts: null,
    };
    const gauges = gpuGauges(bare);
    expect(gauges.map((gauge) => gauge.id)).toEqual(['util', 'vram']);
    expect(gauges[0]?.level).toBe('unknown');
  });
});

describe('power', () => {
  it('adds the GPUs up and measures against a budget when there is one', () => {
    const two = sample({ gpus: [gpu, { ...gpu, uuid: 'GPU-b', powerWatts: 300 }] });
    expect(powerGauge(two, null)?.text).toBe('328 W');
    expect(powerGauge(two, null)?.fraction).toBeNull();
    const budgeted = powerGauge(two, 400);
    expect(budgeted?.fraction).toBeCloseTo(0.82);
    expect(budgeted?.level).toBe('ok');
    expect(powerGauge(two, 340)?.level).toBe('error');
    expect(powerGauge(sample({ gpus: [] }), 400)).toBeNull();
  });
});

describe('alerts', () => {
  it('are empty for a healthy sample and name the scope when something is hot', () => {
    expect(alertsFor(sample(), null)).toEqual([]);
    const hot = sample({
      gpus: [{ ...gpu, temperatureCelsius: 91 }],
      nodes: [{ ...node, fsUsedBytes: 1700 * 1024 ** 3 }],
    });
    const alerts = alertsFor(hot, 30);
    expect(alerts.map((alert) => alert.text)).toEqual([
      'Card A · Temp 91 °C',
      'node-a · Disk 94%',
      'Cluster · GPU power 28 W',
    ]);
    expect(alerts[0]?.level).toBe('error');
  });
});

describe('history', () => {
  it('appends every series a sample feeds and keeps the last N points', () => {
    let history = pushSample(EMPTY_HISTORY, sample());
    expect(history.latest?.at).toBe('2026-01-01T00:00:00.000Z');
    const nodeSeries = seriesFor(history, nodeKey('node-a'));
    expect(nodeSeries.map((series) => series.id)).toEqual([
      'node:node-a:cpu',
      'node:node-a:disk',
      'node:node-a:memory',
      'node:node-a:resident',
      'node:node-a:rx',
      'node:node-a:temp:CPU',
      'node:node-a:temp:NVMe 1',
    ]);
    expect(seriesFor(history, gpuKey('GPU-a')).map((series) => series.unit)).toEqual([
      'megahertz',
      'watts',
      'celsius',
      'percent',
      'fraction',
    ]);
    for (let index = 0; index < HISTORY_LENGTH + 10; index += 1) {
      history = pushSample(
        history,
        sample({ at: `2026-01-01T00:${String(index).padStart(2, '0')}:00.000Z` })
      );
    }
    expect(history.series['node:node-a:cpu']?.points).toHaveLength(HISTORY_LENGTH);
    expect(seriesFor(history, 'nothing')).toEqual([]);
  });
});

describe('usage', () => {
  it('sums the pods a workload owns in its namespace only', () => {
    const pods = sample().pods;
    expect(usageLabel(pods, 'apps', ['web-1', 'web-2'])).toBe(
      '0.07 cores, 1.0 GiB + 512 MiB cache'
    );
    expect(usageLabel(pods, 'apps', ['web-2'])).toBe('0.02 cores, 512 MiB');
    expect(usageLabel(pods, 'apps', ['web-9'])).toBeNull();
    expect(usageLabel(pods, 'other', ['web-1'])).toBe('9.00 cores, 9 B');
  });
});
