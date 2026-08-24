-- A board can have several Discord integrations, therefore thread mappings are
-- scoped to the integration instead of being stored directly on cards.
CREATE TABLE discord_card_threads (
    integration_id UUID NOT NULL REFERENCES discord_integrations(id) ON DELETE CASCADE,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    thread_id TEXT NOT NULL CHECK (char_length(thread_id) BETWEEN 1 AND 128),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (integration_id, card_id),
    UNIQUE (integration_id, thread_id)
);

CREATE INDEX discord_card_threads_card_idx ON discord_card_threads (card_id);

-- The bot polls this durable cursor to mirror archive/restore state into
-- Discord threads. BIGSERIAL gives an unambiguous, monotonic sync cursor.
CREATE TABLE discord_card_thread_events (
    id BIGSERIAL PRIMARY KEY,
    integration_id UUID NOT NULL REFERENCES discord_integrations(id) ON DELETE CASCADE,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL CHECK (event_kind IN ('thread_linked', 'archived', 'restored')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX discord_card_thread_events_poll_idx ON discord_card_thread_events (integration_id, id);
