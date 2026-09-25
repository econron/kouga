ALTER TABLE kouga_jobs ADD COLUMN failure_reason text;
CREATE INDEX kouga_jobs_expired ON kouga_jobs (queue, lease_until) WHERE status = 'running';
