'use client';

import { useMemo, useState } from 'react';
import './inbox-overlay.css';

export type InboxItem = { id: string; card_id: string; board_id: string; card_title: string; board_title: string; actor_name: string | null; action: string; detail: string; is_read: boolean; created_at: string };

type InboxSection = 'all' | 'attention' | 'updates';

function needsAttention(item: InboxItem) {
  return item.action === 'Нужна ваша проверка'
    || item.action === 'Ожидают вашего действия'
    || /(?:вас назначили|ждём вас|ждут вас)/i.test(item.detail);
}

export default function InboxOverlay({ items, onClose, onOpen, onMarkAllRead }: { items: InboxItem[]; onClose: () => void; onOpen: (item: InboxItem) => void; onMarkAllRead: () => void }) {
  const [section, setSection] = useState<InboxSection>('all');
  const unread = items.filter((item) => !item.is_read);
  const attention = useMemo(() => items.filter(needsAttention), [items]);
  const updates = useMemo(() => items.filter((item) => !needsAttention(item)), [items]);
  const availableSections = [
    { id: 'all' as const, label: 'Все', count: items.length },
    ...(attention.length ? [{ id: 'attention' as const, label: 'Нужно от вас', count: attention.length }] : []),
    ...(updates.length ? [{ id: 'updates' as const, label: 'Обновления', count: updates.length }] : []),
  ];
  const visible = section === 'attention' ? attention : section === 'updates' ? updates : items;
  const visibleUnread = visible.filter((item) => !item.is_read);
  const visibleRead = visible.filter((item) => item.is_read);

  return <div className="inbox-overlay-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="inbox-overlay" role="dialog" aria-modal="true" aria-label="Входящие" onMouseDown={(event) => event.stopPropagation()}>
      <header>
        <div><p>ВХОДЯЩИЕ</p><h2>Уведомления и изменения</h2><span>{unread.length ? `${unread.length} непрочитанных` : 'Всё прочитано'}</span></div>
        <div>{unread.length > 0 && <button type="button" onClick={onMarkAllRead}>Прочитать всё</button>}<button type="button" onClick={onClose} aria-label="Закрыть входящие">×</button></div>
      </header>
      {items.length > 0 && <nav className="inbox-tabs" aria-label="Категории входящих">
        {availableSections.map((item) => <button type="button" key={item.id} className={section === item.id ? 'active' : ''} onClick={() => setSection(item.id)}>{item.label}<span>{item.count}</span></button>)}
      </nav>}
      <div className="inbox-sections">
        {visibleUnread.length > 0 && <section><h3>{section === 'attention' ? 'Требуют внимания' : 'Новые'}</h3>{visibleUnread.map((item) => <InboxRow item={item} onOpen={onOpen} key={item.id} />)}</section>}
        {visibleRead.length > 0 && <section><h3>{visibleUnread.length ? 'Ранее' : 'Все события'}</h3>{visibleRead.map((item) => <InboxRow item={item} onOpen={onOpen} key={item.id} />)}</section>}
        {!visible.length && <p className="inbox-empty">В этой категории пока нет событий.</p>}
        {!items.length && <p className="inbox-empty">Пока нет событий. Подписывайтесь на карточки — изменения появятся здесь.</p>}
      </div>
    </section>
  </div>;
}

function InboxRow({ item, onOpen }: { item: InboxItem; onOpen: (item: InboxItem) => void }) {
  return <button className={`inbox-row ${item.is_read ? 'read' : 'unread'} ${needsAttention(item) ? 'attention' : ''}`} type="button" onClick={() => onOpen(item)}><span className="inbox-row-dot" /><div><p>{item.actor_name ? `@${item.actor_name} · ` : ''}{item.action}</p><b>{item.card_title}</b>{item.detail && <small>{item.detail}</small>}<time>{item.board_title} · {new Date(item.created_at).toLocaleString('ru-RU')}</time></div></button>;
}
