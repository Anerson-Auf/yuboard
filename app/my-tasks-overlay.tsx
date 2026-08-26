'use client';

import { useEffect, useMemo, useState } from 'react';
import './my-tasks-overlay.css';

type Task = { id: string; board_id: string; board_title: string; list_title: string; title: string; priority: number; due_at: string | null; completed_at: string | null; updated_at: string };

function dateLabel(value: string | null) {
  return value ? new Date(value).toLocaleString('ru-RU', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' }) : 'Без срока';
}

export default function MyTasksOverlay({ onClose, onOpenTask }: { onClose: () => void; onOpenTask: (task: Task) => void }) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    void fetch('/v1/me/tasks').then(async (response) => { if (!response.ok) throw new Error('load failed'); return response.json() as Promise<Task[]>; })
      .then((items) => { if (active) setTasks(items); })
      .catch(() => { if (active) setError('Не удалось загрузить ваши задачи.'); })
      .finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, []);
  const groups = useMemo(() => ({ active: tasks.filter((task) => !task.completed_at), completed: tasks.filter((task) => task.completed_at) }), [tasks]);
  return <div className="modal-backdrop" role="presentation" onMouseDown={onClose}><section className="my-tasks-overlay" role="dialog" aria-modal="true" aria-label="Мои задачи" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" type="button" onClick={onClose} aria-label="Закрыть">×</button><p className="eyebrow">ЛИЧНАЯ ОЧЕРЕДЬ</p><h2>Мои задачи</h2><p>Все карточки, где вы исполнитель — из всех доступных проектов.</p>{loading ? <p className="detail-loading">Загружаем задачи…</p> : error ? <p className="my-tasks-error">{error}</p> : tasks.length === 0 ? <p className="empty-comments">У вас пока нет назначенных задач.</p> : <div className="my-tasks-groups">{([['active', 'В работе'], ['completed', 'Выполнено']] as const).map(([key, label]) => groups[key].length > 0 && <section key={key}><h3>{label} <span>{groups[key].length}</span></h3>{groups[key].map((task) => <button type="button" key={task.id} onClick={() => onOpenTask(task)}><span className={`my-task-priority priority-${task.priority}`}>{task.priority ? '▮'.repeat(task.priority) : '—'}</span><span><b>{task.title}</b><small>{task.board_title} · {task.list_title}</small></span><time>{dateLabel(task.due_at)}</time></button>)}</section>)}</div>}</section></div>;
}
