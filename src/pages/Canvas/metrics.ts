/**
 * Live metrics as the canvas uses them: a short history per series, gauges with plain-word
 * levels, the alerts a sample raises, and the formatting the cards share. Pure: no React, no
 * store, no IPC. Thresholds are deliberately conservative; a busy GPU is not an alert, a hot
 * one is.
 */
import type { GpuMetrics, MetricsSample, NodeMetrics, PodMetrics } from '@/services/api';
import type { Gauge, GaugeLevel } from './nodes/types';

/** Samples kept per series: ten minutes at the default five-second interval. */
export const HISTORY_LENGTH = 120;

export interface Point {
  /** Epoch milliseconds. */
  at: number;
  value: number;
}

export type SeriesUnit =
  'fraction' | 'percent' | 'celsius' | 'watts' | 'megahertz' | 'bytesPerSecond';

export interface Series {
  id: string;
  label: string;
  unit: SeriesUnit;
  points: Point[];
}

export interface MetricsHistory {
  latest: MetricsSample | null;
  /** Series by id: `node:<name>:cpu`, `gpu:<uuid>:temp`, … */
  series: Record<string, Series>;
}

export const EMPTY_HISTORY: MetricsHistory = { latest: null, series: {} };

export function nodeKey(node: string): string {
  return `node:${node}`;
}

export function gpuKey(uuid: string): string {
  return `gpu:${uuid}`;
}

export const POWER_KEY = 'power:gpus';

interface Threshold {
  warn: number;
  error: number;
}

export const THRESHOLDS = {
  cpu: { warn: 0.85, error: 0.97 },
  memory: { warn: 0.9, error: 0.97 },
  disk: { warn: 0.85, error: 0.95 },
  gpuMemory: { warn: 0.9, error: 0.98 },
  gpuTemperature: { warn: 80, error: 90 },
  cpuTemperature: { warn: 85, error: 95 },
  driveTemperature: { warn: 70, error: 80 },
  boardTemperature: { warn: 70, error: 85 },
  power: { warn: 0.85, error: 0.95 },
} as const satisfies Record<string, Threshold>;

export function levelFor(value: number | null, threshold: Threshold): GaugeLevel {
  if (value === null || Number.isNaN(value)) return 'unknown';
  if (value >= threshold.error) return 'error';
  if (value >= threshold.warn) return 'warn';
  return 'ok';
}

function fraction(part: number | null, whole: number | null): number | null {
  if (part === null || whole === null || whole <= 0) return null;
  return Math.min(1, Math.max(0, part / whole));
}

// --- Formatting ------------------------------------------------------------------

const BINARY_UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
const RATE_UNITS = ['B/s', 'kB/s', 'MB/s', 'GB/s', 'TB/s'];

function scaled(value: number, units: string[], base: number): string {
  let scaledValue = value;
  let index = 0;
  while (scaledValue >= base && index < units.length - 1) {
    scaledValue /= base;
    index += 1;
  }
  const digits = scaledValue < 10 && index > 0 ? 1 : 0;
  return `${scaledValue.toFixed(digits)} ${units[index] ?? ''}`.trim();
}

export function formatBytes(bytes: number): string {
  return scaled(Math.max(0, bytes), BINARY_UNITS, 1024);
}

export function formatRate(bytesPerSecond: number): string {
  return scaled(Math.max(0, bytesPerSecond), RATE_UNITS, 1000);
}

export function formatPercent(value: number | null): string {
  return value === null ? 'n/a' : `${String(Math.round(value * 100))}%`;
}

export function formatCores(cores: number): string {
  return cores < 10 ? cores.toFixed(2) : cores.toFixed(1);
}

export function formatCelsius(celsius: number): string {
  return `${String(Math.round(celsius))} °C`;
}

export function formatWatts(watts: number): string {
  return `${String(Math.round(watts))} W`;
}

export function formatValue(unit: SeriesUnit, value: number): string {
  switch (unit) {
    case 'fraction':
      return formatPercent(value);
    case 'percent':
      return `${String(Math.round(value))}%`;
    case 'celsius':
      return formatCelsius(value);
    case 'watts':
      return formatWatts(value);
    case 'megahertz':
      return `${String(Math.round(value))} MHz`;
    case 'bytesPerSecond':
      return formatRate(value);
  }
}

