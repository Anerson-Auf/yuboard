# Discord bot API

This API is for a bot that imports Discord suggestions into one Flowboard board. It is intentionally not a general account API: a token can create cards in any list of its board and can read or add comments only to cards on that board. It cannot read other boards, manage people, change permissions, or delete content.

## Create a token

Open the project, click **Discord API**, optionally choose the default list for suggestions, and create a token. The token belongs to the entire board, so moving a card between its lists does not affect the integration. Copy it immediately; Flowboard stores only its SHA-256 digest and never shows the original token again. Revoking the token takes effect immediately.

Every bot request uses:

```http
Authorization: Bearer fb_discord_…
Content-Type: application/json
```

## List available columns

```http
GET /v1/integrations/discord/lists
```

The token returns only the columns of its own board:

```json
[
  { "id": "list-uuid", "title": "Предложения" },
  { "id": "another-list-uuid", "title": "В работе" }
]
```

Use one of these values as `list_id` when creating a card.

## List cards on the board

```http
GET /v1/integrations/discord/cards
```

Returns active cards from this token's board only. Each entry contains `id`, `list_id`, `title`, and `description`. Use the card `id` to retrieve or append its conversation.

## Read or change completion status

```http
GET /v1/integrations/discord/cards/{card-id}
```

Returns the current card data plus an explicit completion state:

```json
{
  "id": "card-uuid",
  "list_id": "list-uuid",
  "title": "Добавить ночной режим",
  "description": "…",
  "is_completed": true,
  "completed_at": "2026-08-25T12:34:56Z"
}
```

To set or remove the green **Выполнено** mark:

```http
PATCH /v1/integrations/discord/cards/{card-id}/completion

{ "is_completed": true }
```

Pass `false` to return the card to work. Both endpoints are restricted to the board that owns the Discord token.

## Move a card

```http
POST /v1/integrations/discord/cards/{card-id}/move

{
  "list_id": "target-list-uuid",
  "before_card_id": "optional-card-uuid-in-target-list"
}
```

`list_id` and the optional `before_card_id` must belong to the token's board. Omit `before_card_id` to append the card to the end of the target list. Moving a card keeps its Discord integration and conversation intact.

## Close a suggestion (archive a card)

```http
DELETE /v1/integrations/discord/cards/{card-id}
```

This archives the active card instead of physically deleting it, so the conversation, screenshots and reason remain available in Flowboard's archive. The request is idempotent: closing an already archived card returns `204 No Content`. A card from another board returns `404`.

For `/close Причина` automation: first read comments, let the bot validate that the command author is one of your developers, post the reason into Discord, then call this endpoint with the matching Flowboard card ID.

## Keep a Discord thread and card in sync

After the bot creates a Discord thread for a suggestion, bind its Discord thread ID to the card once:

```http
PUT /v1/integrations/discord/cards/{card-id}/thread

{ "thread_id": "123456789012345678" }
```

The operation is idempotent. One thread cannot be bound to two cards under the same token. It returns the card's `thread_id`, archive state and completion state.

When Discord reports that this thread was opened or unarchived, look up the card and restore it:

```http
GET /v1/integrations/discord/threads/{thread-id}/card
POST /v1/integrations/discord/cards/{card-id}/restore
```

`restore` is idempotent: an already active card returns `200` with its current state. It keeps comments, attachments, cover, completion mark and the linked thread.

For the reverse direction, poll a durable cursor:

```http
GET /v1/integrations/discord/cards/sync?after=0&limit=100
```

Each item contains `event_id`, `event_kind` (`thread_linked`, `archived`, or `restored`), `thread_id`, and current card state. Store the highest `event_id` only after Discord accepted the corresponding archive/unarchive action, then request `after={that-id}`. Thus a restore in Flowboard makes the bot reopen its thread without relying on an in-memory webhook.

## Create (or safely repeat) a suggestion card

`source_id` is the original Discord message ID. Sending the same ID again returns the same card instead of making a duplicate.

```http
POST /v1/integrations/discord/cards

{
  "source_id": "123456789012345678",
  "title": "Добавить ночной режим в лаунчер",
  "description": "Предложка из Discord #ideas",
  "list_id": "optional-list-uuid-on-this-board"
}
```

Response:

```json
{
  "id": "card-uuid",
  "list_id": "target-list-uuid",
  "title": "Добавить ночной режим в лаунчер",
  "description": "Предложка из Discord #ideas"
}
```

Store the returned `id` beside the Discord thread/message. Use it to forward player replies.

Omit `list_id` to use the token's default list. It is required only when the token was created without one.

## Read a card conversation

```http
GET /v1/integrations/discord/cards/{card-id}/comments
```

The response is the ordered card conversation, including Flowboard and Discord-originated comments. The card may be in any list of this token's board.

### Poll only new comments

Do one full request when the bot first starts, store the newest returned comment ID, then poll with that cursor instead of downloading the whole history:

```http
GET /v1/integrations/discord/cards/{card-id}/comments?after={last-comment-id}&limit=100
```

With `after`, the API returns at most `limit` comments strictly newer than that ID, in chronological order. Store the ID of the last returned item and repeat while the response has `limit` items. `limit` defaults to 100 and is capped at 200. A cursor from another card is rejected. This makes `/close причина` processing incremental and prevents the bot from reprocessing old developer messages.

## Add a player comment, screenshot, or video

`message_id` makes the operation idempotent. Discord avatars and media must use Discord CDN HTTPS URLs. Supported attachments are JPEG, PNG, GIF, WebP, MP4, WebM, and MOV, up to 50 MiB each.

```http
POST /v1/integrations/discord/cards/{card-id}/comments

{
  "message_id": "234567890123456789",
  "author_name": "PlayerNick",
  "author_avatar_url": "https://cdn.discordapp.com/avatars/…/avatar.webp",
  "body": "Вот видео с багом:",
  "attachments": [
    {
      "url": "https://cdn.discordapp.com/attachments/…/bug.mp4",
      "filename": "bug.mp4",
      "media_type": "video/mp4",
      "byte_size": 1234567
    }
  ]
}
```

The comment keeps the Discord display name and avatar without creating a Flowboard account. If a Flowboard account is deleted, its historical comments still render as `Deleted user`; Discord comments are separate external identities.

## Set an image from a Discord comment as the card cover

After forwarding a comment, pass the URL of its first image attachment. Flowboard finds that exact image on the card and makes it the cover. `mode: "full"` means **фон**; `mode: "top"` means **сверху**.

```http
POST /v1/integrations/discord/cards/{card-id}/cover

{
  "attachment_url": "https://cdn.discordapp.com/attachments/…/screenshot.png",
  "mode": "full"
}
```

The bot may use `attachment_id` instead of `attachment_url` if it already has Flowboard's ID. Exactly one of them is required. Only images already attached to that card within the token's board are accepted; videos cannot be card covers.

## Minimal Node example

```js
const api = 'https://flowboard.zei.su';
const token = process.env.FLOWBOARD_DISCORD_TOKEN;

async function flowboard(path, payload) {
  const response = await fetch(`${api}${path}`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!response.ok) throw new Error(await response.text());
  return response.json();
}
```

Keep the token only in the bot's server environment (for example PM2 `env`), never in Discord messages, browser code, or a Git repository.
