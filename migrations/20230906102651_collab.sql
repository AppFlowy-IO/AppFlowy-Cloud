-- collab update table.
CREATE TABLE IF NOT EXISTS af_collab (
    oid TEXT NOT NULL,
    blob BYTEA NOT NULL,
    len INTEGER,
    partition_key INTEGER NOT NULL,
    encrypt INTEGER DEFAULT 0,
    owner_uid BIGINT NOT NULL,
    deleted_at TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    PRIMARY KEY (oid, partition_key)
) PARTITION BY LIST (partition_key);
CREATE TABLE af_collab_document PARTITION OF af_collab FOR
VALUES IN (0);
CREATE TABLE af_collab_database PARTITION OF af_collab FOR
VALUES IN (1);
CREATE TABLE af_collab_w_database PARTITION OF af_collab FOR
VALUES IN (2);
CREATE TABLE af_collab_folder PARTITION OF af_collab FOR
VALUES IN (3);
CREATE TABLE af_collab_database_row PARTITION OF af_collab FOR
VALUES IN (4);
CREATE TABLE af_collab_user_awareness PARTITION OF af_collab FOR
VALUES IN (5);

CREATE TABLE af_collab_member (
    uid BIGINT REFERENCES af_user(uid) ON DELETE CASCADE,
    oid TEXT NOT NULL,
    permission_id INTEGER REFERENCES af_permissions(id) NOT NULL,
    PRIMARY KEY(uid, oid)
);

-- Listener
DROP TRIGGER IF EXISTS af_collab_member_change_trigger ON af_collab_member;

CREATE OR REPLACE FUNCTION notify_af_collab_member_change() RETURNS trigger AS $$
DECLARE
payload TEXT;
BEGIN
    payload := json_build_object(
            'old', row_to_json(OLD),
            'new', row_to_json(NEW),
            'action_type', TG_OP
            )::text;

    PERFORM pg_notify('af_collab_member_channel', payload);
    -- Return the new row state for INSERT/UPDATE, and the old state for DELETE.
    IF TG_OP = 'DELETE' THEN
        RETURN OLD;
ELSE
        RETURN NEW;
END IF;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER af_collab_member_change_trigger
    AFTER INSERT OR UPDATE OR DELETE ON af_collab_member
    FOR EACH ROW EXECUTE FUNCTION notify_af_collab_member_change();

-- collab snapshot. It will be used to store the snapshots of the collab.
CREATE TABLE IF NOT EXISTS af_collab_snapshot (
    sid BIGSERIAL PRIMARY KEY,-- snapshot id
    oid TEXT NOT NULL,
    blob BYTEA NOT NULL,
    len INTEGER NOT NULL,
    encrypt INTEGER DEFAULT 0,
    deleted_at TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL
);
CREATE INDEX idx_af_collab_snapshot_oid ON af_collab_snapshot(oid);
-- Enable RLS on the af_collab table
ALTER TABLE af_collab ENABLE ROW LEVEL SECURITY;
CREATE POLICY af_collab_policy ON af_collab FOR ALL TO anon,
    authenticated USING (true);
ALTER TABLE af_collab FORCE ROW LEVEL SECURITY;
