CREATE TABLE card_reviews (
    card_id UUID PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'none'
        CHECK (status IN ('none', 'requested', 'approved', 'changes_requested')),
    updated_by UUID REFERENCES users(id) ON DELETE SET NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE card_reviewers (
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (card_id, user_id)
);

CREATE INDEX card_reviewers_user_id_idx ON card_reviewers(user_id);
