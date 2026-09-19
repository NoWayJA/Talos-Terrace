# Roadmap

Talos Terrace is a live, layered console for Talos Linux clusters. This page is the plan: what it is
for, the order things arrive in, what is done, and what is open. Dated changes are in the
[changelog](../CHANGELOG.md); the reasoning behind the shape of things is in
[decisions](decisions/README.md).

## Positioning

Terrace is to `talosctl` what a Kubernetes IDE is to `kubectl`, with the machine as a
first-class object and the trust boundary drawn rather than implied. The landscape it sits in:
Sidero's Omni (source-available, runs its own service and agents, fleet-scale machine
lifecycle); `talosctl` and its TUI dashboard (complete, one command at a time); Kubernetes GUIs
(no Talos layer at all); platform bundles. The gap Terrace fills is a **local-first desktop
console that uses the talosconfig and kubeconfig you already have, installs nothing on the
cluster, and shows Talos and Kubernetes as one stack**.

## The canvas

Rows are six layers, top to bottom: trust boundary, workloads, GPU, Kubernetes, Talos,
hardware. Columns are nodes; cluster-wide objects span the columns. One node today, control
planes and workers tomorrow, several clusters as tabs. The data model already groups every
fact by node and every workload by owner.

## Capabilities, in order of value

| | Capability | What it means | Milestone |
|---|---|---|---|
| 1 | **Inventory and health, live** | Every node: system, processors, memory, disks, links and addresses, extensions and schematic, stage and readiness; later services including `ext-*`, etcd members, control-plane status. Talos resource watches and Kubernetes watches instead of refresh | M0 (inventory and watches done; services, etcd and control-plane status next) |
| 2 | **Machine config as code** | Render the multi-document config per node, patches as files, `apply-config --dry-run` as a diff view, apply modes (auto, no-reboot, reboot, staged), drift against Git, secrets kept out (SOPS/age aware) | M1 |
| 3 | **Guarded lifecycle operations** | Upgrade through Image Factory: browse extensions, build a schematic, compare extensions across nodes, preview the installer image, upgrade one node at a time with etcd-health gates and the single-node `--drain=false` rule; `upgrade-k8s`; reboot, shutdown; etcd snapshot and defrag; certificate rotation. Reset only with a typed confirmation; `bootstrap` never | M5 |
| 4 | **Bootstrap wizard** | Secrets, config, patches, schematic, installer image, first boot, discovery of nodes in maintenance mode on the LAN, bootstrap | after M5 |
| 5 | **Trust layer** | Secure Boot, UKI, module signing, TPM, disk encryption per volume, SELinux, KubePrism, API certificate SANs and expiry, RBAC summary, Pod Security per namespace, image digests, extension provenance from the schematic; later cosign verification and attestation evidence | M4 (boot, encryption, Pod Security, pinning done) |
| 6 | **Kubernetes for the Talos operator** | Every workload through its Pods (Deployments, DaemonSets, Jobs, virtual machines, static pods), apps versus platform, Services as edges, events, logs, port-forward, Helm releases: what an operator needs, not a Lens clone | M0 to M2 (workloads, roles, VMs and events done) |
| 7 | **Accelerators** | Allocatable devices, who holds them, driver and operator status, utilisation, memory, temperature and power from an exporter | M0 |
| 8 | **Automation** | An MCP server (`terrace mcp`) with reads free and writes gated; a headless mode; a GitOps writer for manifests and patches | M3 |
| 9 | **Many nodes, many clusters** | The grid at full width; contexts from both config files; a cluster switcher | M1 onward |

## Milestones

