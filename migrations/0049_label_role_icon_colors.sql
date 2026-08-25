ALTER TABLE labels
    ADD COLUMN IF NOT EXISTS icon_color TEXT NOT NULL DEFAULT '#FFFFFF'
    CHECK (icon_color ~ '^#[0-9A-Fa-f]{6}$');

ALTER TABLE profile_roles
    ADD COLUMN IF NOT EXISTS icon_color TEXT NOT NULL DEFAULT '#FFFFFF'
    CHECK (icon_color ~ '^#[0-9A-Fa-f]{6}$');
