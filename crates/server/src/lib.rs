//! Daemon composition root and HTTP adapters.
//!
//! The daemon listens on loopback by default. Binding it to a remote address
//! exposes an unauthenticated service, so use that only on a trusted private network.

mod herdr;
#[cfg(test)]
mod test_projects;
mod workspaces;

use std::{
    collections::HashMap,
    convert::Infallible,
    env, fs, io,
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::{
    io::AsyncReadExt,
    sync::{mpsc, watch},
    time::{self, MissedTickBehavior},
};
use tokio_stream::wrappers::ReceiverStream;

use askama::Template;
use axum::{
    Router,
    body::Body,
    extract::{ConnectInfo, Path as AxumPath, Request, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response, Sse, sse::Event},
    routing::{get, post},
};
use forge_app::{
    AgentDispatchAdapter, AgentDispatchAdapterFactory, AgentDispatchContext, AgentProfile,
    Application, ApplicationStatus, AssignmentCapabilityDelivery, ManagedWorktreeAdapter, Project,
    RecordStatus, RecordType, RecordUpdateEnvelope, RecordUpdateId, RecordUpdateKind,
    RecordUpdateKindFilter, RecordUpdateSubscriptionId, RegistrationError, RepositoryInspector,
    TaskKind, WorktreeSafetyAssessment,
};
use forge_rpc::{
    AddRecordCommentRequest, AgentAssignmentResponse, AgentDispatchApiServer,
    AgentDispatchProfileResponse, AgentDispatchReconcileResponse, AgentDispatchStatusRequest,
    AgentDispatchStatusResponse, AgentSubscriptionApiServer, CreateAgentSubscriptionRequest,
    CreateManagedWorktreeRequest, DeleteRecordCommentRequest, DispatchRecordRequest,
    DispatchRecordResponse, DoctorManagedWorktreesRequest, DoneRecordRequest, EditRecordRequest,
    ListManagedWorktreesRequest, ListManagedWorktreesResponse, ListRecordCommentsResponse,
    ListRecordUpdatesResponse, ListRecordsRequest, ListRecordsResponse,
    ManageAgentSubscriptionRequest, ManagedWorktreeResponse, ManagementStatus,
    OpenGrillingUnderWayfinderRequest, OpenRecordRequest, OpenSpecRequest, OpenTaskRequest,
    OpenTaskUnderSpecRequest, OpenTaskUnderWayfinderRequest, OpenWayfinderRequest,
    ProjectApiServer, ProjectHereRequest, ProjectHereResponse, RecordApiServer, RecordIdRequest,
    RecordResponse, RecordUpdateSubscriptionResponse, RegisterProjectRequest,
    RegisterProjectResponse, RelateRecordRequest, RemoveManagedWorktreeRequest,
    RetireAgentAssignmentRequest, RevisionIdentity, SetProjectSetupHookRequest,
    SetWorktreeRootRequest, StartAssignmentTaskRequest, SystemApiServer, SystemStatusResponse,
    WorktreeApiServer, WorktreeRootResponse,
};
use forge_store::SqliteProjectRepository;
use fs2::FileExt;
use futures_util::StreamExt as FuturesStreamExt;
use herdr::{HerdrAdapterFactory, observe_status_changes};
use jsonrpsee::{
    core::{RpcResult, async_trait},
    server::{Server, stop_channel},
};
use tower::ServiceExt;
use tracing::{info, warn};

/// Loopback address used when neither supported configuration source is set.
pub const DEFAULT_BIND_ADDR: &str = "127.0.0.1:45003";

/// Explicit inputs supplied by Forgeup when it launches a Managed Daemon.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedLaunchInputs {
    pub bind_addr: String,
    pub store_path: String,
    pub runtime_record_path: String,
    pub lifecycle_control_endpoint: String,
    pub lifecycle_credential: String,
    pub daemon_instance_id: String,
}

/// Managed-only configuration retained at the server composition boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct ManagedDaemonConfig {
    pub runtime_record_path: PathBuf,
    pub lifecycle_control_endpoint: PathBuf,
    lifecycle_credential: String,
    pub daemon_instance_id: String,
}

impl std::fmt::Debug for ManagedDaemonConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ManagedDaemonConfig")
            .field("runtime_record_path", &self.runtime_record_path)
            .field(
                "lifecycle_control_endpoint",
                &self.lifecycle_control_endpoint,
            )
            .field("lifecycle_credential", &"[redacted]")
            .field("daemon_instance_id", &self.daemon_instance_id)
            .finish()
    }
}

impl ManagedDaemonConfig {
    /// Credential used only by the per-instance lifecycle listener.
    #[must_use]
    pub fn lifecycle_credential(&self) -> &str {
        &self.lifecycle_credential
    }
}

/// Daemon startup configuration resolved before the listener is opened.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonConfig {
    /// Address owned by the one daemon listener.
    pub bind_addr: SocketAddr,
    /// Database path selected for this daemon instance.
    pub store_path: PathBuf,
    /// Present only when Forgeup explicitly owns this daemon launch.
    pub managed: Option<ManagedDaemonConfig>,
}

impl DaemonConfig {
    /// Resolves manual-launch configuration with an explicit flag taking precedence over the environment.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected address is not a socket address.
    pub fn resolve(
        flag_bind_addr: Option<&str>,
        env_bind_addr: Option<&str>,
    ) -> Result<Self, String> {
        let value = flag_bind_addr
            .or(env_bind_addr)
            .unwrap_or(DEFAULT_BIND_ADDR);
        let bind_addr = SocketAddr::from_str(value)
            .map_err(|error| format!("invalid daemon bind address `{value}`: {error}"))?;
        Ok(Self {
            bind_addr,
            store_path: env::var_os("FORGE_STORE_PATH")
                .map_or_else(|| PathBuf::from("forge.sqlite3"), PathBuf::from),
            managed: None,
        })
    }

    /// Resolves the explicit Managed Daemon configuration without consulting ambient settings.
    ///
    /// # Errors
    ///
    /// Returns an error when any managed input is empty or its bind address is malformed.
    pub fn managed(inputs: ManagedLaunchInputs) -> Result<Self, String> {
        let bind_addr = SocketAddr::from_str(&inputs.bind_addr).map_err(|error| {
            format!(
                "invalid managed daemon bind address `{}`: {error}",
                inputs.bind_addr
            )
        })?;
        let required = [
            ("store path", &inputs.store_path),
            ("runtime-record path", &inputs.runtime_record_path),
            (
                "lifecycle-control endpoint",
                &inputs.lifecycle_control_endpoint,
            ),
            ("lifecycle credential", &inputs.lifecycle_credential),
            ("daemon-instance ID", &inputs.daemon_instance_id),
        ];
        if let Some((name, _)) = required.iter().find(|(_, value)| value.trim().is_empty()) {
            return Err(format!("managed daemon {name} must not be empty"));
        }
        Ok(Self {
            bind_addr,
            store_path: PathBuf::from(inputs.store_path),
            managed: Some(ManagedDaemonConfig {
                runtime_record_path: PathBuf::from(inputs.runtime_record_path),
                lifecycle_control_endpoint: PathBuf::from(inputs.lifecycle_control_endpoint),
                lifecycle_credential: inputs.lifecycle_credential,
                daemon_instance_id: inputs.daemon_instance_id,
            }),
        })
    }

    /// Resolves process configuration, preserving manual environment compatibility.
    ///
    /// Managed inputs are explicit and therefore take precedence over all ambient settings.
    ///
    /// # Errors
    ///
    /// Returns an error when the selected configuration is invalid.
    pub fn from_process(
        flag_bind_addr: Option<&str>,
        managed: Option<ManagedLaunchInputs>,
    ) -> Result<Self, String> {
        match managed {
            Some(inputs) => Self::managed(inputs),
            None => Self::resolve(flag_bind_addr, env::var("FORGE_BIND_ADDR").ok().as_deref()),
        }
    }
}

fn authenticate_assignment_secret(
    root: Option<&std::path::Path>,
    assignment_id: &str,
    secret: &str,
) -> RpcResult<()> {
    let valid_id = !assignment_id.is_empty()
        && assignment_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    let expected = root
        .filter(|_| valid_id)
        .and_then(|root| std::fs::read_to_string(root.join(assignment_id)).ok());
    let authenticated = expected.as_deref().is_some_and(|s| {
        let expected = s.trim_end_matches('\n').as_bytes();
        let supplied = secret.as_bytes();
        !expected.is_empty()
            && expected.len() == supplied.len()
            && expected
                .iter()
                .zip(supplied)
                .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                == 0
    });
    if authenticated {
        Ok(())
    } else {
        Err(rpc_error(RegistrationError::new(
            "assignment-capability-denied",
            "invalid Assignment Capability",
        )))
    }
}

/// Owns the application and exposes it through the daemon transport adapters.
#[derive(Clone)]
pub struct Daemon {
    application: Arc<Application>,
    management: ManagementStatus,
    delivery: Arc<SubscriptionDelivery>,
    capability_secret_root: Option<PathBuf>,
}

#[derive(Default)]
struct SubscriptionDelivery {
    connections: Mutex<HashMap<RecordUpdateSubscriptionId, DeliveryConnection>>,
    next_connection_id: AtomicU64,
}

struct DeliveryConnection {
    connection_id: u64,
    assignment_id: String,
    sender: mpsc::Sender<RecordUpdateEnvelope>,
    cancelled: watch::Sender<bool>,
    lease_expires_at_millis: watch::Sender<u64>,
    cursor: RecordUpdateId,
}

type DeliveryConnectionStart = (
    u64,
    mpsc::Receiver<RecordUpdateEnvelope>,
    watch::Receiver<bool>,
    watch::Receiver<u64>,
    Vec<RecordUpdateEnvelope>,
);

impl SubscriptionDelivery {
    /// Claims the active connection slot before replaying under the publication lock.
    /// A committed update is therefore either in `replay` or waits to be queued live.
    fn connect_and_replay(
        &self,
        application: &Application,
        subscription: &forge_app::RecordUpdateSubscription,
        cursor: RecordUpdateId,
    ) -> Result<DeliveryConnectionStart, RegistrationError> {
        let (sender, receiver) = mpsc::channel(256);
        let (cancelled, cancellation) = watch::channel(false);
        let (lease_expires_at_millis, lease) = watch::channel(subscription.lease_expires_at_millis);
        let id = subscription.id;
        let connection_id = self.next_connection_id.fetch_add(1, Ordering::Relaxed);
        let mut connections = self
            .connections
            .lock()
            .expect("delivery lock is not poisoned");
        if let Some(previous) = connections.insert(
            id,
            DeliveryConnection {
                connection_id,
                assignment_id: subscription.assignment_id.clone(),
                sender,
                cancelled,
                lease_expires_at_millis,
                cursor,
            },
        ) {
            let _ = previous.cancelled.send(true);
        }
        let replay = match application.replay_assignment_subscription_updates(id, cursor) {
            Ok(replay) => replay,
            Err(error) => {
                connections.remove(&id);
                return Err(error);
            }
        };
        if let Some(last) = replay.last() {
            connections
                .get_mut(&id)
                .expect("new delivery connection is retained")
                .cursor = last.update.id;
        }
        Ok((connection_id, receiver, cancellation, lease, replay))
    }

    fn cancel_current_connection(&self, id: RecordUpdateSubscriptionId, connection_id: u64) {
        let mut connections = self
            .connections
            .lock()
            .expect("delivery lock is not poisoned");
        if connections
            .get(&id)
            .is_some_and(|connection| connection.connection_id == connection_id)
            && let Some(connection) = connections.remove(&id)
        {
            let _ = connection.cancelled.send(true);
        }
    }

    fn cancel_subscription(&self, id: RecordUpdateSubscriptionId) {
        if let Some(connection) = self
            .connections
            .lock()
            .expect("delivery lock is not poisoned")
            .remove(&id)
        {
            let _ = connection.cancelled.send(true);
        }
    }

    fn cancel_assignment(&self, assignment_id: &str) {
        let mut connections = self
            .connections
            .lock()
            .expect("delivery lock is not poisoned");
        connections.retain(|_, connection| {
            if connection.assignment_id == assignment_id {
                let _ = connection.cancelled.send(true);
                false
            } else {
                true
            }
        });
    }

    fn renew_subscription(&self, id: RecordUpdateSubscriptionId, lease_expires_at_millis: u64) {
        if let Some(connection) = self
            .connections
            .lock()
            .expect("delivery lock is not poisoned")
            .get(&id)
        {
            let _ = connection
                .lease_expires_at_millis
                .send(lease_expires_at_millis);
        }
    }

    fn publish(&self, application: &Application) {
        let mut connections = self
            .connections
            .lock()
            .expect("delivery lock is not poisoned");
        connections.retain(|id, connection| {
            let Ok(updates) =
                application.replay_assignment_subscription_updates(*id, connection.cursor)
            else {
                let _ = connection.cancelled.send(true);
                return false;
            };
            for update in updates {
                connection.cursor = update.update.id;
                if connection.sender.try_send(update).is_err() {
                    let _ = connection.cancelled.send(true);
                    return false;
                }
            }
            true
        });
    }
}

impl Daemon {
    /// Creates a manual daemon around the shared application boundary.
    #[must_use]
    pub fn new(application: Application) -> Self {
        Self::with_daemon_instance_id(application, None)
    }

    /// Creates a daemon that reports the supplied managed-launch ownership ID.
    #[must_use]
    pub fn with_daemon_instance_id(
        application: Application,
        daemon_instance_id: Option<String>,
    ) -> Self {
        Self {
            application: Arc::new(application),
            delivery: Arc::new(SubscriptionDelivery::default()),
            capability_secret_root: None,
            management: ManagementStatus::new(
                RevisionIdentity::from(forge_build_info::current()),
                daemon_instance_id,
            ),
        }
    }
}

/// Git command implementation kept at the daemon composition edge.
pub struct GitInspector;

impl RepositoryInspector for GitInspector {
    fn inspect(&self, folder: &Path) -> Result<Project, RegistrationError> {
        let canonical = fs::canonicalize(folder).map_err(|_| {
            RegistrationError::new(
                "invalid-folder",
                format!("`{}` is not an existing folder", folder.display()),
            )
        })?;
        if !canonical.is_dir() {
            return Err(RegistrationError::new(
                "invalid-folder",
                format!("`{}` is not a folder", canonical.display()),
            ));
        }
        let root = git(&canonical, &["rev-parse", "--show-toplevel"]).map_err(|()| {
            RegistrationError::new(
                "not-git",
                format!(
                    "`{}` is not an eligible Git repository",
                    canonical.display()
                ),
            )
        })?;
        let root = fs::canonicalize(root).map_err(|_| {
            RegistrationError::new("not-git", "could not resolve Git worktree root")
        })?;
        if root != canonical {
            return Err(RegistrationError::new(
                "not-main-worktree",
                "the Project folder must be the Git main worktree root",
            ));
        }
        let git_dir = resolve_git_path(
            &root,
            &git(&root, &["rev-parse", "--git-dir"]).map_err(|()| {
                RegistrationError::new("not-git", "could not inspect Git directory")
            })?,
        );
        let common_dir = resolve_git_path(
            &root,
            &git(&root, &["rev-parse", "--git-common-dir"]).map_err(|()| {
                RegistrationError::new("not-git", "could not inspect Git common directory")
            })?,
        );
        if git_dir != common_dir {
            return Err(RegistrationError::new(
                "not-main-worktree",
                "only a Git main worktree may be registered as a Project",
            ));
        }
        let origin_url = git(&root, &["remote", "get-url", "origin"]).map_err(|()| {
            RegistrationError::new(
                "missing-origin",
                "Git remote `origin` is required to register a Project",
            )
        })?;
        Ok(Project {
            reference: String::new(),
            workspace_id: String::new(),
            slug: String::new(),
            project_root: root,
            origin_url,
        })
    }

    fn head_commit(&self, folder: &Path) -> Result<String, RegistrationError> {
        git(folder, &["rev-parse", "HEAD"]).map_err(|()| {
            RegistrationError::new(
                "base-commit-unavailable",
                "could not resolve Project root HEAD",
            )
        })
    }
}

fn git(folder: &Path, arguments: &[&str]) -> Result<String, ()> {
    let output = Command::new("git")
        .arg("-C")
        .arg(folder)
        .args(arguments)
        .output()
        .map_err(|_| ())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(())
    }
}

/// Server-only adapter for Git worktree lifecycle and platform-shell setup hooks.
pub struct GitWorktreeAdapter;

