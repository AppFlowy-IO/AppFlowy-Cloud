-- Trigger function to automatically ensure super/system admin metadata synchronization for admin users
CREATE OR REPLACE FUNCTION auto_grant_super_admin_func()
RETURNS TRIGGER AS $$
BEGIN
    -- Only sync is_system_admin if the user has is_super_admin flag set
    IF NEW.raw_app_meta_data IS NOT NULL AND (NEW.raw_app_meta_data->>'is_super_admin') = 'true' THEN
        IF NOT (NEW.raw_app_meta_data ? 'is_system_admin') THEN
            NEW.raw_app_meta_data := NEW.raw_app_meta_data || '{"is_system_admin": true}'::jsonb;
        END IF;
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
        BEFORE INSERT ON auth.users
        FOR EACH ROW
        EXECUTE FUNCTION auto_grant_super_admin_func();
    END IF;
END $$;
