//! Protocol-22 client for the local Herdr Unix-socket API.

use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    time::Duration,
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

    fn agent_observation(
        &self,
        name: &str,
        tuple: (&str, &str, &str),
    ) -> Result<AgentObservation, AgentAdapterError> {
        let agent = self.request("agent.get", json!({"target": name}))?;
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
        _: &AgentDispatchContext,
        _: &dyn AgentDispatchRepository,
    ) -> Result<(), AgentAdapterError> {
        Err(AgentAdapterError::Cleanup(
            "Herdr retirement is handled by the retirement adapter task".into(),
        ))
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
}
