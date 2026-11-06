//! Protocol-22 client for the local Herdr Unix-socket API.

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader},
    sync::{mpsc, watch},
    time::sleep,
};

use forge_app::{
    AgentAdapterError, AgentAssignmentResource, AgentDispatchAdapter, AgentDispatchContext,
    AgentDispatchRepository, AgentObservation, AgentProfile, NewAgentAssignmentResource,
};
use forge_store::SqliteProjectRepository;
use serde_json::{Value, json};

const PROTOCOL: u64 = 22;
const SCHEMA_VERSION: u64 = 1;
const PREFIX: &str = "forge-dispatch-";

/// The complete non-secret Pi launch contract. The bearer stays in the referenced file.
fn capability_launch_environment(
    assignment_id: &str,
    capability: &forge_app::AssignmentCapabilityDelivery,
) -> Value {
    json!({
        "FORGE_ASSIGNMENT_ID": assignment_id,
        "FORGE_ASSIGNMENT_DAEMON_URL": capability.endpoint,
        "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE": capability.secret_file,
        "FORGE_ASSIGNMENT_FORGE_CLI": capability.cli_path,
    })
}

/// Constructs Herdr adapters from the configured Unix socket endpoint.
pub struct HerdrAdapterFactory {
    socket_path: Option<PathBuf>,
    configuration_store: Option<PathBuf>,
}

impl HerdrAdapterFactory {
    /// Reads the endpoint for each adapter selection, so RPC endpoint updates take effect
    /// without restarting the daemon.
    #[must_use]
    pub fn from_configuration_store(path: PathBuf) -> Self {
        Self {
            socket_path: None,
            configuration_store: Some(path),
        }
    }

    pub fn adapter_for(&self, profile: &AgentProfile) -> Result<HerdrAdapter, AgentAdapterError> {
        if profile.provider != "herdr" || profile.agent_kind != "pi" {
            return Err(AgentAdapterError::Endpoint(
                "unsupported agent profile".into(),
            ));
        }
        let socket_path = match (&self.socket_path, &self.configuration_store) {
            (Some(path), _) => Some(path.clone()),
            (None, Some(store)) => SqliteProjectRepository::open(store)
                .and_then(|repository| repository.herdr_socket_path())
                .map_err(|error| {
                    AgentAdapterError::Endpoint(format!(
                        "could not read Herdr endpoint: {}",
                        error.code
                    ))
                })?,
            (None, None) => None,
        }
        .ok_or_else(|| {
            AgentAdapterError::Endpoint("Herdr Unix socket endpoint is not configured".into())
        })?;
        let store_root = self
            .configuration_store
            .as_deref()
            .and_then(Path::parent)
            .unwrap_or_else(|| Path::new("."));
        let data_root = if store_root.is_absolute() {
            store_root.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|_| {
                    AgentAdapterError::Endpoint("could not resolve Forge data root".into())
                })?
                .join(store_root)
        };
        Ok(HerdrAdapter {
            socket_path,
            dispatch_root: data_root.join("herdr-dispatch-workspace"),
        })
    }
}

/// A synchronous, one-request-per-connection Herdr protocol-22 client.
pub struct HerdrAdapter {
    socket_path: PathBuf,
    /// Neutral Forge-owned data root, never a Project or Task workspace.
    dispatch_root: PathBuf,
}

/// Runs the long-lived event side of Herdr observation. Events only enqueue a wake; the
/// server-owned reconciliation worker performs the authoritative `agent.get` polling.
pub(crate) async fn observe_status_changes(
    store_path: PathBuf,
    wake: mpsc::Sender<()>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        let Some((socket_path, panes)) = owned_panes(&store_path) else {
            if wait_or_shutdown(&mut shutdown, Duration::from_secs(1)).await {
                return;
            }
            continue;
        };
        if panes.is_empty() {
            if wait_or_shutdown(&mut shutdown, Duration::from_secs(1)).await {
                return;
            }
            continue;
        }
        if observe_connection(&socket_path, &panes, &wake, &mut shutdown)
            .await
            .is_err()
            && wait_or_shutdown(&mut shutdown, Duration::from_secs(1)).await
        {
            return;
        }
    }
}

async fn wait_or_shutdown(shutdown: &mut watch::Receiver<bool>, duration: Duration) -> bool {
    tokio::select! {
        () = sleep(duration) => false,
        changed = shutdown.changed() => changed.is_err() || *shutdown.borrow(),
    }
}

fn owned_panes(store_path: &std::path::Path) -> Option<(PathBuf, Vec<String>)> {
    let repository = SqliteProjectRepository::open(store_path).ok()?;
    let socket_path = repository.herdr_socket_path().ok()??;
    let mut panes = Vec::new();
    for assignment in repository.list_open_agent_assignments().ok()? {
        for resource in repository
            .list_agent_assignment_resources(&assignment.id)
            .ok()?
        {
            if resource.resource_kind == "pane" && resource.retired_at_millis.is_none() {
                panes.push(resource.resource_id);
            }
        }
    }
    panes.sort();
    panes.dedup();
    Some((socket_path, panes))
}

