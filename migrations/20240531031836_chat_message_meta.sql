-- Add migration script here
ALTER TABLE af_chat
    ADD COLUMN meta_data JSONB DEFAULT '{}' NOT NULL;

ALTER TABLE af_chat_messages
    ADD COLUMN meta_data JSONB DEFAULT '{}' NOT NULL,
    ADD COLUMN reply_message_id BIGINT;
