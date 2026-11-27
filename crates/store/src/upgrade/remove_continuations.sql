-- Revision 20 removes Check-scoped Continuation state. Counters, checkpoints and reset
-- audit rows are dropped; no read surface depends on them. Persisted Attempt and outcome
-- JSON loses the retired shapes so historical rows still decode and replay.
UPDATE record_attempts
SET attempt_json = json_remove(attempt_json, '$.continuation')
WHERE json_type(attempt_json, '$.continuation') IS NOT NULL;
UPDATE record_attempts
SET result_json = json_remove(result_json, '$.attempt.continuation')
WHERE result_json IS NOT NULL AND json_type(result_json, '$.attempt.continuation') IS NOT NULL;
UPDATE record_attempts
SET outcome_json = json_set(outcome_json, '$.disposition', 'interrupted')
WHERE outcome_json IS NOT NULL AND json_type(outcome_json, '$.disposition.continuation') IS NOT NULL;
DROP TABLE check_continuation_resets;
DROP TABLE check_continuations;
