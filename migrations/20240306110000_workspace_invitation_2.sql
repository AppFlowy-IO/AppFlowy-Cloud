ALTER TABLE af_workspace_invitation ADD COLUMN invitee_email TEXT NOT NULL;

-- Auto add to af_workspace_member upon invitation accepted
CREATE OR REPLACE FUNCTION add_to_af_workspace_member()
RETURNS TRIGGER AS $$
BEGIN
  IF NEW.status = 1 THEN
    -- workspace permission
    INSERT INTO af_workspace_member (workspace_id, uid, role_id)
    VALUES (
      NEW.workspace_id,
      (SELECT uid FROM af_user WHERE email = NEW.invitee_email),
      NEW.role_id
    )
    ON CONFLICT (workspace_id, uid) DO NOTHING;

    -- collab permission
    INSERT INTO af_collab_member (uid, oid, permission_id)
    VALUES (
      (SELECT uid FROM af_user WHERE email = NEW.invitee_email),
      NEW.workspace_id,
      (SELECT permission_id
       FROM public.af_role_permissions
       WHERE public.af_role_permissions.role_id = NEW.role_id)
    )
    ON CONFLICT (uid, oid)
    DO UPDATE
      SET permission_id = excluded.permission_id;
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

ALTER TABLE af_workspace_invitation DROP COLUMN invitee;
