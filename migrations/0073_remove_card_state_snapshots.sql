-- Snapshots were removed from the product.  Keep migration 0072 in source so
-- already deployed databases retain a consistent SQLx migration history, then
-- explicitly remove its database objects and stored recovery points.
DROP TRIGGER IF EXISTS cards_capture_state_snapshot ON cards;
DROP FUNCTION IF EXISTS flowboard_capture_card_state_snapshot();
DROP TABLE IF EXISTS card_state_snapshots;
