use forge_app::{
    NewRecord, NewTypedRecord, Project, ProjectRepository, RecordListFilter, RecordRepository,
    RecordType, WorkspaceRepository,
};
use forge_store::SqliteProjectRepository;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    store: SqliteProjectRepository,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "forge-workspace-records-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = SqliteProjectRepository::open(&root.join("store.sqlite")).unwrap();
        for workspace in ["one", "two"] {
            let path = root.join(workspace);
            std::fs::create_dir_all(&path).unwrap();
            store.create_workspace(workspace, workspace, &path).unwrap();
            store
                .register(&Project {
                    reference: format!("{workspace}/api"),
                    workspace_id: workspace.into(),
                    slug: "api".into(),
                    project_root: path.canonicalize().unwrap(),
                    origin_url: "https://example.test/api.git".into(),
                })
                .unwrap();
        }
        Self { root, store }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn all() -> RecordListFilter {
    RecordListFilter {
        project_slug: None,
        status: None,
        record_type: None,
    }
}

#[test]
fn projectless_record_ownership_is_explicit_durable_and_blocks_removal() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    let record = NewRecord {
        body: "projectless work".into(),
        project_slugs: vec![],
        generated_by_ids: vec![],
    };
    assert_eq!(
        store.open_record(&record).unwrap_err().code,
        "workspace-selection-required"
    );
    let root = fixture.root.join("empty");
    std::fs::create_dir_all(&root).unwrap();
    store.create_workspace("empty", "Empty", &root).unwrap();
    let selected = store.in_record_workspace("empty").unwrap();
    let opened = selected.open_record(&record).unwrap();
    assert_eq!(selected.record_workspace(opened.id).unwrap(), "empty");
    assert_eq!(
        store.remove_workspace("empty").unwrap_err().code,
        "workspace-not-empty"
    );
    let reopened = SqliteProjectRepository::open(&fixture.root.join("store.sqlite")).unwrap();
    assert_eq!(reopened.record_workspace(opened.id).unwrap(), "empty");
    let wrong = reopened.in_record_workspace("one").unwrap();
    assert_eq!(
        wrong.show_record(opened.id).unwrap_err().code,
        "cross-workspace-record"
    );
    assert!(wrong.list_records(&all()).unwrap().is_empty());
}