async fn observe_connection(
    socket_path: &std::path::Path,
    panes: &[String],
    wake: &mpsc::Sender<()>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<(), ()> {
    #[cfg(unix)]
    {
        let stream = tokio::net::UnixStream::connect(socket_path)
            .await
            .map_err(|_| ())?;
        let mut stream = AsyncBufReader::new(stream);
        let subscriptions: Vec<_> = panes
            .iter()
            .map(|pane_id| json!({"type":"pane.agent_status_changed", "pane_id": pane_id}))
            .collect();
        write_async_request(
            stream.get_mut(),
            "events.subscribe",
            json!({"subscriptions": subscriptions}),
        )
        .await?;
        let response = read_async_json(&mut stream).await?;
        if response.get("error").is_some() {
            return Err(());
        }
        // Herdr requires subscribe-before-snapshot. The snapshot is deliberately not used as
        // durable state; it establishes the reconnect ordering before authoritative polling.
        write_async_request(stream.get_mut(), "session.snapshot", json!({})).await?;
        let snapshot = read_async_json(&mut stream).await?;
        validate_snapshot(snapshot.get("result").unwrap_or(&snapshot))?;
        let _ = wake.try_send(());

        loop {
            let mut line = String::new();
            tokio::select! {
                read = stream.read_line(&mut line) => {
                    if read.map_err(|_| ())? == 0 { return Err(()); }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return Ok(()); }
                    continue;
                }
            }
            let event: Value = serde_json::from_str(&line).map_err(|_| ())?;
            if event_is_owned_status_change(&event, panes) {
                let _ = wake.try_send(());
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (socket_path, panes, wake, shutdown);
        Err(())
    }
}

async fn write_async_request(
    stream: &mut tokio::net::UnixStream,
    method: &str,
    params: Value,
) -> Result<(), ()> {
    let request = json!({"id":"forge-dispatch-observer", "method":method, "params":params});
    let encoded = serde_json::to_vec(&request).map_err(|_| ())?;
    stream.write_all(&encoded).await.map_err(|_| ())?;
    stream.write_all(b"\n").await.map_err(|_| ())?;
    stream.flush().await.map_err(|_| ())
}

async fn read_async_json(stream: &mut AsyncBufReader<tokio::net::UnixStream>) -> Result<Value, ()> {
    let mut line = String::new();
    if stream.read_line(&mut line).await.map_err(|_| ())? == 0 {
        return Err(());
    }
    serde_json::from_str(&line).map_err(|_| ())
}

fn validate_snapshot(snapshot: &Value) -> Result<(), ()> {
    let snapshot = snapshot.get("snapshot").unwrap_or(snapshot);
    (snapshot.get("protocol").and_then(Value::as_u64) == Some(PROTOCOL)
        && snapshot
            .get("schema_version")
            .or_else(|| snapshot.get("schemaVersion"))
            .is_none_or(|version| version.as_u64() == Some(SCHEMA_VERSION))
        && snapshot
            .get("workspaces")
            .and_then(Value::as_array)
            .is_some())
    .then_some(())
    .ok_or(())
}

fn event_is_owned_status_change(event: &Value, panes: &[String]) -> bool {
    let envelope = event
        .get("params")
        .or_else(|| event.get("event"))
        .or_else(|| event.get("payload"))
        .unwrap_or(event);
    let payload = envelope
        .get("event")
        .or_else(|| envelope.get("payload"))
        .unwrap_or(envelope);
    let event_type = payload
        .get("type")
        .or_else(|| envelope.get("type"))
        .or_else(|| event.get("type"))
        .and_then(Value::as_str);
    event_type == Some("pane.agent_status_changed")
        && payload
            .get("pane_id")
            .or_else(|| payload.pointer("/pane/pane_id"))
            .and_then(Value::as_str)
            .is_some_and(|pane_id| panes.iter().any(|owned| owned == pane_id))
}

impl HerdrAdapter {
    pub(crate) fn default_agent_workspace(&self) -> &Path {
        &self.dispatch_root
    }

    #[allow(clippy::needless_pass_by_value)] // Required by the non-Unix fallback's ownership shape.
    fn request(&self, method: &str, params: Value) -> Result<Value, AgentAdapterError> {
        #[cfg(unix)]
        {
            use std::os::unix::net::UnixStream;
            let mut stream = UnixStream::connect(&self.socket_path).map_err(|_| {
                AgentAdapterError::Endpoint("could not connect to Herdr Unix socket".into())
            })?;
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| {
                    AgentAdapterError::Transient("could not configure Herdr read timeout".into())
                })?;
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|_| {
                    AgentAdapterError::Transient("could not configure Herdr write timeout".into())
                })?;
            let request = json!({"id": "forge-dispatch", "method": method, "params": params});
            serde_json::to_writer(&mut stream, &request).map_err(|_| {
                AgentAdapterError::Transient("could not encode Herdr request".into())
            })?;
            stream.write_all(b"\n").map_err(|_| {
                AgentAdapterError::Transient("could not write Herdr request".into())
            })?;
            stream.flush().map_err(|_| {
                AgentAdapterError::Transient("could not flush Herdr request".into())
            })?;
            let mut line = String::new();
            BufReader::new(stream).read_line(&mut line).map_err(|_| {
                AgentAdapterError::Transient("could not read Herdr response".into())
            })?;
            let response: Value = serde_json::from_str(&line)
                .map_err(|_| AgentAdapterError::Endpoint("malformed Herdr response".into()))?;
            if let Some(error) = response.get("error") {
                let code = error
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                return Err(match code {
                    "not_found" | "not-found" => {
                        AgentAdapterError::NotFound(format!("Herdr {method}: {code}"))
                    }
                    // Protocol 22 uses resource-specific codes. Do not treat arbitrary
                    // *_not_found errors (notably workspace loss) as verified absence.
                    "agent_not_found" if method == "agent.get" => {
                        AgentAdapterError::NotFound(format!("Herdr {method}: {code}"))
                    }
                    "tab_not_found" if matches!(method, "tab.get" | "tab.close") => {
                        AgentAdapterError::NotFound(format!("Herdr {method}: {code}"))
                    }
                    "pane_not_found" if matches!(method, "pane.get" | "pane.release_agent") => {
                        AgentAdapterError::NotFound(format!("Herdr {method}: {code}"))
                    }
                    "conflict" | "already_exists" | "already-exists" => {
                        AgentAdapterError::Conflict(format!("Herdr {method}: {code}"))
                    }
                    "unavailable" | "timeout" | "temporarily_unavailable" => {
                        AgentAdapterError::Transient(format!("Herdr {method}: {code}"))
                    }
                    "agentblocked" | "agent_blocked" => {
                        AgentAdapterError::InvalidRequest("agentblocked".into())
                    }
                    _ => AgentAdapterError::InvalidRequest(format!("Herdr {method}: {code}")),
                });
            }
            response
                .get("result")
                .cloned()
                .ok_or_else(|| AgentAdapterError::Endpoint("Herdr response has no result".into()))
        }
        #[cfg(not(unix))]
        {
            let _ = (method, params);
            Err(AgentAdapterError::Endpoint(
                "Herdr Unix sockets require Unix".into(),
            ))
        }
    }

    fn snapshot(&self) -> Result<Value, AgentAdapterError> {
        let response = self.request("session.snapshot", json!({}))?;
        let snapshot = response.get("snapshot").unwrap_or(&response);
        let protocol = snapshot.get("protocol").and_then(Value::as_u64);
        let schema = snapshot
            .get("schema_version")
            .or_else(|| snapshot.get("schemaVersion"))
            .and_then(Value::as_u64);
        if protocol != Some(PROTOCOL)
            || schema.is_some_and(|version| version != SCHEMA_VERSION)
            || snapshot
                .get("workspaces")
                .and_then(Value::as_array)
                .is_none()
        {
            return Err(AgentAdapterError::Endpoint(
                "Herdr protocol/schema is incompatible with protocol 22/schema 1".into(),
            ));
        }
        Ok(snapshot.clone())
    }

    fn label(context: &AgentDispatchContext) -> Result<String, AgentAdapterError> {
        let id = &context.assignment.id;
        if id.is_empty()
            || !id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        {
            return Err(AgentAdapterError::InvalidRequest(
                "assignment ID cannot safely name a Herdr resource".into(),
            ));
        }
        let label = format!("{PREFIX}{id}");
        if label.len() > 32 {
            return Err(AgentAdapterError::InvalidRequest(
                "assignment ID is too long for a Herdr agent name".into(),
            ));
        }
        Ok(label)
    }

    fn active<'a>(resources: &'a [AgentAssignmentResource], kind: &str) -> Option<&'a str> {
        resources
            .iter()
            .find(|resource| resource.resource_kind == kind && resource.retired_at_millis.is_none())
            .map(|resource| resource.resource_id.as_str())
    }

    fn retain(
        resources: &dyn AgentDispatchRepository,
        context: &AgentDispatchContext,
        kind: &str,
        id: &str,
    ) -> Result<(), AgentAdapterError> {
        resources
            .add_agent_assignment_resource(&NewAgentAssignmentResource {
                assignment_id: context.assignment.id.clone(),
                resource_kind: kind.into(),
                resource_id: id.into(),
            })
            .map(|_| ())
            .map_err(|error| {
                AgentAdapterError::Transient(format!("could not retain {kind}: {}", error.code))
            })
    }

    fn dispatch_workspace(
        &self,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<String, AgentAdapterError> {
        if let Some(workspace) = resources.dispatch_workspace().map_err(|error| {
            AgentAdapterError::Transient(format!(
                "could not read dispatch workspace: {}",
                error.code
            ))
        })? {
            let found = self.request("workspace.get", json!({"workspace_id": workspace.id}))?;
            Self::matching_resource(&found, "workspace", &[("workspace_id", &workspace.id)])?;
            return Ok(workspace.id);
        }
        Err(AgentAdapterError::Conflict(
            "no operator-selected Herdr workspace; dispatch is blocked".into(),
        ))
    }

    fn dispatch_workspace_from_context(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<String, AgentAdapterError> {
        let workspace = context.dispatch_workspace.as_ref().ok_or_else(|| {
            AgentAdapterError::Conflict("Forge Dispatch Workspace identity was not retained".into())
        })?;
        let found = self.request("workspace.get", json!({"workspace_id": workspace.id}))?;
        Self::matching_resource(&found, "workspace", &[("workspace_id", &workspace.id)])?;
        Ok(workspace.id.clone())
    }

    fn root_pane(&self, workspace: &str, tab: &str) -> Result<String, AgentAdapterError> {
        let result = self.request("pane.list", json!({"workspace_id": workspace}))?;
        result
            .get("panes")
            .and_then(Value::as_array)
            .and_then(|panes| {
                panes
                    .iter()
                    .find(|pane| pane.get("tab_id").and_then(Value::as_str) == Some(tab))
            })
            .and_then(|pane| pane.get("pane_id").and_then(Value::as_str))
            .map(str::to_owned)
            .ok_or_else(|| {
                AgentAdapterError::NotFound("no root pane found in persisted dedicated tab".into())
            })
    }

    /// Parses an authoritative `AgentInfo` response. Readiness and sequence fields are
    /// deliberately not lifecycle inputs: only the protocol status is normalized.
    fn observation_from_agent(
        agent: &Value,
        tuple: (&str, &str, &str),
    ) -> Result<AgentObservation, AgentAdapterError> {
        if agent.get("workspace_id").and_then(Value::as_str) != Some(tuple.0)
            || agent.get("tab_id").and_then(Value::as_str) != Some(tuple.1)
            || agent.get("pane_id").and_then(Value::as_str) != Some(tuple.2)
        {
            return Err(AgentAdapterError::Conflict(
                "Herdr agent does not match the persisted resource tuple".into(),
            ));
        }
        match agent.get("agent_status").and_then(Value::as_str) {
            Some("working") => Ok(AgentObservation::Working),
            Some("idle") => Ok(AgentObservation::Idle),
            Some("blocked") => Ok(AgentObservation::Blocked),
            Some("done") => Ok(AgentObservation::Done),
            Some("unknown") => Ok(AgentObservation::Unknown),
            _ => Err(AgentAdapterError::Endpoint(
                "Herdr agent response has an invalid status".into(),
            )),
        }
    }

    fn agent_observation(
        &self,
        name: &str,
        tuple: (&str, &str, &str),
    ) -> Result<AgentObservation, AgentAdapterError> {
        match self.request("agent.get", json!({"target": name})) {
            Ok(result) => {
                // agent.get returns AgentInfo inside an agent_info envelope; list entries
                // are already AgentInfo. Decode here, not in the shared tuple validator.
                if result.get("type").and_then(Value::as_str) != Some("agent_info") {
                    return Err(AgentAdapterError::Endpoint(
                        "Herdr agent.get has an invalid response type".into(),
                    ));
                }
                let agent = result
                    .get("agent")
                    .filter(|agent| agent.is_object())
                    .ok_or_else(|| {
                        AgentAdapterError::Endpoint("Herdr agent.get has no agent object".into())
                    })?;
                if agent.get("name").and_then(Value::as_str) != Some(name) {
                    return Err(AgentAdapterError::Conflict(
                        "Herdr agent does not match the persisted name".into(),
                    ));
                }
                Self::observation_from_agent(agent, tuple)
            }
            Err(AgentAdapterError::NotFound(_)) => self.agent_list_fallback(name, tuple),
            Err(error) => Err(error),
        }
    }

    fn cleanup_request(&self, method: &str, params: Value) -> Result<(), AgentAdapterError> {
        match self.request(method, params) {
            Ok(_) | Err(AgentAdapterError::NotFound(_)) => Ok(()),
            Err(AgentAdapterError::Conflict(message)) => Err(AgentAdapterError::Conflict(message)),
            Err(error) => Err(AgentAdapterError::Cleanup(format!("{method}: {error:?}"))),
        }
    }

    fn retire_resource(
        resources: &dyn AgentDispatchRepository,
        context: &AgentDispatchContext,
        kind: &str,
        id: &str,
    ) -> Result<(), AgentAdapterError> {
        resources
            .retire_agent_assignment_resource(&context.assignment.id, kind, id)
            .map(|_| ())
            .map_err(|error| {
                AgentAdapterError::Cleanup(format!(
                    "could not retain retired {kind}: {}",
                    error.code
                ))
            })
    }

    fn resource_is_active(context: &AgentDispatchContext, kind: &str) -> Option<String> {
        Self::active(&context.resources, kind).map(str::to_owned)
    }

    fn matching_resource(
        value: &Value,
        wrapper: &str,
        expected: &[(&str, &str)],
    ) -> Result<(), AgentAdapterError> {
        let value = value.get(wrapper).unwrap_or(value);
        if expected
            .iter()
            .all(|(field, expected)| value.get(*field).and_then(Value::as_str) == Some(*expected))
        {
            Ok(())
        } else {
            Err(AgentAdapterError::Conflict(
                "Herdr resource does not match the persisted resource tuple".into(),
            ))
        }
    }

    /// `agent.get` is authoritative. A single list after a not-found response distinguishes
    /// an absent agent from a renamed/replaced or misplaced one without adopting anything.
    fn agent_list_fallback(
        &self,
        name: &str,
        tuple: (&str, &str, &str),
    ) -> Result<AgentObservation, AgentAdapterError> {
        let result = self.request("agent.list", json!({}))?;
        if result.get("type").and_then(Value::as_str) != Some("agent_list") {
            return Err(AgentAdapterError::Endpoint(
                "Herdr agent.list has an invalid response type".into(),
            ));
        }
        let agents = result
            .get("agents")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentAdapterError::Endpoint("Herdr agent.list has no agents".into()))?;
        let mut observation = None;
        for agent in agents {
            // An incomplete list entry cannot establish that the persisted agent/pane is
            // absent. Names are optional in AgentInfo, but resource identities are not.
            if ["workspace_id", "tab_id", "pane_id"].iter().any(|field| {
                agent
                    .get(*field)
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
            }) {
                return Err(AgentAdapterError::Endpoint(
                    "Herdr agent.list has an incomplete resource tuple".into(),
                ));
            }
            if agent.get("name").and_then(Value::as_str) == Some(name) {
                if observation.is_some() {
                    return Err(AgentAdapterError::Conflict(
                        "Herdr agent.list has duplicate persisted names".into(),
                    ));
                }
                observation = Some(Self::observation_from_agent(agent, tuple)?);
            } else if agent.get("pane_id").and_then(Value::as_str) == Some(tuple.2) {
                return Err(AgentAdapterError::Conflict(
                    "Herdr persisted pane is occupied by a different agent".into(),
                ));
            }
        }
        Ok(observation.unwrap_or(AgentObservation::Absent))
    }
}

