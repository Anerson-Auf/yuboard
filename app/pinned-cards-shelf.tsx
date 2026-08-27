'use client';

import { useMemo } from 'react';
import './pinned-cards-shelf.css';

type Card = { id: string; title: string; listTitle: string; completed: boolean };
export default function PinnedCardsShelf({ ids, cards, onOpen, onUnpin }: { ids: string[]; cards: Card[]; onOpen: (id: string) => void; onUnpin: (id: string) => void }) {
  const pinned = useMemo(() => ids.map((id) => cards.find((card) => card.id === id)).filter((card): card is Card => Boolean(card)), [cards, ids]);
  if (!pinned.length) return null;
  return <section className="pinned-cards-shelf"><header><div><span>ЗАКРЕПЛЁННОЕ</span><b>Мои текущие карточки</b></div><small>ПКМ по карточке — закрепить</small></header><div className="pinned-card-list">{pinned.map((card) => <button type="button" className={card.completed ? 'completed' : ''} key={card.id} onClick={() => onOpen(card.id)}><small>{card.listTitle}</small><b>{card.title}</b><i onClick={(event) => { event.stopPropagation(); onUnpin(card.id); }} aria-label={`Открепить ${card.title}`}>×</i></button>)}</div></section>;
}
