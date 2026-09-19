//! Live inventory of Talos nodes from their resources: everything the hardware, Talos and
//! trust layers show. Read-only (`talosctl get` only). Lenient about field names across Talos
//! versions: a missing field is omitted, never an error. Grouped by node, so multi-node
//! clusters work from the first version. Field names were checked against a Talos v1.14.0
//! node on 17 September 2026 (memory is `sizeMiB`, firmware is `biosVersion`, volume
//! statuses include directories that are filtered by `type`, a link speed of `u32::MAX`
//! means unknown).

use std::collections::BTreeSet;

use serde::Serialize;
use serde_json::Value;

use super::{TalosCli, TalosResource, TalosVersion};
use crate::error::{Result, TerraceError};

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecurityState {
    pub secure_boot: Option<bool>,
    pub booted_with_uki: Option<bool>,
    pub module_signature_enforced: Option<bool>,
    pub selinux: Option<String>,
    pub fips: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub version: Option<String>,
    pub bios_version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Processor {
    pub product: Option<String>,
    pub cores: Option<u64>,
    pub threads: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
    pub device: String,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub size_bytes: Option<u64>,
    pub pretty_size: Option<String>,
    pub transport: Option<String>,
    pub rotational: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub name: String,
    pub up: bool,
    pub kind: Option<String>,
    pub driver: Option<String>,
    pub speed_mbit: Option<u64>,
    pub hardware_addr: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    pub link: String,
    pub address: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub name: String,
    pub phase: Option<String>,
    /// `partition` or `disk`; directories, overlays and symlinks are not listed.
    pub volume_type: Option<String>,
    pub filesystem: Option<String>,
    pub encryption: Option<String>,
    pub device: Option<String>,
    pub mount: Option<String>,
    pub pretty_size: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PciDevice {
    pub vendor: Option<String>,
    pub product: Option<String>,
    pub class: Option<String>,
    pub driver: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeInventory {
    pub node: String,
    pub hostname: Option<String>,
    /// This node's Talos version; nodes differ mid-upgrade.
    pub version: Option<String>,
    pub stage: Option<String>,
    pub ready: Option<bool>,
    pub security: SecurityState,
    pub system: Option<SystemInfo>,
    pub processors: Vec<Processor>,
    pub memory_mib: u64,
    pub disks: Vec<Disk>,
    pub links: Vec<Link>,
    pub addresses: Vec<Address>,
    pub extensions: Vec<Extension>,
    pub schematic: Option<String>,
    pub volumes: Vec<Volume>,
    pub gpus: Vec<PciDevice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalosInventory {
    pub version: TalosVersion,
    pub nodes: Vec<NodeInventory>,
    /// Resources that could not be read, one line each. Empty on a healthy node.
    pub warnings: Vec<String>,
}

/// The raw resources the inventory is built from, one list per resource type.
#[derive(Debug, Default)]
pub struct ResourceSet {
    pub machine_status: Vec<TalosResource>,
    pub security_state: Vec<TalosResource>,
    pub hostname: Vec<TalosResource>,
    pub system_information: Vec<TalosResource>,
    pub processors: Vec<TalosResource>,
    pub memory_modules: Vec<TalosResource>,
    pub disks: Vec<TalosResource>,
    pub links: Vec<TalosResource>,
    pub addresses: Vec<TalosResource>,
    pub extensions: Vec<TalosResource>,
    pub volume_status: Vec<TalosResource>,
    pub pci_devices: Vec<TalosResource>,
}

// --- Lenient JSON access -----------------------------------------------------

fn text(spec: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| match spec.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    })
}

fn flag(spec: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| spec.get(key).and_then(Value::as_bool))
}

fn number(spec: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| spec.get(key).and_then(Value::as_u64))
}

fn node_name(resource: &TalosResource) -> String {
    resource.node.clone().unwrap_or_else(|| "node".to_string())
}

