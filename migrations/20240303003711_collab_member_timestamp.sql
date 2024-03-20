-- Add migration script here
ALTER TABLE af_collab_member
ADD COLUMN created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL DEFAULT (NOW());