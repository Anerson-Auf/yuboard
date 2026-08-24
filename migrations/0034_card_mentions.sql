-- A mention belongs to the referenced content, not to a transient browser state.
-- Re-mentioning somebody in the same source resets the unread marker.
CREATE TABLE card_mentions (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('card_description', 'checklist_item_description', 'comment')),
    source_id UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    read_at TIMESTAMPTZ,
    UNIQUE (user_id, source_kind, source_id)
);

CREATE INDEX card_mentions_user_card_unread_idx
    ON card_mentions (user_id, card_id, created_at DESC)
    WHERE read_at IS NULL;
