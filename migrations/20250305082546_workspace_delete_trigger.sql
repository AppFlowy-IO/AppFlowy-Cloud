-- Edited on 2025-05-04 so that supabase_auth_admin role is not mandatory

CREATE TABLE IF NOT EXISTS af_workspace_deleted (
    workspace_id uuid PRIMARY KEY NOT NULL,
    deleted_at timestamp with time zone DEFAULT now()
);

-- Workspace delete trigger
CREATE OR REPLACE FUNCTION workspace_deleted_trigger_function()
RETURNS trigger AS
$$
DECLARE
    payload jsonb;
BEGIN
    payload := jsonb_build_object(
        'workspace_id', OLD.workspace_id
    );
    INSERT INTO public.af_workspace_deleted (workspace_id, deleted_at)
    VALUES (OLD.workspace_id, now())
    ON CONFLICT DO NOTHING;
    PERFORM pg_notify('af_workspace_deleted', payload::text);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER on_workspace_delete
AFTER DELETE ON public.af_workspace
FOR EACH ROW
EXECUTE FUNCTION workspace_deleted_trigger_function();

-- Delete user trigger
CREATE OR REPLACE FUNCTION notify_user_deletion()
RETURNS TRIGGER AS $$
DECLARE
    payload TEXT;
BEGIN
    payload := jsonb_build_object(
            'user_uuid', OLD.uuid::text
    );

    PERFORM pg_notify('af_user_deleted', payload::text);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE OR REPLACE TRIGGER user_deletion_trigger
AFTER DELETE ON public.af_user
FOR EACH ROW
EXECUTE FUNCTION notify_user_deletion();
