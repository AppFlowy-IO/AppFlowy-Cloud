CREATE OR REPLACE FUNCTION af_workspace_insert_trigger()
RETURNS TRIGGER AS $$
BEGIN
    -- Insert a record into af_workspace_member
    INSERT INTO public.af_workspace_member (
	uid, role_id,
	workspace_id, created_at, updated_at)
    VALUES (
	NEW.owner_uid, (SELECT id FROM public.af_roles WHERE name = 'Owner'),
	NEW.workspace_id, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP);

    -- Return the new record to complete the insert operation
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER af_workspace_after_insert
AFTER INSERT ON public.af_workspace
FOR EACH ROW
EXECUTE FUNCTION af_workspace_insert_trigger();
