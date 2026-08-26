'use client';

import { useEffect, useMemo, useState } from 'react';
import './pinned-cards-shelf.css';

type Card = { id: string; title: string; listTitle: string; completed: boolean };
export default function PinnedCardsShelf({ storageKey, cards, onOpen }: { storageKey: string; cards: Card[]; onOpen: (id: string) => void }) {
  const [ids, setIds] = useState<string[]>([]); const [isOpen, setOpen] = useState(false);
  useEffect(() => { try { setIds(JSON.parse(localStorage.getItem(storageKey) ?? '[]') as string[]); } catch { setIds([]); } }, [storageKey]);
  const pinned = useMemo(() => ids.map((id) => cards.find((card) => card.id === id)).filter((card): card is Card => Boolean(card)), [cards, ids]);
  const update = (next: string[]) => { const normalized = next.slice(0, 12); setIds(normalized); localStorage.setItem(storageKey, JSON.stringify(normalized)); };
  const toggle = (id: string) => update(ids.includes(id) ? ids.filter((value) => value !== id) : [id, ...ids]);
  return <section className="pinned-cards-shelf"><header><div><span>ЗАКРЕПЛЁННОЕ</span><b>Мои текущие карточки</b></div><button type="button" onClick={() => setOpen((value) => !value)}>{isOpen ? 'Готово' : 'Настроить'}</button></header>{pinned.length ? <div className="pinned-card-list">{pinned.map((card) => <button type="button" className={card.completed ? 'completed' : ''} key={card.id} onClick={() => onOpen(card.id)}><small>{card.listTitle}</small><b>{card.title}</b><i onClick={(event) => { event.stopPropagation(); toggle(card.id); }} aria-label={`Открепить ${card.title}`}>×</i></button>)}</div> : <p>Закрепите задачи, которые делаете сейчас.</p>}{isOpen && <div className="pinned-picker">{cards.map((card) => <label key={card.id}><input type="checkbox" checked={ids.includes(card.id)} onChange={() => toggle(card.id)} /><span><b>{card.title}</b><small>{card.listTitle}</small></span></label>)}</div>}</section>;
}
