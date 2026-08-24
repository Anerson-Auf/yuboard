CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE checklists (
    id UUID PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    title TEXT NOT NULL CHECK (char_length(title) BETWEEN 1 AND 200),
    position NUMERIC(20, 10) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (card_id, position)
);

ALTER TABLE checklist_items ADD COLUMN checklist_id UUID REFERENCES checklists(id) ON DELETE CASCADE;

INSERT INTO checklists (id, card_id, title, position)
SELECT gen_random_uuid(), card_id, 'Чек-лист', 1000
FROM checklist_items
GROUP BY card_id;

UPDATE checklist_items item
SET checklist_id = checklist.id
FROM checklists checklist
WHERE checklist.card_id = item.card_id
  AND item.checklist_id IS NULL;

ALTER TABLE checklist_items ALTER COLUMN checklist_id SET NOT NULL;
ALTER TABLE checklist_items DROP CONSTRAINT IF EXISTS checklist_items_card_id_position_key;
ALTER TABLE checklist_items ADD CONSTRAINT checklist_items_checklist_position_key UNIQUE (checklist_id, position);
CREATE INDEX checklist_items_checklist_idx ON checklist_items (checklist_id, position);
