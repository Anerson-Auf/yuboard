-- Labels are a board-level concept.  Older installations stored them per
-- workspace, so copy a legacy label when cards in several boards reference it.
ALTER TABLE labels ADD COLUMN board_id UUID REFERENCES boards(id) ON DELETE CASCADE;

CREATE TEMP TABLE flowboard_legacy_label_boards ON COMMIT DROP AS
SELECT DISTINCT cl.label_id AS old_label_id, c.board_id
FROM card_labels cl
INNER JOIN cards c ON c.id = cl.card_id
UNION
SELECT l.id AS old_label_id,
       (SELECT b.id FROM boards b WHERE b.workspace_id = l.workspace_id ORDER BY b.created_at, b.id LIMIT 1) AS board_id
FROM labels l
WHERE NOT EXISTS (SELECT 1 FROM card_labels cl WHERE cl.label_id = l.id);

DELETE FROM flowboard_legacy_label_boards WHERE board_id IS NULL;

CREATE TEMP TABLE flowboard_legacy_label_ranked ON COMMIT DROP AS
SELECT old_label_id, board_id,
       row_number() OVER (PARTITION BY old_label_id ORDER BY board_id) AS scope_rank
FROM flowboard_legacy_label_boards;

UPDATE labels l
SET board_id = scoped.board_id
FROM flowboard_legacy_label_ranked scoped
WHERE l.id = scoped.old_label_id AND scoped.scope_rank = 1;

CREATE TEMP TABLE flowboard_legacy_label_clones (
    old_label_id UUID NOT NULL,
    board_id UUID NOT NULL,
    new_label_id UUID NOT NULL
) ON COMMIT DROP;

INSERT INTO flowboard_legacy_label_clones (old_label_id, board_id, new_label_id)
SELECT old_label_id, board_id, gen_random_uuid()
FROM flowboard_legacy_label_ranked
WHERE scope_rank > 1;

INSERT INTO labels (id, workspace_id, board_id, name, color)
SELECT clone.new_label_id, label.workspace_id, clone.board_id, label.name, label.color
FROM flowboard_legacy_label_clones clone
INNER JOIN labels label ON label.id = clone.old_label_id;

UPDATE card_labels card_label
SET label_id = clone.new_label_id
FROM cards card
INNER JOIN flowboard_legacy_label_clones clone
    ON clone.board_id = card.board_id
WHERE card_label.card_id = card.id AND card_label.label_id = clone.old_label_id;

ALTER TABLE labels DROP CONSTRAINT IF EXISTS labels_workspace_id_name_key;
ALTER TABLE labels ADD CONSTRAINT labels_board_id_name_key UNIQUE (board_id, name);
CREATE INDEX labels_board_idx ON labels (board_id) WHERE board_id IS NOT NULL;
