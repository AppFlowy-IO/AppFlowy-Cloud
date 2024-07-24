-- stores the comments on a published view
CREATE TABLE IF NOT EXISTS af_published_view_comment (
  comment_id          UUID NOT NULL DEFAULT gen_random_uuid(),
  -- comments are never deleted, only marked as deleted, unless we intentionally wants to clean
  -- the tables by removing the comments from the database
  reply_comment_id    UUID REFERENCES af_published_view_comment(comment_id) ON DELETE CASCADE,
  -- The view id should exists on af_published_collab, However, we can't enforce this foreign key
  -- constraint because af_published_collab primary key is (workspace_id, view_id).
  -- We also have the requirement to keep the comments even if the view is unpublished.
  view_id             UUID NOT NULL,
  content             TEXT NOT NULL,
  -- preserve comment when user is removed
  created_by          BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE SET NULL,
  created_at          TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at          TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  is_deleted          BOOLEAN NOT NULL DEFAULT FALSE,

  PRIMARY KEY         (comment_id)
);
CREATE INDEX IF NOT EXISTS idx_view_id_on_af_published_view_comment ON af_published_view_comment(view_id);
