-- A Discord token authorizes one board.  A column is merely the default place
-- for new suggestions; callers may select any list belonging to that board.
ALTER TABLE discord_integrations
    RENAME COLUMN target_list_id TO default_list_id;

ALTER TABLE discord_integrations
    ALTER COLUMN default_list_id DROP NOT NULL;
