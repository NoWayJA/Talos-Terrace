//! Read-only Talos access through the `talosctl` binary (the M0 sidecar; `tonic` against the
//! Talos protos comes after). Only three subcommands are ever run: `version`,
//! `get … --output json` (also with `--watch` for the live path) and `read` for the few files
//! the resources do not cover (temperature sensors). Nothing here can change the machine. The guarded write path
//! (apply-config, upgrade, reboot) is milestone M5 and will live in its own module with the
//! echo-and-confirm rules from the project's rules.

pub mod inventory;

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{Result, TerraceError};

pub use inventory::TalosInventory;

const TIMEOUT: Duration = Duration::from_secs(30);

/// One node's server version, from the `NODE:` block `talosctl version` prints per node.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NodeVersion {
    pub node: String,
    pub tag: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TalosVersion {
    pub client: String,
    /// The first node's version; the same on a healthy cluster, mid-upgrade see `nodes`.
    pub server: Option<String>,
    pub node: Option<String>,
    /// Every node the talosconfig reached, with its own version.
    pub nodes: Vec<NodeVersion>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TalosResource {
    pub node: Option<String>,
    pub namespace: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub id: String,
    pub version: Option<u64>,
    pub spec: serde_json::Value,
}

/// One document from `talosctl get --watch --output json`: the resource plus what happened
/// to it (`created`, `updated`, `destroyed`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchEvent {
    pub event: String,
    pub resource: TalosResource,
}

#[derive(Deserialize)]
struct RawResource {
    /// Present only in `--watch` output.
    #[serde(default)]
    event: Option<String>,
    node: Option<String>,
    metadata: RawMetadata,
    #[serde(default)]
    spec: serde_json::Value,
}

impl RawResource {
    fn into_resource(self) -> TalosResource {
        TalosResource {
            node: self.node,
            namespace: self.metadata.namespace,
            type_: self.metadata.type_,
            id: self.metadata.id,
            version: self.metadata.version.as_ref().and_then(|v| v.as_u64()),
            spec: self.spec,
        }
    }
}

#[derive(Deserialize)]
struct RawMetadata {
    namespace: String,
    #[serde(rename = "type")]
    type_: String,
    /// Usually a string; `ExtensionStatuses` number theirs (`0`, `1`, …) as JSON integers.
    #[serde(deserialize_with = "string_or_number")]
    id: String,
    #[serde(default)]
    version: Option<serde_json::Value>,
}

fn string_or_number<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::String(text) => Ok(text),
        serde_json::Value::Number(number) => Ok(number.to_string()),
        other => Err(serde::de::Error::custom(format!(
            "resource id must be a string or a number, got {other}"
        ))),
    }
}

pub struct TalosCli {
    binary: String,
    talosconfig: PathBuf,
}

