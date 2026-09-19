# Architecture

One Tauri 2 desktop app: `src/` (React 19, TypeScript strict, Vite, `@xyflow/react` 12,
zustand) and `src-tauri/` (Rust). Cross-platform from day one: no platform-only APIs, no
`unstable` Tauri feature, no child webviews. Everything against a cluster is read-only until a
milestone says otherwise; every future write shows its diff and asks first.

## The layer model

| Layer | Shows | Source |
|---|---|---|
| Trust boundary | Secure Boot, UKI, module signing, SELinux, FIPS, disk encryption per volume, Pod Security per namespace, primary image pinning by digest; later signatures, SBOMs, admission policy, attestation | Talos `securitystate` and `volumestatus`; namespace labels; image references |
| Workloads | Your apps: one card each, whatever owns the pods (Deployment, DaemonSet, StatefulSet, Job, KubeVirt VirtualMachine, static pod) | Pods and their owner chain; Deployments; VirtualMachines and instances |
| GPU | Allocatable accelerators and who holds them, driver and CUDA versions; GPU operators as platform cards | Node capacity and labels; workload limits |
| Kubernetes | Cluster (version, endpoint, namespaces), platform namespaces as one card each with version and health, Kubernetes' own components as one card, recent events | Nodes, Namespaces, Events, the workloads above |
| Talos | Version, stage and readiness, schematic and extensions | `machinestatus`, `extensions`, `talosctl version` |
| Hardware and firmware | System, processors, memory, firmware, disks by serial, links and addresses, PCI GPUs | `systeminformation`, `processors`, `memorymodules`, `disks`, `links`, `addresses`, `pcidevices` |

Facts are pure data (`FactNodeData`: layer, title, subtitle, status, facts, docs) built by
pure functions in `src/pages/Canvas/facts.ts`; the canvas is a view of the store.

## Rust modules

| Module | Role |
|---|---|
| `error.rs` | `TerraceError`, the one error type, serialised as `{ code, message }` |
| `db/` | SQLite through `rusqlite`, migrations recorded in `PRAGMA user_version` |
| `vault/` | A master key in the OS keychain (a `chmod 600` file in debug builds), AES-256-GCM blobs in SQLite |
| `settings.rs` | Known keys, defaults, resolution; paths are settings, secrets are not |
| `cluster/` | kube-rs over rustls: `connect` (reads the kubeconfig in place, discovers `kubevirt.io`), `summary` (lists), the pure `summarise` both paths share; `roles.rs` classifies namespaces; `kubevirt.rs` reads virtual machines through the dynamic API |
| `talos/` | The `talosctl` sidecar: `version`, `get --output json`, `get --output json --watch`, `read <absolute path>`; arguments validated; `inventory.rs` builds the per-node hardware, Talos and trust inventory leniently |
| `watch/` | The live path (below) |
| `ollama/` | An optional integration: off until a URL is set |
| `metrics/` | Live samples: the kubelet summary API and a DCGM exporter through the API server's proxy subresources, the hosts' sensors through `talosctl read`; a Prometheus text parser; quantities |
| `commands/` | Async Tauri commands, snake_case, camelCase payloads |
| `logging.rs` | tracing to a daily file plus an in-memory tail; never a setting value |

`init_crypto()` in `lib.rs` installs rustls's `ring` provider before any TLS use: two providers
are compiled in (kube-rs uses `ring`, reqwest defaults to `aws-lc-rs`) and rustls refuses to
guess. Every entry point calls it.

## Workloads: from Pods to cards

Every pod's controller owner (or first owner when no controller is marked) is walked to the thing a person would name:

| Owner | Root |
|---|---|
| ReplicaSet | the Deployment, by stripping the `pod-template-hash` suffix |
| VirtualMachineInstance | the VirtualMachine of the same name |
| Node | a static control-plane pod, by stripping the node-name suffix |
| anything else (DaemonSet, StatefulSet, Job, a custom kind) | the owner itself |
| none | the pod |

Deployments contribute their own status too. Virtual machines with no pod (stopped) still
appear. Each namespace is then classified: `kube-*` is **system**; a namespace on a known list
of operator homes, ending in `-system`, `-operator` or `-storage`, or holding a workload named
like an operator is **platform**; everything else is an **app**. Two settings force a
namespace either way. The canvas shows apps as cards, folds each platform namespace into one
card with the operator's image tag as its version, and Kubernetes' own components into one.

## The live path

```text
 kubeconfig ──► kube-runtime reflectors (Nodes, Namespaces, Deployments, Pods, Services,
                Events, VirtualMachines, VirtualMachineInstances) ──► debounce ──► summarise ─┐
                                                                                        ├──► Sink
 talosconfig ─► talosctl get <trigger> --watch × 6 ─► debounce ─► inventory() ─────────┘
```

- Kubernetes: one reflector per kind, a watch stream with kube-runtime's default backoff
  feeding a store; any change pokes a notifier and, after a short debounce, the stores are
  summarised and pushed whole. Sorted output keeps the canvas stable between rebuilds. The
  first summary waits up to twenty seconds for every kind's first list, then goes ahead with
  what arrived: a kind these credentials cannot list stays named on the badge (kube-runtime
  keeps retrying it) and the rest of the canvas fills. A 401 or 403 from the API server is
  "not permitted", a code of its own, so the message can say so.
