# Discord bot API

This API is for a bot that imports Discord suggestions into one Flowboard board. It is intentionally not a general account API: a token can create cards in any list of its board and can read or add comments only to cards on that board. It cannot read other boards, manage people, change permissions, or delete content.

## Create a token

Open the project, click **Discord API**, optionally choose the default list for suggestions, and create a token. The token belongs to the entire board, so moving a card between its lists does not affect the integration. Copy it immediately; Flowboard stores only its SHA-256 digest and never shows the original token again. Revoking the token takes effect immediately.

Every bot request uses:

```http
Authorization: Bearer fb_discord_…
Content-Type: application/json
```

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
