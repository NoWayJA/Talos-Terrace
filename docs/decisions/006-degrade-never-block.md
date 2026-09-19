# 006 Degrade, never block: a kind that cannot be read is named, the rest is shown

**Status:** Accepted · **Date:** 19 September 2026

## Context

The cluster watch reflects eight kinds. Its first summary waited for every kind's first list
before showing anything, and a kind that never listed (a kubeconfig bound to a role that
cannot list nodes, or events forbidden) left the canvas on "connecting" for ever, although
pods, services and everything else were readable. The one-shot summary failed the same way on
its first `?`. A public tool will meet kubeconfigs that are not `cluster-admin`; a read-only
console must not demand more than it needs, and must not go blank when it gets less.

## Options considered

1. **Fail fast.** Return the first error; the canvas shows one "not connected" state with the
   message. Honest, but it throws away everything that did answer.
2. **Wait for ever**, as before: correct once every permission is granted, silent and blank
   until then.
3. **A bounded wait, then whatever arrived.** Wait a fixed time for the first lists, then
   summarise the stores that are ready, keep the others retrying, and name the missing kinds on
   the source's badge. Distinguish "not permitted" from "unreachable" so the message can say
   which.

## Decision

Option 3, with twenty seconds as the bound. Every kind's stream keeps retrying through
kube-runtime; the badge goes back to live only when no kind is failing. A 401 or 403 from the
API server becomes a `FORBIDDEN` error code rather than a network failure. The exact
permissions Terrace needs are published as a ClusterRole in `docs/rbac/`, so the honest fix is
one `kubectl apply` away.

## Consequences

- A partial canvas with a named gap, never an empty one.
- The same rule applies to the metrics sources already (each reports its own state) and to
  the sensors, which stand down for the session when Talos refuses the file read.
- The one-shot summary behind the Refresh button still needs every list; it is the watch that
  drives the canvas, and a failed refresh keeps the last good read.
