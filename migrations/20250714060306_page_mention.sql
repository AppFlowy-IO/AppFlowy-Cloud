CREATE TABLE IF NOT EXISTS af_page_mention (
  workspace_id UUID NOT NULL,
  view_id UUID NOT NULL,
  person_id UUID NOT NULL,
  block_id TEXT,
  mentioned_by BIGINT,
  mentioned_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (workspace_id, view_id, person_id),
  FOREIGN KEY (mentioned_by) REFERENCES af_user(uid) ON DELETE SET NULL,
  FOREIGN KEY (workspace_id) REFERENCES af_workspace(workspace_id) ON DELETE CASCADE
);
