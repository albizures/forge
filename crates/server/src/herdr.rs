//! Protocol-22 client for the local Herdr Unix-socket API.

use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
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
        Ok(HerdrAdapter { socket_path })
    }
}

/// A synchronous, one-request-per-connection Herdr protocol-22 client.
pub struct HerdrAdapter {
    socket_path: PathBuf,
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

    fn workspace_id(&self, context: &AgentDispatchContext) -> Result<String, AgentAdapterError> {
        let snapshot = self.snapshot()?;
        let folder = context.registered_project_folder.to_string_lossy();
        let workspaces = snapshot
            .get("workspaces")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AgentAdapterError::Endpoint("Herdr snapshot has no workspaces".into())
            })?;
        let panes = snapshot
            .get("panes")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        let matches: Vec<_> = workspaces
            .iter()
            .filter(|workspace| {
                workspace.get("cwd").and_then(Value::as_str) == Some(folder.as_ref())
                    || workspace
                        .get("workspace_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| {
                            panes.iter().any(|pane| {
                                pane.get("workspace_id").and_then(Value::as_str) == Some(id)
                                    && pane.get("cwd").and_then(Value::as_str)
                                        == Some(folder.as_ref())
                            })
                        })
            })
            .collect();
        match matches.as_slice() {
            [workspace] => workspace
                .get("workspace_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| AgentAdapterError::Endpoint("Herdr workspace has no ID".into())),
            [] => Err(AgentAdapterError::InvalidRequest(
                "no Herdr workspace matches the registered Project folder".into(),
            )),
            _ => Err(AgentAdapterError::Conflict(
                "multiple Herdr workspaces match the registered Project folder".into(),
            )),
        }
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

    /// Parses an authoritative AgentInfo response. Readiness and sequence fields are
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
            Ok(agent) => Self::observation_from_agent(&agent, tuple),
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
        let agents = result
            .get("agents")
            .and_then(Value::as_array)
            .ok_or_else(|| AgentAdapterError::Endpoint("Herdr agent.list has no agents".into()))?;
        let same_name = agents.iter().find(|agent| {
            agent.get("name").and_then(Value::as_str) == Some(name)
                || agent.get("agent_name").and_then(Value::as_str) == Some(name)
        });
        if let Some(agent) = same_name {
            return Self::observation_from_agent(agent, tuple);
        }
        if agents.iter().any(|agent| {
            agent.get("workspace_id").and_then(Value::as_str) == Some(tuple.0)
                && agent.get("tab_id").and_then(Value::as_str) == Some(tuple.1)
                && agent.get("pane_id").and_then(Value::as_str) == Some(tuple.2)
        }) {
            return Err(AgentAdapterError::Conflict(
                "Herdr persisted pane is occupied by a different agent".into(),
            ));
        }
        Ok(AgentObservation::Absent)
    }
}

impl AgentDispatchAdapter for HerdrAdapter {
    fn probe(&self) -> Result<(), AgentAdapterError> {
        self.request("ping", json!({}))?;
        self.snapshot().map(|_| ())
    }

    fn provision(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        self.probe()?;
        let label = Self::label(context)?;
        let workspace = match Self::active(&context.resources, "workspace") {
            Some(id) => id.to_owned(),
            None => {
                let id = self.workspace_id(context)?;
                Self::retain(resources, context, "workspace", &id)?;
                id
            }
        };
        let (tab, root) = match Self::active(&context.resources, "tab") {
            Some(id) => (id.to_owned(), None),
            None => {
                let result = self.request("tab.create", json!({"workspace_id": workspace, "label": label, "cwd": context.registered_project_folder, "focus": false}))?;
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
            }
        };
        let pane = match Self::active(&context.resources, "pane") {
            Some(id) => id.to_owned(),
            None => {
                let root = root.unwrap_or(self.root_pane(&workspace, &tab)?);
                let result = self.request("pane.split", json!({"target_pane_id": root, "workspace_id": workspace, "direction": "right", "focus": false}))?;
                let id = result
                    .pointer("/pane/pane_id")
                    .or_else(|| result.get("pane_id"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AgentAdapterError::Endpoint("pane.split did not return pane_id".into())
                    })?
                    .to_owned();
                Self::retain(resources, context, "pane", &id)?;
                id
            }
        };
        if Self::active(&context.resources, "agent-name").is_none() {
            self.request("agent.start", json!({"name": label, "kind": "pi", "pane_id": pane, "args": ["--model", context.assignment.profile.model], "timeout_ms": context.assignment.profile.startup_timeout_millis}))?;
            Self::retain(resources, context, "agent-name", &label)?;
        }
        let mut prompt = context.rendered_initial_prompt.clone();
        if let Some(capability) = &context.capability_delivery {
            prompt.push_str(&format!(
                "\n\nForge capability endpoint: {}\nForge capability secret file: {}",
                capability.endpoint,
                capability.secret_file.display()
            ));
        }
        match self.request("agent.prompt", json!({"target": label, "text": prompt})) {
            Ok(_) => Ok(()),
            Err(AgentAdapterError::InvalidRequest(message)) if message == "agentblocked" => Ok(()),
            Err(error) => Err(error),
        }
    }

