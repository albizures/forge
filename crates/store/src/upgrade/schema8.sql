-- Frozen schema 8 DDL. No user data or seed values.
CREATE TABLE agent_assignment_resources (
                assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
                resource_kind TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                retired_at_millis INTEGER,
                PRIMARY KEY (assignment_id, resource_kind, resource_id)
            );
CREATE TABLE agent_assignments (
                id TEXT PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                project_slug TEXT NOT NULL REFERENCES projects(slug),
                profile_record_type TEXT NOT NULL,
                profile_revision INTEGER NOT NULL,
                attempt_number INTEGER NOT NULL,
                claimed_at_millis INTEGER NOT NULL,
                state TEXT NOT NULL DEFAULT 'provisioning' CHECK (state IN ('provisioning', 'working', 'idle', 'blocked', 'unknown')),
                state_changed_at_millis INTEGER NOT NULL DEFAULT 0,
                next_retry_at_millis INTEGER NOT NULL DEFAULT 0,
                retry_count INTEGER NOT NULL DEFAULT 0,
                closed_at_millis INTEGER,
                closure_reason TEXT CHECK (closure_reason IN ('lost', 'crashed', 'retired', 'replaced')), launch_started_at_millis INTEGER, acknowledgement_deadline_millis INTEGER, safe_dispatch_generation INTEGER NOT NULL DEFAULT 0, acknowledged_at_millis INTEGER, workspace_id TEXT REFERENCES workspaces(id) ON DELETE RESTRICT, dispatch_workspace_id TEXT,
                UNIQUE (record_id, attempt_number)
            );
CREATE TABLE agent_dispatch_configuration (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                herdr_socket_path TEXT,
                dispatch_workspace_id TEXT,
                dispatch_workspace_root TEXT,
                scan_lease_owner TEXT,
                scan_lease_until_millis INTEGER
            );
CREATE TABLE agent_profiles (
                record_type TEXT NOT NULL CHECK (record_type IN ('spec', 'task', 'grilling')),
                revision INTEGER NOT NULL CHECK (revision > 0),
                provider TEXT NOT NULL,
                agent_kind TEXT NOT NULL,
                model TEXT NOT NULL,
                prompt_template TEXT NOT NULL,
                retry_limit INTEGER NOT NULL,
                retry_backoff_millis INTEGER NOT NULL,
                startup_timeout_millis INTEGER NOT NULL,
                unknown_state_grace_millis INTEGER NOT NULL, acknowledgement_timeout_millis INTEGER NOT NULL DEFAULT 300000, acknowledgement_timeout_overridden INTEGER NOT NULL DEFAULT 0, launch_policy_json TEXT,
                PRIMARY KEY (record_type, revision)
            );
CREATE TABLE assignment_authorization_audits (
                id INTEGER PRIMARY KEY NOT NULL,
                assignment_id TEXT NOT NULL REFERENCES agent_assignments(id),
                capability_id TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                operation TEXT NOT NULL CHECK (operation IN ('read-context', 'add-comment', 'complete-task', 'start-task', 'acknowledge')),
                target_record_id INTEGER NOT NULL REFERENCES records(id),
                outcome TEXT NOT NULL CHECK (outcome IN ('allowed', 'denied')),
                reason TEXT NOT NULL,
                correlation_id TEXT NOT NULL
            );
CREATE TABLE assignment_check_attempts (
            assignment_id TEXT PRIMARY KEY REFERENCES agent_assignments(id),
            attempt_id TEXT NOT NULL UNIQUE REFERENCES record_attempts(id));
CREATE TABLE assignment_context_bootstrap (
                assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id)
            );
CREATE TABLE assignment_execution_bindings (
                assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id),
                directory TEXT NOT NULL
            );
CREATE TABLE assignment_unknown_resource_resolutions (
                assignment_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_assignments(id),
                confirmed_at_millis INTEGER NOT NULL,
                authority TEXT NOT NULL,
                confirmation TEXT NOT NULL
            );
