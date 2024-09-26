CREATE TABLE IF NOT EXISTS af_access_request (
  request_id UUID NOT NULL DEFAULT gen_random_uuid(),
  uid BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
  workspace_id UUID NOT NULL,
  view_id UUID NOT NULL,
  status INT NOT NULL,
  created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE (uid, workspace_id, view_id),
  PRIMARY KEY(request_id)
);

CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_access_request ON af_access_request(workspace_id);
CREATE INDEX IF NOT EXISTS idx_uid_on_af_access_request ON af_access_request(uid);
