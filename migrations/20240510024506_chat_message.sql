-- Add migration script here
-- Create table for chat documents
CREATE TABLE af_chat
(
    chat_id      UUID PRIMARY KEY,
    created_at   TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at   TIMESTAMP WITH TIME ZONE          DEFAULT NULL,
    name         TEXT                     NOT NULL DEFAULT '',
    rag_ids      JSONB                    NOT NULL DEFAULT '[]',
    workspace_id UUID                     NOT NULL,
    FOREIGN KEY (workspace_id) REFERENCES af_workspace (workspace_id) ON DELETE CASCADE
);

-- Create table for chat messages
CREATE TABLE af_chat_messages
(
    message_id BIGSERIAL PRIMARY KEY,
    author     JSONB                    NOT NULL,
    chat_id    UUID                     NOT NULL,
    content    TEXT                     NOT NULL,
    deleted_at TIMESTAMP WITH TIME ZONE          DEFAULT NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    edited_at  TIMESTAMP                         DEFAULT NULL,
    FOREIGN KEY (chat_id) REFERENCES af_chat (chat_id) ON DELETE CASCADE
);

CREATE INDEX idx_chat_messages_chat_id_created_at ON af_chat_messages (message_id ASC, created_at ASC);
