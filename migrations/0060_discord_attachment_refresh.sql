-- Discord CDN URLs expire. Keep source identifiers so Flowboard can ask the
-- Discord bridge for a fresh signed URL without storing the media file.
ALTER TABLE attachments
    ADD COLUMN discord_channel_id TEXT,
    ADD COLUMN discord_message_id TEXT,
    ADD COLUMN discord_attachment_id TEXT;

CREATE INDEX attachments_discord_source_idx
    ON attachments (discord_channel_id, discord_message_id, discord_attachment_id)
    WHERE discord_attachment_id IS NOT NULL;