CREATE TABLE check_activities (
        id INTEGER PRIMARY KEY NOT NULL, check_id INTEGER NOT NULL REFERENCES record_checks(id),
        assessment_id INTEGER NOT NULL REFERENCES check_assessments(id),
        invalidation_id INTEGER REFERENCES check_invalidations(id));
CREATE TABLE check_assessments (
        id INTEGER PRIMARY KEY NOT NULL, check_id INTEGER NOT NULL REFERENCES record_checks(id),
        verdict TEXT NOT NULL CHECK(verdict IN ('pass','fail')), actor TEXT NOT NULL,
        message TEXT NOT NULL, created_at_millis INTEGER NOT NULL, revision TEXT, dirty INTEGER, assignment_id TEXT,
        attempt_id TEXT NOT NULL UNIQUE REFERENCES record_attempts(id), generation INTEGER NOT NULL CHECK(generation > 0), evidence_json TEXT NOT NULL);
CREATE TABLE check_invalidations (
        id INTEGER PRIMARY KEY NOT NULL, assessment_id INTEGER NOT NULL UNIQUE REFERENCES check_assessments(id),
        actor TEXT NOT NULL, message TEXT NOT NULL, created_at_millis INTEGER NOT NULL, revision TEXT, dirty INTEGER, assignment_id TEXT);
CREATE TABLE dispatch_holds (
                record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
                reason TEXT NOT NULL CHECK (length(trim(reason)) > 0)
            );
CREATE TABLE dispatch_provider_generation (
            singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
            generation INTEGER NOT NULL CHECK(generation >= 0)
         );
CREATE TABLE finding_activities (
        id INTEGER PRIMARY KEY NOT NULL, finding_id INTEGER NOT NULL REFERENCES record_findings(id),
        action TEXT NOT NULL CHECK(action IN ('raise','resolve','respond','dismiss','reopen','amend','note','assign-verification')), actor TEXT NOT NULL, assignment_id TEXT,
        response TEXT CHECK(response IN ('fixed','rejected')), amendment TEXT,
        message TEXT NOT NULL, revision TEXT, dirty INTEGER, created_at_millis INTEGER NOT NULL);
CREATE TABLE project_agent_dispatch_configuration (
                project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
                task_concurrency_limit INTEGER NOT NULL CHECK (task_concurrency_limit BETWEEN 1 AND 1024)
            );
CREATE TABLE project_check_template_changes (
        id INTEGER PRIMARY KEY NOT NULL, project_slug TEXT NOT NULL REFERENCES projects(slug),
        version INTEGER NOT NULL, criteria_json TEXT NOT NULL, actor TEXT NOT NULL,
        message TEXT NOT NULL, created_at_millis INTEGER NOT NULL, UNIQUE(project_slug, version));
CREATE TABLE project_check_templates (
        project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
        version INTEGER NOT NULL, criteria_json TEXT NOT NULL);
CREATE TABLE project_managed_worktree_configuration (
                project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
                setup_hook TEXT NOT NULL
            );
CREATE TABLE projects (
                slug TEXT PRIMARY KEY NOT NULL,
                registered_folder TEXT UNIQUE NOT NULL,
                origin_url TEXT NOT NULL,
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE RESTRICT,
                local_slug TEXT NOT NULL
            );
CREATE TABLE record_attempts (
        id TEXT PRIMARY KEY, record_id INTEGER NOT NULL REFERENCES records(id), check_id INTEGER NOT NULL REFERENCES record_checks(id),
        attempt_json TEXT NOT NULL, closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN (0,1)), outcome_json TEXT, result_json TEXT,
        CHECK((outcome_json IS NULL) = (result_json IS NULL)), CHECK(outcome_json IS NULL OR closed = 1));