impl TalosCli {
    pub fn new(binary: impl Into<String>, talosconfig: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            talosconfig: talosconfig.into(),
        }
    }

    /// The `talosctl` invocation every call shares: talosconfig in place, pipes, killed on drop.
    fn command(&self, args: &[&str]) -> Result<tokio::process::Command> {
        if !self.talosconfig.exists() {
            return Err(TerraceError::NotConnected(format!(
                "talosconfig not found at {}",
                self.talosconfig.display()
            )));
        }
        let mut command = tokio::process::Command::new(&self.binary);
        command
            .arg("--talosconfig")
            .arg(&self.talosconfig)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        Ok(command)
    }

    fn spawn_error(&self, error: std::io::Error) -> TerraceError {
        match error.kind() {
            std::io::ErrorKind::NotFound => TerraceError::Config(format!(
                "talosctl not found at '{}'; set talosctl.path in Settings",
                self.binary
            )),
            _ => TerraceError::Talos(format!("cannot run {}: {error}", self.binary)),
        }
    }

    async fn run(&self, args: &[&str]) -> Result<std::process::Output> {
        let mut command = self.command(args)?;
        tokio::time::timeout(TIMEOUT, command.output())
            .await
            .map_err(|_| TerraceError::Timeout(TIMEOUT))?
            .map_err(|error| self.spawn_error(error))
    }

    /// `talosctl get <resource> --output json --watch` as a child process whose stdout streams
    /// documents for as long as it lives. Dropping the child kills it; feed its stdout through
    /// [`drain_documents`].
    pub fn spawn_watch(&self, resource: &str) -> Result<tokio::process::Child> {
        validate(resource, "resource")?;
        let mut command = self.command(&["get", resource, "--output", "json", "--watch"])?;
        command.spawn().map_err(|error| self.spawn_error(error))
    }

    /// Client and, when reachable, server versions. An unreachable server is not an error:
    /// the client half still tells the canvas what it is talking with.
    pub async fn version(&self) -> Result<TalosVersion> {
        let output = self.run(&["version"]).await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut version = parse_version(&stdout);
        if version.client.is_empty() {
            return Err(TerraceError::Talos(format!(
                "talosctl version failed: {}",
                first_line(&String::from_utf8_lossy(&output.stderr))
            )));
        }
        if !output.status.success() {
            tracing::warn!(
                reason = first_line(&String::from_utf8_lossy(&output.stderr)),
                "talosctl reached the client but not the server"
            );
            version.server = None;
            version.node = None;
        }
        Ok(version)
    }

    /// `talosctl get <resource> [--namespace <ns>] --output json`, parsed.
    pub async fn get(&self, resource: &str, namespace: Option<&str>) -> Result<Vec<TalosResource>> {
        validate(resource, "resource")?;
        let mut args = vec!["get", resource, "--output", "json"];
        if let Some(namespace) = namespace {
            validate(namespace, "namespace")?;
            args.push("--namespace");
            args.push(namespace);
        }

        let output = self.run(&args).await?;
        if !output.status.success() {
            return Err(TerraceError::Talos(format!(
                "talosctl get {resource} failed: {}",
                first_line(&String::from_utf8_lossy(&output.stderr))
            )));
        }
        parse_resources(&String::from_utf8_lossy(&output.stdout))
    }

    /// `talosctl --nodes <node> read <path>`: a file's contents as text, for the sysfs and proc
    /// files no resource covers. Absolute paths only.
    pub async fn read(&self, node: &str, path: &str) -> Result<String> {
        validate_node(node)?;
        validate_path(path)?;
        let output = self.run(&["--nodes", node, "read", path]).await?;
        if !output.status.success() {
            return Err(TerraceError::Talos(format!(
                "talosctl read {path} failed: {}",
                first_line(&String::from_utf8_lossy(&output.stderr))
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

fn validate(value: &str, what: &str) -> Result<()> {
    let clean = !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    if clean {
        Ok(())
    } else {
        Err(TerraceError::Config(format!("invalid {what} '{value}'")))
    }
}

/// A node as talosctl names it: a hostname, an IPv4 address or an IPv6 address (with a zone),
/// never something that reads as an option.
fn validate_node(value: &str) -> Result<()> {
    let clean = !value.is_empty()
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '%'));
    if clean {
        Ok(())
    } else {
        Err(TerraceError::Config(format!("invalid node '{value}'")))
    }
}

/// An absolute path made of ordinary characters: no options, no spaces, no traversal.
fn validate_path(path: &str) -> Result<()> {
    let clean = path.starts_with('/')
        && !path.contains("..")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '-' | '_'));
    if clean {
        Ok(())
    } else {
        Err(TerraceError::Config(format!("invalid path '{path}'")))
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

/// Parse the human output of `talosctl version`.
pub fn parse_version(text: &str) -> TalosVersion {
    let mut section = "";
    let mut client = String::new();
    let mut nodes: Vec<NodeVersion> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("Client:") {
            section = "client";
            continue;
        }
        if trimmed.starts_with("Server:") {
            section = "server";
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("Tag:") {
            match section {
                "client" => client = value.trim().to_string(),
                "server" => match nodes.last_mut() {
                    Some(last) if last.tag.is_empty() => last.tag = value.trim().to_string(),
                    // A server block without a NODE line (an older talosctl): one nameless node.
                    _ => nodes.push(NodeVersion {
                        node: String::new(),
                        tag: value.trim().to_string(),
                    }),
                },
                _ => {}
            }
        } else if let Some(value) = trimmed.strip_prefix("NODE:")
            && section == "server"
        {
            nodes.push(NodeVersion {
                node: value.trim().to_string(),
                tag: String::new(),
            });
        }
    }

    let first = nodes.first();
    TalosVersion {
        client,
        server: first
            .map(|entry| entry.tag.clone())
            .filter(|tag| !tag.is_empty()),
        node: first
            .map(|entry| entry.node.clone())
            .filter(|node| !node.is_empty()),
        nodes: nodes
            .into_iter()
            .filter(|entry| !entry.node.is_empty())
            .collect(),
    }
}

/// Parse the stream of JSON documents `talosctl get --output json` prints.
pub fn parse_resources(text: &str) -> Result<Vec<TalosResource>> {
    let mut resources = Vec::new();
    for document in serde_json::Deserializer::from_str(text).into_iter::<RawResource>() {
        resources.push(document?.into_resource());
    }
    Ok(resources)
}

/// Take every complete JSON document off the front of `buffer`, leaving a partial one behind
/// for the next read. The watch output is pretty-printed, so documents span many lines.
pub fn drain_documents(buffer: &mut String) -> Vec<WatchEvent> {
    let mut events = Vec::new();
    let consumed = {
        let mut stream = serde_json::Deserializer::from_str(buffer).into_iter::<RawResource>();
        let mut consumed = 0;
        loop {
            match stream.next() {
                Some(Ok(raw)) => {
                    consumed = stream.byte_offset();
                    events.push(WatchEvent {
                        event: raw.event.clone().unwrap_or_else(|| "updated".to_string()),
                        resource: raw.into_resource(),
                    });
                }
                Some(Err(error)) if error.is_eof() => break,
                Some(Err(error)) => {
                    tracing::warn!(%error, "dropping unreadable talosctl watch output");
                    consumed = buffer.len();
                    break;
                }
                None => {
                    consumed = buffer.len();
                    break;
                }
            }
        }
        consumed
    };
    buffer.drain(..consumed);
    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nodes_may_be_ipv6_but_never_options() {
        assert!(validate_node("10.0.0.1").is_ok());
        assert!(validate_node("fd00::10").is_ok());
        assert!(validate_node("fe80::1%eth0").is_ok());
        assert!(validate_node("node-a.lab").is_ok());
        assert!(validate_node("--endpoints").is_err());
        assert!(validate_node("").is_err());
        assert!(validate_node("a b").is_err());
    }

    const VERSION_OUTPUT: &str = "Client:
\tTag:         v1.14.0
\tSHA:         0123abcd
\tBuilt:       
\tGo version:  go1.25.1
\tOS/Arch:     darwin/arm64
Server:
\tNODE:        10.0.0.5
\tTag:         v1.14.0
\tSHA:         0123abcd
\tBuilt:       
\tGo version:  go1.25.1
\tOS/Arch:     linux/amd64
\tEnabled:     RBAC
";

    #[test]
    fn parses_client_and_server_sections() {
        let version = parse_version(VERSION_OUTPUT);
        assert_eq!(
            version,
            TalosVersion {
                client: "v1.14.0".to_string(),
                server: Some("v1.14.0".to_string()),
                node: Some("10.0.0.5".to_string()),
                nodes: vec![NodeVersion {
                    node: "10.0.0.5".to_string(),
                    tag: "v1.14.0".to_string(),
                }],
            }
        );
    }

    #[test]
    fn keeps_every_node_of_a_multi_node_talosconfig() {
        let text = "Client:\n\tTag:         v1.14.0\nServer:\n\tNODE:        10.0.0.5\n\tTag:         v1.14.0\n\tNODE:        10.0.0.6\n\tTag:         v1.13.4\n";
        let version = parse_version(text);
        assert_eq!(version.server.as_deref(), Some("v1.14.0"));
        assert_eq!(version.nodes.len(), 2);
        assert_eq!(version.nodes[1].node, "10.0.0.6");
        assert_eq!(version.nodes[1].tag, "v1.13.4");
    }

    #[test]
    fn parses_client_only_output() {
        let version = parse_version("Client:\n\tTag:         v1.14.0\n");
        assert_eq!(version.client, "v1.14.0");
        assert_eq!(version.server, None);
    }

    #[test]
    fn parses_a_stream_of_resources() {
        let text = r#"{"node":"10.0.0.5","metadata":{"namespace":"runtime","type":"MachineStatuses.runtime.talos.dev","id":"machine","version":7,"owner":"runtime.MachineStatusController"},"spec":{"stage":"running","status":{"ready":true}}}
{"node":"10.0.0.5","metadata":{"namespace":"runtime","type":"SecurityStates.runtime.talos.dev","id":"securitystate","version":1},"spec":{"secureBoot":false}}
"#;
        let resources = parse_resources(text).expect("parse");
        assert_eq!(resources.len(), 2);
        assert_eq!(resources[0].id, "machine");
        assert_eq!(resources[0].version, Some(7));
        assert_eq!(resources[0].spec["stage"], "running");
        assert_eq!(resources[1].type_, "SecurityStates.runtime.talos.dev");
    }

    #[test]
    fn read_paths_are_absolute_and_plain() {
        assert!(validate_path("/sys/class/hwmon/hwmon0/temp1_input").is_ok());
        assert!(validate_path("/proc/uptime").is_ok());
        assert!(validate_path("sys/class").is_err());
        assert!(validate_path("/etc/../secret").is_err());
        assert!(validate_path("/tmp/a b").is_err());
        assert!(validate_path("--help").is_err());
    }

    #[test]
    fn rejects_suspicious_resource_names() {
        assert!(validate("machinestatus", "resource").is_ok());
        assert!(validate("SecurityStates.runtime.talos.dev", "resource").is_ok());
        assert!(validate("disks; rm -rf /", "resource").is_err());
        assert!(validate("", "resource").is_err());
        assert!(validate("--endpoints", "namespace").is_err());
    }

    #[test]
    fn drains_complete_watch_documents_and_keeps_the_partial_tail() {
        let mut buffer = String::from(
            "{\n  \"event\": \"created\",\n  \"metadata\": {\"namespace\": \"runtime\", \"type\": \"MachineStatuses.runtime.talos.dev\", \"id\": \"machine\", \"version\": 26},\n  \"node\": \"10.0.0.1\",\n  \"spec\": {\"stage\": \"running\"}\n}\n{\n  \"event\": \"updated\",\n  \"metadata\": {\"namespace\": \"runtime\", \"type\": \"Mach",
        );
        let events = drain_documents(&mut buffer);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "created");
        assert_eq!(events[0].resource.id, "machine");
        assert_eq!(events[0].resource.spec["stage"], "running");
        assert!(
            buffer
                .trim_start()
                .starts_with("{\n  \"event\": \"updated\""),
            "partial kept"
        );

        buffer.push_str("ineStatuses.runtime.talos.dev\", \"id\": \"machine\"}, \"spec\": {}}\n");
        let events = drain_documents(&mut buffer);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "updated");
        assert!(buffer.is_empty());
        assert!(drain_documents(&mut buffer).is_empty());
    }

    #[test]
    fn numeric_resource_ids_are_read_as_text() {
        let text = r#"{"node":"10.0.0.1","metadata":{"namespace":"runtime","type":"ExtensionStatuses.runtime.talos.dev","id":0,"version":1},"spec":{"metadata":{"name":"tailscale","version":"1.102.2"}}}"#;
        let resources = parse_resources(text).expect("parse");
        assert_eq!(resources[0].id, "0");
        assert_eq!(resources[0].spec["metadata"]["name"], "tailscale");
    }

    #[test]
    fn a_document_without_an_event_field_counts_as_updated() {
        let mut buffer = String::from(
            r#"{"metadata":{"namespace":"runtime","type":"Disks.block.talos.dev","id":"sda"},"spec":{}}"#,
        );
        let events = drain_documents(&mut buffer);
        assert_eq!(events[0].event, "updated");
    }

    #[tokio::test]
    async fn missing_talosconfig_is_not_connected() {
        let cli = TalosCli::new("talosctl", "/definitely/not/here");
        let error = cli.version().await.expect_err("missing");
        assert_eq!(error.code(), "NOT_CONNECTED");
    }
}
