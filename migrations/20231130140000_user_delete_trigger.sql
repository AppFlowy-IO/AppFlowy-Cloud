GRANT SELECT, INSERT, UPDATE, DELETE ON public.af_user TO supabase_auth_admin;

-- Trigger Function to delete a user from the pulic.af_user table
-- when a user is deleted from auth.users table (with matching uuid) field
CREATE OR REPLACE FUNCTION public.delete_user()
RETURNS TRIGGER AS $$
BEGIN
    DELETE FROM public.af_user WHERE uuid = OLD.id;
    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER delete_user_trigger
AFTER DELETE ON auth.users
FOR EACH ROW EXECUTE FUNCTION public.delete_user();

-- Trigger Function to update the 'deleted_at' field in the pulic.af_user table
-- (Soft Delete)
CREATE OR REPLACE FUNCTION public.update_af_user_deleted_at()
RETURNS TRIGGER AS $$
BEGIN
    -- Check if 'deleted_at' field is modified
    IF OLD.deleted_at IS DISTINCT FROM NEW.deleted_at THEN
        -- Update 'deleted_at' in public.af_user
        UPDATE public.af_user
        SET deleted_at = NEW.deleted_at
        WHERE uuid = NEW.id;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_af_user_deleted_at_trigger
AFTER UPDATE OF deleted_at ON auth.users
FOR EACH ROW EXECUTE FUNCTION public.update_af_user_deleted_at();
