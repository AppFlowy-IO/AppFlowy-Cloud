-- Add a unique constraint on publish_name
ALTER TABLE public.af_published_collab
ADD CONSTRAINT unique_publish_name UNIQUE (publish_name);

-- Add an index on publish_name
CREATE INDEX idx_publish_name ON public.af_published_collab (publish_name);
