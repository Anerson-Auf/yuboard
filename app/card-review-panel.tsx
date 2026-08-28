'use client';

import { useCallback, useEffect, useMemo, useState } from 'react';
import './card-review-panel.css';
import './card-review-compact.css';

type Member = { id: string; username: string; avatar_url?: string | null };
type ReviewStatus = 'none' | 'requested' | 'approved' | 'changes_requested' | 'rejected';
type DecisionStatus = 'approved' | 'changes_requested' | 'rejected';
type ReviewDecision = { reviewer_id: string; reviewer_username: string; reviewer_avatar_url?: string | null; status: DecisionStatus | null; reason: string | null; decided_at: string | null };
type Review = { status: ReviewStatus; reviewers: Member[]; decisions: ReviewDecision[]; requested_by: Member | null; updated_at: string | null };

const statusLabels: Record<ReviewStatus, string> = { none: 'Проверка не запрошена', requested: 'На проверке', approved: 'Одобрено', changes_requested: 'Нужны правки', rejected: 'Отклонено' };
const decisionLabels: Record<DecisionStatus, string> = { approved: 'Одобрил', changes_requested: 'Нужны правки', rejected: 'Отклонил' };

function csrfHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}
function Avatar({ member }: { member: Pick<Member, 'username' | 'avatar_url'> }) { return member.avatar_url ? <img src={member.avatar_url} alt="" /> : <i>{member.username.slice(0, 2).toUpperCase()}</i>; }

