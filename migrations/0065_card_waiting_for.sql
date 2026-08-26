CREATE TABLE card_waiting_for (
    card_id UUID PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    role_id UUID REFERENCES profile_roles(id) ON DELETE CASCADE,
    note TEXT NOT NULL DEFAULT '',
    created_by UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT card_waiting_for_target_check CHECK (num_nonnulls(user_id, role_id) = 1)
);

CREATE INDEX card_waiting_for_user_idx ON card_waiting_for(user_id) WHERE user_id IS NOT NULL;
CREATE INDEX card_waiting_for_role_idx ON card_waiting_for(role_id) WHERE role_id IS NOT NULL;
