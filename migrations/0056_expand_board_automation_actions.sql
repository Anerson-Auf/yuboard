ALTER TABLE board_automations
    DROP CONSTRAINT IF EXISTS board_automations_action_type_check;

ALTER TABLE board_automations
    ADD CONSTRAINT board_automations_action_type_check
    CHECK (action_type IN ('complete_card', 'reopen_card', 'set_priority', 'archive_card'));
