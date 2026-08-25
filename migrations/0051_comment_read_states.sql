-- Read state is intentionally per message, not a global "last seen" cursor:
-- a person can clear a main conversation without losing an unread reply in one
-- of its threads.
CREATE TABLE comment_read_states (
    comment_id UUID NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    read_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (comment_id, user_id)
);

CREATE INDEX comment_read_states_user_comment_idx
    ON comment_read_states (user_id, comment_id);

-- Do not turn the rollout itself into a wall of "new" markers. From this
-- migration onward, only messages created after a member has seen the board
-- can become unread.
INSERT INTO comment_read_states (comment_id, user_id, read_at)
SELECT c.id, member.user_id, now()
FROM comments c
INNER JOIN cards card ON card.id = c.card_id
INNER JOIN board_members member ON member.board_id = card.board_id
ON CONFLICT (comment_id, user_id) DO NOTHING;
