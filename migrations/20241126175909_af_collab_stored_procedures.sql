CREATE OR REPLACE PROCEDURE af_collab_upsert(
    IN p_workspace_id UUID,
    IN p_oid TEXT,
    IN p_partition_key INT,
    IN p_uid BIGINT,
    IN p_encrypt INT,
    IN p_blob BYTEA
)
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO af_collab (oid, blob, len, partition_key, encrypt, owner_uid, workspace_id)
    VALUES (p_oid, p_blob, LENGTH(p_blob), p_partition_key, p_encrypt, p_uid, p_workspace_id)
    ON CONFLICT (oid, partition_key)
    DO UPDATE SET blob = p_blob, len = LENGTH(p_blob), encrypt = p_encrypt, owner_uid = p_uid WHERE excluded.workspace_id = p_workspace_id;

    INSERT INTO af_collab_member (uid, oid, permission_id)
    SELECT p_uid, p_oid, rp.permission_id
    FROM af_role_permissions rp
    JOIN af_roles ON rp.role_id = af_roles.id
    WHERE af_roles.name = 'Owner'
    ON CONFLICT (uid, oid)
    DO UPDATE SET permission_id = excluded.permission_id;
END
$$;

CREATE TYPE af_fragment AS (fragment_id TEXT, content_type INT, contents TEXT, embedding VECTOR(1536));

CREATE OR REPLACE PROCEDURE af_collab_embeddings_upsert(
    IN p_workspace_id UUID,
    IN p_oid TEXT,
    IN p_partition_key INT,
    IN p_tokens_used INT,
    IN p_fragments af_fragment[]
)
LANGUAGE plpgsql
AS $$
BEGIN
    DELETE FROM af_collab_embeddings WHERE oid = p_oid;

    INSERT INTO af_collab_embeddings (fragment_id, oid, partition_key, content_type, content, embedding, indexed_at)
    SELECT f.fragment_id, p_oid, p_partition_key, f.content_type, f.contents, f.embedding, NOW()
    FROM UNNEST(p_fragments) as f;

    INSERT INTO af_workspace_ai_usage(created_at, workspace_id, search_requests, search_tokens_consumed, index_tokens_consumed)
    VALUES (now()::date, p_workspace_id, 0, 0, p_tokens_used)
    ON CONFLICT (created_at, workspace_id)
    DO UPDATE SET index_tokens_consumed = af_workspace_ai_usage.index_tokens_consumed + p_tokens_used;
END
$$;