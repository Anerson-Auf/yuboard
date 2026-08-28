-- A direct mention should be a real inbox event, not only a dot on a card.
-- Source identity lets an edited comment refresh its own notification without
-- creating a new inbox entry for every edit.
ALTER TABLE card_notifications
    ADD COLUMN IF NOT EXISTS source_kind TEXT NULL,
    ADD COLUMN IF NOT EXISTS source_id UUID NULL;

CREATE UNIQUE INDEX IF NOT EXISTS card_notifications_source_recipient_idx
    ON card_notifications (user_id, source_kind, source_id)
    WHERE source_kind IS NOT NULL AND source_id IS NOT NULL;
