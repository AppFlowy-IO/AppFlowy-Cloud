-- Add `updated_at` column to `af_collab` table
ALTER TABLE public.af_collab
ADD COLUMN updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP;

-- Create or replace function to update `updated_at` column
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Create trigger to update `updated_at` column
CREATE TRIGGER set_updated_at
BEFORE INSERT OR UPDATE ON public.af_collab
FOR EACH ROW
EXECUTE FUNCTION update_updated_at_column();

-- Create index on `updated_at` column
CREATE INDEX idx_af_collab_updated_at
ON public.af_collab (updated_at);
