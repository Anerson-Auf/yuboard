-- Freeform is a board view shared by the whole team.  Keep the per-user
-- `view_mode` preference, but move coordinates to a board-owned table.
CREATE TABLE board_freeform_list_positions_shared (
    board_id UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    list_id UUID NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    x INTEGER NOT NULL CHECK (x >= 0 AND x <= 200000),
    y INTEGER NOT NULL CHECK (y >= 0 AND y <= 200000),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (board_id, list_id)
);

-- If a personal layout was already saved during the short-lived first
-- version, promote the most recently changed version for each board/list.
INSERT INTO board_freeform_list_positions_shared (board_id, list_id, x, y, updated_at)
SELECT DISTINCT ON (positions.board_id, positions.list_id)
    positions.board_id,
    positions.list_id,
    positions.x,
    positions.y,
    preferences.updated_at
FROM board_freeform_list_positions positions
INNER JOIN board_layout_preferences preferences
    ON preferences.user_id = positions.user_id
    AND preferences.board_id = positions.board_id
ORDER BY positions.board_id, positions.list_id, preferences.updated_at DESC;

DROP TABLE board_freeform_list_positions;
ALTER TABLE board_freeform_list_positions_shared RENAME TO board_freeform_list_positions;
