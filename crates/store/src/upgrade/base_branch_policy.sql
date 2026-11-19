CREATE TABLE project_base_branch_policy (
    project_slug TEXT PRIMARY KEY NOT NULL REFERENCES projects(slug),
    policy TEXT NOT NULL CHECK (policy IN ('ask', 'planning-branch', 'default-branch'))
);
