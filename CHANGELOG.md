# Changelog

All notable changes to Talos Terrace are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/). The first tag will be `v0.0.1`.

## [Unreleased]

### Added

- A base reader ClusterRole and a separate optional metrics-proxy role, with a binding guide
  explaining that Kubernetes proxy permissions can allow much more than metrics reads.
- Each node's own Talos version on its card; mid-upgrade they differ.
- A details strip under the canvas header: every alert and every failing source's reason as
  text, opened from the badges, so they reach the keyboard and screen readers as well as the
  mouse.
- A first-run notice, with a button to Settings, while nothing is connected.
- Enter or Space on a library entry plans it, without a mouse.
- Settings check a number, `on`/`off` and the exporter's `namespace/name[:port]` form before
  saving; a write that fails keeps what was typed, with the reason under the field.
- "N more in the panel" on a card that holds more facts than it shows.
- Dependabot for npm, cargo and GitHub Actions; a weekly workflow that runs `npm audit` and
  `cargo audit`; a bug-report template that asks for the probe's output; a pull-request
  template.
- Live metrics with nothing installed on the cluster: a third live source samples the
  kubelet's summary API through the API server's node proxy (node and pod CPU, memory,
  filesystem, network), an NVIDIA DCGM exporter through the service proxy when one is
  running (GPU utilisation, memory, temperature, power, clocks) and the hosts' temperature
  sensors through the Talos file read API. Each node's card carries CPU, memory, disk and
  temperature gauges; each reported GPU has a card with busy, VRAM, temperature and power
  dials; app and virtual machine cards say what their pods use; a Power card sums the GPUs
  against an optional budget from Settings; the header counts alerts (hot, full, over budget)
  and shows a Metrics badge; the properties panel charts the last ten minutes of every series
  behind a card. Settings: sample interval, the exporter Service, sensors on or off, the
  power budget. The headless probe takes one sample and prints it.
