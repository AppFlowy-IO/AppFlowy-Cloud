-- stores the published view of a workspace by a user of workspace
CREATE TABLE IF NOT EXISTS af_published_collab (
    doc_name     TEXT   NOT NULL,
    published_by BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    workspace_id UUID   NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    metadata     JSONB  NOT NULL,
    blob         BYTEA  NOT NULL DEFAULT '',
    created_at   TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at   TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,

    PRIMARY KEY (workspace_id, doc_name)
);

-- trigger to update updated_at column
CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER af_published_collab_update_updated_at
BEFORE UPDATE ON af_published_collab
FOR EACH ROW
EXECUTE FUNCTION update_updated_at();

-- every workspace have a prefix for published view
ALTER TABLE af_workspace ADD COLUMN publish_namespace TEXT UNIQUE;
CREATE INDEX IF NOT EXISTS publish_namespace_idx ON af_workspace(publish_namespace);
