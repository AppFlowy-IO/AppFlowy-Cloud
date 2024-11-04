-- We will no longer use `publish_namespace` column to store user defined namespace.
-- `publish_namespace` will only be used to as fallback/default namespace if user did not set custom namespace.
-- `publish_namespace` will be initialized as random UUID and will never be modified.
-- We will remove UNIQUE constraint on `publish_namespace` column to avoid performance penalty on insert.
-- We will use a new table `af_workspace_namespace` to store user defined namespace for workspace.
ALTER TABLE af_workspace DROP CONSTRAINT af_workspace_publish_namespace_key;

-- Table to store user defined namespace for workspace
CREATE TABLE IF NOT EXISTS af_workspace_namespace (
  workspace_id UUID,
  namespace    TEXT      NOT NULL UNIQUE,
  created_at   TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  updated_at   TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,

  FOREIGN KEY (workspace_id) REFERENCES af_workspace (workspace_id) ON DELETE CASCADE
);

-- Create index for workspace_id
CREATE INDEX idx_af_workspace_namespace_workspace_id ON af_workspace_namespace (workspace_id);

-- Create index for namespace
CREATE INDEX idx_af_workspace_namespace_namespace    ON af_workspace_namespace (namespace);

-- Create a function to update the updated_at column
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
  NEW.updated_at = CURRENT_TIMESTAMP;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create a trigger to call the function before each update
CREATE TRIGGER trigger_update_updated_at
BEFORE UPDATE ON af_workspace_namespace
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();
