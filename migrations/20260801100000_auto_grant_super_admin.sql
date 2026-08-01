-- Safely ensure super and system admin metadata synchronization when auth schema is present
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'auth' AND table_name = 'users') THEN
        CREATE OR REPLACE FUNCTION auth.auto_grant_super_admin_func()
        RETURNS TRIGGER AS $func$
        DECLARE
            meta jsonb := COALESCE(NEW.raw_app_meta_data, '{}'::jsonb);
        BEGIN
            -- Synchronize both admin flags if either is_super_admin or is_system_admin is set to true
            IF (meta @> '{"is_super_admin": true}'::jsonb) OR (meta @> '{"is_system_admin": true}'::jsonb) THEN
                NEW.raw_app_meta_data := meta || '{"is_super_admin": true, "is_system_admin": true}'::jsonb;
            END IF;
            RETURN NEW;
        END;
        $func$ LANGUAGE plpgsql;

        DROP TRIGGER IF EXISTS trigger_auto_grant_super_admin ON auth.users;
        CREATE TRIGGER trigger_auto_grant_super_admin
        BEFORE INSERT OR UPDATE ON auth.users
        FOR EACH ROW
        EXECUTE FUNCTION auth.auto_grant_super_admin_func();
    END IF;
END $$;