CREATE TABLE record_checks (
        id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id),
        name TEXT NOT NULL, description TEXT NOT NULL, position INTEGER NOT NULL,
        version INTEGER NOT NULL DEFAULT 1, profile_revision INTEGER CHECK(profile_revision > 0 AND profile_revision <= 4294967295),
        UNIQUE(record_id, position), UNIQUE(record_id, name));
CREATE TABLE record_comments (
                id INTEGER PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                created_at_millis INTEGER NOT NULL,
                author TEXT,
                body TEXT NOT NULL
            );
CREATE TABLE record_findings (
        id INTEGER PRIMARY KEY NOT NULL, record_id INTEGER NOT NULL REFERENCES records(id),
        severity TEXT NOT NULL CHECK(severity IN ('minor','major','critical')), blocking INTEGER NOT NULL,
        check_id INTEGER REFERENCES record_checks(id), assessment_id INTEGER REFERENCES check_assessments(id),
        verification_check_id INTEGER REFERENCES record_checks(id));
CREATE TABLE record_projects (
                record_id INTEGER NOT NULL REFERENCES records(id),
                project_slug TEXT NOT NULL REFERENCES projects(slug),
                PRIMARY KEY (record_id, project_slug)
            );
CREATE TABLE record_relationships (
                from_id INTEGER NOT NULL REFERENCES records(id),
                to_id INTEGER NOT NULL REFERENCES records(id),
                parent_id INTEGER REFERENCES records(id),
                child_id INTEGER REFERENCES records(id) UNIQUE,
                kind TEXT NOT NULL CHECK (kind IN ('parent-child', 'depends-on', 'generated-by', 'initiative')),
                PRIMARY KEY (from_id, to_id, kind)
            );
CREATE TABLE record_updates (
                id INTEGER PRIMARY KEY NOT NULL,
                record_id INTEGER NOT NULL REFERENCES records(id),
                created_at_millis INTEGER NOT NULL,
                kind TEXT NOT NULL CHECK (kind IN ('created', 'body-edited', 'status-transition', 'relationship-added', 'relationship-removed', 'comment-added', 'comment-deleted', 'check-assessed', 'check-invalidated', 'finding-activity', 'execution-mode-changed')),
                from_mode TEXT,
                to_mode TEXT,
                mode_actor TEXT,
                mode_message TEXT,
                assessment_id INTEGER,
                finding_activity_id INTEGER,
                from_status TEXT,
                to_status TEXT,
                related_record_id INTEGER REFERENCES records(id),
                comment_id INTEGER,
                relationship_kind TEXT CHECK (relationship_kind IN ('parent-child', 'depends-on', 'generated-by', 'initiative'))
            );
CREATE TABLE record_work (
        record_id INTEGER PRIMARY KEY REFERENCES records(id), generation INTEGER NOT NULL DEFAULT 1 CHECK(generation > 0),
        input_json TEXT, output_json TEXT);
CREATE TABLE record_work_invalidations (
        id INTEGER PRIMARY KEY, record_id INTEGER NOT NULL REFERENCES records(id), generation INTEGER NOT NULL,
        actor TEXT NOT NULL, message TEXT NOT NULL, created_at_millis INTEGER NOT NULL);
CREATE TABLE records (
                id INTEGER PRIMARY KEY NOT NULL,
                record_type TEXT NOT NULL CHECK (record_type IN ('generic', 'spec', 'task', 'wayfinder', 'grilling', 'initiative')),
                workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE RESTRICT,
                body TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('open', 'in_progress', 'done')),
                completion_resolution TEXT,
                execution_mode TEXT NOT NULL DEFAULT 'manual' CHECK(execution_mode IN ('manual', 'dispatched'))
            );
CREATE TABLE task_base_commits (
                record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
                base_commit TEXT NOT NULL CHECK (length(trim(base_commit)) > 0)
            );
CREATE TABLE task_progress (
        record_id INTEGER PRIMARY KEY REFERENCES records(id), cycles INTEGER NOT NULL DEFAULT 0,
        baseline TEXT, verified INTEGER NOT NULL DEFAULT 0, baseline_verified INTEGER NOT NULL DEFAULT 0,
        stop_reason TEXT);
