-- A review is a consensus workflow: every assigned reviewer records a
-- decision, while card_reviews stores only the aggregate result.
ALTER TABLE card_reviews
    DROP CONSTRAINT IF EXISTS card_reviews_status_check;

ALTER TABLE card_reviews
    ADD CONSTRAINT card_reviews_status_check
    CHECK (status IN ('none', 'requested', 'approved', 'changes_requested', 'rejected'));

ALTER TABLE card_reviews
    ADD COLUMN IF NOT EXISTS requested_by UUID REFERENCES users(id) ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS card_review_decisions (
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    reviewer_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('approved', 'changes_requested', 'rejected')),
    reason TEXT NOT NULL DEFAULT '',
    decided_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (card_id, reviewer_id)
);

CREATE INDEX IF NOT EXISTS card_review_decisions_card_id_idx
    ON card_review_decisions (card_id);
