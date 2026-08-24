CREATE TABLE card_backgrounds (
    card_id UUID PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    uploaded_by UUID REFERENCES users(id) ON DELETE SET NULL,
    object_key TEXT NOT NULL UNIQUE,
    original_name TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('image/jpeg', 'image/png', 'image/gif', 'image/webp')),
    byte_size BIGINT NOT NULL CHECK (byte_size > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
