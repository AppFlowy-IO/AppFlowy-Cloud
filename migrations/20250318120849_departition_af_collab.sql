set statement_timeout to 3600000;

-- create new uniform collab table
create table af_collab_temp
(
    oid uuid not null primary key,
    workspace_id  uuid not null references public.af_workspace on delete cascade,
    owner_uid bigint not null,
    partition_key integer not null,
    len integer,
    blob bytea not null,
    deleted_at timestamp with time zone,
    created_at timestamp with time zone default CURRENT_TIMESTAMP,
    updated_at timestamp with time zone default CURRENT_TIMESTAMP not null,
    indexed_at timestamp with time zone
);

-- create a table for collabs that have non-UUID object ids
create table af_collab_conflicts
(
    oid text not null primary key,
    workspace_id  uuid not null references public.af_workspace on delete cascade,
    owner_uid bigint not null,
    partition_key integer not null,
    len integer,
    blob bytea not null,
    deleted_at timestamp with time zone,
    created_at timestamp with time zone default CURRENT_TIMESTAMP,
    updated_at timestamp with time zone default CURRENT_TIMESTAMP not null,
    indexed_at timestamp with time zone
);

-- move non-UUID object ids to the new table (execution time: 31 secs)
insert into af_collab_conflicts(oid, workspace_id, owner_uid, partition_key, len, blob, deleted_at, created_at, updated_at, indexed_at)
select oid, workspace_id, owner_uid, partition_key, len, blob, deleted_at, created_at, updated_at, indexed_at
from af_collab
where oid !~ E'^[[:xdigit:]]{8}-([[:xdigit:]]{4}-){3}[[:xdigit:]]{12}$';

-- copy data from all collab partitions to new collab table (execution time: 7 min for 4mln rows)
insert into af_collab_temp(oid, workspace_id, owner_uid, partition_key, len, blob, deleted_at, created_at, updated_at, indexed_at)
select oid::uuid as oid, workspace_id, owner_uid, partition_key, len, blob, deleted_at, created_at, updated_at, indexed_at
from af_collab
where oid ~ E'^[[:xdigit:]]{8}-([[:xdigit:]]{4}-){3}[[:xdigit:]]{12}$';

-- modify embeddings to make use of new uuid columns
alter table af_collab_embeddings
drop constraint af_collab_embeddings_oid_partition_key_fkey;

-- add index of oid (execution time: 5 secs)
create index concurrently if not exists ix_af_collab_embeddings_oid
    on af_collab_embeddings(oid);

-- replace af_collab table
drop table af_collab;
alter table af_collab_temp rename to af_collab;

-- add trigger for af_collab.updated_at
create trigger set_updated_at
    before insert or update
    on af_collab
    for each row
    execute procedure update_updated_at_column();

-- add remaining indexes to new af_collab table (execution time: 25 sec + 25sec)
create index concurrently if not exists idx_workspace_id_on_af_collab
    on af_collab (workspace_id);
create index concurrently if not exists idx_af_collab_updated_at
    on af_collab (updated_at);

create or replace procedure af_collab_embeddings_upsert(IN p_workspace_id uuid, IN p_oid uuid, IN p_tokens_used integer, IN p_fragments af_fragment_v3[])
    language plpgsql
as
$$
BEGIN
DELETE FROM af_collab_embeddings WHERE oid = p_oid;
INSERT INTO af_collab_embeddings (fragment_id, oid, content_type, content, embedding, indexed_at, metadata, fragment_index, embedder_type)
SELECT
    f.fragment_id,
    p_oid,
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