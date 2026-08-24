CREATE TABLE card_activity (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL CHECK (char_length(action) BETWEEN 1 AND 120),
    detail TEXT NOT NULL DEFAULT '' CHECK (char_length(detail) <= 1000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX card_activity_card_created_idx ON card_activity (card_id, created_at DESC);
