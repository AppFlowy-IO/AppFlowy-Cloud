-- We will no longer use `publish_namespace` column to store user defined namespace.
-- `publish_namespace` will only be used to as fallback/default namespace if user did not set custom namespace.
-- `publish_namespace` will be initialized as random UUID and will never be modified.
-- We will remove UNIQUE constraint on `publish_namespace` column to avoid performance penalty on insert.
-- We will use a new table `af_workspace_namespace` to store user defined namespace for workspace.
ALTER TABLE af_workspace DROP CONSTRAINT af_workspace_publish_namespace_key;

-- Table to store user defined namespace for workspace
CREATE TABLE IF NOT EXISTS af_workspace_namespace (
  namespace    TEXT NOT NULL PRIMARY KEY,
  workspace_id UUID NOT NULL,
  is_original  BOOLEAN NOT NULL,
  created_at   TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at   TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,

  FOREIGN KEY (workspace_id) REFERENCES af_workspace (workspace_id) ON DELETE CASCADE
);

-- Create index to ensure fast lookup by workspace_id
CREATE INDEX idx_af_workspace_namespace_workspace_id ON af_workspace_namespace (workspace_id);

-- Create a partial unique index to enforce that only one original namespace exists per workspace
CREATE UNIQUE INDEX ON af_workspace_namespace (workspace_id)
WHERE is_original = TRUE;

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

-- Create a trigger that will create a row in `af_workspace_namespace` when a record is inserted in `af_workspace`
CREATE OR REPLACE FUNCTION create_workspace_namespace()
RETURNS TRIGGER AS $$
BEGIN
  INSERT INTO af_workspace_namespace (namespace, workspace_id, is_original)
  VALUES (uuid_generate_v4()::text, NEW.workspace_id, TRUE);
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trigger_create_workspace_namespace
AFTER INSERT ON af_workspace
FOR EACH ROW
EXECUTE FUNCTION create_workspace_namespace();

-- Insert existing workspace records into `af_workspace_namespace`
INSERT INTO af_workspace_namespace (namespace, workspace_id)
SELECT publish_namespace, workspace_id
FROM af_workspace
ON CONFLICT (namespace) DO NOTHING; -- if there happens to be a workspace creation during migration

-- Drop existing `publish_namespace` column for workspace_id
ALTER TABLE af_workspace DROP COLUMN publish_namespace;
