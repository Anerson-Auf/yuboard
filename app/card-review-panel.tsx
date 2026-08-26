'use client';

import { useEffect, useMemo, useState } from 'react';
import './card-review-panel.css';

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

export default function CardReviewPanel({ cardId, canEdit, members }: { cardId: string; canEdit: boolean; members: Member[] }) {
  const [review, setReview] = useState<Review>({ status: 'none', reviewers: [], updated_at: null });
  const [reviewerIds, setReviewerIds] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const available = useMemo(() => members.filter((member) => !reviewerIds.includes(member.id)), [members, reviewerIds]);

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

  return <section className={`card-review-panel status-${review.status}`} aria-label="Проверка карточки">
    <header><h3>Проверка</h3>{canEdit ? <select aria-label="Статус проверки" value={review.status} disabled={saving} onChange={(event) => { const status = event.target.value as ReviewStatus; setReview((current) => ({ ...current, status })); save(status); }}>{(Object.keys(statusLabels) as ReviewStatus[]).map((status) => <option key={status} value={status}>{statusLabels[status]}</option>)}</select> : <b>{statusLabels[review.status]}</b>}</header>
    {(review.status !== 'none' || review.reviewers.length > 0) && <div className="review-members"><span>Проверяющие</span><div>{review.reviewers.map((member) => <button type="button" key={member.id} className="reviewer-chip" disabled={!canEdit || saving} title={canEdit ? `Убрать @${member.username}` : `@${member.username}`} onClick={() => { const next = reviewerIds.filter((id) => id !== member.id); setReviewerIds(next); save(review.status, next); }}>{member.avatar_url && <img src={member.avatar_url} alt="" />}@{member.username}{canEdit && ' ×'}</button>)}{canEdit && available.length > 0 && <select value="" disabled={saving} aria-label="Добавить проверяющего" onChange={(event) => { const id = event.target.value; if (!id) return; const next = [...reviewerIds, id]; setReviewerIds(next); save(review.status, next); }}><option value="">+ Проверяющий</option>{available.map((member) => <option value={member.id} key={member.id}>@{member.username}</option>)}</select>}</div></div>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
