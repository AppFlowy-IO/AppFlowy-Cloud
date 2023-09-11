-- af_workspace contains all the workspaces. Each workspace contains a list of members defined in af_workspace_member
CREATE TABLE IF NOT EXISTS af_workspace (
    workspace_id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    database_storage_id UUID DEFAULT uuid_generate_v4(),
    owner_uid BIGINT REFERENCES af_user(uid) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    -- 0: Free
    workspace_type INTEGER NOT NULL DEFAULT 0,
    deleted_at TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    workspace_name TEXT DEFAULT 'My Workspace'
);
-- This trigger is fired after an insert operation on the af_user table. It automatically creates a workspace
-- in the af_workspace table with the uid of the new user profile as the owner_uid
CREATE OR REPLACE FUNCTION create_af_workspace_func() RETURNS TRIGGER AS $$BEGIN
INSERT INTO af_workspace (owner_uid)
VALUES (NEW.uid);
RETURN NEW;
END $$LANGUAGE plpgsql;
CREATE TRIGGER create_af_workspace_trigger
AFTER
INSERT ON af_user FOR EACH ROW EXECUTE FUNCTION create_af_workspace_func();
-- af_workspace_member contains all the members associated with a workspace and their roles.
CREATE TABLE IF NOT EXISTS af_workspace_member (
    uid BIGINT NOT NULL,
    role_id INT NOT NULL REFERENCES af_roles(id),
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (uid, workspace_id)
);
CREATE UNIQUE INDEX idx_af_workspace_member ON af_workspace_member (uid, workspace_id, role_id);
-- This trigger is fired after an insert operation on the af_workspace table. It automatically creates a workspace
-- member in the af_workspace_member table. If the user is the owner of the workspace, they are given the role 'Owner'.
CREATE OR REPLACE FUNCTION manage_af_workspace_member_role_func() RETURNS TRIGGER AS $$ BEGIN
INSERT INTO af_workspace_member (uid, role_id, workspace_id)
VALUES (
        NEW.owner_uid,
        (
            SELECT id
            FROM af_roles
            WHERE name = 'Owner'
        ),
        NEW.workspace_id
    ) ON CONFLICT (uid, workspace_id) DO
UPDATE
SET role_id = EXCLUDED.role_id;
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER manage_af_workspace_member_role_trigger
AFTER
INSERT ON af_workspace FOR EACH ROW EXECUTE FUNCTION manage_af_workspace_member_role_func();
