-- Add migration script here
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

-- Create supabase_admin user if it does not exist
DO $$ BEGIN IF NOT EXISTS (
    SELECT
    FROM pg_catalog.pg_roles
    WHERE rolname = 'supabase_admin'
) THEN CREATE USER supabase_admin LOGIN CREATEROLE CREATEDB REPLICATION BYPASSRLS;
END IF;
END $$;
-- Create supabase_auth_admin user if it does not exist
DO $$ BEGIN IF NOT EXISTS (
    SELECT
    FROM pg_catalog.pg_roles
    WHERE rolname = 'supabase_auth_admin'
) THEN CREATE USER supabase_auth_admin BYPASSRLS NOINHERIT CREATEROLE LOGIN NOREPLICATION PASSWORD 'root';
END IF;
END $$;
-- Create auth schema if it does not exist
CREATE SCHEMA IF NOT EXISTS auth AUTHORIZATION supabase_auth_admin;
-- Grant permissions
GRANT CREATE ON DATABASE postgres TO supabase_auth_admin;
-- Set search_path for supabase_auth_admin
ALTER USER supabase_auth_admin SET search_path = 'auth';
