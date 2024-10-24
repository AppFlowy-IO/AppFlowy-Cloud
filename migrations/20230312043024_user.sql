-- Add migration script here
-- Required by uuid_generate_v4()
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
-- user table
CREATE TABLE IF NOT EXISTS af_user (
    uid BIGINT PRIMARY KEY,
    uuid UUID NOT NULL , -- related to gotrue
    email TEXT NOT NULL DEFAULT '' UNIQUE, -- not needed when authenticated with gotrue
    password TEXT NOT NULL DEFAULT '', -- not needed when authenticated with gotrue
    name TEXT NOT NULL DEFAULT '',
    metadata JSONB DEFAULT '{}'::JSONB,  -- used to user's metadata such as avatar, OpenAI key, etc.
    encryption_sign TEXT DEFAULT NULL, -- used to encrypt the user's data
    deleted_at TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);
CREATE OR REPLACE FUNCTION update_updated_at_column_func() RETURNS TRIGGER AS $$ BEGIN NEW.updated_at = NOW();
RETURN NEW;
END;
$$ language 'plpgsql';
CREATE OR REPLACE TRIGGER update_af_user_modtime BEFORE
UPDATE ON af_user FOR EACH ROW EXECUTE PROCEDURE update_updated_at_column_func();
CREATE OR REPLACE FUNCTION prevent_reset_encryption_sign_func() RETURNS TRIGGER AS $$ BEGIN IF OLD.encryption_sign IS NOT NULL
    AND NEW.encryption_sign IS DISTINCT
FROM OLD.encryption_sign THEN RAISE EXCEPTION 'The encryption sign can not be reset once it has been set';
END IF;
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE OR REPLACE TRIGGER trigger_prevent_reset_encryption_sign BEFORE
UPDATE ON af_user FOR EACH ROW EXECUTE FUNCTION prevent_reset_encryption_sign_func();
