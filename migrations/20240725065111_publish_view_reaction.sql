-- stores the reactions on a published view
CREATE TABLE IF NOT EXISTS af_published_view_reaction (
  comment_id          UUID NOT NULL REFERENCES af_published_view_comment(comment_id) ON DELETE CASCADE,
  reaction_type       TEXT NOT NULL,
  created_by          BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,
  view_id             UUID NOT NULL,
  PRIMARY KEY         (comment_id, reaction_type, created_by)
);
CREATE INDEX IF NOT EXISTS idx_view_id_on_af_published_view_reaction ON af_published_view_reaction(view_id);