- The GPU layer shows every extended resource a node advertises (`nvidia.com/gpu`, a KubeVirt
  host device's resource name) with who holds it; a device given whole to a virtual machine
  appears as its resource with "passed through"; when a node reports one GPU and schedules
  one, they are one card. Virtual machine cards list the GPUs they were given.
- Services as edges: every Service is read and watched; each one that selects an app is drawn
  as an edge from the Cluster card up to the app's card, labelled with the Service and its
  ports, dashed inside the cluster and solid in the trust colour when a node port or load
  balancer opens it outward. Clicking an edge shows the Service: type, reachability in plain
  words, cluster address, ports, the workloads it reaches. App cards say where they are
  reachable; the Cluster card counts Services and how many reach outside.
- Live watches: kube-runtime reflectors for Nodes, Namespaces, Deployments, Pods, Events and,
  when served, KubeVirt VirtualMachines and instances; `talosctl get --watch` triggers that
  re-read the Talos inventory; a badge per source in the canvas header.
- Every workload through its Pods and their owners (Deployments, DaemonSets, StatefulSets,
  Jobs, virtual machines, static pods, custom kinds), classified as app, platform or system by
  namespace with a Settings override; app cards, one card per platform namespace with version
  and health, a Kubernetes system card; virtual machine cards with status, size, address,
  disks, Secure Boot and guest OS.
- Pods per workload, the newest forty events as an Events card, a Cluster card.
- The headless probe, `cargo run --example probe`, printing what the app would read and push.
- `npm run links:check` and a weekly workflow verifying every documentation link.
- The public posture: Apache-2.0 `LICENSE`, `NOTICE`, `SECURITY.md`, `CONTRIBUTING.md`,
  `CODE_OF_CONDUCT.md`, `.gitattributes`, Windows in CI.
- Documentation in the repository: roadmap, architecture, decisions, this changelog.
- The scaffold (17 September 2026): Tauri 2.11, React 19, TypeScript strict, Vite 8, `@xyflow/react` 12, zustand 5; kube-rs 4.2
  over rustls; SQLite with `user_version` migrations; the vault; settings; logging; the six
  layer bands; the Settings page with connection tests; CI pinned by commit; the pieces lifted
  from Modz with attribution.
- The general-console pass (17 September 2026): upstream documentation links, the live Talos inventory from
  twelve resources per node, Ollama as an optional integration, a public README.

### Changed

- **Renamed to Talos Terrace** ahead of the first public release: the repository is
  `talos-terrace`, the npm package `talos-terrace`, the window title and macOS bundle
  "Talos Terrace", and the application identifier `com.nowayja.talosterrace`. The Rust crate,
  the `terrace` executable, the roles in `docs/rbac/` and the `.terrace-*` CSS namespace keep
  their names. The new identifier means a fresh data directory and keychain entry, so settings
  from a pre-release build are entered once more. See
  [decision 007](docs/decisions/007-talos-terrace-as-the-product-name.md).

- Restored React Flow attribution, removing its development warning.

- Rust now requires 1.89, matching the locked dependencies. Updated `time` and `plist` (and
  its `quick-xml` dependency) to versions fixing the audit's vulnerability findings.
- Documentation links may open only HTTPS URLs. The macOS distribution guide uses a
  Keychain notary profile and verifies signing and stapling without weakening Gatekeeper.
- Image pinning explicitly describes primary container images; sidecars and init containers
  are not yet assessed. GPU requests no longer imply exactly one device, and Service labels
  describe configured exposure rather than claiming network reachability.

- The GPU layer is vendor-neutral: any `vendor/gpu` resource is a whole GPU (`amd.com/gpu`
  as much as `nvidia.com/gpu`), Intel, Habana and named devices count as accelerators, the
  NVIDIA driver facts and links appear only for NVIDIA resources, and node feature discovery's
  PCI labels are carried whatever the vendor.
- A DCGM exporter is read through each of its pods, found through the Service's
  EndpointSlices, so on a multi-node cluster every node's GPUs arrive every sample and the
  pod's node names them; the Service proxy stays as the fallback when EndpointSlices cannot
  be listed.
- The cluster watch no longer waits forever for a kind these credentials cannot list: after
  twenty seconds the canvas fills from what did arrive and the Kubernetes badge names the kind
  still missing. A 401 or 403 from the API server is reported as "not permitted", its own
  error code, rather than as a network failure.
- Layer bands wrap cards into rows after six, growing taller rather than wider without end,
  so a cluster of thirty nodes still fits the screen.
- The watches live as long as the app rather than the canvas page: opening Settings no longer
  aborts and restarts every reflector, and saving a setting a watch captured restarts it
  once.
- A refresh that fails while a source is live keeps the last good read on screen and says
  which source did not answer.
- A card whose data did not change keeps its React Flow object across samples, so nothing is
  re-measured and a selected card stays selected; a dragged card stays put.
- Memory and CPU dials measure against the node's capacity, not its allocatable share, so a
  node with reserved memory cannot read past 100%.
- The vault opens on first use, so a desktop without a keychain service still starts
  Terrace; in debug builds the master key file is created owner-only from its first byte and
  never through a symlink; each secret's name is bound into its ciphertext, so a blob moved to
  another name in the database does not open.
- At start the app appends the usual tool directories (`/usr/local/bin`, Homebrew, `~/.local/bin`,
  `~/.cargo/bin`) to a bare `PATH`, as a Dock-launched app gets one, and path settings accept
  `~`.
- `talosctl read` accepts IPv6 node addresses; a watch's stderr is drained as it comes rather
  than after it ends; the replay burst at a watch's start ends at the first quiet gap instead
  of a fixed two seconds.
- The metrics loop repeats the sensors' last real outcome between reads (the source no
  longer flaps between error and live), never samples faster than once a second, and a bad
  metrics value in the settings database falls back to the default with a warning instead of
  keeping the Kubernetes and Talos watches from starting.
