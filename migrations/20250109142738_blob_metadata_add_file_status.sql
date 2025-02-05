-- Add migration script here
ALTER TABLE af_blob_metadata
ADD COLUMN status SMALLINT NOT NULL DEFAULT 0;