fn for_node<'a>(list: &'a [TalosResource], node: &str) -> impl Iterator<Item = &'a TalosResource> {
    list.iter()
        .filter(move |resource| node_name(resource) == node)
}

const VIRTUAL_LINK_KINDS: &[&str] = &[
    "veth", "dummy", "ipip", "sit", "ip6tnl", "gre", "gretap", "erspan", "ip6gre", "vxlan",
];
const VIRTUAL_LINK_PREFIXES: &[&str] = &[
    "veth",
    "cni",
    "flannel",
    "kube-ipvs",
    "cilium",
    "cali",
    "lxc",
    "docker",
    "nodelocaldns",
    "teql",
];
/// The kernel reports this speed for links that have none (tunnels, unplugged ports).
const UNKNOWN_SPEED: u64 = u32::MAX as u64;
/// Volume types worth showing; the rest are directories, overlays and symlinks.
const BLOCK_VOLUME_TYPES: &[&str] = &["partition", "disk"];

fn is_virtual_link(name: &str, kind: Option<&str>) -> bool {
    name == "lo"
        || VIRTUAL_LINK_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
        || kind.is_some_and(|kind| VIRTUAL_LINK_KINDS.contains(&kind))
}

fn is_link_type(resource: &TalosResource) -> bool {
    resource.type_.starts_with("LinkStatuses")
}

fn is_address_type(resource: &TalosResource) -> bool {
    resource.type_.starts_with("AddressStatuses")
}

/// Whether a change to this resource could change the inventory. Pod churn creates and
/// destroys `veth` links and link-local addresses all day; those never show and are ignored.
pub fn affects_inventory(resource: &TalosResource) -> bool {
    if is_link_type(resource) {
        let kind = text(&resource.spec, &["kind"]);
        return !is_virtual_link(&resource.id, kind.as_deref());
    }
    if is_address_type(resource) {
        let link = text(&resource.spec, &["linkName", "link_name"]).unwrap_or_default();
        let scope = text(&resource.spec, &["scope"]);
        return !is_virtual_link(&link, None) && scope.as_deref().is_none_or(|s| s == "global");
    }
    true
}

fn is_pseudo_disk(name: &str) -> bool {
    name.starts_with("loop")
        || name.starts_with("ram")
        || name.starts_with("sr")
        || name.starts_with("zram")
}

// --- Builder -------------------------------------------------------------------

/// Turn raw resources into one inventory per node. Pure, so it is unit-tested with fixtures.
pub fn build(version: TalosVersion, set: ResourceSet, warnings: Vec<String>) -> TalosInventory {
    let lists: [&[TalosResource]; 12] = [
        &set.machine_status,
        &set.security_state,
        &set.hostname,
        &set.system_information,
        &set.processors,
        &set.memory_modules,
        &set.disks,
        &set.links,
        &set.addresses,
        &set.extensions,
        &set.volume_status,
        &set.pci_devices,
    ];
    let node_names: BTreeSet<String> = lists
        .iter()
        .flat_map(|list| list.iter().map(node_name))
        .collect();

    let nodes = node_names
        .into_iter()
        .map(|node| {
            let mut built = build_node(&node, &set);
            built.version = version
                .nodes
                .iter()
                .find(|entry| entry.node == node)
                .map(|entry| entry.tag.clone())
                .or_else(|| version.server.clone());
            built
        })
        .collect();
    TalosInventory {
        version,
        nodes,
        warnings,
    }
}

