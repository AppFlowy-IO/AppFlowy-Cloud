-- Drop the existing unique constraint on publish_name
ALTER TABLE public.af_published_collab
DROP CONSTRAINT unique_publish_name;

-- Add a new unique constraint for the combination of publish_name and workspace_id
ALTER TABLE public.af_published_collab
ADD CONSTRAINT unique_workspace_id_publish_name UNIQUE (workspace_id, publish_name);
