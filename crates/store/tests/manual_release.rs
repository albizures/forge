use forge_app::check_templates::{CheckCriterion, TemplateReplacement};
use forge_app::checks::Verdict;
use forge_app::findings::Severity;
use forge_app::outcomes::{
    AttemptInput, Evidence, FindingVerification, OutcomeDisposition, OutcomeFinding, OutcomeInput,
    WorkInvalidationInput,
};
use forge_app::{
    ExecutionMode, NewTypedRecord, Project, ProjectRepository, RecordRepository, RecordStatus,
    RecordType, WorkspaceRepository,
};
use forge_store::SqliteProjectRepository;

struct TemporaryRoot(std::path::PathBuf);
impl TemporaryRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "forge-manual-release-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for TemporaryRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn evidence(repository: bool, revision: &str) -> Evidence {
    if repository {
        Evidence::Repository {
            repository: "release/api".into(),
            commit: revision.repeat(40),
        }
    } else {
        Evidence::Artifact {
            uri: format!("urn:release:report:{revision}"),
            sha256: revision.repeat(64),
        }
    }
}

fn begin(
    store: &SqliteProjectRepository,
    task: u64,
    check: u64,
    id: &str,
    generation: u64,
    input: &Evidence,
) -> OutcomeInput {
    store
        .begin_record_attempt(
            task,
            &AttemptInput {
                id: id.into(),
                check_id: check,
                generation,
                evidence: input.clone(),
                actor: "human".into(),
            },
        )
        .unwrap();
    OutcomeInput {
        attempt_id: id.into(),
        generation,
        evidence: input.clone(),
        actor: "human".into(),
        message: "Verified deliverable".into(),
        verdict: Verdict::Pass,
        disposition: OutcomeDisposition::Complete,
        findings: vec![],
        verifications: vec![],
    }
}

