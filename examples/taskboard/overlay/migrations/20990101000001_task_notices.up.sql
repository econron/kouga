-- A job may be executed more than once after lease expiry. This business effect is once per job.
CREATE TABLE task_notice_effects (
    job_id uuid PRIMARY KEY,
    task_id uuid NOT NULL,
    applied_at timestamptz NOT NULL DEFAULT now()
);
