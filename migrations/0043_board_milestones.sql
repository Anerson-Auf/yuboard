CREATE TABLE IF NOT EXISTS milestones (
    id UUID PRIMARY KEY,
    board_id UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(trim(name)) BETWEEN 1 AND 120),
    description TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL DEFAULT '#6ea8fe' CHECK (color ~ '^#[0-9A-Fa-f]{6}$'),
    target_date TIMESTAMPTZ NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (board_id, name)
);

ALTER TABLE cards
    ADD COLUMN IF NOT EXISTS milestone_id UUID NULL REFERENCES milestones(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS milestones_board_id_idx ON milestones(board_id, target_date NULLS LAST, name);
CREATE INDEX IF NOT EXISTS cards_milestone_id_idx ON cards(milestone_id) WHERE milestone_id IS NOT NULL;
