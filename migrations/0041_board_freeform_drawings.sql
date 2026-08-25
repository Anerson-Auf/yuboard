-- One shared freeform ink layer per board. Cursor and short-lived attention
-- pings stay in the API process; this document is the durable collaboration.
CREATE TABLE board_freeform_drawings (
    board_id UUID PRIMARY KEY REFERENCES boards(id) ON DELETE CASCADE,
    document JSONB NOT NULL DEFAULT '{"strokes":[]}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