impl ManagedWorktreeAdapter for GitWorktreeAdapter {
    fn validate_recovery(
        &self,
        repository: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), RegistrationError> {
        let invalid = || {
            RegistrationError::new(
                "worktree-recovery-invalid",
                "Existing worktree does not match the expected repository, path, branch, and base commit",
            )
        };
        let actual_path = fs::canonicalize(path).map_err(|_| invalid())?;
        let root = git(path, &["rev-parse", "--show-toplevel"]).map_err(|()| invalid())?;
        if fs::canonicalize(root).map_err(|_| invalid())? != actual_path {
            return Err(invalid());
        }
        let repo_common = git(
            repository,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .map_err(|()| invalid())?;
        let tree_common = git(
            path,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .map_err(|()| invalid())?;
        if fs::canonicalize(repo_common).map_err(|_| invalid())?
            != fs::canonicalize(tree_common).map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        if git(path, &["symbolic-ref", "--short", "HEAD"]).map_err(|()| invalid())? != branch {
            return Err(invalid());
        }
        git(path, &["merge-base", "--is-ancestor", base, "HEAD"]).map_err(|()| invalid())?;
        Ok(())
    }
    fn create(
        &self,
        repository: &Path,
        path: &Path,
        branch: &str,
        base: &str,
    ) -> Result<(), RegistrationError> {
        git(
            repository,
            &[
                "worktree",
                "add",
                "-b",
                branch,
                &path.to_string_lossy(),
                base,
            ],
        )
        .map(|_| ())
        .map_err(|()| {
            RegistrationError::new(
                "managed-worktree-invalid",
                "could not create Managed Worktree",
            )
        })
    }

    fn remove(&self, path: &Path, force: bool) -> Result<(), RegistrationError> {
        if !path.exists() {
            return Ok(());
        }
        let path = path.to_string_lossy();
        let arguments = if force {
            vec!["worktree", "remove", "--force", &path]
        } else {
            vec!["worktree", "remove", &path]
        };
        match git(Path::new(&*path), &arguments) {
            Ok(_) => Ok(()),
            Err(()) if force => fs::remove_dir_all(&*path).map_err(|_| {
                RegistrationError::new(
                    "managed-worktree-invalid",
                    "could not remove Managed Worktree",
                )
            }),
            Err(()) => Err(RegistrationError::new(
                "managed-worktree-invalid",
                "could not remove Managed Worktree",
            )),
        }
    }

    fn assess(&self, path: &Path) -> WorktreeSafetyAssessment {
        let mut facts = Vec::new();
        let mut blockers = Vec::new();
        if !path.is_dir() {
            return WorktreeSafetyAssessment {
                facts: vec!["checkout-missing".into()],
                blockers: vec!["checkout-missing".into()],
                removable: false,
            };
        }
        if git(path, &["rev-parse", "--is-inside-work-tree"]).as_deref() != Ok("true") {
            return WorktreeSafetyAssessment {
                facts: vec!["git-metadata-invalid".into()],
                blockers: vec!["git-metadata-invalid".into()],
                removable: false,
            };
        }
        match git(path, &["status", "--porcelain"]) {
            Ok(status) if status.is_empty() => facts.push("clean".into()),
            Ok(_) => {
                facts.push("dirty".into());
                blockers.push("dirty");
            }
            Err(()) => {
                facts.push("git-metadata-invalid".into());
                blockers.push("git-metadata-invalid");
            }
        }
        if git(
            path,
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
        )
        .is_err()
        {
            facts.push("upstream-unresolvable".into());
            blockers.push("upstream-unresolvable");
        } else if let Ok(counts) = git(
            path,
            &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
        ) {
            let values: Vec<_> = counts.split_whitespace().collect();
            if let [behind, ahead] = values.as_slice() {
                if *behind != "0" {
                    facts.push(format!("behind:{behind}"));
                }
                if *ahead != "0" {
                    facts.push(format!("ahead:{ahead}"));
                    blockers.push("ahead");
                }
            } else {
                facts.push("upstream-unresolvable".into());
                blockers.push("upstream-unresolvable");
            }
        } else {
            facts.push("upstream-unresolvable".into());
            blockers.push("upstream-unresolvable");
        }
        WorktreeSafetyAssessment {
            removable: blockers.is_empty(),
            facts,
            blockers: blockers.into_iter().map(str::to_owned).collect(),
        }
    }

    fn run_setup_hook(
        &self,
        path: &Path,
        hook: &str,
        project_slug: &str,
        record_id: u64,
        branch: &str,
        base: &str,
    ) -> Result<(), RegistrationError> {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd");
            command.args(["/C", hook]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", hook]);
            command
        };
        let status = command
            .current_dir(path)
            .env("FORGE_PROJECT_SLUG", project_slug)
            .env("FORGE_RECORD_ID", record_id.to_string())
            .env("FORGE_WORKTREE_PATH", path)
            .env("FORGE_BRANCH", branch)
            .env("FORGE_BASE", base)
            .status()
            .map_err(|_| {
                RegistrationError::new("setup-hook-failed", "could not run Project Setup Hook")
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(RegistrationError::new(
                "setup-hook-failed",
                "Project Setup Hook failed",
            ))
        }
    }
}

fn resolve_git_path(root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    fs::canonicalize(if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    })
    .unwrap_or_else(|_| root.join(path))
}

/// Writes an Assignment Capability secret outside its Agent Workspace with owner-only access.
/// The capability is intentionally never persisted by Forge.
///
/// # Errors
///
/// Returns a validation or filesystem error without disclosing the secret.
pub fn write_assignment_capability_secret(
    path: &Path,
    workspace: &Path,
    secret: &str,
) -> Result<(), RegistrationError> {
    if !path.is_absolute() || path.starts_with(workspace) {
        return Err(RegistrationError::new(
            "invalid-capability-secret-path",
            "capability secret file must be absolute and outside the Agent Workspace",
        ));
    }
    let parent = path.parent().ok_or_else(|| {
        RegistrationError::new(
            "invalid-capability-secret-path",
            "capability secret file must have a parent directory",
        )
    })?;
    fs::create_dir_all(parent).map_err(|_| {
        RegistrationError::new(
            "capability-secret-write",
            "could not create capability secret directory",
        )
    })?;
    #[cfg(unix)]
    let options = {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        options
    };
    #[cfg(not(unix))]
    let mut options = {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        options
    };
    let mut file = options.open(path).map_err(|_| {
        RegistrationError::new(
            "capability-secret-write",
            "could not create capability secret file",
        )
    })?;
    file.write_all(secret.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|_| {
            RegistrationError::new(
                "capability-secret-write",
                "could not write capability secret file",
            )
        })
}

struct ServerAgentDispatchAdapterFactory {
    herdr: HerdrAdapterFactory,
    daemon_url: String,
    capability_secret_root: PathBuf,
}

impl AgentDispatchAdapterFactory for ServerAgentDispatchAdapterFactory {
    fn adapter_for(
        &self,
        profile: &AgentProfile,
    ) -> Result<Box<dyn AgentDispatchAdapter>, forge_app::AgentAdapterError> {
        self.herdr.adapter_for(profile).map(|adapter| {
            Box::new(CapabilityDeliveryAdapter {
                inner: adapter,
                daemon_url: self.daemon_url.clone(),
                capability_secret_root: self.capability_secret_root.clone(),
            }) as Box<dyn AgentDispatchAdapter>
        })
    }
}

/// Adds the server-owned, non-persistent capability delivery to the provider context.
/// The bearer is created once in an owner-only file; only its path reaches the provider.
struct CapabilityDeliveryAdapter {
    inner: herdr::HerdrAdapter,
    daemon_url: String,
    capability_secret_root: PathBuf,
}

impl CapabilityDeliveryAdapter {
    fn context(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<AgentDispatchContext, forge_app::AgentAdapterError> {
        if context.assignment.profile.record_type == forge_app::RecordType::Grilling {
            return Ok(context.clone());
        }
        if context.assignment.profile.record_type != forge_app::RecordType::Task {
            return Err(forge_app::AgentAdapterError::InvalidRequest(
                "bootstrap is only supported for Task Assignments".into(),
            ));
        }
        let secret_file = self.capability_secret_root.join(&context.assignment.id);
        if !secret_file.exists() {
            let mut bytes = [0_u8; 32];
            getrandom::fill(&mut bytes).map_err(|_| {
                forge_app::AgentAdapterError::Endpoint(
                    "could not issue Assignment Capability".into(),
                )
            })?;
            let secret = bytes.iter().fold(
                String::with_capacity(bytes.len() * 2),
                |mut secret, byte| {
                    use std::fmt::Write;
                    write!(secret, "{byte:02x}").expect("writing to a String cannot fail");
                    secret
                },
            );
            let workspace = context
                .agent_workspace
                .as_deref()
                .unwrap_or_else(|| self.inner.default_agent_workspace());
            write_assignment_capability_secret(&secret_file, workspace, &secret).map_err(
                |error| {
                    forge_app::AgentAdapterError::Endpoint(format!(
                        "could not issue Assignment Capability: {}",
                        error.code
                    ))
                },
            )?;
        }
        let cli_path = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|parent| parent.join("forge")))
            .filter(|path| path.is_file())
            .ok_or_else(|| {
                forge_app::AgentAdapterError::Endpoint(
                    "matching Forge CLI is unavailable beside the daemon".into(),
                )
            })?;
        let mut delivered = context.clone();
        delivered.bootstrap_prompt = Some("Run `\"$FORGE_ASSIGNMENT_FORGE_CLI\" assignment bootstrap` now. Only if it succeeds, follow the instructions it prints. If it fails, stop and wait; do not begin work.".into());
        delivered.capability_delivery = Some(AssignmentCapabilityDelivery {
            endpoint: self.daemon_url.clone(),
            secret_file,
            cli_path,
        });
        Ok(delivered)
    }
}

impl AgentDispatchAdapter for CapabilityDeliveryAdapter {
    fn probe(&self) -> Result<(), forge_app::AgentAdapterError> {
        self.inner.probe()
    }

    fn dispatch_available(&self, resources: &dyn forge_app::AgentDispatchRepository) -> bool {
        self.inner.dispatch_available(resources)
    }

    fn verify_workspace(
        &self,
        id: &str,
    ) -> Result<forge_app::DispatchWorkspace, forge_app::AgentAdapterError> {
        self.inner.verify_workspace(id)
    }

    fn provision(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn forge_app::AgentDispatchRepository,
    ) -> Result<(), forge_app::AgentAdapterError> {
        self.inner.provision(&self.context(context)?, resources)
    }

    fn observe(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
        self.inner.observe(context)
    }

    fn recover(
        &self,
        context: &AgentDispatchContext,
    ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
        self.inner.recover(context)
    }

