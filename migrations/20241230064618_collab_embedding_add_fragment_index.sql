-- Add migration script here
ALTER TABLE af_collab_embeddings
ADD COLUMN fragment_index INTEGER DEFAULT 0,
ADD COLUMN embedder_type SMALLINT DEFAULT 0;

CREATE TYPE af_fragment_v3 AS (
    fragment_id TEXT,
    content_type INT,
    contents TEXT,
    embedding VECTOR(1536),
    metadata JSONB,
    fragment_index INTEGER,
    embedder_type SMALLINT
);

CREATE OR REPLACE PROCEDURE af_collab_embeddings_upsert(
    IN p_workspace_id UUID,
    IN p_oid TEXT,
    IN p_partition_key INT,
    IN p_tokens_used INT,
    IN p_fragments af_fragment_v3[]
)
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM af_collab_embeddings WHERE oid = p_oid;
    INSERT INTO af_collab_embeddings (fragment_id, oid, partition_key, content_type, content, embedding, indexed_at, metadata, fragment_index, embedder_type)
    SELECT
        f.fragment_id,
        p_oid,
        p_partition_key,
        f.content_type,
        f.contents,
        f.embedding,
        NOW(),
        f.metadata,
        f.fragment_index,
        f.embedder_type
    FROM UNNEST(p_fragments) as f;

    -- Update the usage tracking table
    INSERT INTO af_workspace_ai_usage(created_at, workspace_id, search_requests, search_tokens_consumed, index_tokens_consumed)
    VALUES (now()::date, p_workspace_id, 0, 0, p_tokens_used)
    ON CONFLICT (created_at, workspace_id)
    DO UPDATE SET index_tokens_consumed = af_workspace_ai_usage.index_tokens_consumed + p_tokens_used;
END
$$;