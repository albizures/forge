use super::*;
use forge_app::check_templates::{CheckCriterion, TemplateReplacement};

fn replacement(version: u64, names: &[&str]) -> TemplateReplacement {
    TemplateReplacement {
        expected_version: version,
        criteria: std::iter::once("Implementation").chain(names.iter().copied())
            .map(|name| CheckCriterion {
                name: name.into(),
                description: format!("Establish {name}"),
                profile_revision: None,
            })
            .collect(),
        actor: "operator".into(),
        message: "Set required criteria".into(),
    }
}

#[test]
fn profile_revision_is_snapshotted_including_the_implementation_binding() {
    let repository = super::SqliteProjectRepository::in_memory().unwrap();
    repository.register(&project("alpha", "/pinned-template")).unwrap();
    let input: TemplateReplacement = serde_json::from_value(serde_json::json!({
        "expected_version": 0, "actor": "operator", "message": "Pin configuration",
        "criteria": [{"name": "Implementation", "description": "Deliver the work", "profile_revision": 1}]
    })).unwrap();
    repository.replace_check_template("alpha", &input).unwrap();
    let task = repository.open_typed_record(&NewTypedRecord {
        record_type: RecordType::Task, project_slugs: vec!["alpha".into()],
        body: "Work".into(), generated_by_ids: vec![],
    }).unwrap();
    let checks = serde_json::to_value(repository.list_record_checks(task.id).unwrap()).unwrap();
    assert_eq!(checks[0]["profile_revision"], 1);
}

#[test]
fn stack_template_preflight_is_read_only_and_checks_every_pin() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    repository.register(&project("alpha", "/stack-preflight")).unwrap();
    let baseline = repository.show_check_template("alpha", None, 10).unwrap();
    assert!(baseline.stack_preflight.unwrap().blockers.iter().any(|b| b.contains("Publication")));
    let mut input = replacement(0, &["Review", "Publication"]);
    for criterion in &mut input.criteria {
        criterion.profile_revision = Some(1);
    }
    repository.replace_check_template("alpha", &input).unwrap();
    let before = repository.show_check_template("alpha", None, 10).unwrap();
    assert_eq!(before.stack_preflight.as_ref().unwrap().blockers.len(), 3);
    let policy = forge_app::agent_profiles::ResolvedLaunchPolicy {
        version: 1, runtime: "herdr/pi".into(), model: "test/model".into(), tools: vec![],
        behavior: "Verify".into(), skills: vec![], extensions: vec![], mcp_servers: vec![],
        thinking_level: None,
    };
    repository.register_agent_profile("test", RecordType::Task, &policy).unwrap();
    input.expected_version = 1;
    for criterion in &mut input.criteria {
        criterion.profile_revision = Some(2);
    }
    repository.replace_check_template("alpha", &input).unwrap();
    let good = repository.show_check_template("alpha", None, 10).unwrap();
    assert!(good.stack_preflight.as_ref().unwrap().blockers.is_empty());
    assert_eq!(repository.show_check_template("alpha", None, 10).unwrap(), good);
    input.expected_version = 2;
    input.criteria[1].profile_revision = Some(999);
    repository.replace_check_template("alpha", &input).unwrap();
    let missing = repository.show_check_template("alpha", None, 10).unwrap();
    assert_eq!(missing.stack_preflight.unwrap().blockers, vec!["Review pinned Task profile is unavailable"]);
    assert!(repository.show_check_template("missing", None, 10).is_err());
}