fn build_node(node: &str, set: &ResourceSet) -> NodeInventory {
    let machine = for_node(&set.machine_status, node).next();
    let security = for_node(&set.security_state, node).next().map(|r| &r.spec);
    let system = for_node(&set.system_information, node)
        .next()
        .map(|r| &r.spec);

    let mut extensions = Vec::new();
    let mut schematic = None;
    for resource in for_node(&set.extensions, node) {
        let metadata = resource.spec.get("metadata").unwrap_or(&Value::Null);
        let name = text(metadata, &["name"]).unwrap_or_else(|| resource.id.clone());
        let version = text(metadata, &["version"]);
        if name == "schematic" {
            schematic = version.clone();
        }
        extensions.push(Extension { name, version });
    }
    extensions.sort_by(|a, b| a.name.cmp(&b.name));

    let links: Vec<Link> = for_node(&set.links, node)
        .filter(|r| {
            let kind = text(&r.spec, &["kind"]);
            !is_virtual_link(&r.id, kind.as_deref())
        })
        .map(|r| Link {
            name: r.id.clone(),
            up: text(&r.spec, &["operationalState", "operational_state"])
                .is_some_and(|state| state == "up" || state == "unknown")
                || flag(&r.spec, &["linkState", "link_state"]).unwrap_or(false),
            kind: text(&r.spec, &["kind"]),
            driver: text(&r.spec, &["driver"]),
            speed_mbit: number(&r.spec, &["speedMbit", "speed_mbit"])
                .filter(|speed| *speed > 0 && *speed != UNKNOWN_SPEED),
            hardware_addr: text(&r.spec, &["hardwareAddr", "hardware_addr"]),
        })
        .collect();

    NodeInventory {
        node: node.to_string(),
        version: None,
        hostname: for_node(&set.hostname, node)
            .next()
            .and_then(|r| text(&r.spec, &["hostname"])),
        stage: machine.and_then(|r| text(&r.spec, &["stage"])),
        ready: machine
            .and_then(|r| r.spec.get("status"))
            .and_then(|s| flag(s, &["ready"])),
        security: security
            .map(|spec| SecurityState {
                secure_boot: flag(spec, &["secureBoot", "secure_boot"]),
                booted_with_uki: flag(spec, &["bootedWithUKI", "bootedWithUki", "booted_with_uki"]),
                module_signature_enforced: flag(
                    spec,
                    &["moduleSignatureEnforced", "module_signature_enforced"],
                ),
                selinux: text(spec, &["selinuxState", "selinux_state", "selinux"]),
                fips: flag(spec, &["fips", "fipsState"]),
            })
            .unwrap_or_default(),
        system: system.map(|spec| SystemInfo {
            manufacturer: text(spec, &["manufacturer"]),
            product: text(spec, &["productName", "product_name", "product"]),
            version: text(spec, &["version"]),
            bios_version: text(spec, &["biosVersion", "bios_version"]),
        }),
        processors: for_node(&set.processors, node)
            .map(|r| Processor {
                product: text(&r.spec, &["productName", "product_name"]),
                cores: number(&r.spec, &["coreCount", "core_count"]),
                threads: number(&r.spec, &["threadCount", "thread_count"]),
            })
            .collect(),
        memory_mib: for_node(&set.memory_modules, node)
            .filter_map(|r| number(&r.spec, &["sizeMiB", "size_mib", "size"]))
            .sum(),
        disks: for_node(&set.disks, node)
            .filter(|r| !is_pseudo_disk(&r.id))
            .filter(|r| !flag(&r.spec, &["cdrom"]).unwrap_or(false))
            .map(|r| Disk {
                device: text(&r.spec, &["dev_path", "devPath"]).unwrap_or_else(|| r.id.clone()),
                model: text(&r.spec, &["model"]),
                serial: text(&r.spec, &["serial"]),
                size_bytes: number(&r.spec, &["size"]),
                pretty_size: text(&r.spec, &["pretty_size", "prettySize"]),
                transport: text(&r.spec, &["transport"]),
                rotational: flag(&r.spec, &["rotational"]),
            })
            .collect(),
        links: links.clone(),
        addresses: for_node(&set.addresses, node)
            .filter(|r| {
                let link = text(&r.spec, &["linkName", "link_name"]).unwrap_or_default();
                let scope = text(&r.spec, &["scope"]);
                let address = text(&r.spec, &["address"]).unwrap_or_default();
                let on_shown_link = if links.is_empty() {
                    !is_virtual_link(&link, None)
                } else {
                    links.iter().any(|shown| shown.name == link)
                };
                on_shown_link
                    && !address.starts_with("fe80:")
                    && !address.starts_with("127.")
                    && scope.as_deref().is_none_or(|scope| scope == "global")
            })
            .map(|r| Address {
                link: text(&r.spec, &["linkName", "link_name"]).unwrap_or_default(),
                address: text(&r.spec, &["address"]).unwrap_or_else(|| r.id.clone()),
            })
            .collect(),
        extensions,
        schematic,
        volumes: for_node(&set.volume_status, node)
            .filter(|r| {
                text(&r.spec, &["type"])
                    .is_none_or(|kind| BLOCK_VOLUME_TYPES.contains(&kind.as_str()))
            })
            .map(|r| Volume {
                name: r.id.clone(),
                phase: text(&r.spec, &["phase"]),
                volume_type: text(&r.spec, &["type"]),
                filesystem: text(&r.spec, &["filesystem"]),
                encryption: text(&r.spec, &["encryptionProvider", "encryption_provider"]),
                device: text(&r.spec, &["location", "mountLocation", "mount_location"]),
                mount: r
                    .spec
                    .get("mountSpec")
                    .and_then(|mount| text(mount, &["targetPath", "target_path"])),
                pretty_size: text(&r.spec, &["prettySize", "pretty_size"]),
            })
            .collect(),
        gpus: for_node(&set.pci_devices, node)
            .filter(|r| {
                text(&r.spec, &["class"])
                    .is_some_and(|class| class.contains("Display") || class.contains("3D"))
            })
            .map(|r| PciDevice {
                vendor: text(&r.spec, &["vendor"]),
                product: text(&r.spec, &["product"]),
                class: text(&r.spec, &["subclass", "class"]),
                driver: text(&r.spec, &["driver"]),
            })
            .collect(),
    }
}

