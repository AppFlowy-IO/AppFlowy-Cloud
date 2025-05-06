-- Add foreign key constraint to public.af_user table
-- Edited ON 2025-05-04 in order to prevent supbase_auth_admin role from being mandatory.
-- Rewritten with exception handling such that existing database won't throw an error WHEN
-- this migration is re-applied. Not dropping and recreating the constraint as it is potentially
-- expensive with large number of users.
DO $$
BEGIN
  BEGIN
    ALTER TABLE public.af_user ADD CONSTRAINT af_user_email_foreign_key FOREIGN KEY (uuid) REFERENCES auth.users (id) ON DELETE CASCADE;
  EXCEPTION
    WHEN duplicate_object THEN
      NULL;
    WHEN OTHERS THEN
      RAISE;
  END;
END $$;
