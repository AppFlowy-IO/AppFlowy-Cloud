-- Create the trigger function
CREATE OR REPLACE FUNCTION workspace_deleted_trigger_function()
RETURNS trigger AS
$$
DECLARE
    payload jsonb;
BEGIN
    payload := jsonb_build_object(
        'workspace_id', OLD.workspace_id,
        'owner_id', OLD.owner_uid
    );

    PERFORM pg_notify('af_workspace_deleted', payload::text);
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

-- Create the trigger that calls the function after deleting a workspace
CREATE TRIGGER on_workspace_delete
AFTER DELETE ON public.af_workspace
FOR EACH ROW
EXECUTE FUNCTION workspace_deleted_trigger_function();