ALTER TABLE cards ADD COLUMN start_at TIMESTAMPTZ;

CREATE INDEX cards_board_start_idx
    ON cards (board_id, start_at)
    WHERE archived_at IS NULL AND start_at IS NOT NULL;

CREATE TABLE card_relations (
    id UUID PRIMARY KEY,
    source_card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    target_card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL CHECK (relation_type IN ('blocks', 'depends_on', 'duplicate', 'related')),
    created_by UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (source_card_id <> target_card_id),
    UNIQUE (source_card_id, target_card_id, relation_type)
);

CREATE INDEX card_relations_target_active_idx
    ON card_relations (target_card_id, relation_type);
CREATE INDEX card_relations_source_active_idx
    ON card_relations (source_card_id, relation_type);

CREATE TABLE card_description_versions (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    description TEXT NOT NULL CHECK (char_length(description) <= 20000),
    created_by UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX card_description_versions_card_created_idx
    ON card_description_versions (card_id, created_at DESC, id DESC);