#[test]
fn relationships_and_child_creation_are_confined_and_transactional() {
    use forge_app::RecordRelationshipKind;
    let fixture = Fixture::new();
    let store = &fixture.store;
    let one = store.in_record_workspace("one").unwrap();
    let two = store.in_record_workspace("two").unwrap();
    let open = |selected: &SqliteProjectRepository, record_type, projects| {
        selected
            .open_typed_record(&NewTypedRecord {
                record_type,
                body: "work".into(),
                project_slugs: projects,
                generated_by_ids: vec![],
            })
            .unwrap()
    };
    let task_one = open(&one, RecordType::Task, vec![]);
    let task_two = open(&two, RecordType::Task, vec![]);
    for kind in [
        RecordRelationshipKind::DependsOn,
        RecordRelationshipKind::GeneratedBy,
    ] {
        assert_eq!(
            store
                .relate_records(task_one.id, task_two.id, kind)
                .unwrap_err()
                .code,
            "cross-workspace-record"
        );
        assert!(
            store
                .show_record(task_one.id)
                .unwrap()
                .relationships
                .is_empty()
        );
        assert_eq!(store.list_record_updates(task_one.id).unwrap().len(), 1);
    }
    let spec = open(&one, RecordType::Spec, vec!["one/api".into()]);
    let child = one.open_task_under_spec("child", spec.id, vec![]).unwrap();
    assert_eq!(store.record_workspace(child.id).unwrap(), "one");
    assert_eq!(
        two.open_task_under_spec("wrong parent", spec.id, vec![])
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    let count = store.list_records(&all()).unwrap().len();
    let updates = store.list_record_updates(spec.id).unwrap();
    assert_eq!(
        one.open_task_under_spec("bad provenance", spec.id, vec![task_two.id])
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    assert_eq!(store.list_records(&all()).unwrap().len(), count);
    assert_eq!(store.list_record_updates(spec.id).unwrap(), updates);
    assert_eq!(
        one.open_record(&NewRecord {
            body: "wrong project".into(),
            project_slugs: vec!["two/api".into()],
            generated_by_ids: vec![]
        })
        .unwrap_err()
        .code,
        "cross-workspace-record"
    );
}

#[test]
fn a_selected_handle_cannot_read_or_mutate_another_workspaces_records() {
    use forge_app::{RecordCommentId, RecordRelationshipKind};
    let fixture = Fixture::new();
    let one = fixture.store.in_record_workspace("one").unwrap();
    let two = fixture.store.in_record_workspace("two").unwrap();
    let record = one
        .open_record(&NewRecord {
            body: "private boundary".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    let comment = one.add_record_comment(record.id, "kept", None).unwrap();
    let assert_rejected =
        |error: forge_app::RegistrationError| assert_eq!(error.code, "cross-workspace-record");
    assert_rejected(two.edit_record(record.id, "bad edit").unwrap_err());
    assert_rejected(two.start_record(record.id).unwrap_err());
    assert_rejected(two.done_record(record.id, None).unwrap_err());
    assert_rejected(
        two.add_record_comment(record.id, "bad comment", None)
            .unwrap_err(),
    );
    assert_rejected(two.list_record_comments(record.id).unwrap_err());
    assert_rejected(two.list_record_updates(record.id).unwrap_err());
    assert_rejected(
        two.delete_record_comment(record.id, RecordCommentId(comment.id.0))
            .unwrap_err(),
    );
    let local = one
        .open_record(&NewRecord {
            body: "local".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    one.relate_records(local.id, record.id, RecordRelationshipKind::GeneratedBy)
        .unwrap();
    assert_rejected(
        two.unrelate_records(local.id, record.id, RecordRelationshipKind::GeneratedBy)
            .unwrap_err(),
    );
    assert_eq!(one.show_record(record.id).unwrap().body, "private boundary");
    assert_eq!(one.list_record_comments(record.id).unwrap(), [comment]);
}

#[test]
fn every_existing_record_kind_inherits_one_workspace_and_keeps_local_relationships() {
    use forge_app::RecordRelationshipKind;
    let fixture = Fixture::new();
    let one = fixture.store.in_record_workspace("one").unwrap();
    let two = fixture.store.in_record_workspace("two").unwrap();
    let wayfinder = one
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Wayfinder,
            body: "map".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    let task = one
        .open_task_under_wayfinder("task", wayfinder.id, None, vec![])
        .unwrap();
    let grilling = one
        .open_grilling_under_wayfinder("decision", wayfinder.id, None, vec![])
        .unwrap();
    for record in [&wayfinder, &task, &grilling] {
        assert_eq!(fixture.store.record_workspace(record.id).unwrap(), "one");
        assert_eq!(record.project_slugs, ["one/api"]);
        assert_eq!(
            two.show_record(record.id).unwrap_err().code,
            "cross-workspace-record"
        );
    }
    assert_eq!(
        two.open_task_under_wayfinder("wrong", wayfinder.id, None, vec![])
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    assert_eq!(
        two.open_grilling_under_wayfinder("wrong", wayfinder.id, None, vec![])
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    one.relate_records(task.id, grilling.id, RecordRelationshipKind::DependsOn)
        .unwrap();
    assert_eq!(
        one.start_record(task.id).unwrap_err().code,
        "dependencies-not-done"
    );
    one.start_record(grilling.id).unwrap();
    one.done_record(grilling.id, Some("decided")).unwrap();
    one.start_record(task.id).unwrap();
    for check in one.list_record_checks(task.id).unwrap() {
        one.assess_record_check(
            task.id,
            check.id,
            &forge_app::checks::AssessmentInput {
                expected_version: check.version,
                verdict: forge_app::checks::Verdict::Pass,
                actor: "test operator".into(),
                message: "Fixture deliverables verified".into(),
                revision: None,
                dirty: None,
            },
        )
        .unwrap();
    }
    one.done_record(task.id, Some("implemented")).unwrap();
    assert_eq!(one.list_records(&all()).unwrap().len(), 3);
    assert!(two.list_records(&all()).unwrap().is_empty());
}

#[test]
fn initiatives_and_projectless_maps_are_owned_and_relationships_do_not_cross_workspaces() {
    use forge_app::RecordRelationshipKind;
    let fixture = Fixture::new();
    let one = fixture.store.in_record_workspace("one").unwrap();
    let two = fixture.store.in_record_workspace("two").unwrap();
    let open = |store: &SqliteProjectRepository, kind| {
        store
            .open_typed_record(&NewTypedRecord {
                record_type: kind,
                body: "plan".into(),
                project_slugs: vec![],
                generated_by_ids: vec![],
            })
            .unwrap()
    };
    let initiative = open(&one, RecordType::Initiative);
    let map = open(&one, RecordType::Wayfinder);
    let foreign_initiative = open(&two, RecordType::Initiative);
    let spec = one
        .open_typed_record(&NewTypedRecord {
            record_type: RecordType::Spec,
            body: "scoped work".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    one.relate_records(spec.id, initiative.id, RecordRelationshipKind::Initiative)
        .unwrap();
    let alternative = open(&one, RecordType::Initiative);
    let updates = one.list_record_updates(spec.id).unwrap();
    assert_eq!(
        one.relate_records(spec.id, alternative.id, RecordRelationshipKind::Initiative)
            .unwrap_err()
            .code,
        "initiative-already-attached"
    );
    assert_eq!(one.list_record_updates(spec.id).unwrap(), updates);
    one.relate_records(map.id, initiative.id, RecordRelationshipKind::Initiative)
        .unwrap();
    assert_eq!(
        fixture
            .store
            .relate_records(
                map.id,
                foreign_initiative.id,
                RecordRelationshipKind::Initiative
            )
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    assert_eq!(one.show_record(map.id).unwrap().relationships.len(), 1);
    one.unrelate_records(map.id, initiative.id, RecordRelationshipKind::Initiative)
        .unwrap();
    let task = one
        .open_task_under_wayfinder("explore", map.id, None, vec![])
        .unwrap();
    let grilling = one
        .open_grilling_under_wayfinder("decide", map.id, None, vec![])
        .unwrap();
    assert!(task.project_slugs.is_empty());
    assert!(grilling.project_slugs.is_empty());
    assert_eq!(task.workspace_id, "one");
    assert_eq!(grilling.workspace_id, "one");
    one.relate_records(task.id, grilling.id, RecordRelationshipKind::DependsOn)
        .unwrap();
    assert_eq!(
        two.open_grilling_under_wayfinder("wrong", map.id, None, vec![])
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    let reopened = SqliteProjectRepository::open(&fixture.root.join("store.sqlite")).unwrap();
    assert_eq!(
        reopened.show_record(initiative.id).unwrap().record_type,
        RecordType::Initiative
    );
    assert_eq!(
        reopened.show_record(initiative.id).unwrap().workspace_id,
        "one"
    );
}

#[test]
fn readiness_and_planning_remain_local_without_dispatch_configuration() {
    use forge_app::{Application, RecordRelationshipKind, RecordType};
    let fixture = Fixture::new();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&fixture.root.join("store.sqlite")).unwrap()),
        Box::new(NoRepositoryInspector),
    );
    let one = app.records_in_workspace("one").unwrap();
    let two = app.records_in_workspace("two").unwrap();
    let map = one.open_wayfinder("Projectless planning", vec![]).unwrap();
    let task = one
        .open_task_under_wayfinder("blocked", map.id, None, vec![])
        .unwrap();
    let decision = one
        .open_grilling_under_wayfinder("decision", map.id, None, vec![])
        .unwrap();
    let other = two.open_task("other Workspace", None).unwrap();
    one.relate_records(task.id, decision.id, RecordRelationshipKind::DependsOn)
        .unwrap();
    assert!(one.ready_records(false, false, false).unwrap().is_empty());
    let blocked = one.ready_records(false, false, true).unwrap();
    assert_eq!(blocked[0].record.id, task.id);
    assert_eq!(blocked[0].blocked_by, [decision.id]);
    let planning = one.ready_records(true, true, false).unwrap();
    assert_eq!(
        planning
            .iter()
            .map(|item| item.record.record_type)
            .collect::<Vec<_>>(),
        [RecordType::Grilling, RecordType::Wayfinder]
    );
    assert!(!planning.iter().any(|item| item.record.id == other.id));
    one.start_record(decision.id).unwrap();
    one.done_record(decision.id, Some("decided")).unwrap();
    assert_eq!(
        one.ready_records(false, false, false).unwrap()[0].record.id,
        task.id
    );
    assert_eq!(
        two.ready_records(false, false, false).unwrap()[0].record.id,
        other.id
    );
}

#[test]
fn owned_development_store_is_rejected_without_conversion() {
    let fixture = Fixture::new();
    let database = fixture.root.join("owned-development.sqlite");
    let connection = rusqlite::Connection::open(&database).unwrap();
    connection.execute_batch("CREATE TABLE workspaces(id TEXT PRIMARY KEY, name TEXT NOT NULL, root TEXT UNIQUE NOT NULL);
        INSERT INTO workspaces VALUES ('one', 'One', '/');
        CREATE TABLE projects(slug TEXT PRIMARY KEY, registered_folder TEXT UNIQUE NOT NULL, origin_url TEXT NOT NULL, workspace_id TEXT NOT NULL REFERENCES workspaces(id), local_slug TEXT NOT NULL);
        INSERT INTO projects VALUES ('one/api', '/api', 'https://example.test/api', 'one', 'api');
        CREATE TABLE records(id INTEGER PRIMARY KEY, record_type TEXT NOT NULL CHECK(record_type IN ('generic', 'spec', 'task', 'wayfinder', 'grilling')), task_kind TEXT, workspace_id TEXT REFERENCES workspaces(id), body TEXT NOT NULL, status TEXT NOT NULL, completion_resolution TEXT);
        INSERT INTO records VALUES (1, 'task', 'review', 'one', 'preserved task', 'open', NULL);
        CREATE TABLE record_projects(record_id INTEGER REFERENCES records(id), project_slug TEXT REFERENCES projects(slug), PRIMARY KEY(record_id, project_slug));
        INSERT INTO record_projects VALUES (1, 'one/api');
        CREATE TABLE record_relationships(from_id INTEGER REFERENCES records(id), to_id INTEGER REFERENCES records(id), parent_id INTEGER REFERENCES records(id), child_id INTEGER UNIQUE REFERENCES records(id), kind TEXT CHECK(kind IN ('parent-child', 'depends-on', 'generated-by')), PRIMARY KEY(from_id,to_id,kind));
        CREATE TRIGGER record_project_workspace BEFORE INSERT ON record_projects WHEN (SELECT workspace_id FROM records WHERE id=NEW.record_id) != (SELECT workspace_id FROM projects WHERE slug=NEW.project_slug) BEGIN SELECT RAISE(ABORT, 'cross Workspace'); END;
        CREATE TRIGGER record_relationship_workspace BEFORE INSERT ON record_relationships WHEN (SELECT workspace_id FROM records WHERE id=NEW.from_id) != (SELECT workspace_id FROM records WHERE id=NEW.to_id) BEGIN SELECT RAISE(ABORT, 'cross Workspace'); END;").unwrap();
    drop(connection);
    let before = std::fs::read(&database).unwrap();
    let error = SqliteProjectRepository::open(&database).err().unwrap();
    assert_eq!(error.code, "fresh-store-required");
    assert_eq!(std::fs::read(&database).unwrap(), before);
}

#[test]
fn every_record_kind_rejects_foreign_scope_and_provenance_without_partial_writes() {
    use forge_app::RecordRelationshipKind;
    let fixture = Fixture::new();
    let one = fixture.store.in_record_workspace("one").unwrap();
    let two = fixture.store.in_record_workspace("two").unwrap();
    let foreign = two
        .open_record(&NewRecord {
            body: "foreign generator".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    for record_type in [
        RecordType::Spec,
        RecordType::Task,
        RecordType::Wayfinder,
        RecordType::Grilling,
        RecordType::Initiative,
    ] {
        let valid = NewTypedRecord {
            record_type,
            body: "local".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        };
        let opened = one.open_typed_record(&valid).unwrap();
        assert_eq!(opened.workspace_id, "one");
        let before = one.list_records(&all()).unwrap();
        let mut invalid = valid.clone();
        invalid.project_slugs = vec!["two/api".into()];
        assert_eq!(
            one.open_typed_record(&invalid).unwrap_err().code,
            "cross-workspace-record"
        );
        invalid = valid;
        invalid.generated_by_ids = vec![foreign.id];
        assert_eq!(
            one.open_typed_record(&invalid).unwrap_err().code,
            "cross-workspace-record"
        );
        assert_eq!(one.list_records(&all()).unwrap(), before);
        assert_eq!(
            fixture.store.list_record_updates(foreign.id).unwrap().len(),
            1
        );
    }
    let local = one
        .open_record(&NewRecord {
            body: "local parent".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    let child = one
        .open_record(&NewRecord {
            body: "local child".into(),
            project_slugs: vec![],
            generated_by_ids: vec![],
        })
        .unwrap();
    assert_eq!(
        fixture
            .store
            .relate_records(child.id, foreign.id, RecordRelationshipKind::ParentChild)
            .unwrap_err()
            .code,
        "cross-workspace-record"
    );
    one.relate_records(child.id, local.id, RecordRelationshipKind::ParentChild)
        .unwrap();
    assert_eq!(one.show_record(child.id).unwrap().relationships.len(), 1);
}

struct NoRepositoryInspector;
impl forge_app::RepositoryInspector for NoRepositoryInspector {
    fn inspect(&self, _: &std::path::Path) -> Result<Project, forge_app::RegistrationError> {
        Err(forge_app::RegistrationError::new(
            "no-repository",
            "test has no repository",
        ))
    }
}

#[test]
fn application_selection_uses_the_store_contract_without_changing_other_requests() {
    use forge_app::Application;
    let fixture = Fixture::new();
    let app = Application::new(
        Box::new(SqliteProjectRepository::open(&fixture.root.join("store.sqlite")).unwrap()),
        Box::new(NoRepositoryInspector),
    );
    let one = app.records_in_workspace("one").unwrap();
    let two = app.records_in_workspace("two").unwrap();
    let opened = one.open_task("Projectless task", None).unwrap();
    assert_eq!(
        two.show_record(opened.id).unwrap_err().code,
        "cross-workspace-record"
    );
    assert!(two.list_records(None, None, None).unwrap().is_empty());
    assert_eq!(
        app.open_task("unselected", None).unwrap_err().code,
        "workspace-selection-required"
    );
    assert_eq!(one.show_record(opened.id).unwrap().body, "Projectless task");
}

#[test]
fn records_cannot_open_with_projects_from_different_workspaces() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    for record_type in [RecordType::Generic, RecordType::Wayfinder] {
        let error = if record_type == RecordType::Generic {
            store.open_record(&NewRecord {
                body: "mixed scope".into(),
                project_slugs: vec!["one/api".into(), "two/api".into()],
                generated_by_ids: vec![],
            })
        } else {
            store.open_typed_record(&NewTypedRecord {
                record_type,
                body: "mixed scope".into(),
                project_slugs: vec!["one/api".into(), "two/api".into()],
                generated_by_ids: vec![],
            })
        }
        .unwrap_err();
        assert_eq!(error.code, "cross-workspace-record");
        assert!(store.list_records(&all()).unwrap().is_empty());
    }
    let valid = store
        .open_record(&NewRecord {
            body: "local scope".into(),
            project_slugs: vec!["one/api".into()],
            generated_by_ids: vec![],
        })
        .unwrap();
    assert_eq!(valid.project_slugs, ["one/api"]);
}
