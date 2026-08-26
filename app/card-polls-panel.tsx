'use client';

import { useEffect, useState } from 'react';
import './card-polls-panel.css';

type Option = { id: string; title: string; votes: number; voted: boolean };
type Poll = { id: string; question: string; created_by: string; created_at: string; options: Option[] };
function headers() { const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length); return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' }; }

export default function CardPollsPanel({ cardId, canEdit, showPolls = true, showCreate = canEdit, hideWhenEmpty = false }: { cardId: string; canEdit: boolean; showPolls?: boolean; showCreate?: boolean; hideWhenEmpty?: boolean }) {
  const [polls, setPolls] = useState<Poll[]>([]); const [question, setQuestion] = useState(''); const [options, setOptions] = useState(''); const [isCreating, setCreating] = useState(false);
  const load = () => { void fetch(`/v1/cards/${cardId}/polls`).then((response) => response.ok ? response.json() as Promise<Poll[]> : []).then(setPolls); };
  useEffect(() => { setQuestion(''); setOptions(''); load(); }, [cardId]);
  const create = () => { if (!question.trim() || isCreating) return; setCreating(true); void fetch(`/v1/cards/${cardId}/polls`, { method: 'POST', headers: headers(), body: JSON.stringify({ question: question.trim(), options: options.split('\n') }) }).then((response) => response.ok ? response.json() as Promise<Poll> : Promise.reject()).then((poll) => { setPolls((current) => [poll, ...current]); setQuestion(''); setOptions(''); }).finally(() => setCreating(false)); };
  const vote = (poll: Poll, option: Option) => { void fetch(`/v1/polls/${poll.id}/vote`, { method: 'POST', headers: headers(), body: JSON.stringify({ option_id: option.id }) }).then((response) => response.ok ? response.json() as Promise<Poll> : Promise.reject()).then((updated) => setPolls((current) => current.map((item) => item.id === updated.id ? updated : item))); };
  if (!showPolls && !showCreate) return null;
  if (!showCreate && !hideWhenEmpty) return null;
  if (hideWhenEmpty && showPolls && !showCreate && !polls.length) return null;
  return <section className={`card-polls-panel ${showPolls ? '' : 'card-polls-composer-only'}`}>{showPolls && <><header><h3>Голосования</h3><span>{polls.length}</span></header>{polls.map((poll) => { const total = poll.options.reduce((sum, option) => sum + option.votes, 0); return <article key={poll.id}><b>{poll.question}</b><small>@{poll.created_by} · {new Date(poll.created_at).toLocaleDateString('ru-RU')}</small><div>{poll.options.map((option) => <button type="button" className={option.voted ? 'voted' : ''} onClick={() => vote(poll, option)} key={option.id}><span>{option.title}</span><i style={{ width: `${total ? Math.round(option.votes / total * 100) : 0}%` }} /><em>{option.votes}</em></button>)}</div></article>; })}{!polls.length && !canEdit && <p>Голосований пока нет.</p>}</>}{canEdit && showCreate && <form onSubmit={(event) => { event.preventDefault(); create(); }}><input value={question} onChange={(event) => setQuestion(event.target.value)} maxLength={300} placeholder="Вопрос для голосования" /><textarea value={options} onChange={(event) => setOptions(event.target.value)} placeholder={'Вариант 1\nВариант 2'} /><button disabled={isCreating || !question.trim()}>Создать голосование</button></form>}</section>;
}
