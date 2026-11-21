//! T24: a Forgeup-installed Revision dispatches through the production durable
//! path using only its packaged components, then the daemon switches to another
//! Revision while the Assignment is retained.
//!
//! Both Revisions are registered with the real `forgeup::register_at` (bundle
//! verification, copy into the immutable Installation, re-verification of the
//! installed copy). Their bundles hold the real Node, the real
//! `forge-runner-host` built by this Cargo invocation, the shipped runner
//! sources and the dependency closure. The daemon composition comes from
//! exactly the arguments `forgeup daemon start` passes
//! (`forgeup::managed_durable_arguments`), parsed by `DaemonConfig::managed`.
//!
//! The only test substitution beyond the controlled release gate is the runner
//! entry, as in every tracer scenario: a faux-model worker whose `src` and
//! `node_modules` resolve to the *installed* bundle, so the shipped Check runner
//! and its locked closure run from the Installation. The second Revision's entry
//! records any execution as a failure.
use super::{
    AgentDispatchRepository, Arc, Command, Daemon, SqliteProjectRepository, SupervisedDispatch,
    Value, fs, router, supervised_dispatch,
};
use forge_app::{
    AgentDispatchAdapterFactory as _, AgentObservation, RecordRepository as _,
    durable_execution::{
        Repository as _, bundle,
        completion::{AnswerObservation, Repository as _},
    },
};
use forge_server::{
    DaemonConfig, ManagedLaunchInputs,
    durable_authorization::launch::{
        ComponentCatalog, Execution, attest_component_hash, compose, current_forge_revision,
    },
};
use forgeup::{
    ForgeupPaths, Installation, RegistrationOptions,
    runner::{BundleManifest, tree_hash},
};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub struct Packaged {
    paths: ForgeupPaths,
    pub first: Installation,
    pub second: Installation,
    /// Runner directory of the first Revision's faux-model worker.
    pub worker: PathBuf,
    /// Runner directory of the second Revision's faux-model worker.
    pub second_worker: PathBuf,
}

/// The real `forge-runner-host` that this Cargo invocation built next to the test
/// binary (`target/<profile>/`), never one from `PATH`. It must report this
/// build's Revision, so a stale binary from another commit fails loudly.
fn host_binary() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let host = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("forge-runner-host");
    let output = Command::new(&host)
        .env_clear()
        .arg("--compatibility")
        .output()
        .unwrap_or_else(|_| {
            panic!(
                "build forge-runner-host first (`cargo test --workspace` builds it): {}",
                host.display()
            )
        });
    let reported: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        reported["forge_revision"],
        current_forge_revision(),
        "forge-runner-host is stale; rebuild it with this checkout"
    );
    host
}

fn repository_runner() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../runner")
        .canonicalize()
        .unwrap()
}

/// Copy regular files (permissions kept), skipping symlinks (`--no-bin-links`)
/// and the given top-level development-only entries.
fn copy_tree(from: &Path, to: &Path, skip: &[&str]) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if skip.iter().any(|skipped| name == *skipped) {
            continue;
        }
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_tree(&entry.path(), &to.join(&name), &[]);
        } else if kind.is_file() {
            fs::copy(entry.path(), to.join(&name)).unwrap();
        }
    }
}

/// Hard-link a source tree (same filesystem) so a second bundle shares bytes.
fn link_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            link_tree(&entry.path(), &target);
        } else if entry.file_name() != bundle::MANIFEST {
            fs::hard_link(entry.path(), target).unwrap();
        }
    }
}

