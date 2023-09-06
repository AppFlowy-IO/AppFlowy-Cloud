-- Add migration script here

-- Create supabase_admin user if it does not exist
DO $$ BEGIN
  IF NOT EXISTS (
    SELECT FROM pg_catalog.pg_roles WHERE rolname = 'supabase_admin'
  ) THEN
    CREATE USER supabase_admin LOGIN CREATEROLE CREATEDB REPLICATION BYPASSRLS;
END IF;
END $$;

-- Create supabase_auth_admin user if it does not exist
DO $$ BEGIN
  IF NOT EXISTS (
    SELECT FROM pg_catalog.pg_roles WHERE rolname = 'supabase_auth_admin'
  ) THEN
    CREATE USER supabase_auth_admin NOINHERIT CREATEROLE LOGIN NOREPLICATION PASSWORD 'root';
END IF;
END $$;

-- Create auth schema if it does not exist
CREATE SCHEMA IF NOT EXISTS auth AUTHORIZATION supabase_auth_admin;

-- Grant permissions
GRANT CREATE ON DATABASE postgres TO supabase_auth_admin;

-- Set search_path for supabase_auth_admin
ALTER USER supabase_auth_admin
SET search_path = 'auth';
