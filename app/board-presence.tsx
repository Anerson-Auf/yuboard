'use client';

import { useEffect, useMemo, useState } from 'react';
import './board-presence.css';

type Presence = { user_id: string; username: string; avatar_url: string | null; card_id: string | null; editing_description: boolean };

function csrfHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}

export default function BoardPresence({ boardId, currentUserId, activeCardId, editingDescription }: { boardId?: string | null; currentUserId?: string; activeCardId?: string | null; editingDescription: boolean }) {
  const [people, setPeople] = useState<Presence[]>([]);
  useEffect(() => {
    if (!boardId || !currentUserId) { setPeople([]); return; }
    let active = true;
    const refresh = () => {
      void fetch(`/v1/boards/${boardId}/presence`, { method: 'PUT', headers: csrfHeaders(), body: JSON.stringify({ card_id: activeCardId ?? null, editing_description: editingDescription }) })
        .then((response) => response.ok ? response.json() as Promise<Presence[]> : [])
        .then((next) => { if (active) setPeople(next); });
    };
    refresh();
    const timer = window.setInterval(refresh, 12_000);
    return () => { active = false; window.clearInterval(timer); };
  }, [activeCardId, boardId, currentUserId, editingDescription]);

  const others = useMemo(() => people.filter((person) => person.user_id !== currentUserId), [currentUserId, people]);
  const editors = others.filter((person) => person.card_id === activeCardId && person.editing_description);
  if (!boardId || !currentUserId) return null;
  return <div className="board-presence" title={others.length ? `На доске: ${others.map((person) => '@' + person.username).join(', ')}` : 'Сейчас на доске только вы'}>
    <span className="presence-dot" />
    <div className="presence-avatars">{others.slice(0, 3).map((person) => person.avatar_url ? <img src={person.avatar_url} alt={`@${person.username}`} key={person.user_id} /> : <i key={person.user_id}>{person.username.slice(0, 1).toUpperCase()}</i>)}</div>
    <span>{others.length ? `${others.length + 1} на доске` : 'Вы на доске'}</span>
    {editors.length > 0 && <b>@{editors[0].username}{editors.length > 1 ? ` и ещё ${editors.length - 1}` : ''} редактирует описание</b>}
  </div>;
}
