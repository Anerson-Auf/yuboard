'use client';

import './inbox-overlay.css';

export type InboxItem = { id: string; card_id: string; board_id: string; card_title: string; board_title: string; actor_name: string | null; action: string; detail: string; is_read: boolean; created_at: string };

export default function InboxOverlay({ items, onClose, onOpen, onMarkAllRead }: { items: InboxItem[]; onClose: () => void; onOpen: (item: InboxItem) => void; onMarkAllRead: () => void }) {
  const unread = items.filter((item) => !item.is_read);
  return <div className="inbox-overlay-backdrop" role="presentation" onMouseDown={onClose}><section className="inbox-overlay" role="dialog" aria-modal="true" aria-label="Входящие" onMouseDown={(event) => event.stopPropagation()}><header><div><p>ВХОДЯЩИЕ</p><h2>Уведомления и изменения</h2><span>{unread.length ? `${unread.length} непрочитанных` : 'Всё прочитано'}</span></div><div>{unread.length > 0 && <button type="button" onClick={onMarkAllRead}>Прочитать всё</button>}<button type="button" onClick={onClose} aria-label="Закрыть входящие">×</button></div></header><div className="inbox-sections">{unread.length > 0 && <section><h3>Новые</h3>{unread.map((item) => <InboxRow item={item} onOpen={onOpen} key={item.id} />)}</section>}<section><h3>{unread.length ? 'Ранее' : 'Все события'}</h3>{items.filter((item) => item.is_read).map((item) => <InboxRow item={item} onOpen={onOpen} key={item.id} />)}{!items.length && <p>Пока нет событий. Подписывайтесь на карточки — изменения появятся здесь.</p>}</section></div></section></div>;
}

function InboxRow({ item, onOpen }: { item: InboxItem; onOpen: (item: InboxItem) => void }) {
  return <button className={`inbox-row ${item.is_read ? 'read' : 'unread'}`} type="button" onClick={() => onOpen(item)}><span className="inbox-row-dot" /><div><p>{item.actor_name ? `@${item.actor_name} · ` : ''}{item.action}</p><b>{item.card_title}</b>{item.detail && <small>{item.detail}</small>}<time>{item.board_title} · {new Date(item.created_at).toLocaleString('ru-RU')}</time></div></button>;
}
