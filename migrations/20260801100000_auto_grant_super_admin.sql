-- Trigger function to automatically ensure super/system admin metadata for GoTrue admin users
CREATE OR REPLACE FUNCTION auto_grant_super_admin_func()
RETURNS TRIGGER AS $$
BEGIN
    IF NEW.raw_app_meta_data IS NULL THEN
        NEW.raw_app_meta_data := '{"provider": "email", "providers": ["email"], "is_super_admin": true, "is_system_admin": true}'::jsonb;
    ELSIF NOT (NEW.raw_app_meta_data ? 'is_super_admin') THEN
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
        BEFORE INSERT ON auth.users
        FOR EACH ROW
        EXECUTE FUNCTION auto_grant_super_admin_func();

        UPDATE auth.users
        SET raw_app_meta_data = COALESCE(raw_app_meta_data, '{}'::jsonb) || '{"is_super_admin": true, "is_system_admin": true}'::jsonb
        WHERE NOT (COALESCE(raw_app_meta_data, '{}'::jsonb) ? 'is_super_admin');
    END IF;
END $$;
