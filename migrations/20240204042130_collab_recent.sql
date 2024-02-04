-- Add migration script here
CREATE TABLE IF NOT EXISTS af_collab_recent_access (
    recent_oid PRIMARY KEY TEXT NOT NULL,
    recent_partition_key INTEGER NOT NULL,
    access_time TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (recent_oid, recent_partition_key) REFERENCES af_collab (oid, partition_key) ON DELETE CASCADE
 );
