'use client';

import { useEffect, useMemo, useState } from 'react';
import './board-presence.css';

export type Presence = { user_id: string; username: string; avatar_url: string | null; card_id: string | null; editing_description: boolean };

function csrfHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}

export function useBoardPresence({ boardId, currentUserId, activeCardId, editingDescription, isBoardOpen }: { boardId?: string | null; currentUserId?: string; activeCardId?: string | null; editingDescription: boolean; isBoardOpen: boolean }) {
  const [people, setPeople] = useState<Presence[]>([]);
  useEffect(() => {
    if (!boardId || !currentUserId || !isBoardOpen) { setPeople([]); return; }
    let active = true;
    const refresh = () => {
      void fetch(`/v1/boards/${boardId}/presence`, { method: 'PUT', headers: csrfHeaders(), body: JSON.stringify({ card_id: activeCardId ?? null, editing_description: editingDescription }) })
        .then((response) => response.ok ? response.json() as Promise<Presence[]> : [])
        .then((next) => { if (active) setPeople(next); });
    };
    refresh();
    const timer = window.setInterval(refresh, 6_000);
    return () => { active = false; window.clearInterval(timer); };
  }, [activeCardId, boardId, currentUserId, editingDescription, isBoardOpen]);

  useEffect(() => {
    if (!boardId || !currentUserId || !isBoardOpen) return;
    const leave = () => {
      void fetch(`/v1/boards/${boardId}/presence`, { method: 'DELETE', headers: csrfHeaders(), keepalive: true });
    };
    window.addEventListener('pagehide', leave);
    return () => { window.removeEventListener('pagehide', leave); leave(); };
  }, [boardId, currentUserId, isBoardOpen]);
  return people;
}

function PresenceFaces({ people }: { people: Presence[] }) {
  return <div className="presence-avatars">{people.slice(0, 3).map((person) => person.avatar_url ? <img src={person.avatar_url} alt={`@${person.username}`} key={person.user_id} /> : <i key={person.user_id}>{person.username.slice(0, 1).toUpperCase()}</i>)}</div>;
}

export default function BoardPresence({ people, currentUserId }: { people: Presence[]; currentUserId: string }) {
  const others = useMemo(() => people.filter((person) => person.user_id !== currentUserId), [currentUserId, people]);
  return <div className="board-presence" title={others.length ? `На доске: ${others.map((person) => '@' + person.username).join(', ')}` : 'Сейчас на доске только вы'}>
    <span className="presence-dot" />
    <PresenceFaces people={others} />
    <span>{others.length ? `${others.length + 1} на доске` : 'Вы на доске'}</span>
  </div>;
}

export function CardEditingPresence({ people, currentUserId, cardId }: { people: Presence[]; currentUserId: string; cardId: string }) {
  const editors = people.filter((person) => person.user_id !== currentUserId && person.card_id === cardId && person.editing_description);
  if (!editors.length) return null;
  return <p className="card-editing-presence" role="status"><span className="presence-dot" /><PresenceFaces people={editors} /><span>@{editors.map((person) => person.username).join(', @')} {editors.length === 1 ? 'редактирует' : 'редактируют'} описание</span></p>;
}