#[test]
fn preflight_then_cumulative_plan_readback_and_start_uses_returned_ids() {
    let repository = SqliteProjectRepository::in_memory().unwrap();
    repository.register(&project("alpha", "/planning-sequence")).unwrap();
    let parent = repository.open_typed_record(&NewTypedRecord {
        record_type: RecordType::Spec, project_slugs: vec!["alpha".into()],
        body: "Plan".into(), generated_by_ids: vec![],
    }).unwrap();
    let children = || repository.show_record(parent.id).unwrap().relationships;
    let before = children();
    // A planner stops before its first Task write when preflight is incompatible.
    let bad = repository.show_check_template("alpha", None, 10).unwrap();
    assert!(!bad.stack_preflight.unwrap().blockers.is_empty());
    assert_eq!(children(), before);
    let policy = forge_app::agent_profiles::ResolvedLaunchPolicy {
        version: 1, runtime: "herdr/pi".into(), model: "test/model".into(), tools: vec![],
        behavior: "Verify".into(), skills: vec![], extensions: vec![], mcp_servers: vec![],
        thinking_level: None,
    };
    let profile = repository.register_agent_profile("test", RecordType::Task, &policy).unwrap();
    let mut input = replacement(0, &["Publication"]);
    for criterion in &mut input.criteria {
        criterion.profile_revision = Some(profile.revision);
    }
    repository.replace_check_template("alpha", &input).unwrap();
    let good = repository.show_check_template("alpha", None, 10).unwrap();
    assert!(good.stack_preflight.unwrap().blockers.is_empty());
    let first = repository.open_task_under_spec("Implementation", parent.id, vec![]).unwrap();
    let root = forge_app::stacks::StackSource::Root {
        destination: forge_app::stacks::StackDestination {
            repository: "acme/widgets".into(), remote: "origin".into(), branch: "feature/input".into(),
        },
    };
    repository.configure_task_stack(first.id, &root).unwrap();
    // An unrelated Record makes arithmetic predecessor selection observably wrong.
    repository.open_record(&NewRecord { body: "Interleaving writer".into(), project_slugs: vec![], generated_by_ids: vec![] }).unwrap();
    let review = repository.open_task_under_spec("Review without edits", parent.id, vec![]).unwrap();
    assert!(review.id > first.id + 1);
    repository.relate_records(review.id, first.id, RecordRelationshipKind::DependsOn).unwrap();
    // Missing Stack configuration is a partial plan, not an ordering-only exception.
    assert_eq!(repository.start_record(parent.id).unwrap_err().code, "invalid-spec-plan");
    assert_eq!(repository.show_record(parent.id).unwrap().status, RecordStatus::Open);
    repository.configure_task_stack(review.id, &forge_app::stacks::StackSource::Predecessor { record_id: first.id }).unwrap();
    for id in [first.id, review.id] {
        let record = repository.show_record(id).unwrap();
        assert!(record.relationships.iter().any(|r| r.kind == RecordRelationshipKind::ParentChild && r.parent_id == parent.id));
        let checks = repository.list_record_checks(id).unwrap();
        assert_eq!(checks.last().unwrap().name, "Publication");
        assert!(checks.iter().all(|c| c.profile_revision == Some(profile.revision)));
    }
    assert_eq!(repository.task_stack(first.id).unwrap().unwrap().predecessor, None);
    assert_eq!(repository.task_stack(review.id).unwrap().unwrap().predecessor, Some(first.id));
    assert_eq!(repository.start_record(parent.id).unwrap().status, RecordStatus::InProgress);
    assert_eq!(repository.show_record(review.id).unwrap().status, RecordStatus::Open);
    assert_eq!(repository.show_check_template("alpha", None, 10).unwrap().version, good.version);
}

#[test]
fn templates_require_an_explicit_implementation_baseline() {
    let repository = super::SqliteProjectRepository::in_memory().unwrap();
    repository.register(&project("alpha", "/baseline-template")).unwrap();
    assert_eq!(repository.show_check_template("alpha", None, 10).unwrap().criteria[0].name, "Implementation");
    assert_eq!(repository.replace_check_template("alpha", &TemplateReplacement { criteria: vec![], ..replacement(0, &[]) }).unwrap_err().code, "invalid-check-template");
}

