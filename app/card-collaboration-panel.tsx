'use client';

import { useEffect, useMemo, useRef, useState } from 'react';
import './card-collaboration-panel.css';

type RelationType = 'blocks' | 'depends_on' | 'duplicate' | 'related' | 'part_of';
type Relation = { id: string; relation_type: RelationType; note: string; direction: 'incoming' | 'outgoing'; other_card_id: string; other_card_title: string; other_card_list_id: string; other_card_completed_at: string | null; created_at: string };
type Version = { id: string; description: string; author_name: string; created_at: string };
type SnapshotField = 'title' | 'description' | 'priority' | 'is_frozen' | 'start_at' | 'due_at' | 'completed_at' | 'cover_attachment_id' | 'cover_mode' | 'background_image_url';
type Snapshot = { id: number; state: Record<string, unknown>; changed_fields: SnapshotField[]; created_at: string };
type Candidate = { id: string; title: string; listTitle: string };

const relationLabels: Record<RelationType, string> = { blocks: 'блокирует', depends_on: 'зависит от', duplicate: 'дубликат', related: 'связана с', part_of: 'является частью' };
const snapshotFieldLabels: Record<SnapshotField, string> = { title: 'название', description: 'описание', priority: 'приоритет', is_frozen: 'заморозка', start_at: 'начало', due_at: 'дедлайн', completed_at: 'готовность', cover_attachment_id: 'обложка', cover_mode: 'режим обложки', background_image_url: 'фон' };

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
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [targetId, setTargetId] = useState('');
  const [targetSearch, setTargetSearch] = useState('');
  const [isTargetPickerOpen, setTargetPickerOpen] = useState(false);
  const [relationType, setRelationType] = useState<RelationType>('related');
  const [relationNote, setRelationNote] = useState('');
  const [activeSection, setActiveSection] = useState<'relations' | 'history' | 'snapshots'>('relations');
  const [isSaving, setSaving] = useState(false);
  const [togglingImplementationId, setTogglingImplementationId] = useState<string | null>(null);
  const [error, setError] = useState('');
  const targetPickerRef = useRef<HTMLDivElement>(null);

  const targets = useMemo(() => candidates.filter((card) => card.id !== cardId), [candidates, cardId]);
  const targetMatches = useMemo(() => {
    const needle = targetSearch.trim().toLocaleLowerCase('ru-RU');
    return targets.filter((card) => !needle || `${card.title} ${card.listTitle}`.toLocaleLowerCase('ru-RU').includes(needle));
  }, [targetSearch, targets]);
  const selectedTarget = targets.find((card) => card.id === targetId);
  const load = () => {
    void Promise.all([
      flowboardFetch(`/v1/cards/${cardId}/relations`).then((response) => response.ok ? response.json() as Promise<Relation[]> : []),
      flowboardFetch(`/v1/cards/${cardId}/description-versions`).then((response) => response.ok ? response.json() as Promise<Version[]> : []),
      flowboardFetch(`/v1/cards/${cardId}/state-snapshots`).then((response) => response.ok ? response.json() as Promise<Snapshot[]> : []),
    ]).then(([nextRelations, nextVersions, nextSnapshots]) => { setRelations(nextRelations); setVersions(nextVersions); setSnapshots(nextSnapshots); });
  };

  useEffect(() => { setTargetId(''); setTargetSearch(''); setTargetPickerOpen(false); setActiveSection('relations'); load(); }, [cardId]);

  useEffect(() => {
    if (!isTargetPickerOpen) return;
    const closePicker = (event: PointerEvent) => {
      if (event.target instanceof Node && !targetPickerRef.current?.contains(event.target)) setTargetPickerOpen(false);
    };
    window.addEventListener('pointerdown', closePicker);
    return () => window.removeEventListener('pointerdown', closePicker);
  }, [isTargetPickerOpen]);

  const createRelation = () => {
    if (!targetId || isSaving) return;
    setError('');
    setSaving(true);
    void flowboardFetch(`/v1/cards/${cardId}/relations`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ target_card_id: targetId, relation_type: relationType, note: relationNote }) })
      .then(async (response) => { if (!response.ok) throw new Error(await responseError(response, 'Не удалось создать связь.')); return response.json() as Promise<Relation>; })
      .then(() => { setTargetId(''); setTargetSearch(''); setTargetPickerOpen(false); setRelationNote(''); load(); })
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

  const restoreSnapshotField = (snapshot: Snapshot, field: SnapshotField) => {
    if (!canEdit || isSaving) return;
    setError('');
    setSaving(true);
    void flowboardFetch(`/v1/cards/${cardId}/state-snapshots/${snapshot.id}/restore`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ fields: [field] }) })
      .then(async (response) => { if (!response.ok) throw new Error(await responseError(response, 'Не удалось восстановить поле.')); return response.json() as Promise<{ description: string }>; })
      .then((result) => { if (field === 'description') onDescriptionRestore(result.description); load(); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось восстановить поле.'))
      .finally(() => setSaving(false));
  };

  const blockers = relations.filter((relation) => relation.relation_type === 'blocks' && relation.direction === 'incoming' && !relation.other_card_completed_at);
  const implementationRelations = relations.filter((relation) => relation.relation_type === 'part_of' && relation.direction === 'incoming');
  const ordinaryRelations = relations.filter((relation) => !(relation.relation_type === 'part_of' && relation.direction === 'incoming'));
  const completedImplementationCount = implementationRelations.filter((relation) => relation.other_card_completed_at).length;
  if (!showExisting && !showRelationCreator) return null;
  if (!showRelationCreator && !hideEmptyRelations) return null;
  if (hideEmptyRelations && !relations.length) return null;
  return <section className={`card-collaboration-panel ${showExisting ? '' : 'collaboration-create-only'}`} aria-label="Связи и история карточки">
    <header><div><button type="button" className={activeSection === 'relations' ? 'active' : ''} onClick={() => setActiveSection('relations')}>Связи</button><button type="button" className={activeSection === 'history' ? 'active' : ''} onClick={() => setActiveSection('history')}>История описания</button><button type="button" className={activeSection === 'snapshots' ? 'active' : ''} onClick={() => setActiveSection('snapshots')}>Снимки</button></div>{blockers.length > 0 && <b className="card-blocked">Заблокировано: {blockers.length}</b>}</header>
    {activeSection === 'relations' ? <div className="relation-content">
      {blockers.length > 0 && <p className="relation-warning">Эта задача не может быть завершена, пока не закрыты: {blockers.map((relation) => relation.other_card_title).join(', ')}.</p>}
      {showExisting && implementationRelations.length > 0 && <section className="implementation-todos" aria-label="Задачи для реализации"><header><div><b>Реализация</b><small>{completedImplementationCount} из {implementationRelations.length}</small></div><progress value={completedImplementationCount} max={implementationRelations.length} /></header><div className="implementation-todo-list">{implementationRelations.map((relation) => <article key={relation.id} className={relation.other_card_completed_at ? 'done' : ''}><button type="button" className="implementation-toggle" disabled={!canEdit || togglingImplementationId === relation.id} onClick={() => toggleImplementationCard(relation)} aria-label={relation.other_card_completed_at ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(relation.other_card_completed_at)}>{relation.other_card_completed_at && '✓'}</button><button type="button" className="implementation-open" onClick={() => onOpenCard(relation.other_card_id)}><b>{relation.other_card_title}</b>{relation.note && <small>{relation.note}</small>}<span>{relation.other_card_completed_at ? 'Выполнена' : 'В работе'}</span></button>{canEdit && <button type="button" className="relation-remove" onClick={() => removeRelation(relation.id)} aria-label="Убрать из реализации">×</button>}</article>)}</div></section>}
      {showExisting && (ordinaryRelations.length ? <div className="relation-list">{ordinaryRelations.map((relation) => <article key={relation.id}><button type="button" onClick={() => onOpenCard(relation.other_card_id)}><b>{relationLabels[relation.relation_type]}{relation.direction === 'incoming' ? ' эта задача' : ''}</b><span>{relation.other_card_title}</span>{relation.note && <small>{relation.note}</small>}<small>{relation.other_card_completed_at ? 'Выполнена' : 'Активна'}</small></button>{canEdit && <button type="button" className="relation-remove" onClick={() => removeRelation(relation.id)} aria-label="Удалить связь">×</button>}</article>)}</div> : !hideEmptyRelations && !implementationRelations.length && <p className="collaboration-empty">Связей пока нет.</p>)}
      {showRelationCreator && <section className="relation-composer" aria-label="Создать связь"><header><div><b>Новая связь</b><small>Текущая карточка → выбранная</small></div></header><div className={`relation-create ${relationType === 'part_of' ? 'relation-create-part-of' : ''}`}><label className="relation-type-field"><span>Тип связи</span><select value={relationType} onChange={(event) => setRelationType(event.target.value as RelationType)} aria-label="Тип связи">{Object.entries(relationLabels).map(([value, label]) => <option value={value} key={value}>{label}</option>)}</select></label><span className="relation-direction" aria-hidden="true">→</span><div className="relation-target-picker" ref={targetPickerRef}><span>{relationType === 'part_of' ? 'Основная карточка' : 'Карточка'}</span><button type="button" className={selectedTarget ? 'selected' : ''} onClick={() => { setTargetSearch(''); setTargetPickerOpen((current) => !current); }} aria-expanded={isTargetPickerOpen} aria-haspopup="listbox">{selectedTarget ? <><small>{selectedTarget.listTitle}</small><b>{selectedTarget.title}</b></> : <><b>{relationType === 'part_of' ? 'Найти основную карточку' : 'Найти карточку'}</b><small>По названию или колонке</small></>}<i>⌄</i></button>{isTargetPickerOpen && <div className="relation-target-options" role="listbox"><input autoFocus value={targetSearch} onChange={(event) => setTargetSearch(event.target.value)} placeholder="Введите название или колонку…" aria-label="Поиск карточки" />{targetMatches.length ? targetMatches.map((target) => <button key={target.id} type="button" role="option" aria-selected={target.id === targetId} onClick={() => { setTargetId(target.id); setTargetPickerOpen(false); }}><small>{target.listTitle}</small><b>{target.title}</b></button>) : <p>Ничего не найдено.</p>}</div>}</div><button type="button" className="relation-submit" disabled={!targetId || isSaving} onClick={createRelation}>{relationType === 'part_of' ? 'Добавить в реализацию' : 'Создать связь'}</button></div>{relationType === 'part_of' && <p className="relation-part-of-hint">Текущая карточка появится в TODO выбранной основной карточки.</p>}<label className="relation-note-field"><span>Пояснение <small>необязательно</small></span><input className="relation-note-input" value={relationNote} maxLength={500} onChange={(event) => setRelationNote(event.target.value)} placeholder="Например, ждём макет" aria-label="Пояснение связи" /></label>{error && <p className="relation-error" role="alert">{error}</p>}</section>}
    </div> : activeSection === 'history' ? <div className="description-history">{versions.length ? versions.map((version) => <article key={version.id}><div><b>@{version.author_name}</b><time>{new Date(version.created_at).toLocaleString('ru-RU')}</time><p>{version.description || 'Пустое описание'}</p></div>{canEdit && <button type="button" disabled={isSaving} onClick={() => restoreVersion(version)}>Восстановить</button>}</article>) : <p className="collaboration-empty">Правок описания пока нет.</p>}</div> : <div className="snapshot-history">{snapshots.length ? snapshots.map((snapshot) => <article key={snapshot.id}><time>{new Date(snapshot.created_at).toLocaleString('ru-RU')}</time><div>{snapshot.changed_fields.map((field) => <button key={field} type="button" disabled={!canEdit || isSaving} onClick={() => restoreSnapshotField(snapshot, field)}>↶ {snapshotFieldLabels[field]}</button>)}</div></article>) : <p className="collaboration-empty">Снимки появятся после изменений карточки.</p>}</div>}
  </section>;
}
