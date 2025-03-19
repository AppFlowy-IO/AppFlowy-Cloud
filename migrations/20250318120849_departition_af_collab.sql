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

-- copy data from all collab partitions to new collab table
insert into af_collab_temp(oid, workspace_id, owner_uid, partition_key, len, blob, deleted_at, created_at, updated_at, indexed_at)
select oid::uuid as oid, workspace_id, owner_uid, partition_key, len, blob, deleted_at, created_at, updated_at, indexed_at
from af_collab;

-- modify embeddings to make use of new uuid columns
alter table af_collab_embeddings
    add column object_id uuid null;
update af_collab_embeddings set object_id = oid::uuid;
alter table af_collab_embeddings
    alter column object_id set not null;
alter table af_collab_embeddings
drop column oid,
    drop column partition_key;

-- replace af_collab table
drop table af_collab;
alter table af_collab_temp rename to af_collab;

-- rebind embeddings foreign key to new af_collab table
alter table af_collab_embeddings
    add constraint fk_af_collab_embeddings_af_collab
        foreign key (object_id) references af_collab(oid);
create index ix_af_collab_embeddings_oid on af_collab_embeddings(object_id);

-- add trigger for af_collab.updated_at
create trigger set_updated_at
    before insert or update
                         on af_collab
                         for each row
                         execute procedure update_updated_at_column();

-- add remaining indexes to new af_collab table
create index if not exists idx_workspace_id_on_af_collab
    on af_collab (workspace_id);

create index if not exists idx_af_collab_updated_at
    on af_collab (updated_at);