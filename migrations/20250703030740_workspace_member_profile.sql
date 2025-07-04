CREATE TABLE IF NOT EXISTS af_workspace_member_profile (
  uid BIGINT NOT NULL,
  workspace_id UUID NOT NULL,
  name TEXT,
  avatar_url TEXT,
  cover_image_url TEXT,
  description TEXT,
  PRIMARY KEY (uid, workspace_id),
  FOREIGN KEY (uid, workspace_id) REFERENCES af_workspace_member(uid, workspace_id) ON DELETE CASCADE
);
