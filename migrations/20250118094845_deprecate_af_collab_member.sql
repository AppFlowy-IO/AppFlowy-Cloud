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
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS af_collab_member_change_trigger ON af_collab_member;
DROP FUNCTION IF EXISTS notify_af_collab_member_change;