#[test]
#[allow(clippy::too_many_lines)]
fn complete_manual_release_survives_restart_with_repository_and_artifact_evidence() {
    for repository in [true, false] {
        let root = TemporaryRoot::new();
        let path = root.path().join("release.sqlite");
        let open = || {
            SqliteProjectRepository::open(&path)
                .unwrap()
                .in_record_workspace("release")
                .unwrap()
        };
        let store = SqliteProjectRepository::open(&path).unwrap();
        store
            .create_workspace("release", "Release", root.path())
            .unwrap();
        store
            .register(&Project {
                reference: "release/api".into(),
                workspace_id: "release".into(),
                slug: "api".into(),
                project_root: root.path().canonicalize().unwrap(),
                origin_url: "https://example.test/api.git".into(),
            })
            .unwrap();
        drop(store);
        let store = open();
        store
            .replace_check_template(
                "release/api",
                &TemplateReplacement {
                    expected_version: 0,
                    actor: "human".into(),
                    message: "Release checks".into(),
                    criteria: ["Implementation", "Review 1", "Review 2"]
                        .iter()
                        .enumerate()
                        .map(|(index, name)| CheckCriterion {
                            name: (*name).into(),
                            description: "Verify deliverable".into(),
                            profile_revision: Some(u32::try_from(index).unwrap() + 1),
                        })
                        .collect(),
                },
            )
            .unwrap();
        let task = store
            .open_typed_record(&NewTypedRecord {
                record_type: RecordType::Task,
                body: "Deliver report".into(),
                project_slugs: vec!["release/api".into()],
                generated_by_ids: vec![],
            })
            .unwrap()
            .id;
        store
            .set_record_execution_mode(
                task,
                ExecutionMode::Dispatched,
                ExecutionMode::Manual,
                "human",
                "No agents required",
            )
            .unwrap();
        store.start_record(task).unwrap();
        let checks = store.list_record_checks(task).unwrap();
        let a = evidence(repository, "a");
        let b = evidence(repository, "b");
        let first = begin(&store, task, checks[0].id, "initial", 1, &a);
        // Explicit same-evidence invalidation closes the outstanding attempt.
        store
            .invalidate_record_work(
                task,
                &WorkInvalidationInput {
                    generation: 1,
                    evidence: a.clone(),
                    actor: "human".into(),
                    message: "Reverify unchanged evidence".into(),
                },
            )
            .unwrap();
        for (index, id) in ["implementation-a", "review-1-a"].iter().enumerate() {
            let input = begin(&store, task, checks[index].id, id, 2, &a);
            store.submit_record_outcome(task, &input).unwrap();
        }
        let mut handback = begin(&store, task, checks[2].id, "review-2-a", 2, &a);
        handback.verdict = Verdict::Fail;
        handback.disposition = OutcomeDisposition::Handback;
        handback.findings.push(OutcomeFinding {
            severity: Severity::Major,
            blocking: Some(true),
            message: "Correct report".into(),
        });
        let accepted = store.submit_record_outcome(task, &handback).unwrap();
        assert_eq!(accepted.work.generation, 3);
        assert!(
            store
                .list_record_checks(task)
                .unwrap()
                .iter()
                .all(|c| c.current_verdict.is_none())
        );
        assert!(store.done_record(task, None).is_err());
        // An unaccepted late submission is stale, unlike exact accepted replay.
        assert_eq!(
            store.submit_record_outcome(task, &first).unwrap_err().code,
            "stale-generation"
        );
        let history = store.list_record_checks(task).unwrap();
        let work = store.record_work_state(task).unwrap();
        drop(store);
        let store = open();
        assert_eq!(store.list_record_checks(task).unwrap(), history);
        assert_eq!(store.record_work_state(task).unwrap(), work);
        assert_eq!(
            store.submit_record_outcome(task, &handback).unwrap(),
            accepted
        );
        let mut remediation = begin(&store, task, checks[0].id, "implementation-b", 3, &a);
        remediation.evidence = b.clone();
        store.submit_record_outcome(task, &remediation).unwrap();
        // Review 2 cannot skip the required Review 1 rerun.
        assert!(
            store
                .begin_record_attempt(
                    task,
                    &AttemptInput {
                        id: "out-of-order".into(),
                        check_id: checks[2].id,
                        generation: 3,
                        evidence: b.clone(),
                        actor: "human".into(),
                    }
                )
                .is_err()
        );
        let review = begin(&store, task, checks[1].id, "review-1-b", 3, &b);
        store.submit_record_outcome(task, &review).unwrap();
        let mut review = begin(&store, task, checks[2].id, "review-2-b", 3, &b);
        assert_eq!(
            store.submit_record_outcome(task, &review).unwrap_err().code,
            "unresolved-check-findings"
        );
        review.verifications.push(FindingVerification {
            finding_id: accepted.findings[0].id,
            expected_version: accepted.findings[0].version,
            message: "Verified correction in B".into(),
        });
        store.submit_record_outcome(task, &review).unwrap();
        let final_checks = store.list_record_checks(task).unwrap();
        assert!(
            final_checks
                .iter()
                .all(|c| c.current_verdict == Some(Verdict::Pass))
        );
        assert_eq!(
            final_checks
                .iter()
                .map(|c| (c.id, c.profile_revision))
                .collect::<Vec<_>>(),
            checks
                .iter()
                .map(|c| (c.id, c.profile_revision))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            store.show_record(task).unwrap().status,
            RecordStatus::InProgress
        );
        store
            .done_record(task, Some("Human verified release"))
            .unwrap();
        let findings = store.list_record_findings(task, None, 100).unwrap();
        drop(store);
        let store = open();
        assert_eq!(store.show_record(task).unwrap().status, RecordStatus::Done);
        assert_eq!(store.list_record_checks(task).unwrap(), final_checks);
        assert_eq!(
            store.list_record_findings(task, None, 100).unwrap(),
            findings
        );
        assert_eq!(
            store.submit_record_outcome(task, &handback).unwrap(),
            accepted
        );
        assert_eq!(store.record_work_state(task).unwrap().output, Some(b));
    }
}