| | Scope | Done when |
|---|---|---|
| **M0 Clean core and read-only live canvas** | Tauri 2 core; settings, vault, logging; kube-rs watches; the `talosctl` sidecar with its own watch; the six layers rendered live for every node in the talosconfig; every workload found through its Pods and shown as yours or the platform's; virtual machines when KubeVirt is served; no writes | Everything a node knows about itself is on screen and moves when it changes: a deleted Pod's replacement appears without a refresh |
| **M1 Compose and apply** | Drag a workload from the palette: manifests written as files into a directory you choose, a server-side diff, apply on confirm, live status | A new workload reaches Running from the console with its YAML committed by you |
| **M2 Operations** | Port-forward manager, logs, restart and scale; optional integration panels (an Ollama server's models and what is loaded); GPU occupancy | A day's operating without `kubectl` |
| **M3 MCP server** | The console's reads and gated writes as MCP tools over stdio, for any MCP client | An agent lists the cluster's workloads and proposes an apply that a person approves |
| **M4 Trust layer v1** | Digest pinning, Pod Security, Secure Boot, TPM and encryption state, config documents rendered, cosign status | Every node shows its trust facts; the boundary reflects a rebuild |
| **M5 Talos operations, guarded** | Dry-run and apply config, upgrade to a new schematic, reboot; echo and confirm; `bootstrap` impossible | An upgrade runs from the console with the diff shown first |
| **M6 Attestation** | Measured state and evidence on the boundary; key release conditioned on it | Decided when the hardware is chosen |

## Done so far

- **Scaffold** (17 September 2026): Tauri 2.11, React 19, React Flow 12, kube-rs 4; the
  pieces lifted from Modz (canvas pattern, tokens, vault, services layer, logging, signing
  runbooks) with their fixes; one error type; async commands; pinned dependencies; CI on
  Linux, macOS and Windows.
- **General console** (17 September): upstream documentation links only, the live Talos
  inventory from twelve resources per node, optional integrations, a public README.
- **Live path** (18 September): kube-runtime reflectors for Nodes, Namespaces, Deployments,
  Pods, Events and, when served, KubeVirt VirtualMachines; `talosctl get --watch` triggers that
  re-read the inventory; one `Sink` for the app and the headless probe; a live badge per
  source. Field names verified against a real Talos v1.14 node with the probe.
- **Apps versus platform** (18 September): workloads from Pods and their owners; namespace
  roles with defaults and a Settings override; app cards, platform cards, a Kubernetes system
  card; virtual machines with size, address, disks and Secure Boot; plain-word statuses.
- **Public posture** (18 September): Apache-2.0, `NOTICE`, `SECURITY.md`, `CONTRIBUTING.md`,
  a code of conduct, a weekly documentation-link check.
- **Services as edges** (18 September): Services read and watched; each one that selects an
  app is an edge from the Cluster card to the app's card, labelled with its ports and styled
  by reach (inside the cluster, node port, load balancer); selectable for details; app cards
  say where they are reachable. Field names verified against a real cluster with the probe.
- **Live metrics** (19 September): a metrics loop beside the watches samples the kubelet
  summary API, a DCGM exporter when one is found and the hosts' sensors through the Talos
  file read; gauges on node and GPU cards, pod usage on app cards, a Power card against an
  optional budget, alerts in the header, ten minutes of sparklines in the panel. Nothing
  installed on the cluster ([decision 005](decisions/005-live-metrics-from-what-the-cluster-already-serves.md)).
  The GPU layer now shows every extended resource with its holder, including devices passed
  whole to virtual machines.
- **The pre-release pass** (19 September, later): three independent reviews of the code and
  the repository, sixty findings, worked through: an unused command that could have read the
  machine config removed; a read-only ClusterRole and a truthful security policy (the sensors'
  file read needs `os:admin`, and Terrace stands down when refused); the cluster watch that
  no longer blocks on a forbidden kind ([decision 006](decisions/006-degrade-never-block.md));
  a DCGM exporter read per pod; vendor-neutral accelerators; per-node Talos versions; layer
  bands that wrap into rows; the watches owned by the app, not the page; structural sharing
  in the canvas store; a vault that opens lazily and binds names into ciphertexts; `PATH`
  for Dock-launched apps; validation and honest failure in Settings; a details strip and a
  first-run notice; contrast and keyboard access; neutral fixtures; Dependabot, audits and
  issue templates. `npm run check` 70 tests, `rust:check` 82, probe-verified.

## Next

1. The M0 done-when proved in the app: a deleted Pod's replacement appears without a refresh.
2. A `tonic` client for the Talos API from the protos in `siderolabs/talos`, replacing the
   `talosctl` sidecar and its one process per watched resource; the sidecar stays as a
   fallback.
3. Talos services (`ext-*` included), etcd members and control-plane status on the Talos
   layer.
4. M1: manifests as files, diff, apply.
5. Virtual machine actions (start, stop, reach it) under the same gating as Talos operations.
6. Metrics, second pass: sparklines per workload, guest-side GPU numbers for a virtual machine
   holding a device, the `tonic` client's stat streams (load, per-interface rates, disk I/O)
   replacing polling, history kept across restarts.

## Open decisions

Each becomes a record in [decisions](decisions/README.md) when taken.

1. **Manifest strategy**: plain YAML files in a directory the user chooses versus Helm or
   Kustomize; whether the console may commit.
2. **Apply model**: the console applies through the API after showing the diff, or only
   writes files.
3. **M1 node library**: which workloads the palette offers first.
4. **Workload exposure**: what "open" does on a workload card (port-forward, a node port, an
   ingress, a tailnet route).
5. **Trust-layer inputs**: cosign verification, admission policy, or Secure Boot and TPM state
   first.
6. ~~**Observability**: whether metrics (metrics-server, DCGM) arrive before M2.~~ Taken:
   [decision 005](decisions/005-live-metrics-from-what-the-cluster-already-serves.md), no
   metrics stack; dials read what the cluster already serves.
7. **MCP gating**: whether writes need the console's confirmation as well as the client's.
