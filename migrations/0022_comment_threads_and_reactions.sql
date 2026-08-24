-- Keep a conversation as a single ordered stream while allowing lightweight threads.
ALTER TABLE comments
    ADD COLUMN parent_comment_id UUID REFERENCES comments(id) ON DELETE CASCADE;

CREATE INDEX comments_parent_created_idx ON comments (parent_comment_id, created_at);

CREATE TABLE comment_reactions (
    comment_id UUID NOT NULL REFERENCES comments(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    emoji TEXT NOT NULL CHECK (char_length(emoji) BETWEEN 1 AND 16),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (comment_id, user_id, emoji)
);

CREATE INDEX comment_reactions_comment_idx ON comment_reactions (comment_id);
