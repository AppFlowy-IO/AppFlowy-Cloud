CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_blob_metadata          ON af_blob_metadata          (workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_chat                   ON af_chat                   (workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_collab_snapshot        ON af_collab_snapshot        (workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_collab                 ON af_collab                 (workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_snapshot_meta          ON af_snapshot_meta          (workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_snapshot_state         ON af_snapshot_state         (workspace_id);
CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_workspace_member       ON af_workspace_member       (workspace_id);