- The unused `talos_get` command is gone: it could have handed the webview any Talos resource,
  the machine config and the secrets namespace included.
- Text tokens meet 4.5:1 contrast in both themes and the light theme has deeper layer
  colours, so layer names no longer vanish on a light canvas.
- Messages shown to people no longer start with the internal command name; log lines still
  do.
- An Ollama URL's user and password, if a reverse proxy needs them, never appear in a message
  or on the canvas.
- Test fixtures describe invented hardware.
- The three layers' descriptions and the palette speak plain words; the properties panel
  shows a status as "Healthy", "Needs attention", "Problem", "Unknown" or "Planned" and the
  theme options are capitalised.
- The memory ring separates resident memory from the page cache the kubelet also counts: a
  solid arc for what is really held under a fainter arc for the working set, the split in the
  tooltip, both series in the panel, and a workload's usage line saying "312 MiB + 16 GiB
  cache" when most of its working set is a file it read.
- Layer bands are taller, so a card can carry a row of gauges and three facts. Live cards keep
  their positions when a rebuild changes no card, so a metrics sample does not undo a drag.
- `npm run dev` launches the app (`tauri dev`); the Vite server alone is `npm run web:dev`.
  In a browser the UI has no Rust side, so nothing connects.
- Talos documentation links point at docs.siderolabs.com, where the documentation moved.
- The Talos inventory reads memory from `sizeMiB`, firmware from `biosVersion`, lists only
  partition and disk volumes, treats an unknown link speed as none, and reads module signing
  enforcement.
- Statuses are in plain words; the network fact warns only when no link is up; the hardware
  GPU fact no longer warns when Talos does not report the bound driver.

### Fixed

- Changing a connection clears its old data, metrics and selection; generation-tagged watch
  events and in-flight refresh checks keep old responses out of a newly selected target.
  Listeners attach before watches start and stop delivering after unmount.
- Empty Kubernetes relists now remove vanished cards and recover the watch badge.
- GPU holders are scoped to active pods on the device's node. Pod readiness honours the
  Ready condition, owner selection prefers the controller, and VM CPU counts include sockets
  and threads. Event grouping includes namespace.
- Unknown Talos security booleans remain unknown instead of displaying as disabled.
- Live cards and Service edges cannot be deleted from the view; planned cards remain
  removable. Lower layer bands move when an upper band gains rows; planned IDs stay unique.
- Ollama URLs with credentials, query parameters or fragments are rejected before storage;
  redirects are disabled and transport errors omit the URL. Metrics settings are validated
  by the backend as well as the form.
- A corrupt or missing vault key with existing ciphertext fails without generating a
  replacement key. New key files cannot overwrite an existing file.
- Windows home paths use the platform home directory. Denied Talos sensor reads stand down
  for the session. Malformed GPU metrics cannot overflow the memory total.

- Service edge labels were white on white in the dark theme: two colour tokens that did not
  exist.
- Live cards could be deleted with the keyboard and wired to each other by hand in a
  read-only console; they reappeared on the next sample. Only planned cards can be removed,
  and nothing can be connected until composing arrives.
- Two facts with the same label on one card (a link and its address) shared a React key.
- The security policy said an `os:reader` talosconfig was enough; the file read behind the
  temperature sensors needs `os:admin`. Terrace now notices the refusal, says so on the
  Metrics badge and leaves the sensors alone for the session instead of failing every cycle.
- The signing and notarization runbooks described another app's configuration.
- A crash on the first cluster call: two rustls crypto providers were compiled in; `ring` is
  now installed explicitly at process start.
- Extension resources with integer ids were rejected, leaving extensions and the schematic
  unread.
- The watch start and stop calls are queued, so React's development double-mount can no
  longer leave the watches stopped.
- "Metrics connecting" for ever when metrics were switched off; the badge now says off.
- Two watch starts arriving together could leave one set of tasks running unowned.
- The "N of 3 sources unavailable" warning counted sources that had simply not been asked
  yet, so it flashed before the first refresh.
