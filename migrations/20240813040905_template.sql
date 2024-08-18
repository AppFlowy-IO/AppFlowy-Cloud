-- Appflowy template is based on a published view. Template information should be preserved even if the
-- published view is deleted.
CREATE TABLE IF NOT EXISTS af_template_view (
  view_id           UUID NOT NULL,
  created_at        TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at        TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  name              TEXT NOT NULL,
  description       TEXT NOT NULL,
  about             TEXT NOT NULL,
  view_url          TEXT NOT NULL,
  creator_id        UUID NOT NULL REFERENCES af_template_creator(creator_id) ON DELETE CASCADE,
  is_new_template   BOOLEAN NOT NULL,
  is_featured       BOOLEAN NOT NULL,

  PRIMARY KEY (view_id)
);

CREATE TABLE IF NOT EXISTS af_template_view_template_category (
  view_id       UUID NOT NULL REFERENCES af_template_view(view_id) ON DELETE CASCADE,
  category_id   UUID NOT NULL REFERENCES af_template_category(category_id) ON DELETE CASCADE,

  PRIMARY KEY (view_id, category_id)
);

CREATE TABLE IF NOT EXISTS af_related_template_view (
  view_id           UUID NOT NULL REFERENCES af_template_view(view_id) ON DELETE CASCADE,
  related_view_id   UUID NOT NULL REFERENCES af_template_view(view_id) ON DELETE CASCADE,

  PRIMARY KEY (view_id, related_view_id)
);

BEGIN;

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'account_link_type') THEN
    CREATE TYPE account_link_type AS (
        link_type    TEXT,
        url          TEXT
    );
  END IF;

  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'template_creator_type') THEN
    CREATE TYPE template_creator_type AS (
        creator_id          UUID,
        name                TEXT,
        avatar_url          TEXT,
        account_links       account_link_type[],
        number_of_templates INT
    );
  END IF;

  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'template_creator_minimal_type') THEN
    CREATE TYPE template_creator_minimal_type AS (
        creator_id    UUID,
        name          TEXT,
        avatar_url    TEXT
    );
  END IF;

  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'template_category_minimal_type') THEN
    CREATE TYPE template_category_minimal_type AS (
      category_id   UUID,
      name          TEXT,
      icon          TEXT,
      bg_color      TEXT
    );
  END IF;

  IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'template_minimal_type') THEN
    CREATE TYPE template_minimal_type AS (
      view_id         UUID,
      created_at      TIMESTAMP WITH TIME ZONE,
      updated_at      TIMESTAMP WITH TIME ZONE,
      name            TEXT,
      description     TEXT,
      view_url        TEXT,
      creator         template_creator_minimal_type,
      categories      template_category_minimal_type[],
      is_new_template BOOLEAN,
      is_featured     BOOLEAN
    );
  END IF;
END
$$;
