# 005 Live metrics from what the cluster already serves

**Status:** Accepted · **Date:** 19 September 2026

## Context

Terrace shows a cluster as a live, layered picture, and the first thing a person wants from a
picture of a machine is how hard it is working: CPU, memory, disk, temperatures, and for an
AI node the GPUs' utilisation, memory, temperature and power. The usual answer is a metrics
stack (a metrics server, Prometheus, exporters, dashboards), which Terrace's positioning
rules out: it installs nothing on the cluster and uses the two config files you already have.

## Options considered

1. **Require metrics-server and read `metrics.k8s.io`.** Standard, but it must be installed,
   it covers only CPU and memory, and it says nothing about GPUs or temperatures.
2. **Require Prometheus and query it.** Everything is possible, at the cost of a stack most
   single-node and lab clusters do not run, and a query language in the console.
3. **Read what is already served, through the API server.** The kubelet's summary API
   (`/api/v1/nodes/<node>/proxy/stats/summary`) gives node and pod CPU, memory, filesystem
   and network with nothing installed. A DCGM exporter, which the NVIDIA GPU Operator runs on
   every GPU node, serves GPU utilisation, memory, temperature, power and clocks on a Service
   the API server can proxy. Talos's file read API exposes the hosts' hwmon sensors. Each
   source is optional and detected; a cluster without a GPU exporter simply has no GPU dials.
   **Chosen.**

## Decision

Option 3. A metrics loop beside the two watches samples every few seconds (a setting), pushes
one sample with a state per source, and the canvas keeps ten minutes of history in memory.
Gauges on cards carry plain-word levels with conservative thresholds; a busy GPU is not an
alert, a hot one is. The GPU exporter is found by its well-known Service name or set
explicitly; sensors can be switched off; a power budget is a setting the GPU draw is shown
against, never a hard-coded number.

## Consequences

- No metrics stack is needed for dials, so the "observability before M2" question is closed:
  a Prometheus integration, if it ever comes, is an addition for long history, not a
  requirement.
- A device passed whole to a virtual machine is invisible to the host's exporter; its card
  shows the resource and its holder, and the guest's own numbers are a later feature.
- Sensor reads are one `talosctl read` per file, so they run at a slower cadence and only for
  drivers whose meaning is certain (CPU, NVMe, GPU, board); no fan speeds until a Talos API
  serves them.
- The kubelet summary API is old and stable but not versioned; the probe verifies its field
  names against real nodes like every other source.
- History lives in memory and is lost when the app closes; persisting it is a later choice.
