CREATE TABLE IF NOT EXISTS af_blob_metadata (
    workspace_id UUID REFERENCES af_workspace(workspace_id) ON DELETE CASCADE NOT NULL,
    file_id VARCHAR NOT NULL,
    file_type VARCHAR NOT NULL,
    file_size BIGINT NOT NULL,
    modified_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (workspace_id, file_id)
);
