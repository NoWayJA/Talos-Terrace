# 001 A standalone app built from Modz pieces

**Status:** Accepted · **Date:** 17 September 2026

## Context

Terrace began as the console for a single-node Talos appliance whose owner already had a
Tauri 2 desktop platform, Modz, with a React Flow canvas, a design system, an encrypted
vault, a services layer and an agent engine with an MCP client. The question was where the
console should live.

## Options considered

1. **A page inside Modz.** Reuses everything at once. Rejected: the console would inherit a
   platform's size and its findings (no CI, no CSP, two error conventions), and could never be
   released on its own.
2. **A plugin for an existing Kubernetes dashboard** (Headlamp). Rejected: no Talos layer, no
   machine as a first-class object, and the trust boundary would be a badge.
3. **A standalone Tauri app that lifts Modz's proven pieces**, fixes them on the way in, and
   connects to Modz's agents over MCP later. **Chosen.**

## Decision

Option 3. Terrace is its own repository with a clean Tauri 2 core. Lifted, with attribution:
the canvas pattern, the tokens and CSS system, the services layer and command conventions, the
vault, settings and logging, the signing runbooks. Fixed on the way in: a real CSP, one `Page`
union, an error boundary, no raw `invoke` outside the services layer, no inline styles,
`user_version` migrations, async commands, one error type, pinned dependencies, CI from the
first commit.

## Consequences

- Terrace owes Modz nothing at run time; the pieces are copied, not depended on
  ([NOTICE](../../NOTICE)).
- The agent layer arrives as an MCP server in Terrace (milestone M3) that any MCP client can
  drive, Modz among them.