impl AgentDispatchAdapter for HerdrAdapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        self.request("ping", json!({}))?;
        self.snapshot().map(|_| ())
    }

    fn dispatch_available(&self, resources: &dyn AgentDispatchRepository) -> bool {
        self.dispatch_workspace(resources).is_ok()
    }

    fn verify_workspace(
        &self,
        id: &str,
    ) -> Result<forge_app::DispatchWorkspace, AgentAdapterError> {
        let result = self.request("workspace.get", json!({"workspace_id": id}))?;
        let workspace = result.get("workspace").unwrap_or(&result);
        Self::matching_resource(workspace, "workspace", &[("workspace_id", id)])?;
        Ok(forge_app::DispatchWorkspace { id: id.into() })
    }

    fn provision(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        let bootstrap = context
            .bootstrap_prompt
            .as_deref()
            .filter(|prompt| !prompt.trim().is_empty())
            .ok_or_else(|| {
                AgentAdapterError::InvalidRequest(
                    "verified agent bootstrap is not configured".into(),
                )
            })?;
        if context.capability_delivery.is_none() {
            return Err(AgentAdapterError::InvalidRequest(
                "Assignment Capability delivery is not configured".into(),
            ));
        }
        self.probe()?;
        let label = Self::label(context)?;
        let workspace = self.dispatch_workspace(resources)?;
        if !resources
            .begin_agent_provision(&context.assignment.id)
            .map_err(|error| {
                AgentAdapterError::Transient(format!(
                    "could not record provisioning intent: {}",
                    error.code
                ))
            })?
        {
            return Err(AgentAdapterError::Conflict(
                "provisioning was already attempted; investigate retained resources before recovery".into(),
            ));
        }
        let agent_workspace = context
            .agent_workspace
            .as_deref()
            .unwrap_or(&self.dispatch_root);
        let (tab, root) = if let Some(id) = Self::active(&context.resources, "tab") {
            (id.to_owned(), None)
        } else {
            let mut tab_request = json!({"workspace_id": workspace, "label": label, "cwd": agent_workspace, "focus": false});
            if let Some(capability) = &context.capability_delivery {
                tab_request["env"] =
                    capability_launch_environment(&context.assignment.id, capability);
            }
            let result = self.request("tab.create", tab_request)?;
            let id = result
                .pointer("/tab/tab_id")
                .or_else(|| result.get("tab_id"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AgentAdapterError::Endpoint("tab.create did not return tab_id".into())
                })?
                .to_owned();
            let root = result
                .pointer("/root_pane/pane_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Self::retain(resources, context, "tab", &id)?;
            (id, root)
        };
        let pane = if let Some(id) = Self::active(&context.resources, "pane") {
            id.to_owned()
        } else {
            let pane = match root {
                Some(root) => root,
                None => self.root_pane(&workspace, &tab)?,
            };
            Self::retain(resources, context, "pane", &pane)?;
            pane
        };
        if Self::active(&context.resources, "agent-name").is_none() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| {
                    AgentAdapterError::Transient("system clock is before Unix epoch".into())
                })?
                .as_millis()
                .try_into()
                .map_err(|_| AgentAdapterError::Transient("system clock is out of range".into()))?;
            if !resources
                .begin_agent_launch(&context.assignment.id, now)
                .map_err(|_| {
                    AgentAdapterError::Transient("could not record launch intent".into())
                })?
            {
                return Err(AgentAdapterError::Conflict(
                    "launch was already attempted or Assignment is no longer provisioning".into(),
                ));
            }
            self.request("agent.start", json!({"name": label, "kind": "pi", "pane_id": pane, "args": ["--model", context.assignment.profile.model, bootstrap], "timeout_ms": context.assignment.profile.startup_timeout_millis}))?;
            Self::retain(resources, context, "agent-name", &label)?;
        }
        Ok(())
    }

    fn observe(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        let workspace = self.dispatch_workspace_from_context(context)?;
        let (Some(tab), Some(pane), Some(agent)) = (
            Self::active(&context.resources, "tab"),
            Self::active(&context.resources, "pane"),
            Self::active(&context.resources, "agent-name"),
        ) else {
            return Ok(AgentObservation::Absent);
        };
        self.agent_observation(agent, (&workspace, tab, pane))
    }

    fn recover(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        self.observe(context)
    }

    fn retire(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        let workspace = self.dispatch_workspace_from_context(context)?;
        let tab = Self::resource_is_active(context, "tab");
        let pane = Self::resource_is_active(context, "pane");
        let agent = Self::resource_is_active(context, "agent-name");

        // Re-read every still-present resource before a destructive request. Persisted IDs,
        // rather than labels, are the only cleanup authority.
        let mut tab_present = false;
        if let Some(tab) = &tab {
            match self.request("tab.get", json!({"tab_id": tab})) {
                Ok(found) => {
                    Self::matching_resource(
                        &found,
                        "tab",
                        &[("tab_id", tab), ("workspace_id", &workspace)],
                    )?;
                    tab_present = true;
                }
                Err(AgentAdapterError::NotFound(_)) => {}
                Err(error) => {
                    return Err(AgentAdapterError::Cleanup(format!("tab.get: {error:?}")));
                }
            }
        }
        let mut pane_present = false;
        if let Some(pane) = &pane {
            match self.request("pane.get", json!({"pane_id": pane})) {
                Ok(found) => {
                    Self::matching_resource(
                        &found,
                        "pane",
                        &[
                            ("pane_id", pane),
                            ("workspace_id", &workspace),
                            ("tab_id", tab.as_deref().unwrap_or("")),
                        ],
                    )?;
                    pane_present = true;
                }
                Err(AgentAdapterError::NotFound(_)) => {}
                Err(error) => {
                    return Err(AgentAdapterError::Cleanup(format!("pane.get: {error:?}")));
                }
            }
        }

        if pane_present && !tab_present {
            return Err(AgentAdapterError::Conflict(
                "Herdr dedicated pane exists without its verified dedicated tab".into(),
            ));
        }

        if let (Some(pane), Some(agent)) = (&pane, &agent)
            && pane_present
        {
            if self.agent_observation(agent, (&workspace, tab.as_deref().unwrap_or(""), pane))?
                != AgentObservation::Absent
            {
                self.cleanup_request(
                    "pane.release_agent",
                    json!({"pane_id": pane, "source": "forge-agent-dispatch", "agent": agent, "seq": 1}),
                )?;
            }
        }
        // The pane may be the tab's root; closing the verified dedicated tab removes
        // its panes as well and avoids trying to close a tab's last pane separately.
        if let Some(tab) = &tab
            && tab_present
        {
            self.cleanup_request("tab.close", json!({"tab_id": tab}))?;
        }
        // Mutation admission is not proof of removal. Verify all retained identities before
        // releasing their capacity; a no-op, stale observation or endpoint error is unsafe.
        for (kind, id) in [("tab", tab.as_ref()), ("pane", pane.as_ref())] {
            if let Some(id) = id {
                match self.request(&format!("{kind}.get"), json!({format!("{kind}_id"): id})) {
                    Err(AgentAdapterError::NotFound(_)) => {}
                    Ok(_) => {
                        return Err(AgentAdapterError::Cleanup(format!(
                            "{kind} remains after cleanup"
                        )));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        if let Some(agent) = &agent {
            if self.agent_observation(
                agent,
                (
                    &workspace,
                    tab.as_deref().unwrap_or(""),
                    pane.as_deref().unwrap_or(""),
                ),
            )? != AgentObservation::Absent
            {
                return Err(AgentAdapterError::Cleanup(
                    "agent remains after cleanup".into(),
                ));
            }
        }
        for (kind, id) in [("tab", tab), ("pane", pane), ("agent-name", agent)] {
            if let Some(id) = id {
                Self::retire_resource(resources, context, kind, &id)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use forge_app::AgentDispatchAdapter;

    #[test]
    fn capability_delivery_is_exactly_the_non_secret_pi_launch_contract() {
        let delivery = forge_app::AssignmentCapabilityDelivery {
            endpoint: "http://127.0.0.1:45003".into(),
            secret_file: std::path::PathBuf::from("/forge-data/assignment-capabilities/a-1"),
            cli_path: std::path::PathBuf::from("/forge-bin/forge"),
        };
        assert_eq!(
            super::capability_launch_environment("a-1", &delivery),
            serde_json::json!({
                "FORGE_ASSIGNMENT_ID": "a-1",
                "FORGE_ASSIGNMENT_DAEMON_URL": "http://127.0.0.1:45003",
                "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE": "/forge-data/assignment-capabilities/a-1",
                "FORGE_ASSIGNMENT_FORGE_CLI": "/forge-bin/forge",
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn provisioning_refuses_to_launch_without_a_bootstrap() {
        use forge_app::{
            AgentDispatchRepository, NewAgentAssignment, NewTypedRecord, Project,
            ProjectRepository, RecordRepository, RecordType,
        };
        let store = forge_store::SqliteProjectRepository::in_memory().unwrap();
        store
            .register(&Project {
                slug: "alpha".into(),
                registered_folder: "/project".into(),
                origin_url: "https://example.test/repo".into(),
            })
            .unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "task".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let assignment = store
            .create_agent_assignment(&NewAgentAssignment {
                id: "bootstrap-test".into(),
                record_id: task.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        let context = forge_app::AgentDispatchContext {
            assignment,
            registered_project_folder: "/project".into(),
            agent_workspace: None,
            dispatch_workspace: None,
            rendered_initial_prompt: "full instructions".into(),
            bootstrap_prompt: None,
            capability_delivery: None,
            resources: vec![],
        };
        let adapter = super::HerdrAdapter {
            socket_path: "/nonexistent-herdr-socket".into(),
            dispatch_root: "/tmp".into(),
        };
        assert!(matches!(
            adapter.provision(&context, &store),
            Err(forge_app::AgentAdapterError::InvalidRequest(_))
        ));
        assert_eq!(store.agent_launch_intent("bootstrap-test").unwrap(), None);
    }

    #[cfg(unix)]
    #[test]
    fn probe_uses_protocol_22_socket_requests_and_validates_the_snapshot() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
            thread,
        };

        let path = std::env::temp_dir().join(format!(
            "forge-herdr-double-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            for result in [
                serde_json::json!({"id":"forge-dispatch","result":{"type":"pong","protocol":22,"version":"test"}}),
                serde_json::json!({"id":"forge-dispatch","result":{"type":"session_snapshot","snapshot":{"protocol":22,"schema_version":1,"workspaces":[]}}}),
            ] {
                let mut stream = listener.accept().unwrap().0;
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert!(matches!(
                    request["method"].as_str(),
                    Some("ping" | "session.snapshot")
                ));
                serde_json::to_writer(&mut stream, &result).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        let adapter = super::HerdrAdapter {
            socket_path: path.clone(),
            dispatch_root: std::env::temp_dir().join("forge-dispatch-workspace"),
        };
        adapter.probe().unwrap();
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn provisioning_launches_only_short_bootstrap_and_delivers_secret_path() {
        use forge_app::{
            AgentDispatchContext, AgentDispatchRepository, AssignmentCapabilityDelivery,
            DispatchWorkspace, NewAgentAssignment, NewTypedRecord, Project, ProjectRepository,
            RecordRepository, RecordType,
        };
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
            thread,
        };
        let repository = forge_store::SqliteProjectRepository::in_memory().unwrap();
        repository
            .register(&Project {
                slug: "project".into(),
                registered_folder: "/project".into(),
                origin_url: "https://example.test/project".into(),
            })
            .unwrap();
        let task = repository
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "private full instructions".into(),
                project_slugs: vec!["project".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let assignment = repository
            .create_agent_assignment(&NewAgentAssignment {
                id: "assignment".into(),
                record_id: task.id,
                project_slug: "project".into(),
                profile_revision: 1,
            })
            .unwrap();
        repository
            .retain_dispatch_workspace(&DispatchWorkspace {
                id: "workspace".into(),
            })
            .unwrap();
        let context = AgentDispatchContext {
            assignment: assignment.clone(),
            registered_project_folder: "/project".into(),
            agent_workspace: Some("/worktree".into()),
            dispatch_workspace: Some(DispatchWorkspace {
                id: "workspace".into(),
            }),
            rendered_initial_prompt: "private full instructions".into(),
            bootstrap_prompt: Some("short bootstrap".into()),
            capability_delivery: Some(AssignmentCapabilityDelivery {
                endpoint: "http://127.0.0.1:45003".into(),
                secret_file: "/secrets/assignment".into(),
                cli_path: "/bin/forge".into(),
            }),
            resources: vec![],
        };
        let path = std::env::temp_dir().join(format!(
            "forge-launch-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let model = assignment.profile.model.clone();
        let server = thread::spawn(move || {
            for (method, response) in [
                (
                    "ping",
                    serde_json::json!({"type":"pong","protocol":22,"version":"test"}),
                ),
                (
                    "session.snapshot",
                    serde_json::json!({"type":"session_snapshot","snapshot":{"protocol":22,"schema_version":1,"workspaces":[]}}),
                ),
                (
                    "workspace.get",
                    serde_json::json!({"workspace":{"workspace_id":"workspace"}}),
                ),
                (
                    "tab.create",
                    serde_json::json!({"tab":{"tab_id":"tab"},"root_pane":{"pane_id":"pane"}}),
                ),
                ("agent.start", serde_json::json!({"type":"agent_started"})),
            ] {
                let mut stream = listener.accept().unwrap().0;
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], method);
                if method == "tab.create" {
                    assert_eq!(
                        request["params"]["env"]["FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE"],
                        "/secrets/assignment"
                    );
                }
                if method == "agent.start" {
                    assert_eq!(
                        request["params"]["args"],
                        serde_json::json!(["--model", model, "short bootstrap"])
                    );
                    assert!(!line.contains("private full instructions"));
                    assert!(!line.contains("/secrets/assignment"));
                }
                serde_json::to_writer(
                    &mut stream,
                    &serde_json::json!({"id":"forge-dispatch","result":response}),
                )
                .unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        let adapter = super::HerdrAdapter {
            socket_path: path.clone(),
            dispatch_root: "/worktree".into(),
        };
        adapter.provision(&context, &repository).unwrap();
        assert!(
            repository
                .agent_launch_intent(&assignment.id)
                .unwrap()
                .is_some()
        );
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }

    /// Socket seam with protocol-22 response envelopes (not flattened `AgentInfo` doubles).
    #[cfg(unix)]
    fn observe_transcript(
        transcript: Vec<(&'static str, serde_json::Value)>,
    ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
            thread,
        };
        let path = std::env::temp_dir().join(format!(
            "forge-herdr-agent-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            for (method, response) in transcript {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(std::time::Instant::now() < deadline, "missing {method}");
                            thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], method);
                if method == "agent.get" {
                    assert_eq!(request["params"]["target"], "forge-dispatch-a");
                }
                serde_json::to_writer(&mut stream, &response).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        let adapter = super::HerdrAdapter {
            socket_path: path.clone(),
            dispatch_root: "/unused".into(),
        };
        let result = adapter.agent_observation("forge-dispatch-a", ("workspace", "tab", "pane"));
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
        result
    }

    #[cfg(unix)]
    #[test]
    fn observation_decodes_nested_agent_info_and_validates_identity() {
        use forge_app::{AgentAdapterError, AgentObservation};
        let agent = serde_json::json!({
            "name":"forge-dispatch-a", "workspace_id":"workspace", "tab_id":"tab",
            "pane_id":"pane", "agent_status":"working", "terminal_id":"terminal",
            "focused":false, "revision":1, "interactive_ready":false, "state_change_seq":7
        });
        for (status, expected) in [
            ("working", AgentObservation::Working),
            ("idle", AgentObservation::Idle),
            ("blocked", AgentObservation::Blocked),
            ("done", AgentObservation::Done),
            ("unknown", AgentObservation::Unknown),
        ] {
            let mut info = agent.clone();
            info["agent_status"] = status.into();
            assert_eq!(
                observe_transcript(vec![(
                    "agent.get",
                    serde_json::json!({
                        "result":{"type":"agent_info", "agent":info}
                    })
                )])
                .unwrap(),
                expected
            );
        }
        for field in ["workspace_id", "tab_id", "pane_id", "name"] {
            let mut info = agent.clone();
            info[field] = "foreign".into();
            assert!(
                matches!(
                    observe_transcript(vec![(
                        "agent.get",
                        serde_json::json!({
                            "result":{"type":"agent_info", "agent":info}
                        })
                    )]),
                    Err(AgentAdapterError::Conflict(_))
                ),
                "{field}"
            );
        }
        for info in [
            serde_json::Value::Null,
            serde_json::json!({}),
            agent.clone(),
        ] {
            // A flattened object is not the protocol-22 envelope, even with a valid tuple.
            assert!(
                observe_transcript(vec![("agent.get", serde_json::json!({"result":info}))])
                    .is_err()
            );
        }
        for kind in [serde_json::Value::Null, serde_json::json!("agent_started")] {
            assert!(
                observe_transcript(vec![(
                    "agent.get",
                    serde_json::json!({
                        "result":{"type":kind, "agent":agent}
                    })
                )])
                .is_err()
            );
        }
        let mut invalid = agent;
        invalid["agent_status"] = "absent".into();
        assert!(matches!(
            observe_transcript(vec![(
                "agent.get",
                serde_json::json!({
                    "result":{"type":"agent_info", "agent":invalid}
                })
            )]),
            Err(AgentAdapterError::Endpoint(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn list_fallback_does_not_adopt_replacements_or_infer_absence_from_malformed_data() {
        use forge_app::AgentObservation;
        let agent = serde_json::json!({
            "name":"forge-dispatch-a", "workspace_id":"workspace", "tab_id":"tab",
            "pane_id":"pane", "agent_status":"working"
        });
        let mut renamed = agent.clone();
        renamed["name"] = "replacement".into();
        let mut misplaced = agent.clone();
        misplaced["pane_id"] = "foreign-pane".into();
        let mut unnamed = agent.clone();
        unnamed.as_object_mut().unwrap().remove("name");
        for entries in [
            serde_json::json!([renamed]),
            serde_json::json!([misplaced]),
            serde_json::json!([unnamed]),
            serde_json::json!([{}]),
            serde_json::json!([agent, agent]),
            serde_json::Value::Null,
        ] {
            assert!(
                observe_transcript(vec![
                    (
                        "agent.get",
                        serde_json::json!({"error":{"code":"agent_not_found"}})
                    ),
                    (
                        "agent.list",
                        serde_json::json!({"result":{"type":"agent_list", "agents":entries}})
                    ),
                ])
                .is_err()
            );
        }
        assert_eq!(
            observe_transcript(vec![
                (
                    "agent.get",
                    serde_json::json!({"error":{"code":"agent_not_found"}})
                ),
                (
                    "agent.list",
                    serde_json::json!({"result":{"type":"agent_list", "agents":[]}})
                ),
            ])
            .unwrap(),
            AgentObservation::Absent
        );
        assert!(
            observe_transcript(vec![
                (
                    "agent.get",
                    serde_json::json!({"error":{"code":"agent_not_found"}})
                ),
                (
                    "agent.list",
                    serde_json::json!({"result":{"type":"wrong", "agents":[]}})
                ),
            ])
            .is_err()
        );
        // Workspace/provider failures must not trigger the absence fallback.
        for code in ["workspace_not_found", "internal_error", "timeout"] {
            assert!(
                observe_transcript(vec![(
                    "agent.get",
                    serde_json::json!({"error":{"code":code}})
                )])
                .is_err()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn observation_uses_one_list_fallback_after_get_not_found() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
            thread,
        };

        let path = std::env::temp_dir().join(format!(
            "forge-herdr-observe-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            for (method, response) in [
                (
                    "agent.get",
                    serde_json::json!({"error":{"code":"agent_not_found","message":"agent target not found"}}),
                ),
                (
                    "agent.list",
                    serde_json::json!({"result":{"type":"agent_list","agents":[{
                        "name":"forge-dispatch-a", "workspace_id":"workspace",
                        "tab_id":"tab", "pane_id":"pane", "agent_status":"working",
                        "interactive_ready":false, "state_change_seq":7
                    }]}}),
                ),
            ] {
                let mut stream = listener.accept().unwrap().0;
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], method);
                serde_json::to_writer(&mut stream, &response).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        let adapter = super::HerdrAdapter {
            socket_path: path.clone(),
            dispatch_root: std::env::temp_dir().join("forge-dispatch-workspace"),
        };
        assert_eq!(
            adapter
                .agent_observation("forge-dispatch-a", ("workspace", "tab", "pane"))
                .unwrap(),
            forge_app::AgentObservation::Working
        );
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    #[allow(clippy::too_many_lines)] // The full request/response transcript documents one lifecycle.
    fn retirement_releases_and_closes_only_the_verified_dedicated_resources() {
        for outcome in [
            "absent",
            "tab-live",
            "pane-live",
            "agent-live",
            "provider-error",
            "tuple-mismatch",
            "agent-tuple-mismatch",
            "agent-renamed",
            "agent-malformed",
            "agent-replaced",
            "list-malformed",
            "workspace-missing",
            "cleanup-retry",
        ] {
            retirement_provider_outcome(outcome);
        }
    }

    #[cfg(unix)]
    #[allow(clippy::too_many_lines)]
    fn retirement_provider_outcome(outcome: &'static str) {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
            thread,
        };

        use forge_app::{
            AgentDispatchContext, AgentDispatchRepository, NewAgentAssignment,
            NewAgentAssignmentResource, NewTypedRecord, Project, ProjectRepository,
            RecordRepository, RecordType,
        };
        use forge_store::SqliteProjectRepository;

        let repository = SqliteProjectRepository::in_memory().unwrap();
        repository
            .register(&Project {
                slug: "project".into(),
                registered_folder: "/project".into(),
                origin_url: "https://example.test/project.git".into(),
            })
            .unwrap();
        let record = repository
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "dispatch task".into(),
                project_slugs: vec!["project".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let assignment = repository
            .create_agent_assignment(&NewAgentAssignment {
                id: "assignment".into(),
                record_id: record.id,
                project_slug: "project".into(),
                profile_revision: 1,
            })
            .unwrap();
        for (kind, id) in [
            ("tab", "tab"),
            ("pane", "pane"),
            ("agent-name", "forge-dispatch-assignment"),
        ] {
            repository
                .add_agent_assignment_resource(&NewAgentAssignmentResource {
                    assignment_id: assignment.id.clone(),
                    resource_kind: kind.into(),
                    resource_id: id.into(),
                })
                .unwrap();
        }
        repository
            .retain_dispatch_workspace(&forge_app::DispatchWorkspace {
                id: "workspace".into(),
            })
            .unwrap();
        let context = AgentDispatchContext {
            assignment: assignment.clone(),
            registered_project_folder: "/project".into(),
            agent_workspace: Some("/worktree".into()),
            rendered_initial_prompt: String::new(),
            bootstrap_prompt: None,
            capability_delivery: None,
            dispatch_workspace: Some(forge_app::DispatchWorkspace {
                id: "workspace".into(),
            }),
            resources: repository
                .list_agent_assignment_resources(&assignment.id)
                .unwrap(),
        };
        let path = std::env::temp_dir().join(format!(
            "forge-herdr-retire-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            let mut transcript = vec![
                (
                    "workspace.get",
                    serde_json::json!({"result":{"workspace":{"workspace_id":"workspace","cwd":"/forge-data/herdr-dispatch-workspace"}}}),
                ),
                (
                    "tab.get",
                    serde_json::json!({"result":{"tab":{"tab_id":"tab","workspace_id":"workspace"}}}),
                ),
                (
                    "pane.get",
                    serde_json::json!({"result":{"pane":{"pane_id":"pane","tab_id":"tab","workspace_id":"workspace"}}}),
                ),
                (
                    "agent.get",
                    serde_json::json!({"result":{"type":"agent_info","agent":{"name":"forge-dispatch-assignment","workspace_id":"workspace","tab_id":"tab","pane_id":"pane","agent_status":"done"}}}),
                ),
                (
                    "pane.release_agent",
                    serde_json::json!({"result":{"type":"ok"}}),
                ),
                ("tab.close", serde_json::json!({"result":{"type":"ok"}})),
                (
                    "tab.get",
                    serde_json::json!({"error":{"code":"tab_not_found"}}),
                ),
                (
                    "pane.get",
                    serde_json::json!({"error":{"code":"pane_not_found"}}),
                ),
                (
                    "agent.get",
                    serde_json::json!({"error":{"code":"agent_not_found"}}),
                ),
                (
                    "agent.list",
                    serde_json::json!({"result":{"type":"agent_list","agents":[]}}),
                ),
            ];
            match outcome {
                "workspace-missing" => {
                    transcript[0].1 = serde_json::json!({"error":{"code":"workspace_not_found"}});
                    transcript.truncate(1);
                }
                "agent-tuple-mismatch" => {
                    transcript[3].1["result"]["agent"]["pane_id"] = "foreign-pane".into();
                    transcript.truncate(4);
                }
                "agent-renamed" => {
                    transcript[3].1["result"]["agent"]["name"] = "replacement".into();
                    transcript.truncate(4);
                }
                "agent-malformed" => {
                    transcript[3].1["result"]["agent"] = serde_json::Value::Null;
                    transcript.truncate(4);
                }
                "agent-replaced" => {
                    transcript[9].1["result"]["agents"] = serde_json::json!([{
                        "name":"replacement", "workspace_id":"workspace",
                        "tab_id":"tab", "pane_id":"pane", "agent_status":"working"
                    }]);
                }
                "list-malformed" => {
                    transcript[9].1["result"]["agents"] = serde_json::json!([{}]);
                }
                "cleanup-retry" => {
                    let retry = transcript.clone();
                    transcript[5].1 = serde_json::json!({"error":{"code":"internal_error"}});
                    transcript.truncate(6);
                    transcript.extend(retry);
                }
                "tab-live" => {
                    transcript[6].1 = serde_json::json!({"result":{"tab":{"tab_id":"tab","workspace_id":"workspace"}}});
                    transcript.truncate(7);
                }
                "pane-live" => {
                    transcript[7].1 = serde_json::json!({"result":{"pane":{"pane_id":"pane","tab_id":"tab","workspace_id":"workspace"}}});
                    transcript.truncate(8);
                }
                "agent-live" => {
                    transcript[8].1 = serde_json::json!({"result":{"type":"agent_info","agent":{"name":"forge-dispatch-assignment","workspace_id":"workspace","tab_id":"tab","pane_id":"pane","agent_status":"working"}}});
                    transcript.truncate(9);
                }
                "provider-error" => {
                    transcript[6].1 = serde_json::json!({"error":{"code":"internal_error"}});
                    transcript.truncate(7);
                }
                "tuple-mismatch" => {
                    transcript[1].1 = serde_json::json!({"result":{"tab":{"tab_id":"tab","workspace_id":"foreign-workspace"}}});
                    transcript.truncate(2);
                }
                _ => {}
            }
            for (method, response) in transcript {
                listener.set_nonblocking(true).unwrap();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(
                                std::time::Instant::now() < deadline,
                                "missing verification request: {method}"
                            );
                            thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(error) => panic!("{error}"),
                    }
                };
                stream.set_nonblocking(false).unwrap();
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], method);
                assert_ne!(request["method"], "workspace.close");
                if method == "pane.release_agent" {
                    assert_eq!(request["params"]["pane_id"], "pane");
                    assert_eq!(request["params"]["agent"], "forge-dispatch-assignment");
                }
                serde_json::to_writer(&mut stream, &response).unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        let adapter = super::HerdrAdapter {
            socket_path: path.clone(),
            dispatch_root: std::env::temp_dir().join("forge-dispatch-workspace"),
        };
        let result = adapter.retire(&context, &repository);
        assert_eq!(result.is_ok(), outcome == "absent", "{outcome}: {result:?}");
        let resources = repository
            .list_agent_assignment_resources(&assignment.id)
            .unwrap();
        assert!(
            resources
                .iter()
                .all(|resource| resource.retired_at_millis.is_some() == (outcome == "absent"))
        );
        if outcome == "cleanup-retry" {
            adapter.retire(&context, &repository).unwrap();
            assert!(
                repository
                    .list_agent_assignment_resources(&assignment.id)
                    .unwrap()
                    .iter()
                    .all(|resource| resource.retired_at_millis.is_some())
            );
        }
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn workspace_selection_verifies_exact_identity_with_herdr() {
        use std::{
            io::{BufRead, BufReader, Write},
            os::unix::net::UnixListener,
            thread,
        };
        let path = std::env::temp_dir().join(format!(
            "forge-workspace-select-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let listener = UnixListener::bind(&path).unwrap();
        let server = thread::spawn(move || {
            for returned in [Some("requested"), Some("other"), None] {
                let mut stream = listener.accept().unwrap().0;
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], "workspace.get");
                assert_eq!(request["params"]["workspace_id"], "requested");
                serde_json::to_writer(
                    &mut stream,
                    &serde_json::json!({"result":{"workspace":{"workspace_id":returned}}}),
                )
                .unwrap();
                stream.write_all(b"\n").unwrap();
            }
        });
        let adapter = super::HerdrAdapter {
            socket_path: path.clone(),
            dispatch_root: "/unused".into(),
        };
        assert_eq!(
            adapter.verify_workspace("requested").unwrap(),
            forge_app::DispatchWorkspace {
                id: "requested".into()
            }
        );
        assert!(adapter.verify_workspace("requested").is_err());
        assert!(adapter.verify_workspace("requested").is_err());
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn dispatch_without_selected_workspace_does_not_create_one() {
        let repository = forge_store::SqliteProjectRepository::in_memory().unwrap();
        let adapter = super::HerdrAdapter {
            socket_path: std::env::temp_dir().join("forge-nonexistent-herdr.sock"),
            dispatch_root: std::env::temp_dir().join("forge-unused-dispatch-root"),
        };
        assert!(matches!(
            adapter.dispatch_workspace(&repository),
            Err(forge_app::AgentAdapterError::Conflict(_))
        ));
    }

    #[test]
    fn observation_rejects_a_mismatched_persisted_tuple() {
        let agent = serde_json::json!({
            "workspace_id":"other", "tab_id":"tab", "pane_id":"pane", "agent_status":"done"
        });
        assert!(matches!(
            super::HerdrAdapter::observation_from_agent(&agent, ("workspace", "tab", "pane")),
            Err(forge_app::AgentAdapterError::Conflict(_))
        ));
    }

    #[test]
    fn status_events_wake_only_for_owned_panes_and_are_safe_to_repeat() {
        let owned = vec!["owned-pane".to_owned()];
        let matching = serde_json::json!({
            "method":"event",
            "params":{"event":{"type":"pane.agent_status_changed", "pane_id":"owned-pane"}}
        });
        let mismatched = serde_json::json!({
            "type":"pane.agent_status_changed", "pane_id":"someone-elses-pane"
        });
        let unrelated = serde_json::json!({"type":"pane.created", "pane_id":"owned-pane"});

        // Duplicate delivery only produces equivalent stateless wake eligibility; durable state
        // remains exclusively in the subsequent authoritative reconciliation.
        assert!(super::event_is_owned_status_change(&matching, &owned));
        assert!(super::event_is_owned_status_change(&matching, &owned));
        assert!(!super::event_is_owned_status_change(&mismatched, &owned));
        assert!(!super::event_is_owned_status_change(&unrelated, &owned));
    }

    #[test]
    fn observer_snapshot_validation_requires_the_supported_contract() {
        assert!(
            super::validate_snapshot(&serde_json::json!({
                "snapshot":{"protocol":22, "schema_version":1, "workspaces":[]}
            }))
            .is_ok()
        );
        assert!(
            super::validate_snapshot(&serde_json::json!({
                "protocol":21, "schema_version":1, "workspaces":[]
            }))
            .is_err()
        );
    }
}
