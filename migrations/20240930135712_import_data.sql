-- Add migration script here
CREATE TABLE af_import_task(
    task_id UUID NOT NULL PRIMARY KEY,
    file_size BIGINT NOT NULL,          -- File size in bytes, BIGINT for large files
    workspace_id TEXT NOT NULL,         -- Workspace id
    created_by BIGINT NOT NULL,         -- User ID
    status INT NOT NULL,                -- Status of the file import (e.g., 0 for pending, 1 for completed, 2 for failed)
    metadata JSONB DEFAULT '{}' NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_af_import_task_status_created_at
ON af_import_task (status, created_at);

-- For existing workspaces, this column will be NULL. So Null and true will be considered as
-- initialized and false will be considered as not initialized.
ALTER TABLE af_workspace
ADD COLUMN is_initialized BOOLEAN DEFAULT NULL;

CREATE INDEX idx_af_workspace_is_initialized
ON af_workspace (is_initialized);