/// What `runner/package-bundle.mjs` declares, computed independently here.
fn write_manifest(bundle_root: &Path) {
    let lock = fs::read(bundle_root.join(bundle::LOCK)).unwrap();
    let manifest = BundleManifest {
        schema: 2,
        runner_protocol: 1,
        node_version: bundle::PINNED_NODE_VERSION.into(),
        forge_commit: forge_build_info::current().commit.into(),
        package_lock_sha256: format!("{:x}", Sha256::digest(&lock)),
        tree_sha256: tree_hash(bundle_root).unwrap(),
        runner_component_sha256: Some(
            attest_component_hash(
                &bundle_root.join(bundle::NODE),
                &bundle_root.join("runner/src"),
            )
            .unwrap(),
        ),
    };
    fs::write(
        bundle_root.join(bundle::MANIFEST),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

/// The bundle layout `runner/package-bundle.mjs` produces, from this checkout.
fn source_bundle(destination: &Path, node: &Path, host: &Path) {
    let runner = repository_runner();
    for directory in ["node", "host", "runner/src"] {
        fs::create_dir_all(destination.join(directory)).unwrap();
    }
    fs::copy(node, destination.join(bundle::NODE)).unwrap();
    fs::copy(host, destination.join(bundle::HOST)).unwrap();
    for file in [
        "package.json",
        "package-lock.json",
        "UPSTREAM-LICENSE",
        "prepare-reader.mjs",
    ] {
        fs::copy(runner.join(file), destination.join("runner").join(file)).unwrap();
    }
    copy_tree(&runner.join("src"), &destination.join("runner/src"), &[]);
    // The production closure: development-only packages, bin links and npm's
    // hidden lock are excluded exactly as `npm ci --omit=dev --no-bin-links`.
    copy_tree(
        &runner.join("node_modules"),
        &destination.join("runner/node_modules"),
        &[".bin", ".package-lock.json", "typescript", "@types"],
    );
    write_manifest(destination);
}

fn script(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn register(root: &Path, paths: &ForgeupPaths, label: &str, bundle: &Path) -> Installation {
    // The Revision's executables only need to report this build's identity.
    let identity = serde_json::to_string(&forge_build_info::current()).unwrap();
    let (cli, daemon) = (
        root.join(format!("{label}-cli")),
        root.join(format!("{label}-daemon")),
    );
    for path in [&cli, &daemon] {
        script(
            path,
            &format!(
                "#!/bin/sh\nif [ \"$1\" = \"--build-info\" ]; then echo '{identity}'; exit 0; fi\nexit 1\n"
            ),
        );
    }
    forgeup::register_at(
        paths,
        label,
        &cli,
        &daemon,
        None,
        RegistrationOptions {
            runner_bundle: Some(bundle),
            ..RegistrationOptions::default()
        },
    )
    .unwrap()
}

/// A faux-model worker directory whose `src` and `node_modules` resolve to the
/// given Installation's packaged runner sources and dependency closure.
fn worker_directory(directory: &Path, runner: &forgeup::runner::InstalledRunner) -> PathBuf {
    fs::create_dir_all(directory.join("test")).unwrap();
    for file in ["supervised-check-worker.mjs", "supervised-check-effect.mjs"] {
        fs::copy(
            repository_runner().join("test").join(file),
            directory.join("test").join(file),
        )
        .unwrap();
    }
    for linked in ["src", "node_modules"] {
        std::os::unix::fs::symlink(
            runner.root.join("runner").join(linked),
            directory.join(linked),
        )
        .unwrap();
    }
    directory.canonicalize().unwrap()
}

impl Packaged {
    /// Package and register two Revisions whose runner sources differ, so their
    /// Launch component hashes differ.
    pub fn install(root: &Path, node: &Path) -> Self {
        let base = root.join("packaged");
        let paths = ForgeupPaths {
            config: base.join("forgeup/config"),
            data: base.join("forgeup/data"),
            runtime: base.join("forgeup/runtime"),
            bin: base.join("forgeup/bin"),
        };
        let host = host_binary();
        let first_source = base.join("first-bundle");
        source_bundle(&first_source, node, &host);
        let second_source = base.join("second-bundle");
        link_tree(&first_source, &second_source);
        fs::write(
            second_source.join("runner/src/zz-second-revision.ts"),
            "// Present only in the second Revision's runner sources.\n",
        )
        .unwrap();
        write_manifest(&second_source);
        let first = register(&base, &paths, "first", &first_source);
        let second = register(&base, &paths, "second", &second_source);
        let (one, two) = (
            first.runner.as_ref().unwrap(),
            second.runner.as_ref().unwrap(),
        );
        assert!(one.launchable() && two.launchable());
        assert_ne!(one.component_sha256, two.component_sha256);
        // Installed copies are what run; the sources can disappear.
        fs::remove_dir_all(&first_source).unwrap();
        fs::remove_dir_all(&second_source).unwrap();

        let worker = worker_directory(&base.join("first-worker"), one);
        let second_worker = worker_directory(&base.join("second-worker"), two);
        Self {
            paths,
            first,
            second,
            worker,
            second_worker,
        }
    }

    /// The first Revision's Launch component hash.
    pub fn first_component(&self) -> String {
        component(&self.first)
    }

    /// Exactly the durable composition a managed daemon of `installation` gets:
    /// Forgeup's daemon-start arguments parsed by `DaemonConfig::managed`.
    pub fn daemon_components(&self, installation: &Installation) -> (Execution, Vec<Execution>) {
        let registry = forgeup::RegistryStore::open(self.paths.clone())
            .unwrap()
            .load()
            .unwrap();
        let arguments =
            forgeup::managed_durable_arguments(&self.paths, &registry, installation).unwrap();
        let mut inputs = ManagedLaunchInputs {
            bind_addr: "127.0.0.1:0".into(),
            store_path: "forge.sqlite3".into(),
            runtime_record_path: "runtime.json".into(),
            lifecycle_control_endpoint: "control.sock".into(),
            lifecycle_credential: "credential".into(),
            daemon_instance_id: "instance".into(),
            durable_node: None,
            durable_inspector: None,
            durable_runner_bundle: None,
            durable_retained_runner_bundles: Vec::new(),
        };
        for pair in arguments.chunks(2) {
            let value = pair[1].to_str().unwrap().to_owned();
            match pair[0].to_str().unwrap() {
                "--durable-runner-bundle" => inputs.durable_runner_bundle = Some(value),
                "--durable-retained-runner-bundle" => {
                    inputs.durable_retained_runner_bundles.push(value);
                }
                other => panic!("unexpected managed durable argument {other}"),
            }
        }
        let config = DaemonConfig::managed(inputs).unwrap();
        (config.durable_execution.unwrap(), config.durable_retained)
    }

    /// The tracer's single entry substitution: the faux-model worker running
    /// the owning Installation's packaged sources.
    pub fn substitute(&self, mut execution: Execution) -> Execution {
        let first = &self.first.runner.as_ref().unwrap().root;
        let worker = if execution.components.node.starts_with(first) {
            &self.worker
        } else {
            &self.second_worker
        };
        execution.components.runner = worker.join("test/supervised-check-worker.mjs");
        execution
    }

    pub fn compose(
        database: &Path,
        current: &Execution,
        retained: &[Execution],
        credential: &Path,
    ) -> forge_server::durable_authorization::retirement::Factory {
        compose(
            database,
            None,
            Some(current),
            retained,
            Some(credential.into()),
            Box::new(supervised_dispatch::NoHerdr),
        )
        .unwrap()
    }
}

fn component(installation: &Installation) -> String {
    installation
        .runner
        .as_ref()
        .unwrap()
        .component_sha256
        .clone()
        .unwrap()
}

/// The host and worker of a live runtime execute the given Installation's
/// packaged host and Node (by `/proc/<pid>/exe`), never another copy.
pub fn assert_runs_from(directory: &Path, installation: &Installation) {
    let runner = installation.runner.as_ref().unwrap();
    let owner = supervised_dispatch::owner(directory);
    let exe = |pid: &Value| fs::read_link(format!("/proc/{}/exe", pid.as_u64().unwrap())).unwrap();
    assert_eq!(
        &exe(&owner["owner"]["host_pid"]),
        runner.host_path.as_ref().unwrap()
    );
    assert_eq!(exe(&owner["worker_pid"]), runner.node_path);
}

async fn serve(
    dispatch: &SupervisedDispatch,
    catalog: ComponentCatalog,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = Daemon::new(dispatch.app())
        .with_assignment_capability_root(dispatch.root.join("assignment-capabilities"))
        .with_durable_operator_catalog(&dispatch.database, catalog)
        .unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(
            listener,
            router(server).into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (format!("http://{address}/rpc"), handle)
}

/// Run the real CLI on the blocking pool: the embedded daemon spawned from this
/// task must stay schedulable while the CLI waits for it.
async fn cli(dispatch: &SupervisedDispatch, endpoint: &str, args: &[&str]) -> std::process::Output {
    let mut command = Command::new(&dispatch.cli);
    command
        .env_clear()
        .arg("dispatcher")
        .args(args)
        .args(["--json", "--server-url", endpoint]);
    tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap()
}

/// The daemon restarts under the second Revision while the crashed Assignment
/// from the first is retained. Without the first Revision's verified bundle,
/// recovery and transcripts refuse explicitly and nothing runs. With it, they
/// use the first Revision's original packaged components: recovery resumes the
/// same attempt to an accepted Outcome, and retirement reconciles through the
/// original effect inspector. Finally a new Task dispatched by the second
/// Revision's daemon runs from its own packaged components to an accepted Outcome.
#[allow(clippy::too_many_lines)] // One ordered Revision-switch journey.
pub async fn switch(
    adapter: &SupervisedDispatch,
    store: &SqliteProjectRepository,
    base: &str,
    report: supervised_dispatch::Report,
    packaged: &Packaged,
) {
    let launch = report.launch.clone();
    let id = launch.assignment_id.clone();
    let directory = report.directory.clone();
    assert_eq!(launch.component_hash, packaged.first_component());
    let context = adapter
        .deliveries
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .unwrap();
    let input = fs::read(directory.join("check-runtime.json")).unwrap();
    let attempt = store.assignment_check_attempt(&id).unwrap().unwrap();
    let owner = fs::read(directory.join("owner.json")).unwrap();

    // 1. The first Revision's bundle no longer verifies (tampered entry), so
    //    Forgeup does not offer it; the second Revision's daemon has only its own.
    let first = packaged.first.runner.clone().unwrap();
    let original = fs::read(&first.entry_path).unwrap();
    fs::write(
        &first.entry_path,
        [original.as_slice(), b"// tampered"].concat(),
    )
    .unwrap();
    let (current, retained) = packaged.daemon_components(&packaged.second);
    fs::write(&first.entry_path, &original).unwrap();
    assert!(retained.is_empty());
    let current = packaged.substitute(current);
    let switched = SupervisedDispatch {
        production: Arc::new(Packaged::compose(
            &adapter.database,
            &current,
            &[],
            &adapter.credential,
        )),
        ..adapter.clone()
    };
    let recovery = switched
        .production
        .adapter_for(&context.assignment.profile)
        .unwrap()
        .recover(&context)
        .unwrap_err();
    assert!(
        format!("{recovery:?}").contains("durable-components-unavailable"),
        "{recovery:?}"
    );
    let (endpoint, server) = serve(
        &switched,
        ComponentCatalog::new(Some(&current.inspection()), Some(&current), &[]),
    )
    .await;
    let transcript = cli(
        &switched,
        &endpoint,
        &["assignment-transcript", &id, "--workspace", "test"],
    )
    .await;
    assert!(!transcript.status.success());
    let stderr = String::from_utf8_lossy(&transcript.stderr);
    assert!(
        stderr.contains("durable-components-unavailable"),
        "{stderr}"
    );
    assert!(stderr.contains(&launch.component_hash));
    server.abort();
    // Nothing was claimed, spawned or rewritten, and nothing ran under the new code.
    assert_eq!(fs::read(directory.join("owner.json")).unwrap(), owner);
    assert_eq!(store.runtime("test", &id).unwrap().unwrap().launch, launch);
    assert!(
        store
            .agent_assignment_info(&id)
            .unwrap()
            .capacity_reserved()
    );

    // 2. Both Revisions verify: the second Revision's daemon is also offered the
    //    first Revision's bundle, used only for this retained Launch.
    let (current, retained) = packaged.daemon_components(&packaged.second);
    assert_eq!(retained.len(), 1);
    assert_eq!(
        &retained[0].components.host,
        first.host_path.as_ref().unwrap()
    );
    let current = packaged.substitute(current);
    let retained: Vec<_> = retained
        .into_iter()
        .map(|e| packaged.substitute(e))
        .collect();
    let production = Arc::new(Packaged::compose(
        &adapter.database,
        &current,
        &retained,
        &adapter.credential,
    ));
    let switched = SupervisedDispatch {
        production: production.clone(),
        ..adapter.clone()
    };
    let (endpoint, server) = serve(
        &switched,
        ComponentCatalog::new(Some(&current.inspection()), Some(&current), &retained),
    )
    .await;
    let transcript = cli(
        &switched,
        &endpoint,
        &["assignment-transcript", &id, "--workspace", "test"],
    )
    .await;
    assert!(
        transcript.status.success(),
        "{}",
        String::from_utf8_lossy(&transcript.stderr)
    );
    let page: forge_rpc::AssignmentTranscriptResponse =
        serde_json::from_slice(&transcript.stdout).unwrap();
    assert_eq!(page.assignment_id, id);
    assert_eq!(page.history, forge_rpc::TranscriptHistory::Available);
    assert!(!page.entries.is_empty());

    // Ordinary reconciliation of the second Revision's application recovers the
    // same attempt with the first Revision's packaged host, Node and sources.
    let reconciling = switched.clone();
    tokio::task::spawn_blocking(move || {
        let now = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis(),
        )
        .unwrap();
        reconciling
            .app()
            .reconcile_agent_dispatch("supervised-tracer", now)
            .unwrap();
    })
    .await
    .unwrap();
    let controller = production.recovery.clone().unwrap();
    let recovered = launch.clone();
    tokio::task::spawn_blocking(move || {
        supervised_dispatch::wait(|| {
            (controller.recover("test", &recovered).ok() == Some(AgentObservation::Idle))
                .then_some(())
        });
    })
    .await
    .unwrap();
    // The recovered runtime runs the first Revision's packaged host and Node,
    // never the second's.
    assert_runs_from(&directory, &packaged.first);
    assert_eq!(
        fs::read(directory.join("check-runtime.json")).unwrap(),
        input
    );
    assert_eq!(store.runtime("test", &id).unwrap().unwrap().launch, launch);
    assert_eq!(
        store.assignment_check_attempt(&id).unwrap().unwrap(),
        attempt
    );
    assert_eq!(
        store.observe_answer("test", &launch).unwrap(),
        AnswerObservation::OutcomeAccepted
    );

    // Retirement through the second Revision's daemon reconciles with the first
    // Revision's effect inspector and releases capacity.
    let retire = cli(&switched, &endpoint, &["assignment-retire", &id]).await;
    assert!(
        retire.status.success(),
        "{}",
        String::from_utf8_lossy(&retire.stderr)
    );
    let retired = store.agent_assignment_info(&id).unwrap();
    assert!(retired.assignment.closed_at_millis.is_some());
    assert!(!retired.capacity_reserved());
    server.abort();
    let secret = fs::read_to_string(&report.capability_file).unwrap();
    let mut report = report;
    report.finish_retired(&secret);

    // 3. A new Task dispatched by the second Revision's daemon runs end to end
    //    from that Installation's packaged components to an accepted Outcome.
    let fresh = SupervisedDispatch {
        component_hash: component(&packaged.second),
        node: current.components.node.clone(),
        runner: packaged.second_worker.clone(),
        mode: "packaged-outcome".into(),
        provisions: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        ..switched
    };
    fresh.write_fixture();
    let record = super::task(store, "Packaged second Revision");
    store.set_task_base_commit(record, base).unwrap();
    let dispatch = fresh.clone();
    let report = tokio::task::spawn_blocking(move || {
        let (assignment, existing) = dispatch.app().dispatch_record(record).unwrap();
        assert!(!existing);
        dispatch.drive(&assignment)
    })
    .await
    .unwrap();
    assert_eq!(report.launch.component_hash, component(&packaged.second));
    assert_eq!(report.answer, Some(AnswerObservation::OutcomeAccepted));
    assert_runs_from(&report.directory, &packaged.second);
    supervised_dispatch::terminate(&report.directory);
}
