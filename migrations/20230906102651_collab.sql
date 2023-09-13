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
    oid TEXT NOT NULL ,
    role_id INTEGER REFERENCES af_roles(id),
    PRIMARY KEY(uid, oid)
);

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


