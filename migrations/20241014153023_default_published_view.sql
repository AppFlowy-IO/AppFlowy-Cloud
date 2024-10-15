ALTER TABLE af_workspace ADD COLUMN default_view_id UUID;

ALTER TABLE af_published_collab ALTER COLUMN created_at SET NOT NULL;
ALTER TABLE af_published_collab ALTER COLUMN updated_at SET NOT NULL;
