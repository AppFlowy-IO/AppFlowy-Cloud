-- Add migration script here
ALTER TABLE af_import_task
ADD COLUMN uid BIGINT,
ADD COLUMN file_url TEXT;

-- Update the existing index to include the new uid column
DROP INDEX IF EXISTS idx_af_import_task_status_created_at;

CREATE INDEX idx_af_import_task_uid_status_created_at
ON af_import_task (uid, status, created_at);
