# 003 talosctl as the sidecar first

**Status:** Accepted for M0; a `tonic` client is next · **Date:** 17 September 2026

## Context

Terrace needs the Talos API for the hardware, Talos and trust layers: resources, watches and,
later, machine operations. The Talos API is gRPC with mutual TLS from the talosconfig.

## Options considered

1. **`talosctl` as a sidecar.** Only `version` and `get --output json` (later with
   `--watch`), arguments validated, a timeout, JSON parsed leniently. Nothing to generate;
   works the first day; whatever `talosctl` can reach, Terrace can read. Costs one process per
   call and one long-lived process per watched resource. **Chosen for M0.**
2. **A `tonic` client from the protos in `siderolabs/talos`.** Native, one connection, real
   watches, the machine operations for M5. Costs a build step and the COSI resource decoding
   that `talosctl` does for free.

## Decision

Option 1 for M0, option 2 next. The sidecar stays as a fallback once the client exists.

## Consequences

- The live path runs six `talosctl … --watch` processes per cluster; acceptable for one
  cluster, not for many.
- Field names come from the resources as `talosctl` prints them and are verified against real
  nodes with the headless probe before Terrace relies on them.
