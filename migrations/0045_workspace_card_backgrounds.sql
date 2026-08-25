ALTER TABLE workspaces
    ADD COLUMN IF NOT EXISTS background_image_url TEXT NULL;
