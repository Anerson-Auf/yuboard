'use client';

import { useEffect, useState } from 'react';
import './card-schedule-fields.css';

function dateValue(value?: string) {
  if (!value) return '';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '' : `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function timestampForDate(value: string, hour: string) {
  return `${value}T${hour}:00:00.000Z`;
}

function jsonHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}

export default function CardScheduleFields({ cardId, startAt, dueAt, canEdit, onChange }: { cardId: string; startAt?: string; dueAt?: string; canEdit: boolean; onChange: (patch: { startAt?: string; dueAt?: string }) => void }) {
  const [start, setStart] = useState(dateValue(startAt));
  const [due, setDue] = useState(dateValue(dueAt));
  const [saving, setSaving] = useState(false);
  useEffect(() => { setStart(dateValue(startAt)); setDue(dateValue(dueAt)); }, [cardId, dueAt, startAt]);
  const update = (field: 'start' | 'due', value: string) => {
    if (!canEdit || saving) return;
    const nextStart = field === 'start' ? value : start;
    const nextDue = field === 'due' ? value : due;
    if (nextStart && nextDue && nextStart > nextDue) return;
    setSaving(true);
    const request = field === 'start'
      ? fetch(`/v1/cards/${cardId}`, { method: 'PATCH', headers: jsonHeaders(), body: JSON.stringify({ start_at: value ? timestampForDate(value, '09') : null }) })
      : fetch(`/v1/cards/${cardId}/due-date`, value ? { method: 'PATCH', headers: jsonHeaders(), body: JSON.stringify({ due_at: timestampForDate(value, '18') }) } : { method: 'DELETE', headers: jsonHeaders() });
    void request.then((response) => { if (!response.ok) throw new Error('date save failed'); if (field === 'start') { setStart(value); onChange({ startAt: value ? timestampForDate(value, '09') : undefined }); } else { setDue(value); onChange({ dueAt: value ? timestampForDate(value, '18') : undefined }); } }).catch(() => { setStart(dateValue(startAt)); setDue(dateValue(dueAt)); }).finally(() => setSaving(false));
  };
  return <section className="card-schedule-fields" aria-label="Даты задачи"><span>Период</span><label>Начало<input type="date" value={start} disabled={!canEdit || saving} max={due || undefined} onChange={(event) => update('start', event.target.value)} /></label><label>Завершение<input type="date" value={due} disabled={!canEdit || saving} min={start || undefined} onChange={(event) => update('due', event.target.value)} /></label>{start && due && <small>{start === due ? 'Задача на один день' : `${start} — ${due}`}</small>}</section>;
}