// --- Gauges ----------------------------------------------------------------------

function temperatureThreshold(label: string): Threshold {
  if (label.startsWith('CPU')) return THRESHOLDS.cpuTemperature;
  if (label.startsWith('NVMe') || label.startsWith('Disk')) return THRESHOLDS.driveTemperature;
  if (label.startsWith('GPU')) return THRESHOLDS.gpuTemperature;
  return THRESHOLDS.boardTemperature;
}

function temperatureGauge(id: string, label: string, celsius: number, threshold: Threshold): Gauge {
  return {
    id,
    label,
    fraction: Math.min(1, Math.max(0, celsius / 100)),
    text: formatCelsius(celsius),
    level: levelFor(celsius, threshold),
    detail: `${celsius.toFixed(1)} °C`,
  };
}

/**
 * "21 GiB resident, 17 GiB cache, of 62 GiB": the working set the kubelet counts, split into
 * what is really held and what the kernel could give back.
 */
function memoryDetail(node: NodeMetrics): string | undefined {
  if (node.memoryWorkingSetBytes === null) return undefined;
  const capacity = node.memoryCapacityBytes === null ? '?' : formatBytes(node.memoryCapacityBytes);
  if (node.memoryRssBytes === null) {
    return `${formatBytes(node.memoryWorkingSetBytes)} of ${capacity}`;
  }
  const cache = Math.max(0, node.memoryWorkingSetBytes - node.memoryRssBytes);
  return `${formatBytes(node.memoryRssBytes)} resident, ${formatBytes(cache)} cache, of ${capacity}`;
}

/** CPU, memory, disk and every temperature for a node; unknown numbers stay visible as n/a. */
export function nodeGauges(node: NodeMetrics): Gauge[] {
  const cpu = fraction(node.cpuCores, node.cpuCapacityCores);
  const memory = fraction(node.memoryWorkingSetBytes, node.memoryCapacityBytes);
  const resident = fraction(node.memoryRssBytes, node.memoryCapacityBytes);
  const disk = fraction(node.fsUsedBytes, node.fsCapacityBytes);
  const gauges: Gauge[] = [
    {
      id: 'cpu',
      label: 'CPU',
      fraction: cpu,
      text:
        cpu === null
          ? node.cpuCores === null
            ? 'n/a'
            : formatCores(node.cpuCores)
          : formatPercent(cpu),
      level: levelFor(cpu, THRESHOLDS.cpu),
      detail:
        node.cpuCores === null
          ? undefined
          : `${formatCores(node.cpuCores)} of ${node.cpuCapacityCores === null ? '?' : formatCores(node.cpuCapacityCores)} cores`,
    },
    {
      id: 'memory',
      label: 'Memory',
      fraction: memory,
      ...(resident === null ? {} : { committed: resident }),
      text:
        memory === null
          ? node.memoryWorkingSetBytes === null
            ? 'n/a'
            : formatBytes(node.memoryWorkingSetBytes)
          : formatPercent(memory),
      level: levelFor(memory, THRESHOLDS.memory),
      detail: memoryDetail(node),
    },
    {
      id: 'disk',
      label: 'Disk',
      fraction: disk,
      text: formatPercent(disk),
      level: levelFor(disk, THRESHOLDS.disk),
      detail:
        node.fsUsedBytes === null
          ? undefined
          : `${formatBytes(node.fsUsedBytes)} of ${node.fsCapacityBytes === null ? '?' : formatBytes(node.fsCapacityBytes)} on the root filesystem`,
    },
  ];
  for (const temperature of node.temperatures) {
    gauges.push(
      temperatureGauge(
        `temp:${temperature.label}`,
        temperature.label,
        temperature.celsius,
        temperatureThreshold(temperature.label)
      )
    );
  }
  return gauges;
}