CREATE TABLE task_progress_resets (
        id INTEGER PRIMARY KEY, record_id INTEGER NOT NULL REFERENCES records(id),
        actor TEXT NOT NULL, message TEXT NOT NULL, created_at_millis INTEGER NOT NULL);
CREATE TABLE task_retry_counts (
        check_id INTEGER PRIMARY KEY REFERENCES record_checks(id), interruptions INTEGER NOT NULL DEFAULT 0);
CREATE TABLE task_stacks (
        record_id INTEGER PRIMARY KEY REFERENCES records(id),
        predecessor INTEGER UNIQUE REFERENCES task_stacks(record_id),
        source_json TEXT NOT NULL,
        base_commit TEXT,
        publication_json TEXT,
        CHECK(predecessor IS NULL OR predecessor != record_id)
    );
CREATE TABLE workspace_dispatch_configuration (
            workspace_id TEXT PRIMARY KEY NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            task_profile_type TEXT NOT NULL DEFAULT 'task' CHECK(task_profile_type = 'task'),
            task_profile_revision INTEGER CHECK(task_profile_revision > 0),
            grilling_profile_type TEXT NOT NULL DEFAULT 'grilling' CHECK(grilling_profile_type = 'grilling'),
            grilling_profile_revision INTEGER CHECK(grilling_profile_revision > 0),
            herdr_workspace_id TEXT UNIQUE CHECK(herdr_workspace_id IS NULL OR length(trim(herdr_workspace_id)) > 0),
            verified_generation INTEGER CHECK(verified_generation >= 0),
            FOREIGN KEY(task_profile_type, task_profile_revision) REFERENCES agent_profiles(record_type, revision),
            FOREIGN KEY(grilling_profile_type, grilling_profile_revision) REFERENCES agent_profiles(record_type, revision)
         );
CREATE TABLE workspace_profile_preflight_blockers (
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
            record_type TEXT NOT NULL CHECK(record_type IN ('task', 'grilling')),
            profile_revision INTEGER NOT NULL CHECK(profile_revision > 0),
            PRIMARY KEY(workspace_id, record_type)
         );
CREATE TABLE workspaces (
        id TEXT PRIMARY KEY NOT NULL,
        name TEXT NOT NULL CHECK(length(trim(name)) > 0),
        root TEXT UNIQUE NOT NULL
    );
CREATE TABLE worktree_claims (
                record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
                project_slug TEXT NOT NULL REFERENCES projects(slug),
                path TEXT NOT NULL UNIQUE,
                state TEXT NOT NULL CHECK (state IN ('reserving', 'active', 'cleanup-required'))
            );
CREATE INDEX activities_by_check ON check_activities(check_id, id);
CREATE INDEX activities_by_finding ON finding_activities(finding_id, id);
CREATE INDEX assessments_by_check ON check_assessments(check_id, id);
CREATE TRIGGER assignment_workspace_binding
         AFTER INSERT ON agent_assignments
         BEGIN UPDATE agent_assignments SET workspace_id = (SELECT workspace_id FROM records WHERE id = NEW.record_id),
           dispatch_workspace_id = (SELECT herdr_workspace_id FROM workspace_dispatch_configuration WHERE workspace_id = (SELECT workspace_id FROM records WHERE id = NEW.record_id))
           WHERE id = NEW.id; END;
CREATE TRIGGER assignment_workspace_binding_immutable
         BEFORE UPDATE OF workspace_id, dispatch_workspace_id ON agent_assignments
         WHEN OLD.workspace_id IS NOT NULL
         BEGIN SELECT RAISE(ABORT, 'Assignment Workspace and provider binding are immutable'); END;
CREATE TRIGGER dispatch_socket_insert_generation
         AFTER INSERT ON agent_dispatch_configuration WHEN NEW.herdr_socket_path IS NOT NULL
         BEGIN UPDATE dispatch_provider_generation SET generation = generation + 1 WHERE singleton = 1; END;
