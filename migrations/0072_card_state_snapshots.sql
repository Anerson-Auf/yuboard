-- Previous card states are deliberately bounded.  The trigger captures only
-- meaningful card fields and leaves the newest 80 recovery points per card.
CREATE TABLE card_state_snapshots (
    id BIGSERIAL PRIMARY KEY,
    card_id UUID NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    state JSONB NOT NULL,
    changed_fields TEXT[] NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX card_state_snapshots_card_created_idx
    ON card_state_snapshots (card_id, created_at DESC, id DESC);

CREATE OR REPLACE FUNCTION flowboard_capture_card_state_snapshot()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
    fields TEXT[];
BEGIN
    fields := array_remove(ARRAY[
        CASE WHEN OLD.title IS DISTINCT FROM NEW.title THEN 'title' END,
        CASE WHEN OLD.description IS DISTINCT FROM NEW.description THEN 'description' END,
        CASE WHEN OLD.priority IS DISTINCT FROM NEW.priority THEN 'priority' END,
        CASE WHEN OLD.is_frozen IS DISTINCT FROM NEW.is_frozen THEN 'is_frozen' END,
        CASE WHEN OLD.start_at IS DISTINCT FROM NEW.start_at THEN 'start_at' END,
        CASE WHEN OLD.due_at IS DISTINCT FROM NEW.due_at THEN 'due_at' END,
        CASE WHEN OLD.completed_at IS DISTINCT FROM NEW.completed_at THEN 'completed_at' END,
        CASE WHEN OLD.cover_attachment_id IS DISTINCT FROM NEW.cover_attachment_id THEN 'cover_attachment_id' END,
        CASE WHEN OLD.cover_mode IS DISTINCT FROM NEW.cover_mode THEN 'cover_mode' END,
        CASE WHEN OLD.background_image_url IS DISTINCT FROM NEW.background_image_url THEN 'background_image_url' END
    ], NULL);
    IF coalesce(array_length(fields, 1), 0) = 0 THEN RETURN NEW; END IF;

    INSERT INTO card_state_snapshots (card_id, state, changed_fields)
    VALUES (OLD.id, jsonb_build_object(
        'title', OLD.title,
        'description', OLD.description,
        'priority', OLD.priority,
        'is_frozen', OLD.is_frozen,
        'start_at', OLD.start_at,
        'due_at', OLD.due_at,
        'completed_at', OLD.completed_at,
        'completed_by', OLD.completed_by,
        'cover_attachment_id', OLD.cover_attachment_id,
        'cover_mode', OLD.cover_mode,
        'background_image_url', OLD.background_image_url
    ), fields);

    DELETE FROM card_state_snapshots
    WHERE card_id = OLD.id
      AND id IN (
        SELECT id FROM card_state_snapshots
        WHERE card_id = OLD.id
        ORDER BY created_at DESC, id DESC
        OFFSET 80
      );
    RETURN NEW;
END;
$$;

CREATE TRIGGER cards_capture_state_snapshot
BEFORE UPDATE OF title, description, priority, is_frozen, start_at, due_at,
                 completed_at, cover_attachment_id, cover_mode, background_image_url
ON cards
FOR EACH ROW EXECUTE FUNCTION flowboard_capture_card_state_snapshot();
