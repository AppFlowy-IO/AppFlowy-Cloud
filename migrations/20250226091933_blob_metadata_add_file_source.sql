-- Add migration script here
ALTER TABLE af_blob_metadata
ADD COLUMN source SMALLINT NOT NULL DEFAULT 0,
ADD COLUMN source_metadata JSONB DEFAULT '{}'::jsonb;