#[test]
#[allow(clippy::too_many_lines)]
fn invalid_template_replacements_are_atomic_and_workspace_confined() {
    let repository = super::SqliteProjectRepository::in_memory().unwrap();
    repository
        .register(&project("alpha", "/template-validation"))
        .unwrap();
    repository
        .replace_check_template("alpha", &replacement(0, &["Review", "Tests"]))
        .unwrap();
    let before = repository.show_check_template("alpha", None, 100).unwrap();
    for names in [
        vec!["implementation"],
        vec![" IMPLEMENTATION "],
        vec![" Review", "review "],
        vec![""],
        vec!["  "],
        vec!["École", "école"],
    ] {
        assert_eq!(
            repository
                .replace_check_template("alpha", &replacement(1, &names))
                .unwrap_err()
                .code,
            "invalid-check-template"
        );
        assert_eq!(
            repository.show_check_template("alpha", None, 100).unwrap(),
            before
        );
    }
    for invalid in [
        TemplateReplacement {
            actor: " ".into(),
            ..replacement(1, &["Review"])
        },
        TemplateReplacement {
            message: "\n".into(),
            ..replacement(1, &["Review"])
        },
        TemplateReplacement {
            criteria: vec![CheckCriterion {
                name: "Review".into(),
                description: " ".into(),
                profile_revision: None,
            }],
            ..replacement(1, &[])
        },
    ] {
        assert!(
            repository
                .replace_check_template("alpha", &invalid)
                .is_err()
        );
        assert_eq!(
            repository.show_check_template("alpha", None, 100).unwrap(),
            before
        );
    }
    for limit in [0, 101] {
        assert_eq!(
            repository
                .show_check_template("alpha", None, limit)
                .unwrap_err()
                .code,
            "invalid-page"
        );
    }
    let confined = crate::SqliteProjectRepository::in_memory().unwrap();
    let local_root = project("local", "/template-workspaces/local").project_root;
    let other_root = project("other", "/template-workspaces/other").project_root;
    confined
        .create_workspace("local", "Local", &local_root)
        .unwrap();
    confined
        .create_workspace("other", "Other", &other_root)
        .unwrap();
    confined
        .register(&Project {
            workspace_id: "local".into(),
            reference: "local/alpha".into(),
            slug: "alpha".into(),
            project_root: local_root,
            origin_url: "https://example.test/repo.git".into(),
        })
        .unwrap();
    let foreign = confined.records_in_workspace("other").unwrap();
    assert_eq!(
        foreign
            .show_check_template("local/alpha", None, 10)
            .unwrap_err()
            .code,
        "project-not-found"
    );
    assert_eq!(
        foreign
            .replace_check_template("local/alpha", &replacement(1, &[]))
            .unwrap_err()
            .code,
        "project-not-found"
    );
    assert_eq!(
        repository.show_check_template("alpha", None, 100).unwrap(),
        before
    );
    repository.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_template_history BEFORE INSERT ON project_check_template_changes BEGIN SELECT RAISE(ABORT, 'injected history failure'); END;").unwrap();
    assert!(
        repository
            .replace_check_template("alpha", &replacement(1, &[]))
            .is_err()
    );
    assert_eq!(
        repository.show_check_template("alpha", None, 100).unwrap(),
        before
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn templates_snapshot_all_task_deliverables_paths_and_selected_projects_without_affecting_updates() {
    let repository = super::SqliteProjectRepository::in_memory().unwrap();
    for slug in ["alpha", "beta", "unconfigured"] {
        repository
            .register(&project(slug, &format!("/template-paths/{slug}")))
            .unwrap();
    }
    repository
        .replace_check_template("alpha", &replacement(0, &["Review", "Tests"]))
        .unwrap();
    repository
        .replace_check_template("beta", &replacement(0, &["Docs"]))
        .unwrap();
    let open = |record_type, projects: Vec<String>, body: &str| {
        repository
            .open_typed_record(&NewTypedRecord {
                record_type,
                project_slugs: projects,
                body: body.into(),
                generated_by_ids: vec![],
            })
            .unwrap()
    };
    let spec = open(RecordType::Spec, vec!["alpha".into()], "Plan");
    let map = open(
        RecordType::Wayfinder,
        vec!["alpha".into(), "beta".into()],
        "Map",
    );
    let projectless = open(RecordType::Wayfinder, vec![], "Map");
    let single_project_map = open(RecordType::Wayfinder, vec!["alpha".into()], "Map");
    let spec_updates = repository.list_record_updates(spec.id).unwrap();
    repository
        .replace_check_template("alpha", &replacement(1, &["Review", "Tests"]))
        .unwrap();
    assert_eq!(
        repository.list_record_updates(spec.id).unwrap(),
        spec_updates
    );
    for body in ["Implement a feature", "Research an API", "Review work", "Build a prototype"] {
        let execution_spec = open(RecordType::Spec, vec!["alpha".into()], "Execution plan");
        let tasks = [
            (
                repository
                    .open_task_under_wayfinder(
                        "Inferred Project",
                        single_project_map.id,
                        None,
                        vec![],
                    )
                    .unwrap(),
                vec!["Implementation", "Review", "Tests"],
            ),
            (
                open(RecordType::Task, vec!["alpha".into()], body),
                vec!["Implementation", "Review", "Tests"],
            ),
            (
                repository
                    .open_task_under_spec("Spec task", execution_spec.id, vec![])
                    .unwrap(),
                vec!["Implementation", "Review", "Tests"],
            ),
            (
                repository
                    .open_task_under_wayfinder(
                        "Map task",
                        map.id,
                        Some("beta".into()),
                        vec![],
                    )
                    .unwrap(),
                vec!["Implementation", "Docs"],
            ),
            (
                repository
                    .open_task_under_wayfinder("No project", projectless.id, None, vec![])
                    .unwrap(),
                vec!["Implementation"],
            ),
            (
                open(RecordType::Task, vec!["unconfigured".into()], body),
                vec!["Implementation"],
            ),
            (open(RecordType::Task, vec![], body), vec!["Implementation"]),
        ];
        repository.start_record(execution_spec.id).unwrap();
        for (task, names) in tasks {
            let checks = repository.list_record_checks(task.id).unwrap();
            assert_eq!(
                checks.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
                names
            );
            assert!(
                checks
                    .iter()
                    .enumerate()
                    .all(|(i, c)| c.position == i as u64 && c.latest_assessment.is_none())
            );
            super::check_tests::manual(&repository, task.id);
            repository.start_record(task.id).unwrap();
            super::check_tests::assess(&repository, task.id, forge_app::checks::Verdict::Pass);
            if checks.len() > 1 {
                assert_eq!(
                    repository.done_record(task.id, None).unwrap_err().code,
                    "checks-not-passed"
                );
            }
        }
    }
    let filter = RecordListFilter {
        project_slug: None,
        status: None,
        record_type: None,

    };
    let before = repository.list_records(&filter).unwrap();
    let updates = repository.list_record_updates(spec.id).unwrap();
    assert!(
        repository
            .open_task_under_spec("Fail", spec.id, vec![999_999])
            .is_err()
    );
    assert_eq!(repository.list_records(&filter).unwrap(), before);
    assert_eq!(repository.list_record_updates(spec.id).unwrap(), updates);
    let template = repository.show_check_template("alpha", None, 100).unwrap();
    repository.connection.lock().unwrap().execute_batch("CREATE TRIGGER fail_snapshot BEFORE INSERT ON record_checks WHEN NEW.position = 2 BEGIN SELECT RAISE(ABORT, 'injected snapshot failure'); END;").unwrap();
    assert!(
        repository
            .open_task_under_spec("Fail snapshot", spec.id, vec![])
            .is_err()
    );
    assert_eq!(repository.list_records(&filter).unwrap(), before);
    assert_eq!(repository.list_record_updates(spec.id).unwrap(), updates);
    assert_eq!(
        repository.show_check_template("alpha", None, 100).unwrap(),
        template
    );
    repository
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER fail_snapshot")
        .unwrap();
    let task = repository
        .open_task_under_spec("Successful snapshot", spec.id, vec![])
        .unwrap();
    let checks = repository.list_record_checks(task.id).unwrap();
    assert_eq!(checks.len(), 3);
    super::check_tests::manual(&repository, task.id);
    repository.start_record(spec.id).unwrap();
    repository.start_record(task.id).unwrap();
    for check in checks {
        super::check_tests::assess_check(&repository, task.id, check.id, forge_app::checks::Verdict::Pass);
    }
    assert_eq!(
        repository.done_record(task.id, None).unwrap().status,
        RecordStatus::Done
    );
}

#[test]
fn concurrent_template_replacement_and_task_opening_are_coherent_and_restart_durable() {
    use std::sync::{Arc, Barrier};
    let path = std::env::temp_dir().join(format!(
        "forge-templates-{}-{}.sqlite",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let repository = super::SqliteProjectRepository::open(&path).unwrap();
    repository
        .register(&project("alpha", "/template-concurrent"))
        .unwrap();
    repository
        .replace_check_template("alpha", &replacement(0, &["Old A", "Old B"]))
        .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let writers = ["New", "Other"].map(|prefix| {
        let repository = super::SqliteProjectRepository::open(&path).unwrap();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            repository.replace_check_template(
                "alpha",
                &replacement(1, &[&format!("{prefix} A"), &format!("{prefix} B")]),
            )
        })
    });
    barrier.wait();
    let mut tasks = vec![];
    for _ in 0..20 {
        tasks.push(
            repository
                .open_typed_record(&NewTypedRecord {
                    record_type: RecordType::Task,
                    project_slugs: vec!["alpha".into()],
                    body: "Concurrent".into(),
                    generated_by_ids: vec![],
                })
                .unwrap(),
        );
    }
    let results = writers.map(|writer| writer.join().unwrap());
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .find_map(|result| result.as_ref().err())
            .unwrap()
            .code,
        "stale-version"
    );
    let accepted = results.into_iter().find_map(Result::ok).unwrap();
    let template = repository.show_check_template("alpha", None, 100).unwrap();
    assert_eq!(template.version, 2);
    assert_eq!(template.history.len(), 2);
    let snapshots: Vec<_> = tasks
        .iter()
        .map(|task| repository.list_record_checks(task.id).unwrap())
        .collect();
    for checks in &snapshots {
        let extras: Vec<_> = checks[..]
            .iter()
            .map(|c| CheckCriterion {
                name: c.name.clone(),
                description: c.description.clone(),
                profile_revision: c.profile_revision,
            })
            .collect();
        assert!(
            extras == replacement(0, &["Old A", "Old B"]).criteria || extras == accepted.criteria
        );
    }
    drop(repository);
    let repository = super::SqliteProjectRepository::open(&path).unwrap();
    assert_eq!(
        repository.show_check_template("alpha", None, 100).unwrap(),
        template
    );
    for (task, checks) in tasks.iter().zip(snapshots) {
        assert_eq!(repository.list_record_checks(task.id).unwrap(), checks);
    }
    std::fs::remove_file(path).unwrap();
}