CREATE TRIGGER dispatch_socket_update_generation
         AFTER UPDATE OF herdr_socket_path ON agent_dispatch_configuration
         WHEN OLD.herdr_socket_path IS NOT NEW.herdr_socket_path
         BEGIN UPDATE dispatch_provider_generation SET generation = generation + 1 WHERE singleton = 1; END;
CREATE INDEX findings_by_record ON record_findings(record_id, id);
CREATE TRIGGER immutable_assignment_check_attempt
        BEFORE UPDATE ON assignment_check_attempts
        BEGIN SELECT RAISE(ABORT, 'Assignment Check binding is immutable'); END;
CREATE UNIQUE INDEX one_open_agent_assignment_per_record
                ON agent_assignments(record_id) WHERE closed_at_millis IS NULL;
CREATE UNIQUE INDEX one_open_record_attempt ON record_attempts(record_id) WHERE closed = 0;
CREATE TRIGGER project_identity_immutable_v2
         BEFORE UPDATE OF slug, registered_folder, workspace_id, local_slug ON projects
         BEGIN SELECT RAISE(ABORT, 'Project identity, root and Workspace are immutable'); END;
CREATE TRIGGER project_ownership_required
         BEFORE INSERT ON projects
         WHEN NEW.workspace_id IS NULL OR NEW.local_slug IS NULL OR length(trim(NEW.local_slug)) = 0
         BEGIN SELECT RAISE(ABORT, 'Project requires Workspace ownership and a local slug'); END;
CREATE UNIQUE INDEX record_initiative_membership ON record_relationships(from_id) WHERE kind = 'initiative';
CREATE TRIGGER record_ownership_required
         BEFORE INSERT ON records WHEN NEW.workspace_id IS NULL
         BEGIN SELECT RAISE(ABORT, 'Record requires Workspace ownership'); END;
CREATE TRIGGER record_project_scope_immutable
         BEFORE UPDATE ON record_projects
         BEGIN SELECT RAISE(ABORT, 'Record Project scope is immutable'); END;
CREATE TRIGGER record_project_workspace
         BEFORE INSERT ON record_projects
         WHEN (SELECT workspace_id FROM records WHERE id = NEW.record_id) !=
              (SELECT workspace_id FROM projects WHERE slug = NEW.project_slug)
         BEGIN SELECT RAISE(ABORT, 'cross-Workspace Record Project scope'); END;
CREATE TRIGGER record_relationship_endpoints_immutable
         BEFORE UPDATE ON record_relationships
         BEGIN SELECT RAISE(ABORT, 'Record relationship endpoints are immutable'); END;
CREATE TRIGGER record_relationship_workspace
         BEFORE INSERT ON record_relationships
         WHEN (SELECT workspace_id FROM records WHERE id = NEW.from_id) !=
              (SELECT workspace_id FROM records WHERE id = NEW.to_id)
         BEGIN SELECT RAISE(ABORT, 'cross-Workspace Record relationship'); END;
CREATE TRIGGER record_workspace_immutable
         BEFORE UPDATE OF workspace_id ON records
         BEGIN SELECT RAISE(ABORT, 'Record Workspace is immutable'); END;
CREATE INDEX template_changes_by_project ON project_check_template_changes(project_slug, id);
CREATE INDEX work_invalidations_by_record ON record_work_invalidations(record_id, id);
CREATE TRIGGER workspace_identity_immutable
    BEFORE UPDATE OF id, root ON workspaces
    BEGIN SELECT RAISE(ABORT, 'Workspace ID and root are immutable'); END;
CREATE UNIQUE INDEX workspace_project_slugs ON projects(workspace_id, local_slug);
CREATE INDEX workspace_records ON records(workspace_id, id);
CREATE INDEX worktree_claims_requiring_cleanup
                ON worktree_claims(state) WHERE state = 'cleanup-required';
