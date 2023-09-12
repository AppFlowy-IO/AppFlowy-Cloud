-- Enable RLS on the af_collab table
ALTER TABLE af_collab ENABLE ROW LEVEL SECURITY;
CREATE POLICY af_collab_policy ON af_collab FOR ALL TO anon,
authenticated USING (true);
-- Enable RLS on the af_user table
-- Policy for INSERT
ALTER TABLE af_user ENABLE ROW LEVEL SECURITY;
CREATE POLICY af_user_insert_policy ON public.af_user FOR
INSERT TO anon,
    authenticated WITH CHECK (true);
-- Policy for UPDATE
CREATE POLICY af_user_update_policy ON public.af_user FOR
UPDATE USING (true) WITH CHECK (true);
-- Policy for SELECT
CREATE POLICY af_user_select_policy ON public.af_user FOR
SELECT TO anon,
    authenticated USING (true);
ALTER TABLE af_user FORCE ROW LEVEL SECURITY;
-- Enable RLS on the af_workspace_member table
ALTER TABLE af_workspace_member ENABLE ROW LEVEL SECURITY;
CREATE POLICY af_workspace_member_policy ON af_workspace_member FOR ALL TO anon,
authenticated USING (true);
ALTER TABLE af_workspace_member FORCE ROW LEVEL SECURITY;
-- Enable RLS on the af_workspace table
ALTER TABLE af_workspace ENABLE ROW LEVEL SECURITY;
CREATE POLICY af_workspace_policy ON af_workspace FOR ALL TO anon,
authenticated USING (true);
ALTER TABLE af_workspace FORCE ROW LEVEL SECURITY;