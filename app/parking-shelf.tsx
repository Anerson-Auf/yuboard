'use client';

import { FormEvent, ReactNode, useEffect, useState } from 'react';
import './parking-shelf.css';

export type ParkingMember = { id: string; name: string; initials: string; color: string; avatarUrl?: string | null };
export type ParkingLabel = { id: string; name: string; color: string; icon_shape?: string; icon_color?: string };
export type ParkingRole = { id: string; name: string; color: string; icon_shape?: string; icon_color?: string };
export type ParkingAttachment = { id: string; name: string; type: string; url: string; size?: number };
export type ParkingChecklistItem = { id: string; title: string; done: boolean; description?: string; attachments?: ParkingAttachment[] };
export type ParkingChecklist = { id: string; title: string; items: ParkingChecklistItem[] };
export type ParkingComment = { id: string; body: string; authorName?: string; createdAt: string };

/**
 * Local representation deliberately mirrors the data used by a board card.
 * It lives in the browser only, but is rendered by the same card UI in page.tsx.
 */
export type ParkingCard = {
  id: string;
  title: string;
  description: string;
  priority: number;
  createdAt: string;
  completedAt?: string | null;
  startAt?: string;
  dueAt?: string;
  labels?: ParkingLabel[];
  roles?: ParkingRole[];
  members?: ParkingMember[];
  checklists?: ParkingChecklist[];
  attachments?: ParkingAttachment[];
  comments?: ParkingComment[];
  coverAttachmentId?: string;
  coverMode?: 'full' | 'top';
  backgroundImageUrl?: string;
};

type Props = {
  storageKey: string;
  cards: ParkingCard[];
  onChange: (cards: ParkingCard[]) => void;
  onOpenCard: (card: ParkingCard) => void;
  onCreateCard: (title: string) => void;
  renderCard: (card: ParkingCard) => ReactNode;
  onParkServerCard: () => void;
  draggedServerCard: boolean;
};

export default function ParkingShelf({ storageKey, cards, onChange, onOpenCard, onCreateCard, renderCard, onParkServerCard, draggedServerCard }: Props) {
  const [isComposerOpen, setComposerOpen] = useState(false);
  const [draft, setDraft] = useState('');

  useEffect(() => {
    window.localStorage.setItem(storageKey, JSON.stringify(cards));
  }, [cards, storageKey]);

  function createCard(event: FormEvent) {
    event.preventDefault();
    const title = draft.trim();
    if (!title) return;
    onCreateCard(title);
    setDraft('');
    setComposerOpen(false);
  }

  return <section
    className="column parking-column"
    aria-label="Локальная парковка"
    onDragOver={(event) => { if (draggedServerCard) event.preventDefault(); }}
    onDrop={(event) => { if (!draggedServerCard) return; event.preventDefault(); onParkServerCard(); }}
  >
    <div className="column-header">
      <h2><span className="drag-handle" aria-hidden="true">⁝</span> Парковка <small>локально</small></h2>
      <span className="column-count">{cards.length}</span>
    </div>
    <div className="card-list parking-card-list">
      {cards.map((card) => <div key={card.id} onClick={() => onOpenCard(card)}>{renderCard(card)}</div>)}
    </div>
    {isComposerOpen ? <form className="composer" onSubmit={createCard}>
      <textarea autoFocus value={draft} onChange={(event) => setDraft(event.target.value)} placeholder="Название задачи" />
      <div><button className="add-card" type="submit">Добавить</button><button className="cancel" type="button" onClick={() => { setComposerOpen(false); setDraft(''); }}>Отмена</button></div>
    </form> : <button className="add-task" type="button" onClick={() => setComposerOpen(true)}>＋ Добавить задачу</button>}
  </section>;
}
