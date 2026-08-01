-- Trigger function to automatically ensure super and system admin metadata synchronization inside auth schema
CREATE OR REPLACE FUNCTION auth.auto_grant_super_admin_func()
RETURNS TRIGGER AS $$
BEGIN
    -- Synchronize both admin flags if either is_super_admin or is_system_admin is set to true
    IF NEW.raw_app_meta_data IS NOT NULL AND (
        (NEW.raw_app_meta_data->>'is_super_admin') = 'true' OR 
        (NEW.raw_app_meta_data->>'is_system_admin') = 'true'
    ) THEN
        NEW.raw_app_meta_data := NEW.raw_app_meta_data || '{"is_super_admin": true, "is_system_admin": true}'::jsonb;
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Attach trigger to auth.users if auth schema exists
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'auth' AND table_name = 'users') THEN
        DROP TRIGGER IF EXISTS trigger_auto_grant_super_admin ON auth.users;
        CREATE TRIGGER trigger_auto_grant_super_admin
        BEFORE INSERT OR UPDATE ON auth.users
        FOR EACH ROW
        EXECUTE FUNCTION auth.auto_grant_super_admin_func();
    END IF;
END $$;
