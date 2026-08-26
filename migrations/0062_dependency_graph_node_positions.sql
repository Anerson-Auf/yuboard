-- Dependency graph positions are shared by the board. They are independent
-- from the freeform Kanban layout: moving a graph node never detaches a card.
CREATE TABLE board_dependency_node_positions (
    board_id UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    x INTEGER NOT NULL CHECK (x >= 0 AND x <= 200000),
    y INTEGER NOT NULL CHECK (y >= 0 AND y <= 200000),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (board_id, card_id)
);

CREATE INDEX board_dependency_node_positions_board_updated_idx
    ON board_dependency_node_positions (board_id, updated_at DESC);
