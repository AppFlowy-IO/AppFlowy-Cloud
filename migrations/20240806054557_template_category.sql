CREATE TABLE IF NOT EXISTS af_template_category (
  category_id   UUID NOT NULL DEFAULT gen_random_uuid(),
  created_at    TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at    TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
  name          TEXT NOT NULL,
  icon          TEXT NOT NULL,
  description   TEXT NOT NULL,
  bg_color      TEXT NOT NULL,
  category_type INT NOT NULL,
  priority      INT NOT NULL,

  PRIMARY KEY (category_id)
);
