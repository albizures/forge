use forge_app::outcomes::{
    CheckContinuations, ContinuationCheckpoint, ContinuationHandoff, Evidence, ProgressReset,
    TaskProgress, WorkState,
};

#[test]
fn continuation_human_and_json_inspection_preserve_accounting_and_context() {
    let mut state = WorkState {
        generation: 1,
        input: None,
        output: None,
        latest_invalidation: None,
        progress: TaskProgress {
            continuations: vec![CheckContinuations {
                check_id: 17,
                total: 1,
                consecutive_unchanged: 0,
                checkpoint: ContinuationCheckpoint {
                    attempt_id: "attempt".into(),
                    evidence: Evidence::Repository {
                        repository: "repo".into(),
                        commit: "a".repeat(40),
                    },
                    handoff: ContinuationHandoff {
                        stopping_reason: "Context constraint".into(),
                        completed_work: "Implemented checkpoint".into(),
                        verification: "Focused tests pass; full gates unrun".into(),
                        remaining_work: "Finish tests".into(),
                    },
                },
                resets: vec![],
            }],
            ..TaskProgress::default()
        },
    };
    for (total, unchanged, diagnostic) in [
        (1, 0, "allowance available"),
        (2, 2, "limit exhausted"),
        (20, 0, "limit exhausted"),
        (0, 0, "allowance available"),
    ] {
        state.progress.continuations[0].total = total;
        state.progress.continuations[0].consecutive_unchanged = unchanged;
        if total == 0 {
            state.progress.continuations[0].resets.push(ProgressReset {
                actor: "operator".into(),
                message: "Investigated and renewed".into(),
                created_at_millis: 123,
            });
        }
        let rendered = forge_cli::render_continuations(&state);
        for text in [
            diagnostic,
            "Context constraint",
            "Implemented checkpoint",
            "full gates unrun",
            "Finish tests",
            "not certified meaningful progress",
            "never authorizes dispatch",
        ] {
            assert!(rendered.contains(text), "{rendered}");
        }
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<WorkState>(&json).unwrap(), state);
        if total == 0 {
            assert!(rendered.contains("Reset by operator at 123: Investigated and renewed"));
        }
    }
}
