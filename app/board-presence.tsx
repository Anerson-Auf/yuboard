'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import './board-presence.css';

export type PresenceLocation = 'board' | 'card' | 'diagram';
export type Presence = { user_id: string; username: string; avatar_url: string | null; card_id: string | null; card_title: string | null; editing_description: boolean; location: PresenceLocation };

const PRESENCE_HEARTBEAT_MS = 6_000;
const PRESENCE_IDLE_MS = 90_000;
const PRESENCE_ACTIVITY_SIGNAL_MS = 4_000;

function csrfHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}

export function useBoardPresence({ boardId, currentUserId, activeCardId, editingDescription, location, isBoardOpen }: { boardId?: string | null; currentUserId?: string; activeCardId?: string | null; editingDescription: boolean; location: PresenceLocation; isBoardOpen: boolean }) {
  const [people, setPeople] = useState<Presence[]>([]);
  const [activityRevision, setActivityRevision] = useState(0);
  const lastActivityAtRef = useRef(0);
  const lastActivitySignalAtRef = useRef(0);
  const presenceSentRef = useRef(false);

  useEffect(() => {
    if (!boardId || !currentUserId || !isBoardOpen) {
      lastActivityAtRef.current = 0;
      presenceSentRef.current = false;
      setPeople([]);
      return;
    }
    const markActivity = () => {
      if (document.visibilityState !== 'visible') return;
      const now = Date.now();
      lastActivityAtRef.current = now;
      if (!presenceSentRef.current || now - lastActivitySignalAtRef.current >= PRESENCE_ACTIVITY_SIGNAL_MS) {
        lastActivitySignalAtRef.current = now;
        setActivityRevision((current) => current + 1);
      }
    };
    const handleVisibility = () => {
      if (document.visibilityState === 'visible') markActivity();
      else {
        lastActivityAtRef.current = 0;
        setActivityRevision((current) => current + 1);
      }
    };
    markActivity();
    const activityEvents: Array<keyof DocumentEventMap> = ['pointerdown', 'keydown', 'touchstart', 'wheel'];
    activityEvents.forEach((eventName) => document.addEventListener(eventName, markActivity, { passive: true }));
    document.addEventListener('visibilitychange', handleVisibility);
    return () => {
      activityEvents.forEach((eventName) => document.removeEventListener(eventName, markActivity));
      document.removeEventListener('visibilitychange', handleVisibility);
    };
  }, [boardId, currentUserId, isBoardOpen]);

  useEffect(() => {
    if (!boardId || !currentUserId || !isBoardOpen) { setPeople([]); return; }
    let active = true;
    const snapshot = () => {
      void fetch(`/v1/boards/${boardId}/presence`)
        .then((response) => response.ok ? response.json() as Promise<Presence[]> : [])
        .then((next) => { if (active) setPeople(next); });
    };
    const leave = () => {
      if (!presenceSentRef.current) return;
      presenceSentRef.current = false;
      void fetch(`/v1/boards/${boardId}/presence`, { method: 'DELETE', headers: csrfHeaders(), keepalive: true })
        .finally(snapshot);
    };
    const refresh = () => {
      const activeRecently = document.visibilityState === 'visible' && Date.now() - lastActivityAtRef.current <= PRESENCE_IDLE_MS;
      if (!activeRecently) { leave(); return; }
      void fetch(`/v1/boards/${boardId}/presence`, { method: 'PUT', headers: csrfHeaders(), body: JSON.stringify({ card_id: activeCardId ?? null, editing_description: editingDescription, location }) })
        .then((response) => response.ok ? response.json() as Promise<Presence[]> : [])
        .then((next) => { if (active) { presenceSentRef.current = true; setPeople(next); } });
    };
    refresh();
    const timer = window.setInterval(refresh, PRESENCE_HEARTBEAT_MS);
    return () => { active = false; window.clearInterval(timer); };
  }, [activeCardId, activityRevision, boardId, currentUserId, editingDescription, isBoardOpen, location]);

  useEffect(() => {
    if (!boardId || !currentUserId || !isBoardOpen) return;
    const leave = () => {
      presenceSentRef.current = false;
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

function presenceLocationLabel(person: Presence) {
  const card = person.card_title ? ` «${person.card_title}»` : '';
  if (person.location === 'diagram') return `В схеме карточки${card}`;
  if (person.editing_description) return `Редактирует описание карточки${card}`;
  if (person.location === 'card' || person.card_id) return `В карточке${card}`;
  return 'На главной странице доски';
}

export default function BoardPresence({ people, currentUserId, onPersonClick }: { people: Presence[]; currentUserId: string; onPersonClick?: (person: Presence) => void }) {
  const [isOpen, setOpen] = useState(false);
  const everyone = useMemo(() => [...people].sort((left, right) => (left.user_id === currentUserId ? -1 : right.user_id === currentUserId ? 1 : left.username.localeCompare(right.username, 'ru'))), [currentUserId, people]);
  const others = useMemo(() => everyone.filter((person) => person.user_id !== currentUserId), [currentUserId, everyone]);
  const count = everyone.length || 1;
  return <div className="board-presence-control" onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}>
    <button className="board-presence" type="button" onClick={() => setOpen((current) => !current)} aria-expanded={isOpen} aria-haspopup="dialog" title={others.length ? `На доске: ${others.map((person) => '@' + person.username).join(', ')}` : 'Сейчас на доске только вы'}>
      <span className="presence-dot" />
      <PresenceFaces people={others} />
      <span>{count > 1 ? `${count} на доске` : 'Вы на доске'}</span>
    </button>
    {isOpen && <div className="board-presence-popover" role="dialog" aria-label="Кто сейчас на доске">
      <div className="board-presence-heading"><b>Сейчас на доске</b><span>{count}</span></div>
      <div className="board-presence-list">{everyone.map((person) => <button key={person.user_id} type="button" onClick={() => { setOpen(false); onPersonClick?.(person); }}>
        {person.avatar_url ? <img src={person.avatar_url} alt="" /> : <i>{person.username.slice(0, 1).toUpperCase()}</i>}
        <span><b>@{person.username}{person.user_id === currentUserId && ' · вы'}</b><small>{presenceLocationLabel(person)}</small></span>
      </button>)}</div>
      <small className="board-presence-hint">Нажмите на участника, чтобы открыть его активность.</small>
    </div>}
  </div>;
}

export function CardEditingPresence({ people, currentUserId, cardId }: { people: Presence[]; currentUserId: string; cardId: string }) {
  const editors = people.filter((person) => person.user_id !== currentUserId && person.card_id === cardId && person.editing_description);
  if (!editors.length) return null;
  return <p className="card-editing-presence" role="status"><span className="presence-dot" /><PresenceFaces people={editors} /><span>@{editors.map((person) => person.username).join(', @')} {editors.length === 1 ? 'редактирует' : 'редактируют'} описание</span></p>;
}
