-- Required by uuid_generate_v4()
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";
-- user table
CREATE TABLE IF NOT EXISTS af_user (
    uid BIGSERIAL PRIMARY KEY,
    email TEXT NOT NULL DEFAULT '' UNIQUE,
    password TEXT NOT NULL,
    name TEXT NOT NULL DEFAULT '',
    encryption_sign TEXT,
    deleted_at TIMESTAMP WITH TIME ZONE DEFAULT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);
CREATE OR REPLACE FUNCTION update_updated_at_column_func() RETURNS TRIGGER AS $$ BEGIN NEW.updated_at = NOW();
RETURN NEW;
END;
$$ language 'plpgsql';
CREATE TRIGGER update_af_user_modtime BEFORE
UPDATE ON af_user FOR EACH ROW EXECUTE PROCEDURE update_updated_at_column_func();
CREATE OR REPLACE FUNCTION prevent_reset_encryption_sign_func() RETURNS TRIGGER AS $$ BEGIN IF OLD.encryption_sign IS NOT NULL
    AND NEW.encryption_sign IS DISTINCT
FROM OLD.encryption_sign THEN RAISE EXCEPTION 'The encryption sign can not be reset once it has been set';
END IF;
RETURN NEW;
END;
$$ LANGUAGE plpgsql;
CREATE TRIGGER trigger_prevent_reset_encryption_sign BEFORE
UPDATE ON af_user FOR EACH ROW EXECUTE FUNCTION prevent_reset_encryption_sign_func();
-- Create the anon and authenticated roles if they don't exist
CREATE OR REPLACE FUNCTION create_roles(roles text []) RETURNS void LANGUAGE plpgsql AS $$
DECLARE role_name text;
BEGIN FOREACH role_name IN ARRAY roles LOOP IF NOT EXISTS (
    SELECT 1
    FROM pg_roles
    WHERE rolname = role_name
) THEN EXECUTE 'CREATE ROLE ' || role_name;
END IF;
END LOOP;
END;
$$;
SELECT create_roles(ARRAY ['anon', 'authenticated']);
-- auth schema
CREATE SCHEMA IF NOT EXISTS auth;
CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS pgjwt;
DO $$ BEGIN IF NOT EXISTS (
    SELECT 1
    FROM pg_catalog.pg_proc p
        JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid
    WHERE p.proname = 'jwt'
        AND n.nspname = 'auth'
) THEN EXECUTE '
        CREATE OR REPLACE FUNCTION auth.jwt()
        RETURNS jsonb
        LANGUAGE sql
        STABLE
        AS $function$
            SELECT
                coalesce(
                    nullif(current_setting(''request.jwt.claim'', true), ''''),
                    nullif(current_setting(''request.jwt.claims'', true), '''')
                )::jsonb
        $function$';
END IF;
END $$;