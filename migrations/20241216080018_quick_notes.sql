CREATE TABLE IF NOT EXISTS af_quick_note (
  quick_note_id UUID NOT NULL DEFAULT gen_random_uuid (),
  workspace_id UUID NOT NULL,
  uid BIGINT NOT NULL REFERENCES af_user (uid) ON DELETE CASCADE,
  updated_at TIMESTAMP
  WITH
    TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP
  WITH
    TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    data JSONB NOT NULL,
    PRIMARY KEY (quick_note_id)
);

CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_quick_note ON af_quick_note (workspace_id);

CREATE INDEX IF NOT EXISTS idx_uid_on_af_quick_note ON af_quick_note (uid);
