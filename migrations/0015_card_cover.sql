ALTER TABLE cards
    ADD COLUMN cover_attachment_id UUID REFERENCES attachments(id) ON DELETE SET NULL;

CREATE INDEX cards_cover_attachment_idx ON cards (cover_attachment_id) WHERE cover_attachment_id IS NOT NULL;
