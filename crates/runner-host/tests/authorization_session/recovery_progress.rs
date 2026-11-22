//! Actual SQLite + Git + subreaper + waiting owner + read-only Node inspection.
//! No combined protected proof, fresh lease or writable runtime is produced.
use super::*;
use forge_runner_host::inspection::ExclusiveInspection;
use forge_server::durable_authorization::waiting_recovery_inspection::inspect;

fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}

pub(super) fn repository(root: &Path) -> String {
    git(root, &["init"]);
    fs::write(root.join("tracked"), "base\n").unwrap();
    git(root, &["add", "tracked"]);
    git(
        root,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "base",
        ],
    );
    String::from_utf8(git(root, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned()
}

pub(super) fn worktree(root: &Path, record: u64) -> PathBuf {
    let path = forge_app::managed_worktree_path(root, "alpha", record);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::set_permissions(root.join(".forge"), fs::Permissions::from_mode(0o700)).unwrap();
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            &forge_app::stacks::task_branch(record),
            path.to_str().unwrap(),
        ],
    );
    path
}

fn waiting() -> (
    Fixture,
    forge_app::durable_execution::recovery::Claim,
    PathBuf,
) {
    waiting_with_runner(None)
}

pub(super) fn waiting_with_runner(
    runner: Option<&Path>,
) -> (
    Fixture,
    forge_app::durable_execution::recovery::Claim,
    PathBuf,
) {
    let components = effect_admission::components();
    let mut f = Fixture::with_repository(
        "retire-delayed",
        effect_admission::component_hash(&components),
        true,
    );
    let path = forge_app::managed_worktree_path(&f.root, "alpha", f.record);
    let (mut pipe, operation, _) = quiescence::prepare_admission_at(&mut f, Some("queued"), &path);
    quiesced_inspection::quiesce(&mut f, &mut pipe, &operation);
    let (child, output, errors) = match runner {
        Some(runner) => spawn_component(&f.directory, &path, runner),
        None => spawn(&f.directory, &path),
    };
    f.child = child;
    f.output = output;
    f.errors = errors;
    f.evidence = event(&f.output, "owned")["identity"].clone();
    (f, operation.claim, path)
}

#[test]
fn recovery_progress_combines_real_proofs_without_adopting_or_changing_work() {
    let (mut f, claim, path) = waiting();
    fs::write(path.join("tracked"), "dirty retained implementation\n").unwrap();
    fs::write(path.join("untracked"), "retained\n").unwrap();
    let components = effect_admission::components();
    let inspector = components.inspector.with_file_name("inspect-recovery.ts");
    let runtime = quiesced_inspection::runtime();
    let before = recovery_inspection::runtime_bytes(&f.directory);
    let attempt = f.store.assignment_check_attempt("assignment").unwrap();
    let info = f.store.agent_assignment_info("assignment").unwrap();
    let mut proof = runtime.block_on(async {
        inspect(
            &f.store,
            "test",
            &claim,
            &mut f.child,
            &components.node,
            &inspector,
        )
        .await
        .unwrap()
        .inspect_progress(&f.store, "test", &components.node, &inspector)
        .await
        .unwrap()
    });
    assert!(proof.progress().observation().dirty);
    assert_eq!(proof.progress().binding().candidate, *proof.candidate());
    runtime.block_on(proof.verify(&f.store)).unwrap();
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    assert_eq!(recovery_inspection::runtime_bytes(&f.directory), before);
    assert_eq!(
        f.store.assignment_check_attempt("assignment").unwrap(),
        attempt
    );
    assert_eq!(f.store.agent_assignment_info("assignment").unwrap(), info);
    assert_eq!(
        fs::read_to_string(path.join("tracked")).unwrap(),
        "dirty retained implementation\n"
    );
    assert_eq!(
        fs::read_to_string(path.join("untracked")).unwrap(),
        "retained\n"
    );
    drop(proof);
    assert!(f.child.try_wait().unwrap().is_none());
    assert!(ExclusiveInspection::acquire(&f.directory).is_err());
    assert!(
        !f.output
            .try_iter()
            .any(|s| s.contains("authorized") || s.contains("started"))
    );
}

#[test]
fn recovery_progress_failure_terminally_fences_even_after_original_bytes_are_restored() {
    for fault in 0..5 {
        let (mut f, claim, path) = waiting();
        let components = effect_admission::components();
        let inspector = components.inspector.with_file_name("inspect-recovery.ts");
        let runtime = quiesced_inspection::runtime();
        let mut proof = runtime.block_on(async {
            inspect(
                &f.store,
                "test",
                &claim,
                &mut f.child,
                &components.node,
                &inspector,
            )
            .await
            .unwrap()
            .inspect_progress(&f.store, "test", &components.node, &inspector)
            .await
            .unwrap()
        });
        let receipt = f.directory.join("recovery-quiescence-receipt.json");
        let original = fs::read(&receipt).unwrap();
        match fault {
            0 => fs::write(path.join("tracked"), "changed\n").unwrap(),
            1 => fs::remove_file(&receipt).unwrap(),
            2 => {
                f.store
                    .close_agent_assignment(
                        "assignment",
                        forge_app::AgentAssignmentClosureReason::Retired,
                    )
                    .unwrap();
            }
            3 => fs::write(f.directory.join("watchdog.json"), "PRIVATE-CANARY").unwrap(),
            4 => fs::rename(
                f.directory.join("runtime.sqlite"),
                f.directory.join("held-runtime"),
            )
            .unwrap(),
            _ => unreachable!(),
        }
        assert!(runtime.block_on(proof.verify(&f.store)).is_err());
        match fault {
            0 => fs::write(path.join("tracked"), "base\n").unwrap(),
            1 => {
                fs::write(&receipt, original).unwrap();
                fs::set_permissions(&receipt, fs::Permissions::from_mode(0o600)).unwrap();
            }
            3 => fs::remove_file(f.directory.join("watchdog.json")).unwrap(),
            4 => fs::rename(
                f.directory.join("held-runtime"),
                f.directory.join("runtime.sqlite"),
            )
            .unwrap(),
            _ => {}
        }
        assert!(runtime.block_on(proof.verify(&f.store)).is_err());
        assert!(
            f.store
                .agent_assignment_info("assignment")
                .unwrap()
                .capacity_reserved()
        );
    }
}
