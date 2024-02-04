-- af_workspace contains all the workspaces. Each workspace contains a list of members defined in af_workspace_member
CREATE TABLE IF NOT EXISTS af_workspace (
    workspace_id UUID NOT NULL PRIMARY KEY DEFAULT uuid_generate_v4(),
    database_storage_id UUID NOT NULL DEFAULT uuid_generate_v4(),
    owner_uid BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    -- 0: Free
    workspace_type INTEGER NOT NULL DEFAULT 0,
    deleted_at TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    workspace_name TEXT DEFAULT 'My Workspace'
);

-- Enable RLS on the af_workspace table
ALTER TABLE af_workspace ENABLE ROW LEVEL SECURITY;
CREATE POLICY af_workspace_policy ON af_workspace FOR ALL TO anon,
    authenticated USING (true);
ALTER TABLE af_workspace FORCE ROW LEVEL SECURITY;

-- af_workspace_member contains all the members associated with a workspace and their roles.
CREATE TABLE IF NOT EXISTS af_workspace_member (
    uid BIGINT NOT NULL,
    role_id INT NOT NULL REFERENCES af_roles(id),
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (uid, workspace_id)
);

-- Enable RLS on the af_workspace_member table
ALTER TABLE af_workspace_member ENABLE ROW LEVEL SECURITY;
CREATE POLICY af_workspace_member_policy ON af_workspace_member FOR ALL TO anon,
    authenticated USING (true);
ALTER TABLE af_workspace_member FORCE ROW LEVEL SECURITY;

-- Listener for af_workspace_member table
DROP TRIGGER IF EXISTS af_workspace_member_change_trigger ON af_workspace_member;

CREATE OR REPLACE FUNCTION notify_af_workspace_member_change() RETURNS trigger AS $$
DECLARE
    payload TEXT;
BEGIN
    payload := json_build_object(
            'old', row_to_json(OLD),
            'new', row_to_json(NEW),
            'action_type', TG_OP
            )::text;

    PERFORM pg_notify('af_workspace_member_channel', payload);
    -- Return the new row state for INSERT/UPDATE, and the old state for DELETE.
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
    ELSE
        RETURN NEW;
    END IF;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER af_workspace_member_change_trigger
    AFTER INSERT OR UPDATE OR DELETE ON af_workspace_member
    FOR EACH ROW EXECUTE FUNCTION notify_af_workspace_member_change();

-- Index
CREATE UNIQUE INDEX idx_af_workspace_member ON af_workspace_member (uid, workspace_id, role_id);
-- Insert a workspace member if the user with given uid is the owner of the workspace
CREATE OR REPLACE FUNCTION insert_af_workspace_member_if_owner(
        p_uid BIGINT,
        p_role_id INT,
        p_workspace_id UUID
    ) RETURNS VOID AS $$ BEGIN -- If user is the owner, proceed with the insert operation
INSERT INTO af_workspace_member (uid, role_id, workspace_id)
SELECT p_uid,
       p_role_id,
       p_workspace_id
FROM af_workspace
WHERE workspace_id = p_workspace_id
  AND owner_uid = p_uid;
-- Check if the insert operation was successful. If not, user is not the owner of the workspace.
IF NOT FOUND THEN RAISE EXCEPTION 'Unsupported operation: User is not the owner of the workspace.';
END IF;
END;
$$ LANGUAGE plpgsql;
