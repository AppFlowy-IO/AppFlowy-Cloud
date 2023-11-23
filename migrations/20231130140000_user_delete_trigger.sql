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
