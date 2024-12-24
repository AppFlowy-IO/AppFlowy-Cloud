-- Add migration script here
ALTER TABLE af_collab
ADD COLUMN indexed_at TIMESTAMP WITH TIME ZONE DEFAULT NULL;