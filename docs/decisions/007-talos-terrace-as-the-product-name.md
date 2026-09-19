# 007 Talos Terrace as the Product Name

**Status:** Accepted · **Date:** 19 September 2026

## Context

The console was named **Terrace** in September 2026, before it had a public repository: stepped
layers you walk up, one per level of the stack. The name still describes the product, but it is
about to be published, and on its own it has two problems. Nothing in it says what the tool is
for, so nobody searching for a Talos Linux console finds it; and "terrace" is a common word,
already taken on npm and crowded on GitHub by gardening, architecture and unrelated projects.

Adding "Talos" fixes discovery but has to be done carefully. "Talos Linux" is Sidero Labs'
trademark, and Sidero already ships a management plane of its own. A name like *Talos Console*
or *Talos Dashboard* would read as an official product. The convention for third-party tooling
is `talos-<thing>`: it says *for Talos*, not *by Sidero*.

## Options considered

1. **Talos Terrace**, repository `talos-terrace`, executable `terrace`. Keeps the layers
   metaphor the product was named for, puts the searchable word where discovery happens, and
   stays honestly third-party. Free on crates.io and npm; nothing of the name on GitHub.
   **Chosen.**
2. **Taloscope** — a coined word that swallows "Talos", free everywhere and a stronger mark.
   Rejected: `-scope` promises observation, and the roadmap ends in composing and applying
   configuration. The name would be wrong the first day the app writes.
3. **Talostrata** — literal layers, no collisions anywhere. Rejected as harder to say and to
   spell aloud than what it buys.
4. **Keep Terrace** — rejected: it is exactly the discovery problem, and the npm name is gone.

## Decision

The product is **Talos Terrace**. Repository `talos-terrace`, npm package `talos-terrace`,
window title and bundle "Talos Terrace", application identifier `com.nowayja.talosterrace`.

The **technical names stay `terrace`**: the Rust crate, the `terrace` executable
(`mainBinaryName`, so the product name's space never reaches an executable filename), the
future `terrace mcp` subcommand, the `terrace-reader` and `terrace-metrics-proxy` roles in
`docs/rbac/`, the `TerraceError` type and the `.terrace-*` CSS namespace. Prose uses the full
name in titles and on first mention, and the short form "Terrace" after that.

## Consequences

- The identifier change moves the data directory and the keychain entry
  (`com.nowayja.talosterrace.vault.master_key.<profile>`). Settled deliberately before the
  first signed build, as [002](002-a-general-purpose-console-under-apache-2.md) required.
  Anyone who ran the pre-release build re-enters their settings once.
- The macOS bundle is `Talos Terrace.app`. Every path to it in `docs/signing.md` is quoted.
- Supersedes nothing in this repository: the original name was never recorded as a decision
  here, only used.
