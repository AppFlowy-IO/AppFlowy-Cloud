-- Add migration script here
CREATE OR REPLACE FUNCTION notify_af_user_change() RETURNS TRIGGER AS $$
DECLARE
    payload TEXT;
BEGIN
    payload := json_build_object(
            'payload', row_to_json(NEW),
            'action_type', TG_OP
            )::text;

    PERFORM pg_notify('af_profile_channel', payload);
RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS af_user_change_trigger ON af_user;
CREATE TRIGGER af_user_change_trigger
    AFTER UPDATE ON af_user
    FOR EACH ROW
    EXECUTE FUNCTION notify_af_user_change();