    fn retire(
        &self,
        context: &AgentDispatchContext,
        resources: &dyn forge_app::AgentDispatchRepository,
    ) -> Result<(), forge_app::AgentAdapterError> {
        self.inner.retire(context, resources)
    }
}

fn application_from_config(config: &DaemonConfig) -> Result<Application, String> {
    let store_root = config.store_path.parent().unwrap_or_else(|| Path::new("."));
    let capability_secret_root = if store_root.is_absolute() {
        store_root.to_owned()
    } else {
        env::current_dir()
            .map_err(|error| format!("could not resolve Forge data root: {error}"))?
            .join(store_root)
    }
    .join("assignment-capabilities");
    let records =
        SqliteProjectRepository::open(&config.store_path).map_err(|error| error.message)?;
    let dispatch =
        SqliteProjectRepository::open(&config.store_path).map_err(|error| error.message)?;
    let managed_worktrees =
        SqliteProjectRepository::open(&config.store_path).map_err(|error| error.message)?;
    let workspaces =
        SqliteProjectRepository::open(&config.store_path).map_err(|error| error.message)?;
    Ok(Application::with_managed_worktrees_and_agent_dispatch(
        Box::new(records),
        Box::new(managed_worktrees),
        Box::new(GitWorktreeAdapter),
        Box::new(dispatch),
        Box::new(ServerAgentDispatchAdapterFactory {
            herdr: HerdrAdapterFactory::from_configuration_store(config.store_path.clone()),
            daemon_url: format!("http://{}", config.bind_addr),
            capability_secret_root,
        }),
        Box::new(GitInspector),
    )
    .with_workspace_repository(Box::new(workspaces)))
}

#[async_trait]
impl SystemApiServer for Daemon {
    async fn status(&self) -> RpcResult<SystemStatusResponse> {
        Ok(SystemStatusResponse::new(
            &self.application.status(),
            self.management.clone(),
        ))
    }
}

#[async_trait]
impl AgentDispatchApiServer for Daemon {
    async fn config_get(
        &self,
        request: forge_rpc::DispatcherConfigGetRequest,
    ) -> RpcResult<forge_rpc::DispatcherConfigResponse> {
        self.application
            .dispatcher_config_get(request.key.as_deref(), request.project.as_deref())
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn config_set(
        &self,
        request: forge_rpc::DispatcherConfigSetRequest,
    ) -> RpcResult<forge_rpc::DispatcherConfigResponse> {
        self.application
            .dispatcher_config_set(&request.key, &request.value, request.project.as_deref())
            .and_then(|()| self.application.dispatcher_config_get(Some(&request.key), request.project.as_deref()))
            .map(|entries| {
                let mut response: forge_rpc::DispatcherConfigResponse = entries.into();
                if request.key == "acknowledgement-timeout-ms" {
                    response.applicability = Some("New immutable Agent Profile revision applies to future Assignments only; existing Assignments retain their bound profiles.".into());
                }
                response
            })
            .map_err(rpc_error)
    }

    async fn config_unset(
        &self,
        request: forge_rpc::DispatcherConfigUnsetRequest,
    ) -> RpcResult<forge_rpc::DispatcherConfigResponse> {
        self.application
            .dispatcher_config_unset(&request.key, request.project.as_deref())
            .and_then(|()| self.application.dispatcher_config_get(Some(&request.key), request.project.as_deref()))
            .map(|entries| {
                let mut response: forge_rpc::DispatcherConfigResponse = entries.into();
                if request.key == "acknowledgement-timeout-ms" {
                    response.applicability = Some("Default restored through a new immutable Agent Profile revision for future Assignments only; existing Assignments retain their bound profiles.".into());
                }
                response
            })
            .map_err(rpc_error)
    }

    async fn profile(&self) -> RpcResult<AgentDispatchProfileResponse> {
        self.application
            .task_agent_profile()
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn dispatch_status(
        &self,
        request: AgentDispatchStatusRequest,
    ) -> RpcResult<AgentDispatchStatusResponse> {
        self.application
            .agent_dispatch_status(request.project_slug.as_deref())
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn reconcile(&self) -> RpcResult<AgentDispatchReconcileResponse> {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let daemon_id = self
            .management
            .daemon_instance_id
            .as_deref()
            .unwrap_or("manual-daemon");
        let result = self
            .application
            .reconcile_agent_dispatch(daemon_id, now_millis);
        // Reconciliation can commit a dispatch failure Comment before returning an error.
        self.delivery.publish(&self.application);
        result.map(Into::into).map_err(rpc_error)
    }

    async fn dispatch_record(
        &self,
        request: DispatchRecordRequest,
    ) -> RpcResult<DispatchRecordResponse> {
        let result = self.application.dispatch_record(request.record_id);
        self.delivery.publish(&self.application);
        result
            .map(|(assignment, already_assigned)| DispatchRecordResponse {
                message: if already_assigned {
                    format!(
                        "Record {} is already assigned to {}",
                        assignment.record_id, assignment.id
                    )
                } else {
                    format!(
                        "Record {} dispatched as {}",
                        assignment.record_id, assignment.id
                    )
                },
                assignment: assignment.into(),
                already_assigned,
            })
            .map_err(rpc_error)
    }

    async fn assignment_acknowledge(
        &self,
        request: forge_rpc::AcknowledgeAssignmentRequest,
    ) -> RpcResult<AgentAssignmentResponse> {
        authenticate_assignment_secret(
            self.capability_secret_root.as_deref(),
            &request.assignment_id,
            &request.secret,
        )?;
        self.application
            .acknowledge_assignment(&request.assignment_id, &request.correlation_id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn assignment_instructions(
        &self,
        request: forge_rpc::AcknowledgeAssignmentRequest,
    ) -> RpcResult<String> {
        authenticate_assignment_secret(
            self.capability_secret_root.as_deref(),
            &request.assignment_id,
            &request.secret,
        )?;
        self.application
            .assignment_instructions(&request.assignment_id, &request.correlation_id)
            .map_err(rpc_error)
    }

    async fn assignment_bootstrap_failure(
        &self,
        request: forge_rpc::AssignmentBootstrapFailureRequest,
    ) -> RpcResult<()> {
        authenticate_assignment_secret(
            self.capability_secret_root.as_deref(),
            &request.assignment_id,
            &request.secret,
        )?;
        self.application
            .report_assignment_bootstrap_failure(
                &request.assignment_id,
                &request.correlation_id,
                &request.message,
            )
            .map(|()| self.delivery.publish(&self.application))
            .map_err(rpc_error)
    }

    async fn assignment_task_start(
        &self,
        request: forge_rpc::AuthenticatedAssignmentTaskStartRequest,
    ) -> RpcResult<RecordResponse> {
        authenticate_assignment_secret(
            self.capability_secret_root.as_deref(),
            &request.assignment_id,
            &request.secret,
        )?;
        self.application
            .start_acknowledged_assignment_task(
                &request.assignment_id,
                &request.correlation_id,
                request.task_id,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn task_start(&self, request: StartAssignmentTaskRequest) -> RpcResult<RecordResponse> {
        self.application
            .start_assignment_task(
                &request.assignment_id,
                &request.capability_id,
                &request.correlation_id,
                request.task_id,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn task_reopen(&self, request: RecordIdRequest) -> RpcResult<RecordResponse> {
        self.application
            .reopen_task(request.id)
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn resume(&self, request: RecordIdRequest) -> RpcResult<()> {
        self.application
            .resume_task_dispatch(request.id)
            .map_err(rpc_error)
    }

    async fn assignment_info(
        &self,
        request: forge_rpc::AssignmentInfoRequest,
    ) -> RpcResult<forge_rpc::AgentAssignmentInfoResponse> {
        self.application
            .agent_assignment_info(&request.assignment_id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn assignment_resolve_unknown(
        &self,
        request: forge_rpc::ResolveUnknownAgentResourcesRequest,
    ) -> RpcResult<forge_rpc::AgentAssignmentInfoResponse> {
        self.application
            .resolve_unknown_agent_resources(
                &request.assignment_id,
                request.confirm_old_agent_cannot_act,
                request.confirm_unknown_resources_safe,
            )
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn assignment_retire(&self, request: RetireAgentAssignmentRequest) -> RpcResult<()> {
        let result = self
            .application
            .retire_agent_assignment(&request.assignment_id);
        // Cleanup errors do not undo revocation. End already-open delivery connections too.
        if self
            .application
            .agent_assignment_info(&request.assignment_id)
            .is_ok_and(|info| info.assignment.closed_at_millis.is_some())
        {
            self.delivery.cancel_assignment(&request.assignment_id);
        }
        result.map(|_| ()).map_err(rpc_error)
    }
}

#[async_trait]
impl WorktreeApiServer for Daemon {
    async fn root_get(&self) -> RpcResult<WorktreeRootResponse> {
        self.application
            .worktree_root()
            .map(|root| WorktreeRootResponse {
                root: root.map(|path| path.to_string_lossy().into_owned()),
            })
            .map_err(rpc_error)
    }
    async fn root_set(&self, request: SetWorktreeRootRequest) -> RpcResult<WorktreeRootResponse> {
        self.application
            .set_worktree_root(request.root.as_deref().map(Path::new))
            .and_then(|()| self.application.worktree_root())
            .map(|root| WorktreeRootResponse {
                root: root.map(|path| path.to_string_lossy().into_owned()),
            })
            .map_err(rpc_error)
    }
    async fn setup_hook_set(&self, request: SetProjectSetupHookRequest) -> RpcResult<()> {
        self.application
            .set_project_setup_hook(&request.project_slug, request.hook.as_deref())
            .map_err(rpc_error)
    }
    async fn create(
        &self,
        request: CreateManagedWorktreeRequest,
    ) -> RpcResult<ManagedWorktreeResponse> {
        let result = self
            .application
            .create_managed_worktree(request.record_id, &request.branch, &request.base)
            .and_then(|_| self.application.managed_worktree_info(request.record_id));
        // Preparation failure can commit a durable Task Comment while returning its error.
        self.delivery.publish(&self.application);
        result.map(Into::into).map_err(rpc_error)
    }
    async fn recover(
        &self,
        request: forge_rpc::RecoverManagedWorktreeRequest,
    ) -> RpcResult<ManagedWorktreeResponse> {
        self.application
            .recover_managed_worktree(request.record_id, request.confirm_no_active_agent)
            .and_then(|_| self.application.managed_worktree_info(request.record_id))
            .map(Into::into)
            .map_err(rpc_error)
    }
    async fn info(&self, request: RecordIdRequest) -> RpcResult<ManagedWorktreeResponse> {
        self.application
            .managed_worktree_info(request.id)
            .map(Into::into)
            .map_err(rpc_error)
    }
    async fn list(
        &self,
        request: ListManagedWorktreesRequest,
    ) -> RpcResult<ListManagedWorktreesResponse> {
        self.application
            .list_managed_worktrees(request.include_all)
            .map(|worktrees| ListManagedWorktreesResponse {
                worktrees: worktrees.into_iter().map(Into::into).collect(),
            })
            .map_err(rpc_error)
    }
    async fn remove(&self, request: RemoveManagedWorktreeRequest) -> RpcResult<()> {
        self.application
            .remove_managed_worktree(request.record_id, request.force)
            .map_err(rpc_error)
    }
    async fn doctor(
        &self,
        request: DoctorManagedWorktreesRequest,
    ) -> RpcResult<ListManagedWorktreesResponse> {
        let worktrees = self
            .application
            .list_managed_worktrees(false)
            .map_err(rpc_error)?;
        if request.repair {
            for worktree in &worktrees {
                self.application
                    .repair_managed_worktree(worktree.claim.record_id)
                    .map_err(rpc_error)?;
            }
        }
        Ok(ListManagedWorktreesResponse {
            worktrees: if request.repair {
                Vec::new()
            } else {
                worktrees.into_iter().map(Into::into).collect()
            },
        })
    }
}

#[async_trait]
impl AgentSubscriptionApiServer for Daemon {
    async fn create(
        &self,
        request: CreateAgentSubscriptionRequest,
    ) -> RpcResult<RecordUpdateSubscriptionResponse> {
        let filters = request
            .kind_filters
            .iter()
            .map(|filter| record_update_kind_filter(filter))
            .collect::<Result<Vec<_>, _>>()?;
        self.application
            .create_assignment_subscription(
                &request.assignment_id,
                &request.capability_id,
                &request.correlation_id,
                request.target_record_ids,
                filters,
            )
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn renew(
        &self,
        request: ManageAgentSubscriptionRequest,
    ) -> RpcResult<RecordUpdateSubscriptionResponse> {
        let id = RecordUpdateSubscriptionId(request.subscription_id);
        self.application
            .renew_assignment_subscription(
                &request.assignment_id,
                &request.capability_id,
                &request.correlation_id,
                id,
            )
            .map(|subscription| {
                self.delivery
                    .renew_subscription(id, subscription.lease_expires_at_millis);
                subscription.into()
            })
            .map_err(rpc_error)
    }

    async fn delete(&self, request: ManageAgentSubscriptionRequest) -> RpcResult<()> {
        let id = RecordUpdateSubscriptionId(request.subscription_id);
        self.application
            .delete_assignment_subscription(
                &request.assignment_id,
                &request.capability_id,
                &request.correlation_id,
                id,
            )
            .inspect(|()| self.delivery.cancel_subscription(id))
            .map_err(rpc_error)
    }
}

#[async_trait]
impl RecordApiServer for Daemon {
    async fn open_initiative(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenRecordRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let application =
            self.record_application(&selection, &mut request.project_scope.project_slugs)?;
        application
            .open_typed_record_with_generators(
                forge_app::RecordType::Initiative,
                &request.body,
                request.project_scope.project_slugs,
                None,
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn ready(
        &self,
        request: forge_rpc::SelectedRecordRequest<forge_rpc::ReadyRecordsRequest>,
    ) -> RpcResult<forge_rpc::ReadyRecordsResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .ready_records(request.planning, request.include_hitl, request.blocked)
            .map(|records| forge_rpc::ReadyRecordsResponse {
                records: records
                    .into_iter()
                    .map(|item| forge_rpc::ReadyRecordResponse {
                        record: item.record.into(),
                        blocked_by: item.blocked_by,
                    })
                    .collect(),
            })
            .map_err(rpc_error)
    }

    async fn open(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenRecordRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let application =
            self.record_application(&selection, &mut request.project_scope.project_slugs)?;
        application
            .open_record_with_generators(
                &request.body,
                request.project_scope.project_slugs,
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn open_spec(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenSpecRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let application =
            self.record_application(&selection, std::slice::from_mut(&mut request.project_slug))?;
        application
            .open_typed_record_with_generators(
                forge_app::RecordType::Spec,
                &request.body,
                vec![request.project_slug],
                None,
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn open_task(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenTaskRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let application =
            self.record_application(&selection, request.project_slug.as_mut_slice())?;
        application
            .open_typed_record_with_generators(
                forge_app::RecordType::Task,
                &request.body,
                request.project_slug.into_iter().collect(),
                request.task_kind.as_deref().map(task_kind).transpose()?,
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn open_wayfinder(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenWayfinderRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let application =
            self.record_application(&selection, &mut request.project_scope.project_slugs)?;
        application
            .open_typed_record_with_generators(
                forge_app::RecordType::Wayfinder,
                &request.body,
                request.project_scope.project_slugs,
                None,
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn open_task_under_spec(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenTaskUnderSpecRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        let application = self.record_application(&selection, &mut [])?;
        application
            .open_task_under_spec(
                &request.body,
                request.spec_id,
                request.task_kind.as_deref().map(task_kind).transpose()?,
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn open_task_under_wayfinder(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenTaskUnderWayfinderRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let application =
            self.record_application(&selection, request.project_slug.as_mut_slice())?;
        application
            .open_task_under_wayfinder(
                &request.body,
                request.wayfinder_id,
                request.project_slug,
                request.task_kind.as_deref().map(task_kind).transpose()?,
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn open_grilling_under_wayfinder(
        &self,
        request: forge_rpc::SelectedRecordRequest<OpenGrillingUnderWayfinderRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let projects = request
            .project_scope
            .as_mut()
            .map_or(&mut [][..], |scope| scope.project_slugs.as_mut_slice());
        let application = self.record_application(&selection, projects)?;
        application
            .open_grilling_under_wayfinder(
                &request.body,
                request.wayfinder_id,
                request.project_scope.map(|scope| scope.project_slugs),
                request.generated_by_ids,
            )
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn show(
        &self,
        request: forge_rpc::SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .show_record(request.id)
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn list(
        &self,
        request: forge_rpc::SelectedRecordRequest<ListRecordsRequest>,
    ) -> RpcResult<ListRecordsResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: mut request,
        } = request;
        let application =
            self.record_application(&selection, request.project_slug.as_mut_slice())?;
        let status = request.status.as_deref().map(record_status).transpose()?;
        let record_type = request
            .record_type
            .as_deref()
            .map(record_type)
            .transpose()?;
        let task_kind = request.task_kind.as_deref().map(task_kind).transpose()?;
        application
            .list_records(request.project_slug, status, record_type, task_kind)
            .map(|records| ListRecordsResponse {
                records: records.into_iter().map(Into::into).collect(),
            })
            .map_err(rpc_error)
    }

    async fn edit(
        &self,
        request: forge_rpc::SelectedRecordRequest<EditRecordRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .edit_record(request.id, &request.body)
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn start(
        &self,
        request: forge_rpc::SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .start_record(request.id)
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn done(
        &self,
        request: forge_rpc::SelectedRecordRequest<DoneRecordRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .done_record(request.id, request.completion_resolution.as_deref())
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn relate(
        &self,
        request: forge_rpc::SelectedRecordRequest<RelateRecordRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        let kind = relationship_kind(&request.kind)?;
        self.record_application(&selection, &mut [])?
            .relate_records(request.from_id, request.to_id, kind)
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn unrelate(
        &self,
        request: forge_rpc::SelectedRecordRequest<RelateRecordRequest>,
    ) -> RpcResult<RecordResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        let kind = relationship_kind(&request.kind)?;
        self.record_application(&selection, &mut [])?
            .unrelate_records(request.from_id, request.to_id, kind)
            .map(|record| {
                self.delivery.publish(&self.application);
                record.into()
            })
            .map_err(rpc_error)
    }

    async fn updates(
        &self,
        request: forge_rpc::SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<ListRecordUpdatesResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .list_record_updates(request.id)
            .map(|updates| ListRecordUpdatesResponse {
                updates: updates.into_iter().map(Into::into).collect(),
            })
            .map_err(rpc_error)
    }

    async fn comment(
        &self,
        request: forge_rpc::SelectedRecordRequest<AddRecordCommentRequest>,
    ) -> RpcResult<forge_rpc::RecordCommentResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .add_record_comment(request.record_id, &request.body, request.author.as_deref())
            .map(|comment| {
                self.delivery.publish(&self.application);
                comment.into()
            })
            .map_err(rpc_error)
    }

    async fn comments(
        &self,
        request: forge_rpc::SelectedRecordRequest<RecordIdRequest>,
    ) -> RpcResult<ListRecordCommentsResponse> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .list_record_comments(request.id)
            .map(|comments| ListRecordCommentsResponse {
                comments: comments.into_iter().map(Into::into).collect(),
            })
            .map_err(rpc_error)
    }

    async fn comment_delete(
        &self,
        request: forge_rpc::SelectedRecordRequest<DeleteRecordCommentRequest>,
    ) -> RpcResult<()> {
        let forge_rpc::SelectedRecordRequest {
            selection,
            record: request,
        } = request;
        self.record_application(&selection, &mut [])?
            .delete_record_comment(request.record_id, request.comment_id)
            .inspect(|()| self.delivery.publish(&self.application))
            .map_err(rpc_error)
    }
}

impl Daemon {
    fn record_application(
        &self,
        selection: &forge_rpc::RecordSelection,
        projects: &mut [String],
    ) -> RpcResult<forge_app::Application> {
        let cwd = selection.cwd.as_deref().map(std::path::Path::new);
        if cwd.is_some_and(|cwd| !cwd.is_absolute()) {
            return Err(rpc_error(RegistrationError::new(
                "invalid-folder",
                "client cwd must be absolute",
            )));
        }
        let mut project_workspace = None;
        for project in projects.iter().filter(|project| project.contains('/')) {
            let reference = forge_app::ProjectReference::parse(project).map_err(rpc_error)?;
            if project_workspace
                .as_ref()
                .is_some_and(|owner| owner != &reference.workspace_id)
            {
                return Err(rpc_error(RegistrationError::new(
                    "cross-workspace-record",
                    "Record Projects must share one Workspace",
                )));
            }
            project_workspace = Some(reference.workspace_id);
        }
        if selection.workspace.is_none() && project_workspace.is_none() && cwd.is_none() {
            return Err(rpc_error(RegistrationError::new(
                "workspace-selection-required",
                "supply a Workspace or an absolute client cwd",
            )));
        }
        let cwd = cwd.unwrap_or_else(|| std::path::Path::new("/"));
        let workspace = self
            .application
            .select_workspace(
                selection.workspace.as_deref(),
                project_workspace.as_deref(),
                cwd,
            )
            .map_err(rpc_error)?;
        for project in projects {
            *project = self
                .application
                .select_project(Some(&workspace.id), Some(project), cwd)
                .map_err(rpc_error)?
                .reference;
        }
        self.application
            .records_in_workspace(&workspace.id)
            .map_err(rpc_error)
    }
}

fn record_update_kind_filter(filter: &str) -> RpcResult<RecordUpdateKindFilter> {
    match filter {
        "created" => Ok(RecordUpdateKindFilter::Created),
        "body-edited" => Ok(RecordUpdateKindFilter::BodyEdited),
        "status-transition" => Ok(RecordUpdateKindFilter::StatusTransition),
        "relationship-added" => Ok(RecordUpdateKindFilter::RelationshipAdded),
        "relationship-removed" => Ok(RecordUpdateKindFilter::RelationshipRemoved),
        "comment-added" => Ok(RecordUpdateKindFilter::CommentAdded),
        "comment-deleted" => Ok(RecordUpdateKindFilter::CommentDeleted),
        _ => Err(rpc_error(RegistrationError::new(
            "invalid-subscription-filter",
            "unknown Record Update filter",
        ))),
    }
}

fn record_status(status: &str) -> RpcResult<RecordStatus> {
    match status {
        "open" => Ok(RecordStatus::Open),
        "in_progress" => Ok(RecordStatus::InProgress),
        "done" => Ok(RecordStatus::Done),
        _ => Err(rpc_error(RegistrationError::new(
            "invalid-status",
            "Record status must be open, in_progress, or done",
        ))),
    }
}

fn task_kind(task_kind: &str) -> RpcResult<TaskKind> {
    match task_kind {
        "research" => Ok(TaskKind::Research),
        "prototype" => Ok(TaskKind::Prototype),
        "review" => Ok(TaskKind::Review),
        _ => Err(rpc_error(RegistrationError::new(
            "invalid-task-kind",
            "Task Kind must be research, prototype, or review",
        ))),
    }
}

fn record_type(record_type: &str) -> RpcResult<RecordType> {
    match record_type {
        "generic" => Ok(RecordType::Generic),
        "spec" => Ok(RecordType::Spec),
        "task" => Ok(RecordType::Task),
        "wayfinder" => Ok(RecordType::Wayfinder),
        "grilling" => Ok(RecordType::Grilling),
        "initiative" => Ok(RecordType::Initiative),
        _ => Err(rpc_error(RegistrationError::new(
            "invalid-record-type",
            "Record type must be generic, spec, task, wayfinder, grilling, or initiative",
        ))),
    }
}

fn relationship_kind(kind: &str) -> RpcResult<forge_app::RecordRelationshipKind> {
    match kind {
        "child-of" => Ok(forge_app::RecordRelationshipKind::ParentChild),
        "depends-on" => Ok(forge_app::RecordRelationshipKind::DependsOn),
        "generated-by" => Ok(forge_app::RecordRelationshipKind::GeneratedBy),
        "initiative" => Ok(forge_app::RecordRelationshipKind::Initiative),
        _ => Err(rpc_error(RegistrationError::new(
            "unsupported-relationship-kind",
            "Relationship kind must be child-of, depends-on, generated-by, or initiative",
        ))),
    }
}

#[async_trait]
impl ProjectApiServer for Daemon {
    async fn register(
        &self,
        request: RegisterProjectRequest,
    ) -> RpcResult<RegisterProjectResponse> {
        if !Path::new(&request.folder).is_absolute() || !Path::new(&request.cwd).is_absolute() {
            return Err(rpc_error(RegistrationError::new(
                "invalid-project-root",
                "Project root and client cwd must be absolute paths",
            )));
        }
        self.application
            .register_selected_project(
                request.workspace.as_deref(),
                Path::new(&request.folder),
                &request.slug,
                Path::new(&request.cwd),
            )
            .map(Into::into)
            .map_err(rpc_error)
    }

    async fn here(&self, request: ProjectHereRequest) -> RpcResult<ProjectHereResponse> {
        if !Path::new(&request.folder).is_absolute() {
            return Err(rpc_error(RegistrationError::new(
                "invalid-folder",
                "client cwd must be an absolute path",
            )));
        }
        self.application
            .select_project(
                request.workspace.as_deref(),
                request.project.as_deref(),
                Path::new(&request.folder),
            )
            .map(Into::into)
            .map_err(rpc_error)
    }
}

async fn subscription_events(
    State(daemon): State<Daemon>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    AxumPath((assignment_id, subscription_id)): AxumPath<(String, u64)>,
    request: Request,
) -> Response {
    if !peer.ip().is_loopback() {
        return StatusCode::FORBIDDEN.into_response();
    }
    let id = RecordUpdateSubscriptionId(subscription_id);
    let subscription = match daemon
        .application
        .assignment_subscription_delivery(&assignment_id, id)
    {
        Ok(subscription) => subscription,
        Err(error) if error.code == "subscription-not-found" => {
            return StatusCode::NOT_FOUND.into_response();
        }
        Err(error) if error.code == "subscription-expired" => {
            return StatusCode::GONE.into_response();
        }
        Err(error) if error.code == "assignment-subscription-mismatch" => {
            return StatusCode::FORBIDDEN.into_response();
        }
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };
    let Ok(cursor) = subscription_cursor(&request, subscription.start_cursor) else {
        return (StatusCode::BAD_REQUEST, "invalid-cursor").into_response();
    };
    subscription_event_stream(&daemon, id, &subscription, cursor)
}

fn subscription_cursor(
    request: &Request,
    start_cursor: RecordUpdateId,
) -> Result<RecordUpdateId, ()> {
    let header_cursor = request
        .headers()
        .get("last-event-id")
        .and_then(|value| value.to_str().ok());
    let query_cursor = request.uri().query().and_then(|query| {
        query
            .split('&')
            .find_map(|part| part.strip_prefix("after="))
    });
    let parse_cursor = |value: &str| value.parse::<u64>().ok().map(RecordUpdateId);
    let header_cursor = header_cursor.map(parse_cursor);
    let query_cursor = query_cursor.map(parse_cursor);
    if header_cursor.is_some_and(|cursor| cursor.is_none())
        || query_cursor.is_some_and(|cursor| cursor.is_none())
        || matches!((header_cursor.flatten(), query_cursor.flatten()), (Some(left), Some(right)) if left != right)
    {
        return Err(());
    }
    Ok(header_cursor
        .flatten()
        .or(query_cursor.flatten())
        .unwrap_or(start_cursor))
}

fn subscription_event_stream(
    daemon: &Daemon,
    id: RecordUpdateSubscriptionId,
    subscription: &forge_app::RecordUpdateSubscription,
    cursor: RecordUpdateId,
) -> Response {
    if cursor < subscription.start_cursor {
        return match daemon.application.record_update_head() {
            Ok(head) => resync_required(id, subscription.start_cursor, head),
            Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        };
    }
    let (connection_id, receiver, mut cancellation, mut lease_expires_at_millis, replay) =
        match daemon
            .delivery
            .connect_and_replay(&daemon.application, subscription, cursor)
        {
            Ok(connection) => connection,
            Err(error) if error.code == "invalid-cursor" => {
                return match daemon.application.record_update_head() {
                    Ok(head) => resync_required(id, subscription.start_cursor, head),
                    Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                };
            }
            Err(error) if error.code == "subscription-expired" => {
                return StatusCode::GONE.into_response();
            }
            Err(_) => return StatusCode::NOT_FOUND.into_response(),
        };
    let initial = std::iter::once(Ok::<Event, Infallible>(
        Event::default().retry(Duration::from_millis(1_000)),
    ))
    .chain(
        replay
            .into_iter()
            .map(move |update| Ok(record_update_event(id, &update))),
    );
    let stream = tokio_stream::iter(initial)
        .chain(
            ReceiverStream::new(receiver).map(move |update| Ok(record_update_event(id, &update))),
        )
        .take_until({
            let delivery = daemon.delivery.clone();
            async move {
                loop {
                    let deadline = Duration::from_millis(
                        lease_expires_at_millis
                            .borrow()
                            .saturating_sub(now_millis()),
                    );
                    tokio::select! {
                        () = time::sleep(deadline) => {
                            delivery.cancel_current_connection(id, connection_id);
                            return;
                        }
                        changed = cancellation.changed() => {
                            let _ = changed;
                            return;
                        }
                        changed = lease_expires_at_millis.changed() => {
                            if changed.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
        });
    let mut response = Sse::new(stream)
        .keep_alive(
            axum::response::sse::KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keepalive"),
        )
        .into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache, no-transform"),
    );
    response
        .headers_mut()
        .insert("x-accel-buffering", header::HeaderValue::from_static("no"));
    response
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn resync_required(
    id: RecordUpdateSubscriptionId,
    start: RecordUpdateId,
    head: RecordUpdateId,
) -> Response {
    (
        StatusCode::CONFLICT,
        [(header::CONTENT_TYPE, "application/json")],
        format!("{{\"code\":\"resync-required\",\"subscription_id\":{},\"retention_floor\":0,\"start_after_update_id\":{},\"head\":{}}}", id.0, start.0, head.0),
    ).into_response()
}

fn record_update_event(
    subscription_id: RecordUpdateSubscriptionId,
    envelope: &RecordUpdateEnvelope,
) -> Event {
    let (kind, extra) = match envelope.update.kind {
        RecordUpdateKind::Created => ("created", String::new()),
        RecordUpdateKind::BodyEdited => ("body-edited", String::new()),
        RecordUpdateKind::StatusTransition { from, to } => (
            "status-transition",
            format!(",\"from_status\":\"{from:?}\",\"to_status\":\"{to:?}\""),
        ),
        RecordUpdateKind::RelationshipAdded {
            related_record_id,
            relationship_kind,
        } => (
            "relationship-added",
            format!(
                ",\"related_record_id\":{related_record_id}{}",
                relationship_kind.map_or(String::new(), |kind| format!(
                    ",\"relationship_kind\":\"{}\"",
                    relationship_kind_name(kind)
                ))
            ),
        ),
        RecordUpdateKind::RelationshipRemoved {
            related_record_id,
            relationship_kind,
        } => (
            "relationship-removed",
            format!(
                ",\"related_record_id\":{related_record_id}{}",
                relationship_kind.map_or(String::new(), |kind| format!(
                    ",\"relationship_kind\":\"{}\"",
                    relationship_kind_name(kind)
                ))
            ),
        ),
        RecordUpdateKind::CommentAdded { comment_id } => {
            ("comment-added", format!(",\"comment_id\":{}", comment_id.0))
        }
        RecordUpdateKind::CommentDeleted { comment_id } => (
            "comment-deleted",
            format!(",\"comment_id\":{}", comment_id.0),
        ),
    };
    Event::default()
        .id(envelope.update.id.0.to_string())
        .event("record-update")
        .data(format!("{{\"subscription_id\":{},\"record_id\":{},\"record_update_id\":{},\"kind\":\"{kind}\",\"timestamp\":\"{}\"{} }}", subscription_id.0, envelope.record_id, envelope.update.id.0, rfc3339_utc_timestamp(envelope.update.created_at_millis), extra))
}

fn relationship_kind_name(kind: forge_app::RecordRelationshipKind) -> &'static str {
    match kind {
        forge_app::RecordRelationshipKind::ParentChild => "parent-child",
        forge_app::RecordRelationshipKind::DependsOn => "depends-on",
        forge_app::RecordRelationshipKind::GeneratedBy => "generated-by",
        forge_app::RecordRelationshipKind::Initiative => "initiative",
    }
}

fn rfc3339_utc_timestamp(millis: u64) -> String {
    let seconds = millis / 1_000;
    let milliseconds = millis % 1_000;
    let days = seconds / 86_400;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_date_from_days_since_unix_epoch(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{milliseconds:03}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60,
    )
}

fn civil_date_from_days_since_unix_epoch(days: u64) -> (u64, u64, u64) {
    // Civil-date conversion from Howard Hinnant's public-domain algorithm.
    let days = days + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    year += u64::from(month <= 2);
    (year, month, day)
}

fn rpc_error(error: RegistrationError) -> jsonrpsee::types::ErrorObjectOwned {
    jsonrpsee::types::ErrorObjectOwned::owned(-32001, error.message, Some(error.code))
}

/// Builds the single listener's router, including the browser and typed RPC services.
///
/// # Panics
///
/// Panics if the statically defined RPC method sets overlap.
pub fn router(daemon: Daemon) -> Router {
    let page_daemon = daemon.clone();
    let status_daemon = daemon.clone();
    let subscription_daemon = daemon.clone();
    let mut methods = SystemApiServer::into_rpc(daemon.clone());
    methods
        .merge(forge_rpc::WorkspaceApiServer::into_rpc(daemon.clone()))
        .expect("Workspace RPC methods are unique");
    methods
        .merge(ProjectApiServer::into_rpc(daemon.clone()))
        .expect("RPC method names must not overlap");
    methods
        .merge(AgentDispatchApiServer::into_rpc(daemon.clone()))
        .expect("RPC method names must not overlap");
    methods
        .merge(AgentSubscriptionApiServer::into_rpc(daemon.clone()))
        .expect("RPC method names must not overlap");
    methods
        .merge(WorktreeApiServer::into_rpc(daemon.clone()))
        .expect("RPC method names must not overlap");
    methods
        .merge(RecordApiServer::into_rpc(daemon))
        .expect("RPC method names must not overlap");
    let service_builder = Server::builder().to_service_builder();
    let (stop_handle, server_handle) = stop_channel();
    let server_handle = Arc::new(server_handle);

    Router::new()
        .route(
            "/v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events",
            get(subscription_events).with_state(subscription_daemon),
        )
        .route(
            "/",
            get(move || {
                let daemon = page_daemon.clone();
                async move { status_page(&daemon.application.status()) }
            }),
        )
        .route(
            "/status",
            get(move || {
                let daemon = status_daemon.clone();
                async move { status_fragment(&daemon.application.status()) }
            }),
        )
        .route(
            "/assets/htmx.min.js",
            get(|| async {
                (
                    [(
                        header::CONTENT_TYPE,
                        "application/javascript; charset=utf-8",
                    )],
                    include_str!("../assets/htmx.min.js"),
                )
            }),
        )
        .route(
            "/rpc",
            post(move |request: Request| {
                let rpc_service = service_builder
                    .clone()
                    .build(methods.clone(), stop_handle.clone());
                let server_handle = Arc::clone(&server_handle);
                async move {
                    let _keep_server_handle_alive = server_handle;
                    rpc_service
                        .oneshot(request)
                        .await
                        .map(|response| response.map(Body::new))
                        .map_err(|error| {
                            warn!(%error, "RPC service request failed");
                            StatusCode::INTERNAL_SERVER_ERROR
                        })
                }
            }),
        )
}

#[derive(Template)]
#[template(
    source = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Forge daemon status</title>
  <script src="/assets/htmx.min.js"></script>
</head>
<body>
  <main>
    <h1>Forge daemon</h1>
    {{ status_html|safe }}
  </main>
</body>
</html>"##,
    ext = "html"
)]
struct StatusPageTemplate<'a> {
    status_html: &'a str,
}

#[derive(Template)]
#[template(
    source = r##"<section id="status" aria-live="polite">
  <p><strong>{{ status.name }}</strong> {{ status.version }}</p>
  <p>Ready: {{ status.ready }}</p>
  <button type="button" hx-get="/status" hx-target="#status" hx-swap="outerHTML">Refresh status</button>
</section>"##,
    ext = "html"
)]
struct StatusFragmentTemplate<'a> {
    status: &'a ApplicationStatus,
}

fn status_page(status: &ApplicationStatus) -> Result<Html<String>, StatusCode> {
    let status_html = StatusFragmentTemplate { status }
        .render()
        .map_err(|error| {
            warn!(%error, "failed to render status fragment");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    StatusPageTemplate {
        status_html: &status_html,
    }
    .render()
    .map(Html)
    .map_err(|error| {
        warn!(%error, "failed to render status page");
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

fn status_fragment(status: &ApplicationStatus) -> Result<Html<String>, StatusCode> {
    StatusFragmentTemplate { status }
        .render()
        .map(Html)
        .map_err(|error| {
            warn!(%error, "failed to render status fragment");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

/// Runs the daemon until Ctrl-C or SIGTERM requests graceful shutdown.
///
/// # Errors
///
/// Returns an error when another daemon owns the configured store, or when the
/// listener cannot bind or serve connections.
pub async fn serve(config: DaemonConfig) -> Result<(), std::io::Error> {
    let _store_lock = StoreOwnershipLock::acquire(&config.store_path)?;
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        warn!(%address, "remote daemon binding is unauthenticated; use only a trusted private network");
    }
    info!(%address, "Forge daemon listening");

    let application = application_from_config(&config).map_err(std::io::Error::other)?;
    let daemon_instance_id = config
        .managed
        .as_ref()
        .map(|managed| managed.daemon_instance_id.clone());
    let mut daemon = Daemon::with_daemon_instance_id(application, daemon_instance_id);
    let store_root = config.store_path.parent().unwrap_or_else(|| Path::new("."));
    daemon.capability_secret_root = Some(
        if store_root.is_absolute() {
            store_root.to_owned()
        } else {
            env::current_dir()?.join(store_root)
        }
        .join("assignment-capabilities"),
    );
    let (observer_shutdown, observer_shutdown_receiver) = watch::channel(false);
    let (reconcile_wake, mut reconcile_wakes) = mpsc::channel(1);
    let observer = tokio::spawn(observe_status_changes(
        config.store_path.clone(),
        reconcile_wake,
        observer_shutdown_receiver.clone(),
    ));
    let reconciliation_daemon = daemon.clone();
    let reconciliation_worker = tokio::spawn(async move {
        let mut shutdown = observer_shutdown_receiver;
        // The observer makes provider status changes prompt; this bounded fallback also picks up
        // Task, dependency, and capacity mutations made through any transport or while Herdr is
        // disconnected.
        let mut periodic_reconcile = time::interval(Duration::from_secs(5));
        periodic_reconcile.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                wake = reconcile_wakes.recv() => {
                    if wake.is_none() { return; }
                    reconcile_daemon(&reconciliation_daemon).await;
                }
                _ = periodic_reconcile.tick() => {
                    reconcile_daemon(&reconciliation_daemon).await;
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() { return; }
                }
            }
        }
    });
    let lifecycle_shutdown = match config.managed.as_ref() {
        Some(managed) => Some(bind_lifecycle_control(managed)?),
        None => None,
    };
    let result = axum::serve(
        listener,
        router(daemon).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(wait_for_shutdown(lifecycle_shutdown))
    .await;
    let _ = observer_shutdown.send(true);
    let _ = observer.await;
    let _ = reconciliation_worker.await;
    result
}

/// Retains the exclusive advisory lock that gives a daemon sole ownership of a store.
///
/// The lock is intentionally separate from SQLite itself: direct SQLite writers remain
/// possible, while a second Forge daemon is rejected before it can bind a listener.
#[derive(Debug)]
struct StoreOwnershipLock {
    _file: fs::File,
}

impl StoreOwnershipLock {
    fn acquire(store_path: &Path) -> io::Result<Self> {
        let mut lock_path = store_path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock_path = PathBuf::from(lock_path);
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        file.try_lock_exclusive().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "could not acquire exclusive daemon ownership lock `{}` for store `{}`: {error}",
                    lock_path.display(),
                    store_path.display()
                ),
            )
        })?;
        Ok(Self { _file: file })
    }
}

async fn reconcile_daemon(daemon: &Daemon) {
    let daemon = daemon.clone();
    let result = tokio::task::spawn_blocking(move || {
        let now_millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let result = daemon
            .application
            .reconcile_agent_dispatch("herdr-observer", now_millis);
        daemon.delivery.publish(&daemon.application);
        result
    })
    .await;
    match result {
        Ok(Err(error)) => warn!(code = error.code, "agent dispatch reconciliation failed"),
        Err(_) => warn!("agent dispatch reconciliation task failed"),
        Ok(Ok(_)) => {}
    }
}

async fn wait_for_shutdown(lifecycle_shutdown: Option<LifecycleControl>) {
    match lifecycle_shutdown {
        Some(control) => tokio::select! {
            () = shutdown_signal() => {},
            result = control.wait_for_authenticated_shutdown() => match result {
                Ok(()) => info!("authenticated lifecycle shutdown received; shutting down Forge daemon"),
                Err(error) => warn!(%error, "lifecycle control endpoint failed; shutting down Forge daemon"),
            },
        },
        None => shutdown_signal().await,
    }
}

/// A per-managed-daemon local lifecycle listener. It is deliberately separate
/// from the public HTTP/JSON-RPC listener.
enum LifecycleControl {
    #[cfg(unix)]
    Unix {
        listener: tokio::net::UnixListener,
        endpoint: PathBuf,
        credential: String,
    },
    #[cfg(windows)]
    Windows {
        endpoint: String,
        credential: String,
    },
}

impl LifecycleControl {
    async fn wait_for_authenticated_shutdown(self) -> io::Result<()> {
        match self {
            #[cfg(unix)]
            Self::Unix {
                listener,
                endpoint,
                credential,
            } => {
                struct Cleanup(PathBuf);
                impl Drop for Cleanup {
                    fn drop(&mut self) {
                        if let Err(error) = fs::remove_file(&self.0)
                            && error.kind() != io::ErrorKind::NotFound
                        {
                            warn!(%error, endpoint = %self.0.display(), "failed to remove lifecycle control endpoint");
                        }
                    }
                }
                let _cleanup = Cleanup(endpoint);
                loop {
                    let (mut stream, _) = listener.accept().await?;
                    if credential_matches(&mut stream, &credential).await? {
                        return Ok(());
                    }
                    warn!("rejected lifecycle shutdown request with invalid credential");
                }
            }
            #[cfg(windows)]
            Self::Windows {
                endpoint,
                credential,
            } => {
                use tokio::net::windows::named_pipe::ServerOptions;
                loop {
                    let mut pipe = ServerOptions::new().create(&endpoint)?;
                    pipe.connect().await?;
                    if credential_matches(&mut pipe, &credential).await? {
                        return Ok(());
                    }
                    warn!("rejected lifecycle shutdown request with invalid credential");
                }
            }
        }
    }
}

async fn credential_matches<R: tokio::io::AsyncRead + Unpin>(
    reader: &mut R,
    credential: &str,
) -> io::Result<bool> {
    let mut request = Vec::with_capacity(credential.len() + 1);
    loop {
        let mut byte = [0_u8; 1];
        if reader.read(&mut byte).await? == 0 {
            return Ok(false);
        }
        if byte[0] == b'\n' {
            break;
        }
        if request.len() >= 4096 {
            return Ok(false);
        }
        request.push(byte[0]);
    }
    Ok(request == credential.as_bytes())
}

fn bind_lifecycle_control(managed: &ManagedDaemonConfig) -> io::Result<LifecycleControl> {
    #[cfg(unix)]
    {
        let listener = tokio::net::UnixListener::bind(&managed.lifecycle_control_endpoint)?;
        info!(endpoint = %managed.lifecycle_control_endpoint.display(), "Forge lifecycle control listening");
        Ok(LifecycleControl::Unix {
            listener,
            endpoint: managed.lifecycle_control_endpoint.clone(),
            credential: managed.lifecycle_credential.clone(),
        })
    }
    #[cfg(windows)]
    {
        let endpoint = managed
            .lifecycle_control_endpoint
            .to_string_lossy()
            .into_owned();
        if endpoint.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "lifecycle control endpoint must not be empty",
            ));
        }
        Ok(LifecycleControl::Windows {
            endpoint,
            credential: managed.lifecycle_credential.clone(),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = managed;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "local lifecycle control is unsupported on this platform",
        ))
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("Ctrl-C received; shutting down Forge daemon"),
        () = terminate => info!("SIGTERM received; shutting down Forge daemon"),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };

    #[cfg(unix)]
    use tokio::io::AsyncWriteExt;

    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{Request, StatusCode, header},
        response::{IntoResponse, Sse},
    };
    use forge_app::{
        AgentDispatchRepository, Application, NewAgentAssignment, NewRecordUpdateSubscription,
        NewTypedRecord, Project, RecordRelationshipKind, RecordRepository, RecordStatus,
        RecordType, RecordUpdate, RecordUpdateEnvelope, RecordUpdateId, RecordUpdateKind,
        RecordUpdateSubscriptionId, RecordUpdateSubscriptionRepository, RegistrationError,
        RepositoryInspector,
    };
    use forge_rpc::{
        AgentDispatchApiServer, AgentSubscriptionApiServer, CreateAgentSubscriptionRequest,
        DoneRecordRequest, EditRecordRequest, ListRecordsRequest,
        OpenGrillingUnderWayfinderRequest, OpenRecordRequest, OpenSpecRequest, OpenTaskRequest,
        OpenTaskUnderSpecRequest, OpenTaskUnderWayfinderRequest, OpenWayfinderRequest,
        ProjectApiClient, ProjectHereRequest, ProjectScopeRequest, RecordApiClient,
        RecordIdRequest, RegisterProjectRequest, StartAssignmentTaskRequest, SystemApiClient,
        SystemApiServer,
    };
    use forge_store::SqliteProjectRepository;
    use futures_util::StreamExt;
    use jsonrpsee::http_client::HttpClientBuilder;
    use tower::ServiceExt;

    use super::{
        DEFAULT_BIND_ADDR, Daemon, DaemonConfig, ManagedLaunchInputs, StoreOwnershipLock, router,
        serve, write_assignment_capability_secret,
    };

    #[test]
    fn capability_secret_files_are_owner_restricted_and_outside_the_workspace() {
        let root = std::env::temp_dir().join(format!(
            "forge-capability-secret-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = root.join("workspace");
        let secret_path = root.join("secrets/capability");
        std::fs::create_dir_all(&workspace).unwrap();
        write_assignment_capability_secret(&secret_path, &workspace, "secret-value").unwrap();
        assert_eq!(
            std::fs::read_to_string(&secret_path).unwrap(),
            "secret-value\n"
        );
        assert_eq!(
            write_assignment_capability_secret(&workspace.join("capability"), &workspace, "other")
                .unwrap_err()
                .code,
            "invalid-capability-secret-path"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&secret_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    struct TestInspector;

    impl RepositoryInspector for TestInspector {
        fn inspect(&self, folder: &Path) -> Result<Project, RegistrationError> {
            Ok(Project {
                reference: String::new(),
                workspace_id: String::new(),
                slug: String::new(),
                project_root: PathBuf::from(folder),
                origin_url: "https://example.test/origin.git".into(),
            })
        }

        fn head_commit(&self, _: &Path) -> Result<String, RegistrationError> {
            Ok("test-base".into())
        }
    }

    #[derive(Clone)]
    struct RecordingDispatchAdapter(
        std::sync::Arc<std::sync::Mutex<Vec<forge_app::AgentDispatchContext>>>,
    );

    impl forge_app::AgentDispatchAdapterFactory for RecordingDispatchAdapter {
        fn adapter_for(
            &self,
            _: &forge_app::AgentProfile,
        ) -> Result<Box<dyn forge_app::AgentDispatchAdapter>, forge_app::AgentAdapterError>
        {
            Ok(Box::new(self.clone()))
        }
    }

    impl forge_app::AgentDispatchAdapter for RecordingDispatchAdapter {
        fn probe(&self) -> Result<(), forge_app::AgentAdapterError> {
            Ok(())
        }
        fn provision(
            &self,
            context: &forge_app::AgentDispatchContext,
            _: &dyn AgentDispatchRepository,
        ) -> Result<(), forge_app::AgentAdapterError> {
            self.0.lock().unwrap().push(context.clone());
            Ok(())
        }
        fn observe(
            &self,
            _: &forge_app::AgentDispatchContext,
        ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
            Ok(forge_app::AgentObservation::Working)
        }
        fn recover(
            &self,
            context: &forge_app::AgentDispatchContext,
        ) -> Result<forge_app::AgentObservation, forge_app::AgentAdapterError> {
            self.observe(context)
        }
        fn retire(
            &self,
            _: &forge_app::AgentDispatchContext,
            _: &dyn AgentDispatchRepository,
        ) -> Result<(), forge_app::AgentAdapterError> {
            Ok(())
        }
    }

    fn explicit_dispatch_fixture() -> (PathBuf, SqliteProjectRepository, Vec<forge_app::Record>) {
        use forge_app::ManagedWorktreeRepository;
        let root = std::env::temp_dir().join(format!(
            "forge-explicit-app-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        super::git(&repo, &["init"]).unwrap();
        super::git(
            &repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        )
        .unwrap();
        let base = super::git(&repo, &["rev-parse", "HEAD"]).unwrap();
        let database = root.join("store.sqlite");
        let store = SqliteProjectRepository::open(&database).unwrap();
        super::test_projects::register(&store, "alpha", Some(&repo));
        store
            .set_worktree_root(Some(&root.join("worktrees")))
            .unwrap();
        store
            .set_project_task_concurrency_limit("alpha", Some(2))
            .unwrap();
        let mut records = Vec::new();
        for kind in [RecordType::Task, RecordType::Task, RecordType::Grilling] {
            let record = store
                .open_typed_record(&NewTypedRecord {
                    record_type: kind,
                    task_kind: None,
                    body: "work".into(),
                    project_slugs: vec!["alpha".into()],
                    generated_by_ids: vec![],
                })
                .unwrap();
            if kind == RecordType::Task {
                store.set_task_base_commit(record.id, base.trim()).unwrap();
            }
            records.push(record);
        }
        (root, store, records)
    }

    #[test]
    fn explicit_dispatch_prepares_only_task_worktrees_and_never_relaunches_duplicates() {
        use forge_app::AgentAssignmentState;
        let (root, store, records) = explicit_dispatch_fixture();
        let database = root.join("store.sqlite");
        let launches = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let app = Application::with_managed_worktrees_and_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&database).unwrap()),
            Box::new(SqliteProjectRepository::open(&database).unwrap()),
            Box::new(super::GitWorktreeAdapter),
            Box::new(SqliteProjectRepository::open(&database).unwrap()),
            Box::new(RecordingDispatchAdapter(launches.clone())),
            Box::new(TestInspector),
        );
        let (task_assignment, existing) = app.dispatch_record(records[1].id).unwrap();
        assert!(!existing);
        assert_eq!(task_assignment.state, AgentAssignmentState::Provisioning);
        assert!(
            store
                .list_agent_assignments(records[0].id)
                .unwrap()
                .is_empty()
        );
        let barrier = std::sync::Barrier::new(4);
        let concurrent = std::thread::scope(|scope| {
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        app.dispatch_record(records[2].id).unwrap()
                    })
                })
                .collect();
            threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            concurrent.iter().filter(|(_, existing)| !existing).count(),
            1
        );
        let grilling_assignment = &concurrent.iter().find(|(_, existing)| !existing).unwrap().0;
        assert!(
            concurrent
                .iter()
                .all(|(assignment, _)| assignment.id == grilling_assignment.id)
        );
        assert_eq!(grilling_assignment.state, AgentAssignmentState::Working);
        for (record, expected) in [
            (records[1].id, task_assignment.id),
            (records[2].id, grilling_assignment.id.clone()),
        ] {
            let (assignment, existing) = app.dispatch_record(record).unwrap();
            assert!(existing);
            assert_eq!(assignment.id, expected);
        }
        let contexts = launches.lock().unwrap();
        assert_eq!(contexts.len(), 2);
        assert!(
            contexts[0]
                .agent_workspace
                .as_ref()
                .unwrap()
                .join(".git")
                .exists()
        );
        assert!(
            contexts[0]
                .rendered_initial_prompt
                .contains("Managed Worktree")
        );
        assert!(contexts[1].agent_workspace.is_none());
        assert_eq!(
            store.show_record(records[1].id).unwrap().status,
            RecordStatus::Open
        );
        drop(contexts);
        drop(app);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_validates_repository_branch_path_and_base() {
        use forge_app::ManagedWorktreeAdapter;
        let root = std::env::temp_dir().join(format!(
            "forge-recovery-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let repo = root.join("repo");
        let tree = root.join("tree");
        std::fs::create_dir_all(&repo).unwrap();
        super::git(&repo, &["init"]).unwrap();
        super::git(
            &repo,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "base",
            ],
        )
        .unwrap();
        let base = super::git(&repo, &["rev-parse", "HEAD"]).unwrap();
        super::git(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                "feat/forge-2",
                tree.to_str().unwrap(),
                &base,
            ],
        )
        .unwrap();
        let adapter = super::GitWorktreeAdapter;
        adapter
            .validate_recovery(&repo, &tree, "feat/forge-2", &base)
            .unwrap();
        assert!(
            adapter
                .validate_recovery(&repo, &tree, "wrong", &base)
                .is_err()
        );
        assert!(
            adapter
                .validate_recovery(&repo, &tree, "feat/forge-2", "bad-base")
                .is_err()
        );
        assert!(
            adapter
                .validate_recovery(&repo, &tree.join("missing"), "feat/forge-2", &base)
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_worktree_safety_reports_missing_and_invalid_checkouts() {
        use forge_app::ManagedWorktreeAdapter;

        let adapter = super::GitWorktreeAdapter;
        let missing = adapter.assess(Path::new("/definitely-not-a-forge-worktree"));
        assert_eq!(missing.facts, ["checkout-missing"]);
        assert_eq!(missing.blockers, ["checkout-missing"]);
        assert!(!missing.removable);

        let directory = std::env::temp_dir().join(format!(
            "forge-invalid-worktree-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let invalid = adapter.assess(&directory);
        assert_eq!(invalid.facts, ["git-metadata-invalid"]);
        assert_eq!(invalid.blockers, ["git-metadata-invalid"]);
        adapter.remove(&directory, true).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn provider_configuration_verifies_workspace_and_rechecks_safety_after_provider_call() {
        use forge_app::{AgentAssignmentClosureReason, AgentDispatchRepository};
        use std::os::unix::net::UnixListener;
        let root = std::env::temp_dir().join(format!(
            "forge-config-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("forge.sqlite3");
        let socket = root.join("herdr.sock");
        let store = SqliteProjectRepository::open(&database).unwrap();
        super::test_projects::register(&store, "alpha", None);
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "work".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let provider = workspace_selection_server(listener, database.clone(), task.id);
        let app = super::application_from_config(&super::DaemonConfig {
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            store_path: database,
            managed: None,
        })
        .unwrap();
        verify_workspace_configuration(&app, &store, &socket);
        provider.join().unwrap();
        store
            .close_agent_assignment("concurrent", AgentAssignmentClosureReason::Crashed)
            .unwrap();
        verify_unconfigured_dispatch(&app, &store);
        drop(app);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    fn workspace_selection_server(
        listener: std::os::unix::net::UnixListener,
        provider_database: PathBuf,
        task_id: u64,
    ) -> std::thread::JoinHandle<()> {
        use std::io::{BufRead, BufReader, Write};
        std::thread::spawn(move || {
            for (requested, returned) in [
                ("selected", "selected"),
                ("missing", "different"),
                ("replacement", "replacement"),
            ] {
                let mut stream = listener.accept().unwrap().0;
                let mut line = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                let request: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert_eq!(request["method"], "workspace.get");
                assert_eq!(request["params"]["workspace_id"], requested);
                if requested == "replacement" {
                    // A concurrent provisioner reserves an Assignment after provider verification
                    // begins. Persisting the selection must recheck in storage, not trust a preflight.
                    let other = SqliteProjectRepository::open(&provider_database).unwrap();
                    other
                        .create_agent_assignment(&NewAgentAssignment {
                            id: "concurrent".into(),
                            record_id: task_id,
                            project_slug: "alpha".into(),
                            profile_revision: 1,
                        })
                        .unwrap();
                }
                serde_json::to_writer(
                    &mut stream,
                    &serde_json::json!({"result":{"workspace":{"workspace_id":returned}}}),
                )
                .unwrap();
                stream.write_all(b"\n").unwrap();
            }
        })
    }

    #[cfg(unix)]
    fn verify_workspace_configuration(
        app: &Application,
        store: &SqliteProjectRepository,
        socket: &Path,
    ) {
        app.dispatcher_config_set("herdr-socket", socket.to_str().unwrap(), None)
            .unwrap();
        app.dispatcher_config_set("workspace", "selected", None)
            .unwrap();
        assert!(
            app.dispatcher_config_set("workspace", "missing", None)
                .is_err()
        );
        assert_eq!(store.dispatch_workspace().unwrap().unwrap().id, "selected");
        assert_eq!(
            app.dispatcher_config_set("workspace", "replacement", None)
                .unwrap_err()
                .code,
            "dispatch-workspace-busy"
        );
        assert!(
            app.dispatcher_config_set("herdr-socket", "/tmp/other.sock", None)
                .is_err()
        );
        assert!(app.dispatcher_config_unset("workspace", None).is_err());
        assert!(app.dispatcher_config_unset("herdr-socket", None).is_err());
        assert_eq!(store.dispatch_workspace().unwrap().unwrap().id, "selected");
        assert_eq!(store.herdr_socket_path().unwrap().as_deref(), Some(socket));
    }

    #[cfg(unix)]
    fn verify_unconfigured_dispatch(app: &Application, store: &SqliteProjectRepository) {
        app.dispatcher_config_unset("workspace", None).unwrap();
        app.dispatcher_config_unset("herdr-socket", None).unwrap();
        assert!(
            app.dispatcher_config_get(None, None)
                .unwrap()
                .iter()
                .filter(|entry| entry.key != "acknowledgement-timeout-ms")
                .all(|entry| entry.value.is_none() && entry.provenance == "default")
        );
        let grilling = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Grilling,
                task_kind: None,
                body: "question".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        assert!(app.dispatch_record(grilling.id).is_err());
        let assignment = store
            .list_agent_assignments(grilling.id)
            .unwrap()
            .pop()
            .unwrap();
        assert!(
            store
                .list_agent_assignment_resources(&assignment.id)
                .unwrap()
                .is_empty()
        );
    }

    fn test_application() -> Application {
        let db = std::env::temp_dir().join(format!(
            "forge-server-rpc-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let workspace = SqliteProjectRepository::open(&db).unwrap();
        forge_app::WorkspaceRepository::create_workspace(
            &workspace,
            "test",
            "Test",
            Path::new("/"),
        )
        .unwrap();
        Application::new(
            Box::new(SqliteProjectRepository::open(&db).unwrap()),
            Box::new(TestInspector),
        )
        .with_workspace_repository(Box::new(workspace))
    }

    struct SubscriptionTestDaemon {
        daemon: Daemon,
        database_path: PathBuf,
    }

    impl SubscriptionTestDaemon {
        fn daemon(&self) -> Daemon {
            self.daemon.clone()
        }
    }

    impl Drop for SubscriptionTestDaemon {
        fn drop(&mut self) {
            let daemon = std::mem::replace(&mut self.daemon, Daemon::new(test_application()));
            drop(daemon);
            let _ = std::fs::remove_file(&self.database_path);
        }
    }

    fn subscription_test_daemon() -> (SubscriptionTestDaemon, String, u64, u64, u64) {
        let database_path = std::env::temp_dir().join(format!(
            "forge-subscription-endpoint-{}-{}.sqlite3",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&database_path).unwrap();
        super::test_projects::register(&store, "alpha", None);
        let owner = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "owner".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let target = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                task_kind: None,
                body: "target".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        store
            .relate_records(owner.id, target.id, RecordRelationshipKind::DependsOn)
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "owner-assignment".into(),
                record_id: owner.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "target-assignment".into(),
                record_id: target.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        let subscription = store
            .create_record_update_subscription(&NewRecordUpdateSubscription {
                assignment_id: "owner-assignment".into(),
                target_record_ids: vec![target.id],
                kind_filters: vec![],
            })
            .unwrap();
        store.edit_record(target.id, "delivered update").unwrap();
        drop(store);
        let records = SqliteProjectRepository::open(&database_path).unwrap();
        let dispatch = SqliteProjectRepository::open(&database_path).unwrap();
        (
            SubscriptionTestDaemon {
                daemon: Daemon::new(Application::with_agent_dispatch(
                    Box::new(records),
                    Box::new(dispatch),
                    Box::new(super::ServerAgentDispatchAdapterFactory {
                        herdr: super::HerdrAdapterFactory::from_configuration_store(
                            database_path.clone(),
                        ),
                        daemon_url: "http://127.0.0.1:45003".into(),
                        capability_secret_root: std::env::temp_dir()
                            .join("forge-test-capabilities"),
                    }),
                    Box::new(TestInspector),
                )),
                database_path,
            },
            "owner-assignment".into(),
            subscription.id.0,
            subscription.start_cursor.0,
            target.id,
        )
    }

    #[tokio::test]
    async fn retirement_failure_revokes_access_and_retains_capacity_across_restart() {
        let (fixture, assignment, subscription, _, _) = subscription_test_daemon();
        let response = router(fixture.daemon()).oneshot(loopback_request(format!(
            "/v1/agent-assignments/{assignment}/record-update-subscriptions/{subscription}/events"
        ))).await.unwrap();
        let mut stream = response.into_body().into_data_stream();
        let _ = stream.next().await.unwrap().unwrap();
        let _ = stream.next().await.unwrap().unwrap();
        let request = || forge_rpc::RetireAgentAssignmentRequest {
            assignment_id: assignment.clone(),
        };
        // This fixture has no configured provider workspace: cleanup cannot be established.
        assert!(
            AgentDispatchApiServer::assignment_retire(&fixture.daemon(), request())
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), stream.next())
                .await
                .expect("revoked delivery must end even when cleanup fails")
                .is_none()
        );
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        let info = store.agent_assignment_info(&assignment).unwrap();
        assert!(
            info.assignment.closed_at_millis.is_some(),
            "retirement revokes even when cleanup fails"
        );
        assert!(info.capacity_reserved());
        assert!(info.dispatch_hold.is_some());
        let denied = AgentDispatchApiServer::task_start(
            &fixture.daemon(),
            forge_rpc::StartAssignmentTaskRequest {
                assignment_id: assignment.clone(),
                capability_id: assignment.clone(),
                correlation_id: "after-retirement".into(),
                task_id: info.assignment.record_id,
            },
        )
        .await
        .unwrap_err();
        assert!(denied.to_string().contains("assignment-capability-denied"));
        assert!(
            store
                .record_update_subscription(forge_app::RecordUpdateSubscriptionId(subscription))
                .is_err()
        );
        drop(store);
        // A closed Assignment with unfinished cleanup must remain retryable, not report success.
        assert!(
            AgentDispatchApiServer::assignment_retire(&fixture.daemon(), request())
                .await
                .is_err()
        );
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        assert!(
            store
                .agent_assignment_info(&assignment)
                .unwrap()
                .capacity_reserved()
        );
    }

    #[tokio::test]
    async fn unknown_resolution_requires_confirmation_and_revocation_and_preserves_other_blockers()
    {
        let (fixture, assignment, _, _, _) = subscription_test_daemon();
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        for (kind, id) in [
            ("provision-intent", "once"),
            ("pane", "known-pane"),
            ("retirement-pending", "operator"),
        ] {
            store
                .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
                    assignment_id: assignment.clone(),
                    resource_kind: kind.into(),
                    resource_id: id.into(),
                })
                .unwrap();
        }
        verify_unknown_resolution_request_validation(&fixture, &assignment).await;
        verify_unknown_resolution_confirmation_and_persistence(&fixture, &assignment, store).await;
    }

    async fn verify_unknown_resolution_request_validation(
        fixture: &SubscriptionTestDaemon,
        assignment: &str,
    ) {
        for field in ["secret", "capability_id", "capability", "correlation_id"] {
            let mut payload = serde_json::json!({
                "assignment_id": assignment,
                "confirm_old_agent_cannot_act": true,
                "confirm_unknown_resources_safe": true,
            });
            payload[field] = "agent-credential".into();
            assert!(
                serde_json::from_value::<forge_rpc::ResolveUnknownAgentResourcesRequest>(payload)
                    .is_err()
            );
        }
        let absent = serde_json::from_value::<forge_rpc::ResolveUnknownAgentResourcesRequest>(
            serde_json::json!({"assignment_id": assignment}),
        )
        .unwrap();
        assert!(
            AgentDispatchApiServer::assignment_resolve_unknown(&fixture.daemon(), absent)
                .await
                .unwrap_err()
                .to_string()
                .contains("confirmation-required")
        );
        let missing = forge_rpc::ResolveUnknownAgentResourcesRequest {
            assignment_id: "does-not-exist".into(),
            confirm_old_agent_cannot_act: true,
            confirm_unknown_resources_safe: true,
        };
        assert!(
            AgentDispatchApiServer::assignment_resolve_unknown(&fixture.daemon(), missing)
                .await
                .unwrap_err()
                .to_string()
                .contains("assignment-not-found")
        );
    }

    async fn verify_unknown_resolution_confirmation_and_persistence(
        fixture: &SubscriptionTestDaemon,
        assignment: &str,
        store: SqliteProjectRepository,
    ) {
        let assignment = assignment.to_owned();
        let request = |agent, resources| forge_rpc::ResolveUnknownAgentResourcesRequest {
            assignment_id: assignment.clone(),
            confirm_old_agent_cannot_act: agent,
            confirm_unknown_resources_safe: resources,
        };
        let resolve = |request| {
            let daemon = fixture.daemon();
            async move { AgentDispatchApiServer::assignment_resolve_unknown(&daemon, request).await }
        };
        assert!(
            resolve(request(true, true))
                .await
                .unwrap_err()
                .to_string()
                .contains("assignment-not-revoked")
        );
        store
            .close_agent_assignment(
                &assignment,
                forge_app::AgentAssignmentClosureReason::Retired,
            )
            .unwrap();
        store
            .hold_dispatch(
                store.agent_assignment(&assignment).unwrap().record_id,
                "operator review",
            )
            .unwrap();
        for (agent, resources) in [(false, false), (true, false), (false, true)] {
            assert!(
                resolve(request(agent, resources))
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("confirmation-required")
            );
        }
        let before = AgentDispatchApiServer::assignment_info(
            &fixture.daemon(),
            forge_rpc::AssignmentInfoRequest {
                assignment_id: assignment.clone(),
            },
        )
        .await
        .unwrap();
        assert!(before.unknown_resource_resolution.is_none());
        assert!(
            before
                .resources
                .iter()
                .find(|r| r.resource_kind == "provision-intent")
                .unwrap()
                .retired_at_millis
                .is_none()
        );
        let resolved = resolve(request(true, true)).await.unwrap();
        assert!(resolved.capacity_reserved);
        assert!(resolved.dispatch_hold.is_some());
        assert!(
            !resolved
                .recovery_blockers
                .iter()
                .any(|b| b == "uncertain-provisioning")
        );
        let audit = resolved.unknown_resource_resolution.unwrap();
        assert!(audit.confirmed_at_millis > 0);
        assert_eq!(audit.authority, "trusted-operator-rpc");
        assert!(audit.confirmation.contains("old agent can no longer act"));
        assert!(audit.confirmation.contains("unknown resources"));
        // Repeating the assertion must not rewrite the original audit timestamp.
        assert_eq!(
            resolve(request(true, true))
                .await
                .unwrap()
                .unknown_resource_resolution,
            Some(audit.clone())
        );
        drop(store);
        verify_resolved_resource_persistence(&fixture.database_path, &assignment, audit);
    }

    fn verify_resolved_resource_persistence(
        database_path: &Path,
        assignment: &str,
        audit: forge_rpc::UnknownResourceResolutionResponse,
    ) {
        let reopened = SqliteProjectRepository::open(database_path).unwrap();
        let persisted: forge_rpc::AgentAssignmentInfoResponse =
            reopened.agent_assignment_info(assignment).unwrap().into();
        assert_eq!(persisted.unknown_resource_resolution, Some(audit));
        assert!(persisted.capacity_reserved);
        reopened
            .retire_agent_assignment_resource(assignment, "pane", "known-pane")
            .unwrap();
        assert!(
            reopened
                .agent_assignment_info(assignment)
                .unwrap()
                .capacity_reserved(),
            "retirement-pending still reserves capacity"
        );
        reopened
            .retire_agent_assignment_resource(assignment, "retirement-pending", "operator")
            .unwrap();
        assert!(
            !reopened
                .agent_assignment_info(assignment)
                .unwrap()
                .capacity_reserved()
        );
        assert!(
            reopened
                .agent_assignment_info(assignment)
                .unwrap()
                .dispatch_hold
                .is_some()
        );
        drop(reopened);
        let after_restart = SqliteProjectRepository::open(database_path).unwrap();
        assert!(
            !after_restart
                .agent_assignment_info(assignment)
                .unwrap()
                .capacity_reserved()
        );
        assert!(
            after_restart
                .agent_assignment_info(assignment)
                .unwrap()
                .unknown_resource_resolution
                .is_some()
        );
    }

    #[test]
    fn retirement_rejects_assignment_credential_shaped_requests() {
        assert!(
            serde_json::from_value::<forge_rpc::RetireAgentAssignmentRequest>(serde_json::json!({
                "assignment_id": "owner-assignment", "secret": "agent-secret"
            }))
            .is_err()
        );
    }

    #[tokio::test]
    async fn assignment_info_does_not_invent_missing_evidence() {
        let (fixture, assignment, _, _, _) = subscription_test_daemon();
        let initial = AgentDispatchApiServer::assignment_info(
            &fixture.daemon(),
            forge_rpc::AssignmentInfoRequest {
                assignment_id: assignment.clone(),
            },
        )
        .await
        .unwrap();
        assert!(initial.resources.is_empty());
        assert_eq!(initial.dispatch_hold, None);
        assert_eq!(initial.acknowledged_at_millis, None);
        assert_eq!(initial.launch_started_at_millis, None);
        assert_eq!(initial.acknowledgement_deadline_millis, None);
        assert!(initial.capacity_reserved);
    }

    #[tokio::test]
    async fn assignment_info_reports_persisted_evidence_without_prompts_or_secrets() {
        let (fixture, assignment, _, _, _) = subscription_test_daemon();
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        let task_id = store.agent_assignment(&assignment).unwrap().record_id;
        store.edit_record(task_id, "PRIVATE TASK PROMPT").unwrap();
        store.begin_agent_launch(&assignment, 1000).unwrap();
        store
            .guard_agent_provisioning(&assignment, 301_000)
            .unwrap();
        for (kind, id) in [("pane", "pane-live"), ("tab", "tab-retired")] {
            store
                .add_agent_assignment_resource(&forge_app::NewAgentAssignmentResource {
                    assignment_id: assignment.clone(),
                    resource_kind: kind.into(),
                    resource_id: id.into(),
                })
                .unwrap();
        }
        store
            .retire_agent_assignment_resource(&assignment, "tab", "tab-retired")
            .unwrap();
        let root = fixture.database_path.with_extension("info-secrets");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(&assignment), "PRIVATE CAPABILITY VALUE\n").unwrap();
        let mut daemon = fixture.daemon();
        daemon.capability_secret_root = Some(root.clone());
        let request = || forge_rpc::AssignmentInfoRequest {
            assignment_id: assignment.clone(),
        };
        let before = AgentDispatchApiServer::assignment_info(&daemon, request())
            .await
            .unwrap();
        assert_eq!(before.id, assignment);
        assert_eq!(before.record_id, task_id);
        assert_eq!(before.task_status, "open");
        assert_eq!(before.state, "provisioning");
        assert_eq!(before.launch_started_at_millis, Some(1000));
        assert_eq!(before.acknowledgement_deadline_millis, Some(301_000));
        assert_eq!(before.acknowledged_at_millis, None);
        assert!(
            before
                .dispatch_hold
                .as_deref()
                .unwrap()
                .contains("deadline expired")
        );
        assert!(before.capacity_reserved);
        assert!(
            before
                .recovery_blockers
                .contains(&"unretired-resources".into())
        );
        assert_eq!(before.resources.len(), 2);
        assert!(
            before
                .resources
                .iter()
                .any(|r| r.resource_id == "tab-retired" && r.retired_at_millis.is_some())
        );
        let encoded = serde_json::to_string(&before).unwrap();
        for private in [
            "PRIVATE TASK PROMPT",
            "PRIVATE CAPABILITY VALUE",
            "agent-dispatch/task/v1",
            "prompt_template",
            "capability",
            "secret",
        ] {
            assert!(!encoded.contains(private), "leaked {private}");
        }
        // A late acknowledgement must remain distinguishable from hold clearing.
        store.acknowledge_agent_assignment(&assignment).unwrap();
        let after = AgentDispatchApiServer::assignment_info(&daemon, request())
            .await
            .unwrap();
        assert_eq!(after.state, "working");
        assert!(after.acknowledged_at_millis.unwrap() > 301_000);
        assert_eq!(after.dispatch_hold, before.dispatch_hold);
        assert_eq!(
            after.acknowledgement_deadline_millis,
            before.acknowledgement_deadline_millis
        );
        assert!(
            AgentDispatchApiServer::assignment_info(
                &daemon,
                forge_rpc::AssignmentInfoRequest {
                    assignment_id: "missing".into(),
                }
            )
            .await
            .unwrap_err()
            .data()
            .unwrap()
            .get()
            .contains("assignment-not-found")
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn assignment_info_retains_closed_recovery_evidence_across_reopen() {
        let (fixture, assignment, _, _, _) = subscription_test_daemon();
        {
            let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
            store.begin_agent_provision(&assignment).unwrap();
            store.hold_uncertain_agent_assignment(&assignment).unwrap();
            store
                .close_agent_assignment(
                    &assignment,
                    forge_app::AgentAssignmentClosureReason::Crashed,
                )
                .unwrap();
        }
        let request = || forge_rpc::AssignmentInfoRequest {
            assignment_id: assignment.clone(),
        };
        let info = AgentDispatchApiServer::assignment_info(&fixture.daemon(), request())
            .await
            .unwrap();
        assert!(info.closed_at_millis.is_some());
        assert_eq!(info.closure_reason.as_deref(), Some("crashed"));
        assert_eq!(info.launch_started_at_millis, None);
        assert_eq!(info.acknowledgement_deadline_millis, None);
        assert_eq!(info.acknowledged_at_millis, None);
        assert!(info.capacity_reserved);
        assert_eq!(
            info.recovery_blockers,
            ["unretired-resources", "uncertain-provisioning"]
        );
        {
            let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
            store.resume_dispatch(info.record_id).unwrap();
        }
        let resumed = AgentDispatchApiServer::assignment_info(&fixture.daemon(), request())
            .await
            .unwrap();
        assert_eq!(resumed.dispatch_hold, None);
        assert!(resumed.capacity_reserved);
        assert_eq!(resumed.recovery_blockers, info.recovery_blockers);
        {
            let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
            store
                .retire_agent_assignment_resource(&assignment, "provision-intent", "once")
                .unwrap();
        }
        let resolved = AgentDispatchApiServer::assignment_info(&fixture.daemon(), request())
            .await
            .unwrap();
        assert!(!resolved.capacity_reserved);
        assert!(resolved.recovery_blockers.is_empty());
        assert!(resolved.resources[0].retired_at_millis.is_some());
    }

    #[tokio::test]
    async fn assignment_info_uses_the_operator_contract_not_assignment_credentials() {
        let (fixture, assignment, _, _, _) = subscription_test_daemon();
        // Like resume and retirement, this read is on the existing trusted operator RPC
        // surface. It does not authenticate operators or accept agent credentials.
        let module = AgentDispatchApiServer::into_rpc(fixture.daemon());
        let request = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": forge_rpc::AGENT_DISPATCH_ASSIGNMENT_INFO_METHOD,
            "params": [{"assignment_id": assignment, "secret": "PRIVATE SECRET"}],
        });
        let (response, _) = module
            .raw_json_request(&request.to_string(), 1)
            .await
            .unwrap();
        let denied: serde_json::Value = serde_json::from_str(response.get()).unwrap();
        assert_eq!(denied["error"]["code"], -32602);
        assert!(denied.get("result").is_none());
        assert!(!response.get().contains("PRIVATE SECRET"));
        let request = serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": forge_rpc::AGENT_DISPATCH_ASSIGNMENT_INFO_METHOD,
            "params": [{"assignment_id": assignment}],
        });
        let (response, _) = module
            .raw_json_request(&request.to_string(), 1)
            .await
            .unwrap();
        let allowed: serde_json::Value = serde_json::from_str(response.get()).unwrap();
        assert_eq!(allowed["result"]["id"], assignment);
        assert!(allowed.get("error").is_none());
    }

    #[tokio::test]
    async fn acknowledgement_requires_the_assignment_secret() {
        let (fixture, assignment, _, _, _) = subscription_test_daemon();
        let root = fixture.database_path.with_extension("secrets");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(&assignment), "correct-secret\n").unwrap();
        std::fs::write(root.join("target-assignment"), "other-secret\n").unwrap();
        let mut daemon = fixture.daemon();
        daemon.capability_secret_root = Some(root.clone());
        let request = |secret: &str| forge_rpc::AcknowledgeAssignmentRequest {
            assignment_id: assignment.clone(),
            secret: secret.into(),
            correlation_id: "test".into(),
        };
        assert!(
            AgentDispatchApiServer::assignment_acknowledge(&daemon, request("wrong"))
                .await
                .is_err()
        );
        assert_eq!(
            AgentDispatchApiServer::assignment_acknowledge(&daemon, request("correct-secret"))
                .await
                .unwrap()
                .state,
            "working"
        );
        assert_eq!(
            AgentDispatchApiServer::assignment_acknowledge(&daemon, request("correct-secret"))
                .await
                .unwrap()
                .state,
            "working"
        );
        assert!(
            AgentDispatchApiServer::assignment_acknowledge(
                &daemon,
                forge_rpc::AcknowledgeAssignmentRequest {
                    assignment_id: "target-assignment".into(),
                    secret: "correct-secret".into(),
                    correlation_id: "cross".into()
                }
            )
            .await
            .is_err()
        );
        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        store
            .close_agent_assignment(
                &assignment,
                forge_app::AgentAssignmentClosureReason::Replaced,
            )
            .unwrap();
        assert!(
            AgentDispatchApiServer::assignment_acknowledge(&daemon, request("correct-secret"))
                .await
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn authenticated_assignment_start_checks_secret_binding_and_retries() {
        let (fixture, assignment, _, _, other_task) = subscription_test_daemon();
        let root = fixture.database_path.with_extension("start-secrets");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(&assignment), "correct-secret\n").unwrap();
        let mut daemon = fixture.daemon();
        daemon.capability_secret_root = Some(root.clone());
        verify_assignment_before_ack(&daemon, &assignment, &fixture.database_path).await;
        verify_authenticated_assignment_start(
            &daemon,
            &assignment,
            other_task,
            &fixture.database_path,
        )
        .await;
        std::fs::remove_dir_all(root).unwrap();
    }

    async fn verify_assignment_before_ack(daemon: &Daemon, assignment: &str, database_path: &Path) {
        let daemon = daemon.clone();
        let assignment = assignment.to_owned();
        let read_request = |secret: &str| forge_rpc::AcknowledgeAssignmentRequest {
            assignment_id: assignment.clone(),
            secret: secret.into(),
            correlation_id: "read".into(),
        };
        assert!(
            AgentDispatchApiServer::assignment_instructions(
                &daemon,
                read_request("correct-secret")
            )
            .await
            .is_err()
        );
        assert!(
            AgentDispatchApiServer::assignment_instructions(&daemon, read_request("wrong"))
                .await
                .is_err()
        );
        let initial_store = SqliteProjectRepository::open(database_path).unwrap();
        let owner_id = initial_store
            .agent_assignment(&assignment)
            .unwrap()
            .record_id;
        assert!(
            AgentDispatchApiServer::assignment_task_start(
                &daemon,
                forge_rpc::AuthenticatedAssignmentTaskStartRequest {
                    assignment_id: assignment.clone(),
                    secret: "correct-secret".into(),
                    correlation_id: "before-ack".into(),
                    task_id: owner_id,
                }
            )
            .await
            .is_err()
        );
    }

    async fn verify_authenticated_assignment_start(
        daemon: &Daemon,
        assignment: &str,
        other_task: u64,
        database_path: &Path,
    ) {
        let daemon = daemon.clone();
        let assignment = assignment.to_owned();
        let read_request = |secret: &str| forge_rpc::AcknowledgeAssignmentRequest {
            assignment_id: assignment.clone(),
            secret: secret.into(),
            correlation_id: "read".into(),
        };
        let acknowledged = AgentDispatchApiServer::assignment_acknowledge(
            &daemon,
            forge_rpc::AcknowledgeAssignmentRequest {
                assignment_id: assignment.clone(),
                secret: "correct-secret".into(),
                correlation_id: "ack".into(),
            },
        )
        .await
        .unwrap();
        assert!(
            AgentDispatchApiServer::assignment_instructions(
                &daemon,
                read_request("correct-secret")
            )
            .await
            .is_err()
        );
        let request = |secret: &str, task_id| forge_rpc::AuthenticatedAssignmentTaskStartRequest {
            assignment_id: assignment.clone(),
            secret: secret.into(),
            correlation_id: "start".into(),
            task_id,
        };
        assert!(
            AgentDispatchApiServer::assignment_task_start(
                &daemon,
                request("wrong", acknowledged.record_id)
            )
            .await
            .is_err()
        );
        assert!(
            AgentDispatchApiServer::assignment_task_start(
                &daemon,
                request("correct-secret", other_task)
            )
            .await
            .is_err()
        );
        let store = SqliteProjectRepository::open(database_path).unwrap();
        store.start_record(other_task).unwrap();
        store.done_record(other_task, Some("done")).unwrap();
        assert!(
            AgentDispatchApiServer::assignment_task_start(
                &daemon,
                request("correct-secret", acknowledged.record_id)
            )
            .await
            .is_ok()
        );
        assert!(
            AgentDispatchApiServer::assignment_task_start(
                &daemon,
                request("correct-secret", acknowledged.record_id)
            )
            .await
            .is_ok()
        );
        let instructions = AgentDispatchApiServer::assignment_instructions(
            &daemon,
            read_request("correct-secret"),
        )
        .await
        .unwrap();
        assert!(instructions.contains("owner"));
        assert!(
            AgentDispatchApiServer::assignment_instructions(&daemon, read_request("wrong"))
                .await
                .is_err()
        );
        verify_revoked_assignment_start(
            &daemon,
            &assignment,
            acknowledged.record_id,
            database_path,
        )
        .await;
    }

    async fn verify_revoked_assignment_start(
        daemon: &Daemon,
        assignment: &str,
        task_id: u64,
        database_path: &Path,
    ) {
        let daemon = daemon.clone();
        let assignment = assignment.to_owned();
        let read_request = |secret: &str| forge_rpc::AcknowledgeAssignmentRequest {
            assignment_id: assignment.clone(),
            secret: secret.into(),
            correlation_id: "read".into(),
        };
        let request = |secret: &str, task_id| forge_rpc::AuthenticatedAssignmentTaskStartRequest {
            assignment_id: assignment.clone(),
            secret: secret.into(),
            correlation_id: "start".into(),
            task_id,
        };
        let store = SqliteProjectRepository::open(database_path).unwrap();
        store
            .close_agent_assignment(
                &assignment,
                forge_app::AgentAssignmentClosureReason::Replaced,
            )
            .unwrap();
        assert!(
            AgentDispatchApiServer::assignment_instructions(
                &daemon,
                read_request("correct-secret")
            )
            .await
            .is_err()
        );
        assert!(
            AgentDispatchApiServer::assignment_task_start(
                &daemon,
                request("correct-secret", task_id)
            )
            .await
            .is_err()
        );
    }

    fn loopback_request(uri: String) -> Request<Body> {
        let mut request = Request::builder().uri(uri).body(Body::empty()).unwrap();
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            45678,
        )));
        request
    }

    fn subscription_authorization_fixture() -> (PathBuf, u64, u64, u64) {
        let database_path = std::env::temp_dir().join(format!(
            "forge-provenance-subscription-{}-{}.sqlite3",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = SqliteProjectRepository::open(&database_path).unwrap();
        for slug in ["alpha", "beta", "gamma"] {
            super::test_projects::register(&store, slug, None);
        }
        let task = |body: &str, project_slug: &str| NewTypedRecord {
            record_type: RecordType::Task,
            task_kind: None,
            body: body.into(),
            project_slugs: vec![project_slug.into()],
            generated_by_ids: vec![],
        };
        let owner = store.open_typed_record(&task("owner", "alpha")).unwrap();
        let prerequisite = store
            .open_typed_record(&task("prerequisite", "beta"))
            .unwrap();
        let generated = store
            .open_typed_record(&task("generated", "gamma"))
            .unwrap();
        store
            .relate_records(owner.id, prerequisite.id, RecordRelationshipKind::DependsOn)
            .unwrap();
        store
            .relate_records(owner.id, generated.id, RecordRelationshipKind::GeneratedBy)
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "owner-assignment".into(),
                record_id: owner.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        let spec = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Spec,
                task_kind: None,
                body: "spec".into(),
                project_slugs: vec!["alpha".into()],
                generated_by_ids: vec![],
            })
            .unwrap();
        let child = store
            .open_task_under_spec("child", spec.id, None, vec![])
            .unwrap();
        store
            .create_agent_assignment(&NewAgentAssignment {
                id: "spec-assignment".into(),
                record_id: spec.id,
                project_slug: "alpha".into(),
                profile_revision: 1,
            })
            .unwrap();
        drop(store);
        (database_path, prerequisite.id, generated.id, child.id)
    }

    #[tokio::test]
    async fn subscription_rpc_authorizes_task_prerequisites_and_spec_children_only() {
        let (database_path, prerequisite, generated, child) = subscription_authorization_fixture();
        let daemon = Daemon::new(Application::with_agent_dispatch(
            Box::new(SqliteProjectRepository::open(&database_path).unwrap()),
            Box::new(SqliteProjectRepository::open(&database_path).unwrap()),
            Box::new(super::ServerAgentDispatchAdapterFactory {
                herdr: super::HerdrAdapterFactory::from_configuration_store(database_path.clone()),
                daemon_url: "http://127.0.0.1:45003".into(),
                capability_secret_root: std::env::temp_dir().join("forge-test-capabilities"),
            }),
            Box::new(TestInspector),
        ));
        let request = |assignment_id: &str, target_record_id| CreateAgentSubscriptionRequest {
            assignment_id: assignment_id.into(),
            capability_id: "cap-owner".into(),
            correlation_id: "subscription".into(),
            target_record_ids: vec![target_record_id],
            kind_filters: vec![],
        };
        assert_eq!(
            AgentSubscriptionApiServer::create(&daemon, request("owner-assignment", prerequisite))
                .await
                .unwrap()
                .target_record_ids,
            vec![prerequisite]
        );
        assert!(
            AgentSubscriptionApiServer::create(&daemon, request("owner-assignment", generated))
                .await
                .unwrap_err()
                .to_string()
                .contains("assignment-capability-denied")
        );
        assert_eq!(
            AgentSubscriptionApiServer::create(&daemon, request("spec-assignment", child))
                .await
                .unwrap()
                .target_record_ids,
            vec![child]
        );
        assert!(
            AgentSubscriptionApiServer::create(&daemon, request("spec-assignment", prerequisite))
                .await
                .unwrap_err()
                .to_string()
                .contains("assignment-capability-denied")
        );
        drop(daemon);
        std::fs::remove_file(database_path).unwrap();
    }

    #[test]
    fn store_ownership_lock_rejects_a_second_owner_and_releases_for_a_later_one() {
        let store_path = std::env::temp_dir().join(format!(
            "forge-daemon-ownership-{}-{}.sqlite3",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = StoreOwnershipLock::acquire(&store_path).unwrap();
        let error = StoreOwnershipLock::acquire(&store_path).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("exclusive daemon ownership lock")
        );
        drop(first);
        let later_owner = StoreOwnershipLock::acquire(&store_path).unwrap();
        drop(later_owner);
        std::fs::remove_file(format!("{}.lock", store_path.display())).unwrap();
    }

    #[tokio::test]
    async fn duplicate_store_owner_is_rejected_before_listener_binding() {
        let store_path = std::env::temp_dir().join(format!(
            "forge-daemon-startup-{}-{}.sqlite3",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = StoreOwnershipLock::acquire(&store_path).unwrap();
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bind_addr = probe.local_addr().unwrap();
        drop(probe);

        let error = serve(DaemonConfig {
            bind_addr,
            store_path: store_path.clone(),
            managed: None,
        })
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("exclusive daemon ownership lock")
        );
        let listener = std::net::TcpListener::bind(bind_addr)
            .expect("rejected daemon must not bind its listener");
        drop(listener);
        drop(first);
        std::fs::remove_file(format!("{}.lock", store_path.display())).unwrap();
    }

    #[test]
    fn bind_configuration_prioritizes_flag_then_environment_then_default() {
        assert_eq!(
            DaemonConfig::resolve(Some("127.0.0.1:45004"), Some("127.0.0.1:45005"))
                .unwrap()
                .bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 45004)
        );
        assert_eq!(
            DaemonConfig::resolve(None, Some("127.0.0.1:45005"))
                .unwrap()
                .bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 45005)
        );
        assert_eq!(
            DaemonConfig::resolve(None, None)
                .unwrap()
                .bind_addr
                .to_string(),
            DEFAULT_BIND_ADDR
        );
    }

    #[test]
    fn managed_configuration_is_explicit_and_rejects_invalid_inputs() {
        let inputs = ManagedLaunchInputs {
            bind_addr: "127.0.0.1:45004".into(),
            store_path: "managed.sqlite3".into(),
            runtime_record_path: "runtime.json".into(),
            lifecycle_control_endpoint: "control.sock".into(),
            lifecycle_credential: "credential".into(),
            daemon_instance_id: "instance-123".into(),
        };
        let config =
            DaemonConfig::from_process(Some("127.0.0.1:45005"), Some(inputs.clone())).unwrap();
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:45004");
        assert_eq!(config.store_path, PathBuf::from("managed.sqlite3"));
        let managed = config.managed.unwrap();
        assert_eq!(managed.daemon_instance_id, "instance-123");
        assert_eq!(managed.lifecycle_credential(), "credential");

        let error = DaemonConfig::managed(ManagedLaunchInputs {
            bind_addr: "not-an-address".into(),
            store_path: "managed.sqlite3".into(),
            runtime_record_path: "runtime.json".into(),
            lifecycle_control_endpoint: "control.sock".into(),
            lifecycle_credential: "credential".into(),
            daemon_instance_id: "instance-123".into(),
        })
        .unwrap_err();
        assert!(error.contains("invalid managed daemon bind address"));

        let error = DaemonConfig::managed(ManagedLaunchInputs {
            bind_addr: "127.0.0.1:45004".into(),
            store_path: String::new(),
            runtime_record_path: "runtime.json".into(),
            lifecycle_control_endpoint: "control.sock".into(),
            lifecycle_credential: "credential".into(),
            daemon_instance_id: "instance-123".into(),
        })
        .unwrap_err();
        assert_eq!(error, "managed daemon store path must not be empty");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn lifecycle_control_rejects_wrong_credentials_then_gracefully_stops_and_cleans_up() {
        let unique = format!(
            "forge-lifecycle-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let directory = std::env::temp_dir().join(unique);
        std::fs::create_dir(&directory).unwrap();
        let endpoint = directory.join("control.sock");
        let config = DaemonConfig::managed(ManagedLaunchInputs {
            bind_addr: "127.0.0.1:0".into(),
            store_path: directory
                .join("forge.sqlite3")
                .to_string_lossy()
                .into_owned(),
            runtime_record_path: directory
                .join("runtime.json")
                .to_string_lossy()
                .into_owned(),
            lifecycle_control_endpoint: endpoint.to_string_lossy().into_owned(),
            lifecycle_credential: "expected-credential".into(),
            daemon_instance_id: "instance-123".into(),
        })
        .unwrap();
        let mut server = tokio::spawn(serve(config));

        for _ in 0..100 {
            if endpoint.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(endpoint.exists(), "lifecycle endpoint was not created");
        let mut wrong = tokio::net::UnixStream::connect(&endpoint).await.unwrap();
        wrong.write_all(b"wrong-credential\n").await.unwrap();
        drop(wrong);
        assert!(endpoint.exists());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut server)
                .await
                .is_err()
        );

        let mut accepted = tokio::net::UnixStream::connect(&endpoint).await.unwrap();
        accepted.write_all(b"expected-credential\n").await.unwrap();
        drop(accepted);
        server.await.unwrap().unwrap();
        assert!(!endpoint.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[tokio::test]
    async fn managed_daemon_status_reports_the_launch_instance_id() {
        let daemon =
            Daemon::with_daemon_instance_id(test_application(), Some("instance-123".into()));
        let status = daemon.status().await.unwrap();
        assert_eq!(
            status.management.daemon_instance_id.as_deref(),
            Some("instance-123")
        );
        assert_eq!(
            status.management.revision_identity,
            forge_rpc::RevisionIdentity::from(forge_build_info::current())
        );
    }

    #[tokio::test]
    async fn browser_routes_render_shared_status_without_cors() {
        let response = router(Daemon::new(test_application()))
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        let page = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(page.contains("src=\"/assets/htmx.min.js\""));
        assert!(page.contains("hx-get=\"/status\""));
        assert!(page.contains("forge"));

        let response = router(Daemon::new(test_application()))
            .oneshot(
                Request::builder()
                    .uri("/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        let fragment = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(fragment.contains("id=\"status\""));
        assert!(fragment.contains("forge"));
        assert!(fragment.contains("0.1.0"));
    }

    #[tokio::test]
    async fn htmx_asset_is_served_locally_with_javascript_content_type() {
        let response = router(Daemon::new(test_application()))
            .oneshot(
                Request::builder()
                    .uri("/assets/htmx.min.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/javascript; charset=utf-8"
        );
        let asset = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(asset.starts_with("var htmx="));
    }

    #[tokio::test]
    #[allow(clippy::too_many_lines)]
    async fn typed_rpc_client_round_trips_through_the_listener() {
        fn selected<T>(record: T) -> forge_rpc::SelectedRecordRequest<T> {
            forge_rpc::SelectedRecordRequest::selected("test", record)
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, router(Daemon::new(test_application())))
                .await
                .unwrap();
        });

        let client = HttpClientBuilder::default()
            .build(format!("http://{address}/rpc"))
            .unwrap();
        let status = client.status().await.unwrap();
        assert_eq!(status.name, "forge");
        assert!(status.ready);
        let folder = std::fs::canonicalize(format!("{}/../..", env!("CARGO_MANIFEST_DIR")))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let project = client
            .register(RegisterProjectRequest {
                folder: folder.clone(),
                slug: "server-test-project".into(),
                workspace: Some("test".into()),
                cwd: folder.clone(),
            })
            .await
            .unwrap();
        assert_eq!(project.slug, "server-test-project");
        assert_eq!(project.project_root, folder);
        let here = client
            .here(ProjectHereRequest {
                folder: format!("{folder}/crates"),
                workspace: None,
                project: None,
            })
            .await
            .unwrap();
        assert_eq!(here, project);

        let record = client
            .open(selected(OpenRecordRequest {
                body: "A daemon-backed Record".into(),
                project_scope: ProjectScopeRequest {
                    project_slugs: vec![project.reference.clone()],
                },
                generated_by_ids: vec![],
            }))
            .await
            .unwrap();
        assert_eq!(record.id, 1);
        assert_eq!(record.status, "open");
        assert_eq!(
            record.project_scope.project_slugs,
            std::slice::from_ref(&project.reference)
        );
        let updates = client
            .updates(selected(RecordIdRequest { id: record.id }))
            .await
            .unwrap();
        assert_eq!(updates.updates.len(), 1);
        assert_eq!(updates.updates[0].kind, "created");
        assert_eq!(
            client
                .list(selected(ListRecordsRequest {
                    project_slug: Some(project.reference.clone()),
                    status: Some("open".into()),
                    record_type: None,
                    task_kind: None,
                }))
                .await
                .unwrap()
                .records,
            vec![record.clone()]
        );
        let spec = client
            .open_spec(selected(OpenSpecRequest {
                body: "A daemon-backed Spec".into(),
                project_slug: project.reference.clone(),
                generated_by_ids: vec![],
            }))
            .await
            .unwrap();
        assert_eq!(spec.record_type, "spec");
        let task = client
            .open_task_under_spec(selected(OpenTaskUnderSpecRequest {
                body: "A daemon-backed Task".into(),
                spec_id: spec.id,
                task_kind: None,
                generated_by_ids: vec![],
            }))
            .await
            .unwrap();
        assert_eq!(task.record_type, "task");
        assert_eq!(task.project_scope, spec.project_scope);
        assert_eq!(task.relationships[0].parent_id, spec.id);
        assert_eq!(
            client
                .open_task(selected(OpenTaskRequest {
                    body: "Standalone daemon-backed Task".into(),
                    project_slug: None,
                    task_kind: None,
                    generated_by_ids: vec![],
                }))
                .await
                .unwrap()
                .record_type,
            "task"
        );
        let wayfinder = client
            .open_wayfinder(selected(OpenWayfinderRequest {
                body: "A daemon-backed Wayfinder".into(),
                project_scope: ProjectScopeRequest {
                    project_slugs: vec![project.reference.clone()],
                },
                generated_by_ids: vec![],
            }))
            .await
            .unwrap();
        let wayfinder_task = client
            .open_task_under_wayfinder(selected(OpenTaskUnderWayfinderRequest {
                body: "A scoped daemon-backed Task".into(),
                wayfinder_id: wayfinder.id,
                project_slug: None,
                task_kind: None,
                generated_by_ids: vec![],
            }))
            .await
            .unwrap();
        assert_eq!(wayfinder.record_type, "wayfinder");
        assert_eq!(wayfinder_task.project_scope, wayfinder.project_scope);
        assert_eq!(wayfinder_task.relationships[0].parent_id, wayfinder.id);
        let grilling = client
            .open_grilling_under_wayfinder(selected(OpenGrillingUnderWayfinderRequest {
                body: "A daemon-backed Grilling".into(),
                wayfinder_id: wayfinder.id,
                project_scope: None,
                generated_by_ids: vec![],
            }))
            .await
            .unwrap();
        assert_eq!(grilling.record_type, "grilling");
        assert_eq!(grilling.project_scope, wayfinder.project_scope);
        assert_eq!(grilling.relationships[0].parent_id, wayfinder.id);
        let wayfinders = client
            .list(selected(ListRecordsRequest {
                project_slug: Some(project.reference.clone()),
                status: None,
                record_type: Some("wayfinder".into()),
                task_kind: None,
            }))
            .await
            .unwrap();
        assert_eq!(wayfinders.records.len(), 1);
        assert_eq!(wayfinders.records[0].id, wayfinder.id);
        assert_eq!(wayfinders.records[0].record_type, "wayfinder");
        let specs = client
            .list(selected(ListRecordsRequest {
                project_slug: None,
                status: None,
                record_type: Some("spec".into()),
                task_kind: None,
            }))
            .await
            .unwrap();
        assert_eq!(specs.records.len(), 1);
        assert_eq!(specs.records[0].id, spec.id);
        assert_eq!(specs.records[0].record_type, "spec");
        assert!(
            client
                .open_task_under_spec(selected(OpenTaskUnderSpecRequest {
                    body: "Invalid child".into(),
                    spec_id: record.id,
                    task_kind: None,
                    generated_by_ids: vec![],
                }))
                .await
                .is_err()
        );
        assert!(
            client
                .list(selected(ListRecordsRequest {
                    project_slug: None,
                    status: None,
                    record_type: Some("invalid".into()),
                    task_kind: None,
                }))
                .await
                .is_err()
        );
        let edited = client
            .edit(selected(EditRecordRequest {
                id: record.id,
                body: "Revised Record".into(),
            }))
            .await
            .unwrap();
        assert_eq!(edited.body, "Revised Record");
        let in_progress = client
            .start(selected(RecordIdRequest { id: record.id }))
            .await
            .unwrap();
        assert_eq!(in_progress.status, "in_progress");
        let done = client
            .done(selected(DoneRecordRequest {
                id: record.id,
                completion_resolution: Some("complete".into()),
            }))
            .await
            .unwrap();
        assert_eq!(done.completion_resolution.as_deref(), Some("complete"));
        assert!(
            client
                .start(selected(RecordIdRequest { id: record.id }))
                .await
                .is_err()
        );
        assert_eq!(
            client
                .open(selected(OpenRecordRequest {
                    body: "Unscoped".into(),
                    project_scope: ProjectScopeRequest {
                        project_slugs: vec![],
                    },
                    generated_by_ids: vec![],
                }))
                .await
                .unwrap()
                .project_scope
                .project_slugs,
            Vec::<String>::new()
        );

        server.abort();
    }

    #[tokio::test]
    async fn subscription_endpoint_replays_framed_updates_and_enforces_cursor_guards() {
        let (fixture, assignment_id, subscription_id, start_cursor, _) = subscription_test_daemon();
        let daemon = fixture.daemon();
        let endpoint = format!(
            "/v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events"
        );
        let response = router(daemon.clone())
            .oneshot(loopback_request(endpoint.clone()))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "no-cache, no-transform"
        );
        assert_eq!(response.headers()["x-accel-buffering"], "no");
        let mut stream = response.into_body().into_data_stream();
        let retry = String::from_utf8(stream.next().await.unwrap().unwrap().to_vec()).unwrap();
        let update = String::from_utf8(stream.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert_eq!(retry, "retry: 1000\n\n");
        assert!(update.starts_with("id: "));
        assert!(update.contains("event: record-update\n"));
        assert!(update.contains(&format!("\"subscription_id\":{subscription_id}")));
        assert!(update.contains("\"kind\":\"body-edited\""));
        assert!(update.ends_with("\n\n"));

        let response = router(daemon.clone())
            .oneshot(loopback_request(format!(
                "{endpoint}?after={}",
                start_cursor + 1
            )))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let mut conflict = loopback_request(format!("{endpoint}?after={}", start_cursor + 1));
        conflict
            .headers_mut()
            .insert("last-event-id", start_cursor.to_string().parse().unwrap());
        assert_eq!(
            router(daemon.clone())
                .oneshot(conflict)
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );

        let response = router(daemon)
            .oneshot(loopback_request(format!("{endpoint}?after=0")))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    }

    #[tokio::test]
    async fn active_delivery_publishes_assignment_task_start_and_reopen_updates() {
        let (fixture, assignment_id, subscription_id, _, target_id) = subscription_test_daemon();
        let daemon = fixture.daemon();
        let endpoint = format!(
            "/v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events"
        );
        let response = router(daemon.clone())
            .oneshot(loopback_request(endpoint))
            .await
            .unwrap();
        let mut stream = response.into_body().into_data_stream();
        let _ = stream.next().await.unwrap().unwrap();
        let _ = stream.next().await.unwrap().unwrap();

        AgentDispatchApiServer::task_start(
            &daemon,
            StartAssignmentTaskRequest {
                assignment_id: "target-assignment".into(),
                capability_id: "target-capability".into(),
                correlation_id: "start-target".into(),
                task_id: target_id,
            },
        )
        .await
        .unwrap();
        let started = String::from_utf8(stream.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(started.contains("\"kind\":\"status-transition\""));

        let store = SqliteProjectRepository::open(&fixture.database_path).unwrap();
        store
            .close_agent_assignment(
                "target-assignment",
                forge_app::AgentAssignmentClosureReason::Retired,
            )
            .unwrap();
        drop(store);
        AgentDispatchApiServer::task_reopen(&daemon, RecordIdRequest { id: target_id })
            .await
            .unwrap();
        let reopened = String::from_utf8(stream.next().await.unwrap().unwrap().to_vec()).unwrap();
        assert!(reopened.contains("\"kind\":\"status-transition\""));
    }

    #[tokio::test]
    async fn active_subscription_delivery_ends_at_its_renewed_lease_deadline() {
        let (fixture, assignment_id, subscription_id, _, _) = subscription_test_daemon();
        let daemon = fixture.daemon();
        let endpoint = format!(
            "/v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events"
        );
        let response = router(daemon.clone())
            .oneshot(loopback_request(endpoint))
            .await
            .unwrap();
        let mut stream = response.into_body().into_data_stream();
        let _ = stream.next().await.unwrap().unwrap();
        let _ = stream.next().await.unwrap().unwrap();

        daemon.delivery.renew_subscription(
            RecordUpdateSubscriptionId(subscription_id),
            super::now_millis() + 20,
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(1), stream.next())
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            daemon
                .delivery
                .connections
                .lock()
                .unwrap()
                .get(&RecordUpdateSubscriptionId(subscription_id))
                .is_none()
        );
    }

    #[tokio::test]
    async fn subscription_endpoint_rejects_non_loopback_peers_before_delivery() {
        let (fixture, assignment_id, subscription_id, _, _) = subscription_test_daemon();
        let daemon = fixture.daemon();
        let mut request = Request::builder()
            .uri(format!(
                "/v1/agent-assignments/{assignment_id}/record-update-subscriptions/{subscription_id}/events"
            ))
            .body(Body::empty())
            .unwrap();
        request.extensions_mut().insert(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(203, 0, 113, 1)),
            45678,
        )));
        assert_eq!(
            router(daemon).oneshot(request).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn resync_required_reports_current_head_after_subscription_creation() {
        let response = super::resync_required(
            RecordUpdateSubscriptionId(9),
            RecordUpdateId(12),
            RecordUpdateId(18),
        );
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let body = String::from_utf8(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert_eq!(
            body,
            "{\"code\":\"resync-required\",\"subscription_id\":9,\"retention_floor\":0,\"start_after_update_id\":12,\"head\":18}"
        );
    }

    #[tokio::test]
    async fn record_update_events_use_complete_named_sse_json_frames() {
        let kinds = [
            RecordUpdateKind::Created,
            RecordUpdateKind::BodyEdited,
            RecordUpdateKind::StatusTransition {
                from: RecordStatus::Open,
                to: RecordStatus::InProgress,
            },
            RecordUpdateKind::RelationshipAdded {
                related_record_id: 4,
                relationship_kind: None,
            },
            RecordUpdateKind::RelationshipRemoved {
                related_record_id: 4,
                relationship_kind: None,
            },
            RecordUpdateKind::CommentAdded {
                comment_id: forge_app::RecordCommentId(3),
            },
            RecordUpdateKind::CommentDeleted {
                comment_id: forge_app::RecordCommentId(3),
            },
        ];

        for kind in kinds {
            let event = super::record_update_event(
                RecordUpdateSubscriptionId(9),
                &RecordUpdateEnvelope {
                    record_id: 47,
                    update: RecordUpdate {
                        id: RecordUpdateId(12),
                        created_at_millis: 123,
                        kind,
                    },
                },
            );
            let response = Sse::new(tokio_stream::iter([Ok::<_, std::convert::Infallible>(
                event,
            )]))
            .into_response();
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/event-stream"
            );
            let frame = String::from_utf8(
                to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(frame.starts_with("id: 12\nevent: record-update\ndata: {"));
            assert!(frame.contains("\"timestamp\":\"1970-01-01T00:00:00.123Z\""));
            assert!(frame.ends_with(" }\n\n"));
        }
    }
}
