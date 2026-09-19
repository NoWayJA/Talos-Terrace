# Decisions

One record per choice between real alternatives, so that in a year we know *why* Talos Terrace is
the way it is. Reversing a decision means a new record that supersedes the old one.

| # | Decision | Status | Date |
|---|---|---|---|
| [001](001-a-standalone-app-built-from-modz-pieces.md) | A standalone Tauri app built from Modz's proven pieces, joined to agents over MCP later, rather than a page inside Modz or a plugin for an existing dashboard | Accepted | 17 Sep 2026 |
| [002](002-a-general-purpose-console-under-apache-2.md) | A general-purpose Talos console, public, under Apache-2.0: nothing in the code about one cluster | Accepted | 17 Sep 2026 |
| [003](003-talosctl-as-the-sidecar-first.md) | `talosctl` as the Talos API sidecar for M0, a `tonic` client after | Accepted for M0 | 17 Sep 2026 |
| [004](004-workloads-from-pods-apps-versus-platform.md) | Workloads found through Pods and their owners; the user's apps as cards, platform namespaces folded, Kubernetes itself as one card; virtual machines first-class | Accepted | 18 Sep 2026 |
| [005](005-live-metrics-from-what-the-cluster-already-serves.md) | Live metrics from what the cluster already serves (the kubelet summary API, a DCGM exporter, Talos sensor reads) rather than a metrics stack | Accepted | 19 Sep 2026 |
| [006](006-degrade-never-block.md) | A kind these credentials cannot read is named on the badge and the rest of the canvas is shown, after a bounded wait; "not permitted" is its own error | Accepted | 19 Sep 2026 |
| [007](007-talos-terrace-as-the-product-name.md) | The product is named Talos Terrace, repository `talos-terrace`; the crate, executable, roles and CSS namespace stay `terrace` | Accepted | 19 Sep 2026 |

## Template

```markdown
# NNN Short Title

**Status:** Proposed | Accepted | Superseded by NNN · **Date:** D Month YYYY

## Context
## Options considered
## Decision
## Consequences
```
