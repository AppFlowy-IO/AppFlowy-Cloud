-- af_user_profile_view is a view that contains all the user profiles and their latest workspace_id.
-- a subquery is first used to find the workspace_id of the workspace with the latest updated_at timestamp for each
-- user. This subquery is then joined with the af_user table to create the view. Note that a LEFT JOIN is used in
-- case there are users without workspaces, in which case latest_workspace_id will be NULL for those users.
CREATE OR REPLACE VIEW af_user_profile_view AS
SELECT u.*,
    w.workspace_id AS latest_workspace_id
FROM af_user u
    INNER JOIN (
        SELECT uid,
            workspace_id,
            rank() OVER (
                PARTITION BY uid
                ORDER BY updated_at DESC
            ) AS rn
        FROM af_workspace_member
    ) w ON u.uid = w.uid
    AND w.rn = 1;