-- af_collab contains all the collabs.
CREATE TABLE IF NOT EXISTS af_collab(
    oid TEXT PRIMARY KEY,
    owner_uid BIGINT NOT NULL,
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    -- 0: Private, 1: Shared
    access_level INTEGER NOT NULL DEFAULT 0,
    deleted_at TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);
CREATE UNIQUE INDEX idx_af_collab_oid ON af_collab (oid);
-- collab update table.
CREATE TABLE IF NOT EXISTS af_collab_update (
    oid TEXT REFERENCES af_collab(oid) ON DELETE CASCADE,
    blob BYTEA NOT NULL,
    len INTEGER,
    partition_key INTEGER NOT NULL,
    md5 TEXT DEFAULT '',
    did TEXT DEFAULT '',
    encrypt INTEGER DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    PRIMARY KEY (oid, partition_key)
) PARTITION BY LIST (partition_key);
CREATE TABLE af_collab_update_document PARTITION OF af_collab_update FOR
VALUES IN (0);
CREATE TABLE af_collab_update_database PARTITION OF af_collab_update FOR
VALUES IN (1);
CREATE TABLE af_collab_update_w_database PARTITION OF af_collab_update FOR
VALUES IN (2);
CREATE TABLE af_collab_update_folder PARTITION OF af_collab_update FOR
VALUES IN (3);
CREATE TABLE af_collab_update_database_row PARTITION OF af_collab_update FOR
VALUES IN (4);
CREATE TABLE af_collab_update_user_awareness PARTITION OF af_collab_update FOR
VALUES IN (5);
-- This trigger is fired before an insert operation on the af_collab_update table. It checks if a corresponding collab
-- exists in the af_collab table. If not, it creates one with the oid, uid, and current timestamp. It might cause a
-- performance issue if the af_collab_update table is updated very frequently, especially if the af_collab table is large
-- and if the oid column isn't indexed
CREATE OR REPLACE FUNCTION insert_into_af_collab_if_not_exists() RETURNS TRIGGER AS $$ BEGIN IF NOT EXISTS (
        SELECT 1
        FROM af_collab
        WHERE oid = NEW.oid
    ) THEN
INSERT INTO af_collab (oid, owner_uid, workspace_id, created_at)
VALUES (
        NEW.oid,
        NEW.uid,
        NEW.workspace_id,
        CURRENT_TIMESTAMP
    );
END IF;
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER insert_into_af_collab_trigger BEFORE
INSERT ON af_collab_update FOR EACH ROW EXECUTE FUNCTION insert_into_af_collab_if_not_exists();
CREATE TABLE af_collab_member (
    uid BIGINT REFERENCES af_user(uid) ON DELETE CASCADE,
    oid TEXT REFERENCES af_collab(oid) ON DELETE CASCADE,
    role_id INTEGER REFERENCES af_roles(id),
    PRIMARY KEY(uid, oid)
);
-- This trigger is fired after an insert operation on the af_collab table. It automatically adds the collab's owner
-- to the af_collab_member table with the role 'Owner'.
CREATE OR REPLACE FUNCTION insert_into_af_collab_member() RETURNS TRIGGER AS $$ BEGIN
INSERT INTO af_collab_member (oid, uid, role_id)
VALUES (
        NEW.oid,
        NEW.owner_uid,
        (
            SELECT id
            FROM af_roles
            WHERE name = 'Owner'
        )
    );
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER insert_into_af_collab_member_trigger
AFTER
INSERT ON af_collab FOR EACH ROW EXECUTE FUNCTION insert_into_af_collab_member();
-- collab statistics. It will be used to store the edit_count of the collab.
CREATE TABLE IF NOT EXISTS af_collab_statistics (
    oid TEXT PRIMARY KEY,
    edit_count BIGINT NOT NULL DEFAULT 0
);
-- This trigger is fired after an insert operation on the af_collab_update table. It increments the edit_count of the
-- corresponding collab in the af_collab_statistics table. If the collab doesn't exist in the af_collab_statistics table,
-- it creates one with edit_count set to 1.
CREATE OR REPLACE FUNCTION increment_af_collab_edit_count() RETURNS TRIGGER AS $$BEGIN IF EXISTS(
        SELECT 1
        FROM af_collab_statistics
        WHERE oid = NEW.oid
    ) THEN
UPDATE af_collab_statistics
SET edit_count = edit_count + 1
WHERE oid = NEW.oid;
ELSE
INSERT INTO af_collab_statistics (oid, edit_count)
VALUES (NEW.oid, 1);
END IF;
RETURN NEW;
END;
$$LANGUAGE plpgsql;
CREATE TRIGGER af_collab_update_edit_count_trigger
AFTER
INSERT ON af_collab_update FOR EACH ROW EXECUTE FUNCTION increment_af_collab_edit_count();
-- collab snapshot. It will be used to store the snapshots of the collab.
CREATE TABLE IF NOT EXISTS af_collab_snapshot (
    sid BIGSERIAL PRIMARY KEY,
    oid TEXT NOT NULL,
    name TEXT DEFAULT '',
    blob BYTEA NOT NULL,
    len INTEGER NOT NULL,
    edit_count BIGINT NOT NULL DEFAULT 0,
    encrypt INTEGER DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);
-- This trigger is fired after an insert operation on the af_collab_snapshot table. It automatically sets the edit_count
-- of the new snapshot to the current edit_count of the collab in the af_collab_statistics table.
CREATE OR REPLACE FUNCTION af_collab_snapshot_update_edit_count() RETURNS TRIGGER AS $$ BEGIN NEW.edit_count := COALESCE(
        (
            SELECT af_collab_statistics.edit_count
            FROM af_collab_statistics
            WHERE af_collab_statistics.oid = NEW.oid
        ),
        -- If the row in af_collab_statistics with given oid is found, set edit_count to 0
        0
    );
RETURN NEW;
END;
$$LANGUAGE plpgsql;
CREATE TRIGGER af_collab_snapshot_update_edit_count_trigger BEFORE
INSERT ON af_collab_snapshot FOR EACH ROW EXECUTE FUNCTION af_collab_snapshot_update_edit_count();
-- collab state view. It will be used to get the current state of the collab.
CREATE VIEW af_collab_state AS
SELECT a.oid,
    a.created_at AS snapshot_created_at,
    a.edit_count AS snapshot_edit_count,
    b.edit_count AS current_edit_count
FROM af_collab_snapshot AS a
    JOIN af_collab_statistics AS b ON a.oid = b.oid;
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