- Talos: `talosctl get --watch` on six trigger resources (`machinestatus`, `volumestatus`,
  `disks`, `links`, `addresses`, `extensions`). The output is pretty-printed JSON with an
  `event` field; a drainer takes complete documents off the stream. The replay burst at start,
  `veth` links and link-local addresses are ignored, so pod churn causes no re-reads. Any other
  change re-reads the whole inventory.
- The `Sink` trait carries watch state, summaries and metrics samples. The app's sink emits
  Tauri events (`terrace://cluster`, `terrace://talos`, `terrace://metrics`, `terrace://watch`)
  that the canvas store subscribes to; the header shows a badge per source. The headless
  probe's sink prints JSON lines. Desktop events wrap the payload with a connection
  generation; the store discards output from earlier configurations and clears sources
  when connection settings change. Empty relists also trigger a summary.
- The front end queues start and stop calls, because React's development double-mount fires
  start, stop, start within milliseconds.

## Live metrics

A third loop beside the two watches. Every interval (a setting, five seconds by default) it
samples three optional sources and pushes one `MetricsSample` with a state per source:

| Source | Path | Gives |
|---|---|---|
| Kubelet summary | `GET /api/v1/nodes/<node>/proxy/stats/summary` | node CPU, memory working set and resident memory, root filesystem, network counters (turned into rates), the same per pod |
| DCGM exporter | `GET /api/v1/namespaces/<ns>/pods/<pod>:<port>/proxy/metrics` for each pod behind the Service named `nvidia-dcgm-exporter` (or the one in Settings), found through its EndpointSlices; the Service proxy when those cannot be listed | per GPU: utilisation, memory used and total, temperature, power, clock; the pod's node names its GPUs |
| Host sensors | `talosctl read /sys/class/hwmon/hwmonN/{name,temp1_input}` | temperatures for the drivers Terrace can name (CPU, NVMe, GPU, board); discovered once, read every third sample. Talos allows `read` to `os:admin` only: with an `os:reader` talosconfig the refusal is reported once and the source stands down for the session |

Nothing is installed on the cluster. A source that is absent or failing is reported in the
sample and the rest still arrives; every source failing three times in a row reconnects with
backoff. The canvas store keeps ten minutes of history per series in memory, turns the latest
sample into gauges on the node, GPU and Power cards and a usage line on app cards, and raises
alerts from conservative thresholds. Cards keep their positions across samples.

Memory is shown the way the kubelet counts it, as the working set, because that is what
eviction and limits act on; but the working set includes page cache the kernel can give back,
and a node that has just read a large file looks fuller than it is. The memory ring therefore
draws resident memory as a solid arc under a fainter working-set arc, the tooltip says
"21 GiB resident, 17 GiB cache, of 62 GiB", the panel charts both, and a workload's usage line
names its cache when it is a real share of the total.

The base reader and optional metrics proxy permissions are separated in the
[permission guide](rbac/README.md). Proxy permissions cannot be limited to metrics paths;
`nodes/proxy` can permit command execution through the kubelet. Use the base role alone
when that broader access is unacceptable.

## The headless probe

`cargo run --example probe -- [--kubeconfig] [--context] [--talosconfig] [--talosctl]
[--watch SECONDS]` performs the same reads and, with `--watch`, the same watches as the app,
and prints what it would push, as JSON lines. It is how field names are verified against real
clusters and the tool to attach to a bug report.

## Front end

One `Page` union, an error boundary, a services layer (`services/api.ts`: every command and
event name, typed; components never import `invoke`), the settings store mirroring
`settings.rs`, `useTauriEvent` with the mounted guard. `useLiveFeed`, mounted once in `App`,
subscribes to the four watch events and keeps the watches running for the life of the app,
whichever page is showing; a saved setting that a watch captured restarts it. The canvas
store rebuilds from the pushed summaries with structural sharing: a card whose data has not
changed keeps its object, so React Flow keeps its measurements and selection. Layer bands
wrap cards into rows of six and grow taller, so the layout holds for a cluster of many nodes. The canvas: `layers.ts`, `docs.ts`
(upstream documentation links only), `palette.ts`, `facts.ts`, `nodes/` (module-level
`nodeTypes`), `hooks/useCanvasStore.ts` (store-backed React Flow handlers; pushed summaries
land here), `LiveBadge.tsx`. Styles: tokens in dark and light, BEM-lite classes, no inline
styles, React Flow overrides namespaced under `.terrace-canvas`.

## Security model

Terrace reads kubeconfig and talosconfig where they already live and stores their paths. It
makes no network connections except to the APIs you configure. There is no telemetry. The web
view runs with a Content Security Policy, without `withGlobalTauri`, and with the narrowest
capabilities. Details and the reporting process are in [SECURITY.md](../SECURITY.md).

## Lifted from Modz

Terrace started from proven pieces of Modz (MIT), the author's own Tauri 2 desktop platform:
the React Flow canvas pattern and its CSS, the design tokens, the ESLint and Prettier setup,
the services layer, the vault, logging, the Tauri command conventions and the signing
runbooks. Each lifted file carries a header saying so; the notice is in [NOTICE](../NOTICE).
Fixed on the way in: a real CSP and no `withGlobalTauri`, one `Page` union, an error boundary, no raw `invoke` outside
the services layer, no native dialogs, no inline styles, `user_version` migrations, async
commands, no `lock().unwrap()`, one error type, pinned dependencies, CI from the first commit.
