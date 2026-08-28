'use client';

import { useEffect, useMemo, useState } from 'react';
import './card-collaboration-panel.css';

type RelationType = 'blocks' | 'depends_on' | 'duplicate' | 'related' | 'part_of';
type Relation = { id: string; relation_type: RelationType; note: string; direction: 'incoming' | 'outgoing'; other_card_id: string; other_card_title: string; other_card_list_id: string; other_card_completed_at: string | null; created_at: string };
type Version = { id: string; description: string; author_name: string; created_at: string };
type Candidate = { id: string; title: string; listTitle: string };

const relationLabels: Record<RelationType, string> = { blocks: 'блокирует', depends_on: 'зависит от', duplicate: 'дубликат', related: 'связана с', part_of: 'является частью' };

function csrfCookie() {
  return document.cookie.split('; ').find((part) => part.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
}

function flowboardFetch(input: RequestInfo | URL, init: RequestInit = {}) {
  const method = (init.method ?? 'GET').toUpperCase();
  const headers = new Headers(init.headers);
  if (!['GET', 'HEAD', 'OPTIONS'].includes(method)) {
    const csrf = csrfCookie();
    if (csrf) headers.set('x-flowboard-csrf', csrf);
  }
  return window.fetch(input, { ...init, headers, credentials: 'include' });
}

async function responseError(response: Response, fallback: string) {
  const payload = await response.json().catch(() => null) as { message?: string } | null;
  return payload?.message ?? fallback;
}

export default function CardCollaborationPanel({ cardId, canEdit, candidates, onOpenCard, onDescriptionRestore, showExisting = true, showRelationCreator = canEdit, hideEmptyRelations = false }: { cardId: string; canEdit: boolean; candidates: Candidate[]; onOpenCard: (cardId: string) => void; onDescriptionRestore: (description: string) => void; showExisting?: boolean; showRelationCreator?: boolean; hideEmptyRelations?: boolean }) {
  const [relations, setRelations] = useState<Relation[]>([]);
  const [versions, setVersions] = useState<Version[]>([]);
  const [targetId, setTargetId] = useState('');
  const [implementationTargetId, setImplementationTargetId] = useState('');
  const [relationType, setRelationType] = useState<RelationType>('related');
  const [relationNote, setRelationNote] = useState('');
  const [activeSection, setActiveSection] = useState<'relations' | 'history'>('relations');
  const [isSaving, setSaving] = useState(false);
  const [togglingImplementationId, setTogglingImplementationId] = useState<string | null>(null);
  const [error, setError] = useState('');

  const targets = useMemo(() => candidates.filter((card) => card.id !== cardId), [candidates, cardId]);
  const load = () => {
    void Promise.all([
      flowboardFetch(`/v1/cards/${cardId}/relations`).then((response) => response.ok ? response.json() as Promise<Relation[]> : []),
      flowboardFetch(`/v1/cards/${cardId}/description-versions`).then((response) => response.ok ? response.json() as Promise<Version[]> : []),
    ]).then(([nextRelations, nextVersions]) => { setRelations(nextRelations); setVersions(nextVersions); });
  };

  useEffect(() => { setTargetId(''); setImplementationTargetId(''); setActiveSection('relations'); load(); }, [cardId]);

  const createRelation = () => {
    if (!targetId || isSaving) return;
    setError('');
    setSaving(true);
    void flowboardFetch(`/v1/cards/${cardId}/relations`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ target_card_id: targetId, relation_type: relationType, note: relationNote }) })
      .then(async (response) => { if (!response.ok) throw new Error(await responseError(response, 'Не удалось создать связь.')); return response.json() as Promise<Relation>; })
      .then(() => { setTargetId(''); setRelationNote(''); load(); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось создать связь.'))
      .finally(() => setSaving(false));
  };

  const removeRelation = (relationId: string) => {
    setError('');
    void flowboardFetch(`/v1/cards/${cardId}/relations/${relationId}`, { method: 'DELETE' })
      .then(async (response) => {
        if (!response.ok) throw new Error(await responseError(response, 'Не удалось удалить связь.'));
        setRelations((current) => current.filter((relation) => relation.id !== relationId));
      })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось удалить связь.'));
  };

  // A part-of relation is stored on the implementation card (child -> parent),
  // so the parent can show an ordinary, live TODO list without a second
  // subtask model or a separate completion state.
  const addImplementationCard = () => {
    if (!implementationTargetId || isSaving) return;
    setError('');
    setSaving(true);
    void flowboardFetch(`/v1/cards/${implementationTargetId}/relations`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ target_card_id: cardId, relation_type: 'part_of', note: '' }) })
      .then(async (response) => { if (!response.ok) throw new Error(await responseError(response, 'Не удалось добавить задачу реализации.')); })
      .then(() => { setImplementationTargetId(''); load(); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось добавить задачу реализации.'))
      .finally(() => setSaving(false));
  };

  const toggleImplementationCard = (relation: Relation) => {
    if (!canEdit || togglingImplementationId) return;
    const wasCompleted = Boolean(relation.other_card_completed_at);
    setError('');
    setTogglingImplementationId(relation.id);
    setRelations((current) => current.map((item) => item.id === relation.id ? { ...item, other_card_completed_at: wasCompleted ? null : new Date().toISOString() } : item));
    void flowboardFetch(`/v1/cards/${relation.other_card_id}/completion`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ is_completed: !wasCompleted }) })
      .then(async (response) => { if (!response.ok) throw new Error(await responseError(response, 'Не удалось изменить статус задачи.')); })
      .then(() => load())
      .catch((reason) => {
        setRelations((current) => current.map((item) => item.id === relation.id ? { ...item, other_card_completed_at: wasCompleted ? relation.other_card_completed_at : null } : item));
        setError(reason instanceof Error ? reason.message : 'Не удалось изменить статус задачи.');
      })
      .finally(() => setTogglingImplementationId(null));
  };

  const restoreVersion = (version: Version) => {
    if (!canEdit || isSaving) return;
    setError('');
    setSaving(true);
    void flowboardFetch(`/v1/cards/${cardId}/description-versions/${version.id}/restore`, { method: 'POST' })
      .then(async (response) => { if (!response.ok) throw new Error(await responseError(response, 'Не удалось восстановить описание.')); return response.json() as Promise<{ description: string }>; })
      .then((result) => { onDescriptionRestore(result.description); load(); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось восстановить описание.'))
      .finally(() => setSaving(false));
  };

  const blockers = relations.filter((relation) => relation.relation_type === 'blocks' && relation.direction === 'incoming' && !relation.other_card_completed_at);
  const implementationRelations = relations.filter((relation) => relation.relation_type === 'part_of' && relation.direction === 'incoming');
  const ordinaryRelations = relations.filter((relation) => !(relation.relation_type === 'part_of' && relation.direction === 'incoming'));
  const completedImplementationCount = implementationRelations.filter((relation) => relation.other_card_completed_at).length;
  if (!showExisting && !showRelationCreator) return null;
  if (!showRelationCreator && !hideEmptyRelations) return null;
  if (hideEmptyRelations && !relations.length) return null;
  return <section className={`card-collaboration-panel ${showExisting ? '' : 'collaboration-create-only'}`} aria-label="Связи и история описания">
    <header><div><button type="button" className={activeSection === 'relations' ? 'active' : ''} onClick={() => setActiveSection('relations')}>Связи</button><button type="button" className={activeSection === 'history' ? 'active' : ''} onClick={() => setActiveSection('history')}>История описания</button></div>{blockers.length > 0 && <b className="card-blocked">Заблокировано: {blockers.length}</b>}</header>
    {activeSection === 'relations' ? <div className="relation-content">
      {blockers.length > 0 && <p className="relation-warning">Эта задача не может быть завершена, пока не закрыты: {blockers.map((relation) => relation.other_card_title).join(', ')}.</p>}
      {implementationRelations.length > 0 && <section className="implementation-todos" aria-label="Задачи для реализации"><header><div><b>Реализация</b><small>{completedImplementationCount} из {implementationRelations.length}</small></div><progress value={completedImplementationCount} max={implementationRelations.length} /></header><div className="implementation-todo-list">{implementationRelations.map((relation) => <article key={relation.id} className={relation.other_card_completed_at ? 'done' : ''}><button type="button" className="implementation-toggle" disabled={!canEdit || togglingImplementationId === relation.id} onClick={() => toggleImplementationCard(relation)} aria-label={relation.other_card_completed_at ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(relation.other_card_completed_at)}>{relation.other_card_completed_at && '✓'}</button><button type="button" className="implementation-open" onClick={() => onOpenCard(relation.other_card_id)}><b>{relation.other_card_title}</b>{relation.note && <small>{relation.note}</small>}<span>{relation.other_card_completed_at ? 'Выполнена' : 'В работе'}</span></button>{canEdit && <button type="button" className="relation-remove" onClick={() => removeRelation(relation.id)} aria-label="Убрать из реализации">×</button>}</article>)}</div></section>}
      {ordinaryRelations.length ? <div className="relation-list">{ordinaryRelations.map((relation) => <article key={relation.id}><button type="button" onClick={() => onOpenCard(relation.other_card_id)}><b>{relationLabels[relation.relation_type]}{relation.direction === 'incoming' ? ' эта задача' : ''}</b><span>{relation.other_card_title}</span>{relation.note && <small>{relation.note}</small>}<small>{relation.other_card_completed_at ? 'Выполнена' : 'Активна'}</small></button>{canEdit && <button type="button" className="relation-remove" onClick={() => removeRelation(relation.id)} aria-label="Удалить связь">×</button>}</article>)}</div> : !hideEmptyRelations && !implementationRelations.length && <p className="collaboration-empty">Связей пока нет.</p>}
      {showRelationCreator && <><div className="relation-create"><select value={relationType} onChange={(event) => setRelationType(event.target.value as RelationType)} aria-label="Тип связи">{Object.entries(relationLabels).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select><select value={targetId} onChange={(event) => setTargetId(event.target.value)} aria-label="Связать с карточкой"><option value="">Выберите карточку…</option>{targets.map((target) => <option value={target.id} key={target.id}>{target.listTitle} · {target.title}</option>)}</select><button type="button" disabled={!targetId || isSaving} onClick={createRelation}>Связать</button></div><input className="relation-note-input" value={relationNote} maxLength={500} onChange={(event) => setRelationNote(event.target.value)} placeholder="Пояснение (необязательно)" aria-label="Пояснение связи" /><div className="implementation-create"><label htmlFor={`implementation-card-${cardId}`}>Задача для реализации</label><select id={`implementation-card-${cardId}`} value={implementationTargetId} onChange={(event) => setImplementationTargetId(event.target.value)} aria-label="Добавить задачу в реализацию"><option value="">Выберите карточку…</option>{targets.map((target) => <option value={target.id} key={target.id}>{target.listTitle} · {target.title}</option>)}</select><button type="button" disabled={!implementationTargetId || isSaving} onClick={addImplementationCard}>Добавить</button></div>{error && <p className="relation-error" role="alert">{error}</p>}</>}
    </div> : <div className="description-history">{versions.length ? versions.map((version) => <article key={version.id}><div><b>@{version.author_name}</b><time>{new Date(version.created_at).toLocaleString('ru-RU')}</time><p>{version.description || 'Пустое описание'}</p></div>{canEdit && <button type="button" disabled={isSaving} onClick={() => restoreVersion(version)}>Восстановить</button>}</article>) : <p className="collaboration-empty">Правок описания пока нет.</p>}</div>}
  </section>;
}
