CREATE TABLE IF NOT EXISTS af_file_metadata (
    owner_uid BIGINT REFERENCES af_user(uid) ON DELETE CASCADE NOT NULL,
    path VARCHAR NOT NULL,
    file_type VARCHAR NOT NULL,
    file_size BIGINT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP NOT NULL,
    UNIQUE (owner_uid, path)
);
