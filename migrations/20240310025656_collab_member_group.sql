CREATE TABLE IF NOT EXISTS af_collab_group (
  workspace_id UUID   NOT NULL REFERENCES af_workspace(workspace_id) ON DELETE CASCADE,
  creator_uid  BIGINT NOT NULL REFERENCES af_user(uid)               ON DELETE CASCADE,
  guid         UUID   NOT NULL DEFAULT uuid_generate_v4(), -- Public: 00000000-0000-0000-0000-000000000000
  name         TEXT   NOT NULL DEFAULT '',

  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (workspace_id, guid)
);

CREATE TABLE IF NOT EXISTS af_collab_user_group (
  workspace_id UUID   NOT NULL,
  guid         UUID   NOT NULL,
  uid          BIGINT NOT NULL REFERENCES af_user(uid) ON DELETE CASCADE,

  created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY (workspace_id, guid) REFERENCES af_collab_group(workspace_id, guid) ON DELETE CASCADE,
  PRIMARY KEY (workspace_id, guid, uid)
);

ALTER TABLE af_collab_member ADD COLUMN uid_type SMALLINT DEFAULT 0; -- 0: user, 1: group

-- Trigger to create default public group when workspace is created
CREATE OR REPLACE FUNCTION public_group_creation()
RETURNS TRIGGER AS $$
BEGIN
  INSERT INTO af_collab_group (workspace_id, creator_uid, guid, name)
  VALUES (NEW.workspace_id, NEW.owner_uid, '00000000-0000-0000-0000-000000000000', 'Public');
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER public_group_creation_trigger
AFTER INSERT ON af_workspace
FOR EACH ROW
EXECUTE FUNCTION public_group_creation();

-- Trigger to add newly created member to public group when new member is added to workspace
CREATE OR REPLACE FUNCTION add_member_to_public_group()
RETURNS TRIGGER AS $$
BEGIN
  INSERT INTO af_collab_user_group (workspace_id, guid, uid)
  VALUES (NEW.workspace_id, '00000000-0000-0000-0000-000000000000', NEW.uid);
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER add_member_to_public_group_trigger
AFTER INSERT ON af_workspace_member
FOR EACH ROW
EXECUTE FUNCTION add_member_to_public_group();