// --- Reading -------------------------------------------------------------------

impl TalosCli {
    /// Read every resource the layers need, concurrently, tolerating individual failures.
    pub async fn inventory(&self) -> Result<TalosInventory> {
        let version = self.version().await?;
        if version.server.is_none() {
            return Err(TerraceError::NotConnected(format!(
                "Talos API not reachable (talosctl client {})",
                version.client
            )));
        }

        let (
            machine_status,
            security_state,
            hostname,
            system_information,
            processors,
            memory_modules,
            disks,
            links,
            addresses,
            extensions,
            volume_status,
            pci_devices,
        ) = tokio::join!(
            self.get("machinestatus", None),
            self.get("securitystate", None),
            self.get("hostname", None),
            self.get("systeminformation", None),
            self.get("processors", None),
            self.get("memorymodules", None),
            self.get("disks", None),
            self.get("links", None),
            self.get("addresses", None),
            self.get("extensions", None),
            self.get("volumestatus", None),
            self.get("pcidevices", None),
        );

        let mut warnings = Vec::new();
        let mut take = |name: &str, result: Result<Vec<TalosResource>>| match result {
            Ok(list) => list,
            Err(error) => {
                warnings.push(format!("{name}: {error}"));
                Vec::new()
            }
        };
        let set = ResourceSet {
            machine_status: take("machinestatus", machine_status),
            security_state: take("securitystate", security_state),
            hostname: take("hostname", hostname),
            system_information: take("systeminformation", system_information),
            processors: take("processors", processors),
            memory_modules: take("memorymodules", memory_modules),
            disks: take("disks", disks),
            links: take("links", links),
            addresses: take("addresses", addresses),
            extensions: take("extensions", extensions),
            volume_status: take("volumestatus", volume_status),
            pci_devices: take("pcidevices", pci_devices),
        };
        Ok(build(version, set, warnings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resource(node: &str, type_: &str, id: &str, spec: Value) -> TalosResource {
        TalosResource {
            node: Some(node.to_string()),
            namespace: "runtime".to_string(),
            type_: type_.to_string(),
            id: id.to_string(),
            version: Some(1),
            spec,
        }
    }

    fn version() -> TalosVersion {
        TalosVersion {
            client: "v1.14.0".to_string(),
            server: Some("v1.14.0".to_string()),
            node: Some("10.0.0.1".to_string()),
            nodes: vec![crate::talos::NodeVersion {
                node: "10.0.0.1".to_string(),
                tag: "v1.14.0".to_string(),
            }],
        }
    }

    fn fixture() -> ResourceSet {
        let n = "10.0.0.1";
        ResourceSet {
            machine_status: vec![resource(
                n,
                "MachineStatuses",
                "machine",
                json!({"stage": "running", "status": {"ready": true, "unmetConditions": []}}),
            )],
            security_state: vec![resource(
                n,
                "SecurityStates",
                "securitystate",
                json!({"secureBoot": false, "bootedWithUKI": true, "moduleSignatureEnforced": true, "selinuxState": "enabled, permissive"}),
            )],
            hostname: vec![resource(
                n,
                "HostnameStatuses",
                "hostname",
                json!({"hostname": "node-a"}),
            )],
            system_information: vec![resource(
                n,
                "SystemInformations",
                "systeminformation",
                json!({"manufacturer": "Example Boards", "productName": "Board X1", "version": "1.0", "biosVersion": "1.20"}),
            )],
            processors: vec![resource(
                n,
                "Processors",
                "CPU0",
                json!({"productName": "Example CPU 12-Core", "coreCount": 12, "threadCount": 24}),
            )],
            memory_modules: vec![
                resource(n, "MemoryModules", "DIMM_A1", json!({"sizeMiB": 32768})),
                resource(n, "MemoryModules", "DIMM_B1", json!({"sizeMiB": 32768})),
                resource(n, "MemoryModules", "DIMM_A2", json!({"sizeMiB": 0})),
            ],
            disks: vec![
                resource(
                    n,
                    "Disks",
                    "nvme0n1",
                    json!({"dev_path": "/dev/nvme0n1", "model": "Disk Maker A", "serial": "S1", "size": 1000000000000_u64, "pretty_size": "1.0 TB", "transport": "nvme", "rotational": false}),
                ),
                resource(
                    n,
                    "Disks",
                    "loop0",
                    json!({"dev_path": "/dev/loop0", "readonly": true}),
                ),
            ],
            links: vec![
                resource(
                    n,
                    "LinkStatuses",
                    "lo",
                    json!({"type": "loopback", "operationalState": "unknown"}),
                ),
                resource(
                    n,
                    "LinkStatuses",
                    "eth0",
                    json!({"type": "ether", "kind": "", "operationalState": "up", "driver": "nic", "speedMbit": 2500, "hardwareAddr": "aa:bb"}),
                ),
                resource(
                    n,
                    "LinkStatuses",
                    "tailscale0",
                    json!({"type": "nohdr", "kind": "tun", "operationalState": "unknown", "linkState": true, "speedMbit": 4294967295_u64}),
                ),
                resource(
                    n,
                    "LinkStatuses",
                    "flannel.1",
                    json!({"type": "ether", "kind": "vxlan", "operationalState": "unknown", "linkState": true}),
                ),
                resource(
                    n,
                    "LinkStatuses",
                    "teql0",
                    json!({"type": "void", "kind": "", "operationalState": "down", "linkState": false}),
                ),
                resource(
                    n,
                    "LinkStatuses",
                    "veth1234",
                    json!({"type": "ether", "kind": "veth", "operationalState": "up"}),
                ),
            ],
            addresses: vec![
                resource(
                    n,
                    "AddressStatuses",
                    "lo/127.0.0.1/8",
                    json!({"address": "127.0.0.1/8", "linkName": "lo", "scope": "host"}),
                ),
                resource(
                    n,
                    "AddressStatuses",
                    "eth0/192.168.1.10/24",
                    json!({"address": "192.168.1.10/24", "linkName": "eth0", "scope": "global"}),
                ),
                resource(
                    n,
                    "AddressStatuses",
                    "eth0/fe80::1/64",
                    json!({"address": "fe80::1/64", "linkName": "eth0", "scope": "link"}),
                ),
                resource(
                    n,
                    "AddressStatuses",
                    "tailscale0/100.64.0.1/32",
                    json!({"address": "100.64.0.1/32", "linkName": "tailscale0", "scope": "global"}),
                ),
                resource(
                    n,
                    "AddressStatuses",
                    "flannel.1/10.244.0.0/32",
                    json!({"address": "10.244.0.0/32", "linkName": "flannel.1", "scope": "global"}),
                ),
            ],
            extensions: vec![
                resource(
                    n,
                    "ExtensionStatuses",
                    "0",
                    json!({"image": "x", "metadata": {"name": "tailscale", "version": "1.0.0"}}),
                ),
                resource(
                    n,
                    "ExtensionStatuses",
                    "1",
                    json!({"image": "y", "metadata": {"name": "schematic", "version": "0123abcd"}}),
                ),
            ],
            volume_status: vec![
                resource(
                    n,
                    "VolumeStatuses",
                    "STATE",
                    json!({"phase": "ready", "type": "partition", "filesystem": "xfs", "encryptionProvider": "", "location": "/dev/nvme0n1p3", "mountSpec": {"targetPath": "/system/state"}, "prettySize": "105 MB"}),
                ),
                resource(
                    n,
                    "VolumeStatuses",
                    "EPHEMERAL",
                    json!({"phase": "ready", "type": "partition", "filesystem": "xfs", "encryptionProvider": "luks2", "location": "/dev/nvme0n1p4", "mountSpec": {"targetPath": "/var"}}),
                ),
                resource(
                    n,
                    "VolumeStatuses",
                    "/var/lib/cni",
                    json!({"phase": "ready", "type": "directory"}),
                ),
                resource(
                    n,
                    "VolumeStatuses",
                    "/opt",
                    json!({"phase": "ready", "type": "overlay"}),
                ),
            ],
            pci_devices: vec![
                resource(
                    n,
                    "PCIDevices",
                    "0000:0a:00.0",
                    json!({"class": "Display controller", "class_id": "0x03", "subclass": "VGA compatible controller", "vendor": "Example Silicon", "product": "Example GPU [Card A]"}),
                ),
                resource(
                    n,
                    "PCIDevices",
                    "0000:00:00.0",
                    json!({"class": "Bridge", "vendor": "Example Silicon"}),
                ),
            ],
        }
    }

    #[test]
    fn builds_one_inventory_per_node_with_filtered_hardware() {
        let inventory = build(version(), fixture(), vec![]);
        assert_eq!(inventory.nodes.len(), 1);
        let node = &inventory.nodes[0];
        assert_eq!(node.hostname.as_deref(), Some("node-a"));
        assert_eq!(node.version.as_deref(), Some("v1.14.0"));
        assert_eq!(node.stage.as_deref(), Some("running"));
        assert_eq!(node.ready, Some(true));
        assert_eq!(node.security.secure_boot, Some(false));
        assert_eq!(node.security.booted_with_uki, Some(true));
        assert_eq!(
            node.security.selinux.as_deref(),
            Some("enabled, permissive")
        );
        assert_eq!(node.security.module_signature_enforced, Some(true));
        assert_eq!(
            node.system
                .as_ref()
                .and_then(|s| s.manufacturer.clone())
                .as_deref(),
            Some("Example Boards")
        );
        assert_eq!(
            node.system.as_ref().and_then(|s| s.bios_version.as_deref()),
            Some("1.20")
        );
        assert_eq!(node.processors[0].cores, Some(12));
        assert_eq!(node.memory_mib, 65536, "sizeMiB is summed");
        assert_eq!(node.disks.len(), 1, "loop devices are dropped");
        assert_eq!(node.disks[0].pretty_size.as_deref(), Some("1.0 TB"));
        let link_names: Vec<&str> = node.links.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(link_names, vec!["eth0", "tailscale0"]);
        assert!(
            node.links[1].up,
            "a tun link with linkState true counts as up"
        );
        assert_eq!(node.links[0].speed_mbit, Some(2500));
        assert_eq!(node.links[1].speed_mbit, None, "u32::MAX means unknown");
        let addresses: Vec<&str> = node.addresses.iter().map(|a| a.address.as_str()).collect();
        assert_eq!(
            addresses,
            vec!["192.168.1.10/24", "100.64.0.1/32"],
            "only addresses on shown links"
        );
        assert_eq!(node.schematic.as_deref(), Some("0123abcd"));
        assert_eq!(node.extensions.len(), 2);
        assert_eq!(
            node.volumes.len(),
            2,
            "directories and overlays are dropped"
        );
        assert_eq!(node.volumes[1].encryption.as_deref(), Some("luks2"));
        assert_eq!(node.volumes[1].device.as_deref(), Some("/dev/nvme0n1p4"));
        assert_eq!(node.volumes[1].mount.as_deref(), Some("/var"));
        assert_eq!(node.gpus.len(), 1);
        assert_eq!(
            node.gpus[0].driver, None,
            "Talos may not report the bound driver"
        );
        assert_eq!(
            node.gpus[0].product.as_deref(),
            Some("Example GPU [Card A]")
        );
    }

    #[test]
    fn groups_resources_by_node() {
        let mut set = fixture();
        set.hostname.push(resource(
            "10.0.0.2",
            "HostnameStatuses",
            "hostname",
            json!({"hostname": "node-b"}),
        ));
        set.disks.push(resource(
            "10.0.0.2",
            "Disks",
            "sda",
            json!({"dev_path": "/dev/sda", "size": 1}),
        ));
        let inventory = build(version(), set, vec!["pcidevices: unsupported".to_string()]);
        assert_eq!(inventory.nodes.len(), 2);
        assert_eq!(inventory.nodes[1].hostname.as_deref(), Some("node-b"));
        assert_eq!(inventory.nodes[1].disks.len(), 1);
        assert!(inventory.nodes[1].gpus.is_empty());
        assert_eq!(inventory.warnings.len(), 1);
    }

    #[test]
    fn pod_churn_does_not_affect_the_inventory() {
        let veth = resource(
            "n",
            "LinkStatuses.net.talos.dev",
            "veth12ab",
            json!({"kind": "veth"}),
        );
        let link_local = resource(
            "n",
            "AddressStatuses.net.talos.dev",
            "eth0/fe80::1/64",
            json!({"linkName": "eth0", "scope": "link"}),
        );
        let real = resource(
            "n",
            "AddressStatuses.net.talos.dev",
            "eth0/10.0.0.1/24",
            json!({"linkName": "eth0", "scope": "global"}),
        );
        let disk = resource("n", "Disks.block.talos.dev", "sda", json!({}));
        assert!(!affects_inventory(&veth));
        assert!(!affects_inventory(&link_local));
        assert!(affects_inventory(&real));
        assert!(affects_inventory(&disk));
    }

    #[test]
    fn missing_fields_are_omitted_not_errors() {
        let set = ResourceSet {
            security_state: vec![resource("n", "SecurityStates", "securitystate", json!({}))],
            ..ResourceSet::default()
        };
        let inventory = build(version(), set, vec![]);
        assert_eq!(inventory.nodes[0].security, SecurityState::default());
        assert!(inventory.nodes[0].hostname.is_none());
    }
}
