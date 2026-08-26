'use client';

import { useEffect, useState } from 'react';
import './automations-overlay.css';

type Automation = { id: string; name: string; list_id: string; list_title: string; enabled: boolean; created_at: string };
type Column = { id: string | number; title: string };

function headers() { const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length); return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' }; }

export default function AutomationsOverlay({ boardId, columns, onClose }: { boardId: string; columns: Column[]; onClose: () => void }) {
  const [items, setItems] = useState<Automation[]>([]); const [name, setName] = useState(''); const [listId, setListId] = useState(''); const [loading, setLoading] = useState(true);
  const load = () => { setLoading(true); void fetch(`/v1/boards/${boardId}/automations`).then((response) => response.ok ? response.json() as Promise<Automation[]> : []).then(setItems).finally(() => setLoading(false)); };
  useEffect(load, [boardId]);
  const create = () => { if (!name.trim() || !listId) return; void fetch(`/v1/boards/${boardId}/automations`, { method: 'POST', headers: headers(), body: JSON.stringify({ name: name.trim(), list_id: listId }) }).then((response) => response.ok ? response.json() as Promise<Automation> : Promise.reject()).then((item) => { setItems((current) => [item, ...current]); setName(''); setListId(''); }); };
  const toggle = (item: Automation) => { void fetch(`/v1/boards/${boardId}/automations/${item.id}`, { method: 'PATCH', headers: headers(), body: JSON.stringify({ enabled: !item.enabled }) }).then((response) => response.ok ? response.json() as Promise<Automation> : Promise.reject()).then((updated) => setItems((current) => current.map((value) => value.id === updated.id ? updated : value))); };
  const remove = (item: Automation) => { void fetch(`/v1/boards/${boardId}/automations/${item.id}`, { method: 'DELETE', headers: headers() }).then((response) => { if (response.ok) setItems((current) => current.filter((value) => value.id !== item.id)); }); };
  return <div className="automations-backdrop" role="presentation" onMouseDown={onClose}><section className="automations-overlay" role="dialog" aria-modal="true" aria-label="Автоматизации" onMouseDown={(event) => event.stopPropagation()}><header><div><p>АВТОМАТИЗАЦИИ</p><h2>Правила без кода</h2><span>Сейчас: при переносе в колонку — завершить карточку.</span></div><button type="button" onClick={onClose} aria-label="Закрыть">×</button></header><form onSubmit={(event) => { event.preventDefault(); create(); }}><input value={name} onChange={(event) => setName(event.target.value)} placeholder="Например, Готово закрывает задачу" maxLength={120} /><select value={listId} onChange={(event) => setListId(event.target.value)}><option value="">Выберите колонку…</option>{columns.map((column) => <option value={column.id} key={column.id}>{column.title}</option>)}</select><button disabled={!name.trim() || !listId}>Создать правило</button></form><div className="automation-list">{loading ? <p>Загружаем…</p> : items.length ? items.map((item) => <article key={item.id}><div><b>{item.name}</b><small>Когда карточка попадает в «{item.list_title}» → отметить выполненной</small></div><label><input type="checkbox" checked={item.enabled} onChange={() => toggle(item)} /> Вкл.</label><button type="button" onClick={() => remove(item)} aria-label={`Удалить ${item.name}`}>×</button></article>) : <p>Правил ещё нет.</p>}</div></section></div>;
}
