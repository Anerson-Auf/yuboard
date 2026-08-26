'use client';

import { useEffect, useState } from 'react';
import './automations-overlay.css';

type AutomationAction = 'complete_card' | 'reopen_card' | 'set_priority' | 'archive_card';
type Automation = { id: string; name: string; list_id: string; list_title: string; action_type: AutomationAction; action_priority: number | null; enabled: boolean; created_at: string };
type Column = { id: string | number; title: string };

const actionLabels: Record<AutomationAction, string> = {
  complete_card: 'отметить выполненной',
  reopen_card: 'снять отметку «Выполнено»',
  set_priority: 'установить приоритет',
  archive_card: 'архивировать карточку',
};

function headers() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}

function actionDescription(item: Pick<Automation, 'action_type' | 'action_priority'>) {
  return item.action_type === 'set_priority'
    ? `${actionLabels[item.action_type]}: ${item.action_priority ?? 0}/5`
    : actionLabels[item.action_type];
}

export default function AutomationsOverlay({ boardId, columns, onClose }: { boardId: string; columns: Column[]; onClose: () => void }) {
  const [items, setItems] = useState<Automation[]>([]);
  const [name, setName] = useState('');
  const [listId, setListId] = useState('');
  const [actionType, setActionType] = useState<AutomationAction>('complete_card');
  const [priority, setPriority] = useState(3);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  const load = () => {
    setLoading(true);
    void fetch(`/v1/boards/${boardId}/automations`)
      .then((response) => response.ok ? response.json() as Promise<Automation[]> : Promise.reject(new Error('Не удалось загрузить правила.')))
      .then(setItems)
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось загрузить правила.'))
      .finally(() => setLoading(false));
  };

  useEffect(load, [boardId]);

  const create = () => {
    if (!name.trim() || !listId) return;
    setError('');
    void fetch(`/v1/boards/${boardId}/automations`, {
      method: 'POST',
      headers: headers(),
      body: JSON.stringify({ name: name.trim(), list_id: listId, action_type: actionType, action_priority: actionType === 'set_priority' ? priority : undefined }),
    })
      .then(async (response) => {
        if (!response.ok) {
          const payload = await response.json().catch(() => null) as { message?: string } | null;
          throw new Error(payload?.message ?? 'Не удалось создать правило.');
        }
        return response.json() as Promise<Automation>;
      })
      .then((item) => { setItems((current) => [item, ...current]); setName(''); setListId(''); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось создать правило.'));
  };

  const toggle = (item: Automation) => {
    setError('');
    void fetch(`/v1/boards/${boardId}/automations/${item.id}`, { method: 'PATCH', headers: headers(), body: JSON.stringify({ enabled: !item.enabled }) })
      .then((response) => response.ok ? response.json() as Promise<Automation> : Promise.reject(new Error('Не удалось обновить правило.')))
      .then((updated) => setItems((current) => current.map((value) => value.id === updated.id ? updated : value)))
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось обновить правило.'));
  };

  const remove = (item: Automation) => {
    setError('');
    void fetch(`/v1/boards/${boardId}/automations/${item.id}`, { method: 'DELETE', headers: headers() })
      .then((response) => { if (!response.ok) throw new Error('Не удалось удалить правило.'); setItems((current) => current.filter((value) => value.id !== item.id)); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось удалить правило.'));
  };

  return <div className="automations-backdrop" role="presentation" onMouseDown={onClose}>
    <section className="automations-overlay" role="dialog" aria-modal="true" aria-label="Автоматизации" onMouseDown={(event) => event.stopPropagation()}>
      <header><div><p>АВТОМАТИЗАЦИИ</p><h2>Правила без кода</h2><span>Когда карточка попадает в колонку — выполнить выбранное действие.</span></div><button type="button" onClick={onClose} aria-label="Закрыть">×</button></header>
      <form onSubmit={(event) => { event.preventDefault(); create(); }}>
        <input value={name} onChange={(event) => setName(event.target.value)} placeholder="Название правила" maxLength={120} />
        <label>Когда попадёт в<select value={listId} onChange={(event) => setListId(event.target.value)}><option value="">Выберите колонку…</option>{columns.map((column) => <option value={column.id} key={column.id}>{column.title}</option>)}</select></label>
        <label>То<span className="automation-action-control"><select value={actionType} onChange={(event) => setActionType(event.target.value as AutomationAction)}>{Object.entries(actionLabels).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select>{actionType === 'set_priority' && <select value={priority} onChange={(event) => setPriority(Number(event.target.value))} aria-label="Уровень приоритета">{[0, 1, 2, 3, 4, 5].map((value) => <option value={value} key={value}>{value}/5</option>)}</select>}</span></label>
        <button disabled={!name.trim() || !listId}>Создать правило</button>
      </form>
      {error && <p className="automation-error" role="alert">{error}</p>}
      <div className="automation-list">{loading ? <p>Загружаем…</p> : items.length ? items.map((item) => <article key={item.id}><div><b>{item.name}</b><small>Когда карточка попадает в «{item.list_title}» → {actionDescription(item)}</small></div><label><input type="checkbox" checked={item.enabled} onChange={() => toggle(item)} /> Вкл.</label><button type="button" onClick={() => remove(item)} aria-label={`Удалить ${item.name}`}>×</button></article>) : <p>Правил ещё нет.</p>}</div>
    </section>
  </div>;
}
