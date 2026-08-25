-- A board can be arranged as a manual two-dimensional grid.  Existing
-- columns retain their horizontal order and start in the first row.
ALTER TABLE lists ADD COLUMN grid_column INTEGER NOT NULL DEFAULT 0;
ALTER TABLE lists ADD COLUMN grid_row INTEGER NOT NULL DEFAULT 0;

WITH ordered_lists AS (
    SELECT id, row_number() OVER (PARTITION BY board_id ORDER BY position, id)::INTEGER AS grid_column
    FROM lists
)
UPDATE lists
SET grid_column = ordered_lists.grid_column,
    grid_row = 1
FROM ordered_lists
WHERE lists.id = ordered_lists.id;

ALTER TABLE lists ADD CONSTRAINT lists_grid_coordinates_positive CHECK (grid_column > 0 AND grid_row > 0);
CREATE UNIQUE INDEX lists_board_grid_coordinates_idx ON lists (board_id, grid_column, grid_row);
