//! A revision-19 store that still carries Continuation rows upgrades through the real Forgeup
//! plan, prepare and activate commands, run by the real `forgeup` and `forge-server` binaries.
//! Only the *source* Revision is a stub: its executables are never run, they only identify it.
use forge_app::{NewTypedRecord, RecordRepository, RecordType, WorkspaceRepository};
use forge_store::SqliteProjectRepository;
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

const OLD_COMMIT: &str = "0123456789012345678901234567890123456789";
const EVIDENCE: &str = r#"{"kind":"repository","repository":"repo","commit":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
const HANDOFF: &str = r#"{"stopping_reason":"budget","completed_work":"partial","verification":"unrun","remaining_work":"finish"}"#;

/// The current `forge-server` built next to `forgeup`. A test run only builds this package's
/// binaries, so always ask Cargo (a no-op when fresh) rather than trust a stale sibling.
fn forge_server() -> PathBuf {
    let forgeup = PathBuf::from(env!("CARGO_BIN_EXE_forgeup"));
    let sibling = forgeup.with_file_name("forge-server");
    {
        let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args(["build", "-q", "-p", "forge-server"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .status()
            .unwrap();
        assert!(
            status.success() && sibling.exists(),
            "cannot build forge-server"
        );
    }
    sibling
}

fn forgeup(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_forgeup"))
        .args(args)
        .env("FORGEUP_HOME", home)
        .output()
        .unwrap()
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// A current store walked back to revision 19: no Nudge tables, Continuation rows present.
fn revision_nineteen_store(path: &Path, workspace_root: &Path) {
    let repo = SqliteProjectRepository::open(path).unwrap();
    repo.create_workspace("personal", "Personal", workspace_root)
        .unwrap();
    let repo = repo.in_record_workspace("personal").unwrap();
    repo.open_typed_record(&NewTypedRecord {
        record_type: RecordType::Task,
        body: "Keep my old Task".into(),
        project_slugs: vec![],
        generated_by_ids: vec![],
    })
    .unwrap();
    drop(repo);
    let c = Connection::open(path).unwrap();
    c.execute_batch("DROP TABLE assignment_prompt_intents; DROP TABLE assignment_nudges;")
        .unwrap();
    for sql in [
        include_str!("../../store/src/upgrade/continuations.sql"),
        include_str!("../../store/src/upgrade/resettable_continuations.sql"),
        include_str!("../../store/src/upgrade/continuation_resets.sql"),
    ] {
        c.execute_batch(sql).unwrap();
    }
    let check: i64 = c
        .query_row(
            "SELECT id FROM record_checks WHERE record_id=1 AND position=0",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let checkpoint =
        format!(r#"{{"attempt_id":"retained","evidence":{EVIDENCE},"handoff":{HANDOFF}}}"#);
    c.execute(
        "INSERT INTO check_continuations(check_id,total,consecutive_unchanged,checkpoint_json) VALUES (?1,20,1,?2)",
        rusqlite::params![check, checkpoint],
    )
    .unwrap();
    c.execute(
        "INSERT INTO check_continuation_resets(check_id,actor,message,created_at_millis) VALUES (?1,'operator','renew',1)",
        [check],
    )
    .unwrap();
    let attempt = format!(
        r#"{{"id":"closed","record_id":1,"check_id":{check},"generation":2,"evidence":{EVIDENCE},"actor":"agent","body":"Keep my old Task","continuation":{checkpoint},"recovery":null}}"#
    );
    let outcome = format!(
        r#"{{"attempt_id":"closed","generation":2,"evidence":{EVIDENCE},"verdict":"fail","actor":"agent","message":"checkpoint","disposition":{{"continuation":{HANDOFF}}},"findings":[],"verifications":[]}}"#
    );
    c.execute(
        "INSERT INTO record_attempts(id,record_id,check_id,attempt_json,closed,outcome_json,result_json) VALUES ('closed',1,?1,?2,1,?3,?4)",
        rusqlite::params![check, attempt, outcome, format!(r#"{{"attempt":{attempt},"marker":"kept"}}"#)],
    )
    .unwrap();
    c.pragma_update(None, "user_version", 19).unwrap();
}

#[test]
#[allow(clippy::too_many_lines)] // One Forgeup lifecycle, asserted end to end.
fn a_revision_nineteen_store_with_continuation_rows_upgrades_through_plan_prepare_and_activate() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let old = home.join("old-revision.sh");
    std::fs::write(
        &old,
        format!("#!/bin/sh\nif [ \"$1\" = \"--build-info\" ]; then echo '{{\"commit\":\"{OLD_COMMIT}\",\"dirty\":false,\"management_protocol_revision\":1,\"version\":\"old\"}}'; exit 0; fi\nexit 1\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let server = forge_server();
    let (old_exe, server_exe) = (old.to_str().unwrap(), server.to_str().unwrap());
    let old_selector = ok(&forgeup(
        &home,
        &[
            "register",
            "--source-label",
            "old",
            "--cli",
            old_exe,
            "--daemon",
            old_exe,
            "--local-build-alias",
            "old",
        ],
    ))
    .trim()
    .to_owned();
    ok(&forgeup(
        &home,
        &[
            "register",
            "--source-label",
            "new",
            "--cli",
            server_exe,
            "--daemon",
            server_exe,
            "--local-build-alias",
            "new",
        ],
    ));

    let database = home.join("data/state").join(&old_selector).join("forge.db");
    std::fs::remove_file(&database).unwrap();
    revision_nineteen_store(&database, &home);
    let source_bytes = std::fs::read(&database).unwrap();
    assert!(
        SqliteProjectRepository::open(&database).is_err(),
        "ordinary opening rejects schema 19"
    );

    let plan = ok(&forgeup(
        &home,
        &["upgrade", "plan", "--from", "old", "--to", "new"],
    ));
    let plan: serde_json::Value = serde_json::from_str(&plan).unwrap();
    assert_eq!(plan["migrations"], serde_json::json!([[19, 20]]), "{plan}");
    assert!(plan["blockers"].as_array().unwrap().is_empty(), "{plan}");

    ok(&forgeup(
        &home,
        &[
            "upgrade",
            "prepare",
            "--from",
            "old",
            "--to",
            "new",
            "--confirm-agents-stopped",
        ],
    ));
    ok(&forgeup(
        &home,
        &[
            "upgrade",
            "activate",
            "--to",
            "new",
            "--confirm-agents-stopped",
        ],
    ));
    assert_eq!(
        std::fs::read(&database).unwrap(),
        source_bytes,
        "the source store is preserved"
    );

    let target = std::fs::read_dir(home.join("data/state"))
        .unwrap()
        .map(|entry| entry.unwrap().path().join("forge.db"))
        .find(|path| path != &database)
        .unwrap();
    let repo = SqliteProjectRepository::open(&target).expect("the upgraded store opens normally");
    let repo = repo.in_record_workspace("personal").unwrap();
    assert_eq!(repo.show_record(1).unwrap().body, "Keep my old Task");
    let c = Connection::open(&target).unwrap();
    let tables: Vec<String> = c
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'check_continuation%'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(tables.is_empty(), "{tables:?}");
    let (attempt, outcome): (String, String) = c
        .query_row(
            "SELECT attempt_json, outcome_json FROM record_attempts WHERE id = 'closed'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let attempt: forge_app::outcomes::Attempt = serde_json::from_str(&attempt).unwrap();
    assert_eq!(attempt.id, "closed");
    let outcome: forge_app::outcomes::OutcomeInput = serde_json::from_str(&outcome).unwrap();
    assert_eq!(
        outcome.disposition,
        forge_app::outcomes::OutcomeDisposition::Interrupted
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM assignment_nudges", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0,
        "no Nudge is invented for existing Assignments"
    );
}
