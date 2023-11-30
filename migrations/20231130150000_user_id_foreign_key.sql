-- Revert the last migration
REVOKE SELECT, INSERT, UPDATE, DELETE ON public.af_user FROM supabase_auth_admin;
DROP TRIGGER delete_user_trigger ON auth.users;
DROP TRIGGER update_af_user_deleted_at_trigger ON auth.users;
DROP FUNCTION public.delete_user();
DROP FUNCTION public.update_af_user_deleted_at();

-- Delete all users from public.af_user table that are not in auth.users table
DELETE FROM public.af_user
WHERE NOT EXISTS (
    SELECT 1
    FROM auth.users
    WHERE af_user.uuid = users.id
);

-- Add foreign key constraint to public.af_user table
ALTER TABLE public.af_user
ADD CONSTRAINT af_user_email_foreign_key
FOREIGN KEY (uuid)
REFERENCES auth.users(id)
ON DELETE CASCADE;
