CREATE TABLE IF NOT EXISTS af_workspace_ai_usage (
    created_at DATE NOT NULL,               -- day level of granularity
    workspace_id UUID NOT NULL,             -- workspace id for which the usage is being recorded
    search_requests INT,                    -- number of search requests made
    search_tokens_consumed BIGINT,          -- number of tokens consumed for search requests
    index_tokens_consumed BIGINT,           -- number of tokens consumed for indexing documents
    PRIMARY KEY (created_at, workspace_id)
);

-- migrate token usage data from af_workspace to af_workspace_ai_usage
INSERT INTO af_workspace_ai_usage (created_at, workspace_id, search_tokens_consumed, index_tokens_consumed)
SELECT
    now()::date as created_at,
    workspace_id,
    search_token_usage as search_tokens_consumed,
    index_token_usage as index_tokens_consumed
FROM af_workspace
WHERE search_token_usage IS NOT NULL
   OR index_token_usage IS NOT NULL;

-- drop the redundant columns from af_workspace
ALTER TABLE af_workspace DROP COLUMN IF EXISTS search_token_usage;
ALTER TABLE af_workspace DROP COLUMN IF EXISTS index_token_usage;