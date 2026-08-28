-- Search stays field-scoped: each EXISTS branch in search_board_cards can use
-- its own GIN index instead of aggregating a whole board for every keystroke.
CREATE INDEX IF NOT EXISTS cards_search_document_idx
    ON cards USING gin (to_tsvector('simple', concat_ws(' ', title, description)));
CREATE INDEX IF NOT EXISTS comments_search_document_idx
    ON comments USING gin (to_tsvector('simple', body));
CREATE INDEX IF NOT EXISTS checklists_search_document_idx
    ON checklists USING gin (to_tsvector('simple', title));
CREATE INDEX IF NOT EXISTS checklist_items_search_document_idx
    ON checklist_items USING gin (to_tsvector('simple', concat_ws(' ', title, description)));
CREATE INDEX IF NOT EXISTS labels_search_document_idx
    ON labels USING gin (to_tsvector('simple', name));
CREATE INDEX IF NOT EXISTS users_search_document_idx
    ON users USING gin (to_tsvector('simple', display_name));
CREATE INDEX IF NOT EXISTS profile_roles_search_document_idx
    ON profile_roles USING gin (to_tsvector('simple', name));
