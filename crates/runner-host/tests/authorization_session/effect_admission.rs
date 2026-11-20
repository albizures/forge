//! Actual lock-owning supervisor -> controller -> committed Node inspector.
//! Runtime tasks are storage fixtures, not real external mutations (those have
//! separate Bash/process/endpoint tests). No production recovery is enabled.
use super::*;
use forge_server::durable_authorization::operator::InspectionComponents;
use std::io::Write;

pub(super) fn components() -> InspectionComponents {
    let output = Command::new("node")
        .args(["--print", "process.execPath"])
        .output()
        .unwrap();
    assert!(output.status.success());
    InspectionComponents {
        node: PathBuf::from(String::from_utf8(output.stdout).unwrap().trim()),
        inspector: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../runner/src/inspect-effects.ts")
            .canonicalize()
            .unwrap(),
    }
}

pub(super) fn component_hash(components: &InspectionComponents) -> String {
    let contract = components.inspector.with_file_name("contract.ts");
    let output = Command::new(&components.node)
        .env_clear()
        .args(["--input-type=module", "-e"])
        .arg(format!(
            "import {{componentHash}} from {}; process.stdout.write(componentHash());",
            serde_json::json!(contract)
        ))
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

pub(super) fn seed(f: &Fixture, components: &InspectionComponents, mode: &str) {
    let script = components
        .inspector
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test/effect-admission-fixture.mjs");
    let mut child = Command::new(&components.node)
        .env_clear()
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            &serde_json::to_vec(&serde_json::json!({
                "directory": f.directory, "launch": f.launch, "mode": mode,
            }))
            .unwrap(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn lease_count(database: &Path) -> i64 {
    rusqlite::Connection::open(database)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM assignment_authorization_leases",
            [],
            |r| r.get(0),
        )
        .unwrap()
}

fn denied_fixture(mode: &str, components: &InspectionComponents) -> Fixture {
    let hash = if mode == "incompatible" {
        "unmatched-component".into()
    } else {
        component_hash(components)
    };
    let f = Fixture::with_component("model", hash);
    if mode == "sidecar" {
        fs::write(f.directory.join("runtime.sqlite-wal"), b"retained-sidecar").unwrap();
        fs::set_permissions(
            f.directory.join("runtime.sqlite-wal"),
            fs::Permissions::from_mode(0o600),
        )
        .unwrap();
    } else if mode == "accepted-missing" {
        use forge_app::durable_execution::Admission;
        f.store.begin_send("test", &f.launch).unwrap();
        f.store
            .record_result(
                "test",
                &f.launch,
                &Admission::Accepted {
                    admission_id: "original".into(),
                },
            )
            .unwrap();
    } else {
        seed(&f, components, mode);
    }
    f
}

#[test]
fn owned_controller_retains_interrupted_intent_before_any_grant_and_rejects_bypass() {
    let components = components();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for mode in [
        "interrupted",
        "missing-root",
        "unconfigured",
        "legacy",
        "sidecar",
        "accepted-missing",
        "unavailable",
        "incompatible",
        "retention-failed",
    ] {
        let mut f = denied_fixture(mode, &components);
        if mode == "retention-failed" {
            rusqlite::Connection::open(&f.database).unwrap().execute_batch("CREATE TRIGGER fail_effect_retention BEFORE INSERT ON agent_assignment_resources WHEN NEW.resource_kind = 'interrupted-effect' BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
        }
        let before = fs::read(f.directory.join("runtime.sqlite")).ok();
        let attempt = f.store.assignment_check_attempt("assignment").unwrap();
        let mut selected = components.clone();
        if mode == "unavailable" {
            selected.node = PathBuf::from("/missing-node");
        }
        let mut authority = DaemonAuthority::new().unwrap();
        if mode != "unconfigured" {
            authority = authority.with_inspection(selected).unwrap();
        }
        let result = if mode == "legacy" {
            authority.bind(&f.store, "test".into(), f.launch.clone(), 1, &mut f.child)
        } else {
            runtime.block_on(authority.bind_inspected(
                &f.store,
                "test".into(),
                f.launch.clone(),
                1,
                &mut f.child,
            ))
        };
        assert!(result.is_err(), "{mode} unexpectedly authorized");
        drop(result);
        assert_eq!(lease_count(&f.database), 0, "{mode}");
        assert!(!f.directory.join("controlled.sqlite").exists());
        assert_eq!(fs::read(f.directory.join("runtime.sqlite")).ok(), before);
        let reopened = open(&f.database);
        let info = reopened.agent_assignment_info("assignment").unwrap();
        assert!(info.capacity_reserved());
        assert!(info.dispatch_hold.is_some());
        assert_eq!(
            reopened.assignment_check_attempt("assignment").unwrap(),
            attempt
        );
        if mode == "interrupted" {
            assert!(
                info.recovery_blockers()
                    .contains(&"interrupted-effect".into())
            );
            let observation = forge_server::durable_authorization::effects::inspect_and_retain;
            // A new daemon identity and connection cannot erase the effect with
            // another read-only scan or an explicit hold clearance.
            let _restarted = DaemonAuthority::new().unwrap();
            reopened.resume_dispatch(f.record).unwrap();
            runtime
                .block_on(observation(
                    &reopened,
                    "test",
                    &f.launch,
                    &components.node,
                    &components.inspector,
                ))
                .unwrap();
            assert_eq!(
                reopened
                    .agent_assignment_info("assignment")
                    .unwrap()
                    .resources,
                info.resources
            );
            assert_eq!(lease_count(&f.database), 0);
        }
    }
}

fn resolve_fixture(
    f: &Fixture,
    components: &InspectionComponents,
    runtime: &tokio::runtime::Runtime,
) -> forge_app::durable_execution::effect_resolution::ResolutionReceipt {
    use forge_app::durable_execution::{
        effect_resolution::{ExternalOutcome, Resolution},
        effects::InterruptedEffect,
    };
    use forge_server::durable_authorization::effects::{inspect_and_retain, resolve_exclusive};
    runtime
        .block_on(inspect_and_retain(
            &f.store,
            "test",
            &f.launch,
            &components.node,
            &components.inspector,
        ))
        .unwrap();
    let effect: InterruptedEffect = serde_json::from_str(
        &f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .resources
            .into_iter()
            .find(|r| r.resource_kind == "interrupted-effect")
            .unwrap()
            .resource_id,
    )
    .unwrap();
    let resolution = Resolution {
        operation_id: "investigated".into(),
        effect,
        outcome: ExternalOutcome::NotApplied,
        actor: "fixture-operator".into(),
        evidence: "Storage fixture; no tool ran".into(),
        message: "Record conclusion without runtime replay".into(),
        confirm_old_agent_cannot_act: true,
        confirm_external_work_safe: true,
    };
    runtime
        .block_on(resolve_exclusive(
            &f.store,
            "test",
            &f.launch,
            &components.node,
            &components.inspector,
            &resolution,
        ))
        .unwrap()
}

#[test]
fn resolved_external_audit_never_waives_unsettled_intent_on_owned_restart() {
    use forge_app::durable_execution::effect_resolution::Repository as _;
    let components = components();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for closed in [false, true] {
        let mut f = Fixture::with_component("model", component_hash(&components));
        seed(&f, &components, "interrupted");
        let before = fs::read(f.directory.join("runtime.sqlite")).unwrap();
        // Fixture teardown of our own ungranted worker, not product retirement.
        // It has never executed tools or spawned external children.
        let pid = u32::try_from(f.evidence["worker_pid"].as_u64().unwrap()).unwrap();
        rustix::process::kill_process_group(
            rustix::process::Pid::from_raw(pid.cast_signed()).unwrap(),
            rustix::process::Signal::KILL,
        )
        .unwrap();
        diagnostic(&f.errors, "runner-worker-exited");
        f.child.kill().unwrap();
        f.child.wait().unwrap();
        if closed {
            f.store
                .close_agent_assignment(
                    "assignment",
                    forge_app::AgentAssignmentClosureReason::Retired,
                )
                .unwrap();
        }
        let receipt = resolve_fixture(&f, &components, &runtime);
        let audit = f
            .store
            .agent_assignment_info("assignment")
            .unwrap()
            .resources;
        let (child, stdout, stderr) = spawn(&f.directory, &f.root.join("worktree"));
        f.child = child;
        f.output = stdout;
        f.errors = stderr;
        f.evidence = event(&f.output, "owned")["identity"].clone();
        let restarted = DaemonAuthority::new()
            .unwrap()
            .with_inspection(components.clone())
            .unwrap();
        let result = runtime.block_on(restarted.bind_inspected(
            &open(&f.database),
            "test".into(),
            f.launch.clone(),
            1,
            &mut f.child,
        ));
        assert!(
            matches!(result, Err(ref error) if error.code == "durable-interrupted-effect-blocked")
        );
        drop(result);
        assert_eq!(lease_count(&f.database), 0);
        let reopened = open(&f.database);
        assert_eq!(
            reopened
                .accepted_effect_resolution("test", &f.launch, &receipt.resolution)
                .unwrap(),
            Some(receipt)
        );
        let retained = reopened.agent_assignment_info("assignment").unwrap();
        assert!(retained.capacity_reserved());
        for original in &audit {
            assert!(retained.resources.contains(original));
        }
        assert_eq!(
            fs::read(f.directory.join("runtime.sqlite")).unwrap(),
            before
        );
        assert!(!f.directory.join("controlled.sqlite").exists());
    }
}

#[test]
fn owned_controller_accepts_committed_receipt_without_replaying_tools() {
    let components = components();
    let mut f = Fixture::with_component("model", component_hash(&components));
    seed(&f, &components, "completed");
    let before = fs::read(f.directory.join("runtime.sqlite")).unwrap();
    let authority = DaemonAuthority::new()
        .unwrap()
        .with_inspection(components)
        .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let mut service = runtime
        .block_on(authority.bind_inspected(
            &f.store,
            "test".into(),
            f.launch.clone(),
            1,
            &mut f.child,
        ))
        .unwrap();
    assert_eq!(lease_count(&f.database), 0); // inspection itself delivered nothing
    assert!(matches!(
        service.tick(&f.store).unwrap(),
        Tick::Delivered { .. }
    ));
    assert_eq!(event(&f.output, "authorized")["sequence"], 1);
    event(&f.output, "started");
    drop(service);
    assert_eq!(lease_count(&f.database), 1);
    assert_eq!(
        fs::read(f.directory.join("runtime.sqlite")).unwrap(),
        before
    );
    assert!(
        !f.store
            .agent_assignment_info("assignment")
            .unwrap()
            .recovery_blockers()
            .contains(&"interrupted-effect".into())
    );
}
