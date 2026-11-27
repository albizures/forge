CREATE TABLE IF NOT EXISTS record_planning_context (
    record_id INTEGER NOT NULL,
    project_slug TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('available', 'unavailable')),
    repository TEXT,
    branch TEXT,
    reason TEXT,
    PRIMARY KEY (record_id, project_slug),
    FOREIGN KEY (record_id, project_slug) REFERENCES record_projects(record_id, project_slug),
    CHECK (
        (status = 'available' AND repository IS NOT NULL AND length(trim(repository)) > 0
            AND branch IS NOT NULL AND length(trim(branch)) > 0 AND reason IS NULL)
        OR
        (status = 'unavailable' AND repository IS NULL AND branch IS NULL
            AND reason IS NOT NULL AND length(trim(reason)) > 0)
    )
);
