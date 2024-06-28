-- Update existing null values to ensure no nulls are present before adding NOT NULL constraint
UPDATE public.af_workspace
SET publish_namespace = uuid_generate_v4()::text
WHERE publish_namespace IS NULL;

-- Alter the column to set NOT NULL constraint and a default value
ALTER TABLE public.af_workspace
ALTER COLUMN publish_namespace SET NOT NULL,
ALTER COLUMN publish_namespace SET DEFAULT uuid_generate_v4()::text;
