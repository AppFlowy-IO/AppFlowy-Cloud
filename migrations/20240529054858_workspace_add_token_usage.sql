-- Add migration script here
ALTER TABLE af_workspace ADD COLUMN search_token_usage BIGINT NOT NULL DEFAULT 0;
ALTER TABLE af_workspace ADD COLUMN index_token_usage BIGINT NOT NULL DEFAULT 0;