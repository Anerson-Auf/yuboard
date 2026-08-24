# RFC: Canvas-схемы в карточках

## Решение

Добавить в карточку отдельный объект `diagram`, а не сохранять JSON canvas внутри `cards.description` или как произвольное вложение.

## Почему

Canvas — совместно редактируемый документ, а не файл. Ему нужны ревизии, access check по card workspace, отдельная история и предсказуемый лимит размера. Blob в комментарии или data URL быстро ломает autosave, realtime и backup.

## Первый безопасный инкремент

- `card_diagrams`: `id`, `card_id`, `title`, `document JSONB`, `version`, `created_by`, timestamps;
- JSON schema только для nodes/edges/shapes; максимум 1 MiB на документ и 500 nodes;
- API read требует membership, write требует `edit_cards`;
- optimistic concurrency через `version`, conflict возвращает `409`;
- audit events `diagram.created`, `diagram.updated`, `diagram.deleted`;
- realtime event scoped строго workspace/board, без отправки документа пользователям без доступа.

## UI

В карточке появится блок «Схема»: создать, открыть в полноэкранной modal, превью последней версии. Для первого релиза рекомендован Excalidraw-compatible JSON или минимальный React canvas. Не добавлять multiplayer CRDT до появления реального сценария одновременного редактирования: это существенно повышает сложность storage, conflict resolution и observability.
