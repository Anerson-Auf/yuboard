'use client';

import { CSSProperties, DragEvent, useMemo, useState } from 'react';
import './schedule-view.css';

export type ScheduledCard = {
  id: string | number;
  title: string;
  listTitle: string;
  startAt?: string;
  dueAt?: string;
  completedAt?: string;
};

type ScheduleViewProps = {
  cards: ScheduledCard[];
  onDateChange: (cardId: string | number, dueAt: string) => void;
  onOpenCard: (cardId: string | number) => void;
};

type Mode = 'month' | 'week' | 'timeline';

const dayNames = ['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс'];

function localDay(value: Date) {
  return new Date(value.getFullYear(), value.getMonth(), value.getDate());
}

function dateKey(value: Date) {
  const year = value.getFullYear();
  const month = String(value.getMonth() + 1).padStart(2, '0');
  const day = String(value.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

function parseDay(value?: string) {
  if (!value) return null;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? null : localDay(parsed);
}

function monday(value: Date) {
  const day = localDay(value);
  day.setDate(day.getDate() - ((day.getDay() + 6) % 7));
  return day;
}

function dayRange(anchor: Date, mode: Mode) {
  if (mode === 'week') return Array.from({ length: 7 }, (_, index) => { const day = monday(anchor); day.setDate(day.getDate() + index); return day; });
  const first = new Date(anchor.getFullYear(), anchor.getMonth(), 1);
  const start = monday(first);
  return Array.from({ length: 42 }, (_, index) => { const day = new Date(start); day.setDate(day.getDate() + index); return day; });
}

function shiftAnchor(anchor: Date, mode: Mode, direction: -1 | 1) {
  const next = new Date(anchor);
  if (mode === 'week') next.setDate(next.getDate() + direction * 7);
  else next.setMonth(next.getMonth() + direction);
  return next;
}

function cardDays(card: ScheduledCard) {
  const due = parseDay(card.dueAt);
  const start = parseDay(card.startAt) ?? due;
  if (!start || !due) return null;
  return start <= due ? { start, due } : { start: due, due: start };
}

export default function ScheduleView({ cards, onDateChange, onOpenCard }: ScheduleViewProps) {
  const [mode, setMode] = useState<Mode>('month');
  const [anchor, setAnchor] = useState(() => new Date());
  const [draggedCardId, setDraggedCardId] = useState<string | number | null>(null);
  const days = useMemo(() => dayRange(anchor, mode), [anchor, mode]);
  const today = dateKey(new Date());
  const datedCards = useMemo(() => cards.filter((card) => parseDay(card.dueAt)), [cards]);
  const title = mode === 'week'
    ? `${days[0].toLocaleDateString('ru-RU', { day: 'numeric', month: 'long' })} — ${days.at(-1)?.toLocaleDateString('ru-RU', { day: 'numeric', month: 'long', year: 'numeric' })}`
    : anchor.toLocaleDateString('ru-RU', { month: 'long', year: 'numeric' });

  const dropOnDay = (event: DragEvent<HTMLElement>, day: Date) => {
    event.preventDefault();
    const cardId = event.dataTransfer.getData('text/flowboard-card') || String(draggedCardId ?? '');
    if (!cardId) return;
    const card = cards.find((item) => String(item.id) === cardId);
    if (card) onDateChange(card.id, `${dateKey(day)}T12:00:00`);
    setDraggedCardId(null);
  };

  if (mode === 'timeline') {
    const timelineStart = monday(anchor);
    const timelineDays = Array.from({ length: 28 }, (_, index) => { const day = new Date(timelineStart); day.setDate(day.getDate() + index); return day; });
    return <section className="schedule-view" aria-label="Шкала задач">
      <ScheduleToolbar mode={mode} setMode={setMode} title={title} onPrevious={() => setAnchor((current) => shiftAnchor(current, 'month', -1))} onNext={() => setAnchor((current) => shiftAnchor(current, 'month', 1))} onToday={() => setAnchor(new Date())} />
      <div className="timeline-scroll"><div className="timeline-grid" style={{ '--timeline-days': timelineDays.length } as CSSProperties}>
        <header className="timeline-days"><span>Карточка</span>{timelineDays.map((day) => <time key={dateKey(day)} className={dateKey(day) === today ? 'today' : ''}>{day.getDate()}<small>{dayNames[(day.getDay() + 6) % 7]}</small></time>)}</header>
        {datedCards.length ? datedCards.map((card) => {
          const range = cardDays(card);
          if (!range) return null;
          const startOffset = Math.max(0, Math.round((range.start.getTime() - timelineStart.getTime()) / 86_400_000));
          const endOffset = Math.min(timelineDays.length - 1, Math.round((range.due.getTime() - timelineStart.getTime()) / 86_400_000));
          if (endOffset < 0 || startOffset >= timelineDays.length) return null;
          return <article className="timeline-row" key={card.id}><button type="button" onClick={() => onOpenCard(card.id)} title={card.title}><b>{card.title}</b><small>{card.listTitle}</small></button><span className={`timeline-bar ${card.completedAt ? 'completed' : ''}`} style={{ gridColumn: `${startOffset + 2} / ${Math.max(startOffset + 3, endOffset + 3)}` }} title={`${card.startAt ? 'Начало: ' + new Date(card.startAt).toLocaleDateString('ru-RU') + ' · ' : ''}Срок: ${new Date(card.dueAt!).toLocaleDateString('ru-RU')}`} /> </article>;
        }) : <p className="schedule-empty">У задач пока нет дат. Поставь дедлайн — и они появятся на шкале.</p>}
      </div></div>
    </section>;
  }

  return <section className="schedule-view" aria-label={mode === 'month' ? 'Календарь задач' : 'Неделя задач'}>
    <ScheduleToolbar mode={mode} setMode={setMode} title={title} onPrevious={() => setAnchor((current) => shiftAnchor(current, mode, -1))} onNext={() => setAnchor((current) => shiftAnchor(current, mode, 1))} onToday={() => setAnchor(new Date())} />
    <div className={`schedule-calendar ${mode === 'week' ? 'week' : ''}`}>
      {dayNames.map((name) => <b className="schedule-day-name" key={name}>{name}</b>)}
      {days.map((day) => {
        const key = dateKey(day);
        const items = datedCards.filter((card) => dateKey(parseDay(card.dueAt)!) === key);
        const outside = mode === 'month' && day.getMonth() !== anchor.getMonth();
        return <article className={`schedule-day ${outside ? 'outside' : ''} ${key === today ? 'today' : ''}`} key={key} onDragOver={(event) => event.preventDefault()} onDrop={(event) => dropOnDay(event, day)}>
          <time dateTime={key}>{day.getDate()}</time>
          <div>{items.slice(0, mode === 'week' ? 9 : 4).map((card) => <button className={card.completedAt ? 'completed' : ''} draggable key={card.id} type="button" onDragStart={(event) => { event.dataTransfer.setData('text/flowboard-card', String(card.id)); event.dataTransfer.effectAllowed = 'move'; setDraggedCardId(card.id); }} onDragEnd={() => setDraggedCardId(null)} onClick={() => onOpenCard(card.id)} title={`${card.listTitle}: ${card.title}`}><span>{card.title}</span><small>{card.listTitle}</small></button>)}{items.length > (mode === 'week' ? 9 : 4) && <em>ещё {items.length - (mode === 'week' ? 9 : 4)}</em>}</div>
        </article>;
      })}
    </div>
  </section>;
}

function ScheduleToolbar({ mode, setMode, title, onPrevious, onNext, onToday }: { mode: Mode; setMode: (value: Mode) => void; title: string; onPrevious: () => void; onNext: () => void; onToday: () => void }) {
  return <header className="schedule-toolbar"><div><button type="button" onClick={onPrevious} aria-label="Предыдущий период">‹</button><button type="button" onClick={onToday}>Сегодня</button><button type="button" onClick={onNext} aria-label="Следующий период">›</button><h2>{title}</h2></div><div className="schedule-mode-switch" role="group" aria-label="Представление задач">{([['month', 'Месяц'], ['week', 'Неделя'], ['timeline', 'Шкала']] as const).map(([value, label]) => <button type="button" className={mode === value ? 'active' : ''} key={value} onClick={() => setMode(value)}>{label}</button>)}</div></header>;
}
