ALTER TABLE card_relations
    ADD COLUMN note TEXT NOT NULL DEFAULT ''
    CHECK (char_length(note) <= 500);
