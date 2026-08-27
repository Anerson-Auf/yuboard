-- Idempotency ledger for collaborative diagram operations. The current
-- document remains the durable snapshot; this table only prevents a retry
-- from applying the same client operation twice.
CREATE TABLE card_diagram_operations (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX card_diagram_operations_card_created_idx
    ON card_diagram_operations (card_id, created_at DESC);
