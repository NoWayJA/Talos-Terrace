# 002 A general-purpose console under Apache-2.0

**Status:** Accepted · **Date:** 17 September 2026

## Context

The first cut carried its origin in the code: links to a private wiki, facts about one
machine typed in by hand, an inference server as a required setting. The owner intended to
release Terrace publicly. A public tool cannot know about one cluster.

## Options considered

1. **Keep it the appliance console, private.** Simplest; every fact can be hard-coded.
   Rejected: it would never survive a second cluster, let alone a second user.
2. **A general Talos console, public.** Every fact from the Talos or Kubernetes API, links to
   upstream documentation, integrations optional, multi-node and multi-cluster as the data
   model from the start. **Chosen.**
3. **General but private.** The same code without the release work; a later choice, not a
   different one.

## Decision

Option 2, under **Apache-2.0**: a patent grant matters for infrastructure tooling and it is
the licence of the Kubernetes ecosystem. MIT was equally workable and is what the lifted Modz
code stays under. The rules that follow are in `CLAUDE.md`: no project-specific facts, links or
private references in the repository, upstream documentation only, integrations optional and
detected or configured, the node grid as the data model, and the repository carrying its own
documentation.

## Consequences

- `LICENSE`, `NOTICE`, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, CI on three
  operating systems, no telemetry, a weekly documentation-link check.
- Things about any particular deployment live with that deployment, never here. Test fixtures
  use neutral names and addresses.
- The application identifier names the keychain entry that holds the vault's master key and
  the data directory, so it does not change once anyone has run the app. It was settled as
  `com.nowayja.talosterrace` with the rename in
  [007](007-talos-terrace-as-the-product-name.md). Still ahead: signed releases with SBOMs.
