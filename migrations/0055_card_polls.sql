CREATE TABLE card_polls (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    question TEXT NOT NULL,
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE card_poll_options (
    id UUID PRIMARY KEY,
    poll_id UUID NOT NULL REFERENCES card_polls(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    position INTEGER NOT NULL,
    UNIQUE (poll_id, position)
);

CREATE TABLE card_poll_votes (
    poll_id UUID NOT NULL REFERENCES card_polls(id) ON DELETE CASCADE,
    option_id UUID NOT NULL REFERENCES card_poll_options(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (poll_id, user_id)
);

CREATE INDEX card_polls_card_created_idx ON card_polls (card_id, created_at DESC);
CREATE INDEX card_poll_votes_option_idx ON card_poll_votes (option_id);
