'use client';

import { FormEvent, useEffect, useState } from 'react';
import './parking-shelf.css';

export type ParkingCard = { id: string; title: string; description: string; priority: number; createdAt: string };
type ColumnChoice = { id: string; title: string };

export default function ParkingShelf({ storageKey, columns, cards, onChange, onMoveToBoard }: {
  storageKey: string;
  columns: ColumnChoice[];
  cards: ParkingCard[];
  onChange: (cards: ParkingCard[]) => void;
  onMoveToBoard: (card: ParkingCard, listId: string) => Promise<boolean>;
}) {
  const [isOpen, setOpen] = useState(false);
  const [draftTitle, setDraftTitle] = useState('');
  const [draftDescription, setDraftDescription] = useState('');
  const [targetListId, setTargetListId] = useState('');
  const [movingId, setMovingId] = useState<string | null>(null);

  useEffect(() => { if (!targetListId && columns[0]) setTargetListId(columns[0].id); }, [columns, targetListId]);
  useEffect(() => { window.localStorage.setItem(storageKey, JSON.stringify(cards)); }, [cards, storageKey]);

  function addLocalCard(event: FormEvent) {
    event.preventDefault(); const title = draftTitle.trim();
    if (!title) return;
    onChange([{ id: `parking-${crypto.randomUUID()}`, title, description: draftDescription.trim(), priority: 0, createdAt: new Date().toISOString() }, ...cards]);
    setDraftTitle(''); setDraftDescription('');
  }
  function updateCard(id: string, patch: Partial<ParkingCard>) { onChange(cards.map((card) => card.id === id ? { ...card, ...patch } : card)); }
  async function moveCard(card: ParkingCard) {
    if (!targetListId || movingId) return;
    setMovingId(card.id);
    try { if (await onMoveToBoard(card, targetListId)) onChange(cards.filter((item) => item.id !== card.id)); }
    finally { setMovingId(null); }
  }

  return <aside className={`parking-shelf ${isOpen ? 'open' : ''}`} aria-label="Локальная парковка задач">
    {isOpen && <section className="parking-panel"><header><div><span>ЛОКАЛЬНО НА ЭТОМ УСТРОЙСТВЕ</span><h2>Парковка</h2></div><button type="button" onClick={() => setOpen(false)} aria-label="Закрыть парковку">×</button></header><p>Эти карточки хранятся только в браузере. Они не попадают в API и вернутся после перезагрузки.</p><form onSubmit={addLocalCard}><input value={draftTitle} onChange={(event) => setDraftTitle(event.target.value)} maxLength={200} placeholder="Новая локальная задача" /><textarea value={draftDescription} onChange={(event) => setDraftDescription(event.target.value)} maxLength={10_000} placeholder="Описание (необязательно)" /><button type="submit">＋ Добавить в парковку</button></form><label className="parking-target">Вернуть в колонку<select value={targetListId} onChange={(event) => setTargetListId(event.target.value)}>{columns.map((column) => <option key={column.id} value={column.id}>{column.title}</option>)}</select></label><div className="parking-list">{cards.length ? cards.map((card) => <article key={card.id}><input value={card.title} onChange={(event) => updateCard(card.id, { title: event.target.value.slice(0, 200) })} aria-label="Название локальной задачи" /><textarea value={card.description} onChange={(event) => updateCard(card.id, { description: event.target.value.slice(0, 10_000) })} placeholder="Описание" aria-label="Описание локальной задачи" /><div><label>Приоритет<select value={card.priority} onChange={(event) => updateCard(card.id, { priority: Number(event.target.value) })}>{[0, 1, 2, 3, 4, 5].map((value) => <option key={value} value={value}>{value || 'Нет'}</option>)}</select></label><button type="button" onClick={() => void moveCard(card)} disabled={!columns.length || movingId === card.id}>{movingId === card.id ? 'Переносим…' : 'На доску →'}</button><button type="button" className="parking-delete" onClick={() => onChange(cards.filter((item) => item.id !== card.id))} aria-label={`Удалить ${card.title}`}>×</button></div></article>) : <span className="parking-empty">Парковка пуста.</span>}</div></section>}
    <button type="button" className="parking-trigger" onClick={() => setOpen((current) => !current)} aria-expanded={isOpen} title="Локальная парковка">▱ <span>Парковка</span>{cards.length > 0 && <b>{cards.length}</b>}</button>
  </aside>;
}
