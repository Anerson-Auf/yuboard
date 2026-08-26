-- Card access is intentionally more granular than board membership.  The
-- public flag controls anonymous visitors; these two presets control the
-- minimum workspace role required for signed-in project members.
ALTER TABLE cards
    ADD COLUMN min_view_preset TEXT NOT NULL DEFAULT 'viewer'
        CHECK (min_view_preset IN ('viewer', 'contributor', 'editor', 'full_access')),
    ADD COLUMN min_edit_preset TEXT NOT NULL DEFAULT 'contributor'
        CHECK (min_edit_preset IN ('contributor', 'editor', 'full_access'));

CREATE INDEX cards_board_access_thresholds_idx
    ON cards (board_id, min_view_preset, min_edit_preset)
    WHERE archived_at IS NULL;
