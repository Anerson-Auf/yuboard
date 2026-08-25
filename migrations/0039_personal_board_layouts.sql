-- The canonical list order belongs to the board.  A freeform arrangement is
-- deliberately personal: one teammate moving columns must not rearrange
-- another teammate's workspace.
CREATE TABLE board_layout_preferences (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    board_id UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    view_mode TEXT NOT NULL DEFAULT 'standard' CHECK (view_mode IN ('standard', 'freeform')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, board_id)
);

CREATE TABLE board_freeform_list_positions (
    user_id UUID NOT NULL,
    board_id UUID NOT NULL,
    list_id UUID NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    x INTEGER NOT NULL CHECK (x >= 0 AND x <= 200000),
    y INTEGER NOT NULL CHECK (y >= 0 AND y <= 200000),
    PRIMARY KEY (user_id, board_id, list_id),
    FOREIGN KEY (user_id, board_id) REFERENCES board_layout_preferences(user_id, board_id) ON DELETE CASCADE
);

CREATE INDEX board_freeform_list_positions_board_user_idx
    ON board_freeform_list_positions (board_id, user_id);
