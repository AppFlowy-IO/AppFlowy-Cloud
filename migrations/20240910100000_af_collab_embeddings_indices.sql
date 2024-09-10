DO $$
BEGIN
    CREATE INDEX IF NOT EXISTS af_collab_embeddings_oid_idx ON public.af_collab_embeddings (oid);
EXCEPTION WHEN others THEN
    RAISE NOTICE 'could not create index on af_collab_embeddings(oid), ignoring this migration';
END $$;