/** Utilisation, memory, temperature and power for one GPU. */
export function gpuGauges(gpu: GpuMetrics): Gauge[] {
  const utilization = gpu.utilizationPercent === null ? null : gpu.utilizationPercent / 100;
  const memory = fraction(gpu.memoryUsedBytes, gpu.memoryTotalBytes);
  const gauges: Gauge[] = [
    {
      id: 'util',
      label: 'Busy',
      fraction: utilization,
      text: formatPercent(utilization),
      level: utilization === null ? 'unknown' : 'ok',
      detail: 'GPU utilisation',
    },
    {
      id: 'vram',
      label: 'VRAM',
      fraction: memory,
      text: formatPercent(memory),
      level: levelFor(memory, THRESHOLDS.gpuMemory),
      detail:
        gpu.memoryUsedBytes === null
          ? undefined
          : `${formatBytes(gpu.memoryUsedBytes)} of ${gpu.memoryTotalBytes === null ? '?' : formatBytes(gpu.memoryTotalBytes)}`,
    },
  ];
  if (gpu.temperatureCelsius !== null) {
    gauges.push(
      temperatureGauge('temp', 'Temp', gpu.temperatureCelsius, THRESHOLDS.gpuTemperature)
    );
  }
  if (gpu.powerWatts !== null) {
    gauges.push({
      id: 'power',
      label: 'Power',
      fraction: null,
      text: formatWatts(gpu.powerWatts),
      level: 'ok',
      detail: `${gpu.powerWatts.toFixed(1)} W drawn`,
    });
  }
  return gauges;
}

/** Every GPU's power draw added up, against the budget when one is set. */
export function powerGauge(sample: MetricsSample, budgetWatts: number | null): Gauge | null {
  const draws = sample.gpus
    .map((gpu) => gpu.powerWatts)
    .filter((watts): watts is number => watts !== null);
  if (draws.length === 0) return null;
  const total = draws.reduce((sum, watts) => sum + watts, 0);
  const share = budgetWatts === null ? null : Math.min(1, total / budgetWatts);
  return {
    id: 'power',
    label: 'GPU power',
    fraction: share,
    text: formatWatts(total),
    level: budgetWatts === null ? 'ok' : levelFor(total / budgetWatts, THRESHOLDS.power),
    detail:
      budgetWatts === null
        ? `${String(draws.length)} GPU${draws.length === 1 ? '' : 's'}; no budget set`
        : `${formatWatts(total)} of a ${formatWatts(budgetWatts)} budget`,
  };
}

/** The worst level among gauges, as a card status. */
export function worstLevel(gauges: Gauge[]): GaugeLevel {
  if (gauges.some((gauge) => gauge.level === 'error')) return 'error';
  if (gauges.some((gauge) => gauge.level === 'warn')) return 'warn';
  return gauges.some((gauge) => gauge.level === 'ok') ? 'ok' : 'unknown';
}

// --- Alerts ----------------------------------------------------------------------

export interface Alert {
  id: string;
  level: 'warn' | 'error';
  text: string;
}

function alertsFromGauges(scope: string, key: string, gauges: Gauge[]): Alert[] {
  const alerts: Alert[] = [];
  for (const gauge of gauges) {
    if (gauge.level === 'warn' || gauge.level === 'error') {
      alerts.push({
        id: `${key}:${gauge.id}`,
        level: gauge.level,
        text: `${scope} · ${gauge.label} ${gauge.text}`,
      });
    }
  }
  return alerts;
}

/** What a sample says is worth attention, errors first. */
export function alertsFor(sample: MetricsSample, budgetWatts: number | null): Alert[] {
  const alerts: Alert[] = [];
  for (const node of sample.nodes) {
    alerts.push(...alertsFromGauges(node.node, nodeKey(node.node), nodeGauges(node)));
  }
  for (const gpu of sample.gpus) {
    alerts.push(...alertsFromGauges(gpu.model, gpuKey(gpu.uuid), gpuGauges(gpu)));
  }
  const power = powerGauge(sample, budgetWatts);
  if (power) alerts.push(...alertsFromGauges('Cluster', POWER_KEY, [power]));
  return alerts.sort((a, b) => (a.level === b.level ? 0 : a.level === 'error' ? -1 : 1));
}

// --- History ---------------------------------------------------------------------

