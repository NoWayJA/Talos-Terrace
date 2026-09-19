# Security policy

Talos Terrace is a desktop console that holds the keys to your Talos and Kubernetes APIs. Treat a
weakness in it as seriously as one in `talosctl` or `kubectl`.

## Reporting a vulnerability

Please do not open a public issue for a security problem. Use GitHub's private vulnerability
reporting on this repository (**Security** tab, then **Report a vulnerability**). You will get
an acknowledgement within a week and a fix or a clear answer as soon as one exists. Credit is
given in the release notes unless you prefer otherwise.

If private reporting is unavailable for any reason, use the contact details on the `NoWayJA`
GitHub profile and ask for a private channel before sending details.

## Supported versions

Terrace is pre-release and has no binary releases yet: build it from source at the tip of
`main`. Once tags exist, only the latest tag is supported; fixes are not backported.

## What is in scope

- The Terrace application: the Rust side (`src-tauri/`), the web side (`src/`) and the IPC
  between them.
- How Terrace invokes `talosctl` (argument handling, output parsing).
- The vault (master key handling, AES-256-GCM blobs) and the settings database.
- The build and release pipeline in `.github/workflows/`.

Out of scope: vulnerabilities in Talos Linux, Kubernetes or `talosctl` themselves (report them
upstream), and dependency advisories that cannot be reached through Terrace. A dependency
advisory that *is* reachable is very much in scope.

## What Terrace does with your credentials

- It reads your kubeconfig and talosconfig **where they already are** and stores only their
  paths. It never copies them into its data directory or this repository; `.gitignore`
  refuses the usual file names.
- Secrets, when the first one arrives, go into the vault: one entry in the OS keychain holds a
  master key and everything else is AES-256-GCM in SQLite. In debug builds the master key is a
  file in the data directory instead (mode `0600` on Unix). A missing key with existing
  encrypted secrets, or an invalid key, causes an error; it is never silently replaced.
  Back up the original key and encrypted database together.
- Setting values are never logged; only keys and names are.
- It makes no network connections except to the APIs you configure: the Kubernetes API from
  your kubeconfig, the Talos API through `talosctl` with your talosconfig, and any optional
  integration whose URL you enter, plus authentication performed by configured credential
  plugins. Opening a documentation link launches the system browser. There is no telemetry,
  no update check and no analytics.
- `talosctl` is invoked with a fixed set of subcommands (`version`, `get`, `get --watch`, and
  `read` of absolute `/sys` paths for the hosts' temperature sensors) and validated arguments.
  The resources it reads are the hardware, network, volume, extension and machine-status
  resources the inventory needs; it never asks for the machine config or the secrets
  namespace. Terrace has no write path against a cluster yet, and when one arrives every
  write will show its diff and ask first.
- The base [reader role](docs/rbac/terrace-reader.yaml) grants inventory and discovery
  access only. Metrics proxies are a separate [optional role](docs/rbac/terrace-metrics-proxy.yaml).
  **`get` on `nodes/proxy` can authorize command execution in containers through the kubelet;
  it is not read-only access.** Pod and Service proxies reach application endpoints too.
  Kubernetes RBAC cannot restrict these grants to the metrics paths Terrace uses. Read the
  [permission guide](docs/rbac/README.md) before binding either role.
- Kubeconfigs are executable configuration: an `exec` credential plugin can run a local
  program with your account's permissions. Use only configuration files and credential
  plugins from sources you trust. Plugins may contact their identity provider.
- Ollama settings accept HTTP(S) server URLs without embedded credentials, query parameters
  or fragments; authentication in URL settings is unsupported. Redirects are refused.
- The web view runs with a Content Security Policy, without `withGlobalTauri`, and with the
  narrowest Tauri capabilities the app needs.

## Hardening tips for users

- Give Terrace a **read-only talosconfig**: `talosctl config new --roles os:reader` produces
  one. Everything Terrace reads works with it except the hosts' temperature sensors, because
  Talos reserves the file read API (`talosctl read`) for `os:admin`; Terrace notices the
  refusal, says so on the Metrics badge and leaves the sensors alone for the session, or you
  can switch them off in Settings.
- Give Terrace a kubeconfig bound to the **base reader role**, instead of `cluster-admin`.
  Bind the separate proxy role only if you accept its broader permissions.
- Keep your kubeconfig and talosconfig files mode `0600`, as their tools already expect.
- Build from source. When signed releases exist, verify the signature; do not run an unsigned
  binary somebody sent you.
