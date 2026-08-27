-- Durable, cursor-based user-action feed for a Discord bridge. Events are not
-- deleted when read: consumers advance their own cursor only after successful
-- processing, which gives the integration at-least-once delivery semantics.
CREATE TABLE discord_outbox_events (
    id BIGSERIAL PRIMARY KEY,
    integration_id UUID NOT NULL REFERENCES discord_integrations(id) ON DELETE CASCADE,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL CHECK (char_length(event_type) BETWEEN 1 AND 80),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX discord_outbox_events_pull_idx
    ON discord_outbox_events (integration_id, id);
