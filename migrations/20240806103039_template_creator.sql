CREATE TABLE IF NOT EXISTS af_template_creator (
  creator_id        UUID NOT NULL DEFAULT gen_random_uuid(),
  name              TEXT NOT NULL,
  avatar_url        TEXT NOT NULL,

  PRIMARY KEY (creator_id)
);

CREATE TABLE IF NOT EXISTS af_template_creator_account_link (
  creator_id        UUID NOT NULL REFERENCES af_template_creator(creator_id) ON DELETE CASCADE,
  link_type         TEXT NOT NULL,
  url               TEXT NOT NULL,

  PRIMARY KEY (creator_id, link_type)
);
