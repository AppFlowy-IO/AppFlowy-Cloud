-- Adds the `Admin` role to the `af_roles` lookup table.
--
-- The role hierarchy (lower numeric value in the priority function = higher
-- privilege per spec) is fixed downstream in `libs/database-entity/src/dto.rs`:
--   Owner = 3, Admin = 2, Member = 1, Guest = 0
--
-- The id auto-assigned to 'Admin' by the SERIAL sequence is preserved by the
-- `ON CONFLICT DO NOTHING` so this migration is idempotent.

INSERT INTO af_roles (name) VALUES ('Admin')
ON CONFLICT (name) DO NOTHING;
