-- Add migration script here
CREATE EXTENSION IF NOT EXISTS vector;

-- create table to store collab embeddings
CREATE TABLE IF NOT EXISTS af_collab_embeddings
(
    fragment_id TEXT NOT NULL PRIMARY KEY,
    oid TEXT NOT NULL,
    partition_key INTEGER NOT NULL,
    content TEXT,
    embedding VECTOR(1536),
    FOREIGN KEY (oid, partition_key) REFERENCES af_collab (oid, partition_key) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS af_collab_embeddings_similarity_idx ON af_collab_embeddings USING hnsw (embedding vector_cosine_ops);