    fn observe(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentObservation, AgentAdapterError> {
        let (Some(workspace), Some(tab), Some(pane), Some(agent)) = (
            Self::active(&context.resources, "workspace"),
            Self::active(&context.resources, "tab"),
            Self::active(&context.resources, "pane"),
            Self::active(&context.resources, "agent-name"),
        ) else {
            return Ok(AgentObservation::Absent);
        };
        self.agent_observation(agent, (workspace, tab, pane))
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
        let Some(workspace) = Self::resource_is_active(context, "workspace") else {
            return Err(AgentAdapterError::Conflict(
                "cannot retire Herdr resources without a persisted workspace ID".into(),
            ));
        };
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
                Err(AgentAdapterError::NotFound(_)) => {
                    Self::retire_resource(resources, context, "tab", tab)?
                }
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
                Err(AgentAdapterError::NotFound(_)) => {
                    Self::retire_resource(resources, context, "pane", pane)?;
                    if let Some(agent) = &agent {
                        Self::retire_resource(resources, context, "agent-name", agent)?;
                    }
                }
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
            match self.agent_observation(agent, (&workspace, tab.as_deref().unwrap_or(""), pane))? {
                AgentObservation::Absent => {
                    Self::retire_resource(resources, context, "agent-name", agent)?
                }
                _ => {
                    self.cleanup_request(
                        "pane.release_agent",
                        json!({"pane_id": pane, "source": "forge-agent-dispatch", "agent": agent, "seq": 1}),
                    )?;
                    Self::retire_resource(resources, context, "agent-name", agent)?;
                }
            }
        }
        if let Some(pane) = &pane
            && pane_present
        {
            self.cleanup_request("pane.close", json!({"pane_id": pane}))?;
            Self::retire_resource(resources, context, "pane", pane)?;
        }
        if let Some(tab) = &tab
            && tab_present
        {
            self.cleanup_request("tab.close", json!({"tab_id": tab}))?;
            Self::retire_resource(resources, context, "tab", tab)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use forge_app::AgentDispatchAdapter;

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
        };
        adapter.probe().unwrap();
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
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
                    serde_json::json!({"error":{"code":"not_found"}}),
                ),
                (
                    "agent.list",
                    serde_json::json!({"result":{"agents":[{
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
    fn retirement_releases_and_closes_only_the_verified_dedicated_resources() {
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
                body: "dispatch task".into(),
                project_slugs: vec!["project".into()],
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
            ("workspace", "workspace"),
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
        let context = AgentDispatchContext {
            assignment: assignment.clone(),
            registered_project_folder: "/project".into(),
            rendered_initial_prompt: String::new(),
            capability_delivery: None,
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
            for (method, response) in [
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
                    serde_json::json!({"result":{"name":"forge-dispatch-assignment","workspace_id":"workspace","tab_id":"tab","pane_id":"pane","agent_status":"done"}}),
                ),
                ("pane.release_agent", serde_json::json!({"result":{}})),
                ("pane.close", serde_json::json!({"result":{}})),
                ("tab.close", serde_json::json!({"result":{}})),
            ] {
                let mut stream = listener.accept().unwrap().0;
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
        };
        adapter.retire(&context, &repository).unwrap();
        server.join().unwrap();
        let resources = repository
            .list_agent_assignment_resources(&assignment.id)
            .unwrap();
        assert!(
            resources
                .iter()
                .find(|resource| resource.resource_kind == "workspace")
                .unwrap()
                .retired_at_millis
                .is_none()
        );
        assert!(
            resources
                .iter()
                .filter(|resource| resource.resource_kind != "workspace")
                .all(|resource| resource.retired_at_millis.is_some())
        );
        std::fs::remove_file(path).unwrap();
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
