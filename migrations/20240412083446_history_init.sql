CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- create af_snapshot_meta table
CREATE TABLE IF NOT EXISTS af_snapshot_meta(
    oid TEXT NOT NULL,
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    snapshot BYTEA NOT NULL,
    snapshot_version INTEGER NOT NULL,
    partition_key INTEGER NOT NULL,
    created_at BIGINT NOT NULL,
    metadata JSONB,
    PRIMARY KEY (oid, created_at, partition_key)
) PARTITION BY LIST (partition_key);
CREATE TABLE af_snapshot_meta_document PARTITION OF af_snapshot_meta FOR
VALUES IN (0);
CREATE TABLE af_snapshot_meta_database PARTITION OF af_snapshot_meta FOR
VALUES IN (1);
CREATE TABLE af_snapshot_meta_workspace_database PARTITION OF af_snapshot_meta FOR
VALUES IN (2);
CREATE TABLE af_snapshot_meta_folder PARTITION OF af_snapshot_meta FOR
VALUES IN (3);
CREATE TABLE af_snapshot_meta_database_row PARTITION OF af_snapshot_meta FOR
VALUES IN (4);
CREATE TABLE af_snapshot_meta_user_awareness PARTITION OF af_snapshot_meta FOR
VALUES IN (5);

-- create af_snapshot_state table
CREATE TABLE IF NOT EXISTS af_snapshot_state(
    snapshot_id UUID NOT NULL DEFAULT uuid_generate_v4(),
    workspace_id UUID NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
    oid TEXT NOT NULL,
    doc_state BYTEA NOT NULL,
    doc_state_version INTEGER NOT NULL,
    deps_snapshot_id UUID,
    partition_key INTEGER NOT NULL,
    created_at BIGINT NOT NULL,
    PRIMARY KEY (snapshot_id, partition_key)
) PARTITION BY LIST (partition_key);
CREATE TABLE af_snapshot_state_document PARTITION OF af_snapshot_state FOR
VALUES IN (0);
CREATE TABLE af_snapshot_state_database PARTITION OF af_snapshot_state FOR
VALUES IN (1);
CREATE TABLE af_snapshot_state_workspace_database PARTITION OF af_snapshot_state FOR
VALUES IN (2);
CREATE TABLE af_snapshot_state_folder PARTITION OF af_snapshot_state FOR
VALUES IN (3);
CREATE TABLE af_snapshot_state_database_row PARTITION OF af_snapshot_state FOR
VALUES IN (4);
CREATE TABLE af_snapshot_state_user_awareness PARTITION OF af_snapshot_state FOR
VALUES IN (5);

-- Index for af_snapshot_state table
CREATE INDEX IF NOT EXISTS idx_snapshot_state_oid_created ON af_snapshot_state (oid, created_at DESC);
