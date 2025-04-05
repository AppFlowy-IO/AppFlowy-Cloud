CREATE TABLE IF NOT EXISTS af_workspace_invite_code (
  id UUID PRIMARY KEY DEFAULT UUID_GENERATE_V4 (),
  invite_code TEXT NOT NULL,
  expires_at TIMESTAMP,
  workspace_id UUID NOT NULL REFERENCES af_workspace (workspace_id) ON DELETE CASCADE
);

CREATE INDEX idx_af_workspace_invite_code ON af_workspace_invite_code (invite_code);
