ALTER TABLE attachments
    ALTER COLUMN object_key DROP NOT NULL,
    ADD COLUMN external_url TEXT;
