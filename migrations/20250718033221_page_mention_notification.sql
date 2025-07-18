ALTER TABLE af_page_mention
  ADD COLUMN IF NOT EXISTS require_notification BOOLEAN DEFAULT FALSE;
