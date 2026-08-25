'use client';
/* eslint-disable @next/next/no-img-element -- self-hosted attachment URLs are served by the Rust API. */

import { ChangeEvent, ClipboardEvent as ReactClipboardEvent, DragEvent as ReactDragEvent, FormEvent, MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent, ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import './auth.css';

type EntityId = number | string;
type Member = { id: EntityId; initials: string; color: string; name: string; avatarUrl?: string | null };
type Label = { id: string; name: string; color: string };
type Card = { id: EntityId; title: string; description?: string; priority?: number; lastActivityAt?: string; dueAt?: string; coverAttachmentId?: string; coverUrl?: string; coverMode?: 'full' | 'top'; backgroundImageUrl?: string; completedAt?: string; isPublic?: boolean; hasUnreadMentions?: boolean; labels: Label[]; checklist?: string; comments?: number; attachments?: number; members: Member[] };
type Column = { id: EntityId; title: string; gridColumn: number; gridRow: number; cards: Card[] };
type View = 'home' | 'board';
type PersistenceStatus = 'connecting' | 'connected';
type BoardBackgroundFit = 'cover' | 'contain' | 'fill';
type BoardBackgroundPosition = 'center' | 'top' | 'bottom';
type ApiMember = { id: string; username: string; avatar_url?: string | null };
type ApiBoard = { id: string; workspace_id: string; title: string; background_image_url: string | null; background_fit: BoardBackgroundFit; background_position: BoardBackgroundPosition; visibility: 'public' | 'private' | 'workspace'; can_edit: boolean; labels: Label[]; members: ApiMember[]; lists: { id: string; title: string; grid_column: number; grid_row: number; cards: { id: string; title: string; description: string; priority: number; last_activity_at: string | null; is_public: boolean; background_image_url: string | null; due_at: string | null; cover_attachment_id: string | null; cover_url: string | null; cover_mode: 'full' | 'top'; completed_at: string | null; checklist_total: number; checklist_completed: number; comment_count: number; attachment_count: number; has_unread_mentions: boolean; labels: Label[]; assignees: ApiMember[] }[] }[] };
type DragState = { cardId: EntityId; sourceListId: EntityId };
type DragDropTarget = { listId: EntityId; beforeCardId: EntityId | null };
type ChecklistItem = { id: EntityId; title: string; is_completed: boolean; description: string; attachments: Attachment[] };
type Checklist = { id: string; title: string; items: ChecklistItem[] };
type Comment = { id: EntityId; body: string; author_id?: string | null; author_name: string; author_avatar_url?: string | null; parent_comment_id?: string | null; created_at?: string; edited_at?: string | null };
type Attachment = { id: string; original_name: string; media_type: string; byte_size: number; url: string };
type Activity = { id: string; action: string; detail: string; actor_name: string | null; created_at: string };
type CardDetail = { checklists: Checklist[]; comments: Comment[]; attachments: Attachment[]; activity: Activity[]; cover_attachment_id: string | null; cover_mode: 'full' | 'top'; background_image_url: string | null; unread_mention_source_ids: string[] };
type AuthAccount = { user: { id: string; username: string; avatar_url: string | null; is_system_owner: boolean } };
type AuthState = 'checking' | 'signed-out' | 'signed-in' | 'public';
type Workspace = { id: string; name: string };
type BoardSummary = { id: string; title: string; visibility: string };
type FilterMode = 'all' | 'assigned' | 'due' | 'overdue';
type CardSort = 'manual' | 'priority' | 'activity';
type ArchivedCard = { id: string; list_id: string; title: string; description: string; archived_at: string };
type TeamMember = { id: string; username: string; preset: 'owner' | 'viewer' | 'contributor' | 'editor' | 'full_access'; avatar_url?: string | null };
const roleLabels: Record<TeamMember['preset'], string> = { owner: 'Владелец', viewer: 'Наблюдатель', contributor: 'Участник', editor: 'Редактор', full_access: 'Полный доступ' };
const roleDescriptions: Record<TeamMember['preset'], string> = { owner: 'Все права навсегда, включая доступы и удаление.', viewer: 'Только просмотр доски и карточек.', contributor: 'Просмотр, создание и редактирование карточек.', editor: 'Работа с карточками, колонками и метками.', full_access: 'Все действия в проекте, включая команду и настройки.' };
type AdminAccount = { id: string; username: string; avatar_url?: string | null; disabled_at: string | null; is_system_owner: boolean; created_at: string };
type AccountInvite = { id: string; expires_at: string; token?: string | null };
type AdminWorkspace = { id: string; name: string; owner_username: string; member_count: number; archived_at: string | null };
type AuthSession = { id: string; created_at: string; last_seen_at: string; expires_at: string; current: boolean };
type DiscordIntegration = { id: string; name: string; default_list_id: string | null; created_at: string; last_used_at: string | null; token?: string };
type DiagramPoint = { x: number; y: number };
type DiagramStroke = { points: DiagramPoint[]; color?: string; width?: number };
type DiagramRectangle = { type: 'rectangle' | 'ellipse'; x: number; y: number; width: number; height: number; color: string; lineWidth: number };
type DiagramArrow = { type: 'arrow'; x: number; y: number; x2: number; y2: number; color: string; lineWidth: number };
type DiagramText = { type: 'text'; x: number; y: number; text: string; color: string; fontSize: number; fontFamily: string; fontWeight: 'normal' | 'bold' };
type DiagramCallout = { type: 'callout'; x: number; y: number; x2: number; y2: number; text: string; color: string; fontSize: number; fontFamily: string; fontWeight: 'normal' | 'bold' };
type DiagramElement = DiagramRectangle | DiagramArrow | DiagramText | DiagramCallout;
type DiagramDocument = { strokes: DiagramStroke[]; elements?: DiagramElement[] };
type Diagram = { id: string; card_id: string; title: string; document: DiagramDocument; version: number };
type DiagramTool = 'select' | 'draw' | 'rectangle' | 'ellipse' | 'arrow' | 'text' | 'callout';
type DiagramSnapshot = { strokes: DiagramStroke[]; elements: DiagramElement[] };
type DiagramHandle = 'move' | 'nw' | 'ne' | 'se' | 'sw' | 'start' | 'end';
type DiagramInteraction = { kind: 'move' | 'resize'; index: number; handle: DiagramHandle; start: DiagramPoint; initial: DiagramElement; historyStored: boolean };
type CardContextMenu = { card: Card; x: number; y: number };
type ColumnContextMenu = { column: Column; x: number; y: number };
type ColumnDropTarget = { beforeColumnId: EntityId | null; visualColumnId: EntityId; edge: 'before' | 'after' };

// Empty in local development and production: requests stay on the current origin.
// Vite forwards /v1 to Rust locally; nginx does the same after deployment.
const API_URL = process.env.NEXT_PUBLIC_FLOWBOARD_API_URL ?? '';
const browserFetch = globalThis.fetch.bind(globalThis);

function assetUrl(url: string | null | undefined) {
  if (!url) return '';
  return /^https?:\/\//i.test(url) ? url : `${API_URL}${url}`;
}

function csrfCookie() {
  return document.cookie.split('; ').find((part) => part.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
}

function fetch(input: RequestInfo | URL, init: RequestInit = {}) {
  const method = (init.method ?? 'GET').toUpperCase();
  const headers = new Headers(init.headers);
  if (!['GET', 'HEAD', 'OPTIONS'].includes(method)) {
    const csrf = csrfCookie();
    if (csrf) headers.set('x-flowboard-csrf', csrf);
  }
  return browserFetch(input, { ...init, headers, credentials: 'include' });
}

const initialColumns: Column[] = [];
const monthNames = ['Январь', 'Февраль', 'Март', 'Апрель', 'Май', 'Июнь', 'Июль', 'Август', 'Сентябрь', 'Октябрь', 'Ноябрь', 'Декабрь'];
const weekdayNames = ['Пн', 'Вт', 'Ср', 'Чт', 'Пт', 'Сб', 'Вс'];
const dueTimeOptions = ['09:00', '12:00', '15:00', '18:00'];

function inlineMarkdown(value: string, highlightMentions = false): ReactNode[] {
  const token = /(\!\[[^\]]*\]\([^\s)]+(?:\s+['"][^'"]*['"])?\)|\[[^\]]+\]\([^\s)]+(?:\s+['"][^'"]*['"])?\)|`[^`]+`|\*\*[^*]+\*\*|__[^_]+__|~~[^~]+~~|(?<!\*)\*[^*\r\n]+\*(?!\*)|(?<!_)_[^_\r\n]+_(?!_)|@[a-zA-Z0-9_.-]+|https?:\/\/[^\s<]+)/g;
  return value.split(token).filter(Boolean).map((part, index) => {
    const image = /^!\[([^\]]*)\]\(([^\s)]+)/.exec(part);
    if (image) {
      const isVideo = image[1].startsWith('video:');
      const name = image[1].replace(/^video:/, '') || (isVideo ? 'Видео' : 'Изображение');
      return isVideo
        ? <video className="markdown-media markdown-video" key={index} controls preload="metadata" src={image[2]} aria-label={name} />
        : <a className="markdown-image-link" key={index} href={image[2]} target="_blank" rel="noreferrer"><img className="markdown-media" src={image[2]} alt={name} /></a>;
    }
    const link = /^\[([^\]]+)\]\(([^\s)]+)/.exec(part);
    if (link) return <a key={index} href={link[2]} target="_blank" rel="noreferrer">{link[1]}</a>;
    if (/^https?:\/\//.test(part)) return <a key={index} href={part} target="_blank" rel="noreferrer">{part}</a>;
    if (part.startsWith('`') && part.endsWith('`')) return <code key={index}>{part.slice(1, -1)}</code>;
    if ((part.startsWith('**') && part.endsWith('**')) || (part.startsWith('__') && part.endsWith('__'))) return <strong key={index}>{part.slice(2, -2)}</strong>;
    if (part.startsWith('~~') && part.endsWith('~~')) return <s key={index}>{part.slice(2, -2)}</s>;
    if ((part.startsWith('*') && part.endsWith('*')) || (part.startsWith('_') && part.endsWith('_'))) return <em key={index}>{part.slice(1, -1)}</em>;
    if (/^@[a-zA-Z0-9_.-]+$/.test(part)) return highlightMentions ? <mark className="mention-highlight" key={index}>{part}</mark> : part;
    return part;
  });
}

function MarkdownDescription({ value, highlightMentions = false, emptyText = 'Добавьте описание задачи…' }: { value: string; highlightMentions?: boolean; emptyText?: string }) {
  if (!value.trim()) return <p className="description-empty">{emptyText}</p>;
  const lines = value.replace(/\r\n?/g, '\n').split('\n');
  const blocks: ReactNode[] = [];
  let index = 0;
  const isRule = (line: string) => /^\s{0,3}([-*_])(?:\s*\1){2,}\s*$/.test(line);
  while (index < lines.length) {
    const line = lines[index];
    if (!line.trim()) { blocks.push(<br key={`blank-${index}`} />); index += 1; continue; }
    if (isRule(line)) { blocks.push(<hr key={`rule-${index}`} />); index += 1; continue; }
    const heading = /^(#{1,6})\s*(\S.*)$/.exec(line);
    if (heading) { const Tag = `h${heading[1].length}` as keyof JSX.IntrinsicElements; blocks.push(<Tag key={`heading-${index}`}>{inlineMarkdown(heading[2], highlightMentions)}</Tag>); index += 1; continue; }
    if (/^\s*>\s?/.test(line)) {
      const quote: string[] = [];
      while (index < lines.length && /^\s*>\s?/.test(lines[index])) { quote.push(lines[index].replace(/^\s*>\s?/, '')); index += 1; }
      blocks.push(<blockquote key={`quote-${index}`}>{quote.map((item, quoteIndex) => <p key={quoteIndex}>{inlineMarkdown(item, highlightMentions)}</p>)}</blockquote>);
      continue;
    }
    const unordered = /^\s*[-+*]\s*(.+)$/.exec(line);
    const ordered = /^\s*\d+[.)]\s+(.+)$/.exec(line);
    if (unordered || ordered) {
      const orderedList = Boolean(ordered); const items: string[] = [];
      const pattern = orderedList ? /^\s*\d+[.)]\s+(.+)$/ : /^\s*[-+*]\s*(.+)$/;
      while (index < lines.length) { const item = pattern.exec(lines[index]); if (!item) break; items.push(item[1]); index += 1; }
      const List = orderedList ? 'ol' : 'ul'; blocks.push(<List key={`list-${index}`}>{items.map((item, itemIndex) => <li key={itemIndex}>{inlineMarkdown(item, highlightMentions)}</li>)}</List>);
      continue;
    }
    blocks.push(<p key={`paragraph-${index}`}>{inlineMarkdown(line, highlightMentions)}</p>); index += 1;
  }
  return <div className="markdown-description">{blocks}</div>;
}

function isSameDay(left: Date, right: Date) {
  return left.getFullYear() === right.getFullYear() && left.getMonth() === right.getMonth() && left.getDate() === right.getDate();
}

function calendarDays(cursor: Date) {
  const first = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
  const startOffset = (first.getDay() + 6) % 7;
  return Array.from({ length: 42 }, (_, index) => new Date(cursor.getFullYear(), cursor.getMonth(), index - startOffset + 1));
}

function dueDateFrom(day: Date, time: string) {
  const [hours, minutes] = time.split(':').map(Number);
  return new Date(day.getFullYear(), day.getMonth(), day.getDate(), hours, minutes, 0, 0).toISOString();
}

function formatDue(value: string) {
  return new Intl.DateTimeFormat('ru-RU', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' }).format(new Date(value));
}

function activityLabel(action: string) {
  // Old checklist activity records used this vague status. Keep history useful:
  // it represents an unchecked item, not an unfinished card.
  if (action.trim().toLocaleLowerCase('ru-RU') === 'не доделано') return 'снял(а) отметку с пункта чек-листа';
  const labels: Record<string, string> = {
    updateCard: 'изменил(а) карточку',
    createCard: 'создал(а) карточку',
    copyCard: 'скопировал(а) карточку',
    moveCard: 'переместил(а) карточку',
    addMemberToCard: 'добавил(а) участника',
    removeMemberFromCard: 'убрал(а) участника',
    addLabelToCard: 'добавил(а) метку',
    removeLabelFromCard: 'убрал(а) метку',
    updateChecklist: 'изменил(а) чек-лист',
    createChecklist: 'создал(а) чек-лист',
    deleteChecklist: 'удалил(а) чек-лист',
  };
  return labels[action] ?? action;
}

function isChecklistComplete(value?: string) {
  if (!value) return false;
  const [done, total] = value.split('/').map(Number);
  return Number.isFinite(done) && Number.isFinite(total) && total > 0 && done === total;
}

function memberFromApi(member: ApiMember): Member {
  const words = member.username.trim().split(/\s+/).filter(Boolean);
  const initials = words.map((word) => word[0]).join('').slice(0, 2).toUpperCase() || 'У';
  const color = ['violet', 'mint', 'amber'][member.id.charCodeAt(0) % 3];
  return { id: member.id, initials, color, name: member.username, avatarUrl: member.avatar_url };
}

function Avatar({ member }: { member: Member }) {
  if (member.name === 'Deleted user') return <span className="avatar deleted-user" title="Deleted user">—</span>;
  if (member.avatarUrl) {
    const src = assetUrl(member.avatarUrl);
    return <img className="avatar profile-image" src={src} alt={`Аватар @${member.name}`} title={`@${member.name}`} />;
  }
  return <span className={`avatar ${member.color}`} title={member.name}>{member.initials}</span>;
}

function ProfileAvatar({ account, member, version = 0 }: { account: AuthAccount | null; member: Member; version?: number }) {
  return account?.user.avatar_url ? <img className="avatar profile-image" src={`${account.user.avatar_url}?v=${encodeURIComponent(`${account.user.id}-${version}`)}`} alt="Аватар профиля" /> : <Avatar member={member} />;
}

function PrioritySignal({ priority }: { priority?: number }) {
  const level = Math.max(0, Math.min(5, priority ?? 0));
  if (!level) return null;
  return <span className={`priority-signal level-${level}`} title={`Приоритет ${level} из 5`} aria-label={`Приоритет ${level} из 5`}>{[1, 2, 3, 4, 5].map((bar) => <i className={bar <= level ? 'active' : ''} key={bar} />)}</span>;
}

function CardMetaIcon({ type }: { type: 'comments' | 'checklist' | 'attachments' }) {
  const paths = {
    comments: 'M0 3.125A2.625 2.625 0 0 1 2.625.5h10.75A2.625 2.625 0 0 1 16 3.125v8.25A2.625 2.625 0 0 1 13.375 14H4.449l-3.327 1.901A.75.75 0 0 1 0 15.25zM2.625 2C2.004 2 1.5 2.504 1.5 3.125v10.833L4.05 12.5h9.325c.621 0 1.125-.504 1.125-1.125v-8.25C14.5 2.504 13.996 2 13.375 2zM12 6.5H4V5h8zm-3 3H4V8h5z',
    checklist: 'M1 3a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2zm2-.5a.5.5 0 0 0-.5.5v10a.5.5 0 0 0 .5.5h10a.5.5 0 0 0 .5-.5V3a.5.5 0 0 0-.5-.5zm9.326 2.98-5 6a.75.75 0 0 1-1.152 0l-2.5-3 1.152-.96L6.75 9.828l4.424-5.308z',
    attachments: 'M15 3.5H1V2h14zm0 5.25H1v-1.5h14zM8 14H1v-1.5h7z',
  };
  return <svg className="card-meta-icon" viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" fillRule="evenodd" clipRule="evenodd" d={paths[type]} /></svg>;
}

type MentionTextareaProps = {
  value: string;
  onValueChange: (value: string) => void;
  members: Member[];
  className?: string;
  placeholder?: string;
  maxLength?: number;
  ariaLabel: string;
  autoFocus?: boolean;
  disabled?: boolean;
  onBlur?: () => void;
  onDragOver?: (event: ReactDragEvent<HTMLTextAreaElement>) => void;
  onDrop?: (event: ReactDragEvent<HTMLTextAreaElement>) => void;
  onPaste?: (event: ReactClipboardEvent<HTMLTextAreaElement>) => void;
};

function MentionTextarea({ value, onValueChange, members, className, placeholder, maxLength, ariaLabel, autoFocus, disabled, onBlur, onDragOver, onDrop, onPaste }: MentionTextareaProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const [query, setQuery] = useState<string | null>(null);
  const isChecklistDescription = ariaLabel.startsWith('Описание пункта ');
  const [isMarkdownPreview, setMarkdownPreview] = useState(isChecklistDescription);
  useEffect(() => {
    if (!isMarkdownPreview) textareaRef.current?.focus();
  }, [isMarkdownPreview]);
  const findQuery = (nextValue: string, caret: number | null) => {
    const beforeCaret = nextValue.slice(0, caret ?? nextValue.length);
    const match = beforeCaret.match(/(?:^|\s)@([a-zA-Z0-9_.-]*)$/);
    setQuery(match ? match[1].toLowerCase() : null);
  };
  const suggestions = query === null ? [] : members.filter((member) => member.name.toLowerCase().startsWith(query)).slice(0, 7);
  const insertMention = (member: Member) => {
    const textarea = textareaRef.current;
    const caret = textarea?.selectionStart ?? value.length;
    const at = value.slice(0, caret).lastIndexOf('@');
    if (at < 0) return;
    const next = `${value.slice(0, at)}@${member.name} ${value.slice(caret)}`;
    onValueChange(next);
    setQuery(null);
    window.requestAnimationFrame(() => {
      const nextCaret = at + member.name.length + 2;
      textarea?.focus(); textarea?.setSelectionRange(nextCaret, nextCaret);
    });
  };
  if (isChecklistDescription && isMarkdownPreview) {
    return <div className={`checklist-description-preview markdown-editable-description ${className ?? ''}`} role={disabled ? undefined : 'button'} tabIndex={disabled ? undefined : 0} onClick={() => { if (!disabled) setMarkdownPreview(false); }} onKeyDown={(event) => { if (!disabled && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); setMarkdownPreview(false); } }}><MarkdownDescription value={value} emptyText="Добавьте описание пункта…" /></div>;
  }
  return <div className="mention-textarea"><textarea ref={textareaRef} className={className} value={value} onChange={(event) => { onValueChange(event.target.value); findQuery(event.target.value, event.target.selectionStart); }} onClick={(event) => findQuery(event.currentTarget.value, event.currentTarget.selectionStart)} onKeyUp={(event) => findQuery(event.currentTarget.value, event.currentTarget.selectionStart)} onKeyDown={(event) => { if (event.key === 'Escape') setQuery(null); if (event.key === 'Tab' && suggestions[0]) { event.preventDefault(); insertMention(suggestions[0]); } }} onBlur={() => { setQuery(null); if (isChecklistDescription) setMarkdownPreview(true); onBlur?.(); }} onDragOver={onDragOver} onDrop={onDrop} onPaste={onPaste} maxLength={maxLength} placeholder={placeholder} aria-label={ariaLabel} autoFocus={autoFocus} disabled={disabled} />{suggestions.length > 0 && <div className="mention-suggestions" role="listbox" aria-label="Участники доски"><p>Участники доски</p>{suggestions.map((member) => <button key={member.id} type="button" onMouseDown={(event) => { event.preventDefault(); insertMention(member); }}><Avatar member={member} /><span>@{member.name}</span></button>)}</div>}</div>;
}

function drawDiagramElement(context: CanvasRenderingContext2D, element: DiagramElement) {
  context.save();
  context.strokeStyle = element.color;
  context.fillStyle = element.color;
  context.lineWidth = 'lineWidth' in element ? element.lineWidth : 1;
  context.lineCap = 'round';
  context.lineJoin = 'round';

  if (element.type === 'rectangle') {
    context.strokeRect(element.x, element.y, element.width, element.height);
  } else if (element.type === 'ellipse') {
    const centerX = element.x + element.width / 2;
    const centerY = element.y + element.height / 2;
    context.beginPath();
    context.ellipse(centerX, centerY, Math.abs(element.width) / 2, Math.abs(element.height) / 2, 0, 0, Math.PI * 2);
    context.stroke();
  } else if (element.type === 'arrow') {
    const angle = Math.atan2(element.y2 - element.y, element.x2 - element.x);
    const head = Math.max(10, element.lineWidth * 4);
    context.beginPath();
    context.moveTo(element.x, element.y);
    context.lineTo(element.x2, element.y2);
    context.lineTo(element.x2 - head * Math.cos(angle - Math.PI / 6), element.y2 - head * Math.sin(angle - Math.PI / 6));
    context.moveTo(element.x2, element.y2);
    context.lineTo(element.x2 - head * Math.cos(angle + Math.PI / 6), element.y2 - head * Math.sin(angle + Math.PI / 6));
    context.stroke();
  } else if (element.type === 'callout') {
    const angle = Math.atan2(element.y - element.y2, element.x - element.x2);
    const head = 10;
    context.beginPath();
    context.moveTo(element.x, element.y);
    context.lineTo(element.x2, element.y2);
    context.lineTo(element.x2 + head * Math.cos(angle - Math.PI / 6), element.y2 + head * Math.sin(angle - Math.PI / 6));
    context.moveTo(element.x2, element.y2);
    context.lineTo(element.x2 + head * Math.cos(angle + Math.PI / 6), element.y2 + head * Math.sin(angle + Math.PI / 6));
    context.stroke();
    context.font = `${element.fontWeight === 'bold' ? '700' : '400'} ${element.fontSize}px ${element.fontFamily}`;
    context.textBaseline = 'top';
    element.text.split('\n').forEach((line, index) => context.fillText(line, element.x2 + 8, element.y2 + 8 + index * element.fontSize * 1.28));
  } else {
    context.font = `${element.fontWeight === 'bold' ? '700' : '400'} ${element.fontSize}px ${element.fontFamily}`;
    context.textBaseline = 'top';
    element.text.split('\n').forEach((line, index) => context.fillText(line, element.x, element.y + index * element.fontSize * 1.28));
  }
  context.restore();
}

function diagramBounds(element: DiagramElement) {
  if (element.type === 'arrow') return { left: Math.min(element.x, element.x2), top: Math.min(element.y, element.y2), right: Math.max(element.x, element.x2), bottom: Math.max(element.y, element.y2) };
  if (element.type === 'callout') {
    const textWidth = Math.max(...element.text.split('\n').map((line) => line.length), 1) * element.fontSize * .62;
    const textHeight = element.text.split('\n').length * element.fontSize * 1.28;
    return { left: Math.min(element.x, element.x2), top: Math.min(element.y, element.y2), right: Math.max(element.x, element.x2 + 8 + textWidth), bottom: Math.max(element.y, element.y2 + 8 + textHeight) };
  }
  if (element.type === 'text') {
    const textWidth = Math.max(...element.text.split('\n').map((line) => line.length), 1) * element.fontSize * .62;
    return { left: element.x, top: element.y, right: element.x + textWidth, bottom: element.y + element.text.split('\n').length * element.fontSize * 1.28 };
  }
  return { left: Math.min(element.x, element.x + element.width), top: Math.min(element.y, element.y + element.height), right: Math.max(element.x, element.x + element.width), bottom: Math.max(element.y, element.y + element.height) };
}

function pointToSegmentDistance(point: DiagramPoint, start: DiagramPoint, end: DiagramPoint) {
  const deltaX = end.x - start.x;
  const deltaY = end.y - start.y;
  const lengthSquared = deltaX * deltaX + deltaY * deltaY;
  if (!lengthSquared) return Math.hypot(point.x - start.x, point.y - start.y);
  const t = Math.max(0, Math.min(1, ((point.x - start.x) * deltaX + (point.y - start.y) * deltaY) / lengthSquared));
  return Math.hypot(point.x - (start.x + deltaX * t), point.y - (start.y + deltaY * t));
}

function drawDiagramSelection(context: CanvasRenderingContext2D, element: DiagramElement) {
  const bounds = diagramBounds(element);
  const pad = 7;
  context.save();
  context.setLineDash([5, 4]);
  context.strokeStyle = '#86aaf1';
  context.lineWidth = 1.5;
  context.strokeRect(bounds.left - pad, bounds.top - pad, bounds.right - bounds.left + pad * 2, bounds.bottom - bounds.top + pad * 2);
  context.setLineDash([]);
  context.fillStyle = '#d5e2ff';
  const handles = element.type === 'arrow' || element.type === 'callout'
    ? [{ x: element.x, y: element.y }, { x: element.x2, y: element.y2 }]
    : [{ x: bounds.left, y: bounds.top }, { x: bounds.right, y: bounds.top }, { x: bounds.right, y: bounds.bottom }, { x: bounds.left, y: bounds.bottom }];
  handles.forEach((handle) => { context.fillRect(handle.x - 4, handle.y - 4, 8, 8); context.strokeRect(handle.x - 4, handle.y - 4, 8, 8); });
  context.restore();
}

export default function Home() {
  const [columns, setColumns] = useState(initialColumns);
  const [view, setView] = useState<View>('home');
  const [theme, setTheme] = useState<'dark' | 'light'>('dark');
  const [isComposerOpen, setComposerOpen] = useState<EntityId | null>(null);
  const [draft, setDraft] = useState('');
  const [selected, setSelected] = useState<Card | null>(null);
  const [toast, setToast] = useState('');
  const [query, setQuery] = useState('');
  const [filterMode, setFilterMode] = useState<FilterMode>('all');
  const [cardSort, setCardSort] = useState<CardSort>('manual');
  const [labelsCollapsed, setLabelsCollapsed] = useState(false);
  const [isFilterOpen, setFilterOpen] = useState(false);
  const [isBoardLabelsOpen, setBoardLabelsOpen] = useState(false);
  const [isSortOpen, setSortOpen] = useState(false);
  const [nextCardId, setNextCardId] = useState(100);
  const [checklists, setChecklists] = useState<Checklist[]>([]);
  const [checklistNameDraft, setChecklistNameDraft] = useState('');
  const [checklistItemDrafts, setChecklistItemDrafts] = useState<Record<string, string>>({});
  const [expandedChecklistItemIds, setExpandedChecklistItemIds] = useState<string[]>([]);
  const [checklistItemDescriptionDrafts, setChecklistItemDescriptionDrafts] = useState<Record<string, string>>({});
  const [isUploadingChecklistItemAttachment, setUploadingChecklistItemAttachment] = useState(false);
  const [imagePreview, setImagePreview] = useState<{ url: string; name: string } | null>(null);
  const [comments, setComments] = useState<Comment[]>([]);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [activity, setActivity] = useState<Activity[]>([]);
  const [isUploadingAttachment, setUploadingAttachment] = useState(false);
  const [coverModeDraft, setCoverModeDraft] = useState<'full' | 'top'>('full');
  const [commentDraft, setCommentDraft] = useState('');
  const [replyToCommentId, setReplyToCommentId] = useState<EntityId | null>(null);
  const [editingCommentId, setEditingCommentId] = useState<EntityId | null>(null);
  const [commentEditDraft, setCommentEditDraft] = useState('');
  const [isSavingChecklist, setSavingChecklist] = useState(false);
  const [isSendingComment, setSendingComment] = useState(false);
  const [isDetailsLoading, setDetailsLoading] = useState(false);
  const [cardTitleDraft, setCardTitleDraft] = useState('');
  const [cardDescriptionDraft, setCardDescriptionDraft] = useState('');
  const [isEditingCardDescription, setEditingCardDescription] = useState(false);
  const [cardSaveStatus, setCardSaveStatus] = useState<'saved' | 'saving' | 'error'>('saved');
  const [dueCursor, setDueCursor] = useState(new Date());
  const [dueTime, setDueTime] = useState('09:00');
  const [isSavingDueAt, setSavingDueAt] = useState(false);
  const [sidebarPanel, setSidebarPanel] = useState<'labels' | 'due' | 'assignees' | 'background' | 'public-visibility' | null>(null);
  const [existingLabelsOnly, setExistingLabelsOnly] = useState(false);
  const [isUploadingCardBackground, setUploadingCardBackground] = useState(false);
  const [boardLabels, setBoardLabels] = useState<Label[]>([]);
  const [workspaceMembers, setWorkspaceMembers] = useState<Member[]>([]);
  const [workspaceId, setWorkspaceId] = useState<string | null>(null);
  const [workspaceName, setWorkspaceName] = useState('');
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [isWorkspaceComposerOpen, setWorkspaceComposerOpen] = useState(false);
  const [newWorkspaceName, setNewWorkspaceName] = useState('');
  const [workspaceCreateError, setWorkspaceCreateError] = useState('');
  const [isCreatingWorkspace, setCreatingWorkspace] = useState(false);
  const [boards, setBoards] = useState<BoardSummary[]>([]);
  const [isNewBoardComposer, setNewBoardComposer] = useState(false);
  const [newBoardTitle, setNewBoardTitle] = useState('');
  const [isCreatingBoard, setCreatingBoard] = useState(false);
  const [isArchiveOpen, setArchiveOpen] = useState(false);
  const [archivedCards, setArchivedCards] = useState<ArchivedCard[]>([]);
  const [isArchiveLoading, setArchiveLoading] = useState(false);
  const [isTeamOpen, setTeamOpen] = useState(false);
  const [teamMembers, setTeamMembers] = useState<TeamMember[]>([]);
  const [isTeamLoading, setTeamLoading] = useState(false);
  const [isAdminOpen, setAdminOpen] = useState(false);
  const [adminAccounts, setAdminAccounts] = useState<AdminAccount[]>([]);
  const [adminInvites, setAdminInvites] = useState<AccountInvite[]>([]);
  const [adminWorkspaces, setAdminWorkspaces] = useState<AdminWorkspace[]>([]);
  const [isAdminLoading, setAdminLoading] = useState(false);
  const [isSessionsOpen, setSessionsOpen] = useState(false);
  const [sessions, setSessions] = useState<AuthSession[]>([]);
  const [availableAccounts, setAvailableAccounts] = useState<ApiMember[]>([]);
  const [selectedAccountId, setSelectedAccountId] = useState('');
  const [accountSearch, setAccountSearch] = useState('');
  const [isSavingMember, setSavingMember] = useState(false);
  const [newLabelName, setNewLabelName] = useState('');
  const [newLabelColor, setNewLabelColor] = useState('#6B7CFF');
  const [isSavingLabel, setSavingLabel] = useState(false);
  const [editingBoardLabel, setEditingBoardLabel] = useState<Label | null>(null);
  const [boardLabelNameDraft, setBoardLabelNameDraft] = useState('');
  const [boardLabelColorDraft, setBoardLabelColorDraft] = useState('#6B7CFF');
  const [isSavingBoardLabel, setSavingBoardLabel] = useState(false);
  const [boardTitle, setBoardTitle] = useState('');
  const [boardBackgroundUrl, setBoardBackgroundUrl] = useState<string | null>(null);
  const [boardBackgroundFit, setBoardBackgroundFit] = useState<BoardBackgroundFit>('cover');
  const [boardBackgroundPosition, setBoardBackgroundPosition] = useState<BoardBackgroundPosition>('center');
  const [boardVisibility, setBoardVisibility] = useState<'public' | 'private'>('private');
  const [canEditBoard, setCanEditBoard] = useState(true);
  const [backgroundDraft, setBackgroundDraft] = useState('');
  const [isUploadingBoardBackground, setUploadingBoardBackground] = useState(false);
  const [isBoardMenuOpen, setBoardMenuOpen] = useState(false);
  const [isDiscordIntegrationOpen, setDiscordIntegrationOpen] = useState(false);
  const [discordIntegrations, setDiscordIntegrations] = useState<DiscordIntegration[]>([]);
  const [discordIntegrationName, setDiscordIntegrationName] = useState('Предложки Discord');
  const [discordTargetListId, setDiscordTargetListId] = useState('');
  const [createdDiscordToken, setCreatedDiscordToken] = useState('');
  const [isDiscordIntegrationLoading, setDiscordIntegrationLoading] = useState(false);
  const [isCreatingDiscordIntegration, setCreatingDiscordIntegration] = useState(false);
  const [cardContextMenu, setCardContextMenu] = useState<CardContextMenu | null>(null);
  const [columnContextMenu, setColumnContextMenu] = useState<ColumnContextMenu | null>(null);
  const [isSavingBackground, setSavingBackground] = useState(false);
  const [boardId, setBoardId] = useState<string | null>(null);
  const [isEditingBoardTitle, setEditingBoardTitle] = useState(false);
  const [boardTitleDraft, setBoardTitleDraft] = useState('');
  const [isSavingBoardTitle, setSavingBoardTitle] = useState(false);
  const [persistence, setPersistence] = useState<PersistenceStatus>('connecting');
  const [authState, setAuthState] = useState<AuthState>('checking');
  const [account, setAccount] = useState<AuthAccount | null>(null);
  const [authMode, setAuthMode] = useState<'login' | 'register'>('login');
  const [registrationOpen, setRegistrationOpen] = useState(false);
  const [authName, setAuthName] = useState('');
  const [authPassword, setAuthPassword] = useState('');
  const [inviteToken] = useState<string | null>(() => typeof window === 'undefined' ? null : new URLSearchParams(window.location.search).get('invite'));
  const [sharedBoardId, setSharedBoardId] = useState<string | null>(() => typeof window === 'undefined' ? null : new URLSearchParams(window.location.search).get('board'));
  const [authError, setAuthError] = useState('');
  const [isAuthorizing, setAuthorizing] = useState(false);
  const [isProfileOpen, setProfileOpen] = useState(false);
  const [profilePanel, setProfilePanel] = useState<'overview' | 'username' | 'password'>('overview');
  const [profileName, setProfileName] = useState('');
  const [currentPassword, setCurrentPassword] = useState('');
  const [nextPassword, setNextPassword] = useState('');
  const [profileError, setProfileError] = useState('');
  const [isSavingProfile, setSavingProfile] = useState(false);
  const [avatarVersion, setAvatarVersion] = useState(0);
  const [isDiagramOpen, setDiagramOpen] = useState(false);
  const [diagram, setDiagram] = useState<Diagram | null>(null);
  const [diagramTitle, setDiagramTitle] = useState('Схема');
  const [diagramStrokes, setDiagramStrokes] = useState<DiagramStroke[]>([]);
  const [diagramElements, setDiagramElements] = useState<DiagramElement[]>([]);
  const [diagramHistory, setDiagramHistory] = useState<DiagramSnapshot[]>([]);
  const [selectedDiagramElement, setSelectedDiagramElement] = useState<number | null>(null);
  const [diagramPreview, setDiagramPreview] = useState<DiagramElement | null>(null);
  const [diagramTool, setDiagramTool] = useState<DiagramTool>('draw');
  const [diagramColor, setDiagramColor] = useState('#9ea7ff');
  const [diagramLineWidth, setDiagramLineWidth] = useState(3);
  const [diagramTextDraft, setDiagramTextDraft] = useState('');
  const [diagramFontSize, setDiagramFontSize] = useState(22);
  const [diagramFontFamily, setDiagramFontFamily] = useState('Inter, system-ui, sans-serif');
  const [diagramFontWeight, setDiagramFontWeight] = useState<'normal' | 'bold'>('normal');
  const [diagramZoom, setDiagramZoom] = useState(.7);
  const [isDiagramSaving, setDiagramSaving] = useState(false);
  const [dragging, setDragging] = useState<DragState | null>(null);
  const [dragOverListId, setDragOverListId] = useState<EntityId | null>(null);
  const [dragDropTarget, setDragDropTarget] = useState<DragDropTarget | null>(null);
  const [draggingColumnId, setDraggingColumnId] = useState<EntityId | null>(null);
  const [columnDropTarget, setColumnDropTarget] = useState<ColumnDropTarget | null>(null);
  const [isBoardPanning, setBoardPanning] = useState(false);
  const [columnMenuId, setColumnMenuId] = useState<EntityId | null>(null);
  const [editingColumnId, setEditingColumnId] = useState<EntityId | null>(null);
  const [columnTitleDraft, setColumnTitleDraft] = useState('');
  const [isSavingColumn, setSavingColumn] = useState(false);
  const [cardDetailRevision, setCardDetailRevision] = useState(0);
  const [unreadMentionSourceIds, setUnreadMentionSourceIds] = useState<string[]>([]);
  const didDragRef = useRef(false);
  const dragScrollFrameRef = useRef<number | null>(null);
  const previousBackgroundDraftRef = useRef('');
  const boardBackgroundDisplayRef = useRef<{ fit: BoardBackgroundFit; position: BoardBackgroundPosition }>({ fit: 'cover', position: 'center' });
  const dragScrollTargetRef = useRef<{ element: HTMLDivElement; direction: -1 | 1 } | null>(null);
  const boardRef = useRef<HTMLElement | null>(null);
  const boardDragScrollFrameRef = useRef<number | null>(null);
  const boardDragScrollDirectionRef = useRef<-1 | 1 | null>(null);
  const boardPanRef = useRef<{ pointerId: number; startX: number; startScrollLeft: number; moved: boolean } | null>(null);
  const diagramCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const importFileRef = useRef<HTMLInputElement | null>(null);
  const boardBackgroundFileRef = useRef<HTMLInputElement | null>(null);
  const cardBackgroundFileRef = useRef<HTMLInputElement | null>(null);
  const isDrawingRef = useRef(false);
  const diagramStartRef = useRef<DiagramPoint | null>(null);
  const diagramInteractionRef = useRef<DiagramInteraction | null>(null);
  const selectedCardId = selected?.id;
  const isPublicViewer = authState === 'public' || !canEditBoard;
  const dueDays = useMemo(() => calendarDays(dueCursor), [dueCursor]);
  const currentMember = account ? memberFromApi({ id: account.user.id, username: account.user.username, avatar_url: account.user.avatar_url }) : { id: '', initials: '—', color: 'violet', name: 'Пользователь' };
  const boardBackgroundStyle = view === 'board' && boardBackgroundUrl ? { backgroundImage: `linear-gradient(rgb(18 17 16 / 48%), rgb(18 17 16 / 72%)), url("${assetUrl(boardBackgroundUrl)}")`, backgroundSize: boardBackgroundFit === 'fill' ? '100% 100%' : boardBackgroundFit, backgroundPosition: boardBackgroundPosition === 'top' ? 'center top' : boardBackgroundPosition === 'bottom' ? 'center bottom' : 'center', backgroundRepeat: 'no-repeat' } : undefined;

  const visibleColumns = useMemo(() => columns.map((column) => {
    const cards = column.cards.filter((card) => {
      if (!card.title.toLowerCase().includes(query.toLowerCase())) return false;
      if (filterMode === 'assigned') return card.members.some((member) => member.id === currentMember.id);
      if (filterMode === 'due') return Boolean(card.dueAt);
      if (filterMode === 'overdue') return Boolean(card.dueAt && new Date(card.dueAt).getTime() < Date.now());
      return true;
    });
    if (cardSort === 'manual') return { ...column, cards };
    const activityTime = (card: Card) => card.lastActivityAt ? new Date(card.lastActivityAt).getTime() || 0 : 0;
    return { ...column, cards: [...cards].sort((left, right) => cardSort === 'priority'
      ? (right.priority ?? 0) - (left.priority ?? 0)
      : activityTime(right) - activityTime(left)) };
  }).sort((left, right) => left.gridColumn - right.gridColumn || left.gridRow - right.gridRow), [cardSort, columns, currentMember.id, filterMode, query]);

  useEffect(() => {
    const board = boardRef.current;
    if (!board) return;
    board.querySelectorAll<HTMLElement>(':scope > .column').forEach((element, index) => {
      const column = visibleColumns[index];
      if (!column) return;
      element.style.gridColumn = String(column.gridColumn);
      element.style.gridRow = String(column.gridRow);
    });
  }, [visibleColumns]);

  useEffect(() => {
    async function connectToApi() {
      try {
        const setupRequest = fetch(`${API_URL}/v1/auth/setup`);
        if (sharedBoardId) {
          const [sharedResponse, setup, identity] = await Promise.all([
            fetch(`${API_URL}/v1/boards/${sharedBoardId}`),
            setupRequest,
            fetch(`${API_URL}/v1/auth/state`),
          ]);
          if (!setup.ok) throw new Error('identity setup is unavailable');
          const { registration_open } = await setup.json() as { registration_open: boolean };
          setRegistrationOpen(registration_open);
          if (!sharedResponse.ok) throw new Error('shared board could not be loaded');
          applyBoard(await sharedResponse.json() as ApiBoard);
          setWorkspaceName('Публичный доступ');
          if (!identity.ok) throw new Error('identity service is unavailable');
          const account = await identity.json() as AuthAccount | null;
          setAccount(account);
          setAuthState(account ? 'signed-in' : 'public');
          setView('board'); setPersistence('connected'); return;
        }
        const [me, setup] = await Promise.all([fetch(`${API_URL}/v1/auth/me`), setupRequest]);
        if (!setup.ok) throw new Error('identity setup is unavailable');
        const { registration_open } = await setup.json() as { registration_open: boolean };
        setRegistrationOpen(registration_open);
        if (me.status === 401) { setAuthMode(inviteToken || registration_open ? 'register' : 'login'); setAuthState('signed-out'); return; }
        if (!me.ok) throw new Error('identity service is unavailable');
        setAccount(await me.json() as AuthAccount);
        setAuthState('signed-in');
        const spacesResponse = await fetch(`${API_URL}/v1/workspaces`);
        if (!spacesResponse.ok) throw new Error('workspaces could not be loaded');
        const spaces = await spacesResponse.json() as Workspace[];
        setWorkspaces(spaces);
        const rememberedWorkspaceId = typeof window === 'undefined' ? null : window.localStorage.getItem('flowboard.workspace_id');
        const workspace = spaces.find((item) => item.id === rememberedWorkspaceId) ?? spaces[0];
        if (!workspace) { setWorkspaceId(null); setWorkspaceName(''); setBoards([]); setBoardId(null); setColumns([]); setView('home'); setPersistence('connected'); return; }
        setWorkspaceId(workspace.id); setWorkspaceName(workspace.name);
        const boardsResponse = await fetch(`${API_URL}/v1/workspaces/${workspace.id}/boards`);
        if (!boardsResponse.ok) throw new Error('boards could not be loaded');
        const availableBoards = await boardsResponse.json() as BoardSummary[];
        setBoards(availableBoards);
        setBoardId(null); setColumns([]); setBoardTitle(''); setView('home'); setPersistence('connected');
      } catch {
        setPersistence('connecting');
        setAccount(null);
        setAuthError('Сервис временно недоступен. Проверьте, что API и база данных запущены, затем войдите снова.');
        setAuthState('signed-out');
      }
    }
    void connectToApi();
  }, [inviteToken, sharedBoardId]);

  useEffect(() => {
    if (persistence !== 'connected' || typeof selectedCardId !== 'string') return;
    let cancelled = false;
    void fetch(`${API_URL}/v1/cards/${selectedCardId}/details`)
      .then(async (response) => {
        if (!response.ok) throw new Error('details failed');
        return response.json() as Promise<CardDetail>;
      })
      .then((detail) => {
        if (cancelled) return;
        setChecklists(detail.checklists);
        setComments(detail.comments);
        setAttachments(detail.attachments);
        setActivity(detail.activity);
        setUnreadMentionSourceIds((current) => [...new Set([...current, ...detail.unread_mention_source_ids])]);
        const checklistItems = detail.checklists.flatMap((checklist) => checklist.items);
        const cover = detail.attachments.find((attachment) => attachment.id === detail.cover_attachment_id);
        const cardMeta = { checklist: checklistItems.length ? `${checklistItems.filter((item) => item.is_completed).length}/${checklistItems.length}` : undefined, comments: detail.comments.length || undefined, attachments: detail.attachments.length || undefined, coverAttachmentId: detail.cover_attachment_id ?? undefined, coverMode: detail.cover_mode, coverUrl: cover?.url, backgroundImageUrl: detail.background_image_url ?? undefined };
        setCoverModeDraft(detail.cover_mode);
        setSelected((current) => current ? { ...current, ...cardMeta } : current);
        setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => card.id === selectedCardId ? { ...card, ...cardMeta } : card) })));
        if (detail.unread_mention_source_ids.length && account && !isPublicViewer) {
          setSelected((current) => current?.id === selectedCardId ? { ...current, hasUnreadMentions: false } : current);
          setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => card.id === selectedCardId ? { ...card, hasUnreadMentions: false } : card) })));
          void fetch(`${API_URL}/v1/cards/${selectedCardId}/mentions/read`, { method: 'POST' });
        }
      })
      .catch(() => { if (!cancelled) showToast('Не удалось загрузить детали карточки'); })
      .finally(() => { if (!cancelled) setDetailsLoading(false); });
    return () => { cancelled = true; };
  }, [selectedCardId, persistence, cardDetailRevision, account, isPublicViewer]);

  useEffect(() => {
    const items = checklists.flatMap((checklist) => checklist.items);
    document.querySelectorAll<HTMLButtonElement>('.check-item').forEach((button, index) => {
      const hasDescription = Boolean(items[index]?.description.trim());
      button.dataset.hasDescription = String(hasDescription);
      button.title = hasDescription ? 'У пункта есть описание' : '';
    });
  }, [checklists]);

  useEffect(() => {
    const openAttachmentPreview = (event: MouseEvent) => {
      const image = event.target instanceof Element ? event.target.closest<HTMLImageElement>('.attachment-preview img') : null;
      if (!image) return;
      const name = image.alt || image.closest('figure')?.querySelector('figcaption span')?.textContent || 'Изображение';
      setImagePreview({ url: image.currentSrc || image.src, name });
    };
    document.addEventListener('click', openAttachmentPreview);
    return () => document.removeEventListener('click', openAttachmentPreview);
  }, []);

  useEffect(() => {
    if (persistence !== 'connected' || !boardId) return;
    let refreshTimer: number | undefined;
    const stream = new EventSource(`${API_URL}/v1/boards/${boardId}/events`, { withCredentials: true });
    const refresh = () => {
      window.clearTimeout(refreshTimer);
      refreshTimer = window.setTimeout(() => {
        void fetch(`${API_URL}/v1/boards/${boardId}`).then(async (response) => {
          if (!response.ok) throw new Error('realtime refresh failed');
          applyBoard(await response.json() as ApiBoard);
          // The board payload only has comment counters. Reload an open card as
          // well, so Discord/API comments appear without closing the modal.
          if (typeof selectedCardId === 'string') setCardDetailRevision((current) => current + 1);
        }).catch(() => undefined);
      }, 180);
    };
    stream.addEventListener('refresh', refresh);
    stream.addEventListener('access-revoked', () => { stream.close(); setSelected(null); setView('home'); showToast('Доступ к пространству отозван'); });
    return () => { window.clearTimeout(refreshTimer); stream.close(); };
  }, [boardId, persistence, isPublicViewer, selectedCardId]);

  useEffect(() => {
    const canvas = diagramCanvasRef.current;
    if (!isDiagramOpen || !canvas) return;
    const context = canvas.getContext('2d');
    if (!context) return;
    context.clearRect(0, 0, canvas.width, canvas.height);
    context.fillStyle = '#171923'; context.fillRect(0, 0, canvas.width, canvas.height);
    diagramStrokes.forEach((stroke) => {
      const [first, ...rest] = stroke.points;
      if (!first) return;
      context.save();
      context.strokeStyle = stroke.color ?? '#9ea7ff';
      context.lineWidth = stroke.width ?? 3;
      context.lineCap = 'round';
      context.lineJoin = 'round';
      context.beginPath();
      context.moveTo(first.x, first.y);
      rest.forEach((point) => context.lineTo(point.x, point.y));
      context.stroke();
      context.restore();
    });
    diagramElements.forEach((element) => drawDiagramElement(context, element));
    if (diagramPreview) drawDiagramElement(context, diagramPreview);
    if (selectedDiagramElement !== null && diagramElements[selectedDiagramElement]) drawDiagramSelection(context, diagramElements[selectedDiagramElement]);
  }, [diagramElements, diagramPreview, diagramStrokes, isDiagramOpen, selectedDiagramElement]);

  useEffect(() => {
    if (!isDiagramOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'z' && !target?.matches('input, textarea, select')) {
        event.preventDefault();
        undoDiagram();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [diagramHistory, isDiagramOpen]);

  useEffect(() => {
    if (!isBoardMenuOpen && !isFilterOpen && !isBoardLabelsOpen && !isSortOpen && !sidebarPanel && !columnMenuId) return;
    const closePopovers = (event: PointerEvent) => {
      if (!(event.target instanceof Element)) return;
      if (isBoardMenuOpen && !event.target.closest('.board-menu-control')) setBoardMenuOpen(false);
      if (isFilterOpen && !event.target.closest('.filter-control')) setFilterOpen(false);
      if (isBoardLabelsOpen && !event.target.closest('.board-labels-control')) { setBoardLabelsOpen(false); setEditingBoardLabel(null); }
      if (isSortOpen && !event.target.closest('.board-sort-control')) setSortOpen(false);
      if (columnMenuId && !event.target.closest('.column-actions')) setColumnMenuId(null);
      if (sidebarPanel && !event.target.closest('.property-popover, .quick-action, .member-plus, .label-plus')) setSidebarPanel(null);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setBoardMenuOpen(false); setFilterOpen(false); setBoardLabelsOpen(false); setEditingBoardLabel(null); setSortOpen(false); setColumnMenuId(null); setSidebarPanel(null);
    };
    window.addEventListener('pointerdown', closePopovers);
    window.addEventListener('keydown', closeOnEscape);
    return () => { window.removeEventListener('pointerdown', closePopovers); window.removeEventListener('keydown', closeOnEscape); };
  }, [columnMenuId, isBoardLabelsOpen, isBoardMenuOpen, isFilterOpen, isSortOpen, sidebarPanel]);

  useEffect(() => {
    if (!cardContextMenu && !columnContextMenu) return;
    const close = () => { setCardContextMenu(null); setColumnContextMenu(null); };
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    window.addEventListener('pointerdown', close);
    window.addEventListener('keydown', closeOnEscape);
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('keydown', closeOnEscape); };
  }, [cardContextMenu, columnContextMenu]);

  useEffect(() => {
    if (!selected || !cardTitleDraft.trim() || (selected.title === cardTitleDraft.trim() && (selected.description ?? '') === cardDescriptionDraft)) return;
    const timer = window.setTimeout(() => {
      const updated = { title: cardTitleDraft.trim(), description: cardDescriptionDraft };
      persistCardDraft(selected, updated);
    }, 650);
    return () => window.clearTimeout(timer);
  }, [cardDescriptionDraft, cardTitleDraft, persistence, selected]);

  useEffect(() => {
    const previous = previousBackgroundDraftRef.current;
    previousBackgroundDraftRef.current = backgroundDraft;
    if (previous.trim() && !backgroundDraft.trim() && boardBackgroundUrl) clearBoardBackground();
  }, [backgroundDraft, boardBackgroundUrl]);

  useEffect(() => {
    const previous = boardBackgroundDisplayRef.current;
    if (!boardId || (previous.fit === boardBackgroundFit && previous.position === boardBackgroundPosition)) return;
    const next = { fit: boardBackgroundFit, position: boardBackgroundPosition };
    boardBackgroundDisplayRef.current = next;
    const background_image_url = backgroundDraft.trim() || boardBackgroundUrl?.replace(/\?v=[^&]+$/, '') || null;
    void fetch(`${API_URL}/v1/boards/${boardId}/background`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ background_image_url, background_fit: next.fit, background_position: next.position }) })
      .then((response) => { if (!response.ok) throw new Error('background display save failed'); })
      .catch(() => { boardBackgroundDisplayRef.current = previous; setBoardBackgroundFit(previous.fit); setBoardBackgroundPosition(previous.position); showToast('Не удалось сохранить отображение фона'); });
  }, [backgroundDraft, boardBackgroundFit, boardBackgroundPosition, boardBackgroundUrl, boardId]);

  function showToast(message: string) { setToast(message); window.setTimeout(() => setToast(''), 2600); }
  function persistCardDraft(card: Card, updated: { title: string; description: string }) {
    setCardSaveStatus('saving');
    setSelected((current) => current?.id === card.id ? { ...current, ...updated } : current);
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((item) => item.id === card.id ? { ...item, ...updated } : item) })));
    if (persistence !== 'connected' || typeof card.id !== 'string') { setCardSaveStatus('saved'); return; }
    void fetch(`${API_URL}/v1/cards/${card.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(updated) })
      .then((response) => { if (!response.ok) throw new Error('auto save failed'); setCardSaveStatus('saved'); })
      .catch(() => { setCardSaveStatus('error'); showToast('Изменения не сохранились'); });
  }
  function closeSelectedCard() {
    const card = selected;
    const title = cardTitleDraft.trim();
    if (card && title && (card.title !== title || (card.description ?? '') !== cardDescriptionDraft)) {
      persistCardDraft(card, { title, description: cardDescriptionDraft });
    }
    setSelected(null);
  }
  function applyChecklists(nextChecklists: Checklist[]) {
    setChecklists(nextChecklists);
    if (!selectedCardId) return;
    const items = nextChecklists.flatMap((checklist) => checklist.items);
    const checklist = items.length ? `${items.filter((item) => item.is_completed).length}/${items.length}` : undefined;
    setSelected((current) => current ? { ...current, checklist } : current);
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => card.id === selectedCardId ? { ...card, checklist } : card) })));
  }
  function updateBoardRoute(nextBoardId: string | null, replace = false) {
    if (typeof window === 'undefined') return;
    const url = new URL(window.location.href);
    if (nextBoardId) url.searchParams.set('board', nextBoardId); else url.searchParams.delete('board');
    window.history[replace ? 'replaceState' : 'pushState']({}, '', `${url.pathname}${url.search}${url.hash}`);
    setSharedBoardId(nextBoardId);
  }
  function openHome() { updateBoardRoute(null); setView('home'); setQuery(''); }
  function openBoard() { setView('board'); setQuery(''); }
  function stopCardListAutoScroll() {
    dragScrollTargetRef.current = null;
    if (dragScrollFrameRef.current !== null) {
      window.cancelAnimationFrame(dragScrollFrameRef.current);
      dragScrollFrameRef.current = null;
    }
  }
  function startCardListAutoScroll() {
    if (dragScrollFrameRef.current !== null) return;
    const scroll = () => {
      const target = dragScrollTargetRef.current;
      if (!target) { dragScrollFrameRef.current = null; return; }
      const previousTop = target.element.scrollTop;
      target.element.scrollTop += target.direction * 12;
      if (target.element.scrollTop === previousTop) { dragScrollFrameRef.current = null; return; }
      dragScrollFrameRef.current = window.requestAnimationFrame(scroll);
    };
    dragScrollFrameRef.current = window.requestAnimationFrame(scroll);
  }
  function updateCardListAutoScroll(event: ReactDragEvent<HTMLElement>) {
    const column = event.currentTarget.closest('.column');
    const list = column?.querySelector<HTMLDivElement>('.card-list');
    if (!list) return;
    const bounds = list.getBoundingClientRect();
    const edge = Math.min(62, Math.max(36, bounds.height * 0.18));
    const direction: -1 | 1 | null = event.clientY < bounds.top + edge ? -1 : event.clientY > bounds.bottom - edge ? 1 : null;
    if (!direction) { stopCardListAutoScroll(); return; }
    dragScrollTargetRef.current = { element: list, direction };
    startCardListAutoScroll();
  }
  function stopBoardAutoScroll() {
    boardDragScrollDirectionRef.current = null;
    if (boardDragScrollFrameRef.current !== null) { window.cancelAnimationFrame(boardDragScrollFrameRef.current); boardDragScrollFrameRef.current = null; }
  }
  function startBoardAutoScroll() {
    if (boardDragScrollFrameRef.current !== null) return;
    const scroll = () => {
      const board = boardRef.current;
      const direction = boardDragScrollDirectionRef.current;
      if (!board || !direction) { boardDragScrollFrameRef.current = null; return; }
      const previous = board.scrollLeft;
      board.scrollLeft += direction * 18;
      if (board.scrollLeft === previous) { boardDragScrollFrameRef.current = null; return; }
      boardDragScrollFrameRef.current = window.requestAnimationFrame(scroll);
    };
    boardDragScrollFrameRef.current = window.requestAnimationFrame(scroll);
  }
  function startBoardPan(event: ReactPointerEvent<HTMLElement>) {
    if (event.button !== 0 || event.pointerType !== 'mouse' || dragging || draggingColumnId) return;
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest('.column-head, button, input, textarea, select, a, .composer') || (!isPublicViewer && target?.closest('.task-card'))) return;
    const board = event.currentTarget;
    boardPanRef.current = { pointerId: event.pointerId, startX: event.clientX, startScrollLeft: board.scrollLeft, moved: false };
  }
  function moveBoardPan(event: ReactPointerEvent<HTMLElement>) {
    const pan = boardPanRef.current;
    if (!pan || pan.pointerId !== event.pointerId) return;
    if (Math.abs(event.clientX - pan.startX) <= 4 && !pan.moved) return;
    if (!pan.moved) {
      pan.moved = true;
      event.currentTarget.setPointerCapture(event.pointerId);
      setBoardPanning(true);
    }
    event.currentTarget.scrollLeft = pan.startScrollLeft - (event.clientX - pan.startX);
    event.preventDefault();
  }
  function stopBoardPan(event: ReactPointerEvent<HTMLElement>) {
    const pan = boardPanRef.current;
    if (!pan || pan.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    boardPanRef.current = null;
    setBoardPanning(false);
    if (pan.moved) {
      didDragRef.current = true;
      window.setTimeout(() => { didDragRef.current = false; }, 0);
    }
  }
  function updateBoardAutoScroll(event: ReactDragEvent<HTMLElement>) {
    const board = boardRef.current;
    if (!board) return;
    const bounds = board.getBoundingClientRect();
    const edge = Math.min(100, Math.max(52, bounds.width * .12));
    const direction: -1 | 1 | null = event.clientX < bounds.left + edge ? -1 : event.clientX > bounds.right - edge ? 1 : null;
    if (!direction) { stopBoardAutoScroll(); return; }
    boardDragScrollDirectionRef.current = direction;
    startBoardAutoScroll();
  }
  function clearColumnDragState() {
    stopBoardAutoScroll();
    setDraggingColumnId(null);
    setColumnDropTarget(null);
  }
  function clearDragState() {
    stopCardListAutoScroll();
    setDragging(null);
    setDragOverListId(null);
    setDragDropTarget(null);
  }
  useEffect(() => () => {
    dragScrollTargetRef.current = null;
    if (dragScrollFrameRef.current !== null) window.cancelAnimationFrame(dragScrollFrameRef.current);
    stopBoardAutoScroll();
  }, []);
  function addCard(event: FormEvent, columnId: EntityId) {
    event.preventDefault();
    const title = draft.trim();
    if (!title) return;
    const card: Card = { id: nextCardId, title, labels: [], members: [] };
    setColumns((current) => current.map((column) => column.id === columnId ? { ...column, cards: [...column.cards, card] } : column));
    setNextCardId((current) => current + 1); setDraft(''); setComposerOpen(null); showToast('Задача добавлена в доску');
    if (persistence === 'connected' && typeof columnId === 'string') {
      void fetch(`${API_URL}/v1/lists/${columnId}/cards`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title }) })
        .then(async (response) => {
          if (!response.ok) throw new Error('save failed');
          return response.json() as Promise<{ id: string }>;
        })
        .then((saved) => setColumns((current) => current.map((column) => column.id === columnId ? { ...column, cards: column.cards.map((item) => item.id === card.id ? { ...item, id: saved.id } : item) } : column)))
        .catch(() => showToast('Задача добавлена локально; сервер недоступен'));
    }
  }
  function moveCard(cardId: EntityId, sourceListId: EntityId, targetListId: EntityId, beforeCardId?: EntityId) {
    const card = columns.find((column) => column.id === sourceListId)?.cards.find((item) => item.id === cardId);
    if (!card) return;
    setColumns((current) => {
      const withoutCard = current.map((column) => column.id === sourceListId ? { ...column, cards: column.cards.filter((item) => item.id !== cardId) } : column);
      return withoutCard.map((column) => {
        if (column.id !== targetListId) return column;
        const insertionIndex = beforeCardId === undefined ? column.cards.length : Math.max(0, column.cards.findIndex((item) => item.id === beforeCardId));
        const cards = [...column.cards];
        cards.splice(insertionIndex, 0, card);
        return { ...column, cards };
      });
    });
    clearDragState(); showToast(`«${card.title}» перемещена`);
    if (persistence === 'connected' && typeof cardId === 'string' && typeof targetListId === 'string') {
      void fetch(`${API_URL}/v1/cards/${cardId}/move`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ target_list_id: targetListId, before_card_id: typeof beforeCardId === 'string' ? beforeCardId : null }) })
        .then((response) => { if (!response.ok) throw new Error('move failed'); })
        .catch(() => showToast('Перемещение не сохранено: обновите доску'));
    }
  }
  function moveColumn(columnId: EntityId, beforeColumnId?: EntityId) {
    if (columnId === beforeColumnId) return;
    setColumns((current) => {
      const moving = current.find((column) => column.id === columnId);
      if (!moving) return current;
      const target = beforeColumnId === undefined ? Math.max(0, ...current.map((column) => column.gridColumn)) + 1 : current.find((column) => column.id === beforeColumnId)?.gridColumn;
      if (!target || moving.gridColumn === target) return current;
      return current.map((column) => column.id === moving.id ? { ...column, gridColumn: target, gridRow: 1 } : column.id !== moving.id && column.gridColumn >= target ? { ...column, gridColumn: column.gridColumn + 1 } : column);
    });
    clearColumnDragState();
    if (persistence === 'connected' && typeof columnId === 'string') {
      void fetch(`${API_URL}/v1/lists/${columnId}/move`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ before_list_id: typeof beforeColumnId === 'string' ? beforeColumnId : null }) })
        .then((response) => { if (!response.ok) throw new Error('list move failed'); })
        .catch(() => showToast('Порядок колонок не сохранён: обновите доску'));
    }
  }
  function moveColumnBelow(columnId: EntityId, belowColumnId: EntityId) {
    if (columnId === belowColumnId) return;
    setColumns((current) => {
      const moving = current.find((column) => column.id === columnId);
      const target = current.find((column) => column.id === belowColumnId);
      if (!moving || !target) return current;
      const gridRow = Math.max(0, ...current.filter((column) => column.id !== moving.id && column.gridColumn === target.gridColumn).map((column) => column.gridRow)) + 1;
      return current.map((column) => column.id === moving.id ? { ...column, gridColumn: target.gridColumn, gridRow } : column);
    });
    clearColumnDragState();
    if (persistence === 'connected' && typeof columnId === 'string' && typeof belowColumnId === 'string') {
      void fetch(`${API_URL}/v1/lists/${columnId}/move`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ before_list_id: null, below_list_id: belowColumnId }) })
        .then((response) => { if (!response.ok) throw new Error('vertical list move failed'); showToast('Колонка перемещена ниже'); })
        .catch(() => showToast('Позиция колонки не сохранена: обновите доску'));
    }
  }
  function addColumn(belowColumn?: Column) {
    const title = `Новая колонка ${columns.length + 1}`;
    if (persistence === 'connected' && boardId) {
      void fetch(`${API_URL}/v1/boards/${boardId}/lists`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title, below_list_id: typeof belowColumn?.id === 'string' ? belowColumn.id : null }) })
        .then(async (response) => { if (!response.ok) throw new Error('save failed'); return response.json() as Promise<{ id: string; title: string; grid_column: number; grid_row: number }>; })
        .then((saved) => { setColumns((current) => [...current, { id: saved.id, title: saved.title, gridColumn: saved.grid_column, gridRow: saved.grid_row, cards: [] }]); showToast(belowColumn ? 'Колонка добавлена ниже' : 'Колонка добавлена'); })
        .catch(() => showToast('Не удалось сохранить колонку'));
      return;
    }
    setColumns((current) => {
      const gridColumn = belowColumn?.gridColumn ?? Math.max(0, ...current.map((column) => column.gridColumn)) + 1;
      const gridRow = belowColumn ? Math.max(0, ...current.filter((column) => column.gridColumn === gridColumn).map((column) => column.gridRow)) + 1 : 1;
      return [...current, { id: current.length + 1, title, gridColumn, gridRow, cards: [] }];
    }); showToast(belowColumn ? 'Колонка добавлена ниже' : 'Колонка добавлена');
  }
  useEffect(() => {
    const board = boardRef.current;
    if (!board || !draggingColumnId) return;
    const columnsInBoard = () => Array.from(board.querySelectorAll<HTMLElement>(':scope > .column'));
    const clearDropHint = () => columnsInBoard().forEach((column) => column.classList.remove('column-drop-below-target'));
    const targetForEvent = (event: DragEvent) => {
      const target = event.target instanceof Element ? event.target.closest<HTMLElement>('.column') : null;
      if (!target || !board.contains(target)) return null;
      const bounds = target.getBoundingClientRect();
      return event.clientY >= bounds.bottom - Math.min(76, bounds.height * .28) ? target : null;
    };
    const onDragOver = (event: DragEvent) => {
      const target = targetForEvent(event);
      if (!target) { clearDropHint(); return; }
      event.preventDefault(); event.stopPropagation(); clearDropHint(); target.classList.add('column-drop-below-target');
    };
    const onDrop = (event: DragEvent) => {
      const target = targetForEvent(event);
      if (!target) return;
      event.preventDefault(); event.stopPropagation();
      const index = columnsInBoard().indexOf(target);
      const belowColumn = visibleColumns[index];
      clearDropHint();
      if (belowColumn) moveColumnBelow(draggingColumnId, belowColumn.id);
    };
    const onDragEnd = () => clearDropHint();
    board.addEventListener('dragover', onDragOver, true);
    board.addEventListener('drop', onDrop, true);
    window.addEventListener('dragend', onDragEnd);
    return () => { board.removeEventListener('dragover', onDragOver, true); board.removeEventListener('drop', onDrop, true); window.removeEventListener('dragend', onDragEnd); clearDropHint(); };
  }, [draggingColumnId, visibleColumns]);

  function openCard(card: Card) {
    setSelected(card);
    setEditingCardDescription(false);
    setChecklists([]);
    setChecklistNameDraft('');
    setChecklistItemDrafts({});
    setExpandedChecklistItemIds([]);
    setChecklistItemDescriptionDrafts({});
    setComments([]);
    setReplyToCommentId(null);
    setEditingCommentId(null);
    setCommentEditDraft('');
    setAttachments([]);
    setActivity([]);
    setUnreadMentionSourceIds([]);
    setDetailsLoading(persistence === 'connected' && typeof card.id === 'string');
    setCardTitleDraft(card.title);
    setCardDescriptionDraft(card.description ?? '');
    const due = card.dueAt ? new Date(card.dueAt) : new Date();
    setDueCursor(due);
    setDueTime(card.dueAt ? `${String(due.getHours()).padStart(2, '0')}:${String(due.getMinutes()).padStart(2, '0')}` : '09:00');
    setSidebarPanel(null);
    setCommentDraft('');
  }
  function archiveCard(card: Card) {
    if (persistence === 'connected' && typeof card.id === 'string') {
      void fetch(`${API_URL}/v1/cards/${card.id}`, { method: 'DELETE' })
        .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось архивировать задачу'); setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.filter((item) => item.id !== card.id) }))); setSelected((current) => current?.id === card.id ? null : current); showToast('Задача перемещена в архив'); })
        .catch((error) => showToast(error instanceof Error ? error.message : 'Не удалось сохранить архивирование'));
    } else { setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.filter((item) => item.id !== card.id) }))); setSelected((current) => current?.id === card.id ? null : current); showToast('Задача удалена'); }
  }
  function archiveSelectedCard() {
    if (!selected) return;
    archiveCard(selected);
  }
  function openArchive() {
    if (!boardId) return;
    setArchiveOpen(true);
    setArchiveLoading(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/archived-cards`)
      .then(async (response) => { if (!response.ok) throw new Error('archive load failed'); return response.json() as Promise<ArchivedCard[]>; })
      .then(setArchivedCards)
      .catch(() => showToast('Не удалось загрузить архив'))
      .finally(() => setArchiveLoading(false));
  }
  function restoreArchivedCard(card: ArchivedCard) {
    if (persistence !== 'connected') return;
    void fetch(`${API_URL}/v1/cards/${card.id}/restore`, { method: 'POST' })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось восстановить задачу'); return response.json() as Promise<{ id: string; list_id: string; title: string; description: string }>; })
      .then((restored) => {
        setArchivedCards((current) => current.filter((item) => item.id !== card.id));
        setColumns((current) => current.map((column) => column.id === restored.list_id
          ? { ...column, cards: [...column.cards, { id: restored.id, title: restored.title, description: restored.description, labels: [], members: [] }] }
          : column));
        showToast('Задача восстановлена');
      })
      .catch((error) => showToast(error instanceof Error ? error.message : 'Не удалось восстановить задачу'));
  }
  function openTeam() {
    if (!boardId) return;
    setTeamOpen(true);
    setTeamLoading(true);
    setAccountSearch('');
    void Promise.all([fetch(`${API_URL}/v1/boards/${boardId}/members`), fetch(`${API_URL}/v1/boards/${boardId}/available-accounts`)])
      .then(async ([members, accounts]) => { if (!members.ok || !accounts.ok) throw new Error('team load failed'); setAvailableAccounts(await accounts.json() as ApiMember[]); return members.json() as Promise<TeamMember[]>; })
      .then(setTeamMembers)
      .catch(() => { setTeamOpen(false); showToast('Управлять составом команды может только владелец'); })
      .finally(() => setTeamLoading(false));
  }
  function openAdmin() {
    setAdminOpen(true); setAdminLoading(true);
    void Promise.all([fetch(`${API_URL}/v1/admin/accounts`), fetch(`${API_URL}/v1/admin/account-invitations`), fetch(`${API_URL}/v1/admin/workspaces`)])
      .then(async ([accounts, invites, workspaces]) => { if (!accounts.ok || !invites.ok || !workspaces.ok) throw new Error('admin load failed'); setAdminAccounts(await accounts.json() as AdminAccount[]); setAdminInvites(await invites.json() as AccountInvite[]); setAdminWorkspaces(await workspaces.json() as AdminWorkspace[]); })
      .catch(() => { setAdminOpen(false); showToast('Недостаточно прав для админ-панели'); })
      .finally(() => setAdminLoading(false));
  }
  function createAccountInvite() {
    void fetch(`${API_URL}/v1/admin/account-invitations`, { method: 'POST' })
      .then(async (response) => { if (!response.ok) throw new Error('invite failed'); return response.json() as Promise<AccountInvite>; })
      .then((invite) => { setAdminInvites((current) => [invite, ...current]); const link = `${window.location.origin}${window.location.pathname}?invite=${encodeURIComponent(invite.token ?? '')}`; void navigator.clipboard?.writeText(link); showToast('Ссылка для активации скопирована'); })
      .catch(() => showToast('Не удалось создать invite'));
  }
  function openWorkspaceComposer() {
    setNewWorkspaceName('');
    setWorkspaceCreateError('');
    setWorkspaceComposerOpen(true);
  }
  function createWorkspace(event: FormEvent) {
    event.preventDefault();
    const name = newWorkspaceName.trim();
    if (!name) { setWorkspaceCreateError('Введите название пространства.'); return; }
    if (isCreatingWorkspace) return;
    setCreatingWorkspace(true);
    setWorkspaceCreateError('');
    void fetch(`${API_URL}/v1/workspaces`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name }) })
      .then(async (response) => { if (!response.ok) throw new Error('workspace create failed'); return response.json() as Promise<Workspace>; })
      .then((workspace) => { setWorkspaces((current) => [workspace, ...current]); setAdminWorkspaces((current) => [{ id: workspace.id, name: workspace.name, owner_username: account?.user.username ?? 'owner', member_count: 1, archived_at: null }, ...current]); setWorkspaceComposerOpen(false); void selectWorkspace(workspace); showToast(`Пространство «${workspace.name}» создано`); })
      .catch(() => setWorkspaceCreateError('Не удалось создать пространство. Проверьте подключение и повторите.'))
      .finally(() => setCreatingWorkspace(false));
  }
  function openSessions() {
    setSessionsOpen(true);
    void fetch(`${API_URL}/v1/auth/sessions`).then(async (response) => { if (!response.ok) throw new Error('sessions failed'); return response.json() as Promise<AuthSession[]>; })
      .then(setSessions).catch(() => { setSessionsOpen(false); showToast('Не удалось загрузить сессии'); });
  }
  function revokeSession(session: AuthSession) {
    void fetch(`${API_URL}/v1/auth/sessions/${session.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('revoke failed'); setSessions((current) => current.filter((item) => item.id !== session.id)); })
      .catch(() => showToast('Не удалось отозвать сессию'));
  }
  function revokeOtherSessions() {
    void fetch(`${API_URL}/v1/auth/sessions`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('revoke failed'); setSessions((current) => current.filter((item) => item.current)); showToast('Другие сессии отозваны'); })
      .catch(() => showToast('Не удалось отозвать сессии'));
  }
  function deleteAccount(account: AdminAccount) {
    if (!window.confirm(`Удалить @${account.username}? Это отзовёт сессии и удалит доступ ко всем workspace. Действие нельзя отменить.`)) return;
    void fetch(`${API_URL}/v1/admin/accounts/${account.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('account delete failed'); setAdminAccounts((current) => current.filter((item) => item.id !== account.id)); showToast(`@${account.username} удалён`); })
      .catch(() => showToast('Не удалось удалить аккаунт'));
  }
  function archiveWorkspace(workspace: AdminWorkspace) {
    void fetch(`${API_URL}/v1/admin/workspaces/${workspace.id}/archive`, { method: 'PATCH' })
      .then(async (response) => { if (!response.ok) throw new Error('workspace archive failed'); return response.json() as Promise<AdminWorkspace>; })
      .then((updated) => setAdminWorkspaces((current) => current.map((item) => item.id === updated.id ? updated : item)))
      .catch(() => showToast('Не удалось изменить статус пространства'));
  }
  function deleteWorkspace(workspace: AdminWorkspace) {
    if (!window.confirm(`Удалить пространство «${workspace.name}»? Будут безвозвратно удалены все доски, карточки, схемы и вложения.`)) return;
    void fetch(`${API_URL}/v1/admin/workspaces/${workspace.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('workspace delete failed'); setAdminWorkspaces((current) => current.filter((item) => item.id !== workspace.id)); showToast(`Пространство «${workspace.name}» удалено`); })
      .catch(() => showToast('Не удалось удалить пространство'));
  }
  function changeTeamPreset(member: TeamMember, preset: TeamMember['preset']) {
    setTeamMembers((current) => current.map((item) => item.id === member.id ? { ...item, preset } : item));
    if (!boardId) return;
    void fetch(`${API_URL}/v1/boards/${boardId}/members/${member.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ preset }) })
      .then((response) => { if (!response.ok) throw new Error('preset update failed'); })
      .catch(() => showToast('Не удалось изменить набор прав'));
  }
  function removeTeamMember(member: TeamMember) {
    setTeamMembers((current) => current.filter((item) => item.id !== member.id));
    setWorkspaceMembers((current) => current.filter((item) => item.id !== member.id));
    if (!boardId) return;
    void fetch(`${API_URL}/v1/boards/${boardId}/members/${member.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('member remove failed'); showToast('Участник исключён из пространства'); })
      .catch(() => showToast('Не удалось исключить участника'));
  }
  function updateSelectedCard(patch: Partial<Card>) {
    if (!selected) return;
    const updated = { ...selected, ...patch };
    setSelected(updated);
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => card.id === updated.id ? { ...card, ...patch } : card) })));
  }
  function saveDueDate(day: Date, time: string) {
    if (!selected || isSavingDueAt) return;
    const dueAt = dueDateFrom(day, time);
    setDueCursor(day);
    setDueTime(time);
    setSidebarPanel(null);
    updateSelectedCard({ dueAt });
    if (persistence !== 'connected' || typeof selected.id !== 'string') { showToast('Дедлайн сохранён'); return; }
    setSavingDueAt(true);
    void fetch(`${API_URL}/v1/cards/${selected.id}/due-date`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ due_at: dueAt }) })
      .then((response) => { if (!response.ok) throw new Error('due date save failed'); showToast('Дедлайн сохранён'); })
      .catch(() => showToast('Не удалось сохранить дедлайн'))
      .finally(() => setSavingDueAt(false));
  }
  function clearDueDate() {
    if (!selected) return;
    updateSelectedCard({ dueAt: undefined });
    setSidebarPanel(null);
    if (persistence === 'connected' && typeof selected.id === 'string') {
      void fetch(`${API_URL}/v1/cards/${selected.id}/due-date`, { method: 'DELETE' })
        .then((response) => { if (!response.ok) throw new Error('due date clear failed'); showToast('Дедлайн снят'); })
        .catch(() => showToast('Не удалось снять дедлайн'));
    }
  }
  function replaceSelectedLabels(labels: Label[]) {
    if (!selected) return;
    updateSelectedCard({ labels });
    if (persistence === 'connected' && typeof selected.id === 'string') {
      void fetch(`${API_URL}/v1/cards/${selected.id}/labels`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ label_ids: labels.map((label) => label.id) }) })
        .then(async (response) => { if (!response.ok) throw new Error('labels save failed'); return response.json() as Promise<Label[]>; })
        .then((saved) => updateSelectedCard({ labels: saved }))
        .catch(() => showToast('Не удалось сохранить метки'));
    }
  }
  function toggleSelectedLabel(label: Label) {
    if (!selected) return;
    const exists = selected.labels.some((current) => current.id === label.id);
    replaceSelectedLabels(exists ? selected.labels.filter((current) => current.id !== label.id) : [...selected.labels, label]);
  }
  function createLabel(event: FormEvent) {
    event.preventDefault();
    const name = newLabelName.trim();
    if (!boardId || !name || isSavingLabel) return;
    if (persistence !== 'connected') { const label = { id: `local-label-${Date.now()}`, name, color: newLabelColor }; setBoardLabels((current) => [...current, label]); setNewLabelName(''); toggleSelectedLabel(label); return; }
    setSavingLabel(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/labels`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, color: newLabelColor }) })
      .then(async (response) => { if (!response.ok) throw new Error('label save failed'); return response.json() as Promise<Label>; })
      .then((label) => { setBoardLabels((current) => current.some((item) => item.id === label.id) ? current.map((item) => item.id === label.id ? label : item) : [...current, label]); setNewLabelName(''); toggleSelectedLabel(label); })
      .catch(() => showToast('Не удалось создать метку'))
      .finally(() => setSavingLabel(false));
  }
  function beginBoardLabelEdit(label: Label) {
    setEditingBoardLabel(label);
    setBoardLabelNameDraft(label.name);
    setBoardLabelColorDraft(label.color);
  }
  function saveBoardLabel(event: FormEvent) {
    event.preventDefault();
    const label = editingBoardLabel;
    const name = boardLabelNameDraft.trim();
    if (!label || !name || isSavingBoardLabel) return;
    if (persistence !== 'connected') {
      const saved = { ...label, name, color: boardLabelColorDraft };
      setBoardLabels((current) => current.map((item) => item.id === saved.id ? saved : item));
      setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => ({ ...card, labels: card.labels.map((item) => item.id === saved.id ? saved : item) })) })));
      setSelected((current) => current ? { ...current, labels: current.labels.map((item) => item.id === saved.id ? saved : item) } : current);
      setEditingBoardLabel(null);
      return;
    }
    setSavingBoardLabel(true);
    void fetch(`${API_URL}/v1/labels/${label.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, color: boardLabelColorDraft }) })
      .then(async (response) => { if (!response.ok) throw new Error('label update failed'); return response.json() as Promise<Label>; })
      .then((saved) => {
        setBoardLabels((current) => current.map((item) => item.id === saved.id ? saved : item));
        setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => ({ ...card, labels: card.labels.map((item) => item.id === saved.id ? saved : item) })) })));
        setSelected((current) => current ? { ...current, labels: current.labels.map((item) => item.id === saved.id ? saved : item) } : current);
        setEditingBoardLabel(null); showToast('Метка сохранена');
      })
      .catch(() => showToast('Не удалось сохранить метку'))
      .finally(() => setSavingBoardLabel(false));
  }
  function removeBoardLabel(label: Label) {
    if (isSavingBoardLabel) return;
    if (persistence !== 'connected') {
      setBoardLabels((current) => current.filter((item) => item.id !== label.id));
      setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => ({ ...card, labels: card.labels.filter((item) => item.id !== label.id) })) })));
      setSelected((current) => current ? { ...current, labels: current.labels.filter((item) => item.id !== label.id) } : current);
      setEditingBoardLabel(null);
      return;
    }
    setSavingBoardLabel(true);
    void fetch(`${API_URL}/v1/labels/${label.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('label delete failed'); setBoardLabels((current) => current.filter((item) => item.id !== label.id)); setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => ({ ...card, labels: card.labels.filter((item) => item.id !== label.id) })) }))); setSelected((current) => current ? { ...current, labels: current.labels.filter((item) => item.id !== label.id) } : current); setEditingBoardLabel(null); showToast('Метка удалена'); })
      .catch(() => showToast('Не удалось удалить метку'))
      .finally(() => setSavingBoardLabel(false));
  }
  function replaceSelectedMembers(members: Member[]) {
    if (!selected) return;
    updateSelectedCard({ members });
    if (persistence === 'connected' && typeof selected.id === 'string') {
      void fetch(`${API_URL}/v1/cards/${selected.id}/assignees`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ user_ids: members.filter((member) => typeof member.id === 'string').map((member) => member.id) }) })
        .then(async (response) => { if (!response.ok) throw new Error('assignees save failed'); return response.json() as Promise<ApiMember[]>; })
        .then((saved) => updateSelectedCard({ members: saved.map(memberFromApi) }))
        .catch(() => showToast('Не удалось сохранить исполнителей'));
    }
  }
  function toggleSelectedMember(member: Member) {
    if (!selected) return;
    const exists = selected.members.some((current) => current.id === member.id);
    replaceSelectedMembers(exists ? selected.members.filter((current) => current.id !== member.id) : [...selected.members, member]);
  }
  function openDiagram() {
    if (!selected || typeof selected.id !== 'string') return;
    setSidebarPanel(null);
    void fetch(`${API_URL}/v1/cards/${selected.id}/diagram`)
      .then(async (response) => { if (!response.ok) throw new Error('diagram load failed'); return response.json() as Promise<Diagram | null>; })
      .then((saved) => {
        setDiagram(saved);
        setDiagramTitle(saved?.title ?? 'Схема');
        setDiagramStrokes(saved?.document?.strokes ?? []);
        setDiagramElements(saved?.document?.elements ?? []);
        setDiagramPreview(null);
        setDiagramHistory([]);
        setSelectedDiagramElement(null);
        setDiagramTool('select');
        setDiagramZoom(.7);
        setDiagramOpen(true);
      })
      .catch(() => showToast('Не удалось загрузить схему'));
  }
  function diagramPoint(event: ReactPointerEvent<HTMLCanvasElement>): DiagramPoint | null {
    const canvas = diagramCanvasRef.current;
    if (!canvas) return null;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return null;
    return { x: (event.clientX - rect.left) * canvas.width / rect.width, y: (event.clientY - rect.top) * canvas.height / rect.height };
  }
  function rememberDiagramState() {
    setDiagramHistory((current) => [...current.slice(-49), { strokes: diagramStrokes, elements: diagramElements }]);
  }
  function undoDiagram() {
    const previous = diagramHistory.at(-1);
    if (!previous) return;
    setDiagramStrokes(previous.strokes);
    setDiagramElements(previous.elements);
    setDiagramHistory((current) => current.slice(0, -1));
    setSelectedDiagramElement(null);
    setDiagramPreview(null);
  }
  function diagramElementAtPoint(point: DiagramPoint) {
    for (let index = diagramElements.length - 1; index >= 0; index -= 1) {
      const element = diagramElements[index];
      const bounds = diagramBounds(element);
      if (element.type === 'arrow' && pointToSegmentDistance(point, { x: element.x, y: element.y }, { x: element.x2, y: element.y2 }) <= 13) return index;
      if (element.type === 'callout' && (pointToSegmentDistance(point, { x: element.x, y: element.y }, { x: element.x2, y: element.y2 }) <= 13 || (point.x >= element.x2 && point.x <= bounds.right && point.y >= element.y2 && point.y <= bounds.bottom))) return index;
      if (element.type === 'ellipse') {
        const radiusX = Math.max((bounds.right - bounds.left) / 2, 1);
        const radiusY = Math.max((bounds.bottom - bounds.top) / 2, 1);
        const centerX = (bounds.left + bounds.right) / 2;
        const centerY = (bounds.top + bounds.bottom) / 2;
        if (((point.x - centerX) / radiusX) ** 2 + ((point.y - centerY) / radiusY) ** 2 <= 1.2) return index;
      } else if (point.x >= bounds.left - 9 && point.x <= bounds.right + 9 && point.y >= bounds.top - 9 && point.y <= bounds.bottom + 9) return index;
    }
    return null;
  }
  function diagramHandleAtPoint(element: DiagramElement, point: DiagramPoint): DiagramHandle | null {
    const near = (target: DiagramPoint) => Math.hypot(point.x - target.x, point.y - target.y) <= 11;
    if (element.type === 'arrow' || element.type === 'callout') return near({ x: element.x, y: element.y }) ? 'start' : near({ x: element.x2, y: element.y2 }) ? 'end' : null;
    const bounds = diagramBounds(element);
    if (near({ x: bounds.left, y: bounds.top })) return 'nw';
    if (near({ x: bounds.right, y: bounds.top })) return 'ne';
    if (near({ x: bounds.right, y: bounds.bottom })) return 'se';
    if (near({ x: bounds.left, y: bounds.bottom })) return 'sw';
    return null;
  }
  function moveDiagramElement(element: DiagramElement, deltaX: number, deltaY: number): DiagramElement {
    if (element.type === 'arrow' || element.type === 'callout') return { ...element, x: element.x + deltaX, y: element.y + deltaY, x2: element.x2 + deltaX, y2: element.y2 + deltaY };
    return { ...element, x: element.x + deltaX, y: element.y + deltaY };
  }
  function resizeDiagramElement(element: DiagramElement, handle: DiagramHandle, point: DiagramPoint): DiagramElement {
    if (element.type === 'arrow' || element.type === 'callout') return handle === 'start' ? { ...element, x: point.x, y: point.y } : { ...element, x2: point.x, y2: point.y };
    if (element.type === 'text') {
      const bounds = diagramBounds(element);
      const initialWidth = Math.max(bounds.right - bounds.left, 1);
      const ratio = Math.max(.5, Math.min(3, Math.abs(point.x - bounds.left) / initialWidth));
      return { ...element, fontSize: Math.round(Math.max(12, Math.min(96, element.fontSize * ratio))) };
    }
    const bounds = diagramBounds(element);
    let { left, top, right, bottom } = bounds;
    if (handle === 'nw' || handle === 'sw') left = point.x;
    if (handle === 'ne' || handle === 'se') right = point.x;
    if (handle === 'nw' || handle === 'ne') top = point.y;
    if (handle === 'sw' || handle === 'se') bottom = point.y;
    return { ...element, x: Math.min(left, right), y: Math.min(top, bottom), width: Math.max(4, Math.abs(right - left)), height: Math.max(4, Math.abs(bottom - top)) };
  }
  function startDiagramStroke(event: ReactPointerEvent<HTMLCanvasElement>) {
    const point = diagramPoint(event);
    if (!point) return;
    if (diagramTool === 'select') {
      const index = diagramElementAtPoint(point);
      if (index === null) { setSelectedDiagramElement(null); return; }
      const element = diagramElements[index];
      const handle = selectedDiagramElement === index ? diagramHandleAtPoint(element, point) : null;
      setSelectedDiagramElement(index);
      event.currentTarget.setPointerCapture(event.pointerId);
      isDrawingRef.current = true;
      diagramInteractionRef.current = { kind: handle ? 'resize' : 'move', index, handle: handle ?? 'move', start: point, initial: element, historyStored: false };
      return;
    }
    if (diagramTool === 'text') {
      const text = diagramTextDraft.trim();
      if (!text) { showToast('Сначала напишите текст в панели схемы'); return; }
      rememberDiagramState();
      setDiagramElements((current) => [...current, { type: 'text', x: point.x, y: point.y, text, color: diagramColor, fontSize: diagramFontSize, fontFamily: diagramFontFamily, fontWeight: diagramFontWeight }]);
      setSelectedDiagramElement(diagramElements.length);
      return;
    }
    if (diagramTool === 'callout' && !diagramTextDraft.trim()) { showToast('Сначала напишите текст выноски в панели схемы'); return; }
    event.currentTarget.setPointerCapture(event.pointerId);
    isDrawingRef.current = true;
    if (diagramTool === 'draw') {
      rememberDiagramState();
      setDiagramStrokes((current) => [...current, { points: [point], color: diagramColor, width: diagramLineWidth }]);
      return;
    }
    diagramStartRef.current = point;
    setDiagramPreview(null);
  }

  function diagramElementFromDrag(tool: Exclude<DiagramTool, 'select' | 'draw' | 'text'>, start: DiagramPoint, end: DiagramPoint): DiagramElement {
    if (tool === 'arrow') return { type: 'arrow', x: start.x, y: start.y, x2: end.x, y2: end.y, color: diagramColor, lineWidth: diagramLineWidth };
    if (tool === 'callout') return { type: 'callout', x: start.x, y: start.y, x2: end.x, y2: end.y, text: diagramTextDraft.trim(), color: diagramColor, fontSize: diagramFontSize, fontFamily: diagramFontFamily, fontWeight: diagramFontWeight };
    return { type: tool, x: Math.min(start.x, end.x), y: Math.min(start.y, end.y), width: Math.abs(end.x - start.x), height: Math.abs(end.y - start.y), color: diagramColor, lineWidth: diagramLineWidth };
  }

  function continueDiagramStroke(event: ReactPointerEvent<HTMLCanvasElement>) {
    if (!isDrawingRef.current) return;
    const point = diagramPoint(event);
    if (!point) return;
    const interaction = diagramInteractionRef.current;
    if (interaction) {
      if (!interaction.historyStored && Math.hypot(point.x - interaction.start.x, point.y - interaction.start.y) > 1) { rememberDiagramState(); interaction.historyStored = true; }
      const element = interaction.kind === 'move' ? moveDiagramElement(interaction.initial, point.x - interaction.start.x, point.y - interaction.start.y) : resizeDiagramElement(interaction.initial, interaction.handle, point);
      setDiagramElements((current) => current.map((item, index) => index === interaction.index ? element : item));
      return;
    }
    if (diagramTool === 'draw') {
      setDiagramStrokes((current) => current.map((stroke, index) => index === current.length - 1 ? { ...stroke, points: [...stroke.points, point] } : stroke));
      return;
    }
    const start = diagramStartRef.current;
    if (start) setDiagramPreview(diagramElementFromDrag(diagramTool, start, point));
  }

  function finishDiagramInteraction(event: ReactPointerEvent<HTMLCanvasElement>) {
    if (!isDrawingRef.current) return;
    isDrawingRef.current = false;
    if (diagramInteractionRef.current) { diagramInteractionRef.current = null; return; }
    const start = diagramStartRef.current;
    const point = diagramPoint(event);
    if (diagramTool !== 'draw' && start && point) {
      const element = diagramElementFromDrag(diagramTool, start, point);
      const length = element.type === 'arrow' || element.type === 'callout' ? Math.hypot(element.x2 - element.x, element.y2 - element.y) : Math.max(element.width, element.height);
      if (length >= 5) { rememberDiagramState(); setDiagramElements((current) => [...current, element]); setSelectedDiagramElement(diagramElements.length); }
    }
    diagramStartRef.current = null;
    setDiagramPreview(null);
  }
  function saveDiagram() {
    if (!selected || typeof selected.id !== 'string' || !diagramTitle.trim() || isDiagramSaving) return;
    setDiagramSaving(true);
    void fetch(`${API_URL}/v1/cards/${selected.id}/diagram`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title: diagramTitle.trim(), document: { strokes: diagramStrokes, elements: diagramElements }, version: diagram?.version ?? null }) })
      .then(async (response) => { if (!response.ok) { const message = (await response.json().catch(() => null) as { message?: string } | null)?.message; throw new Error(message ?? 'diagram save failed'); } return response.json() as Promise<Diagram>; })
      .then((saved) => { setDiagram(saved); setDiagramOpen(false); showToast('Схема сохранена'); })
      .catch((error) => showToast(error instanceof Error ? error.message : 'Не удалось сохранить схему'))
      .finally(() => setDiagramSaving(false));
  }
  function addWorkspaceMember(event: FormEvent) {
    event.preventDefault();
    if (!boardId || !selectedAccountId || isSavingMember) return;
    setSavingMember(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/members/existing`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ user_id: selectedAccountId }) })
      .then(async (response) => { if (!response.ok) throw new Error('member save failed'); return response.json() as Promise<ApiMember>; })
      .then((member) => { setWorkspaceMembers((current) => [...current, memberFromApi(member)]); setAvailableAccounts((current) => current.filter((account) => account.id !== member.id)); setSelectedAccountId(''); setAccountSearch(''); showToast('Участник добавлен в проект'); })
      .catch(() => showToast('Не удалось добавить участника'))
      .finally(() => setSavingMember(false));
  }
  function createChecklist(event: FormEvent) {
    event.preventDefault();
    const title = checklistNameDraft.trim();
    if (!selected || !title || isSavingChecklist) return;
    if (persistence !== 'connected' || typeof selected.id !== 'string') {
      applyChecklists([...checklists, { id: `local-checklist-${Date.now()}`, title, items: [] }]);
      setChecklistNameDraft('');
      return;
    }
    setSavingChecklist(true);
    void fetch(`${API_URL}/v1/cards/${selected.id}/checklists`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title }) })
      .then(async (response) => { if (!response.ok) throw new Error('checklist save failed'); return response.json() as Promise<Checklist>; })
      .then((checklist) => { applyChecklists([...checklists, checklist]); setChecklistNameDraft(''); })
      .catch(() => showToast('Не удалось создать чек-лист'))
      .finally(() => setSavingChecklist(false));
  }
  function addChecklistItem(event: FormEvent, checklistId: string) {
    event.preventDefault();
    const title = (checklistItemDrafts[checklistId] ?? '').trim();
    if (!title || isSavingChecklist) return;
    if (persistence !== 'connected' || checklistId.startsWith('local-')) {
      applyChecklists(checklists.map((checklist) => checklist.id === checklistId ? { ...checklist, items: [...checklist.items, { id: `local-check-${Date.now()}`, title, is_completed: false, description: '', attachments: [] }] } : checklist));
      setChecklistItemDrafts((current) => ({ ...current, [checklistId]: '' }));
      return;
    }
    setSavingChecklist(true);
    void fetch(`${API_URL}/v1/checklists/${checklistId}/items`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title }) })
      .then(async (response) => { if (!response.ok) throw new Error('checklist item save failed'); return response.json() as Promise<ChecklistItem>; })
      .then((item) => { applyChecklists(checklists.map((checklist) => checklist.id === checklistId ? { ...checklist, items: [...checklist.items, item] } : checklist)); setChecklistItemDrafts((current) => ({ ...current, [checklistId]: '' })); })
      .catch(() => showToast('Не удалось добавить пункт чек-листа'))
      .finally(() => setSavingChecklist(false));
  }
  function deleteChecklist(checklist: Checklist) {
    applyChecklists(checklists.filter((item) => item.id !== checklist.id));
    if (persistence === 'connected' && !checklist.id.startsWith('local-')) {
      void fetch(`${API_URL}/v1/checklists/${checklist.id}`, { method: 'DELETE' })
        .then((response) => { if (!response.ok) throw new Error('checklist delete failed'); })
        .catch(() => showToast('Удаление чек-листа не сохранено'));
    }
  }
  function toggleChecklistItem(checklistId: string, item: ChecklistItem) {
    const next = !item.is_completed;
    applyChecklists(checklists.map((checklist) => checklist.id === checklistId ? { ...checklist, items: checklist.items.map((value) => value.id === item.id ? { ...value, is_completed: next } : value) } : checklist));
    if (persistence === 'connected' && typeof item.id === 'string') {
      void fetch(`${API_URL}/v1/checklist-items/${item.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ is_completed: next }) })
        .then((response) => { if (!response.ok) throw new Error('toggle failed'); })
        .catch(() => showToast('Статус пункта не сохранён'));
    }
  }
  function removeChecklistItem(checklistId: string, item: ChecklistItem) {
    applyChecklists(checklists.map((checklist) => checklist.id === checklistId ? { ...checklist, items: checklist.items.filter((value) => value.id !== item.id) } : checklist));
    if (persistence === 'connected' && typeof item.id === 'string') {
      void fetch(`${API_URL}/v1/checklist-items/${item.id}`, { method: 'DELETE' })
        .then((response) => { if (!response.ok) throw new Error('delete failed'); })
        .catch(() => showToast('Удаление пункта не сохранено'));
    }
  }
  function addComment(event: FormEvent) {
    event.preventDefault();
    const body = commentDraft.trim();
    if (!selected || !body || isSendingComment) return;
    if (persistence !== 'connected' || typeof selected.id !== 'string') {
      setComments((current) => [{ id: `local-comment-${Date.now()}`, body, author_id: account?.user.id, author_name: 'Вы', parent_comment_id: typeof replyToCommentId === 'string' ? replyToCommentId : null, created_at: new Date().toISOString(), reactions: [] }, ...current]);
      setCommentDraft('');
      setReplyToCommentId(null);
      return;
    }
    setSendingComment(true);
    void fetch(`${API_URL}/v1/cards/${selected.id}/comments`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ body, parent_comment_id: typeof replyToCommentId === 'string' ? replyToCommentId : null }) })
      .then(async (response) => { if (!response.ok) throw new Error('comment save failed'); return response.json() as Promise<Comment>; })
      .then((comment) => { setComments((current) => [comment, ...current]); setCommentDraft(''); setReplyToCommentId(null); })
      .catch(() => showToast('Не удалось отправить комментарий'))
      .finally(() => setSendingComment(false));
  }
  function beginCommentEdit(comment: Comment) {
    setEditingCommentId(comment.id);
    setCommentEditDraft(comment.body);
  }
  function saveCommentEdit(comment: Comment) {
    const body = commentEditDraft.trim();
    if (!body || body === comment.body) { setEditingCommentId(null); return; }
    setComments((current) => current.map((item) => item.id === comment.id ? { ...item, body } : item));
    setEditingCommentId(null);
    if (persistence !== 'connected' || typeof comment.id !== 'string') return;
    void fetch(`${API_URL}/v1/comments/${comment.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ body }) })
      .then((response) => { if (!response.ok) throw new Error('comment edit failed'); })
      .catch(() => showToast('Не удалось изменить комментарий'));
  }
  function removeComment(comment: Comment) {
    setComments((current) => current.filter((item) => item.id !== comment.id));
    if (persistence !== 'connected' || typeof comment.id !== 'string') return;
    void fetch(`${API_URL}/v1/comments/${comment.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('comment delete failed'); })
      .catch(() => showToast('Не удалось удалить комментарий'));
  }
  const supportedMediaTypes = new Set(['image/jpeg', 'image/png', 'image/gif', 'image/webp', 'video/mp4', 'video/webm', 'video/quicktime']);
  function isSupportedMedia(file: File) { return supportedMediaTypes.has(file.type); }
  function markdownForAttachment(attachment: Attachment) {
    const name = attachment.original_name.replace(/[\[\]\r\n]/g, '').trim() || 'Вложение';
    const url = assetUrl(attachment.url);
    return attachment.media_type.startsWith('video/') ? `![video:${name}](${url})` : `![${name}](${url})`;
  }
  function appendEmbeddedMedia(target: 'description' | 'comment', uploaded: Attachment[]) {
    if (!uploaded.length) return;
    const snippets = uploaded.map(markdownForAttachment).join('\n');
    const append = (value: string) => value ? `${value}${value.endsWith('\n') ? '' : '\n'}${snippets}` : snippets;
    if (target === 'description') setCardDescriptionDraft((current) => append(current));
    else setCommentDraft((current) => append(current));
  }
  function incrementAttachmentCount() {
    if (!selectedCardId) return;
    const patch = (card: Card) => ({ ...card, attachments: (card.attachments ?? 0) + 1 });
    setSelected((current) => current ? patch(current) : current);
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => card.id === selectedCardId ? patch(card) : card) })));
  }
  async function uploadMediaFiles(files: File[], target?: 'description' | 'comment') {
    if (!selected || !files.length || isUploadingAttachment) return;
    if (persistence !== 'connected' || typeof selected.id !== 'string') { showToast('Для вложений нужно подключение к серверу'); return; }
    const unsupported = files.filter((file) => !isSupportedMedia(file));
    if (unsupported.length) showToast('Можно добавить только JPEG, PNG, GIF, WebP, MP4, WebM или MOV');
    const accepted = files.filter(isSupportedMedia);
    if (!accepted.length) return;
    setUploadingAttachment(true);
    const uploaded: Attachment[] = [];
    for (const file of accepted) {
      if (file.size > 50 * 1024 * 1024) { showToast(`«${file.name}» больше 50 МиБ`); continue; }
      const form = new FormData();
      form.append('file', file);
      try {
        const response = await fetch(`${API_URL}/v1/cards/${selected.id}/attachments`, { method: 'POST', body: form });
        if (!response.ok) throw new Error('upload failed');
        const attachment = await response.json() as Attachment;
        uploaded.push(attachment);
        setAttachments((current) => [attachment, ...current]);
        incrementAttachmentCount();
      } catch { showToast(`Не удалось загрузить «${file.name}»`); }
    }
    if (target) appendEmbeddedMedia(target, uploaded);
    setUploadingAttachment(false);
  }
  async function uploadAttachments(event: ChangeEvent<HTMLInputElement>) {
    const files = Array.from(event.target.files ?? []);
    event.target.value = '';
    await uploadMediaFiles(files);
  }
  function handleMediaDrop(event: ReactDragEvent<HTMLTextAreaElement>, target: 'description' | 'comment') {
    const files = Array.from(event.dataTransfer.files);
    if (!files.length) return;
    event.preventDefault();
    void uploadMediaFiles(files, target);
  }
  function handleMediaPaste(event: ReactClipboardEvent<HTMLTextAreaElement>, target: 'description' | 'comment') {
    const files = Array.from(event.clipboardData.files);
    if (!files.length) return;
    event.preventDefault();
    void uploadMediaFiles(files, target);
  }
  function deleteAttachment(attachment: Attachment) {
    const wasCover = selected?.coverAttachmentId === attachment.id;
    setAttachments((current) => current.filter((item) => item.id !== attachment.id));
    if (wasCover) updateSelectedCard({ coverAttachmentId: undefined, coverUrl: undefined });
    void fetch(`${API_URL}/v1/attachments/${attachment.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('delete failed'); })
      .catch(() => showToast('Не удалось удалить вложение'));
  }
  function updateCardCover(attachment: Attachment | null, mode = coverModeDraft) {
    if (!selected || (attachment && !attachment.media_type.startsWith('image/'))) return;
    const patch = attachment ? { coverAttachmentId: attachment.id, coverUrl: attachment.url, coverMode: mode } : { coverAttachmentId: undefined, coverUrl: undefined, coverMode: 'full' as const };
    updateSelectedCard(patch);
    setCoverModeDraft(mode);
    if (persistence !== 'connected' || typeof selected.id !== 'string') return;
    void fetch(`${API_URL}/v1/cards/${selected.id}/cover`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ attachment_id: attachment?.id ?? null, mode }) })
      .then((response) => { if (!response.ok) throw new Error('cover save failed'); showToast(attachment ? 'Обложка установлена' : 'Обложка снята'); })
      .catch(() => showToast('Не удалось сохранить обложку'));
  }
  function clearCardBackground() {
    if (!selected) return;
    updateSelectedCard({ backgroundImageUrl: undefined });
    setSidebarPanel(null);
    if (persistence !== 'connected' || typeof selected.id !== 'string') return;
    void fetch(`${API_URL}/v1/cards/${selected.id}/background`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ background_image_url: null }) })
      .then((response) => { if (!response.ok) throw new Error('background clear failed'); showToast('Фон карточки снят'); })
      .catch(() => showToast('Не удалось снять фон карточки'));
  }
  function updateChecklistItem(checklistId: string, itemId: EntityId, patch: Partial<ChecklistItem>) {
    applyChecklists(checklists.map((checklist) => checklist.id === checklistId ? { ...checklist, items: checklist.items.map((item) => item.id === itemId ? { ...item, ...patch } : item) } : checklist));
  }
  function saveChecklistItemDescription(checklistId: string, item: ChecklistItem) {
    const description = checklistItemDescriptionDrafts[String(item.id)] ?? item.description;
    if (description === item.description) return;
    updateChecklistItem(checklistId, item.id, { description });
    if (persistence !== 'connected' || typeof item.id !== 'string') return;
    void fetch(`${API_URL}/v1/checklist-items/${item.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ description }) })
      .then(async (response) => { if (!response.ok) throw new Error('description save failed'); return response.json() as Promise<ChecklistItem>; })
      .then((saved) => updateChecklistItem(checklistId, item.id, saved))
      .catch(() => showToast('Описание пункта не сохранено'));
  }
  async function uploadChecklistItemAttachments(checklistId: string, item: ChecklistItem, files: File[]) {
    if (!files.length || isUploadingChecklistItemAttachment) return;
    if (persistence !== 'connected' || typeof item.id !== 'string') { showToast('Для вложений нужно подключение к серверу'); return; }
    const accepted = files.filter(isSupportedMedia);
    if (accepted.length !== files.length) showToast('Можно добавить только JPEG, PNG, GIF, WebP, MP4, WebM или MOV');
    if (!accepted.length) return;
    setUploadingChecklistItemAttachment(true);
    try {
      const uploaded = await Promise.all(accepted.map(async (file) => {
        const form = new FormData();
        form.append('file', file);
        const response = await fetch(`${API_URL}/v1/checklist-items/${item.id}/attachments`, { method: 'POST', body: form });
        if (!response.ok) throw new Error('upload failed');
        return response.json() as Promise<Attachment>;
      }));
      updateChecklistItem(checklistId, item.id, { attachments: [...item.attachments, ...uploaded] });
    } catch { showToast('Не удалось загрузить вложение пункта'); }
    finally { setUploadingChecklistItemAttachment(false); }
  }
  function deleteChecklistItemAttachment(checklistId: string, item: ChecklistItem, attachment: Attachment) {
    updateChecklistItem(checklistId, item.id, { attachments: item.attachments.filter((value) => value.id !== attachment.id) });
    void fetch(`${API_URL}/v1/attachments/${attachment.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('attachment delete failed'); })
      .catch(() => showToast('Вложение пункта не удалено'));
  }
  function uploadCardBackground(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file || !selected || isUploadingCardBackground) return;
    if (persistence !== 'connected' || typeof selected.id !== 'string') { showToast('Для загрузки фона нужно подключение к серверу'); return; }
    if (!['image/jpeg', 'image/png', 'image/gif', 'image/webp'].includes(file.type) || file.size > 50 * 1024 * 1024) {
      showToast('Выберите JPEG, PNG, GIF или WebP до 50 МиБ'); return;
    }
    const form = new FormData();
    form.append('file', file);
    setUploadingCardBackground(true);
    void fetch(`${API_URL}/v1/cards/${selected.id}/background/file`, { method: 'POST', body: form })
      .then(async (response) => { if (!response.ok) throw new Error('upload failed'); return response.json() as Promise<{ url: string }>; })
      .then(({ url }) => { updateSelectedCard({ backgroundImageUrl: url }); showToast('Фон карточки загружен'); })
      .catch(() => showToast('Не удалось загрузить фон карточки'))
      .finally(() => setUploadingCardBackground(false));
  }
  function toggleCardCompletion(card: Card, event?: ReactMouseEvent<HTMLButtonElement>) {
    event?.stopPropagation();
    const completedAt = card.completedAt ? undefined : new Date().toISOString();
    updateSelectedCard(card.id === selected?.id ? { completedAt } : {});
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((item) => item.id === card.id ? { ...item, completedAt } : item) })));
    if (persistence !== 'connected' || typeof card.id !== 'string') return;
    void fetch(`${API_URL}/v1/cards/${card.id}/completion`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ is_completed: !card.completedAt }) })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось сохранить статус задачи'); })
      .catch((error) => { updateSelectedCard(card.id === selected?.id ? { completedAt: card.completedAt } : {}); setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((item) => item.id === card.id ? { ...item, completedAt: card.completedAt } : item) }))); showToast(error instanceof Error ? error.message : 'Не удалось сохранить статус задачи'); });
  }
  function setSelectedCardPublicVisibility(isPublic: boolean) {
    if (!selected) return;
    const previous = selected.isPublic ?? true;
    updateSelectedCard({ isPublic });
    if (persistence !== 'connected' || typeof selected.id !== 'string') return;
    void fetch(`${API_URL}/v1/cards/${selected.id}/public-visibility`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ is_public: isPublic }) })
      .then((response) => { if (!response.ok) throw new Error('visibility update failed'); showToast(isPublic ? 'Карточка видна гостям' : 'Карточка скрыта от гостей'); })
      .catch(() => { updateSelectedCard({ isPublic: previous }); showToast('Не удалось изменить видимость карточки'); });
  }
  function setSelectedCardPriority(priority: number) {
    if (!selected || priority < 0 || priority > 5) return;
    const previous = selected.priority ?? 0;
    if (priority === previous) return;
    updateSelectedCard({ priority });
    if (persistence !== 'connected' || typeof selected.id !== 'string') return;
    void fetch(`${API_URL}/v1/cards/${selected.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ priority }) })
      .then((response) => { if (!response.ok) throw new Error('priority update failed'); showToast(priority ? `Приоритет: ${priority}/5` : 'Приоритет снят'); })
      .catch(() => { updateSelectedCard({ priority: previous }); showToast('Не удалось сохранить приоритет'); });
  }
  function beginColumnRename(column: Column) { setColumnMenuId(null); setEditingColumnId(column.id); setColumnTitleDraft(column.title); }
  function saveColumnTitle(columnId: EntityId) {
    const title = columnTitleDraft.trim();
    if (!title || isSavingColumn) return;
    const apply = () => { setColumns((current) => current.map((column) => column.id === columnId ? { ...column, title } : column)); setEditingColumnId(null); showToast('Колонка переименована'); };
    if (persistence !== 'connected' || typeof columnId !== 'string') { apply(); return; }
    setSavingColumn(true);
    void fetch(`${API_URL}/v1/lists/${columnId}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title }) })
      .then((response) => { if (!response.ok) throw new Error('rename failed'); apply(); })
      .catch(() => showToast('Не удалось переименовать колонку'))
      .finally(() => setSavingColumn(false));
  }
  function deleteColumn(column: Column) {
    if (column.cards.length > 0) { showToast(`Сначала перенесите или архивируйте ${column.cards.length} задач`); setColumnMenuId(null); return; }
    if (persistence !== 'connected' || typeof column.id !== 'string') { setColumns((current) => current.filter((item) => item.id !== column.id)); setColumnMenuId(null); return; }
    void fetch(`${API_URL}/v1/lists/${column.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('delete failed'); setColumns((current) => current.filter((item) => item.id !== column.id)); setColumnMenuId(null); showToast('Пустая колонка удалена'); })
      .catch(() => showToast('Колонку нельзя удалить: сначала освободите её'));
  }

  function applyBoard(data: ApiBoard) {
    setColumns(data.lists.map((list) => ({ id: list.id, title: list.title, gridColumn: list.grid_column, gridRow: list.grid_row, cards: list.cards.map((card) => ({ id: card.id, title: card.title, description: card.description, priority: card.priority, lastActivityAt: card.last_activity_at ?? undefined, isPublic: card.is_public, hasUnreadMentions: card.has_unread_mentions, backgroundImageUrl: card.background_image_url ?? undefined, dueAt: card.due_at ?? undefined, coverAttachmentId: card.cover_attachment_id ?? undefined, coverUrl: card.cover_url ?? undefined, coverMode: card.cover_mode, completedAt: card.completed_at ?? undefined, checklist: card.checklist_total ? `${card.checklist_completed}/${card.checklist_total}` : undefined, comments: card.comment_count || undefined, attachments: card.attachment_count || undefined, labels: card.labels, members: card.assignees.map(memberFromApi) })) })));
    setBoardLabels(data.labels);
    setWorkspaceMembers(data.members.map(memberFromApi));
    setWorkspaceId(data.workspace_id);
    setBoardTitle(data.title);
    setBoardBackgroundUrl(data.background_image_url);
    boardBackgroundDisplayRef.current = { fit: data.background_fit ?? 'cover', position: data.background_position ?? 'center' };
    setBoardBackgroundFit(data.background_fit ?? 'cover');
    setBoardBackgroundPosition(data.background_position ?? 'center');
    setBoardVisibility(data.visibility === 'public' ? 'public' : 'private');
    setCanEditBoard(data.can_edit);
    // Uploaded board backgrounds are returned with a revision query parameter.
    // Keep the settings input on the canonical path so saving it later cannot
    // persist an obsolete cache-buster into the database.
    setBackgroundDraft((data.background_image_url ?? '').replace(/^(\/v1\/boards\/[^/]+\/background\/file)\?v=[^&]+$/, '$1'));
    setBoardId(data.id);
  }

  async function selectBoard(nextBoardId: string) {
    try {
      const response = await fetch(`${API_URL}/v1/boards/${nextBoardId}`);
      if (!response.ok) throw new Error('board load failed');
      applyBoard(await response.json() as ApiBoard);
      updateBoardRoute(nextBoardId);
      setView('board');
    } catch { showToast('Не удалось открыть проект'); }
  }

  async function selectWorkspace(nextWorkspace: Workspace) {
    try {
      setWorkspaceId(nextWorkspace.id); setWorkspaceName(nextWorkspace.name); setView('home');
      if (typeof window !== 'undefined') window.localStorage.setItem('flowboard.workspace_id', nextWorkspace.id);
      const response = await fetch(`${API_URL}/v1/workspaces/${nextWorkspace.id}/boards`);
      if (!response.ok) throw new Error('boards load failed');
      const nextBoards = await response.json() as BoardSummary[];
      setBoards(nextBoards);
      setBoardId(null); setBoardTitle(''); setColumns([]);
    } catch { showToast('Не удалось переключить пространство'); }
  }

  function createBoard(event: FormEvent) {
    event.preventDefault();
    const title = newBoardTitle.trim();
    if (!title || !workspaceId || isCreatingBoard) return;
    if (persistence !== 'connected') { showToast('Нужна связь с сервером, чтобы создать проект'); return; }
    setCreatingBoard(true);
    void fetch(`${API_URL}/v1/workspaces/${workspaceId}/boards`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title }) })
      .then(async (response) => { if (!response.ok) throw new Error('board create failed'); return response.json() as Promise<BoardSummary>; })
      .then((board) => { setBoards((current) => [board, ...current]); setNewBoardTitle(''); setNewBoardComposer(false); void selectBoard(board.id); })
      .catch(() => showToast('Не удалось создать проект'))
      .finally(() => setCreatingBoard(false));
  }

  function beginBoardRename() {
    setBoardTitleDraft(boardTitle);
    setEditingBoardTitle(true);
  }

  function saveBoardTitle() {
    const title = boardTitleDraft.trim();
    if (!title || title === boardTitle || isSavingBoardTitle) { setEditingBoardTitle(false); return; }
    setBoardTitle(title);
    setEditingBoardTitle(false);
    if (persistence !== 'connected' || !boardId) return;
    setSavingBoardTitle(true);
    void fetch(`${API_URL}/v1/boards/${boardId}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title }) })
      .then((response) => { if (!response.ok) throw new Error('board title save failed'); showToast('Название проекта обновлено'); })
      .catch(() => { setBoardTitle(boardTitle); showToast('Не удалось переименовать проект'); })
      .finally(() => setSavingBoardTitle(false));
  }
  function deleteCurrentBoard() {
    if (!boardId || !window.confirm(`Удалить проект «${boardTitle}»? Все его колонки, задачи, вложения и схемы будут удалены безвозвратно.`)) return;
    const deletedBoardId = boardId;
    void fetch(`${API_URL}/v1/boards/${deletedBoardId}`, { method: 'DELETE' })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'delete failed'); setBoards((current) => current.filter((item) => item.id !== deletedBoardId)); setBoardId(null); setColumns([]); setBoardTitle(''); openHome(); showToast('Проект удалён'); })
      .catch((error) => showToast(error instanceof Error ? error.message : 'Не удалось удалить проект'));
  }
  function deleteOwnedWorkspace(workspace: Workspace) {
    if (!window.confirm(`Удалить пространство «${workspace.name}»? Все проекты, задачи, вложения и схемы будут удалены безвозвратно.`)) return;
    void fetch(`${API_URL}/v1/workspaces/${workspace.id}`, { method: 'DELETE' })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'delete failed'); const remaining = workspaces.filter((item) => item.id !== workspace.id); setWorkspaces(remaining); if (workspaceId === workspace.id) { const next = remaining[0]; setWorkspaceId(next?.id ?? null); setWorkspaceName(next?.name ?? ''); setBoards([]); setBoardId(null); setColumns([]); } showToast('Пространство удалено'); })
      .catch((error) => showToast(error instanceof Error ? error.message : 'Не удалось удалить пространство'));
  }

  function saveBoardBackground(event: FormEvent) {
    event.preventDefault();
    if (!boardId || isSavingBackground) return;
    const background_image_url = backgroundDraft.trim() || null;
    setSavingBackground(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/background`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ background_image_url, background_fit: boardBackgroundFit, background_position: boardBackgroundPosition }) })
      .then((response) => { if (!response.ok) throw new Error('background save failed'); setBoardBackgroundUrl(background_image_url); showToast(background_image_url ? 'Фон проекта установлен' : 'Фон проекта снят'); })
      .catch(() => showToast('Не удалось сохранить фон: нужен HTTPS-адрес изображения'))
      .finally(() => setSavingBackground(false));
  }
  function clearBoardBackground() {
    if (!boardId || isSavingBackground) return;
    setSavingBackground(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/background`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ background_image_url: null, background_fit: boardBackgroundFit, background_position: boardBackgroundPosition }) })
      .then((response) => { if (!response.ok) throw new Error('background clear failed'); setBoardBackgroundUrl(null); setBackgroundDraft(''); showToast('Фон проекта снят'); })
      .catch(() => showToast('Не удалось снять фон проекта'))
      .finally(() => setSavingBackground(false));
  }
  function uploadBoardBackground(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file || !boardId || isUploadingBoardBackground) return;
    if (!['image/jpeg', 'image/png', 'image/gif', 'image/webp'].includes(file.type) || file.size > 50 * 1024 * 1024) {
      showToast('Выберите JPEG, PNG, GIF или WebP до 50 МиБ'); return;
    }
    const form = new FormData(); form.append('file', file);
    setUploadingBoardBackground(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/background/file`, { method: 'POST', body: form })
      .then(async (response) => { if (!response.ok) throw new Error('upload failed'); return response.json() as Promise<{ url: string }>; })
      .then(({ url }) => { setBoardBackgroundUrl(url); setBackgroundDraft(`/v1/boards/${boardId}/background/file`); showToast('Фон проекта загружен'); })
      .catch(() => showToast('Не удалось загрузить фон проекта'))
      .finally(() => setUploadingBoardBackground(false));
  }
  function openDiscordIntegration() {
    if (!boardId) return;
    setDiscordIntegrationOpen(true);
    setCreatedDiscordToken('');
    setDiscordTargetListId((current) => current || String(columns[0]?.id ?? ''));
    setDiscordIntegrationLoading(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/integrations/discord`)
      .then(async (response) => { if (!response.ok) throw new Error('discord integration load failed'); return response.json() as Promise<DiscordIntegration[]>; })
      .then(setDiscordIntegrations)
      .catch(() => { setDiscordIntegrationOpen(false); showToast('Управлять Discord API может только владелец проекта'); })
      .finally(() => setDiscordIntegrationLoading(false));
  }
  function createDiscordIntegration(event: FormEvent) {
    event.preventDefault();
    if (!boardId || !discordIntegrationName.trim() || isCreatingDiscordIntegration) return;
    setCreatingDiscordIntegration(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/integrations/discord`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name: discordIntegrationName.trim(), default_list_id: discordTargetListId || null }) })
      .then(async (response) => { if (!response.ok) throw new Error('discord integration create failed'); return response.json() as Promise<DiscordIntegration>; })
      .then((integration) => { setDiscordIntegrations((current) => [integration, ...current]); setCreatedDiscordToken(integration.token ?? ''); if (integration.token) void navigator.clipboard?.writeText(integration.token); showToast('Токен создан и скопирован — сохраните его сейчас'); })
      .catch(() => showToast('Не удалось создать Discord API-токен'))
      .finally(() => setCreatingDiscordIntegration(false));
  }
  function revokeDiscordIntegration(integration: DiscordIntegration) {
    if (!boardId) return;
    void fetch(`${API_URL}/v1/boards/${boardId}/integrations/discord/${integration.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('discord integration revoke failed'); setDiscordIntegrations((current) => current.filter((item) => item.id !== integration.id)); showToast('Discord API-токен отозван'); })
      .catch(() => showToast('Не удалось отозвать Discord API-токен'));
  }
  function changeBoardVisibility(visibility: 'public' | 'private') {
    if (!boardId || visibility === boardVisibility) return;
    const previous = boardVisibility; setBoardVisibility(visibility);
    void fetch(`${API_URL}/v1/boards/${boardId}/visibility`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ visibility }) })
      .then((response) => { if (!response.ok) throw new Error('visibility save failed'); showToast(visibility === 'public' ? 'Доска открыта всем только для просмотра' : 'Доска стала приватной'); })
      .catch(() => { setBoardVisibility(previous); showToast('Не удалось изменить доступ к доске'); });
  }
  async function copyPublicBoardLink() {
    if (!boardId) return;
    try { await navigator.clipboard.writeText(`${window.location.origin}${window.location.pathname}?board=${boardId}`); showToast('Ссылка на публичную доску скопирована'); }
    catch { showToast('Не удалось скопировать ссылку'); }
  }
  async function exportCurrentBoard() {
    if (!boardId) return;
    try {
      const response = await fetch(`${API_URL}/v1/boards/${boardId}/export`);
      if (!response.ok) throw new Error('export failed');
      const data = await response.json();
      const href = URL.createObjectURL(new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' }));
      const anchor = document.createElement('a'); anchor.href = href; anchor.download = `${boardTitle || 'flowboard-project'}.json`; anchor.click(); URL.revokeObjectURL(href);
      showToast('Экспорт проекта скачан');
    } catch { showToast('Не удалось экспортировать проект'); }
  }
  async function importBoardFile(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]; event.target.value = '';
    if (!file || !workspaceId) return;
    if (file.size > 20 * 1024 * 1024) { showToast('JSON больше 20 МиБ'); return; }
    try {
      const document = JSON.parse(await file.text());
      const response = await fetch(`${API_URL}/v1/workspaces/${workspaceId}/boards/import`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(document) });
      if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'import failed');
      const imported = await response.json() as { id: string; title: string; imported_cards: number; imported_comments: number };
      const boardsResponse = await fetch(`${API_URL}/v1/workspaces/${workspaceId}/boards`);
      if (boardsResponse.ok) setBoards(await boardsResponse.json() as BoardSummary[]);
      setBoardMenuOpen(false); await selectBoard(imported.id); showToast(`Импортировано: ${imported.imported_cards} задач и ${imported.imported_comments} сообщений`);
    } catch (error) { showToast(error instanceof Error ? error.message : 'Не удалось импортировать JSON'); }
  }

  async function submitAuth(event: FormEvent) {
    event.preventDefault();
    setAuthError('');
    setAuthorizing(true);
    try {
      const nickname = authName.trim().toLowerCase();
      const isInviteAcceptance = Boolean(inviteToken);
      const isRegistering = isInviteAcceptance || authMode === 'register';
      const response = await fetch(`${API_URL}/v1/auth/${isInviteAcceptance ? 'accept-invitation' : isRegistering ? 'register' : 'login'}`, {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(isInviteAcceptance ? { token: inviteToken, username: nickname, password: authPassword } : isRegistering ? { username: nickname, password: authPassword } : { username: nickname, password: authPassword }),
      });
      if (!response.ok) {
        const error = await response.json().catch(() => null) as { message?: string } | null;
        throw new Error(error?.message ?? 'Не удалось выполнить вход.');
      }
      setAccount(await response.json() as AuthAccount);
      setAuthState('signed-in');
      if (inviteToken) window.history.replaceState({}, '', window.location.pathname);
      window.location.reload();
    } catch (error) {
      setAuthError(error instanceof Error ? error.message : 'Не удалось выполнить вход.');
    } finally {
      setAuthorizing(false);
    }
  }

  function signOut() {
    void fetch(`${API_URL}/v1/auth/logout`, { method: 'POST' })
      .finally(() => { setAccount(null); setAuthState('signed-out'); setColumns([]); });
  }
  function saveProfileName(event: FormEvent) {
    event.preventDefault(); if (!profileName.trim() || isSavingProfile) return;
    setSavingProfile(true); setProfileError('');
    void fetch(`${API_URL}/v1/auth/me`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ username: profileName }) })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось сохранить имя'); return response.json() as Promise<AuthAccount>; })
      .then((updated) => { setAccount(updated); setProfilePanel('overview'); showToast('Имя сохранено'); })
      .catch((error) => setProfileError(error instanceof Error ? error.message : 'Не удалось сохранить имя')).finally(() => setSavingProfile(false));
  }
  function changeProfilePassword(event: FormEvent) {
    event.preventDefault(); if (!currentPassword || !nextPassword || isSavingProfile) return;
    setSavingProfile(true); setProfileError('');
    void fetch(`${API_URL}/v1/auth/password`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ current_password: currentPassword, new_password: nextPassword }) })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось сменить пароль'); setCurrentPassword(''); setNextPassword(''); setProfilePanel('overview'); showToast('Пароль обновлён'); })
      .catch((error) => setProfileError(error instanceof Error ? error.message : 'Не удалось сменить пароль')).finally(() => setSavingProfile(false));
  }
  function uploadProfileAvatar(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0]; event.target.value = ''; if (!file || isSavingProfile) return;
    setSavingProfile(true); setProfileError(''); const form = new FormData(); form.append('file', file);
    void fetch(`${API_URL}/v1/auth/avatar`, { method: 'POST', body: form })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось загрузить аватар'); return response.json() as Promise<AuthAccount>; })
      .then((updated) => { setAccount(updated); setAvatarVersion((version) => version + 1); showToast('Аватар обновлён'); })
      .catch((error) => setProfileError(error instanceof Error ? error.message : 'Не удалось загрузить аватар')).finally(() => setSavingProfile(false));
  }

  if (authState === 'checking') {
    return <main className={`app-shell ${theme}`}><div className="board-loading auth-loading" role="status"><span className="loading-dot" />Проверяем доступ к пространству</div></main>;
  }

  if (authState === 'signed-out') {
    const canRegister = registrationOpen || Boolean(inviteToken);
    const isRegistering = Boolean(inviteToken) || (authMode === 'register' && canRegister);
    return <main className={`app-shell ${theme} auth-shell`}><section className="auth-card"><button className="brand auth-brand" type="button" onClick={() => setAuthMode('login')}><span className="brand-mark">✓</span><span>Flowboard</span></button><p className="eyebrow">FLOWBOARD</p><h1>{isRegistering ? inviteToken ? 'Активировать аккаунт' : 'Создать первый аккаунт' : 'С возвращением'}</h1><p className="auth-copy">{isRegistering ? inviteToken ? 'Выберите уникальный ник и пароль.' : 'Первый аккаунт станет system owner.' : 'Войдите по нику, чтобы продолжить.'}</p><form className="auth-form" onSubmit={submitAuth}><label>Ник<input value={authName} onChange={(event) => setAuthName(event.target.value)} maxLength={32} required autoComplete="username" placeholder="your_nick" /></label><label>Пароль<input type="password" value={authPassword} onChange={(event) => setAuthPassword(event.target.value)} minLength={10} maxLength={256} required autoComplete={isRegistering ? 'new-password' : 'current-password'} /></label>{authError && <p className="auth-error">{authError}</p>}<button className="create-button auth-submit" type="submit" disabled={isAuthorizing}>{isAuthorizing ? 'Подключаем…' : isRegistering ? inviteToken ? 'Активировать' : 'Создать аккаунт' : 'Войти'}</button></form></section></main>;
  }

  return <main className={`app-shell ${theme} ${view === 'home' ? 'home-mode' : ''} ${isPublicViewer ? 'public-viewer' : ''} ${boardBackgroundUrl && view === 'board' ? 'has-board-background' : ''} ${!boardBackgroundUrl && view === 'board' ? 'default-board-background' : ''}`} style={boardBackgroundStyle}>
    <header className="topbar">
      <button className="brand" type="button" onClick={openHome} aria-label="Flowboard: перейти на главную"><span className="brand-mark">✓</span><span>Flowboard</span></button>
      <label className="search"><span>⌕</span><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Поиск по задачам" aria-label="Поиск по задачам" /></label>
      <div className="top-actions">{account && <button className="top-utility-button" type="button" onClick={openSessions} aria-label="Открыть сессии">◷ <span>Сессии</span></button>}{account?.user.is_system_owner && <button className="top-utility-button" type="button" onClick={openAdmin} aria-label="Открыть администрирование">⚙ <span>Админ</span></button>}<button className="theme-button" onClick={() => setTheme((current) => current === 'dark' ? 'light' : 'dark')} aria-label="Переключить тему">{theme === 'dark' ? '☾' : '☀'} <span>{theme === 'dark' ? 'Ночь' : 'День'}</span></button>{!isPublicViewer && <button className="create-button" onClick={() => { openBoard(); if (persistence !== 'connecting') { const firstColumn = columns[0]; if (firstColumn) setComposerOpen(firstColumn.id); else addColumn(); } }}>＋ Создать</button>}{account && <button className="profile-trigger" onClick={() => { setProfileOpen(true); setProfilePanel('overview'); setProfileName(account.user.username); setProfileError(''); }} aria-label="Открыть профиль"><ProfileAvatar account={account} member={currentMember} version={avatarVersion} /></button>}</div>
    </header>

    {(view === 'home' || (view === 'board' && !boardBackgroundUrl)) && <div className="default-board-ambient" aria-hidden="true">{Array.from({ length: 84 }, (_, index) => <i key={index} />)}</div>}

    {isProfileOpen && account && <div className="modal-backdrop" role="presentation" onMouseDown={() => setProfileOpen(false)}><section className="archive-modal profile-modal" role="dialog" aria-modal="true" aria-label="Профиль" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setProfileOpen(false)} aria-label="Закрыть профиль">×</button>{profilePanel === 'overview' ? <><header className="profile-modal-head"><ProfileAvatar account={account} member={currentMember} version={avatarVersion} /><div><p className="eyebrow">ПРОФИЛЬ</p><h2>@{account.user.username}</h2></div></header><div className="profile-action-list"><button onClick={() => { setProfileName(account.user.username); setProfilePanel('username'); }}>Изменить ник <span>›</span></button><button onClick={() => setProfilePanel('password')}>Сменить пароль <span>›</span></button><label>Изменить аватар<input type="file" accept="image/jpeg,image/png,image/gif,image/webp" onChange={uploadProfileAvatar} disabled={isSavingProfile} /></label></div><button className="profile-signout" onClick={signOut}>Выйти из аккаунта</button></> : profilePanel === 'username' ? <><button className="text-action" onClick={() => setProfilePanel('overview')}>← Профиль</button><h2>Изменить ник</h2><form className="profile-form" onSubmit={saveProfileName}><label>Новый ник<input autoFocus value={profileName} onChange={(event) => setProfileName(event.target.value)} maxLength={32} /></label><div><button type="button" className="secondary-button" onClick={() => setProfilePanel('overview')}>Отмена</button><button className="create-button" type="submit" disabled={isSavingProfile}>Сохранить</button></div></form></> : <><button className="text-action" onClick={() => setProfilePanel('overview')}>← Профиль</button><h2>Сменить пароль</h2><form className="profile-form" onSubmit={changeProfilePassword}><label>Текущий пароль<input autoFocus type="password" value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} /></label><label>Новый пароль<input type="password" value={nextPassword} onChange={(event) => setNextPassword(event.target.value)} minLength={10} /></label><div><button type="button" className="secondary-button" onClick={() => setProfilePanel('overview')}>Отмена</button><button className="create-button" type="submit" disabled={isSavingProfile}>Сохранить</button></div></form></>}{profileError && <p className="profile-error">{profileError}</p>}</section></div>}

    {isWorkspaceComposerOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => !isCreatingWorkspace && setWorkspaceComposerOpen(false)}><section className="archive-modal workspace-create-modal" role="dialog" aria-modal="true" aria-label="Создать пространство" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" type="button" onClick={() => setWorkspaceComposerOpen(false)} disabled={isCreatingWorkspace} aria-label="Закрыть">×</button><p className="eyebrow">НОВОЕ ПРОСТРАНСТВО</p><h2>Создать пространство</h2><p className="archive-copy">Вы станете его owner и сможете добавить команду в настройках пространства.</p><form className="workspace-create-form" onSubmit={createWorkspace}><label htmlFor="workspace-name">Название</label><input id="workspace-name" autoFocus value={newWorkspaceName} onChange={(event) => { setNewWorkspaceName(event.target.value); setWorkspaceCreateError(''); }} maxLength={120} placeholder="Например, Маркетинг" disabled={isCreatingWorkspace} />{workspaceCreateError && <p className="form-error" role="alert">{workspaceCreateError}</p>}<div><button className="secondary-button" type="button" onClick={() => setWorkspaceComposerOpen(false)} disabled={isCreatingWorkspace}>Отмена</button><button className="create-button" type="submit" disabled={!newWorkspaceName.trim() || isCreatingWorkspace}>{isCreatingWorkspace ? 'Создаём…' : 'Создать пространство'}</button></div></form></section></div>}
    {isAdminOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setAdminOpen(false)}><section className="archive-modal team-modal admin-modal" role="dialog" aria-modal="true" aria-label="Администрирование" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setAdminOpen(false)} aria-label="Закрыть">×</button><p className="eyebrow">SYSTEM OWNER</p><h2>Администрирование</h2><button className="create-button admin-invite-button" type="button" onClick={createAccountInvite}>Создать account-invite</button>{isAdminLoading ? <p className="detail-loading">Загружаем данные…</p> : <><section className="admin-section"><h3>Аккаунты</h3><div className="team-list">{adminAccounts.map((item) => <article key={item.id}><Avatar member={memberFromApi({ id: item.id, username: item.username, avatar_url: item.avatar_url })} /><div><b>@{item.username}</b><small>{item.is_system_owner ? 'System owner' : 'Активен'}</small></div>{!item.is_system_owner && <button className="danger-action" onClick={() => deleteAccount(item)}>Удалить</button>}</article>)}</div></section><section className="admin-section"><h3>Пространства</h3><div className="team-list">{adminWorkspaces.map((item) => <article key={item.id}><div><b>{item.name}</b><small>Owner: @{item.owner_username} · {item.member_count} уч.</small></div><span className="workspace-admin-actions"><button onClick={() => archiveWorkspace(item)}>{item.archived_at ? 'Восстановить' : 'Архивировать'}</button><button className="danger-action" onClick={() => deleteWorkspace(item)}>Удалить</button></span></article>)}</div></section><section className="admin-section"><h3>Активные invite</h3><div className="team-list">{adminInvites.length ? adminInvites.map((item) => <article key={item.id}><div><b>Invite</b><small>до {new Date(item.expires_at).toLocaleString('ru-RU')}</small></div></article>) : <p className="empty-comments">Нет активных invite.</p>}</div></section></>}</section></div>}
    {isSessionsOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setSessionsOpen(false)}><section className="archive-modal team-modal security-modal" role="dialog" aria-modal="true" aria-label="Сессии" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setSessionsOpen(false)} aria-label="Закрыть">×</button><p className="eyebrow">БЕЗОПАСНОСТЬ</p><h2>Активные сессии</h2><button className="secondary-button session-revoke-all" onClick={revokeOtherSessions}>Выйти на других устройствах</button><div className="team-list">{sessions.map((session) => <article key={session.id}><div><b>{session.current ? 'Это устройство' : 'Активная сессия'}</b><small>Последняя активность: {new Date(session.last_seen_at).toLocaleString('ru-RU')}</small></div>{!session.current && <button className="danger-action" onClick={() => revokeSession(session)}>Отозвать</button>}</article>)}</div></section></div>}

    {view === 'home' ? (!workspaceId ? <section className="home-screen empty-workspaces"><div className="welcome"><p className="eyebrow">ПРОСТРАНСТВ ПОКА НЕТ</p><h1>Начните с пространства.</h1><p>Аккаунт существует отдельно от рабочих пространств. Создайте своё первое пространство или присоединитесь к уже созданному.</p><button className="create-button" onClick={openWorkspaceComposer}>Создать пространство</button></div></section> : <section className="home-screen">
      <div className="welcome"><p className="eyebrow">МОЯ РАБОТА</p><h1>Добрый вечер, {currentMember.name.split(' ')[0]}.</h1><p>Всё важное для команды — спокойно, в одном ритме.</p><button className="create-button" onClick={openBoard}>Открыть последнюю доску <span>→</span></button></div>
      <section className="workspace-picker" aria-label="Выбор пространства"><div className="section-title"><div><p className="eyebrow">ПРОСТРАНСТВА</p><h2>Ваши пространства</h2></div><button className="subtle-button" onClick={openWorkspaceComposer}>＋ Создать пространство</button></div><div className="workspace-options">{workspaces.map((item) => <button className={item.id === workspaceId ? 'selected' : ''} key={item.id} onClick={() => { if (item.id !== workspaceId) void selectWorkspace(item); }}><span className="workspace-option-icon">⌁</span><span><b>{item.name}</b><small>{item.id === workspaceId ? 'Открыто сейчас' : 'Открыть пространство'}</small></span>{item.id === workspaceId && <i>✓</i>}</button>)}</div></section>
      <section className="workspace-section"><div className="section-title"><div><p className="eyebrow">ПРОСТРАНСТВО</p><h2>{workspaceName}</h2></div><span className="workspace-actions"><button className="subtle-button" onClick={() => setNewBoardComposer((current) => !current)}>＋ Новый проект</button><button className="subtle-button danger-text" onClick={() => { const workspace = workspaces.find((item) => item.id === workspaceId); if (workspace) deleteOwnedWorkspace(workspace); }}>Удалить пространство</button></span></div>{isNewBoardComposer && <form className="new-board-form" onSubmit={createBoard}><input autoFocus value={newBoardTitle} onChange={(event) => setNewBoardTitle(event.target.value)} maxLength={200} placeholder="Название проекта" /><button className="create-button" type="submit" disabled={!newBoardTitle.trim() || isCreatingBoard}>{isCreatingBoard ? 'Создаём…' : 'Создать'}</button></form>}{boards.length ? boards.map((board) => <button className="board-tile" key={board.id} onClick={() => void selectBoard(board.id)}><span className="board-tile-icon">⌁</span><span><b>{board.title}</b><small>{board.id === boardId ? `${columns.length} колонки · ${columns.reduce((sum, column) => sum + column.cards.length, 0)} задач` : 'Открыть проект'}</small></span><span className="board-tile-arrow">→</span></button>) : <div className="empty-board-state"><b>В этом пространстве пока нет досок.</b><span>Создайте первую доску, когда будете готовы начать работу.</span></div>}</section>
    </section>) : <>
      <section className="board-header">
        <div><button className="breadcrumbs" onClick={openHome}>{workspaceName} <span>/</span> {boardTitle}</button><div className="board-title-row">{isEditingBoardTitle ? <form className="board-title-form" onSubmit={(event) => { event.preventDefault(); saveBoardTitle(); }}><input autoFocus value={boardTitleDraft} onChange={(event) => setBoardTitleDraft(event.target.value)} maxLength={200} onKeyDown={(event) => { if (event.key === 'Escape') setEditingBoardTitle(false); }} /><button type="submit" disabled={isSavingBoardTitle}>✓</button></form> : <h1>{boardTitle}</h1>}<span className={`sync-status ${persistence}`}>{persistence === 'connected' ? 'Сохранено' : persistence === 'connecting' ? 'Подключение…' : 'Нет подключения'}</span><button className="title-edit" onClick={beginBoardRename} aria-label="Переименовать доску">✎</button></div></div>
        <div className="board-tools"><div className="avatars">{workspaceMembers.slice(0, 3).map((person) => <Avatar key={person.name} member={person} />)}{workspaceMembers.length > 3 && <span className="more-members">+{workspaceMembers.length - 3}</span>}</div><div className="filter-control"><button className={`secondary-button ${filterMode !== 'all' ? 'active-filter' : ''}`} onClick={() => setFilterOpen((current) => !current)}>⌘ Фильтры</button>{isFilterOpen && <div className="filter-popover"><p>Показывать</p>{([['all', 'Все задачи'], ['assigned', 'Назначенные мне'], ['due', 'С дедлайном'], ['overdue', 'Просроченные']] as [FilterMode, string][]).map(([mode, label]) => <button key={mode} className={filterMode === mode ? 'active' : ''} onClick={() => { setFilterMode(mode); setFilterOpen(false); }}>{label}{filterMode === mode && <b>✓</b>}</button>)}</div>}</div><button className="secondary-button" onClick={openArchive}>Архив</button><button className="share-button" onClick={openTeam}>Команда</button><div className="board-menu-control"><button className="secondary-button more" onClick={() => setBoardMenuOpen((current) => !current)} aria-expanded={isBoardMenuOpen}>•••</button>{isBoardMenuOpen && <div className="board-menu">{!isPublicViewer && <button onClick={() => { setBoardMenuOpen(false); openDiscordIntegration(); }}>⌁ Discord API</button>}<button onClick={exportCurrentBoard}>⇩ Экспорт JSON</button><button onClick={() => importFileRef.current?.click()}>⇧ Импорт Trello / Flowboard JSON</button><button className="danger-action" onClick={deleteCurrentBoard}>Удалить проект</button><input ref={importFileRef} type="file" accept="application/json,.json" onChange={importBoardFile} /><section className="visibility-control"><b>Доступ к доске</b><p>{boardVisibility === 'public' ? 'Public: любой аккаунт может только смотреть.' : 'Private: видят только участники проекта.'}</p><div><button type="button" className={boardVisibility === 'public' ? 'selected' : ''} onClick={() => changeBoardVisibility('public')}>Public · просмотр всем</button><button type="button" className={boardVisibility === 'private' ? 'selected' : ''} onClick={() => changeBoardVisibility('private')}>Private</button></div>{boardVisibility === 'public' && <button className="copy-public-link" type="button" onClick={copyPublicBoardLink}>Скопировать публичную ссылку</button>}</section><form onSubmit={saveBoardBackground}><label>Фон проекта по ссылке<input value={backgroundDraft} onChange={(event) => setBackgroundDraft(event.target.value)} placeholder="https://…/background.jpg" /></label><section className="background-display-control"><b>Отображение фона</b><div className="background-fit-options"><button type="button" className={boardBackgroundFit === 'cover' ? 'selected' : ''} onClick={() => setBoardBackgroundFit('cover')}>Заполнить</button><button type="button" className={boardBackgroundFit === 'contain' ? 'selected' : ''} onClick={() => setBoardBackgroundFit('contain')}>Целиком</button><button type="button" className={boardBackgroundFit === 'fill' ? 'selected' : ''} onClick={() => setBoardBackgroundFit('fill')}>Растянуть</button></div><div className="background-position-options"><button type="button" className={boardBackgroundPosition === 'top' ? 'selected' : ''} onClick={() => setBoardBackgroundPosition('top')}>↑ Верх</button><button type="button" className={boardBackgroundPosition === 'center' ? 'selected' : ''} onClick={() => setBoardBackgroundPosition('center')}>⊙ Центр</button><button type="button" className={boardBackgroundPosition === 'bottom' ? 'selected' : ''} onClick={() => setBoardBackgroundPosition('bottom')}>↓ Низ</button></div><small>«Целиком» сохраняет изображение без обрезки, «Растянуть» подгоняет его под экран.</small></section><div><button type="submit" disabled={isSavingBackground}>{isSavingBackground ? 'Сохраняем…' : 'Сохранить фон'}</button><button type="button" onClick={() => { setBackgroundDraft(''); }}>Снять</button></div></form><input ref={boardBackgroundFileRef} type="file" accept="image/jpeg,image/png,image/gif,image/webp" onChange={uploadBoardBackground} /><button type="button" onClick={() => boardBackgroundFileRef.current?.click()} disabled={isUploadingBoardBackground}>{isUploadingBoardBackground ? 'Загружаем фон…' : '▧ Загрузить фон проекта'}</button></div>}</div></div>
        <div className="board-sort-control"><button className={`secondary-button ${cardSort !== 'manual' ? 'active-filter' : ''}`} onClick={() => setSortOpen((current) => !current)} aria-expanded={isSortOpen}>⇅ Сортировка</button>{isSortOpen && <div className="filter-popover sort-popover"><p>Порядок в колонках</p>{([['manual', 'Как на доске'], ['priority', 'Сначала важные'], ['activity', 'Недавно обновлённые']] as [CardSort, string][]).map(([mode, label]) => <button key={mode} className={cardSort === mode ? 'active' : ''} onClick={() => { setCardSort(mode); setSortOpen(false); }}>{label}{cardSort === mode && <b>✓</b>}</button>)}</div>}</div>
        <div className="board-labels-control">
          <button className={`secondary-button ${isBoardLabelsOpen ? 'active-filter' : ''}`} type="button" onClick={() => { setBoardLabelsOpen((current) => !current); setEditingBoardLabel(null); }}>▰ Метки</button>
          {isBoardLabelsOpen && <div className="board-labels-popover">
            <div className="popover-heading"><b>Метки проекта</b><button type="button" onClick={() => { setBoardLabelsOpen(false); setEditingBoardLabel(null); }} aria-label="Закрыть">×</button></div>
            <p className="board-labels-copy">Метки доступны только внутри этого проекта.</p>
            <div className="board-labels-list">{boardLabels.length ? boardLabels.map((label) => <article key={label.id}>
              {editingBoardLabel?.id === label.id ? <form onSubmit={saveBoardLabel}><input autoFocus value={boardLabelNameDraft} onChange={(event) => setBoardLabelNameDraft(event.target.value)} maxLength={60} aria-label="Название метки" /><input type="color" value={boardLabelColorDraft} onChange={(event) => setBoardLabelColorDraft(event.target.value)} aria-label="Цвет метки" /><button type="submit" disabled={!boardLabelNameDraft.trim() || isSavingBoardLabel}>Сохранить</button><button type="button" className="text-action" onClick={() => setEditingBoardLabel(null)}>Отмена</button></form> : <><span className="board-label-chip" style={{ backgroundColor: label.color }}><i />{label.name}</span>{!isPublicViewer && <span><button type="button" className="text-action" onClick={() => beginBoardLabelEdit(label)}>Изменить</button><button type="button" className="text-action danger-text" onClick={() => removeBoardLabel(label)}>Удалить</button></span>}</>}</article>) : <p className="empty-comments">На этой доске пока нет меток.</p>}</div>
            {!isPublicViewer && <form className="new-label-form board-label-create" onSubmit={createLabel}><input value={newLabelName} onChange={(event) => setNewLabelName(event.target.value)} maxLength={60} placeholder="Новая метка" aria-label="Название новой метки" /><input type="color" value={newLabelColor} onChange={(event) => setNewLabelColor(event.target.value)} aria-label="Цвет метки" /><button type="submit" disabled={!newLabelName.trim() || isSavingLabel}>{isSavingLabel ? 'Создаём…' : 'Создать'}</button></form>}
          </div>}
        </div>
      </section>
      {!isPublicViewer && isDiscordIntegrationOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setDiscordIntegrationOpen(false)}>
          <section className="archive-modal discord-integration-modal" role="dialog" aria-modal="true" aria-label="Discord API" onMouseDown={(event) => event.stopPropagation()}>
            <button className="modal-close" type="button" onClick={() => setDiscordIntegrationOpen(false)} aria-label="Закрыть">×</button>
            <p className="eyebrow">ИНТЕГРАЦИЯ</p><h2>Discord API</h2>
            <p className="archive-copy">Токен принадлежит всей этой доске: перенос карточек не ломает связь. Колонка ниже — только место для новых предложек по умолчанию.</p>
            {createdDiscordToken && <div className="discord-token"><b>Скопируйте токен</b><code>{createdDiscordToken}</code><button type="button" className="secondary-button" onClick={() => { void navigator.clipboard?.writeText(createdDiscordToken); showToast('Токен скопирован'); }}>Скопировать</button></div>}
            <form className="profile-form discord-integration-form" onSubmit={createDiscordIntegration}>
              <label>Название<input value={discordIntegrationName} maxLength={120} onChange={(event) => setDiscordIntegrationName(event.target.value)} /></label>
              <label>Колонка по умолчанию<select value={discordTargetListId} onChange={(event) => setDiscordTargetListId(event.target.value)}><option value="">Не выбирать — list_id обязателен в API</option>{columns.map((column) => <option key={column.id} value={column.id}>{column.title}</option>)}</select></label>
              <button className="create-button" type="submit" disabled={isCreatingDiscordIntegration}>{isCreatingDiscordIntegration ? 'Создаём…' : 'Создать токен'}</button>
            </form>
            <section className="discord-integration-list"><h3>Активные токены</h3>{isDiscordIntegrationLoading ? <p className="detail-loading">Загружаем…</p> : discordIntegrations.length ? discordIntegrations.map((integration) => <article key={integration.id}><div><b>{integration.name}</b><small>Вся доска · по умолчанию: {columns.find((column) => String(column.id) === integration.default_list_id)?.title ?? 'не выбрана'} · {integration.last_used_at ? `последнее использование ${new Date(integration.last_used_at).toLocaleString('ru-RU')}` : 'ещё не использовался'}</small></div><button className="danger-action" type="button" onClick={() => revokeDiscordIntegration(integration)}>Отозвать</button></article>) : <p className="empty-comments">Активных токенов нет.</p>}</section>
          </section>
        </div>}
      {isPublicViewer && <p className="public-board-notice">Публичный просмотр · Войдите в аккаунт, чтобы работать с задачами.</p>}
      <section className={`board ${isBoardPanning ? 'board-panning' : ''}`} ref={boardRef} aria-label="Канбан-доска" onPointerDown={startBoardPan} onPointerMove={moveBoardPan} onPointerUp={stopBoardPan} onPointerCancel={stopBoardPan}>
        {persistence === 'connecting' ? <div className="board-loading" role="status"><span className="loading-dot" />Загружаем вашу доску</div> : <>{visibleColumns.map((column) => <section className={`column ${dragOverListId === column.id ? 'drag-target' : ''} ${draggingColumnId === column.id ? 'column-dragging' : ''} ${columnDropTarget?.visualColumnId === column.id ? `column-drop-${columnDropTarget.edge}` : ''}`} key={column.id} aria-label={column.title} onContextMenu={(event) => { if (isPublicViewer || (event.target instanceof Element && event.target.closest('.task-card'))) return; event.preventDefault(); event.stopPropagation(); setColumnContextMenu({ column, x: Math.min(event.clientX, window.innerWidth - 210), y: Math.min(event.clientY, window.innerHeight - 150) }); }} onDragEnter={() => { if (dragging) setDragOverListId(column.id); }} onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'move'; if (draggingColumnId) { const bounds = event.currentTarget.getBoundingClientRect(); const after = event.clientX > bounds.left + bounds.width / 2; const index = columns.findIndex((item) => item.id === column.id); const next = after ? columns[index + 1] : undefined; setColumnDropTarget({ beforeColumnId: after ? next?.id ?? null : column.id, visualColumnId: column.id, edge: after ? 'after' : 'before' }); updateBoardAutoScroll(event); return; } const cardTarget = event.target instanceof Element ? event.target.closest('.task-card') : null; if (!cardTarget) setDragDropTarget({ listId: column.id, beforeCardId: null }); updateCardListAutoScroll(event); }} onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node)) { if (draggingColumnId) { setColumnDropTarget(null); stopBoardAutoScroll(); } else { setDragOverListId(null); setDragDropTarget(null); stopCardListAutoScroll(); } } }} onDrop={(event) => { event.preventDefault(); if (draggingColumnId) { const target = columnDropTarget?.visualColumnId === column.id ? columnDropTarget.beforeColumnId ?? undefined : column.id; moveColumn(draggingColumnId, target); return; } const beforeCardId = dragDropTarget?.listId === column.id ? dragDropTarget.beforeCardId ?? undefined : undefined; if (dragging) moveCard(dragging.cardId, dragging.sourceListId, column.id, beforeCardId); }}>
          <div className="column-head column-drag-handle" draggable={!isPublicViewer} onDragStart={(event) => { event.stopPropagation(); event.dataTransfer.setData('application/x-flowboard-column', String(column.id)); event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setDragImage(event.currentTarget, 30, 22); setDraggingColumnId(column.id); setColumnDropTarget(null); }} onDragEnd={clearColumnDragState}><span className="column-drag-icon" aria-hidden="true">⠿</span><div>{editingColumnId === column.id ? <form className="column-rename" onSubmit={(event) => { event.preventDefault(); saveColumnTitle(column.id); }}><input autoFocus maxLength={200} value={columnTitleDraft} onChange={(event) => setColumnTitleDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Escape') setEditingColumnId(null); }} aria-label="Название колонки" /><button type="submit" disabled={isSavingColumn}>✓</button></form> : <><h2>{column.title}</h2><span>{column.cards.length}</span></>}</div><div className="column-actions"><button className="column-menu" aria-label={`Меню колонки ${column.title}`} onClick={() => setColumnMenuId((current) => current === column.id ? null : column.id)}>•••</button>{columnMenuId === column.id && <div className="column-popover"><button onClick={() => beginColumnRename(column)}>Переименовать</button><button className="danger-action" onClick={() => deleteColumn(column)}>Удалить пустую</button></div>}</div></div>
          <div className={`card-list ${dragDropTarget?.listId === column.id && dragDropTarget.beforeCardId === null ? 'drop-at-end' : ''}`}>{column.cards.map((card) => <article className={`task-card ${card.completedAt ? 'completed' : ''} ${labelsCollapsed ? 'labels-collapsed' : ''} ${dragging?.cardId === card.id ? 'dragging' : ''} ${dragDropTarget?.listId === column.id && dragDropTarget.beforeCardId === card.id ? 'drop-before' : ''}`} key={card.id} draggable={!isPublicViewer} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setCardContextMenu({ card, x: Math.min(event.clientX, window.innerWidth - 210), y: Math.min(event.clientY, window.innerHeight - 170) }); }} onDragStart={(event) => { didDragRef.current = false; event.dataTransfer.setData('application/x-flowboard-card', String(card.id)); event.dataTransfer.effectAllowed = 'move'; setDragging({ cardId: card.id, sourceListId: column.id }); setDragDropTarget(null); }} onDragEnd={() => { didDragRef.current = true; clearDragState(); window.setTimeout(() => { didDragRef.current = false; }, 0); }} onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'move'; if (draggingColumnId || !dragging || dragging.cardId === card.id) return; const bounds = event.currentTarget.getBoundingClientRect(); const cardIndex = column.cards.findIndex((item) => item.id === card.id); const nextCard = event.clientY > bounds.top + bounds.height / 2 ? column.cards[cardIndex + 1] : card; setDragOverListId(column.id); setDragDropTarget({ listId: column.id, beforeCardId: nextCard?.id ?? null }); updateCardListAutoScroll(event); }} onDrop={(event) => { event.preventDefault(); event.stopPropagation(); if (draggingColumnId) { moveColumn(draggingColumnId, column.id); return; } const beforeCardId = dragDropTarget?.listId === column.id ? dragDropTarget.beforeCardId ?? undefined : card.id; if (!dragging || dragging.cardId === card.id || beforeCardId === dragging.cardId) { clearDragState(); return; } moveCard(dragging.cardId, dragging.sourceListId, column.id, beforeCardId); }} onClick={() => { if (!didDragRef.current) openCard(card); }}>
            {card.hasUnreadMentions && <span className="card-mention-dot" title="Вас упомянули в этой карточке" aria-label="Вас упомянули в этой карточке" />}{card.coverUrl && <div className={`card-cover ${card.coverMode ?? 'full'}`}><img src={assetUrl(card.coverUrl)} alt="" /></div>}<div className="card-main">{card.labels.length > 0 && <div className="card-top"><div className="card-labels">{card.labels.map((label) => <button className="label custom-label" key={label.id} style={{ color: '#F7F8FC', backgroundColor: `${label.color}66` }} onClick={(event) => { event.stopPropagation(); setLabelsCollapsed((current) => !current); }}>{label.name}</button>)}</div></div>}<div className="card-title-row"><button className="card-complete" aria-label={card.completedAt ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(card.completedAt)} onClick={(event) => toggleCardCompletion(card, event)}>{card.completedAt && '✓'}</button><h3>{card.title}</h3></div>{card.dueAt && <p className={`due ${new Date(card.dueAt).getTime() < Date.now() ? 'today' : ''}`}>◷ {formatDue(card.dueAt)}</p>}</div>
            {card.priority ? <span className="card-priority-corner" style={{ right: card.members.length ? 96 : 14 }}><PrioritySignal priority={card.priority} /></span> : null}
            {(card.checklist || card.comments || card.attachments || card.members.length > 0) && <footer className="card-footer"><div className="card-meta">{card.checklist && <span className={isChecklistComplete(card.checklist) ? 'checklist-complete' : ''}><CardMetaIcon type="checklist" />{card.checklist}</span>}{card.comments && <span><CardMetaIcon type="comments" />{card.comments}</span>}{card.attachments && <span title="Есть вложения"><CardMetaIcon type="attachments" /></span>}</div><div className="card-avatars">{card.members.map((member) => <Avatar key={member.id} member={member} />)}</div></footer>}
          </article>)}</div>
          {isComposerOpen === column.id ? <form className="composer" onSubmit={(event) => addCard(event, column.id)}><textarea autoFocus value={draft} onChange={(event) => setDraft(event.target.value)} placeholder="Название задачи" /><div><button className="add-card" type="submit">Добавить</button><button className="cancel" type="button" onClick={() => { setComposerOpen(null); setDraft(''); }}>Отмена</button></div></form> : <button className="add-task" onClick={() => setComposerOpen(column.id)}>＋ Добавить задачу</button>}
        </section>)}
        <button className="add-column" onClick={addColumn}>＋ Добавить колонку</button></>}
      </section>
      {isArchiveOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setArchiveOpen(false)}><section className="archive-modal" role="dialog" aria-modal="true" aria-label="Архив задач" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setArchiveOpen(false)} aria-label="Закрыть архив">×</button><p className="eyebrow">АРХИВ ПРОЕКТА</p><h2>Архивированные задачи</h2><p className="archive-copy">Восстановленная задача вернётся в свою последнюю колонку.</p>{isArchiveLoading ? <p className="detail-loading">Загружаем архив…</p> : archivedCards.length ? <div className="archive-list">{archivedCards.map((card) => <article key={card.id}><div><b>{card.title}</b>{card.description && <small>{card.description}</small>}<time>{new Date(card.archived_at).toLocaleString('ru-RU')}</time></div><button onClick={() => restoreArchivedCard(card)}>Восстановить</button></article>)}</div> : <p className="empty-comments">В архиве пока нет задач.</p>}</section></div>}
      {isTeamOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setTeamOpen(false)}><section className="archive-modal team-modal" role="dialog" aria-modal="true" aria-label="Команда проекта" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setTeamOpen(false)} aria-label="Закрыть команду">×</button><p className="eyebrow">ПРОЕКТ</p><h2>Команда и доступы</h2><p className="archive-copy">Участник получает доступ только к этому проекту. Выберите готовую роль — сервер применяет права на каждом запросе.</p><div className="role-guide">{(['viewer', 'contributor', 'editor', 'full_access'] as TeamMember['preset'][]).map((role) => <div key={role}><b>{roleLabels[role]}</b><span>{roleDescriptions[role]}</span></div>)}</div>{isTeamLoading ? <p className="detail-loading">Загружаем участников…</p> : <><form className="member-picker" onSubmit={addWorkspaceMember}><label><span>Добавить участника</span><input autoFocus value={accountSearch} onChange={(event) => { setAccountSearch(event.target.value); setSelectedAccountId(''); }} placeholder="Найти по @нику" /></label><div className="member-picker-results">{availableAccounts.filter((item) => item.username.toLowerCase().includes(accountSearch.trim().replace(/^@/, '').toLowerCase())).slice(0, 6).map((item) => <button type="button" className={selectedAccountId === item.id ? 'selected' : ''} key={item.id} onClick={() => setSelectedAccountId(item.id)}><Avatar member={memberFromApi(item)} /><span>@{item.username}</span><small>{selectedAccountId === item.id ? 'Выбран' : 'Выбрать'}</small></button>)}{!availableAccounts.filter((item) => item.username.toLowerCase().includes(accountSearch.trim().replace(/^@/, '').toLowerCase())).length && <p className="empty-comments">Подходящих активных аккаунтов нет.</p>}</div><button className="create-button" disabled={!selectedAccountId || isSavingMember}>{isSavingMember ? 'Добавляем…' : 'Добавить в проект'}</button></form><div className="team-list">{teamMembers.map((member) => <article key={member.id}><Avatar member={memberFromApi({ id: member.id, username: member.username, avatar_url: member.avatar_url })} /><div><b>@{member.username}</b><small>{roleDescriptions[member.preset]}</small></div>{member.preset === 'owner' ? <span className="role-badge owner">Владелец</span> : <div className="team-actions"><select value={member.preset} onChange={(event) => changeTeamPreset(member, event.target.value as TeamMember['preset'])} aria-label={`Роль для @${member.username}`}><option value="viewer">Наблюдатель — только просмотр</option><option value="contributor">Участник — карточки</option><option value="editor">Редактор — карточки, колонки, метки</option><option value="full_access">Полный доступ — команда и настройки</option></select><button onClick={() => removeTeamMember(member)}>Исключить</button></div>}</article>)}</div></>}</section></div>}
    </>}

    {isDiagramOpen && <div className="modal-backdrop diagram-backdrop" role="presentation" onMouseDown={() => setDiagramOpen(false)}>
      <section className="diagram-modal" role="dialog" aria-modal="true" aria-label="Схема задачи" onMouseDown={(event) => event.stopPropagation()}>
        <button className="modal-close" onClick={() => setDiagramOpen(false)} aria-label="Закрыть">×</button>
        <p className="eyebrow">CANVAS</p>
        <input className="diagram-title" value={diagramTitle} onChange={(event) => setDiagramTitle(event.target.value)} maxLength={120} aria-label="Название схемы" />
        <p className="diagram-hint">Выберите инструмент, затем рисуйте на полотне. Текст вставляется кликом по полотну.</p>
        <div className="diagram-toolbar" role="toolbar" aria-label="Инструменты схемы">
          <div className="diagram-tool-group">
            {([
              ['select', '⌖', 'Выбрать и перемещать'],
              ['draw', '✎', 'Карандаш'],
              ['rectangle', '▭', 'Прямоугольник'],
              ['ellipse', '◯', 'Круг / овал'],
              ['arrow', '→', 'Стрелка'],
              ['text', 'T', 'Текст'],
              ['callout', '↗', 'Текстовая выноска'],
            ] as [DiagramTool, string, string][]).map(([tool, icon, label]) => <button key={tool} type="button" className={`diagram-tool ${diagramTool === tool ? 'active' : ''}`} onClick={() => setDiagramTool(tool)} title={label} aria-label={label}>{icon}</button>)}
          </div>
          <label className="diagram-control">Цвет<input type="color" value={diagramColor} onChange={(event) => setDiagramColor(event.target.value)} aria-label="Цвет" /></label>
          <div className="diagram-width-picker" aria-label="Толщина кисти и линий"><span>Кисть / линия</span><div>{([2, 3, 6, 12] as const).map((width) => <button type="button" key={width} className={diagramLineWidth === width ? 'active' : ''} onClick={() => setDiagramLineWidth(width)} aria-label={`${width} px`} title={`${width} px`}><i style={{ width, height: width }} /></button>)}</div></div>
          {(diagramTool === 'text' || diagramTool === 'callout') && <>
            <label className="diagram-control">Шрифт<select value={diagramFontFamily} onChange={(event) => setDiagramFontFamily(event.target.value)} aria-label="Шрифт"><option value="Inter, system-ui, sans-serif">Sans</option><option value="Georgia, serif">Serif</option><option value="ui-monospace, SFMono-Regular, Menlo, monospace">Mono</option></select></label>
            <label className="diagram-control">Размер<select value={diagramFontSize} onChange={(event) => setDiagramFontSize(Number(event.target.value))} aria-label="Размер шрифта"><option value={16}>16 px</option><option value={22}>22 px</option><option value={30}>30 px</option><option value={42}>42 px</option></select></label>
            <button type="button" className={`diagram-tool diagram-bold ${diagramFontWeight === 'bold' ? 'active' : ''}`} onClick={() => setDiagramFontWeight((current) => current === 'bold' ? 'normal' : 'bold')} aria-label="Полужирный текст"><b>B</b></button>
          </>}
        </div>
        {(diagramTool === 'text' || diagramTool === 'callout') && <label className="diagram-text-draft">Текст для вставки<textarea value={diagramTextDraft} onChange={(event) => setDiagramTextDraft(event.target.value)} maxLength={4000} placeholder={diagramTool === 'callout' ? 'Напишите текст, затем протяните выноску от объекта…' : 'Напишите текст, затем кликните по полотну…'} /></label>}
        <div className="diagram-zoom" aria-label="Масштаб схемы"><span>Масштаб</span><button type="button" onClick={() => setDiagramZoom((current) => Math.max(.4, Number((current - .1).toFixed(2))))} disabled={diagramZoom <= .4} aria-label="Отдалить">−</button><output>{Math.round(diagramZoom * 100)}%</output><button type="button" onClick={() => setDiagramZoom((current) => Math.min(1.6, Number((current + .1).toFixed(2))))} disabled={diagramZoom >= 1.6} aria-label="Приблизить">+</button><button type="button" onClick={() => setDiagramZoom(1)}>100%</button></div>
        <div className="diagram-viewport"><canvas ref={diagramCanvasRef} className={`diagram-canvas tool-${diagramTool}`} width="1600" height="960" style={{ width: `${Math.round(1600 * diagramZoom)}px`, height: `${Math.round(960 * diagramZoom)}px` }} onPointerDown={startDiagramStroke} onPointerMove={continueDiagramStroke} onPointerUp={finishDiagramInteraction} onPointerCancel={finishDiagramInteraction} /></div>
        <div className="diagram-actions"><button className="secondary-button" onClick={undoDiagram} disabled={!diagramHistory.length}>↶ Отменить</button><button className="secondary-button" onClick={() => { rememberDiagramState(); setDiagramStrokes([]); setDiagramElements([]); setDiagramPreview(null); setSelectedDiagramElement(null); }} disabled={!diagramStrokes.length && !diagramElements.length}>Очистить</button><button className="create-button" onClick={saveDiagram} disabled={isDiagramSaving}>{isDiagramSaving ? 'Сохраняем…' : 'Сохранить схему'}</button></div>
      </section>
    </div>}

    {selected && <div className="modal-backdrop" role="presentation" onMouseDown={closeSelectedCard}>
      <section className="task-modal" role="dialog" aria-modal="true" aria-label="Детали задачи" onMouseDown={(event) => event.stopPropagation()}>
        <button className="modal-close" onClick={closeSelectedCard} aria-label="Закрыть">×</button>
        <div className="task-layout">
          <div className="task-content">
            <div className={`card-detail-top ${selected.backgroundImageUrl ? 'has-card-background' : ''}`} style={selected.backgroundImageUrl ? { backgroundImage: `linear-gradient(rgb(13 18 23 / 45%), rgb(13 18 23 / 72%)), url("${assetUrl(selected.backgroundImageUrl)}")` } : undefined}>
            <div className="card-property-area">
              <div className="card-quick-actions"><button className="quick-action" onClick={openDiagram}>⌁ Схема</button><button className={`quick-action ${sidebarPanel === 'labels' ? 'active' : ''}`} onClick={() => { setExistingLabelsOnly(false); setSidebarPanel((current) => current === 'labels' ? null : 'labels'); }}>🏷 Метки</button><button className={`quick-action ${sidebarPanel === 'due' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'due' ? null : 'due')}>◷ {selected.dueAt ? formatDue(selected.dueAt) : 'Дедлайн'}</button><button className={`quick-action ${sidebarPanel === 'background' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'background' ? null : 'background')}>▧ Фон</button><button className={`quick-action ${sidebarPanel === 'public-visibility' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'public-visibility' ? null : 'public-visibility')}>◉ Доступ</button></div>
              {sidebarPanel && sidebarPanel !== 'assignees' && <div className="property-popover quick-property-popover" role="dialog" aria-label="Настройки карточки">
                {sidebarPanel === 'labels' && <><div className="popover-heading"><b>Метки</b><button onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><div className="label-options">{boardLabels.map((label) => <button key={label.id} className={`label-option ${selected.labels.some((current) => current.id === label.id) ? 'selected' : ''}`} style={{ borderColor: label.color, backgroundColor: `${label.color}22` }} onClick={() => toggleSelectedLabel(label)}><i style={{ backgroundColor: label.color }} /><span>{label.name}</span>{selected.labels.some((current) => current.id === label.id) && <b>✓</b>}</button>)}</div>{!existingLabelsOnly && <form className="new-label-form" onSubmit={createLabel}><input value={newLabelName} onChange={(event) => setNewLabelName(event.target.value)} maxLength={60} placeholder="Новая метка" aria-label="Название новой метки" /><input type="color" value={newLabelColor} onChange={(event) => setNewLabelColor(event.target.value)} aria-label="Цвет метки" /><button type="submit" disabled={!newLabelName.trim() || isSavingLabel}>{isSavingLabel ? 'Создаём…' : 'Создать метку'}</button></form>}</>}
                {sidebarPanel === 'due' && <div className="date-panel"><div className="popover-heading"><b>Дедлайн</b><button onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><div className="calendar-head"><button onClick={() => setDueCursor((current) => new Date(current.getFullYear(), current.getMonth() - 1, 1))} aria-label="Предыдущий месяц">‹</button><strong>{monthNames[dueCursor.getMonth()]} {dueCursor.getFullYear()}</strong><button onClick={() => setDueCursor((current) => new Date(current.getFullYear(), current.getMonth() + 1, 1))} aria-label="Следующий месяц">›</button></div><div className="calendar-weekdays">{weekdayNames.map((day) => <span key={day}>{day}</span>)}</div><div className="calendar-grid">{dueDays.map((day) => <button key={day.toISOString()} className={`${day.getMonth() !== dueCursor.getMonth() ? 'outside' : ''} ${selected.dueAt && isSameDay(day, new Date(selected.dueAt)) ? 'chosen' : ''} ${isSameDay(day, new Date()) ? 'today' : ''}`} onClick={() => saveDueDate(day, dueTime)}>{day.getDate()}</button>)}</div><div className="time-options">{dueTimeOptions.map((time) => <button key={time} className={dueTime === time ? 'selected' : ''} onClick={() => { setDueTime(time); if (selected.dueAt) saveDueDate(new Date(selected.dueAt), time); }}>{time}</button>)}</div>{selected.dueAt && <button className="clear-deadline" onClick={clearDueDate}>Снять дедлайн</button>}</div>}
                {sidebarPanel === 'background' && <div className="card-background-form"><div className="popover-heading"><b>Фон карточки</b><button type="button" onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><p>Загрузите изображение с компьютера — оно сохранится в Flowboard.</p><input ref={cardBackgroundFileRef} type="file" accept="image/jpeg,image/png,image/gif,image/webp" onChange={uploadCardBackground} /><div><button className="secondary-button" type="button" onClick={clearCardBackground}>Снять</button><button className="create-button" type="button" onClick={() => cardBackgroundFileRef.current?.click()} disabled={isUploadingCardBackground}>{isUploadingCardBackground ? 'Загружаем…' : 'Выбрать файл'}</button></div></div>}
                {sidebarPanel === 'public-visibility' && <div className="card-public-visibility"><div className="popover-heading"><b>Видимость карточки</b><button type="button" onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><label><input type="checkbox" checked={selected.isPublic ?? true} onChange={(event) => setSelectedCardPublicVisibility(event.target.checked)} /> Видна гостям</label><p>Снимите галочку, чтобы карточка и её вложения были доступны только после входа в аккаунт.</p></div>}
              </div>}
              <div className="detail-title-row"><button className={`detail-card-complete ${selected.completedAt ? 'done' : ''}`} aria-label={selected.completedAt ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(selected.completedAt)} onClick={(event) => toggleCardCompletion(selected, event)}>{selected.completedAt && '✓'}</button><input className="card-title-input" value={cardTitleDraft} onChange={(event) => setCardTitleDraft(event.target.value)} aria-label="Название задачи" /></div>
              <section className="card-priority-editor" aria-label="Приоритет задачи"><span>Приоритет</span><div><button type="button" className={(selected.priority ?? 0) === 0 ? 'selected' : ''} onClick={() => setSelectedCardPriority(0)}>Нет</button>{[1, 2, 3, 4, 5].map((level) => <button type="button" className={(selected.priority ?? 0) === level ? 'selected' : ''} onClick={() => setSelectedCardPriority(level)} key={level}><PrioritySignal priority={level} /></button>)}</div></section>
              <div className="card-members-labels-row"><div className="card-members-row"><span>Исполнители</span><div className="card-member-control">{selected.members.map((member) => <Avatar member={member} key={member.id} />)}<button className={`member-plus ${sidebarPanel === 'assignees' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'assignees' ? null : 'assignees')} aria-label="Назначить исполнителя">＋</button>{sidebarPanel === 'assignees' && <div className="property-popover assignees-popover" role="dialog" aria-label="Выбор исполнителей"><div className="popover-heading"><b>Исполнители</b><button onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><div className="member-options">{workspaceMembers.map((member) => <button key={member.id} className={`member-option ${selected.members.some((current) => current.id === member.id) ? 'selected' : ''}`} onClick={() => toggleSelectedMember(member)}><Avatar member={member} /><span>{member.name}</span>{selected.members.some((current) => current.id === member.id) && <b>✓</b>}</button>)}</div><p className="empty-comments">Состав пространства меняется в разделе «Команда».</p></div>}</div></div><div className="detail-card-labels"><span>Метки</span><div className="card-labels">{selected.labels.map((label) => <span className="label custom-label" key={label.id} style={{ color: '#F7F8FC', backgroundColor: `${label.color}66` }}>{label.name}</span>)}<button className={`label-plus ${sidebarPanel === 'labels' ? 'active' : ''}`} onClick={() => { setExistingLabelsOnly(true); setSidebarPanel((current) => current === 'labels' ? null : 'labels'); }} aria-label="Добавить существующую метку">＋</button></div></div></div>
            </div>
            <p className="modal-subtitle">Задача в проекте «{boardTitle}» · {cardSaveStatus === 'saving' ? 'Сохраняем…' : cardSaveStatus === 'error' ? 'Ошибка сохранения' : 'Сохранено'}</p>
            <section className="description-section"><div className="section-heading"><h3>Описание</h3></div>{isEditingCardDescription ? <MentionTextarea autoFocus className={`card-description-input media-drop-target ${isUploadingAttachment ? 'uploading' : ''} ${unreadMentionSourceIds.includes(String(selected.id)) ? 'mention-highlight' : ''}`} value={cardDescriptionDraft} onValueChange={setCardDescriptionDraft} onBlur={() => setEditingCardDescription(false)} members={account ? workspaceMembers : []} onDragOver={(event) => { if (event.dataTransfer.types.includes('Files')) event.preventDefault(); }} onDrop={(event) => handleMediaDrop(event, 'description')} onPaste={(event) => handleMediaPaste(event, 'description')} placeholder="Добавьте описание или перетащите изображение/видео…" ariaLabel="Описание задачи" /> : <div className={`markdown-editable-description ${unreadMentionSourceIds.includes(String(selected.id)) ? 'mention-highlight' : ''}`} role={isPublicViewer ? undefined : 'button'} tabIndex={isPublicViewer ? undefined : 0} onClick={() => { if (!isPublicViewer) setEditingCardDescription(true); }} onKeyDown={(event) => { if (!isPublicViewer && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); setEditingCardDescription(true); } }}><MarkdownDescription value={cardDescriptionDraft} highlightMentions={unreadMentionSourceIds.includes(String(selected.id))} /></div>}</section>
            </div>
            <section className="checklists"><div className="section-heading"><h3>Чек-листы</h3><span>{checklists.length || '—'}</span></div>{isDetailsLoading ? <p className="detail-loading">Загружаем чек-листы…</p> : <>{checklists.map((checklist) => { const completed = checklist.items.filter((item) => item.is_completed).length; const itemIds = checklist.items.map((item) => String(item.id)); const allExpanded = itemIds.length > 0 && itemIds.every((id) => expandedChecklistItemIds.includes(id)); return <section className="checklist" key={checklist.id}><div className="section-heading"><h4>{checklist.title}</h4><span>{completed}/{checklist.items.length}</span><button className="text-action checklist-all-toggle" type="button" title={allExpanded ? 'Свернуть все детали' : 'Раскрыть все детали'} onClick={() => setExpandedChecklistItemIds((current) => allExpanded ? current.filter((id) => !itemIds.includes(id)) : [...new Set([...current, ...itemIds])])}>{allExpanded ? '⌃ Все' : '⌄ Все'}</button><button className="text-action danger-text" onClick={() => deleteChecklist(checklist)}>Удалить</button></div><div className="progress"><i style={{ width: `${checklist.items.length ? completed / checklist.items.length * 100 : 0}%` }} /></div>{checklists.map((checklist) => checklist).filter((currentChecklist) => currentChecklist.id === checklist.id).flatMap((currentChecklist) => currentChecklist.items).map((item) => { const itemId = String(item.id); const isExpanded = expandedChecklistItemIds.includes(itemId); return <div className="checklist-item" key={item.id}><div className="check-row"><button className={`check-item ${item.is_completed ? 'done' : ''}`} onClick={() => toggleChecklistItem(checklist.id, item)} aria-pressed={item.is_completed}><span className="check-control">{item.is_completed && '✓'}</span>{item.title}</button><button className={`check-item-toggle ${isExpanded ? 'open' : ''}`} type="button" title={isExpanded ? 'Скрыть детали пункта' : 'Раскрыть детали пункта'} aria-expanded={isExpanded} onClick={() => setExpandedChecklistItemIds((current) => current.includes(itemId) ? current.filter((id) => id !== itemId) : [...current, itemId])}>⌄</button><button className="remove-check" onClick={() => removeChecklistItem(checklist.id, item)} aria-label={`Удалить пункт ${item.title}`}>×</button></div>{isExpanded && <div className="check-item-detail"><MentionTextarea className={unreadMentionSourceIds.includes(itemId) ? 'mention-highlight' : undefined} value={checklistItemDescriptionDrafts[itemId] ?? item.description} onValueChange={(value) => setChecklistItemDescriptionDrafts((current) => ({ ...current, [itemId]: value }))} onBlur={() => saveChecklistItemDescription(checklist.id, item)} members={account ? workspaceMembers : []} maxLength={4000} placeholder="Описание пункта…" ariaLabel={`Описание пункта ${item.title}`} /><label className="check-item-upload">{isUploadingChecklistItemAttachment ? 'Загружаем…' : '＋ Картинка или видео'}<input type="file" accept="image/jpeg,image/png,image/gif,image/webp,video/mp4,video/webm,video/quicktime" multiple disabled={isUploadingChecklistItemAttachment} onChange={(event) => { const files = Array.from(event.target.files ?? []); event.target.value = ''; void uploadChecklistItemAttachments(checklist.id, item, files); }} /></label>{item.attachments.length > 0 && <div className="check-item-attachments">{item.attachments.map((attachment) => <figure key={attachment.id}>{attachment.media_type.startsWith('image/') ? <button className="check-item-image" type="button" onClick={() => setImagePreview({ url: assetUrl(attachment.url), name: attachment.original_name })}><img src={assetUrl(attachment.url)} alt={attachment.original_name} /></button> : attachment.media_type.startsWith('video/') ? <video controls preload="metadata" src={assetUrl(attachment.url)} /> : <a href={assetUrl(attachment.url)} target="_blank" rel="noreferrer">{attachment.original_name}</a>}<figcaption><span>{attachment.original_name}</span><button type="button" onClick={() => deleteChecklistItemAttachment(checklist.id, item, attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></figcaption></figure>)}</div>}</div>}</div>; })}<form className="inline-composer" onSubmit={(event) => addChecklistItem(event, checklist.id)}><input value={checklistItemDrafts[checklist.id] ?? ''} onChange={(event) => setChecklistItemDrafts((current) => ({ ...current, [checklist.id]: event.target.value }))} maxLength={500} placeholder="Добавить пункт…" aria-label={`Новый пункт для ${checklist.title}`} /><button type="submit" disabled={isSavingChecklist || !(checklistItemDrafts[checklist.id] ?? '').trim()}>Добавить</button></form></section>; })}<form className="new-checklist-form" onSubmit={createChecklist}><input value={checklistNameDraft} onChange={(event) => setChecklistNameDraft(event.target.value)} maxLength={200} placeholder="Название нового чек-листа" aria-label="Название нового чек-листа" /><button type="submit" disabled={isSavingChecklist || !checklistNameDraft.trim()}>＋ Чек-лист</button></form></>}</section>
            <div className="attachments"><div className="section-heading"><h3>Вложения</h3><span>{attachments.length}</span></div>{attachments.length ? <div className="attachment-grid">{attachments.map((attachment) => attachment.media_type.startsWith('image/') ? <figure className="attachment-preview" key={attachment.id}><img src={assetUrl(attachment.url)} alt={attachment.original_name} /><figcaption><span>{attachment.original_name}</span><div className="cover-controls"><select value={selected.coverAttachmentId === attachment.id ? selected.coverMode ?? 'full' : coverModeDraft} onChange={(event) => { const mode = event.target.value as 'full' | 'top'; setCoverModeDraft(mode); if (selected.coverAttachmentId === attachment.id) updateCardCover(attachment, mode); }} aria-label="Тип обложки"><option value="full">Фон</option><option value="top">Сверху</option></select><button className="cover-button" onClick={() => updateCardCover(selected.coverAttachmentId === attachment.id ? null : attachment)}>{selected.coverAttachmentId === attachment.id ? 'Снять' : 'Установить'}</button></div><button className="attachment-remove" onClick={() => deleteAttachment(attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></figcaption></figure> : attachment.media_type.startsWith('video/') ? <figure className="attachment-preview" key={attachment.id}><video controls preload="metadata" src={assetUrl(attachment.url)} /><figcaption>{attachment.original_name}<button onClick={() => deleteAttachment(attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></figcaption></figure> : <div className="attachment-file" key={attachment.id}><span>▶</span><a href={assetUrl(attachment.url)} target="_blank" rel="noreferrer">{attachment.original_name}</a><button onClick={() => deleteAttachment(attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></div>)}</div> : <p className="empty-attachments">Прикрепите изображение или видео до 50 МиБ.</p>}<label className="upload-button">{isUploadingAttachment ? 'Загружаем…' : '＋ Добавить файл'}<input type="file" accept="image/jpeg,image/png,image/gif,image/webp,video/mp4,video/webm,video/quicktime" multiple disabled={isUploadingAttachment} onChange={uploadAttachments} /></label></div>
            <footer className="modal-actions"><button className="archive-button" onClick={archiveSelectedCard}>Архивировать</button><span className={`autosave-status ${cardSaveStatus}`}>{cardSaveStatus === 'saving' ? 'Изменения сохраняются' : cardSaveStatus === 'error' ? 'Не удалось сохранить' : 'Все изменения сохранены'}</span></footer>
          </div>
          <aside className="task-sidebar" aria-label="Комментарии и активность">
            <section className="conversation-panel" aria-label="Комментарии и активность">
              <div className="conversation-heading"><div><p className="sidebar-caption">ОБСУЖДЕНИЕ</p><h3>Комментарии и активность</h3></div><span>{comments.length}</span></div>
              {isDetailsLoading ? <p className="detail-loading">Загружаем сообщения…</p> : <div className="conversation-scroll">
                {comments.filter((comment) => !comment.parent_comment_id).map((comment) => <div className="comment-thread" key={comment.id}>
                  <div className="comment"><Avatar member={comment.author_id === account?.user.id ? currentMember : { id: `comment-${comment.id}`, initials: comment.author_name.slice(0, 2).toUpperCase() || 'У', color: 'mint', name: comment.author_name, avatarUrl: comment.author_avatar_url }} /><div className="comment-body">{editingCommentId === comment.id ? <form className="comment-edit" onSubmit={(event) => { event.preventDefault(); saveCommentEdit(comment); }}><MentionTextarea autoFocus value={commentEditDraft} onValueChange={setCommentEditDraft} members={account ? workspaceMembers : []} maxLength={10000} ariaLabel="Изменить комментарий" /><div><button type="submit">Сохранить</button><button type="button" onClick={() => setEditingCommentId(null)}>Отмена</button></div></form> : <><header><b>@{comment.author_name}</b><time>{comment.created_at ? new Date(comment.created_at).toLocaleString('ru-RU', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' }) : 'только что'}{comment.edited_at && ' · изменено'}</time></header><div className="comment-text"><MarkdownDescription value={comment.body} highlightMentions={unreadMentionSourceIds.includes(String(comment.id))} /></div><div className="comment-actions"><button onClick={() => { setReplyToCommentId(comment.id); setCommentDraft(''); }}>Ответить</button>{comment.author_id === account?.user.id && <><button onClick={() => beginCommentEdit(comment)}>Изменить</button><button onClick={() => removeComment(comment)}>Удалить</button></>}</div></>}</div></div>
                  {comments.filter((reply) => reply.parent_comment_id === String(comment.id)).map((reply) => <div className="comment comment-reply" key={reply.id}><Avatar member={reply.author_id === account?.user.id ? currentMember : { id: `comment-${reply.id}`, initials: reply.author_name.slice(0, 2).toUpperCase() || 'У', color: 'mint', name: reply.author_name, avatarUrl: reply.author_avatar_url }} /><div className="comment-body">{editingCommentId === reply.id ? <form className="comment-edit" onSubmit={(event) => { event.preventDefault(); saveCommentEdit(reply); }}><MentionTextarea autoFocus value={commentEditDraft} onValueChange={setCommentEditDraft} members={account ? workspaceMembers : []} maxLength={10000} ariaLabel="Изменить ответ" /><div><button type="submit">Сохранить</button><button type="button" onClick={() => setEditingCommentId(null)}>Отмена</button></div></form> : <><header><b>@{reply.author_name}</b><time>{reply.created_at ? new Date(reply.created_at).toLocaleString('ru-RU', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' }) : 'только что'}{reply.edited_at && ' · изменено'}</time></header><div className="comment-text"><MarkdownDescription value={reply.body} highlightMentions={unreadMentionSourceIds.includes(String(reply.id))} /></div><div className="comment-actions">{reply.author_id === account?.user.id && <><button onClick={() => beginCommentEdit(reply)}>Изменить</button><button onClick={() => removeComment(reply)}>Удалить</button></>}</div></>}</div></div>)}</div>)}
                {!comments.length && <p className="empty-comments">Пока нет сообщений. Начните обсуждение.</p>}
                {activity.map((item) => <div className="activity-message" key={item.id}><i>Console</i><p><b>@{item.actor_name ?? 'Deleted user'}</b> {activityLabel(item.action)}{item.detail && <> · {item.detail}</>}<small>{new Date(item.created_at).toLocaleString('ru-RU', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' })}</small></p></div>)}
              </div>}
              <form className="comment-composer" onSubmit={addComment}>{replyToCommentId && <div className="replying-to">Ответ на сообщение <button type="button" onClick={() => setReplyToCommentId(null)}>×</button></div>}<MentionTextarea className={`media-drop-target ${isUploadingAttachment ? 'uploading' : ''}`} value={commentDraft} onValueChange={setCommentDraft} members={account ? workspaceMembers : []} onDragOver={(event) => { if (event.dataTransfer.types.includes('Files')) event.preventDefault(); }} onDrop={(event) => handleMediaDrop(event, 'comment')} onPaste={(event) => handleMediaPaste(event, 'comment')} maxLength={10000} placeholder={replyToCommentId ? 'Написать ответ…' : 'Написать комментарий или перетащить медиа…'} ariaLabel="Написать комментарий" /><button className="add-card" type="submit" disabled={isSendingComment || !commentDraft.trim()}>{isSendingComment ? 'Отправка…' : replyToCommentId ? 'Ответить' : 'Отправить'}</button></form>
            </section>
          </aside>
        </div>
      </section>
    </div>}
    {imagePreview && <div className="image-preview-backdrop" role="presentation" onMouseDown={() => setImagePreview(null)}><figure className="image-preview-modal" role="dialog" aria-modal="true" aria-label={`Просмотр ${imagePreview.name}`} onMouseDown={(event) => event.stopPropagation()}><button type="button" onClick={() => setImagePreview(null)} aria-label="Закрыть просмотр">×</button><img src={imagePreview.url} alt={imagePreview.name} /><figcaption>{imagePreview.name}</figcaption></figure></div>}
    {cardContextMenu && <div className="card-context-menu" role="menu" style={{ left: cardContextMenu.x, top: cardContextMenu.y }} onPointerDown={(event) => event.stopPropagation()}><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); openCard(cardContextMenu.card); setCardContextMenu(null); }}>Открыть карточку</button>{!isPublicViewer && <><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); toggleCardCompletion(cardContextMenu.card); setCardContextMenu(null); }}>{cardContextMenu.card.completedAt ? 'Вернуть в работу' : 'Отметить выполненной'}</button><button className="danger-action" type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); archiveCard(cardContextMenu.card); setCardContextMenu(null); }}>Архивировать</button></>}</div>}
    {columnContextMenu && <div className="card-context-menu" role="menu" style={{ left: columnContextMenu.x, top: columnContextMenu.y }} onPointerDown={(event) => event.stopPropagation()}><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); setComposerOpen(columnContextMenu.column.id); setColumnContextMenu(null); }}>Добавить задачу</button><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); addColumn(columnContextMenu.column); setColumnContextMenu(null); }}>Добавить колонку ниже</button><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); beginColumnRename(columnContextMenu.column); setColumnContextMenu(null); }}>Переименовать</button><button className="danger-action" type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); deleteColumn(columnContextMenu.column); setColumnContextMenu(null); }}>Удалить пустую</button></div>}
    {toast && <div className="toast">✓ {toast}</div>}
  </main>;
}
