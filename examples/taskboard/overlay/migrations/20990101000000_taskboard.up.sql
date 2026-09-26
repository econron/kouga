CREATE TABLE projects (
    id uuid PRIMARY KEY,
    owner_id uuid NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    slug text NOT NULL CHECK (length(slug) BETWEEN 1 AND 80),
    name text NOT NULL CHECK (length(name) BETWEEN 1 AND 100),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (owner_id, slug),
    UNIQUE (id, owner_id)
);
CREATE INDEX projects_owner_created_idx ON projects (owner_id, created_at DESC, id);

CREATE TABLE tasks (
    id uuid PRIMARY KEY,
    project_id uuid NOT NULL,
    owner_id uuid NOT NULL,
    title text NOT NULL CHECK (length(title) BETWEEN 1 AND 200),
    completed boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT tasks_project_owner_fk FOREIGN KEY (project_id, owner_id)
        REFERENCES projects (id, owner_id) ON DELETE RESTRICT,
    UNIQUE (project_id, title)
);
CREATE INDEX tasks_owner_created_idx ON tasks (owner_id, created_at DESC, id);
CREATE INDEX tasks_project_created_idx ON tasks (project_id, created_at DESC, id);
