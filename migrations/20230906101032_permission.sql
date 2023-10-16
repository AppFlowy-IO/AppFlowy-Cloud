-- Create the af_roles table
CREATE TABLE IF NOT EXISTS af_roles (
    id SERIAL PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
);
-- Insert default roles
INSERT INTO af_roles (name)
VALUES ('Owner'),
    ('Member'),
    ('Guest');
CREATE TABLE af_permissions (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) UNIQUE NOT NULL,
    access_level INTEGER NOT NULL,
    description TEXT
);
-- Insert default permissions
INSERT INTO af_permissions (name, description, access_level)
VALUES ('Read only', 'Can read', 10),
    (
        'Read and comment',
        'Can read and comment, but not edit',
        20
    ),
    (
        'Read and write',
        'Can read and edit, but not share with others',
        30
    ),
    (
        'Full access',
        'Can edit and share with others',
        50
    );
-- Represents a permission that a role has. The list of all permissions a role has can be obtained by querying this table for all rows with a given role_id.
CREATE TABLE af_role_permissions (
    role_id INT REFERENCES af_roles(id),
    permission_id INT REFERENCES af_permissions(id),
    PRIMARY KEY (role_id, permission_id)
);
-- Associate permissions with roles
WITH role_ids AS (
    SELECT id,
        name
    FROM af_roles
    WHERE name IN ('Owner', 'Member', 'Guest')
),
permission_ids AS (
    SELECT id,
        name
    FROM af_permissions
    WHERE name IN ('Full access', 'Read and write', 'Read only')
)
INSERT INTO af_role_permissions (role_id, permission_id)
SELECT r.id,
    p.id
FROM role_ids r
    CROSS JOIN permission_ids p
WHERE (
        r.name = 'Owner'
        AND p.name = 'Full access'
    )
    OR (
        r.name = 'Member'
        AND p.name = 'Read and write'
    )
    OR (
        r.name = 'Guest'
        AND p.name = 'Read only'
    );