/** Append a sample to every series it feeds, keeping the last `capacity` points. */
export function pushSample(
  history: MetricsHistory,
  sample: MetricsSample,
  capacity = HISTORY_LENGTH
): MetricsHistory {
  const parsed = Date.parse(sample.at);
  const at = Number.isNaN(parsed) ? Date.now() : parsed;
  const series: Record<string, Series> = { ...history.series };
  const push = (id: string, label: string, unit: SeriesUnit, value: number | null) => {
    if (value === null || !Number.isFinite(value)) return;
    const previous = series[id];
    const points = previous
      ? [...previous.points, { at, value }].slice(-capacity)
      : [{ at, value }];
    series[id] = { id, label, unit, points };
  };

  for (const node of sample.nodes) {
    const key = nodeKey(node.node);
    push(`${key}:cpu`, 'CPU', 'fraction', fraction(node.cpuCores, node.cpuCapacityCores));
    push(
      `${key}:memory`,
      'Memory',
      'fraction',
      fraction(node.memoryWorkingSetBytes, node.memoryCapacityBytes)
    );
    push(
      `${key}:resident`,
      'Memory resident',
      'fraction',
      fraction(node.memoryRssBytes, node.memoryCapacityBytes)
    );
    push(`${key}:disk`, 'Disk', 'fraction', fraction(node.fsUsedBytes, node.fsCapacityBytes));
    push(`${key}:rx`, 'Network in', 'bytesPerSecond', node.networkReceiveBytesPerSecond);
    push(`${key}:tx`, 'Network out', 'bytesPerSecond', node.networkTransmitBytesPerSecond);
    for (const temperature of node.temperatures) {
      push(`${key}:temp:${temperature.label}`, temperature.label, 'celsius', temperature.celsius);
    }
  }
  for (const gpu of sample.gpus) {
    const key = gpuKey(gpu.uuid);
    push(`${key}:util`, 'Busy', 'percent', gpu.utilizationPercent);
    push(`${key}:vram`, 'VRAM', 'fraction', fraction(gpu.memoryUsedBytes, gpu.memoryTotalBytes));
    push(`${key}:temp`, 'Temperature', 'celsius', gpu.temperatureCelsius);
    push(`${key}:power`, 'Power', 'watts', gpu.powerWatts);
    push(`${key}:clock`, 'Clock', 'megahertz', gpu.smClockMhz);
  }
  const power = powerGauge(sample, null);
  if (power) {
    const total = sample.gpus.reduce((sum, gpu) => sum + (gpu.powerWatts ?? 0), 0);
    push(POWER_KEY, 'GPU power', 'watts', total);
  }
  return { latest: sample, series };
}

/** The series behind one card, in a stable order. */
export function seriesFor(history: MetricsHistory, key: string): Series[] {
  return Object.values(history.series)
    .filter((entry) => entry.id === key || entry.id.startsWith(`${key}:`))
    .sort((a, b) => a.id.localeCompare(b.id));
}

// --- Workloads -------------------------------------------------------------------

/** Cache below this, or below a tenth of the working set, is not worth a mention. */
const CACHE_WORTH_MENTIONING = 64 * 1024 ** 2;

/** "312 MiB + 16 GiB cache" when a workload's working set is mostly reclaimable cache. */
function memoryWithCache(resident: number, workingSet: number): string {
  const cache = workingSet - resident;
  if (cache < CACHE_WORTH_MENTIONING || cache < workingSet * 0.1) return formatBytes(resident);
  return `${formatBytes(resident)} + ${formatBytes(cache)} cache`;
}

/**
 * "0.07 cores, 16.2 GiB" for the pods a workload owns, or null when none reported. Memory is
 * what the pods hold; the cache the kubelet also counts is named when it is a real share.
 */
export function usageLabel(pods: PodMetrics[], namespace: string, names: string[]): string | null {
  const mine = pods.filter((pod) => pod.namespace === namespace && names.includes(pod.name));
  if (mine.length === 0) return null;
  let cores = 0;
  let workingSet = 0;
  let resident = 0;
  let seen = false;
  let residentSeen = false;
  for (const pod of mine) {
    if (pod.cpuCores !== null) {
      cores += pod.cpuCores;
      seen = true;
    }
    if (pod.memoryWorkingSetBytes !== null) {
      workingSet += pod.memoryWorkingSetBytes;
      seen = true;
      // A pod that reports no resident figure counts whole, so nothing shows up as cache
      // merely because one kubelet left the field out.
      if (pod.memoryRssBytes === null) resident += pod.memoryWorkingSetBytes;
    }
    if (pod.memoryRssBytes !== null) {
      resident += pod.memoryRssBytes;
      residentSeen = true;
    }
  }
  if (!seen) return null;
  const memory = residentSeen ? memoryWithCache(resident, workingSet) : formatBytes(workingSet);
  return `${formatCores(cores)} cores, ${memory}`;
}
