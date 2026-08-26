'use client';

import { useEffect, useMemo, useState } from 'react';
import './card-review-panel.css';
import './card-review-compact.css';

type Member = { id: string; username: string; avatar_url?: string | null };
type ReviewStatus = 'none' | 'requested' | 'approved' | 'changes_requested';
type Review = { status: ReviewStatus; reviewers: Member[]; updated_at: string | null };

const statusLabels: Record<ReviewStatus, string> = {
  none: 'Проверка не нужна',
  requested: 'Нужна проверка',
  approved: 'Одобрено',
  changes_requested: 'Нужны правки',
};

function csrfHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}

export default function CardReviewPanel({ cardId, canEdit, members, compact = false }: { cardId: string; canEdit: boolean; members: Member[]; compact?: boolean }) {
  const [review, setReview] = useState<Review>({ status: 'none', reviewers: [], updated_at: null });
  const [reviewerIds, setReviewerIds] = useState<string[]>([]);
  const [isReviewerPickerOpen, setReviewerPickerOpen] = useState(false);
  const [reviewerQuery, setReviewerQuery] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [isCompactOpen, setCompactOpen] = useState(false);
  const available = useMemo(() => members.filter((member) => !reviewerIds.includes(member.id) && member.username.toLocaleLowerCase().includes(reviewerQuery.trim().toLocaleLowerCase())), [members, reviewerIds, reviewerQuery]);

  useEffect(() => {
    let active = true;
    setError('');
    void fetch(`/v1/cards/${cardId}/review`)
      .then(async (response) => { if (!response.ok) throw new Error('load failed'); return response.json() as Promise<Review>; })
      .then((next) => { if (active) { setReview(next); setReviewerIds(next.reviewers.map((member) => member.id)); } })
      .catch(() => { if (active) setError('Не удалось загрузить проверку.'); });
    return () => { active = false; };
  }, [cardId]);

  function save(nextStatus = review.status, nextReviewerIds = reviewerIds) {
    if (!canEdit || saving) return;
    setSaving(true); setError('');
    void fetch(`/v1/cards/${cardId}/review`, { method: 'PUT', headers: csrfHeaders(), body: JSON.stringify({ status: nextStatus, reviewer_ids: nextReviewerIds }) })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось сохранить проверку.'); return response.json() as Promise<Review>; })
      .then((next) => { setReview(next); setReviewerIds(next.reviewers.map((member) => member.id)); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось сохранить проверку.'))
      .finally(() => setSaving(false));
  }

  const initials = (username: string) => username.slice(0, 2).toUpperCase();

  const controls = <>
    <header><div><h3>Проверка</h3><span>Статус и ответственные за приёмку</span></div>{canEdit ? <select aria-label="Статус проверки" value={review.status} disabled={saving} onChange={(event) => { const status = event.target.value as ReviewStatus; setReview((current) => ({ ...current, status })); save(status); }}>{(Object.keys(statusLabels) as ReviewStatus[]).map((status) => <option key={status} value={status}>{statusLabels[status]}</option>)}</select> : <b>{statusLabels[review.status]}</b>}</header>
    {(review.status !== 'none' || review.reviewers.length > 0) && <div className="review-members"><div className="review-members-heading"><span>Проверяющие</span>{canEdit && <button type="button" className={isReviewerPickerOpen ? 'reviewer-add active' : 'reviewer-add'} disabled={saving || available.length === 0} onClick={() => { setReviewerPickerOpen((current) => !current); setReviewerQuery(''); }} aria-expanded={isReviewerPickerOpen}>＋ Добавить</button>}</div><div className="reviewer-chips">{review.reviewers.map((member) => <button type="button" key={member.id} className="reviewer-chip" disabled={!canEdit || saving} title={canEdit ? `Убрать @${member.username}` : `@${member.username}`} onClick={() => { const next = reviewerIds.filter((id) => id !== member.id); setReviewerIds(next); save(review.status, next); }}>{member.avatar_url ? <img src={member.avatar_url} alt="" /> : <i>{initials(member.username)}</i>}<span>@{member.username}</span>{canEdit && <b aria-hidden="true">×</b>}</button>)}</div>{canEdit && isReviewerPickerOpen && <div className="reviewer-picker"><input autoFocus value={reviewerQuery} onChange={(event) => setReviewerQuery(event.target.value)} placeholder="Найти участника" aria-label="Поиск проверяющего" />{available.length ? <div>{available.map((member) => <button type="button" key={member.id} disabled={saving} onClick={() => { const next = [...reviewerIds, member.id]; setReviewerIds(next); setReviewerQuery(''); setReviewerPickerOpen(false); save(review.status, next); }}>{member.avatar_url ? <img src={member.avatar_url} alt="" /> : <i>{initials(member.username)}</i>}<span>@{member.username}</span><b>Добавить</b></button>)}</div> : <p>Подходящих участников нет.</p>}</div>}</div>}
    {error && <p role="alert">{error}</p>}
  </>;

  if (compact) return <section className={`card-review-panel compact status-${review.status} ${isCompactOpen ? 'open' : ''}`} aria-label="Проверка карточки"><button type="button" className="review-compact-trigger" title={statusLabels[review.status]} aria-label={statusLabels[review.status]} onClick={() => setCompactOpen((current) => !current)}><span aria-hidden="true">{review.status === 'approved' ? '✓' : review.status === 'changes_requested' ? '!' : review.status === 'requested' ? '?' : '○'}</span><small>{review.reviewers.length || ''}</small></button>{isCompactOpen && <div className="review-compact-popover">{controls}</div>}</section>;

  return <section className={`card-review-panel status-${review.status}`} aria-label="Проверка карточки">
    {controls}
  </section>;
}
