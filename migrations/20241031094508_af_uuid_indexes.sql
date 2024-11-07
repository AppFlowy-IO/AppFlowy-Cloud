CREATE UNIQUE INDEX IF NOT EXISTS uq_af_user_uuid
    ON af_user (uuid)
    INCLUDE (uid);