export default function CardReviewPanel({ cardId, canEdit, members, currentUserId, compact = false }: { cardId: string; canEdit: boolean; members: Member[]; currentUserId?: string; compact?: boolean }) {
  const [review, setReview] = useState<Review>({ status: 'none', reviewers: [], decisions: [], requested_by: null, updated_at: null });
  const [reviewerIds, setReviewerIds] = useState<string[]>([]);
  const [isConfiguring, setConfiguring] = useState(false);
  const [isReviewerPickerOpen, setReviewerPickerOpen] = useState(false);
  const [reviewerQuery, setReviewerQuery] = useState('');
  const [decisionDraft, setDecisionDraft] = useState<DecisionStatus | null>(null);
  const [decisionReason, setDecisionReason] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [isCompactOpen, setCompactOpen] = useState(false);
  const available = useMemo(() => members.filter((member) => !reviewerIds.includes(member.id) && member.username.toLocaleLowerCase().includes(reviewerQuery.trim().toLocaleLowerCase())), [members, reviewerIds, reviewerQuery]);
  const answered = review.decisions.filter((decision) => decision.status).length;
  const ownDecision = review.decisions.find((decision) => decision.reviewer_id === currentUserId);
  const canDecide = review.status === 'requested' && Boolean(ownDecision);
  const apply = (next: Review) => { setReview(next); setReviewerIds(next.reviewers.map((member) => member.id)); };
  const loadReview = useCallback(async (showError = false) => {
    try {
      const response = await fetch(`/v1/cards/${cardId}/review`);
      if (!response.ok) throw new Error('load failed');
      const next = await response.json() as Review;
      setReview(next); setReviewerIds(next.reviewers.map((member) => member.id));
    } catch {
      if (showError) setError('Не удалось загрузить review.');
    }
  }, [cardId]);

  useEffect(() => {
    if (!compact) return;
    setError(''); setDecisionDraft(null); setDecisionReason(''); setConfiguring(false);
    void loadReview(true);
  }, [cardId, compact, loadReview]);

  useEffect(() => {
    if (!compact || !isCompactOpen || isConfiguring || saving) return;
    const timer = window.setInterval(() => { void loadReview(); }, 5_000);
    return () => window.clearInterval(timer);
  }, [compact, isCompactOpen, isConfiguring, saving, loadReview]);

  function saveReview(status: 'requested' | 'none') {
    if (!canEdit || saving) return;
    setSaving(true); setError('');
    void fetch(`/v1/cards/${cardId}/review`, { method: 'PUT', headers: csrfHeaders(), body: JSON.stringify({ status, reviewer_ids: status === 'requested' ? reviewerIds : [] }) })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось сохранить review.'); return response.json() as Promise<Review>; })
      .then((next) => { apply(next); setConfiguring(false); setReviewerPickerOpen(false); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось сохранить review.'))
      .finally(() => setSaving(false));
  }
  function submitDecision(status: DecisionStatus) {
    if (!canDecide || saving) return;
    if (status !== 'approved' && !decisionReason.trim()) { setError('Укажите причину решения.'); return; }
    setSaving(true); setError('');
    void fetch(`/v1/cards/${cardId}/review/decision`, { method: 'PUT', headers: csrfHeaders(), body: JSON.stringify({ status, reason: decisionReason.trim() }) })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось отправить решение.'); return response.json() as Promise<Review>; })
      .then((next) => { apply(next); setDecisionDraft(null); setDecisionReason(''); })
      .catch((reason) => setError(reason instanceof Error ? reason.message : 'Не удалось отправить решение.'))
      .finally(() => setSaving(false));
  }
  function openConfig() { setReviewerIds(review.reviewers.map((member) => member.id)); setConfiguring(true); setDecisionDraft(null); setError(''); }
  function iconForStatus(status: ReviewStatus) { return status === 'approved' ? '✓' : status === 'changes_requested' ? '↶' : status === 'rejected' ? '×' : status === 'requested' ? '◷' : '○'; }

  const reviewerSetup = <div className="review-members"><div className="review-members-heading"><span>Проверяющие</span>{canEdit && <button type="button" className={isReviewerPickerOpen ? 'reviewer-add active' : 'reviewer-add'} disabled={saving} onClick={() => { setReviewerPickerOpen((current) => !current); setReviewerQuery(''); }} aria-expanded={isReviewerPickerOpen}>＋ Добавить</button>}</div><div className="reviewer-chips">{reviewerIds.map((id) => {
    const member = members.find((item) => item.id === id) ?? review.reviewers.find((item) => item.id === id);
    return member ? <button type="button" key={member.id} className="reviewer-chip" disabled={!canEdit || saving} title={canEdit ? `Убрать @${member.username}` : `@${member.username}`} onClick={() => setReviewerIds((current) => current.filter((currentId) => currentId !== member.id))}><Avatar member={member} /><span>@{member.username}</span>{canEdit && <b aria-hidden="true">×</b>}</button> : null;
  })}</div>{isReviewerPickerOpen && <div className="reviewer-picker"><input autoFocus value={reviewerQuery} onChange={(event) => setReviewerQuery(event.target.value)} placeholder="Найти участника" aria-label="Поиск проверяющего" />{available.length ? <div>{available.map((member) => <button type="button" key={member.id} disabled={saving} onClick={() => { setReviewerIds((current) => [...current, member.id]); setReviewerQuery(''); }}><Avatar member={member} /><span>@{member.username}</span><b>Добавить</b></button>)}</div> : <p>Подходящих участников нет.</p>}</div>}</div>;
  const decisions = <div className="review-decisions"><div className="review-progress"><span>Мнения проверяющих</span><b>{answered}/{review.reviewers.length}</b></div>{review.decisions.map((decision) => <article className={`review-decision ${decision.status ?? 'waiting'}`} key={decision.reviewer_id}><Avatar member={{ username: decision.reviewer_username, avatar_url: decision.reviewer_avatar_url }} /><div><b>@{decision.reviewer_username}</b><span>{decision.status ? decisionLabels[decision.status] : 'Ожидаем решение'}</span>{decision.reason && <p>{decision.reason}</p>}</div>{decision.status && <i aria-label={decisionLabels[decision.status]}>{decision.status === 'approved' ? '✓' : decision.status === 'changes_requested' ? '↶' : '×'}</i>}</article>)}</div>;
  const controls = <><header><div><h3>Review</h3><span>{review.status === 'requested' ? `Ожидаем все мнения · ${answered}/${review.reviewers.length}` : statusLabels[review.status]}</span></div><b>{statusLabels[review.status]}</b></header>{(isConfiguring || review.status === 'none') ? <><p className="review-explainer">Review завершится только после решения каждого выбранного человека.</p>{reviewerSetup}<div className="review-flow-actions"><button type="button" className="create-button" disabled={saving || !reviewerIds.length} onClick={() => saveReview('requested')}>{review.status === 'none' ? 'Отправить на проверку' : 'Запустить повторно'}</button>{review.status !== 'none' && <button type="button" className="secondary-button" disabled={saving} onClick={() => setConfiguring(false)}>Отмена</button>}</div></> : <>{decisions}{canDecide && !ownDecision?.status && <div className="review-decision-actions">{decisionDraft ? <><textarea autoFocus value={decisionReason} onChange={(event) => setDecisionReason(event.target.value)} maxLength={4000} placeholder={decisionDraft === 'rejected' ? 'Почему работа отклонена?' : 'Какие правки нужны?'} aria-label="Причина решения review" /><div><button type="button" className={decisionDraft === 'rejected' ? 'danger-action' : 'secondary-button'} disabled={saving || !decisionReason.trim()} onClick={() => submitDecision(decisionDraft)}>Отправить решение</button><button type="button" disabled={saving} onClick={() => { setDecisionDraft(null); setDecisionReason(''); }}>Отмена</button></div></> : <><button type="button" className="review-approve" disabled={saving} onClick={() => submitDecision('approved')}>Одобрить</button><button type="button" disabled={saving} onClick={() => setDecisionDraft('changes_requested')}>Нужны правки</button><button type="button" className="danger-action" disabled={saving} onClick={() => setDecisionDraft('rejected')}>Отклонить</button></>}</div>}{canEdit && <div className="review-flow-actions"><button type="button" className="secondary-button" disabled={saving} onClick={openConfig}>{review.status === 'requested' ? 'Изменить состав' : 'Повторить review'}</button><button type="button" className="text-action danger-text" disabled={saving} onClick={() => saveReview('none')}>Снять review</button></div>}</>}{error && <p role="alert">{error}</p>}</>;
  if (!compact) return null;
  const pending = Math.max(0, review.reviewers.length - answered);
  return <section className={`card-review-panel compact status-${review.status} ${isCompactOpen ? 'open' : ''}`} aria-label="Review карточки"><button type="button" className="review-compact-trigger" title={statusLabels[review.status]} aria-label={statusLabels[review.status]} onClick={() => setCompactOpen((current) => { if (!current) void loadReview(); return !current; })}><span aria-hidden="true">{iconForStatus(review.status)}</span>{review.status === 'requested' && <small>{pending}</small>}</button>{isCompactOpen && <div className="review-compact-popover">{controls}</div>}</section>;
}
