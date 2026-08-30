-- Browser push subscriptions belong to an account, not to one browser tab.
-- The outbox is populated by a database trigger so every existing producer of
-- card_notifications automatically receives Web Push delivery.
CREATE TABLE IF NOT EXISTS web_push_subscriptions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    endpoint TEXT NOT NULL UNIQUE,
    p256dh TEXT NOT NULL,
    auth TEXT NOT NULL,
    user_agent TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_success_at TIMESTAMPTZ NULL,
    disabled_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS web_push_subscriptions_user_idx
    ON web_push_subscriptions (user_id) WHERE disabled_at IS NULL;

CREATE TABLE IF NOT EXISTS web_push_delivery_jobs (
    notification_id UUID NOT NULL REFERENCES card_notifications(id) ON DELETE CASCADE,
    subscription_id UUID NOT NULL REFERENCES web_push_subscriptions(id) ON DELETE CASCADE,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_until TIMESTAMPTZ NULL,
    delivered_at TIMESTAMPTZ NULL,
    last_error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (notification_id, subscription_id)
);

CREATE INDEX IF NOT EXISTS web_push_delivery_pending_idx
    ON web_push_delivery_jobs (next_attempt_at)
    WHERE delivered_at IS NULL;

CREATE OR REPLACE FUNCTION flowboard_enqueue_web_push_delivery()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
    INSERT INTO web_push_delivery_jobs (notification_id, subscription_id)
    SELECT NEW.id, subscription.id
    FROM web_push_subscriptions subscription
    WHERE subscription.user_id = NEW.user_id
      AND subscription.disabled_at IS NULL
    ON CONFLICT (notification_id, subscription_id) DO UPDATE
    SET attempts = 0,
        next_attempt_at = now(),
        locked_until = NULL,
        delivered_at = NULL,
        last_error = '',
        updated_at = now();
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS card_notifications_web_push_outbox ON card_notifications;
CREATE TRIGGER card_notifications_web_push_outbox
AFTER INSERT OR UPDATE OF created_at ON card_notifications
FOR EACH ROW EXECUTE FUNCTION flowboard_enqueue_web_push_delivery();
