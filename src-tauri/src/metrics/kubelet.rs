//! The kubelet's summary API (`/stats/summary`), reached through the API server's node proxy,
//! so node and pod CPU, memory, filesystem and network usage need no metrics server. Only the
//! fields Terrace shows are modelled; everything else is ignored.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub node: NodeStats,
    #[serde(default)]
    pub pods: Vec<PodStats>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeStats {
    pub node_name: String,
    pub cpu: Option<CpuStats>,
    pub memory: Option<MemoryStats>,
    pub network: Option<NetworkStats>,
    pub fs: Option<FsStats>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuStats {
    pub usage_nano_cores: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    /// What the kubelet counts against limits and eviction: resident memory plus the page
    /// cache that is still active. A large model file just read sits here for a while.
    pub working_set_bytes: Option<u64>,
    /// Resident (anonymous) memory alone: the part the kernel cannot reclaim.
    pub rss_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

/// The node's network: the kubelet fills the top-level counters only when it knows the
/// default interface; otherwise `interfaces` carries every link and Terrace adds up the ones
/// that face the outside (see [`NetworkStats::totals`]).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkStats {
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
    #[serde(default)]
    pub interfaces: Vec<InterfaceStats>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceStats {
    pub name: String,
    pub rx_bytes: Option<u64>,
    pub tx_bytes: Option<u64>,
}

/// Link names that carry pod-to-pod or loopback traffic rather than the node's own.
const VIRTUAL_PREFIXES: &[&str] = &[
    "lo", "veth", "cni", "flannel", "docker", "kube", "dummy", "teql", "tunl", "sit", "ip6tnl",
    "cilium", "lxc",
];

impl NetworkStats {
    /// Received and transmitted bytes for the node: the top-level counters when present,
    /// otherwise the sum over interfaces that are not pod-side or loopback.
    pub fn totals(&self) -> (Option<u64>, Option<u64>) {
        if self.rx_bytes.is_some() || self.tx_bytes.is_some() {
            return (self.rx_bytes, self.tx_bytes);
        }
        let outward = self.interfaces.iter().filter(|interface| {
            !VIRTUAL_PREFIXES
                .iter()
                .any(|prefix| interface.name.starts_with(prefix))
        });
        let mut rx = None;
        let mut tx = None;
        for interface in outward {
            if let Some(bytes) = interface.rx_bytes {
                rx = Some(rx.unwrap_or(0) + bytes);
            }
            if let Some(bytes) = interface.tx_bytes {
                tx = Some(tx.unwrap_or(0) + bytes);
            }
        }
        (rx, tx)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsStats {
    pub used_bytes: Option<u64>,
    pub capacity_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodStats {
    pub pod_ref: PodRef,
    pub cpu: Option<CpuStats>,
    pub memory: Option<MemoryStats>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PodRef {
    pub name: String,
    pub namespace: String,
}

pub fn parse(text: &str) -> Result<Summary, serde_json::Error> {
    serde_json::from_str(text)
}

/// A CPU quantity as cores: `32`, `31800m`, `500000u`, `1500000000n`, `1.5`.
pub fn cpu_cores(quantity: &str) -> Option<f64> {
    let text = quantity.trim();
    let (number, scale) = if let Some(rest) = text.strip_suffix('n') {
        (rest, 1e-9)
    } else if let Some(rest) = text.strip_suffix('u') {
        (rest, 1e-6)
    } else if let Some(rest) = text.strip_suffix('m') {
        (rest, 1e-3)
    } else {
        (text, 1.0)
    };
    let value: f64 = number.trim().parse().ok()?;
    (value >= 0.0).then_some(value * scale)
}

/// A memory or storage quantity as bytes: `65722296Ki`, `64Gi`, `1.5Gi`, `1G`, `1000000`,
/// `1e9`, `123456k`.
pub fn bytes(quantity: &str) -> Option<u64> {
    const BINARY: [(&str, f64); 6] = [
        ("Ki", 1024.0),
        ("Mi", 1024.0 * 1024.0),
        ("Gi", 1024.0 * 1024.0 * 1024.0),
        ("Ti", 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Pi", 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0),
        ("Ei", 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0),
    ];
    const DECIMAL: [(&str, f64); 7] = [
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
        ("m", 1e-3),
    ];
    let text = quantity.trim();
    let mut scale = 1.0;
    let mut number = text;
    for (suffix, factor) in BINARY.iter().chain(DECIMAL.iter()) {
        if let Some(rest) = text.strip_suffix(suffix) {
            number = rest;
            scale = *factor;
            break;
        }
    }
    let value: f64 = number.trim().parse().ok()?;
    if value < 0.0 || !value.is_finite() {
        return None;
    }
    Some((value * scale).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_summary_fields_the_canvas_uses() {
        let text = r#"{"node":{"nodeName":"node-a","startTime":"2026-01-01T00:00:00Z",
            "cpu":{"time":"2026-01-01T00:00:10Z","usageNanoCores":315864297,"usageCoreNanoSeconds":1},
            "memory":{"time":"t","availableBytes":24000000000,"usageBytes":1,"workingSetBytes":40700000000,"rssBytes":21000000000},
            "network":{"time":"t","name":"eth0","rxBytes":1000,"txBytes":2000,"interfaces":[]},
            "fs":{"time":"t","availableBytes":1,"capacityBytes":2000000000000,"usedBytes":50000000000}},
            "pods":[{"podRef":{"name":"web-1","namespace":"apps","uid":"u"},
                     "cpu":{"time":"t","usageNanoCores":73000000},
                     "memory":{"time":"t","workingSetBytes":17000000000,"rssBytes":300000000}},
                    {"podRef":{"name":"idle","namespace":"apps","uid":"v"}}]}"#;
        let summary = parse(text).expect("summary");
        assert_eq!(summary.node.node_name, "node-a");
        assert_eq!(
            summary.node.cpu.as_ref().and_then(|c| c.usage_nano_cores),
            Some(315_864_297)
        );
        assert_eq!(
            summary.node.network.as_ref().and_then(|n| n.rx_bytes),
            Some(1000)
        );
        assert_eq!(
            summary.node.memory.as_ref().and_then(|m| m.rss_bytes),
            Some(21_000_000_000)
        );
        assert_eq!(summary.pods.len(), 2);
        assert_eq!(summary.pods[0].pod_ref.namespace, "apps");
        assert_eq!(
            summary.pods[0].memory.as_ref().and_then(|m| m.rss_bytes),
            Some(300_000_000)
        );
        assert!(summary.pods[1].cpu.is_none());
    }

    #[test]
    fn network_totals_come_from_the_top_level_or_the_outward_interfaces() {
        let top: NetworkStats = serde_json::from_str(
            r#"{"rxBytes":10,"txBytes":20,"interfaces":[{"name":"eth0","rxBytes":1,"txBytes":1}]}"#,
        )
        .expect("network");
        assert_eq!(top.totals(), (Some(10), Some(20)));

        let per_interface: NetworkStats = serde_json::from_str(
            r#"{"name":"","interfaces":[
                {"name":"enp6s0f1","rxBytes":1000,"txBytes":400},
                {"name":"tailscale0","rxBytes":10,"txBytes":260},
                {"name":"cni0","rxBytes":99999,"txBytes":99999},
                {"name":"flannel.1","rxBytes":5,"txBytes":5},
                {"name":"veth1234","rxBytes":7,"txBytes":7},
                {"name":"lo","rxBytes":3,"txBytes":3},
                {"name":"bond0","rxBytes":0,"txBytes":0}]}"#,
        )
        .expect("network");
        assert_eq!(per_interface.totals(), (Some(1010), Some(660)));

        let empty: NetworkStats = serde_json::from_str(r#"{"name":"eth0"}"#).expect("network");
        assert_eq!(empty.totals(), (None, None));
    }

    #[test]
    fn cpu_quantities() {
        assert_eq!(cpu_cores("32"), Some(32.0));
        assert_eq!(cpu_cores("31800m"), Some(31.8));
        assert_eq!(cpu_cores("500000u"), Some(0.5));
        assert_eq!(cpu_cores("1500000000n"), Some(1.5));
        assert_eq!(cpu_cores("1.5"), Some(1.5));
        assert_eq!(cpu_cores("many"), None);
        assert_eq!(cpu_cores("-1"), None);
    }

    #[test]
    fn byte_quantities() {
        assert_eq!(bytes("65722296Ki"), Some(65_722_296 * 1024));
        assert_eq!(bytes("64Gi"), Some(64 * 1024 * 1024 * 1024));
        assert_eq!(bytes("1.5Gi"), Some(1_610_612_736));
        assert_eq!(bytes("1G"), Some(1_000_000_000));
        assert_eq!(bytes("123456k"), Some(123_456_000));
        assert_eq!(bytes("1000000"), Some(1_000_000));
        assert_eq!(bytes("1e9"), Some(1_000_000_000));
        assert_eq!(bytes("lots"), None);
    }
}
