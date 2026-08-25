CREATE TABLE board_stickers (
    id UUID PRIMARY KEY,
    board_id UUID NOT NULL REFERENCES boards(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(trim(name)) BETWEEN 1 AND 80),
    object_key TEXT NOT NULL UNIQUE,
    media_type TEXT NOT NULL CHECK (media_type IN ('image/jpeg', 'image/png', 'image/gif', 'image/webp')),
    byte_size BIGINT NOT NULL CHECK (byte_size > 0 AND byte_size <= 5242880),
    uploaded_by UUID NULL REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX board_stickers_board_created_idx ON board_stickers (board_id, created_at);

-- Custom stickers are stored as a stable reference in the existing reaction
-- table ("sticker:<uuid>").  The earlier limit was sufficient for Unicode
-- emoji but not for that reference.
ALTER TABLE comment_reactions DROP CONSTRAINT IF EXISTS comment_reactions_emoji_check;
ALTER TABLE comment_reactions
    ADD CONSTRAINT comment_reactions_emoji_check CHECK (char_length(emoji) BETWEEN 1 AND 64);
