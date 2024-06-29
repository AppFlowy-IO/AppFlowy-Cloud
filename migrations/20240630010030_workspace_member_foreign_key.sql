ALTER TABLE public.af_workspace_member
ADD CONSTRAINT af_workspace_member_uid_fkey
FOREIGN KEY (uid) REFERENCES af_user(uid) ON DELETE CASCADE;
