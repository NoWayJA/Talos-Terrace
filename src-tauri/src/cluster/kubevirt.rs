//! KubeVirt virtual machines, read through the dynamic API when the `kubevirt.io` group is
//! served. Optional and detected: a cluster without KubeVirt simply has no VMs. Field names
//! were checked against KubeVirt v1.9.0 on 18 September 2026.

use kube::Client;
use kube::api::DynamicObject;
use kube::discovery::{self, ApiResource};
use serde::Serialize;
use serde_json::Value;

pub const GROUP: &str = "kubevirt.io";

/// The two resources Terrace reads: the desired machine and its running instance.
#[derive(Debug, Clone)]
pub struct KubeVirtApi {
    pub vm: ApiResource,
    pub vmi: ApiResource,
}

/// Discover the KubeVirt API group. `None` when it is not installed (or not reachable).
pub async fn discover(client: &Client) -> Option<KubeVirtApi> {
    let group = match discovery::group(client, GROUP).await {
        Ok(group) => group,
        Err(error) => {
            tracing::debug!(%error, "kubevirt.io not served; virtual machines off");
            return None;
        }
    };
    let vm = group.recommended_kind("VirtualMachine")?.0;
    let vmi = group.recommended_kind("VirtualMachineInstance")?.0;
    tracing::info!(version = %vm.version, "KubeVirt detected");
    Some(KubeVirtApi { vm, vmi })
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VmDetails {
    /// KubeVirt's `printableStatus`: Running, Stopped, Starting, Stopping, Provisioning…
    pub status: String,
    pub ready: bool,
    pub run_strategy: Option<String>,
    /// Total virtual CPUs (sockets × cores × threads).
    pub cores: Option<u64>,
    /// Guest memory as written (`16Gi`).
    pub memory: Option<String>,
    /// From the guest agent, when installed (`Microsoft Windows 11 Pro`).
    pub guest_os: Option<String>,
    pub addresses: Vec<String>,
    pub node: Option<String>,
    /// Disk names with their kind: `win11-disk (disk)`, `win11-iso (cdrom)`.
    pub disks: Vec<String>,
    pub secure_boot: Option<bool>,
    /// Resource names of the GPUs and host devices the machine is given (`devices.gpus` and
    /// `devices.hostDevices`), as the cluster advertises them.
    pub gpus: Vec<String>,
}

fn text(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    match current {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn flag(value: &Value, path: &[&str]) -> Option<bool> {
    let mut current = value;
    for key in path {
        current = current.get(key)?;
    }
    current.as_bool()
}

/// KubeVirt topology counts sockets × cores × threads, each omitted dimension defaulting
/// to one. An absent topology is unknown (for example an unapplied instance type).
fn vcpu_count(topology: &serde_json::Value) -> Option<u64> {
    topology.as_object()?;
    ["sockets", "cores", "threads"]
        .into_iter()
        .try_fold(1u64, |total, key| {
            let count = match topology.get(key) {
                None => 1,
                Some(value) => value.as_u64()?.max(1),
            };
            total.checked_mul(count)
        })
}

/// Combine a VirtualMachine with its VirtualMachineInstance (absent when stopped).
pub fn details(vm: &DynamicObject, vmi: Option<&DynamicObject>) -> VmDetails {
    let spec = &vm.data["spec"];
    let domain = &spec["template"]["spec"]["domain"];
    let status = &vm.data["status"];
    let instance = vmi.map(|object| &object.data["status"]);

    let disks = domain["devices"]["disks"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|disk| {
                    let name = text(disk, &["name"])?;
                    let kind = ["disk", "cdrom", "lun", "floppy"]
                        .into_iter()
                        .find(|kind| disk.get(kind).is_some())
                        .unwrap_or("disk");
                    Some(format!("{name} ({kind})"))
                })
                .collect()
        })
        .unwrap_or_default();

    let gpus: Vec<String> = ["gpus", "hostDevices"]
        .into_iter()
        .filter_map(|kind| domain["devices"][kind].as_array())
        .flatten()
        .filter_map(|device| text(device, &["deviceName"]))
        .collect();

    let addresses = instance
        .and_then(|status| status["interfaces"].as_array())
        .map(|list| {
            list.iter()
                .filter_map(|interface| text(interface, &["ipAddress"]))
                .collect()
        })
        .unwrap_or_default();

    VmDetails {
        status: text(status, &["printableStatus"])
            .or_else(|| instance.and_then(|status| text(status, &["phase"])))
            .unwrap_or_else(|| "Unknown".to_string()),
        ready: flag(status, &["ready"]).unwrap_or(false),
        run_strategy: text(spec, &["runStrategy"]).or_else(|| {
            flag(spec, &["running"]).map(|running| {
                if running {
                    "Always".to_string()
                } else {
                    "Halted".to_string()
                }
            })
        }),
        cores: instance
            .and_then(|status| vcpu_count(&status["currentCPUTopology"]))
            .or_else(|| vcpu_count(&domain["cpu"])),
        memory: instance
            .and_then(|status| text(status, &["memory", "guestCurrent"]))
            .or_else(|| text(domain, &["memory", "guest"]))
            .or_else(|| text(domain, &["resources", "requests", "memory"])),
        guest_os: instance.and_then(|status| text(status, &["guestOSInfo", "prettyName"])),
        addresses,
        node: instance.and_then(|status| text(status, &["nodeName"])),
        disks,
        secure_boot: flag(domain, &["firmware", "bootloader", "efi", "secureBoot"]),
        gpus,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use kube::api::ObjectMeta;
    use serde_json::json;

    pub fn object(namespace: &str, name: &str, data: Value) -> DynamicObject {
        DynamicObject {
            types: None,
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                ..ObjectMeta::default()
            },
            data,
        }
    }

    fn vm() -> DynamicObject {
        object(
            "vm",
            "win11",
            json!({
                "spec": {
                    "runStrategy": "Always",
                    "template": {"spec": {"domain": {
                        "cpu": {"cores": 8, "sockets": 1, "threads": 1},
                        "memory": {"guest": "16Gi"},
                        "firmware": {"bootloader": {"efi": {"persistent": true, "secureBoot": true}}},
                        "devices": {"disks": [
                            {"name": "win11-disk", "bootOrder": 1, "disk": {"bus": "virtio"}},
                            {"name": "win11-iso", "bootOrder": 2, "cdrom": {"bus": "sata"}}
                        ], "gpus": [{"name": "gpu1", "deviceName": "vendor.com/CARD_A"}]}
                    }}}
                },
                "status": {"printableStatus": "Running", "ready": true}
            }),
        )
    }

    #[test]
    fn cpu_topology_counts_every_dimension() {
        assert_eq!(
            vcpu_count(&json!({"sockets": 2, "cores": 4, "threads": 2})),
            Some(16)
        );
        assert_eq!(vcpu_count(&json!({"sockets": 4})), Some(4));
        assert_eq!(vcpu_count(&json!(null)), None);
        assert_eq!(vcpu_count(&json!({"cores": u64::MAX, "sockets": 2})), None);
    }

    #[test]
    fn combines_the_machine_with_its_instance() {
        let vmi = object(
            "vm",
            "win11",
            json!({"status": {
                "phase": "Running", "nodeName": "node-a",
                "guestOSInfo": {"prettyName": "Microsoft Windows 11 Pro"},
                "interfaces": [{"ipAddress": "10.244.0.66", "name": "default"}],
                "memory": {"guestCurrent": "16Gi"},
                "currentCPUTopology": {"cores": 8}
            }}),
        );
        let details = details(&vm(), Some(&vmi));
        assert_eq!(details.status, "Running");
        assert!(details.ready);
        assert_eq!(details.cores, Some(8));
        assert_eq!(details.memory.as_deref(), Some("16Gi"));
        assert_eq!(
            details.guest_os.as_deref(),
            Some("Microsoft Windows 11 Pro")
        );
        assert_eq!(details.addresses, vec!["10.244.0.66"]);
        assert_eq!(details.node.as_deref(), Some("node-a"));
        assert_eq!(
            details.disks,
            vec!["win11-disk (disk)", "win11-iso (cdrom)"]
        );
        assert_eq!(details.secure_boot, Some(true));
        assert_eq!(details.run_strategy.as_deref(), Some("Always"));
        assert_eq!(details.gpus, vec!["vendor.com/CARD_A"]);
    }

    #[test]
    fn a_stopped_machine_has_no_instance() {
        let mut stopped = vm();
        stopped.data["status"] = json!({"printableStatus": "Stopped", "ready": false});
        let details = details(&stopped, None);
        assert_eq!(details.status, "Stopped");
        assert!(!details.ready);
        assert_eq!(details.cores, Some(8), "falls back to the spec");
        assert!(details.addresses.is_empty());
        assert_eq!(details.guest_os, None);
    }
}
