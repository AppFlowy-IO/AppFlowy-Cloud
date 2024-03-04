CREATE TABLE IF NOT EXISTS af_workspace_invitation (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    workspace_id UUID NOT NULL,
    inviter BIGINT NOT NULL,
    invitee BIGINT NOT NULL,
    role_id INT NOT NULL,
    status SMALLINT NOT NULL DEFAULT 0, -- 0: pending, 1: accepted, 2: rejected

    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL
);
CREATE INDEX idx_af_workspace_invitation_inviter ON af_workspace_invitation (inviter);
CREATE INDEX idx_af_workspace_invitation_invitee ON af_workspace_invitation (invitee);

-- Auto update updated_at column upon status change
CREATE OR REPLACE FUNCTION update_af_workspace_invitation_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    IF OLD.status IS DISTINCT FROM NEW.status THEN
        NEW.updated_at = CURRENT_TIMESTAMP;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER af_workspace_invitation_status_update
BEFORE UPDATE ON af_workspace_invitation
FOR EACH ROW
WHEN (OLD.status IS DISTINCT FROM NEW.status)
EXECUTE FUNCTION update_af_workspace_invitation_updated_at_column();

-- Auto add to af_workspace_member upon invitation accepted
CREATE OR REPLACE FUNCTION add_to_af_workspace_member()
RETURNS TRIGGER AS $$
BEGIN
  IF NEW.status = 1 THEN
    -- workspace permission
    INSERT INTO af_workspace_member (workspace_id, uid, role_id)
    VALUES (NEW.workspace_id, NEW.invitee, NEW.role_id)
    ON CONFLICT (workspace_id, uid) DO NOTHING;

    -- collab permission
    INSERT INTO af_collab_member (uid, oid, permission_id)
    VALUES (
      NEW.invitee,
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

CREATE TRIGGER af_workspace_invitation_accepted
BEFORE UPDATE ON af_workspace_invitation
FOR EACH ROW
WHEN (OLD.status IS DISTINCT FROM NEW.status AND NEW.status = 1)
EXECUTE FUNCTION add_to_af_workspace_member();
