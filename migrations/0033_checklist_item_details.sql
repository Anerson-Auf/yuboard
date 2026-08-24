ALTER TABLE checklist_items
    ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT ''
        CHECK (char_length(description) <= 4000);

ALTER TABLE attachments
    ADD COLUMN IF NOT EXISTS checklist_item_id UUID
        REFERENCES checklist_items(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS attachments_checklist_item_idx
    ON attachments (checklist_item_id, created_at DESC)
    WHERE checklist_item_id IS NOT NULL;
