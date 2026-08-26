ALTER TABLE board_layout_preferences
    DROP CONSTRAINT IF EXISTS board_layout_preferences_view_mode_check;

ALTER TABLE board_layout_preferences
    ADD CONSTRAINT board_layout_preferences_view_mode_check
    CHECK (view_mode IN ('standard', 'freeform', 'dependencies'));
