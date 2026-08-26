'use client';

import { useEffect, useState } from 'react';
import './saved-views-overlay.css';

export type SavedView = { id: string; name: string; filterMode: string; cardSort: string; milestoneId: string | null };
export type CurrentView = Omit<SavedView, 'id' | 'name'>;

export default function SavedViewsOverlay({ storageKey, current, onApply, onClose }: { storageKey: string; current: CurrentView; onApply: (view: SavedView) => void; onClose: () => void }) {
  const [views, setViews] = useState<SavedView[]>([]);
  const [name, setName] = useState('');
  useEffect(() => { try { setViews(JSON.parse(window.localStorage.getItem(storageKey) ?? '[]') as SavedView[]); } catch { setViews([]); } }, [storageKey]);
  const persist = (next: SavedView[]) => { setViews(next); window.localStorage.setItem(storageKey, JSON.stringify(next)); };
  const save = () => { const trimmed = name.trim(); if (!trimmed) return; persist([{ id: crypto.randomUUID(), name: trimmed.slice(0, 60), ...current }, ...views].slice(0, 30)); setName(''); };
  return <div className="saved-views-backdrop" role="presentation" onMouseDown={onClose}><section className="saved-views-overlay" role="dialog" aria-modal="true" aria-label="Сохранённые представления" onMouseDown={(event) => event.stopPropagation()}><header><div><p>ПРЕДСТАВЛЕНИЯ</p><h2>Сохранённые фильтры</h2></div><button type="button" onClick={onClose} aria-label="Закрыть">×</button></header><form onSubmit={(event) => { event.preventDefault(); save(); }}><input autoFocus value={name} onChange={(event) => setName(event.target.value)} maxLength={60} placeholder="Например, Мои срочные задачи" /><button type="submit" disabled={!name.trim()}>Сохранить текущий вид</button></form><div>{views.length ? views.map((view) => <article key={view.id}><button type="button" onClick={() => { onApply(view); onClose(); }}><b>{view.name}</b><small>{view.filterMode === 'assigned' ? 'Назначенные мне' : view.filterMode === 'my_roles' ? 'По моим ролям' : view.filterMode === 'overdue' ? 'Просроченные' : view.filterMode === 'due' ? 'С дедлайном' : 'Все задачи'} · {view.cardSort === 'priority' ? 'сначала важные' : view.cardSort === 'activity' ? 'по активности' : 'ручной порядок'}</small></button><button type="button" onClick={() => persist(views.filter((item) => item.id !== view.id))} aria-label={`Удалить «${view.name}»`}>×</button></article>) : <p>Сохраните текущие фильтры, чтобы возвращаться к ним одним кликом.</p>}</div></section></div>;
}
