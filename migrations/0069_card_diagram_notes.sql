-- Threaded point notes for a collaborative card diagram. They are deliberately
-- separate from card comments: diagram discussion must not be forwarded to a
-- Discord thread and must retain its coordinate on the canvas.
CREATE TABLE card_diagram_notes (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    x INTEGER NOT NULL CHECK (x >= 0 AND x <= 20000),
    y INTEGER NOT NULL CHECK (y >= 0 AND y <= 20000),
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX card_diagram_notes_card_created_idx
    ON card_diagram_notes (card_id, created_at ASC);

CREATE TABLE card_diagram_note_comments (
    id UUID PRIMARY KEY,
    note_id UUID NOT NULL REFERENCES card_diagram_notes(id) ON DELETE CASCADE,
    author_id UUID REFERENCES users(id) ON DELETE SET NULL,
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 4000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX card_diagram_note_comments_note_created_idx
    ON card_diagram_note_comments (note_id, created_at ASC);
