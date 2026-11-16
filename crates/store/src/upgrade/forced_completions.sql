CREATE TABLE IF NOT EXISTS forced_completions (
        record_id INTEGER PRIMARY KEY NOT NULL REFERENCES records(id),
        update_id INTEGER NOT NULL UNIQUE REFERENCES record_updates(id),
        actor TEXT NOT NULL CHECK(length(trim(actor)) > 0),
        resolution TEXT NOT NULL CHECK(length(trim(resolution)) > 0),
        bypassed_gates_json TEXT NOT NULL);
