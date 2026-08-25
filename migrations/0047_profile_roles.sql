CREATE TABLE IF NOT EXISTS profile_roles (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE CHECK (char_length(trim(name)) BETWEEN 1 AND 80),
    color TEXT NOT NULL DEFAULT '#6ea8fe' CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
    icon_shape TEXT NOT NULL DEFAULT 'circle' CHECK (icon_shape IN ('circle', 'square', 'diamond', 'star', 'triangle', 'hexagon', 'bolt', 'flag')),
    created_by UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS user_profile_roles (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES profile_roles(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role_id)
);

CREATE TABLE IF NOT EXISTS card_profile_roles (
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    role_id UUID NOT NULL REFERENCES profile_roles(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (card_id, role_id)
);

CREATE INDEX IF NOT EXISTS user_profile_roles_role_id_idx ON user_profile_roles(role_id, user_id);
CREATE INDEX IF NOT EXISTS card_profile_roles_role_id_idx ON card_profile_roles(role_id, card_id);

ALTER TABLE labels
    ADD COLUMN IF NOT EXISTS icon_shape TEXT NOT NULL DEFAULT 'circle'
    CHECK (icon_shape IN ('circle', 'square', 'diamond', 'star', 'triangle', 'hexagon', 'bolt', 'flag'));
