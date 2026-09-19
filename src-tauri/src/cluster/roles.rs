//! Which workloads are *yours* and which are the platform's. Kubernetes has no such flag, so
//! Terrace classifies by namespace: Kubernetes' own namespaces are `system`; namespaces that
//! look like an operator's home are `platform`; everything else is an `app`. The user can
//! force a namespace either way from Settings (`workloads.apps`, `workloads.platform`).

use std::collections::BTreeSet;

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkloadRole {
    /// Something the user runs: shown as a card in the Workloads layer.
    App,
    /// Machinery the cluster runs for the user (operators, storage, networking add-ons):
    /// folded into one card per namespace in the Kubernetes layer.
    Platform,
    /// Kubernetes itself (`kube-system` and friends): one card.
    System,
}

const SYSTEM_NAMESPACES: &[&str] = &["kube-system", "kube-public", "kube-node-lease"];

/// Well-known homes of operators and add-ons. A namespace not listed here can still be
/// platform through the suffix or operator heuristics below.
const PLATFORM_NAMESPACES: &[&str] = &[
    "argocd",
    "calico-system",
    "cdi",
    "cert-manager",
    "cilium",
    "cnpg-system",
    "democratic-csi",
    "external-secrets",
    "flux-system",
    "gatekeeper-system",
    "gpu-operator",
    "ingress-nginx",
    "istio-system",
    "knative-serving",
    "kube-flannel",
    "kubernetes-dashboard",
    "kubevirt",
    "kubevirt-hyperconverged",
    "kyverno",
    "linkerd",
    "local-path-storage",
    "longhorn-system",
    "metallb-system",
    "metrics-server",
    "monitoring",
    "nvidia",
    "olm",
    "openebs",
    "operators",
    "rook-ceph",
    "sealed-secrets",
    "snapshot-controller",
    "tailscale",
    "tigera-operator",
    "velero",
];

const PLATFORM_SUFFIXES: &[&str] = &["-system", "-operator", "-operators", "-storage"];

/// A namespace with a workload named like this is running an operator, not an app.
fn looks_like_operator(workload_name: &str) -> bool {
    workload_name.contains("operator")
        || workload_name.ends_with("-controller-manager")
        || workload_name.ends_with("-provisioner")
}

#[derive(Debug, Clone, Default)]
pub struct Classifier {
    /// Namespaces the user forced to `App`.
    pub apps: BTreeSet<String>,
    /// Namespaces the user forced to `Platform`.
    pub platform: BTreeSet<String>,
}

fn split_list(value: &str) -> BTreeSet<String> {
    value
        .split([',', ' ', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

impl Classifier {
    /// From the two comma- or space-separated settings values.
    pub fn from_settings(apps: &str, platform: &str) -> Self {
        Self {
            apps: split_list(apps),
            platform: split_list(platform),
        }
    }

    /// Classify a namespace given the names of the workloads found in it. A user override
    /// wins (apps over platform when both name the namespace), then Kubernetes' own
    /// namespaces, then the platform list, suffixes and operator heuristic.
    pub fn classify(&self, namespace: &str, workload_names: &[&str]) -> WorkloadRole {
        if self.apps.contains(namespace) {
            return WorkloadRole::App;
        }
        if self.platform.contains(namespace) {
            return WorkloadRole::Platform;
        }
        if SYSTEM_NAMESPACES.contains(&namespace) {
            return WorkloadRole::System;
        }
        let listed = PLATFORM_NAMESPACES.contains(&namespace);
        let suffixed = PLATFORM_SUFFIXES
            .iter()
            .any(|suffix| namespace.ends_with(suffix));
        let operated = workload_names.iter().any(|name| looks_like_operator(name));
        if listed || suffixed || operated {
            WorkloadRole::Platform
        } else {
            WorkloadRole::App
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kubernetes_own_namespaces_are_system() {
        let classifier = Classifier::default();
        assert_eq!(
            classifier.classify("kube-system", &["coredns"]),
            WorkloadRole::System
        );
        assert_eq!(
            classifier.classify("kube-public", &[]),
            WorkloadRole::System
        );
    }

    #[test]
    fn operators_are_platform_by_list_suffix_or_name() {
        let classifier = Classifier::default();
        assert_eq!(
            classifier.classify("kubevirt", &["virt-api"]),
            WorkloadRole::Platform,
            "listed"
        );
        assert_eq!(
            classifier.classify("my-thing-system", &["thing"]),
            WorkloadRole::Platform,
            "suffix"
        );
        assert_eq!(
            classifier.classify("storage", &["nfs-provisioner"]),
            WorkloadRole::Platform,
            "provisioner"
        );
        assert_eq!(
            classifier.classify("acme", &["acme-operator", "acme-webhook"]),
            WorkloadRole::Platform,
            "operator"
        );
    }

    #[test]
    fn everything_else_is_an_app_and_overrides_win() {
        let classifier = Classifier::default();
        assert_eq!(classifier.classify("ai", &["ollama"]), WorkloadRole::App);
        assert_eq!(classifier.classify("vm", &["win11"]), WorkloadRole::App);
        assert_eq!(classifier.classify("default", &[]), WorkloadRole::App);

        let forced = Classifier::from_settings("kubevirt, monitoring", "ai");
        assert_eq!(
            forced.classify("kubevirt", &["virt-api"]),
            WorkloadRole::App
        );
        assert_eq!(forced.classify("ai", &["ollama"]), WorkloadRole::Platform);
        assert_eq!(forced.apps.len(), 2);
    }
}
