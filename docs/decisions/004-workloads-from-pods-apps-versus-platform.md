# 004 Workloads from Pods, apps versus platform

**Status:** Accepted · **Date:** 18 September 2026

## Context

The first run against a real cluster showed the user's inference server but not their
virtual machine, among sixteen operator components drawn as identical cards. Two causes.
Terrace listed Deployments only, and a KubeVirt virtual machine's pod is owned by a
VirtualMachineInstance, not a Deployment; DaemonSets, Jobs and static pods were invisible for
the same reason. And Kubernetes has no notion of "yours" versus "the machinery's".

## Options considered

1. **Apps versus platform, everything from Pods.** Every pod traced to its owner root, so
   nothing running can hide; the Workloads layer shows only the user's apps; operators and
   add-ons fold into one card per namespace; Kubernetes' own components into one more; a
   per-namespace override in Settings. **Chosen.**
2. **Everything, grouped by namespace.** Honest but still a wall of operator cards.
3. **Just add virtual machines.** Smallest change; DaemonSets and Jobs stay invisible.

## Decision

Option 1. Terrace finds workloads through Pods and their owners, classifies each namespace as
app, platform or system, and renders apps as cards, each platform namespace as one card with
its version (the operator's image tag) and health, and Kubernetes itself as one card. Virtual
machines are first-class when `kubevirt.io` is served: status, size, address, disks, guest OS,
Secure Boot. Every status is in plain words; the Kubernetes vocabulary stays in the properties
panel. This is rule 9 in `CLAUDE.md`: intuitive first, nothing running invisible.

## Consequences

- The classification is a heuristic (a known list, suffixes, an operator-name check). A
  namespace that fools it is one Settings edit away; the list grows with bug reports. Helm's
  `app.kubernetes.io/managed-by` label and OLM's operator groups are candidates for a better
  default.
- Two more reflectors (VirtualMachines, instances) when KubeVirt is served.
- Not yet: virtual machine start and stop, and "reach it" actions, both gated like Talos
  operations.
