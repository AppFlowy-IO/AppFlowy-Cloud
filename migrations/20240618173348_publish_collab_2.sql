ALTER TABLE af_published_collab ADD COLUMN view_id UUID NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE af_published_collab DROP CONSTRAINT af_published_collab_pkey;
ALTER TABLE af_published_collab ADD PRIMARY KEY (workspace_id, view_id);

CREATE INDEX IF NOT EXISTS idx_workspace_id_on_af_published_collab ON af_published_collab (workspace_id);
CREATE INDEX IF NOT EXISTS idx_published_by_on_af_published_collab ON af_published_collab (published_by);
