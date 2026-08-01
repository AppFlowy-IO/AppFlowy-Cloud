-- Safely ensure super and system admin metadata synchronization when auth schema is present
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'auth' AND table_name = 'users') THEN
        CREATE OR REPLACE FUNCTION auth.auto_grant_super_admin_func()
        RETURNS TRIGGER AS $func$
        BEGIN
            IF NEW.raw_app_meta_data IS NOT NULL AND (
                (NEW.raw_app_meta_data->>'is_super_admin') = 'true' OR 
                (NEW.raw_app_meta_data->>'is_system_admin') = 'true'
            ) THEN
                NEW.raw_app_meta_data := NEW.raw_app_meta_data || '{"is_super_admin": true, "is_system_admin": true}'::jsonb;
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
