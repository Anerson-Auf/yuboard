'use client';
/* eslint-disable @next/next/no-img-element -- self-hosted attachment URLs are served by the Rust API. */

import { ChangeEvent, ClipboardEvent as ReactClipboardEvent, CSSProperties, DragEvent as ReactDragEvent, FormEvent, MouseEvent as ReactMouseEvent, MouseEventHandler, PointerEvent as ReactPointerEvent, ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import './auth.css';

type EntityId = number | string;
type Member = { id: EntityId; initials: string; color: string; name: string; avatarUrl?: string | null };
type RoleShape = 'circle' | 'square' | 'diamond' | 'star' | 'triangle' | 'hexagon' | 'bolt' | 'flag' | 'check' | 'cross';
type Label = { id: string; name: string; color: string; icon_shape?: RoleShape; icon_color?: string };
type ProfileRole = { id: string; name: string; color: string; icon_shape: RoleShape; icon_color: string };
type Milestone = { id: string; name: string; description: string; color: string; target_date?: string | null };
type Card = { id: EntityId; title: string; description?: string; priority?: number; lastActivityAt?: string; dueAt?: string; coverAttachmentId?: string; coverUrl?: string; coverMode?: 'full' | 'top'; backgroundImageUrl?: string; completedAt?: string; isPublic?: boolean; hasUnreadMentions?: boolean; labels: Label[]; roles: ProfileRole[]; milestone?: Milestone | null; checklist?: string; comments?: number; attachments?: number; members: Member[] };
type Column = { id: EntityId; title: string; cards: Card[] };
type View = 'home' | 'board';
type PersistenceStatus = 'connecting' | 'connected';
type BoardBackgroundFit = 'cover' | 'contain' | 'fill';
type BoardBackgroundPosition = 'center' | 'top' | 'bottom';
type ApiMember = { id: string; username: string; avatar_url?: string | null };
type ApiBoard = { id: string; workspace_id: string; title: string; background_image_url: string | null; background_fit: BoardBackgroundFit; background_position: BoardBackgroundPosition; visibility: 'public' | 'private' | 'workspace'; can_edit: boolean; labels: Label[]; milestones: Milestone[]; members: ApiMember[]; lists: { id: string; title: string; grid_column: number; grid_row: number; cards: { id: string; title: string; description: string; priority: number; last_activity_at: string | null; is_public: boolean; background_image_url: string | null; due_at: string | null; cover_attachment_id: string | null; cover_url: string | null; cover_mode: 'full' | 'top'; completed_at: string | null; checklist_total: number; checklist_completed: number; comment_count: number; attachment_count: number; has_unread_mentions: boolean; labels: Label[]; roles?: ProfileRole[]; milestone?: Milestone | null; assignees: ApiMember[] }[] }[] };
type DragState = { cardId: EntityId; sourceListId: EntityId };
type DragDropTarget = { listId: EntityId; beforeCardId: EntityId | null };
type ChecklistItem = { id: EntityId; title: string; is_completed: boolean; description: string; attachments: Attachment[] };
type Checklist = { id: string; title: string; items: ChecklistItem[] };
type Comment = { id: EntityId; body: string; author_id?: string | null; author_name: string; author_avatar_url?: string | null; parent_comment_id?: string | null; created_at?: string; edited_at?: string | null };
type Attachment = { id: string; original_name: string; media_type: string; byte_size: number; url: string };
type Activity = { id: string; action: string; detail: string; actor_name: string | null; created_at: string };
type CardDetail = { checklists: Checklist[]; comments: Comment[]; attachments: Attachment[]; activity: Activity[]; cover_attachment_id: string | null; cover_mode: 'full' | 'top'; background_image_url: string | null; unread_mention_source_ids: string[]; watching: boolean };
type CardNotification = { id: string; card_id: string; board_id: string; card_title: string; board_title: string; actor_name: string | null; action: string; detail: string; is_read: boolean; created_at: string };
type AuthAccount = { user: { id: string; username: string; avatar_url: string | null; is_system_owner: boolean } };
type AuthState = 'checking' | 'signed-out' | 'signed-in' | 'public';
type Workspace = { id: string; name: string; background_image_url?: string | null; can_manage?: boolean };
type BoardSummary = { id: string; title: string; visibility: string };
type FilterMode = 'all' | 'assigned' | 'my_roles' | 'due' | 'overdue';
type CardSort = 'manual' | 'priority' | 'activity';
type BoardViewMode = 'standard' | 'freeform';
type BoardContentMode = 'columns' | 'members';
type MemberDragState = { card: Card; sourceMemberId: string | null };
type FreeformPosition = { x: number; y: number };
type BoardLayout = { view_mode: BoardViewMode; positions: { list_id: string; x: number; y: number }[] };
type FreeformCardPosition = { card_id: string; x: number; y: number };
type FreeformStroke = { id?: string; author_id?: string; points: DiagramPoint[]; color: string; width: number };
type FreeformDrawing = { strokes: FreeformStroke[] };
type FreeformLiveCursor = { user_id: string; username: string; avatar_url?: string | null; x: number; y: number };
type FreeformPing = FreeformLiveCursor & { id: string; expires_in_ms: number };
type FreeformLive = { cursors: FreeformLiveCursor[]; pings: FreeformPing[] };
type FreeformLiveSocketEvent =
  | ({ type: 'cursor' } & FreeformLiveCursor)
  | ({ type: 'ping' } & FreeformPing);
type FreeformContextMenu = { x: number; y: number; position: FreeformPosition };
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
type ViewportRect = { left: number; top: number; width: number; height: number };
type CardMoveMotion = { key: number; cardId: string; title: string; from: ViewportRect; to: ViewportRect };
type CardDragPreview = { card: Card; x: number; y: number; width: number; height: number };
type PointerCardDrag = { card: Card; sourceListId: EntityId; startX: number; startY: number; width: number; height: number; active: boolean };

// Empty in local development and production: requests stay on the current origin.
// Vite forwards /v1 to Rust locally; nginx does the same after deployment.
const API_URL = process.env.NEXT_PUBLIC_FLOWBOARD_API_URL ?? '';
const browserFetch = globalThis.fetch.bind(globalThis);

function assetUrl(url: string | null | undefined) {
  if (!url) return '';
  return /^https?:\/\//i.test(url) ? url : `${API_URL}${url}`;
}

function homeGreetingForLocalTime(date = new Date()) {
  const hour = date.getHours();
  if (hour >= 5 && hour < 12) return 'Доброе утро';
  if (hour >= 12 && hour < 18) return 'Добрый день';
  if (hour >= 18 && hour < 23) return 'Добрый вечер';
  return 'Рад видеть вас в столь поздний час';
}

type StarfallParticle = { x: number; y: number; vx: number; vy: number; size: number; alpha: number; spin: number; rotation: number };

function AmbientStarfall() {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const context = canvas?.getContext('2d');
    if (!canvas || !context) return;
    let width = 0;
    let height = 0;
    let frameId = 0;
    let lastTime = performance.now();
    let lastPaint = 0;
    let particles: StarfallParticle[] = [];
    let coinReady = false;
    const random = (min: number, max: number) => min + Math.random() * (max - min);
    const createParticle = (): StarfallParticle => {
      const speed = random(70, 185);
      const angle = random(108, 162) * Math.PI / 180;
      const fromTop = Math.random() < .46;
      return {
        x: fromTop ? random(-width * .05, width + 280) : random(width + 20, width + 280),
        y: fromTop ? random(-height * .35, -16) : random(-height * .35, height * .65),
        vx: Math.cos(angle) * speed,
        vy: Math.sin(angle) * speed,
        size: random(14, 33),
        alpha: random(.3, .72),
        spin: random(-2.8, 2.8),
        rotation: random(0, Math.PI * 2),
      };
    };
    const resize = () => {
      const pixelRatio = Math.min(window.devicePixelRatio || 1, 1.5);
      width = window.innerWidth;
      height = window.innerHeight;
      canvas.width = Math.round(width * pixelRatio);
      canvas.height = Math.round(height * pixelRatio);
      canvas.style.width = `${width}px`;
      canvas.style.height = `${height}px`;
      context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
      particles = Array.from({ length: 48 }, createParticle);
    };
    const coin = new Image();
    const drawParticle = (particle: StarfallParticle, time: number) => {
      if (!coinReady) return;
      context.save();
      context.globalAlpha = particle.alpha;
      context.translate(particle.x, particle.y);
      context.rotate(particle.rotation + particle.spin * time);
      context.drawImage(coin, -particle.size / 2, -particle.size / 2, particle.size, particle.size);
      context.restore();
    };
    const paint = (now: number, delta: number) => {
      context.clearRect(0, 0, width, height);
      const padding = Math.max(width, height) * .2;
      particles.forEach((particle, index) => {
        particle.x += particle.vx * delta;
        particle.y += particle.vy * delta;
        if (particle.y > height + padding || particle.x < -padding) particles[index] = createParticle();
        drawParticle(particles[index], now / 1000);
      });
    };
    const render = (now: number) => {
      const delta = Math.min((now - lastTime) / 1000, .05);
      lastTime = now;
      if (now - lastPaint >= 33) { paint(now, delta); lastPaint = now; }
      frameId = window.requestAnimationFrame(render);
    };

    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    coin.onload = () => { coinReady = true; if (reducedMotion) paint(0, 0); };
    coin.src = '/flowboard-coin.png';
    resize();
    if (reducedMotion) paint(0, 0); else frameId = window.requestAnimationFrame(render);
    window.addEventListener('resize', resize);
    return () => { window.cancelAnimationFrame(frameId); window.removeEventListener('resize', resize); };
  }, []);

  return <div className="default-board-ambient" aria-hidden="true"><canvas ref={canvasRef} /></div>;
}

function freeformLiveSocketUrl(boardId: string) {
  const base = API_URL || window.location.origin;
  const url = new URL(`/v1/boards/${boardId}/freeform/live/ws`, base);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.toString();
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

function PrioritySignal({ priority, wave = false }: { priority?: number; wave?: boolean }) {
  const level = Math.max(0, Math.min(5, priority ?? 0));
  if (!level) return null;
  return <span className={`priority-signal level-${level}${wave ? ' priority-wave' : ''}`} title={`Приоритет ${level} из 5`} aria-label={`Приоритет ${level} из 5`}>{[1, 2, 3, 4, 5].map((bar) => <i className={bar <= level ? 'active' : ''} key={bar} />)}</span>;
}

const roleShapes: RoleShape[] = ['circle', 'square', 'diamond', 'star', 'triangle', 'hexagon', 'bolt', 'flag', 'check', 'cross'];

function ShapeIcon({ shape = 'circle', color, size = 12 }: { shape?: RoleShape; color?: string; size?: number }) {
  const common = { fill: 'currentColor' };
  const glyphs: Record<RoleShape, ReactNode> = {
    circle: <circle cx="8" cy="8" r="5.25" {...common} />,
    square: <rect x="3" y="3" width="10" height="10" rx="1.4" {...common} />,
    diamond: <path d="m8 1.7 6.3 6.3L8 14.3 1.7 8z" {...common} />,
    star: <path d="m8 1.15 1.92 4.1 4.48.55-3.31 3.05.88 4.42L8 11.13l-3.97 2.14.88-4.42-3.31-3.05 4.48-.55z" {...common} />,
    triangle: <path d="M8 2 14.2 13H1.8z" {...common} />,
    hexagon: <path d="m8 1.55 5.58 3.22v6.46L8 14.45l-5.58-3.22V4.77z" {...common} />,
    bolt: <path d="M9.15 1 2.8 8.35h4.05L6.2 15 13.2 6.9H9.1z" {...common} />,
    flag: <path d="M3 1.5h1.5v1h7.6l-1.2 2.7 1.2 2.8H4.5v6.5H3z" {...common} />,
    check: <path d="m2.1 8.2 3.5 3.5 8.3-8.25 1.05 1.05-9.35 9.35-4.55-4.55z" {...common} />,
    cross: <path d="m3.05 2 4.95 4.95L12.95 2 14 3.05 9.05 8 14 12.95 12.95 14 8 9.05 3.05 14 2 12.95 6.95 8 2 3.05z" {...common} />,
  };
  return <svg className="role-shape-icon" viewBox="0 0 16 16" width={size} height={size} style={color ? { color } : undefined} aria-hidden="true">{glyphs[shape] ?? glyphs.circle}</svg>;
}

function LabelChip({ label, asButton = false, onClick }: { label: Label; asButton?: boolean; onClick?: ReactMouseEventHandler<HTMLButtonElement> }) {
  const className = 'label custom-label label-with-shape';
  const content = <><ShapeIcon shape={label.icon_shape} color={label.icon_color ?? label.color} /><span>{label.name}</span></>;
  return asButton ? <button className={className} style={{ color: '#F7F8FC', backgroundColor: `${label.color}66` }} onClick={onClick}>{content}</button> : <span className={className} style={{ color: '#F7F8FC', backgroundColor: `${label.color}66` }}>{content}</span>;
}

function ProfileRoleChip({ role, compact = false }: { role: ProfileRole; compact?: boolean }) {
  return <span className={`label custom-label profile-role-chip ${compact ? 'compact' : ''}`} style={{ color: '#F7F8FC', backgroundColor: `${role.color}66` }}><ShapeIcon shape={role.icon_shape} color={role.icon_color ?? role.color} /><span>{role.name}</span></span>;
}

function ShapePicker({ value, onChange, label }: { value: RoleShape; onChange: (shape: RoleShape) => void; label: string }) {
  return <div className="shape-picker" role="group" aria-label={label}>{roleShapes.map((shape) => <button type="button" key={shape} className={shape === value ? 'selected' : ''} onClick={() => onChange(shape)} title={shape} aria-label={shape}><ShapeIcon shape={shape} /></button>)}</div>;
}

function ChipNamePreview({ value, onChange, color, iconColor, shape, placeholder, ariaLabel, maxLength = 80 }: { value: string; onChange: (value: string) => void; color: string; iconColor: string; shape: RoleShape; placeholder: string; ariaLabel: string; maxLength?: number }) {
  return <span className="chip-name-preview" style={{ backgroundColor: color }}><ShapeIcon shape={shape} color={iconColor} /><input autoFocus value={value} onChange={(event) => onChange(event.target.value)} maxLength={maxLength} placeholder={placeholder} aria-label={ariaLabel} /></span>;
}

function randomChipColor() {
  const channel = () => Math.floor(64 + Math.random() * 176).toString(16).padStart(2, '0');
  return `#${channel()}${channel()}${channel()}`.toUpperCase();
}

function CardMetaIcon({ type }: { type: 'comments' | 'checklist' | 'attachments' }) {
  const paths = {
    comments: 'M0 3.125A2.625 2.625 0 0 1 2.625.5h10.75A2.625 2.625 0 0 1 16 3.125v8.25A2.625 2.625 0 0 1 13.375 14H4.449l-3.327 1.901A.75.75 0 0 1 0 15.25zM2.625 2C2.004 2 1.5 2.504 1.5 3.125v10.833L4.05 12.5h9.325c.621 0 1.125-.504 1.125-1.125v-8.25C14.5 2.504 13.996 2 13.375 2zM12 6.5H4V5h8zm-3 3H4V8h5z',
    checklist: 'M1 3a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H3a2 2 0 0 1-2-2zm2-.5a.5.5 0 0 0-.5.5v10a.5.5 0 0 0 .5.5h10a.5.5 0 0 0 .5-.5V3a.5.5 0 0 0-.5-.5zm9.326 2.98-5 6a.75.75 0 0 1-1.152 0l-2.5-3 1.152-.96L6.75 9.828l4.424-5.308z',
    attachments: 'M15 3.5H1V2h14zm0 5.25H1v-1.5h14zM8 14H1v-1.5h7z',
  };
  return <svg className="card-meta-icon" viewBox="0 0 16 16" aria-hidden="true"><path fill="currentColor" fillRule="evenodd" clipRule="evenodd" d={paths[type]} /></svg>;
}

function BoardToolbarIcon({ type }: { type: 'filter' | 'labels' | 'milestones' | 'archive' | 'team' }) {
  const paths = {
    filter: ['M11 12v1.5H5V12zm2-4.75v1.5H3v-1.5zm2-4.75V4H1V2.5z'],
    labels: ['M11 4a1 1 0 1 0 0 2 1 1 0 0 0 0-2', 'M9.286 1a3.25 3.25 0 0 0-2.299.952L1.604 7.336a2 2 0 0 0 0 2.828l4.232 4.232a2 2 0 0 0 2.828 0l5.384-5.383A3.25 3.25 0 0 0 15 6.714V3a2 2 0 0 0-2-2zM8.048 3.013A1.75 1.75 0 0 1 9.286 2.5H13a.5.5 0 0 1 .5.5v3.714c0 .465-.184.91-.513 1.238l-5.383 5.384a.5.5 0 0 1-.708 0L2.664 9.104a.5.5 0 0 1 0-.708z'],
    milestones: ['M8 1.25 14.75 8 8 14.75 1.25 8z'],
    archive: ['M1 1h14v5h-1v7a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6H1zm2.5 5v7a.5.5 0 0 0 .5.5h8a.5.5 0 0 0 .5-.5V6zm10-1.5h-11v-2h11zm-3 4.5h-5V7.5h5z'],
    team: ['M1 12.75A3.75 3.75 0 0 1 4.75 9H9v1.5H4.75a2.25 2.25 0 0 0-2.25 2.25V15H1zM9.5 4a2.5 2.5 0 1 0-5 0 2.5 2.5 0 0 0 0-5M11 4a4 4 0 1 1-8 0 4 4 0 0 1 8 0m2.75 6v2.25H16v1.5h-2.25V16h-1.5v-2.25H10v-1.5h2.25V10z'],
  };
  return <svg className="board-toolbar-icon" fill="none" viewBox="0 0 16 16" aria-hidden="true">{paths[type].map((path, index) => <path key={path} fill="currentColor" fillRule={type === 'labels' && index === 1 ? 'evenodd' : undefined} clipRule={type === 'labels' && index === 1 ? 'evenodd' : undefined} d={path} />)}</svg>;
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
  const [homeGreeting, setHomeGreeting] = useState('Добрый день');
  const [isComposerOpen, setComposerOpen] = useState<EntityId | null>(null);
  const [draft, setDraft] = useState('');
  const [selected, setSelected] = useState<Card | null>(null);
  const [toast, setToast] = useState('');
  const [query, setQuery] = useState('');
  const [filterMode, setFilterMode] = useState<FilterMode>('all');
  const [milestoneFilterId, setMilestoneFilterId] = useState<string | null>(null);
  const [cardSort, setCardSort] = useState<CardSort>('manual');
  const [boardContentMode, setBoardContentMode] = useState<BoardContentMode>('columns');
  const [priorityMotionKey, setPriorityMotionKey] = useState(0);
  const [cardMoveMotion, setCardMoveMotion] = useState<CardMoveMotion | null>(null);
  const [cardDragPreview, setCardDragPreview] = useState<CardDragPreview | null>(null);
  const [memberDrag, setMemberDrag] = useState<MemberDragState | null>(null);
  const [labelsCollapsed, setLabelsCollapsed] = useState(false);
  const [isFilterOpen, setFilterOpen] = useState(false);
  const [isBoardLabelsOpen, setBoardLabelsOpen] = useState(false);
  const [isMilestonesOpen, setMilestonesOpen] = useState(false);
  const [isMembersPopoverOpen, setMembersPopoverOpen] = useState(false);
  const [nextCardId, setNextCardId] = useState(100);
  const [checklists, setChecklists] = useState<Checklist[]>([]);
  const [collapsedChecklistIds, setCollapsedChecklistIds] = useState<string[]>([]);
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
  const [sidebarPanel, setSidebarPanel] = useState<'labels' | 'roles' | 'due' | 'assignees' | 'background' | 'public-visibility' | null>(null);
  const [existingLabelsOnly, setExistingLabelsOnly] = useState(false);
  const [isUploadingCardBackground, setUploadingCardBackground] = useState(false);
  const [boardLabels, setBoardLabels] = useState<Label[]>([]);
  const [profileRoles, setProfileRoles] = useState<ProfileRole[]>([]);
  const [myProfileRoleIds, setMyProfileRoleIds] = useState<string[]>([]);
  const [isProfileRolePickerOpen, setProfileRolePickerOpen] = useState(false);
  const [newProfileRoleName, setNewProfileRoleName] = useState('');
  const [newProfileRoleColor, setNewProfileRoleColor] = useState('#6B7CFF');
  const [newProfileRoleShape, setNewProfileRoleShape] = useState<RoleShape>('circle');
  const [newProfileRoleIconColor, setNewProfileRoleIconColor] = useState('#FFFFFF');
  const [editingProfileRole, setEditingProfileRole] = useState<ProfileRole | null>(null);
  const [isSavingProfileRole, setSavingProfileRole] = useState(false);
  const [milestones, setMilestones] = useState<Milestone[]>([]);
  const [milestoneNameDraft, setMilestoneNameDraft] = useState('');
  const [milestoneColorDraft, setMilestoneColorDraft] = useState('#6ea8fe');
  const [isSavingMilestone, setSavingMilestone] = useState(false);
  const [isCardMilestoneOpen, setCardMilestoneOpen] = useState(false);
  const [workspaceMembers, setWorkspaceMembers] = useState<Member[]>([]);
  const [workspaceId, setWorkspaceId] = useState<string | null>(null);
  const [workspaceName, setWorkspaceName] = useState('');
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [workspaceBoards, setWorkspaceBoards] = useState<Record<string, BoardSummary[]>>({});
  const [workspaceBackgroundEditorId, setWorkspaceBackgroundEditorId] = useState<string | null>(null);
  const [workspaceBackgroundDraft, setWorkspaceBackgroundDraft] = useState('');
  const [isSavingWorkspaceBackground, setSavingWorkspaceBackground] = useState(false);
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
  const [notifications, setNotifications] = useState<CardNotification[]>([]);
  const [isNotificationsOpen, setNotificationsOpen] = useState(false);
  const [isNotificationsLoading, setNotificationsLoading] = useState(false);
  const [pendingNotificationCardId, setPendingNotificationCardId] = useState<string | null>(null);
  const [availableAccounts, setAvailableAccounts] = useState<ApiMember[]>([]);
  const [selectedAccountId, setSelectedAccountId] = useState('');
  const [accountSearch, setAccountSearch] = useState('');
  const [isSavingMember, setSavingMember] = useState(false);
  const [newLabelName, setNewLabelName] = useState('');
  const [newLabelColor, setNewLabelColor] = useState('#6B7CFF');
  const [newLabelShape, setNewLabelShape] = useState<RoleShape>('circle');
  const [newLabelIconColor, setNewLabelIconColor] = useState('#FFFFFF');
  const [isColorShakeActive, setColorShakeActive] = useState(false);
  const [isSavingLabel, setSavingLabel] = useState(false);
  const [editingBoardLabel, setEditingBoardLabel] = useState<Label | null>(null);
  const [boardLabelNameDraft, setBoardLabelNameDraft] = useState('');
  const [boardLabelColorDraft, setBoardLabelColorDraft] = useState('#6B7CFF');
  const [boardLabelShapeDraft, setBoardLabelShapeDraft] = useState<RoleShape>('circle');
  const [boardLabelIconColorDraft, setBoardLabelIconColorDraft] = useState('#FFFFFF');
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
  const [columnDropBeforeId, setColumnDropBeforeId] = useState<EntityId | null>(null);
  const [boardViewMode, setBoardViewMode] = useState<BoardViewMode>('standard');
  const [freeformLayout, setFreeformLayout] = useState<Record<string, FreeformPosition>>({});
  const [freeformCardLayout, setFreeformCardLayout] = useState<Record<string, FreeformPosition>>({});
  const [freeformLive, setFreeformLive] = useState<FreeformLive>({ cursors: [], pings: [] });
  const [freeformContextMenu, setFreeformContextMenu] = useState<FreeformContextMenu | null>(null);
  const [freeformDrawing, setFreeformDrawing] = useState<FreeformDrawing>({ strokes: [] });
  const [isFreeformDrawing, setFreeformDrawingMode] = useState(false);
  const [isFreeformErasing, setFreeformErasing] = useState(false);
  const [freeformInkColor, setFreeformInkColor] = useState('#8ab4ff');
  const [freeformInkWidth, setFreeformInkWidth] = useState(4);
  const [freeformZoom, setFreeformZoom] = useState(1);
  const [freeformViewport, setFreeformViewport] = useState({ x: 0, y: 0, width: 0, height: 0 });
  const [isBoardPanning, setBoardPanning] = useState(false);
  const [columnMenuId, setColumnMenuId] = useState<EntityId | null>(null);
  const [editingColumnId, setEditingColumnId] = useState<EntityId | null>(null);
  const [columnTitleDraft, setColumnTitleDraft] = useState('');
  const [isSavingColumn, setSavingColumn] = useState(false);
  const [cardDetailRevision, setCardDetailRevision] = useState(0);
  const [unreadMentionSourceIds, setUnreadMentionSourceIds] = useState<string[]>([]);
  const [isWatchingCard, setWatchingCard] = useState(false);
  const didDragRef = useRef(false);
  const dragScrollFrameRef = useRef<number | null>(null);
  const previousBackgroundDraftRef = useRef('');
  const previousWorkspaceBackgroundDraftRef = useRef('');
  const boardBackgroundDisplayRef = useRef<{ fit: BoardBackgroundFit; position: BoardBackgroundPosition }>({ fit: 'cover', position: 'center' });
  const dragScrollTargetRef = useRef<{ element: HTMLDivElement; direction: -1 | 1 } | null>(null);
  const boardRef = useRef<HTMLElement | null>(null);
  const boardDragScrollFrameRef = useRef<number | null>(null);
  const boardDragScrollDirectionRef = useRef<-1 | 1 | null>(null);
  const boardPanRef = useRef<{ pointerId: number; startX: number; startY: number; startScrollLeft: number; startScrollTop: number; moved: boolean } | null>(null);
  const freeformDragRef = useRef<{ pointerId: number; columnId: EntityId; startX: number; startY: number; origin: FreeformPosition } | null>(null);
  const freeformCanvasRef = useRef<HTMLDivElement | null>(null);
  const freeformInkRef = useRef<{ pointerId: number; erasing: boolean } | null>(null);
  const freeformLiveSentAtRef = useRef(0);
  const freeformLiveSocketRef = useRef<WebSocket | null>(null);
  const freeformLiveExpiryTimersRef = useRef<Map<string, number>>(new Map());
  const freeformDrawingRef = useRef<FreeformDrawing>({ strokes: [] });
  const freeformDrawingDirtyRef = useRef(false);
  const freeformEraseForeignRef = useRef(false);
  const freeformDrawingSaveTimerRef = useRef<number | null>(null);
  const pointerCardDragRef = useRef<PointerCardDrag | null>(null);
  const pointerCardDropRef = useRef<{ listId: EntityId; beforeCardId?: EntityId } | null>(null);
  const cardDragPreviewElementRef = useRef<HTMLDivElement | null>(null);
  const diagramCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const importFileRef = useRef<HTMLInputElement | null>(null);
  const boardBackgroundFileRef = useRef<HTMLInputElement | null>(null);
  const workspaceBackgroundFileRef = useRef<HTMLInputElement | null>(null);
  const cardBackgroundFileRef = useRef<HTMLInputElement | null>(null);
  const isDrawingRef = useRef(false);
  const diagramStartRef = useRef<DiagramPoint | null>(null);
  const diagramInteractionRef = useRef<DiagramInteraction | null>(null);
  const selectedCardId = selected?.id;
  const isPublicViewer = authState === 'public' || !canEditBoard;
  const unreadNotificationCount = notifications.filter((notification) => !notification.is_read).length;
  const dueDays = useMemo(() => calendarDays(dueCursor), [dueCursor]);
  const currentMember = account ? memberFromApi({ id: account.user.id, username: account.user.username, avatar_url: account.user.avatar_url }) : { id: '', initials: '—', color: 'violet', name: 'Пользователь' };
  const boardBackgroundStyle = view === 'board' && boardBackgroundUrl ? { backgroundImage: `linear-gradient(rgb(18 17 16 / 48%), rgb(18 17 16 / 72%)), url("${assetUrl(boardBackgroundUrl)}")`, backgroundSize: boardBackgroundFit === 'fill' ? '100% 100%' : boardBackgroundFit, backgroundPosition: boardBackgroundPosition === 'top' ? 'center top' : boardBackgroundPosition === 'bottom' ? 'center bottom' : 'center', backgroundRepeat: 'no-repeat' } : undefined;

  const visibleColumns = useMemo(() => columns.map((column) => {
    const cards = column.cards.filter((card) => {
      if (!card.title.toLowerCase().includes(query.toLowerCase())) return false;
      if (milestoneFilterId && card.milestone?.id !== milestoneFilterId) return false;
      if (filterMode === 'assigned') return card.members.some((member) => member.id === currentMember.id);
      if (filterMode === 'my_roles') return card.roles.some((role) => myProfileRoleIds.includes(role.id));
      if (filterMode === 'due') return Boolean(card.dueAt);
      if (filterMode === 'overdue') return Boolean(card.dueAt && new Date(card.dueAt).getTime() < Date.now());
      return true;
    });
    // Manual order keeps completed work out of the active queue. The actual
    // list order is updated at completion time, this is only a safe fallback
    // for data imported from an older board.
    if (cardSort === 'manual') return { ...column, cards: [...cards.filter((card) => !card.completedAt), ...cards.filter((card) => card.completedAt)] };
    const activityTime = (card: Card) => card.lastActivityAt ? new Date(card.lastActivityAt).getTime() || 0 : 0;
    return { ...column, cards: [...cards].sort((left, right) => cardSort === 'priority'
      ? (right.priority ?? 0) - (left.priority ?? 0)
      : activityTime(right) - activityTime(left)) };
  }), [cardSort, columns, currentMember.id, filterMode, milestoneFilterId, myProfileRoleIds, query]);

  const memberLanes = useMemo(() => {
    const cards = visibleColumns.flatMap((column) => column.cards);
    return [
      { id: null as string | null, member: null as Member | null, cards: cards.filter((card) => card.members.length === 0) },
      ...workspaceMembers.map((member) => ({ id: String(member.id), member, cards: cards.filter((card) => card.members.some((assignee) => String(assignee.id) === String(member.id))) })),
    ];
  }, [visibleColumns, workspaceMembers]);

  const renderedColumns = useMemo(() => boardViewMode === 'freeform'
    ? visibleColumns.map((column) => ({ ...column, cards: column.cards.filter((card) => !freeformCardLayout[String(card.id)]) }))
    : visibleColumns, [boardViewMode, freeformCardLayout, visibleColumns]);
  const freeformDetachedCards = useMemo(() => visibleColumns.flatMap((column) => column.cards
    .filter((card) => Boolean(freeformCardLayout[String(card.id)]))
    .map((card) => ({ card, listId: column.id, position: freeformCardLayout[String(card.id)] }))), [freeformCardLayout, visibleColumns]);

  const freeformCanvasSize = useMemo(() => {
    const positions = renderedColumns.map((column, index) => freeformLayout[String(column.id)] ?? { x: index * 336, y: 0 });
    const cardPositions = freeformDetachedCards.map(({ position }) => position);
    const inkPoints = freeformDrawing.strokes.flatMap((stroke) => stroke.points);
    return {
      width: Math.max(5_200, ...positions.map((position) => position.x + 354), ...cardPositions.map((position) => position.x + 330), ...inkPoints.map((point) => point.x + 80)),
      height: Math.max(3_200, ...positions.map((position) => position.y + 860), ...cardPositions.map((position) => position.y + 260), ...inkPoints.map((point) => point.y + 80)),
    };
  }, [freeformDetachedCards, freeformDrawing, freeformLayout, renderedColumns]);

  useEffect(() => {
    const syncGreeting = () => setHomeGreeting(homeGreetingForLocalTime());
    syncGreeting();
    const timer = window.setInterval(syncGreeting, 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!dragging) return;
    const followNativeDrag = (event: DragEvent) => {
      if (event.clientX <= 0 || event.clientY <= 0) return;
      setCardDragPreview((current) => current ? { ...current, x: event.clientX, y: event.clientY } : current);
    };
    window.addEventListener('dragover', followNativeDrag);
    return () => window.removeEventListener('dragover', followNativeDrag);
  }, [dragging]);

  useEffect(() => {
    const movePointerCard = (event: PointerEvent) => {
      const pointerDrag = pointerCardDragRef.current;
      if (!pointerDrag || boardViewMode !== 'standard') return;
      if (!pointerDrag.active) {
        if (Math.hypot(event.clientX - pointerDrag.startX, event.clientY - pointerDrag.startY) < 6) return;
        pointerDrag.active = true;
        setDragging({ cardId: pointerDrag.card.id, sourceListId: pointerDrag.sourceListId });
        setCardDragPreview({ card: pointerDrag.card, x: event.clientX, y: event.clientY, width: pointerDrag.width, height: pointerDrag.height });
      }
      const preview = cardDragPreviewElementRef.current;
      if (preview) { preview.style.left = `${event.clientX - 28}px`; preview.style.top = `${event.clientY - 20}px`; }
      const target = document.elementFromPoint(event.clientX, event.clientY);
      const targetColumn = target instanceof Element ? target.closest<HTMLElement>('[data-list-id]') : null;
      const targetList = columns.find((column) => String(column.id) === targetColumn?.dataset.listId);
      if (!targetList || !targetColumn) {
        if (pointerCardDropRef.current) { pointerCardDropRef.current = null; setDragOverListId(null); setDragDropTarget(null); }
        return;
      }
      let beforeCardId: EntityId | undefined;
      const visibleCards = Array.from(targetColumn.querySelectorAll<HTMLElement>('[data-card-id]')).filter((element) => element.dataset.cardId !== String(pointerDrag.card.id));
      for (const element of visibleCards) {
        const bounds = element.getBoundingClientRect();
        if (event.clientY <= bounds.top + bounds.height / 2) { beforeCardId = targetList.cards.find((card) => String(card.id) === element.dataset.cardId)?.id; break; }
      }
      const previousTarget = pointerCardDropRef.current;
      if (!previousTarget || previousTarget.listId !== targetList.id || previousTarget.beforeCardId !== beforeCardId) {
        pointerCardDropRef.current = { listId: targetList.id, beforeCardId };
        setDragOverListId(targetList.id);
        setDragDropTarget({ listId: targetList.id, beforeCardId: beforeCardId ?? null });
      }
      event.preventDefault();
    };
    const endPointerCard = () => {
      const pointerDrag = pointerCardDragRef.current;
      if (!pointerDrag) return;
      pointerCardDragRef.current = null;
      if (!pointerDrag.active) return;
      didDragRef.current = true;
      const target = pointerCardDropRef.current;
      pointerCardDropRef.current = null;
      if (target && (target.listId !== pointerDrag.sourceListId || target.beforeCardId !== pointerDrag.card.id)) moveCard(pointerDrag.card.id, pointerDrag.sourceListId, target.listId, target.beforeCardId);
      else clearDragState();
      window.setTimeout(() => { didDragRef.current = false; }, 0);
    };
    window.addEventListener('pointermove', movePointerCard);
    window.addEventListener('pointerup', endPointerCard);
    window.addEventListener('pointercancel', endPointerCard);
    return () => { window.removeEventListener('pointermove', movePointerCard); window.removeEventListener('pointerup', endPointerCard); window.removeEventListener('pointercancel', endPointerCard); };
  }, [boardViewMode, columns]);

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
        const boardEntries = await Promise.all(spaces.map(async (space) => {
          const response = await fetch(`${API_URL}/v1/workspaces/${space.id}/boards`);
          if (!response.ok) throw new Error('boards could not be loaded');
          return [space.id, await response.json() as BoardSummary[]] as const;
        }));
        const boardsByWorkspace = Object.fromEntries(boardEntries) as Record<string, BoardSummary[]>;
        setWorkspaceBoards(boardsByWorkspace);
        const rememberedWorkspaceId = typeof window === 'undefined' ? null : window.localStorage.getItem('flowboard.workspace_id');
        const workspace = spaces.find((item) => item.id === rememberedWorkspaceId) ?? spaces[0];
        if (!workspace) { setWorkspaceId(null); setWorkspaceName(''); setBoards([]); setBoardId(null); setColumns([]); setView('home'); setPersistence('connected'); return; }
        setWorkspaceId(workspace.id); setWorkspaceName(workspace.name);
        setBoards(boardsByWorkspace[workspace.id] ?? []);
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
    if (authState !== 'signed-in') { setNotifications([]); setNotificationsOpen(false); return; }
    void loadNotifications();
    const intervalId = window.setInterval(() => void loadNotifications(), 20_000);
    return () => window.clearInterval(intervalId);
  }, [authState]);

  useEffect(() => {
    if (authState !== 'signed-in') { setProfileRoles([]); setMyProfileRoleIds([]); return; }
    let active = true;
    void fetch(`${API_URL}/v1/profile-roles`)
      .then(async (response) => { if (!response.ok) throw new Error('profile roles failed'); return response.json() as Promise<{ roles: ProfileRole[]; assigned_role_ids: string[] }>; })
      .then((payload) => { if (active) { setProfileRoles(payload.roles); setMyProfileRoleIds(payload.assigned_role_ids); } })
      .catch(() => { if (active) { setProfileRoles([]); setMyProfileRoleIds([]); } });
    return () => { active = false; };
  }, [authState, account?.user.id]);

  useEffect(() => {
    if (!pendingNotificationCardId) return;
    const card = columns.flatMap((column) => column.cards).find((item) => item.id === pendingNotificationCardId);
    if (!card) return;
    setPendingNotificationCardId(null);
    openCard(card);
  }, [columns, pendingNotificationCardId]);

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
        setWatchingCard(detail.watching);
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
          if (authState === 'signed-in') {
            void fetch(`${API_URL}/v1/boards/${boardId}/layout`)
              .then(async (layoutResponse) => { if (!layoutResponse.ok) throw new Error('layout refresh failed'); return layoutResponse.json() as Promise<BoardLayout>; })
              .then((layout) => {
                setFreeformLayout(Object.fromEntries(layout.positions.map((position) => [position.list_id, { x: position.x, y: position.y }])));
              })
              .catch(() => undefined);
            void fetch(`${API_URL}/v1/boards/${boardId}/freeform/cards`)
              .then(async (positionsResponse) => { if (!positionsResponse.ok) throw new Error('freeform cards refresh failed'); return positionsResponse.json() as Promise<FreeformCardPosition[]>; })
              .then((positions) => setFreeformCardLayout(Object.fromEntries(positions.map((position) => [position.card_id, { x: position.x, y: position.y }]))))
              .catch(() => undefined);
            if (!freeformInkRef.current && !freeformDrawingDirtyRef.current) {
              void fetch(`${API_URL}/v1/boards/${boardId}/freeform/drawing`)
                .then(async (drawingResponse) => { if (!drawingResponse.ok) throw new Error('drawing refresh failed'); return drawingResponse.json() as Promise<{ document: FreeformDrawing }>; })
                .then((drawing) => replaceFreeformDrawing({ strokes: Array.isArray(drawing.document?.strokes) ? drawing.document.strokes : [] }))
                .catch(() => undefined);
            }
          }
          // The board payload only has comment counters. Reload an open card as
          // well, so Discord/API comments appear without closing the modal.
          if (typeof selectedCardId === 'string') setCardDetailRevision((current) => current + 1);
          if (authState === 'signed-in') void loadNotifications();
        }).catch(() => undefined);
      }, 180);
    };
    stream.addEventListener('refresh', refresh);
    stream.addEventListener('access-revoked', () => { stream.close(); setSelected(null); setView('home'); showToast('Доступ к пространству отозван'); });
    return () => { window.clearTimeout(refreshTimer); stream.close(); };
  }, [authState, boardId, persistence, isPublicViewer, selectedCardId]);

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
    if (!isBoardMenuOpen && !isFilterOpen && !isBoardLabelsOpen && !isMilestonesOpen && !isMembersPopoverOpen && !isCardMilestoneOpen && !isNotificationsOpen && !sidebarPanel && !columnMenuId) return;
    const closePopovers = (event: PointerEvent) => {
      if (!(event.target instanceof Element)) return;
      if (isBoardMenuOpen && !event.target.closest('.board-menu-control')) setBoardMenuOpen(false);
      if (isFilterOpen && !event.target.closest('.filter-control')) setFilterOpen(false);
      if (isBoardLabelsOpen && !event.target.closest('.board-labels-control')) { setBoardLabelsOpen(false); setEditingBoardLabel(null); }
      if (isMilestonesOpen && !event.target.closest('.board-milestones-control')) setMilestonesOpen(false);
      if (isMembersPopoverOpen && !event.target.closest('.board-members-control')) setMembersPopoverOpen(false);
      if (isCardMilestoneOpen && !event.target.closest('.card-milestone-control')) setCardMilestoneOpen(false);
      if (isNotificationsOpen && !event.target.closest('.notifications-control')) setNotificationsOpen(false);
      if (columnMenuId && !event.target.closest('.column-actions')) setColumnMenuId(null);
      if (sidebarPanel && !event.target.closest('.property-popover, .quick-action, .member-plus, .label-plus')) setSidebarPanel(null);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      setBoardMenuOpen(false); setFilterOpen(false); setBoardLabelsOpen(false); setEditingBoardLabel(null); setMilestonesOpen(false); setMembersPopoverOpen(false); setCardMilestoneOpen(false); setNotificationsOpen(false); setColumnMenuId(null); setSidebarPanel(null);
    };
    window.addEventListener('pointerdown', closePopovers);
    window.addEventListener('keydown', closeOnEscape);
    return () => { window.removeEventListener('pointerdown', closePopovers); window.removeEventListener('keydown', closeOnEscape); };
  }, [columnMenuId, isBoardLabelsOpen, isBoardMenuOpen, isCardMilestoneOpen, isFilterOpen, isMembersPopoverOpen, isMilestonesOpen, isNotificationsOpen, sidebarPanel]);

  useEffect(() => {
    if (!cardContextMenu && !columnContextMenu && !freeformContextMenu) return;
    const close = (event?: PointerEvent) => { if (event?.target instanceof Element && event.target.closest('.freeform-context-menu')) return; setCardContextMenu(null); setColumnContextMenu(null); setFreeformContextMenu(null); };
    const closeOnEscape = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    window.addEventListener('pointerdown', close);
    window.addEventListener('keydown', closeOnEscape);
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('keydown', closeOnEscape); };
  }, [cardContextMenu, columnContextMenu, freeformContextMenu]);

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
    const previous = previousWorkspaceBackgroundDraftRef.current;
    previousWorkspaceBackgroundDraftRef.current = workspaceBackgroundDraft;
    if (workspaceBackgroundEditorId && previous.trim() && !workspaceBackgroundDraft.trim()) clearWorkspaceBackground();
  }, [workspaceBackgroundDraft, workspaceBackgroundEditorId]);

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
  function replaceFreeformDrawing(next: FreeformDrawing | ((current: FreeformDrawing) => FreeformDrawing)) {
    const resolved = typeof next === 'function' ? next(freeformDrawingRef.current) : next;
    freeformDrawingRef.current = resolved;
    setFreeformDrawing(resolved);
  }
  function saveFreeformDrawing(document: FreeformDrawing, eraseForeign: boolean) {
    if (!boardId || authState !== 'signed-in' || boardViewMode !== 'freeform') return;
    void fetch(`${API_URL}/v1/boards/${boardId}/freeform/drawing`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ document, erase_foreign: eraseForeign }) })
      .then((response) => { if (!response.ok) throw new Error('drawing save failed'); })
      .catch(() => {
        freeformDrawingDirtyRef.current = true;
        freeformEraseForeignRef.current ||= eraseForeign;
        window.setTimeout(() => replaceFreeformDrawing({ ...freeformDrawingRef.current, strokes: [...freeformDrawingRef.current.strokes] }), 1_000);
        showToast('Рисунок не сохранён: попробуйте ещё раз');
      });
  }
  function flushFreeformDrawing() {
    if (!freeformDrawingDirtyRef.current) return;
    if (freeformDrawingSaveTimerRef.current !== null) window.clearTimeout(freeformDrawingSaveTimerRef.current);
    freeformDrawingSaveTimerRef.current = null;
    freeformDrawingDirtyRef.current = false;
    const eraseForeign = freeformEraseForeignRef.current;
    freeformEraseForeignRef.current = false;
    saveFreeformDrawing(freeformDrawingRef.current, eraseForeign);
  }
  function triggerColorShake() {
    setColorShakeActive(false);
    window.requestAnimationFrame(() => {
      setColorShakeActive(true);
      window.setTimeout(() => setColorShakeActive(false), 520);
    });
  }
  function renderBoardLabelsControl() {
    return <div className="board-labels-control">
      <button className={`board-icon-button ${isBoardLabelsOpen ? 'active-filter' : ''}`} type="button" title="Метки проекта" aria-label="Метки проекта" aria-expanded={isBoardLabelsOpen} onClick={() => { setBoardLabelsOpen((current) => !current); setEditingBoardLabel(null); }}><BoardToolbarIcon type="labels" /></button>
      {isBoardLabelsOpen && <div className="board-labels-popover">
        <div className="popover-heading"><b>Метки проекта</b><button type="button" onClick={() => { setBoardLabelsOpen(false); setEditingBoardLabel(null); }} aria-label="Закрыть">×</button></div>
        <p className="board-labels-copy">Метки доступны только внутри этого проекта.</p>
        <div className="board-labels-list">{boardLabels.length ? boardLabels.map((label) => <article key={label.id}>
          {editingBoardLabel?.id === label.id ? <form className={`label-editor ${isColorShakeActive ? 'is-shaking' : ''}`} onSubmit={saveBoardLabel}>
            <label>Название<ChipNamePreview value={boardLabelNameDraft} onChange={setBoardLabelNameDraft} color={boardLabelColorDraft} iconColor={boardLabelIconColorDraft} shape={boardLabelShapeDraft} placeholder="Название метки" ariaLabel="Название метки" maxLength={60} /></label>
            <label>Цвет<input type="color" value={boardLabelColorDraft} onChange={(event) => setBoardLabelColorDraft(event.target.value)} aria-label="Цвет метки" /></label>
            <label className="label-editor-shape">Фигура<ShapePicker value={boardLabelShapeDraft} onChange={setBoardLabelShapeDraft} label="Фигура метки" /></label>
            <label>Цвет фигуры<input type="color" value={boardLabelIconColorDraft} onChange={(event) => setBoardLabelIconColorDraft(event.target.value)} aria-label="Цвет фигуры метки" /></label>
            <div className="label-editor-actions"><button type="button" className="shake-colors-button" onClick={() => { setBoardLabelColorDraft(randomChipColor()); setBoardLabelIconColorDraft(randomChipColor()); setBoardLabelShapeDraft(roleShapes[Math.floor(Math.random() * roleShapes.length)]); triggerColorShake(); }}>Взболтнуть</button><button type="submit" disabled={!boardLabelNameDraft.trim() || isSavingBoardLabel}>Сохранить</button><button type="button" className="text-action" onClick={() => setEditingBoardLabel(null)}>Отмена</button></div>
          </form> : <><span className="board-label-chip" style={{ backgroundColor: label.color }}><ShapeIcon shape={label.icon_shape} color={label.icon_color ?? '#fff'} /><span>{label.name}</span></span>{!isPublicViewer && <span><button type="button" className="text-action" onClick={() => beginBoardLabelEdit(label)}>Изменить</button><button type="button" className="text-action danger-text" onClick={() => removeBoardLabel(label)}>Удалить</button></span>}</>}</article>) : <p className="empty-comments">На этой доске пока нет меток.</p>}</div>
        {!isPublicViewer && <form className={`label-editor label-create-editor ${isColorShakeActive ? 'is-shaking' : ''}`} onSubmit={createLabel}>
          <label>Название<ChipNamePreview value={newLabelName} onChange={setNewLabelName} color={newLabelColor} iconColor={newLabelIconColor} shape={newLabelShape} placeholder="Новая метка" ariaLabel="Название новой метки" maxLength={60} /></label>
          <label>Цвет<input type="color" value={newLabelColor} onChange={(event) => setNewLabelColor(event.target.value)} aria-label="Цвет метки" /></label>
          <label className="label-editor-shape">Фигура<ShapePicker value={newLabelShape} onChange={setNewLabelShape} label="Фигура метки" /></label>
          <label>Цвет фигуры<input type="color" value={newLabelIconColor} onChange={(event) => setNewLabelIconColor(event.target.value)} aria-label="Цвет фигуры метки" /></label>
          <div className="label-editor-actions"><button type="button" className="shake-colors-button" onClick={() => { setNewLabelColor(randomChipColor()); setNewLabelIconColor(randomChipColor()); setNewLabelShape(roleShapes[Math.floor(Math.random() * roleShapes.length)]); triggerColorShake(); }}>Взболтнуть</button><button type="submit" disabled={!newLabelName.trim() || isSavingLabel}>{isSavingLabel ? 'Создаём…' : 'Создать метку'}</button></div>
        </form>}
      </div>}
    </div>;
  }
  function createMilestone(event: FormEvent) {
    event.preventDefault();
    const name = milestoneNameDraft.trim();
    if (!boardId || !name || isSavingMilestone) return;
    if (persistence !== 'connected') { setMilestones((current) => [...current, { id: `local-milestone-${Date.now()}`, name, description: '', color: milestoneColorDraft }]); setMilestoneNameDraft(''); return; }
    setSavingMilestone(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/milestones`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, color: milestoneColorDraft }) })
      .then(async (response) => { if (!response.ok) throw new Error('milestone save failed'); return response.json() as Promise<Milestone>; })
      .then((milestone) => { setMilestones((current) => [...current.filter((item) => item.id !== milestone.id), milestone].sort((left, right) => left.name.localeCompare(right.name, 'ru'))); setMilestoneNameDraft(''); showToast('Milestone создан'); })
      .catch(() => showToast('Не удалось создать milestone'))
      .finally(() => setSavingMilestone(false));
  }
  function deleteMilestone(milestone: Milestone) {
    setMilestones((current) => current.filter((item) => item.id !== milestone.id));
    if (milestoneFilterId === milestone.id) setMilestoneFilterId(null);
    const clearFromCards = (card: Card) => card.milestone?.id === milestone.id ? { ...card, milestone: null } : card;
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map(clearFromCards) })));
    setSelected((current) => current?.milestone?.id === milestone.id ? { ...current, milestone: null } : current);
    if (persistence === 'connected' && !milestone.id.startsWith('local-')) void fetch(`${API_URL}/v1/milestones/${milestone.id}`, { method: 'DELETE' }).then((response) => { if (!response.ok) throw new Error('milestone delete failed'); showToast('Milestone удалён'); }).catch(() => showToast('Не удалось удалить milestone'));
  }
  function replaceSelectedMilestone(milestone: Milestone | null) {
    if (!selected) return;
    updateSelectedCard({ milestone });
    setCardMilestoneOpen(false);
    if (persistence === 'connected' && typeof selected.id === 'string') void fetch(`${API_URL}/v1/cards/${selected.id}/milestone`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ milestone_id: milestone?.id ?? null }) })
      .then(async (response) => { if (!response.ok) throw new Error('milestone assignment failed'); return response.json() as Promise<Milestone | null>; })
      .then((saved) => updateSelectedCard({ milestone: saved }))
      .catch(() => showToast('Не удалось сохранить milestone'));
  }
  function renderMilestonesControl() {
    const usage = (milestoneId: string) => columns.reduce((count, column) => count + column.cards.filter((card) => card.milestone?.id === milestoneId).length, 0);
    return <div className="board-milestones-control"><button className={`board-icon-button ${isMilestonesOpen || milestoneFilterId ? 'active-filter' : ''}`} type="button" title="Milestones" aria-label="Milestones" aria-expanded={isMilestonesOpen} onClick={() => setMilestonesOpen((current) => !current)}><BoardToolbarIcon type="milestones" /></button>{isMilestonesOpen && <div className="board-milestones-popover"><div className="popover-heading"><b>Milestones</b><button type="button" onClick={() => setMilestonesOpen(false)} aria-label="Закрыть">×</button></div><p className="board-labels-copy">Группы задач для релизов и крупных целей.</p><div className="milestone-list">{milestones.length ? milestones.map((milestone) => <article key={milestone.id}><button type="button" className={milestoneFilterId === milestone.id ? 'selected' : ''} onClick={() => setMilestoneFilterId((current) => current === milestone.id ? null : milestone.id)}><i style={{ backgroundColor: milestone.color }} /><span><b>{milestone.name}</b><small>{usage(milestone.id)} задач</small></span>{milestoneFilterId === milestone.id && <em>✓</em>}</button>{!isPublicViewer && <button type="button" className="text-action danger-text" onClick={() => deleteMilestone(milestone)}>Удалить</button>}</article>) : <p className="empty-comments">Milestones пока нет.</p>}</div>{milestoneFilterId && <button type="button" className="text-action milestone-show-all" onClick={() => setMilestoneFilterId(null)}>Показать все задачи</button>}{!isPublicViewer && <form className="new-label-form board-label-create" onSubmit={createMilestone}><input value={milestoneNameDraft} onChange={(event) => setMilestoneNameDraft(event.target.value)} maxLength={120} placeholder="Новый milestone" aria-label="Название milestone" /><input type="color" value={milestoneColorDraft} onChange={(event) => setMilestoneColorDraft(event.target.value)} aria-label="Цвет milestone" /><button type="submit" disabled={isSavingMilestone || !milestoneNameDraft.trim()}>{isSavingMilestone ? 'Создаём…' : 'Создать'}</button></form>}</div>}</div>;
  }
  function renderCardMilestoneControl() {
    if (!selected) return null;
    if (isPublicViewer) return selected.milestone ? <span className="quick-action card-milestone-indicator" title={selected.milestone.name} aria-label={`Milestone: ${selected.milestone.name}`}><BoardToolbarIcon type="milestones" /></span> : null;
    return <div className="card-milestone-control"><button type="button" className={`quick-action ${isCardMilestoneOpen ? 'active' : ''}`} onClick={() => setCardMilestoneOpen((current) => !current)} title={selected.milestone ? `Milestone: ${selected.milestone.name}` : 'Назначить milestone'} aria-label={selected.milestone ? `Milestone: ${selected.milestone.name}` : 'Назначить milestone'}><BoardToolbarIcon type="milestones" /></button>{isCardMilestoneOpen && <div className="property-popover card-milestone-popover"><div className="popover-heading"><b>Milestone</b><button type="button" onClick={() => setCardMilestoneOpen(false)} aria-label="Закрыть">×</button></div><button type="button" className={!selected.milestone ? 'selected' : ''} onClick={() => replaceSelectedMilestone(null)}>Без milestone {!selected.milestone && <b>✓</b>}</button>{milestones.map((milestone) => <button key={milestone.id} type="button" className={selected.milestone?.id === milestone.id ? 'selected' : ''} onClick={() => replaceSelectedMilestone(milestone)}><i style={{ backgroundColor: milestone.color }} />{milestone.name}{selected.milestone?.id === milestone.id && <b>✓</b>}</button>)}</div>}</div>;
  }
  function renderChecklists() {
    return <section className="checklists checklist-panel">
      <div className="section-heading"><h3>Чек-листы</h3><span>{checklists.length || '—'}</span></div>
      {isDetailsLoading ? <p className="detail-loading">Загружаем чек-листы…</p> : <>
        {checklists.map((checklist) => {
          const completed = checklist.items.filter((item) => item.is_completed).length;
          const itemIds = checklist.items.map((item) => String(item.id));
          const allExpanded = itemIds.length > 0 && itemIds.every((id) => expandedChecklistItemIds.includes(id));
          const isCollapsed = collapsedChecklistIds.includes(checklist.id);
          return <section className={`checklist ${isCollapsed ? 'collapsed' : ''}`} key={checklist.id}>
            <div className="section-heading">
              <h4>{checklist.title}</h4><span>{completed}/{checklist.items.length}</span>
              <button className={`text-action checklist-collapse-toggle ${isCollapsed ? 'collapsed' : ''}`} type="button" title={isCollapsed ? 'Развернуть чек-лист' : 'Свернуть чек-лист'} aria-expanded={!isCollapsed} onClick={() => setCollapsedChecklistIds((current) => current.includes(checklist.id) ? current.filter((id) => id !== checklist.id) : [...current, checklist.id])}>{isCollapsed ? '⌄' : '⌃'}</button>
              <button className="text-action checklist-all-toggle" type="button" title={allExpanded ? 'Свернуть все детали' : 'Раскрыть все детали'} onClick={() => setExpandedChecklistItemIds((current) => allExpanded ? current.filter((id) => !itemIds.includes(id)) : [...new Set([...current, ...itemIds])])}>{allExpanded ? '⌃ Все' : '⌄ Все'}</button>
              <button className="text-action danger-text" onClick={() => deleteChecklist(checklist)}>Удалить</button>
            </div>
            {!isCollapsed && <>
              <div className="progress"><i style={{ width: `${checklist.items.length ? completed / checklist.items.length * 100 : 0}%` }} /></div>
              {checklist.items.map((item) => {
                const itemId = String(item.id);
                const isExpanded = expandedChecklistItemIds.includes(itemId);
                return <div className="checklist-item" key={item.id}>
                  <div className="check-row"><button className={`check-item ${item.is_completed ? 'done' : ''}`} onClick={() => toggleChecklistItem(checklist.id, item)} aria-pressed={item.is_completed}><span className="check-control">{item.is_completed && '✓'}</span>{item.title}</button><button className={`check-item-toggle ${isExpanded ? 'open' : ''}`} type="button" title={isExpanded ? 'Скрыть детали пункта' : 'Раскрыть детали пункта'} aria-expanded={isExpanded} onClick={() => setExpandedChecklistItemIds((current) => current.includes(itemId) ? current.filter((id) => id !== itemId) : [...current, itemId])}>⌄</button><button className="remove-check" onClick={() => removeChecklistItem(checklist.id, item)} aria-label={`Удалить пункт ${item.title}`}>×</button></div>
                  {isExpanded && <div className="check-item-detail"><MentionTextarea className={unreadMentionSourceIds.includes(itemId) ? 'mention-highlight' : undefined} value={checklistItemDescriptionDrafts[itemId] ?? item.description} onValueChange={(value) => setChecklistItemDescriptionDrafts((current) => ({ ...current, [itemId]: value }))} onBlur={() => saveChecklistItemDescription(checklist.id, item)} members={account ? workspaceMembers : []} maxLength={4000} placeholder="Описание пункта…" ariaLabel={`Описание пункта ${item.title}`} /><label className="check-item-upload">{isUploadingChecklistItemAttachment ? 'Загружаем…' : '＋ Картинка или видео'}<input type="file" accept="image/jpeg,image/png,image/gif,image/webp,video/mp4,video/webm,video/quicktime" multiple disabled={isUploadingChecklistItemAttachment} onChange={(event) => { const files = Array.from(event.target.files ?? []); event.target.value = ''; void uploadChecklistItemAttachments(checklist.id, item, files); }} /></label>{item.attachments.length > 0 && <div className="check-item-attachments">{item.attachments.map((attachment) => <figure key={attachment.id}>{attachment.media_type.startsWith('image/') ? <button className="check-item-image" type="button" onClick={() => setImagePreview({ url: assetUrl(attachment.url), name: attachment.original_name })}><img src={assetUrl(attachment.url)} alt={attachment.original_name} /></button> : attachment.media_type.startsWith('video/') ? <video controls preload="metadata" src={assetUrl(attachment.url)} /> : <a href={assetUrl(attachment.url)} target="_blank" rel="noreferrer">{attachment.original_name}</a>}<figcaption><span>{attachment.original_name}</span><button type="button" onClick={() => deleteChecklistItemAttachment(checklist.id, item, attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></figcaption></figure>)}</div>}</div>}
                </div>;
              })}
              <form className="inline-composer" onSubmit={(event) => addChecklistItem(event, checklist.id)}><input value={checklistItemDrafts[checklist.id] ?? ''} onChange={(event) => setChecklistItemDrafts((current) => ({ ...current, [checklist.id]: event.target.value }))} maxLength={500} placeholder="Добавить пункт…" aria-label={`Новый пункт для ${checklist.title}`} /><button type="submit" disabled={isSavingChecklist || !(checklistItemDrafts[checklist.id] ?? '').trim()}>Добавить</button></form>
            </>}
          </section>;
        })}
        <form className="new-checklist-form" onSubmit={createChecklist}><input value={checklistNameDraft} onChange={(event) => setChecklistNameDraft(event.target.value)} maxLength={200} placeholder="Название нового чек-листа" aria-label="Название нового чек-листа" /><button type="submit" disabled={isSavingChecklist || !checklistNameDraft.trim()}>＋ Чек-лист</button></form>
      </>}
    </section>;
  }
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
  useEffect(() => {
    if (!boardId || authState !== 'signed-in') {
      setBoardViewMode('standard');
      setFreeformLayout({});
      setFreeformCardLayout({});
      replaceFreeformDrawing({ strokes: [] });
      setFreeformLive({ cursors: [], pings: [] });
      return;
    }
    let active = true;
    void fetch(`${API_URL}/v1/boards/${boardId}/layout`)
      .then(async (response) => { if (!response.ok) throw new Error('layout load failed'); return response.json() as Promise<BoardLayout>; })
      .then((layout) => {
        if (!active) return;
        setBoardViewMode(layout.view_mode === 'freeform' ? 'freeform' : 'standard');
        setFreeformLayout(Object.fromEntries(layout.positions.map((position) => [position.list_id, { x: position.x, y: position.y }])));
      })
      .catch(() => { if (active) { setBoardViewMode('standard'); setFreeformLayout({}); } });
    return () => { active = false; };
  }, [authState, boardId]);

  useEffect(() => {
    if (!boardId || authState !== 'signed-in') return;
    let active = true;
    void fetch(`${API_URL}/v1/boards/${boardId}/freeform/drawing`)
      .then(async (response) => { if (!response.ok) throw new Error('drawing load failed'); return response.json() as Promise<{ document: FreeformDrawing }>; })
      .then((drawing) => { if (active) replaceFreeformDrawing({ strokes: Array.isArray(drawing.document?.strokes) ? drawing.document.strokes : [] }); })
      .catch(() => { if (active) replaceFreeformDrawing({ strokes: [] }); });
    return () => { active = false; };
  }, [authState, boardId]);

  useEffect(() => {
    if (!boardId || authState !== 'signed-in') { setFreeformCardLayout({}); return; }
    let active = true;
    void fetch(`${API_URL}/v1/boards/${boardId}/freeform/cards`)
      .then(async (response) => { if (!response.ok) throw new Error('freeform cards load failed'); return response.json() as Promise<FreeformCardPosition[]>; })
      .then((positions) => { if (active) setFreeformCardLayout(Object.fromEntries(positions.map((position) => [position.card_id, { x: position.x, y: position.y }]))); })
      .catch(() => { if (active) setFreeformCardLayout({}); });
    return () => { active = false; };
  }, [authState, boardId]);

  useEffect(() => {
    if (!boardId || authState !== 'signed-in' || boardViewMode !== 'freeform') {
      setFreeformLive({ cursors: [], pings: [] });
      return;
    }
    let active = true;
    let socket: WebSocket | null = null;
    let reconnectTimer: number | null = null;
    const expiryTimers = freeformLiveExpiryTimersRef.current;
    const clearExpiry = (key: string) => {
      const timer = expiryTimers.get(key);
      if (timer !== undefined) window.clearTimeout(timer);
      expiryTimers.delete(key);
    };
    const scheduleExpiry = (key: string, delay: number, remove: () => void) => {
      clearExpiry(key);
      expiryTimers.set(key, window.setTimeout(() => { expiryTimers.delete(key); if (active) remove(); }, delay));
    };
    const applyEvent = (event: FreeformLiveSocketEvent) => {
      if (event.type === 'cursor') {
        setFreeformLive((current) => ({ ...current, cursors: [...current.cursors.filter((cursor) => cursor.user_id !== event.user_id), event] }));
        scheduleExpiry(`cursor:${event.user_id}`, 12_200, () => setFreeformLive((current) => ({ ...current, cursors: current.cursors.filter((cursor) => cursor.user_id !== event.user_id) })));
        return;
      }
      setFreeformLive((current) => ({ ...current, pings: [...current.pings.filter((ping) => ping.id !== event.id), event] }));
      scheduleExpiry(`ping:${event.id}`, Math.max(100, event.expires_in_ms), () => setFreeformLive((current) => ({ ...current, pings: current.pings.filter((ping) => ping.id !== event.id) })));
    };
    const loadSnapshot = () => {
      void fetch(`${API_URL}/v1/boards/${boardId}/freeform/live`)
        .then(async (response) => { if (!response.ok) throw new Error('live snapshot failed'); return response.json() as Promise<FreeformLive>; })
        .then((live) => {
          if (!active) return;
          setFreeformLive(live);
          live.cursors.forEach((cursor) => scheduleExpiry(`cursor:${cursor.user_id}`, 12_200, () => setFreeformLive((current) => ({ ...current, cursors: current.cursors.filter((item) => item.user_id !== cursor.user_id) }))));
          live.pings.forEach((ping) => scheduleExpiry(`ping:${ping.id}`, Math.max(100, ping.expires_in_ms), () => setFreeformLive((current) => ({ ...current, pings: current.pings.filter((item) => item.id !== ping.id) }))));
        })
        .catch(() => undefined);
    };
    const connect = () => {
      if (!active) return;
      let nextSocket: WebSocket;
      try { nextSocket = new WebSocket(freeformLiveSocketUrl(boardId)); } catch { reconnectTimer = window.setTimeout(connect, 1_000); return; }
      socket = nextSocket;
      freeformLiveSocketRef.current = nextSocket;
      nextSocket.onopen = loadSnapshot;
      nextSocket.onmessage = (message) => {
        try {
          const event = JSON.parse(String(message.data)) as FreeformLiveSocketEvent;
          if (event.type === 'cursor' || event.type === 'ping') applyEvent(event);
        } catch { /* Ignore malformed broadcast packets. */ }
      };
      nextSocket.onerror = () => nextSocket.close();
      nextSocket.onclose = () => {
        if (freeformLiveSocketRef.current === nextSocket) freeformLiveSocketRef.current = null;
        if (active) reconnectTimer = window.setTimeout(connect, 1_000);
      };
    };
    connect();
    return () => {
      active = false;
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      if (freeformLiveSocketRef.current === socket) freeformLiveSocketRef.current = null;
      socket?.close();
      expiryTimers.forEach((timer) => window.clearTimeout(timer));
      expiryTimers.clear();
    };
  }, [authState, boardId, boardViewMode]);

  useEffect(() => {
    if (boardViewMode !== 'freeform') return;
    const board = boardRef.current;
    if (!board) return;
    setFreeformViewport({ x: board.scrollLeft / freeformZoom, y: board.scrollTop / freeformZoom, width: board.clientWidth / freeformZoom, height: board.clientHeight / freeformZoom });
  }, [boardId, boardViewMode, freeformZoom]);

  useEffect(() => {
    if (boardViewMode !== 'freeform') return;
    const board = boardRef.current;
    if (!board) return;
    const onWheel = (event: WheelEvent) => {
      if (event.target instanceof Element && event.target.closest('.card-list, textarea, input, select')) return;
      event.preventDefault();
      event.stopPropagation();
      zoomFreeform(board, event.clientX, event.clientY, event.deltaY);
    };
    // React delegates wheel events passively in some browser paths, which lets
    // the document scroll even after preventDefault(). This listener must stay
    // native and explicitly non-passive.
    board.addEventListener('wheel', onWheel, { passive: false });
    return () => board.removeEventListener('wheel', onWheel);
  }, [boardViewMode, freeformZoom]);

  useEffect(() => {
    if (!freeformDrawingDirtyRef.current || !boardId || authState !== 'signed-in' || boardViewMode !== 'freeform') return;
    if (freeformDrawingSaveTimerRef.current !== null) window.clearTimeout(freeformDrawingSaveTimerRef.current);
    freeformDrawingSaveTimerRef.current = window.setTimeout(() => flushFreeformDrawing(), 520);
    return () => { if (freeformDrawingSaveTimerRef.current !== null) window.clearTimeout(freeformDrawingSaveTimerRef.current); };
  }, [authState, boardId, boardViewMode, freeformDrawing]);

  function getFreeformPoint(clientX: number, clientY: number): FreeformPosition | null {
    const canvas = freeformCanvasRef.current;
    if (!canvas) return null;
    const bounds = canvas.getBoundingClientRect();
    return { x: Math.max(0, Math.round((clientX - bounds.left) / freeformZoom)), y: Math.max(0, Math.round((clientY - bounds.top) / freeformZoom)) };
  }
  function publishFreeformCursor(point: FreeformPosition, ping = false) {
    if (!boardId || authState !== 'signed-in' || boardViewMode !== 'freeform') return;
    const now = Date.now();
    if (!ping && now - freeformLiveSentAtRef.current < 32) return;
    freeformLiveSentAtRef.current = now;
    const socket = freeformLiveSocketRef.current;
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify(ping ? { type: 'ping', ...point } : { type: 'cursor', ...point }));
      return;
    }
    void fetch(`${API_URL}/v1/boards/${boardId}/freeform/live`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ ...point, ping }) })
      .then(async (response) => { if (!response.ok) throw new Error('live update failed'); return response.json() as Promise<FreeformLive>; })
      .then((live) => setFreeformLive(live))
      .catch(() => undefined);
  }
  function updateFreeformCursor(event: ReactPointerEvent<HTMLElement>) {
    if (isFreeformDrawing && event.target instanceof Element && event.target.closest('.freeform-ink')) return;
    const point = getFreeformPoint(event.clientX, event.clientY);
    if (point) publishFreeformCursor(point);
  }
  function zoomFreeform(board: HTMLElement, clientX: number, clientY: number, deltaY: number) {
    const delta = deltaY > 0 ? -0.08 : 0.08;
    const bounds = board.getBoundingClientRect();
    const cursorX = (clientX - bounds.left + board.scrollLeft) / freeformZoom;
    const cursorY = (clientY - bounds.top + board.scrollTop) / freeformZoom;
    const nextZoom = Math.max(0.42, Math.min(1.45, Math.round((freeformZoom + delta) * 100) / 100));
    if (nextZoom === freeformZoom) return;
    setFreeformZoom(nextZoom);
    window.requestAnimationFrame(() => {
      board.scrollLeft = Math.max(0, cursorX * nextZoom - (clientX - bounds.left));
      board.scrollTop = Math.max(0, cursorY * nextZoom - (clientY - bounds.top));
    });
  }
  function startFreeformInk(event: ReactPointerEvent<SVGSVGElement>) {
    if ((!isFreeformDrawing && !isFreeformErasing) || event.button !== 0 || isPublicViewer) return;
    const point = getFreeformPoint(event.clientX, event.clientY);
    if (!point) return;
    const erasing = isFreeformErasing;
    freeformInkRef.current = { pointerId: event.pointerId, erasing };
    event.currentTarget.setPointerCapture(event.pointerId);
    if (erasing) { eraseFreeformAt(point); publishFreeformCursor(point); event.preventDefault(); event.stopPropagation(); return; }
    const stroke: FreeformStroke = { id: globalThis.crypto.randomUUID(), author_id: account?.user.id, points: [point], color: freeformInkColor, width: freeformInkWidth };
    freeformDrawingDirtyRef.current = true;
    replaceFreeformDrawing((current) => ({ strokes: [...current.strokes, stroke] }));
    publishFreeformCursor(point);
    event.preventDefault(); event.stopPropagation();
  }
  function continueFreeformInk(event: ReactPointerEvent<SVGSVGElement>) {
    const interaction = freeformInkRef.current;
    if ((!isFreeformDrawing && !isFreeformErasing) || interaction?.pointerId !== event.pointerId) return;
    const point = getFreeformPoint(event.clientX, event.clientY);
    if (!point) return;
    if (interaction.erasing) { eraseFreeformAt(point); publishFreeformCursor(point); event.preventDefault(); event.stopPropagation(); return; }
    freeformDrawingDirtyRef.current = true;
    replaceFreeformDrawing((current) => {
      if (!current.strokes.length) return current;
      const strokes = [...current.strokes];
      const last = strokes.length - 1;
      const previous = strokes[last];
      const finalPoint = previous.points[previous.points.length - 1];
      if (finalPoint && Math.hypot(finalPoint.x - point.x, finalPoint.y - point.y) < 2) return current;
      strokes[last] = { ...previous, points: [...previous.points, point] };
      return { strokes };
    });
    publishFreeformCursor(point);
    event.preventDefault(); event.stopPropagation();
  }
  function finishFreeformInk(event: ReactPointerEvent<SVGSVGElement>) {
    const interaction = freeformInkRef.current;
    if (interaction?.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    freeformInkRef.current = null;
    if (interaction.erasing) flushFreeformDrawing();
    event.preventDefault(); event.stopPropagation();
  }
  function eraseFreeformAt(point: FreeformPosition) {
    const radius = Math.max(14, freeformInkWidth * 4);
    let removedForeign = false;
    const current = freeformDrawingRef.current;
    let changed = false;
    const strokes = current.strokes.flatMap((stroke) => {
      const chunks: DiagramPoint[][] = [];
      let chunk: DiagramPoint[] = [];
      let hit = false;
      const flush = () => { if (chunk.length >= 2) chunks.push(chunk); chunk = []; };
      stroke.points.forEach((item) => {
        if (Math.hypot(item.x - point.x, item.y - point.y) <= radius + stroke.width / 2) { hit = true; flush(); }
        else chunk.push(item);
      });
      flush();
      if (!hit) return [stroke];
      changed = true;
      if (stroke.author_id !== account?.user.id) removedForeign = true;
      return chunks.map((points, index) => ({
        ...stroke,
        // Old drawings predate collaboration metadata. The user who erases a
        // legacy line becomes the author of its remaining fragments.
        id: index === 0 ? stroke.id ?? globalThis.crypto.randomUUID() : globalThis.crypto.randomUUID(),
        author_id: stroke.author_id ?? account?.user.id,
        points,
      }));
    });
    if (!changed) return;
    freeformDrawingDirtyRef.current = true;
    freeformEraseForeignRef.current ||= removedForeign;
    replaceFreeformDrawing({ strokes });
  }
  function clearOwnFreeformInk() {
    if (!window.confirm('Стереть только ваши линии на свободной доске?')) return;
    freeformDrawingDirtyRef.current = true;
    replaceFreeformDrawing((current) => ({ strokes: current.strokes.filter((stroke) => stroke.author_id !== account?.user.id) }));
  }

  function defaultFreeformPosition(layout: Record<string, FreeformPosition>, listId: EntityId) {
    const positions = Object.values(layout);
    return layout[String(listId)] ?? { x: positions.length ? Math.max(...positions.map((position) => position.x)) + 336 : 0, y: 0 };
  }
  function persistBoardLayout(mode: BoardViewMode, layout: Record<string, FreeformPosition>, sourceColumns = columns) {
    if (persistence !== 'connected' || !boardId || authState !== 'signed-in') return;
    const positions = sourceColumns.filter((column) => typeof column.id === 'string').map((column, index) => {
      const position = defaultFreeformPosition(layout, column.id) ?? { x: index * 336, y: 0 };
      return { list_id: String(column.id), x: Math.max(0, Math.round(position.x)), y: Math.max(0, Math.round(position.y)) };
    });
    void fetch(`${API_URL}/v1/boards/${boardId}/layout`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ view_mode: mode, positions }) })
      .then((response) => { if (!response.ok) throw new Error('layout save failed'); })
      .catch(() => showToast('Раскладка не сохранена: обновите доску'));
  }
  function changeBoardViewMode(mode: BoardViewMode) {
    if (authState !== 'signed-in') return;
    const nextLayout = { ...freeformLayout };
    columns.forEach((column, index) => { if (!nextLayout[String(column.id)]) nextLayout[String(column.id)] = { x: index * 336, y: 0 }; });
    setFreeformLayout(nextLayout);
    setBoardViewMode(mode);
    persistBoardLayout(mode, nextLayout);
  }
  function beginFreeformColumnDrag(event: ReactPointerEvent<HTMLDivElement>, column: Column, index: number) {
    if (boardViewMode !== 'freeform' || authState !== 'signed-in' || event.button !== 0) return;
    const origin = freeformLayout[String(column.id)] ?? { x: index * 336, y: 0 };
    freeformDragRef.current = { pointerId: event.pointerId, columnId: column.id, startX: event.clientX, startY: event.clientY, origin };
    event.currentTarget.setPointerCapture(event.pointerId);
    setDraggingColumnId(column.id);
    event.preventDefault();
    event.stopPropagation();
  }
  function moveFreeformColumnDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const drag = freeformDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const snap = event.shiftKey ? 24 : 1;
    const x = Math.max(0, Math.round((drag.origin.x + (event.clientX - drag.startX) / freeformZoom) / snap) * snap);
    const y = Math.max(0, Math.round((drag.origin.y + (event.clientY - drag.startY) / freeformZoom) / snap) * snap);
    setFreeformLayout((current) => ({ ...current, [String(drag.columnId)]: { x, y } }));
    event.preventDefault();
  }
  function endFreeformColumnDrag(event: ReactPointerEvent<HTMLDivElement>) {
    const drag = freeformDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    freeformDragRef.current = null;
    setDraggingColumnId(null);
    const snap = event.shiftKey ? 24 : 1;
    const position = {
      x: Math.max(0, Math.round((drag.origin.x + (event.clientX - drag.startX) / freeformZoom) / snap) * snap),
      y: Math.max(0, Math.round((drag.origin.y + (event.clientY - drag.startY) / freeformZoom) / snap) * snap),
    };
    const nextLayout = { ...freeformLayout, [String(drag.columnId)]: position };
    setFreeformLayout(nextLayout);
    persistBoardLayout('freeform', nextLayout);
  }
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
    boardPanRef.current = { pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, startScrollLeft: board.scrollLeft, startScrollTop: board.scrollTop, moved: false };
  }
  function moveBoardPan(event: ReactPointerEvent<HTMLElement>) {
    const pan = boardPanRef.current;
    if (!pan || pan.pointerId !== event.pointerId) return;
    if (Math.max(Math.abs(event.clientX - pan.startX), Math.abs(event.clientY - pan.startY)) <= 4 && !pan.moved) return;
    if (!pan.moved) {
      pan.moved = true;
      event.currentTarget.setPointerCapture(event.pointerId);
      setBoardPanning(true);
    }
    event.currentTarget.scrollLeft = pan.startScrollLeft - (event.clientX - pan.startX);
    event.currentTarget.scrollTop = pan.startScrollTop - (event.clientY - pan.startY);
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
    setColumnDropBeforeId(null);
  }
  function clearDragState() {
    stopCardListAutoScroll();
    setDragging(null);
    setCardDragPreview(null);
    setDragOverListId(null);
    setDragDropTarget(null);
  }
  function beginCardDragPreview(event: ReactDragEvent<HTMLElement>, card: Card) {
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = event.clientX || bounds.left + Math.min(44, bounds.width / 2);
    const y = event.clientY || bounds.top + Math.min(28, bounds.height / 2);
    setCardDragPreview({ card, x, y, width: bounds.width, height: bounds.height });
    // The browser drag image is deliberately transparent: the animated preview below is more responsive.
    const transparentImage = new Image();
    transparentImage.src = 'data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==';
    event.dataTransfer.setDragImage(transparentImage, 0, 0);
  }
  function moveCardDragPreview(event: ReactDragEvent<HTMLElement>) {
    if (event.clientX <= 0 || event.clientY <= 0) return;
    setCardDragPreview((current) => current ? { ...current, x: event.clientX, y: event.clientY } : current);
  }
  function beginPointerCardDrag(event: ReactPointerEvent<HTMLElement>, card: Card, sourceListId: EntityId) {
    if (event.button !== 0 || boardViewMode !== 'standard' || isPublicViewer) return;
    if (event.target instanceof Element && event.target.closest('button, a, input, textarea, select')) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    pointerCardDragRef.current = { card, sourceListId, startX: event.clientX, startY: event.clientY, width: bounds.width, height: bounds.height, active: false };
    pointerCardDropRef.current = null;
    event.currentTarget.setPointerCapture?.(event.pointerId);
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
    const card: Card = { id: nextCardId, title, labels: [], roles: [], members: [] };
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
    const sourceElement = typeof document === 'undefined' ? null : Array.from(document.querySelectorAll<HTMLElement>('[data-card-id]')).find((element) => element.dataset.cardId === String(cardId));
    const sourceBounds = cardDragPreview?.card.id === cardId ? { left: cardDragPreview.x - 28, top: cardDragPreview.y - 20, width: cardDragPreview.width, height: cardDragPreview.height } : sourceElement?.getBoundingClientRect();
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
    if (sourceBounds && typeof window !== 'undefined') {
      const motionKey = Date.now();
      window.requestAnimationFrame(() => window.requestAnimationFrame(() => {
        const targetElement = Array.from(document.querySelectorAll<HTMLElement>('[data-card-id]')).find((element) => element.dataset.cardId === String(cardId));
        if (!targetElement) return;
        const targetBounds = targetElement.getBoundingClientRect();
        if (Math.abs(targetBounds.left - sourceBounds.left) < 2 && Math.abs(targetBounds.top - sourceBounds.top) < 2) return;
        setCardMoveMotion({ key: motionKey, cardId: String(cardId), title: card.title, from: sourceBounds, to: targetBounds });
        window.setTimeout(() => setCardMoveMotion((current) => current?.key === motionKey ? null : current), 480);
      }));
    }
    clearFreeformCardPosition(cardId);
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
      const withoutMoving = current.filter((column) => column.id !== columnId);
      const targetIndex = beforeColumnId === undefined ? withoutMoving.length : withoutMoving.findIndex((column) => column.id === beforeColumnId);
      if (targetIndex < 0) return current;
      const next = [...withoutMoving];
      next.splice(targetIndex, 0, moving);
      return next;
    });
    clearColumnDragState();
    if (persistence === 'connected' && typeof columnId === 'string') {
      void fetch(`${API_URL}/v1/lists/${columnId}/move`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ before_list_id: typeof beforeColumnId === 'string' ? beforeColumnId : null }) })
        .then((response) => { if (!response.ok) throw new Error('list move failed'); })
        .catch(() => showToast('Порядок колонок не сохранён: обновите доску'));
    }
  }
  function addColumn(position?: FreeformPosition) {
    const title = `Новая колонка ${columns.length + 1}`;
    if (persistence === 'connected' && boardId) {
      void fetch(`${API_URL}/v1/boards/${boardId}/lists`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ title }) })
        .then(async (response) => { if (!response.ok) throw new Error('save failed'); return response.json() as Promise<{ id: string; title: string; grid_column: number; grid_row: number }>; })
        .then((saved) => {
          const nextColumn: Column = { id: saved.id, title: saved.title, cards: [] };
          setColumns((current) => [...current, nextColumn]);
          if (boardViewMode === 'freeform') {
            const nextLayout = { ...freeformLayout, [saved.id]: position ?? defaultFreeformPosition(freeformLayout, saved.id) };
            setFreeformLayout(nextLayout);
            persistBoardLayout('freeform', nextLayout, [...columns, nextColumn]);
          }
          showToast('Колонка добавлена');
        })
        .catch(() => showToast('Не удалось сохранить колонку'));
      return;
    }
    setColumns((current) => [...current, { id: current.length + 1, title, cards: [] }]); showToast('Колонка добавлена');
  }
  function setFreeformCardPosition(cardId: EntityId, position: FreeformPosition) {
    if (typeof cardId !== 'string') return;
    const normalized = { x: Math.max(0, Math.round(position.x)), y: Math.max(0, Math.round(position.y)) };
    setFreeformCardLayout((current) => ({ ...current, [cardId]: normalized }));
    if (persistence !== 'connected' || !boardId || authState !== 'signed-in') return;
    void fetch(`${API_URL}/v1/boards/${boardId}/freeform/cards/${cardId}`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(normalized) })
      .then((response) => { if (!response.ok) throw new Error('freeform card save failed'); })
      .catch(() => showToast('Положение карточки не сохранено'));
  }
  function clearFreeformCardPosition(cardId: EntityId) {
    if (typeof cardId !== 'string' || !freeformCardLayout[cardId]) return;
    setFreeformCardLayout((current) => { const { [cardId]: _removed, ...rest } = current; return rest; });
    if (persistence !== 'connected' || !boardId || authState !== 'signed-in') return;
    void fetch(`${API_URL}/v1/boards/${boardId}/freeform/cards/${cardId}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok && response.status !== 404) throw new Error('freeform card clear failed'); })
      .catch(() => showToast('Не удалось вернуть карточку в колонку'));
  }
  function detachDraggedCard(event: ReactDragEvent<HTMLDivElement>) {
    if (boardViewMode !== 'freeform' || !dragging || isPublicViewer || (event.target instanceof Element && event.target.closest('.column, .freeform-detached-card'))) return;
    const point = getFreeformPoint(event.clientX, event.clientY);
    if (!point) return;
    event.preventDefault();
    setFreeformCardPosition(dragging.cardId, point);
    clearDragState();
  }

  function openCard(card: Card) {
    setSelected(card);
    setEditingCardDescription(false);
    setChecklists([]);
    setCollapsedChecklistIds([]);
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
    setWatchingCard(false);
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
      .then((workspace) => { setWorkspaces((current) => [workspace, ...current]); setWorkspaceBoards((current) => ({ ...current, [workspace.id]: [] })); setAdminWorkspaces((current) => [{ id: workspace.id, name: workspace.name, owner_username: account?.user.username ?? 'owner', member_count: 1, archived_at: null }, ...current]); setWorkspaceComposerOpen(false); void selectWorkspace(workspace); showToast(`Пространство «${workspace.name}» создано`); })
      .catch(() => setWorkspaceCreateError('Не удалось создать пространство. Проверьте подключение и повторите.'))
      .finally(() => setCreatingWorkspace(false));
  }
  function openSessions() {
    setSessionsOpen(true);
    void fetch(`${API_URL}/v1/auth/sessions`).then(async (response) => { if (!response.ok) throw new Error('sessions failed'); return response.json() as Promise<AuthSession[]>; })
      .then(setSessions).catch(() => { setSessionsOpen(false); showToast('Не удалось загрузить сессии'); });
  }
  function loadNotifications() {
    if (authState !== 'signed-in') return Promise.resolve();
    return fetch(`${API_URL}/v1/notifications`)
      .then(async (response) => { if (!response.ok) throw new Error('notifications failed'); return response.json() as Promise<CardNotification[]>; })
      .then(setNotifications)
      .catch(() => undefined);
  }
  function toggleNotifications() {
    setNotificationsOpen((current) => {
      const next = !current;
      if (next) { setNotificationsLoading(true); void loadNotifications().finally(() => setNotificationsLoading(false)); }
      return next;
    });
  }
  function openNotification(notification: CardNotification) {
    setNotifications((current) => current.map((item) => item.id === notification.id ? { ...item, is_read: true } : item));
    setNotificationsOpen(false);
    if (!notification.is_read) void fetch(`${API_URL}/v1/notifications/${notification.id}/read`, { method: 'POST' }).catch(() => undefined);
    if (notification.board_id !== boardId) { setPendingNotificationCardId(notification.card_id); void selectBoard(notification.board_id); }
    else {
      const card = columns.flatMap((column) => column.cards).find((item) => item.id === notification.card_id);
      if (card) openCard(card);
    }
  }
  function markAllNotificationsRead() {
    setNotifications((current) => current.map((item) => ({ ...item, is_read: true })));
    void fetch(`${API_URL}/v1/notifications/read`, { method: 'POST' }).catch(() => { void loadNotifications(); });
  }
  function toggleCardWatch() {
    if (!selected || authState !== 'signed-in' || typeof selected.id !== 'string') return;
    const watching = !isWatchingCard;
    setWatchingCard(watching);
    void fetch(`${API_URL}/v1/cards/${selected.id}/watch`, { method: watching ? 'PUT' : 'DELETE' })
      .then(async (response) => { if (!response.ok) throw new Error('watch failed'); return response.json() as Promise<{ watching: boolean }>; })
      .then((result) => { setWatchingCard(result.watching); showToast(result.watching ? 'Вы подписались на изменения карточки' : 'Подписка на карточку отключена'); })
      .catch(() => { setWatchingCard(!watching); showToast('Не удалось изменить подписку'); });
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
  function transferCardAssignee(card: Card, sourceMemberId: string | null, targetMember: Member | null) {
    const targetMemberId = targetMember ? String(targetMember.id) : null;
    if (sourceMemberId === targetMemberId || isPublicViewer) return;
    let members = card.members.filter((member) => sourceMemberId === null || String(member.id) !== sourceMemberId);
    if (targetMember && !members.some((member) => String(member.id) === targetMemberId)) members = [...members, targetMember];
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((item) => item.id === card.id ? { ...item, members } : item) })));
    setSelected((current) => current?.id === card.id ? { ...current, members } : current);
    setMemberDrag(null);
    if (persistence !== 'connected' || typeof card.id !== 'string') { showToast('Исполнитель изменён'); return; }
    void fetch(`${API_URL}/v1/cards/${card.id}/assignees`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ user_ids: members.filter((member) => typeof member.id === 'string').map((member) => member.id) }) })
      .then(async (response) => { if (!response.ok) throw new Error('assignees save failed'); return response.json() as Promise<ApiMember[]>; })
      .then((saved) => {
        const persistedMembers = saved.map(memberFromApi);
        setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((item) => item.id === card.id ? { ...item, members: persistedMembers } : item) })));
        setSelected((current) => current?.id === card.id ? { ...current, members: persistedMembers } : current);
        showToast(targetMember ? `Задача назначена @${targetMember.name}` : 'Исполнитель снят');
      })
      .catch(() => { setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((item) => item.id === card.id ? card : item) }))); setSelected((current) => current?.id === card.id ? card : current); showToast('Не удалось сохранить исполнителя'); });
  }
  function renderMemberBoard() {
    return <div className="member-lanes" aria-label="Задачи по исполнителям">{memberLanes.map((lane) => <section className={`member-lane ${memberDrag ? 'member-drop-enabled' : ''}`} key={lane.id ?? 'unassigned'} onDragOver={(event) => { if (memberDrag && !isPublicViewer) event.preventDefault(); }} onDrop={(event) => { if (!memberDrag || isPublicViewer) return; event.preventDefault(); event.stopPropagation(); transferCardAssignee(memberDrag.card, memberDrag.sourceMemberId, lane.member); }}><header><div>{lane.member ? <Avatar member={lane.member} /> : <span className="unassigned-avatar">—</span>}<span><b>{lane.member ? `@${lane.member.name}` : 'Без исполнителя'}</b><small>{lane.cards.length} задач</small></span></div></header><div className="member-card-list">{lane.cards.map((card) => <article className={`task-card member-task-card ${card.completedAt ? 'completed' : ''} ${memberDrag?.card.id === card.id ? 'dragging' : ''}`} key={`${lane.id ?? 'unassigned'}-${card.id}`} draggable={!isPublicViewer} onDragStart={(event) => { didDragRef.current = false; event.dataTransfer.effectAllowed = 'move'; setMemberDrag({ card, sourceMemberId: lane.id }); }} onDragEnd={() => { didDragRef.current = true; setMemberDrag(null); window.setTimeout(() => { didDragRef.current = false; }, 0); }} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setCardContextMenu({ card, x: Math.min(event.clientX, window.innerWidth - 210), y: Math.min(event.clientY, window.innerHeight - 170) }); }} onClick={() => { if (!didDragRef.current) openCard(card); }}>{card.hasUnreadMentions && <span className="card-mention-dot" title="Вас упомянули в этой карточке" />}{card.coverUrl && <div className={`card-cover ${card.coverMode ?? 'full'}`}><img src={assetUrl(card.coverUrl)} alt="" /></div>}<div className="card-main">{(card.labels.length > 0 || card.roles.length > 0) && <div className="card-top"><div className="card-labels">{card.labels.map((label) => <LabelChip label={label} key={label.id} />)}{card.roles.map((role) => <ProfileRoleChip role={role} key={role.id} compact />)}</div></div>}<div className="card-title-row"><button className="card-complete" aria-label={card.completedAt ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(card.completedAt)} onClick={(event) => toggleCardCompletion(card, event)}>{card.completedAt && '✓'}</button><h3>{card.title}</h3></div></div>{card.priority ? <span className="card-priority-corner" style={{ right: card.members.length ? 96 : 14 }}><PrioritySignal priority={card.priority} /></span> : null}{(card.checklist || card.comments || card.attachments || card.members.length > 0) && <footer className="card-footer"><div className="card-meta">{card.checklist && <span className={isChecklistComplete(card.checklist) ? 'checklist-complete' : ''}><CardMetaIcon type="checklist" />{card.checklist}</span>}{card.comments && <span><CardMetaIcon type="comments" />{card.comments}</span>}{card.attachments && <span title="Есть вложения"><CardMetaIcon type="attachments" /></span>}</div><div className="card-avatars">{card.members.map((member) => <Avatar key={member.id} member={member} />)}</div></footer>}</article>)}</div>{!lane.cards.length && <p className="member-lane-empty">Перетащите сюда задачу</p>}</section>)}</div>;
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
    if (persistence !== 'connected') { const label = { id: `local-label-${Date.now()}`, name, color: newLabelColor, icon_shape: newLabelShape, icon_color: newLabelIconColor }; setBoardLabels((current) => [...current, label]); setNewLabelName(''); toggleSelectedLabel(label); return; }
    setSavingLabel(true);
    void fetch(`${API_URL}/v1/boards/${boardId}/labels`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, color: newLabelColor, icon_shape: newLabelShape, icon_color: newLabelIconColor }) })
      .then(async (response) => { if (!response.ok) throw new Error('label save failed'); return response.json() as Promise<Label>; })
      .then((label) => { setBoardLabels((current) => current.some((item) => item.id === label.id) ? current.map((item) => item.id === label.id ? label : item) : [...current, label]); setNewLabelName(''); setNewLabelShape('circle'); setNewLabelIconColor('#FFFFFF'); toggleSelectedLabel(label); })
      .catch(() => showToast('Не удалось создать метку'))
      .finally(() => setSavingLabel(false));
  }
  function beginBoardLabelEdit(label: Label) {
    setEditingBoardLabel(label);
    setBoardLabelNameDraft(label.name);
    setBoardLabelColorDraft(label.color);
    setBoardLabelShapeDraft(label.icon_shape ?? 'circle');
    setBoardLabelIconColorDraft(label.icon_color ?? '#FFFFFF');
  }
  function saveBoardLabel(event: FormEvent) {
    event.preventDefault();
    const label = editingBoardLabel;
    const name = boardLabelNameDraft.trim();
    if (!label || !name || isSavingBoardLabel) return;
    if (persistence !== 'connected') {
      const saved = { ...label, name, color: boardLabelColorDraft, icon_shape: boardLabelShapeDraft, icon_color: boardLabelIconColorDraft };
      setBoardLabels((current) => current.map((item) => item.id === saved.id ? saved : item));
      setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => ({ ...card, labels: card.labels.map((item) => item.id === saved.id ? saved : item) })) })));
      setSelected((current) => current ? { ...current, labels: current.labels.map((item) => item.id === saved.id ? saved : item) } : current);
      setEditingBoardLabel(null);
      return;
    }
    setSavingBoardLabel(true);
    void fetch(`${API_URL}/v1/labels/${label.id}`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, color: boardLabelColorDraft, icon_shape: boardLabelShapeDraft, icon_color: boardLabelIconColorDraft }) })
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
    try {
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
    } finally {
      // A rejected parse or a state update must never leave the card in its loading appearance.
      setUploadingAttachment(false);
    }
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
  function applyProfileRoleCatalog(next: ProfileRole[]) {
    const sorted = [...next].sort((left, right) => left.name.localeCompare(right.name, 'ru'));
    setProfileRoles(sorted);
    setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((card) => ({ ...card, roles: card.roles.flatMap((role) => sorted.find((item) => item.id === role.id) ?? []) })) })));
    setSelected((current) => current ? { ...current, roles: current.roles.flatMap((role) => sorted.find((item) => item.id === role.id) ?? []) } : current);
  }
  function toggleSelfProfileRole(role: ProfileRole) {
    const assigned = myProfileRoleIds.includes(role.id);
    setMyProfileRoleIds((current) => assigned ? current.filter((id) => id !== role.id) : [...current, role.id]);
    if (persistence !== 'connected') return;
    void fetch(`${API_URL}/v1/profile-roles/self/${role.id}`, { method: assigned ? 'DELETE' : 'PUT' })
      .then((response) => { if (!response.ok) throw new Error('profile role assignment failed'); })
      .catch(() => { setMyProfileRoleIds((current) => assigned ? [...current, role.id] : current.filter((id) => id !== role.id)); showToast('Не удалось изменить роли профиля'); });
  }
  function saveProfileRole(event: FormEvent) {
    event.preventDefault();
    const name = newProfileRoleName.trim();
    if (!name || isSavingProfileRole) return;
    const editing = editingProfileRole;
    setSavingProfileRole(true);
    const url = editing ? `${API_URL}/v1/profile-roles/${editing.id}` : `${API_URL}/v1/profile-roles`;
    void fetch(url, { method: editing ? 'PATCH' : 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, color: newProfileRoleColor, icon_shape: newProfileRoleShape, icon_color: newProfileRoleIconColor }) })
      .then(async (response) => { if (!response.ok) throw new Error('profile role save failed'); return response.json() as Promise<ProfileRole>; })
      .then((role) => { applyProfileRoleCatalog(editing ? profileRoles.map((item) => item.id === role.id ? role : item) : [...profileRoles, role]); setNewProfileRoleName(''); setNewProfileRoleColor('#6B7CFF'); setNewProfileRoleShape('circle'); setNewProfileRoleIconColor('#FFFFFF'); setEditingProfileRole(null); showToast(editing ? 'Роль сохранена' : 'Роль создана'); })
      .catch(() => showToast('Не удалось сохранить роль'))
      .finally(() => setSavingProfileRole(false));
  }
  function beginProfileRoleEdit(role: ProfileRole) {
    setEditingProfileRole(role); setNewProfileRoleName(role.name); setNewProfileRoleColor(role.color); setNewProfileRoleShape(role.icon_shape); setNewProfileRoleIconColor(role.icon_color ?? '#FFFFFF');
  }
  function removeProfileRole(role: ProfileRole) {
    if (isSavingProfileRole) return;
    setSavingProfileRole(true);
    void fetch(`${API_URL}/v1/profile-roles/${role.id}`, { method: 'DELETE' })
      .then((response) => { if (!response.ok) throw new Error('profile role delete failed'); applyProfileRoleCatalog(profileRoles.filter((item) => item.id !== role.id)); setMyProfileRoleIds((current) => current.filter((id) => id !== role.id)); if (editingProfileRole?.id === role.id) setEditingProfileRole(null); showToast('Роль удалена'); })
      .catch(() => showToast('Не удалось удалить роль'))
      .finally(() => setSavingProfileRole(false));
  }
  function replaceSelectedProfileRoles(roles: ProfileRole[]) {
    if (!selected) return;
    updateSelectedCard({ roles });
    if (persistence !== 'connected' || typeof selected.id !== 'string') return;
    void fetch(`${API_URL}/v1/cards/${selected.id}/profile-roles`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ role_ids: roles.map((role) => role.id) }) })
      .then(async (response) => { if (!response.ok) throw new Error('card profile roles failed'); return response.json() as Promise<ProfileRole[]>; })
      .then((saved) => updateSelectedCard({ roles: saved }))
      .catch(() => showToast('Не удалось сохранить роли карточки'));
  }
  function toggleSelectedProfileRole(role: ProfileRole) {
    if (!selected) return;
    replaceSelectedProfileRoles(selected.roles.some((item) => item.id === role.id) ? selected.roles.filter((item) => item.id !== role.id) : [...selected.roles, role]);
  }
  function toggleCardCompletion(card: Card, event?: ReactMouseEvent<HTMLButtonElement>) {
    event?.stopPropagation();
    const completedAt = card.completedAt ? undefined : new Date().toISOString();
    const shouldReorder = cardSort === 'manual';
    const sourceList = columns.find((column) => column.cards.some((item) => item.id === card.id));
    const sourceElement = typeof document === 'undefined' ? null : Array.from(document.querySelectorAll<HTMLElement>('[data-card-id]')).find((element) => element.dataset.cardId === String(card.id));
    const sourceBounds = sourceElement?.getBoundingClientRect();
    updateSelectedCard(card.id === selected?.id ? { completedAt } : {});
    setColumns((current) => current.map((column) => {
      const matchingCard = column.cards.find((item) => item.id === card.id);
      if (!matchingCard) return column;
      const updatedCard = { ...matchingCard, completedAt };
      if (!shouldReorder) return { ...column, cards: column.cards.map((item) => item.id === card.id ? updatedCard : item) };
      const remaining = column.cards.filter((item) => item.id !== card.id);
      const insertionIndex = completedAt ? remaining.length : Math.max(0, remaining.findIndex((item) => Boolean(item.completedAt)));
      const cards = [...remaining];
      cards.splice(insertionIndex, 0, updatedCard);
      return { ...column, cards };
    }));
    if (shouldReorder && sourceList && sourceBounds && typeof window !== 'undefined') {
      const motionKey = Date.now();
      window.requestAnimationFrame(() => window.requestAnimationFrame(() => {
        const targetElement = Array.from(document.querySelectorAll<HTMLElement>('[data-card-id]')).find((element) => element.dataset.cardId === String(card.id));
        if (!targetElement) return;
        const targetBounds = targetElement.getBoundingClientRect();
        if (Math.abs(targetBounds.top - sourceBounds.top) < 2 && Math.abs(targetBounds.left - sourceBounds.left) < 2) return;
        setCardMoveMotion({ key: motionKey, cardId: String(card.id), title: card.title, from: sourceBounds, to: targetBounds });
        window.setTimeout(() => setCardMoveMotion((current) => current?.key === motionKey ? null : current), 480);
      }));
    }
    if (persistence !== 'connected' || typeof card.id !== 'string') return;
    void fetch(`${API_URL}/v1/cards/${card.id}/completion`, { method: 'PATCH', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ is_completed: !card.completedAt }) })
      .then(async (response) => { if (!response.ok) throw new Error((await response.json().catch(() => null) as { message?: string } | null)?.message ?? 'Не удалось сохранить статус задачи'); })
      .catch((error) => { updateSelectedCard(card.id === selected?.id ? { completedAt: card.completedAt } : {}); setColumns((current) => current.map((column) => ({ ...column, cards: column.cards.map((item) => item.id === card.id ? { ...item, completedAt: card.completedAt } : item) }))); showToast(error instanceof Error ? error.message : 'Не удалось сохранить статус задачи'); });
    if (shouldReorder && typeof sourceList.id === 'string') {
      const targetColumn = columns.find((column) => column.id === sourceList.id);
      const remaining = (targetColumn?.cards ?? []).filter((item) => item.id !== card.id);
      const beforeCard = completedAt ? undefined : remaining.find((item) => Boolean(item.completedAt));
      void fetch(`${API_URL}/v1/cards/${card.id}/move`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ target_list_id: sourceList.id, before_card_id: beforeCard?.id ?? null }) })
        .then((response) => { if (!response.ok) throw new Error('move failed'); })
        .catch(() => showToast('Порядок выполненных задач не сохранён: обновите доску'));
    }
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
    setPriorityMotionKey((current) => current + 1);
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
    setColumns(data.lists.map((list) => ({ id: list.id, title: list.title, cards: list.cards.map((card) => ({ id: card.id, title: card.title, description: card.description, priority: card.priority, lastActivityAt: card.last_activity_at ?? undefined, isPublic: card.is_public, hasUnreadMentions: card.has_unread_mentions, backgroundImageUrl: card.background_image_url ?? undefined, dueAt: card.due_at ?? undefined, coverAttachmentId: card.cover_attachment_id ?? undefined, coverUrl: card.cover_url ?? undefined, coverMode: card.cover_mode, completedAt: card.completed_at ?? undefined, checklist: card.checklist_total ? `${card.checklist_completed}/${card.checklist_total}` : undefined, comments: card.comment_count || undefined, attachments: card.attachment_count || undefined, labels: card.labels, roles: card.roles ?? [], milestone: card.milestone ?? null, members: card.assignees.map(memberFromApi) })) })));
    setBoardLabels(data.labels);
    setMilestones(data.milestones ?? []);
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
      const board = await response.json() as ApiBoard;
      applyBoard(board);
      const workspace = workspaces.find((item) => item.id === board.workspace_id);
      if (workspace) { setWorkspaceName(workspace.name); if (typeof window !== 'undefined') window.localStorage.setItem('flowboard.workspace_id', workspace.id); setBoards(workspaceBoards[workspace.id] ?? []); }
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
      setWorkspaceBoards((current) => ({ ...current, [nextWorkspace.id]: nextBoards }));
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
      .then((board) => { setBoards((current) => [board, ...current]); setWorkspaceBoards((current) => ({ ...current, [workspaceId]: [board, ...(current[workspaceId] ?? [])] })); setNewBoardTitle(''); setNewBoardComposer(false); void selectBoard(board.id); })
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
  function openWorkspaceBackgroundEditor(workspace: Workspace) {
    setWorkspaceBackgroundEditorId(workspace.id);
    setWorkspaceBackgroundDraft(workspace.background_image_url ?? '');
  }
  function saveWorkspaceBackground(event: FormEvent) {
    event.preventDefault();
    if (!workspaceBackgroundEditorId || isSavingWorkspaceBackground) return;
    workspaceBackgroundFileRef.current?.click();
  }
  function uploadWorkspaceBackground(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file || !workspaceBackgroundEditorId || isSavingWorkspaceBackground) return;
    if (!['image/jpeg', 'image/png', 'image/gif', 'image/webp'].includes(file.type) || file.size > 50 * 1024 * 1024) { showToast('Фон должен быть JPEG, PNG, GIF или WebP до 50 МиБ'); return; }
    const workspaceIdToUpload = workspaceBackgroundEditorId;
    const form = new FormData(); form.append('file', file);
    setSavingWorkspaceBackground(true);
    void fetch(`${API_URL}/v1/workspaces/${workspaceIdToUpload}/background/file`, { method: 'POST', body: form })
      .then(async (response) => { if (!response.ok) throw new Error('workspace background upload failed'); return response.json() as Promise<{ url: string }>; })
      .then(({ url }) => { setWorkspaces((current) => current.map((workspace) => workspace.id === workspaceIdToUpload ? { ...workspace, background_image_url: url } : workspace)); setWorkspaceBackgroundDraft(`/v1/workspaces/${workspaceIdToUpload}/background/file`); setWorkspaceBackgroundEditorId(null); showToast('Фон карты пространства загружен'); })
      .catch(() => showToast('Не удалось загрузить фон карты пространства'))
      .finally(() => setSavingWorkspaceBackground(false));
  }
  function clearWorkspaceBackground() {
    if (!workspaceBackgroundEditorId || isSavingWorkspaceBackground) return;
    const workspaceIdToClear = workspaceBackgroundEditorId;
    setSavingWorkspaceBackground(true);
    void fetch(`${API_URL}/v1/workspaces/${workspaceIdToClear}/background`, { method: 'PUT', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ background_image_url: null }) })
      .then((response) => { if (!response.ok) throw new Error('workspace background clear failed'); setWorkspaces((current) => current.map((workspace) => workspace.id === workspaceIdToClear ? { ...workspace, background_image_url: null } : workspace)); setWorkspaceBackgroundDraft(''); showToast('Фон карты пространства снят'); })
      .catch(() => showToast('Не удалось снять фон карты пространства'))
      .finally(() => setSavingWorkspaceBackground(false));
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
    return <main className="app-shell dark"><div className="board-loading auth-loading" role="status"><span className="loading-dot" />Проверяем доступ к пространству</div></main>;
  }

  if (authState === 'signed-out') {
    const canRegister = registrationOpen || Boolean(inviteToken);
    const isRegistering = Boolean(inviteToken) || (authMode === 'register' && canRegister);
    return <main className="app-shell dark auth-shell"><section className="auth-card"><button className="brand auth-brand" type="button" onClick={() => setAuthMode('login')}><span className="brand-mark">✓</span><span>Flowboard</span></button><p className="eyebrow">FLOWBOARD</p><h1>{isRegistering ? inviteToken ? 'Активировать аккаунт' : 'Создать первый аккаунт' : 'С возвращением'}</h1><p className="auth-copy">{isRegistering ? inviteToken ? 'Выберите уникальный ник и пароль.' : 'Первый аккаунт станет system owner.' : 'Войдите по нику, чтобы продолжить.'}</p><form className="auth-form" onSubmit={submitAuth}><label>Ник<input value={authName} onChange={(event) => setAuthName(event.target.value)} maxLength={32} required autoComplete="username" placeholder="your_nick" /></label><label>Пароль<input type="password" value={authPassword} onChange={(event) => setAuthPassword(event.target.value)} minLength={10} maxLength={256} required autoComplete={isRegistering ? 'new-password' : 'current-password'} /></label>{authError && <p className="auth-error">{authError}</p>}<button className="create-button auth-submit" type="submit" disabled={isAuthorizing}>{isAuthorizing ? 'Подключаем…' : isRegistering ? inviteToken ? 'Активировать' : 'Создать аккаунт' : 'Войти'}</button></form></section></main>;
  }

  return <main className={`app-shell dark ${view === 'home' ? 'home-mode' : ''} ${isPublicViewer ? 'public-viewer' : ''} ${boardBackgroundUrl && view === 'board' ? 'has-board-background' : ''} ${!boardBackgroundUrl && view === 'board' ? 'default-board-background' : ''}`} style={boardBackgroundStyle}>
    <header className="topbar">
      <button className="brand" type="button" onClick={openHome} aria-label="Flowboard: перейти на главную"><span className="brand-mark">✓</span><span>Flowboard</span></button>
      <label className="search"><span>⌕</span><input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Поиск по задачам" aria-label="Поиск по задачам" /></label>
      <div className="top-actions">
        {account && <div className="notifications-control"><button className={`top-utility-button notification-trigger ${unreadNotificationCount ? 'has-unread' : ''}`} type="button" onClick={toggleNotifications} aria-label="Открыть уведомления" aria-expanded={isNotificationsOpen}>♢ <span>Уведомления</span>{unreadNotificationCount > 0 && <i>{unreadNotificationCount > 9 ? '9+' : unreadNotificationCount}</i>}</button>{isNotificationsOpen && <div className="notifications-popover" role="dialog" aria-label="Уведомления"><div className="popover-heading"><b>Уведомления</b>{unreadNotificationCount > 0 && <button type="button" className="text-action notification-mark-all" onClick={markAllNotificationsRead} title="Прочитать всё" aria-label="Прочитать всё"><svg viewBox="0 0 16 16" fill="none" aria-hidden="true"><path d="m1.75 8.25 3.05 3.05L9.45 5.5m-2.9 2.75L9.6 11.3l4.65-5.8" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" /></svg></button>}</div>{isNotificationsLoading ? <p className="empty-comments">Загружаем…</p> : notifications.length ? <div className="notification-list">{notifications.map((notification) => <button type="button" key={notification.id} className={notification.is_read ? 'read' : 'unread'} onClick={() => openNotification(notification)}><span>{notification.actor_name ? `@${notification.actor_name} · ` : ''}{notification.action}</span><b>{notification.card_title}</b>{notification.detail && <small>{notification.detail}</small>}<time>{new Date(notification.created_at).toLocaleString('ru-RU')}</time></button>)}</div> : <p className="empty-comments">Новых событий нет.</p>}</div>}</div>}
        {account && <button className="top-utility-button" type="button" onClick={openSessions} aria-label="Открыть сессии">◷ <span>Сессии</span></button>}
        {account?.user.is_system_owner && <button className="top-utility-button" type="button" onClick={openAdmin} aria-label="Открыть администрирование">⚙ <span>Админ</span></button>}
        {!isPublicViewer && <button className="create-button" onClick={() => { openBoard(); if (persistence !== 'connecting') { const firstColumn = columns[0]; if (firstColumn) setComposerOpen(firstColumn.id); else addColumn(); } }}>＋ Создать</button>}{account && <button className="profile-trigger" onClick={() => { setProfileOpen(true); setProfilePanel('overview'); setProfileName(account.user.username); setProfileError(''); }} aria-label="Открыть профиль"><ProfileAvatar account={account} member={currentMember} version={avatarVersion} /></button>}</div>
    </header>
    <input ref={workspaceBackgroundFileRef} className="workspace-background-file-input" type="file" accept="image/jpeg,image/png,image/gif,image/webp" onChange={uploadWorkspaceBackground} />

    <AmbientStarfall />

    {isProfileOpen && account && <div className="modal-backdrop" role="presentation" onMouseDown={() => setProfileOpen(false)}><section className="archive-modal profile-modal" role="dialog" aria-modal="true" aria-label="Профиль" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setProfileOpen(false)} aria-label="Закрыть профиль">×</button>{profilePanel === 'overview' ? <><header className="profile-modal-head"><ProfileAvatar account={account} member={currentMember} version={avatarVersion} /><div><p className="eyebrow">ПРОФИЛЬ</p><div className="profile-name-row"><h2>@{account.user.username}</h2><div className="profile-role-list">{profileRoles.filter((role) => myProfileRoleIds.includes(role.id)).map((role) => <ProfileRoleChip role={role} key={role.id} compact />)}<button className="profile-role-plus" type="button" onClick={() => setProfileRolePickerOpen((current) => !current)} aria-label="Выбрать роли">＋</button></div></div>{isProfileRolePickerOpen && <div className="profile-role-picker"><b>Мои роли</b>{profileRoles.length ? profileRoles.map((role) => <button type="button" key={role.id} className={myProfileRoleIds.includes(role.id) ? 'selected' : ''} onClick={() => toggleSelfProfileRole(role)}><ProfileRoleChip role={role} />{myProfileRoleIds.includes(role.id) && <span>✓</span>}</button>) : <p>System owner ещё не создал роли.</p>}</div>}</div></header><div className="profile-action-list"><button onClick={() => { setProfileName(account.user.username); setProfilePanel('username'); }}>Изменить ник <span>›</span></button><button onClick={() => setProfilePanel('password')}>Изменить пароль <span>›</span></button><label>Изменить аватар<input type="file" accept="image/jpeg,image/png,image/gif,image/webp" onChange={uploadProfileAvatar} disabled={isSavingProfile} /></label></div><button className="profile-signout" onClick={signOut}>Выйти из аккаунта</button></> : profilePanel === 'username' ? <><button className="text-action" onClick={() => setProfilePanel('overview')}>← Профиль</button><h2>Изменить ник</h2><form className="profile-form" onSubmit={saveProfileName}><label>Новый ник<input autoFocus value={profileName} onChange={(event) => setProfileName(event.target.value)} maxLength={32} /></label><div><button type="button" className="secondary-button" onClick={() => setProfilePanel('overview')}>Отмена</button><button className="create-button" type="submit" disabled={isSavingProfile}>Сохранить</button></div></form></> : <><button className="text-action" onClick={() => setProfilePanel('overview')}>← Профиль</button><h2>Изменить пароль</h2><form className="profile-form" onSubmit={changeProfilePassword}><label>Текущий пароль<input autoFocus type="password" value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} /></label><label>Новый пароль<input type="password" value={nextPassword} onChange={(event) => setNextPassword(event.target.value)} minLength={10} /></label><div><button type="button" className="secondary-button" onClick={() => setProfilePanel('overview')}>Отмена</button><button className="create-button" type="submit" disabled={isSavingProfile}>Сохранить</button></div></form></>}{profileError && <p className="profile-error">{profileError}</p>}</section></div>}

    {isWorkspaceComposerOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => !isCreatingWorkspace && setWorkspaceComposerOpen(false)}><section className="archive-modal workspace-create-modal" role="dialog" aria-modal="true" aria-label="Создать пространство" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" type="button" onClick={() => setWorkspaceComposerOpen(false)} disabled={isCreatingWorkspace} aria-label="Закрыть">×</button><p className="eyebrow">НОВОЕ ПРОСТРАНСТВО</p><h2>Создать пространство</h2><p className="archive-copy">Вы станете его owner и сможете добавить команду в настройках пространства.</p><form className="workspace-create-form" onSubmit={createWorkspace}><label htmlFor="workspace-name">Название</label><input id="workspace-name" autoFocus value={newWorkspaceName} onChange={(event) => { setNewWorkspaceName(event.target.value); setWorkspaceCreateError(''); }} maxLength={120} placeholder="Например, Маркетинг" disabled={isCreatingWorkspace} />{workspaceCreateError && <p className="form-error" role="alert">{workspaceCreateError}</p>}<div><button className="secondary-button" type="button" onClick={() => setWorkspaceComposerOpen(false)} disabled={isCreatingWorkspace}>Отмена</button><button className="create-button" type="submit" disabled={!newWorkspaceName.trim() || isCreatingWorkspace}>{isCreatingWorkspace ? 'Создаём…' : 'Создать пространство'}</button></div></form></section></div>}
    {isAdminOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setAdminOpen(false)}><section className="archive-modal team-modal admin-modal" role="dialog" aria-modal="true" aria-label="Администрирование" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setAdminOpen(false)} aria-label="Закрыть">×</button><p className="eyebrow">SYSTEM OWNER</p><h2>Администрирование</h2><button className="create-button admin-invite-button" type="button" onClick={createAccountInvite}>Создать account-invite</button>{isAdminLoading ? <p className="detail-loading">Загружаем данные…</p> : <><section className="admin-section"><h3>Аккаунты</h3><div className="team-list">{adminAccounts.map((item) => <article key={item.id}><Avatar member={memberFromApi({ id: item.id, username: item.username, avatar_url: item.avatar_url })} /><div><b>@{item.username}</b><small>{item.is_system_owner ? 'System owner' : 'Активен'}</small></div>{!item.is_system_owner && <button className="danger-action" onClick={() => deleteAccount(item)}>Удалить</button>}</article>)}</div></section><section className="admin-section"><h3>Пространства</h3><div className="team-list">{adminWorkspaces.map((item) => <article key={item.id}><div><b>{item.name}</b><small>Owner: @{item.owner_username} · {item.member_count} уч.</small></div><span className="workspace-admin-actions"><button onClick={() => archiveWorkspace(item)}>{item.archived_at ? 'Восстановить' : 'Архивировать'}</button><button className="danger-action" onClick={() => deleteWorkspace(item)}>Удалить</button></span></article>)}</div></section><section className="admin-section"><h3>Активные invite</h3><div className="team-list">{adminInvites.length ? adminInvites.map((item) => <article key={item.id}><div><b>Invite</b><small>до {new Date(item.expires_at).toLocaleString('ru-RU')}</small></div></article>) : <p className="empty-comments">Нет активных invite.</p>}</div></section></>}</section></div>}
    {isAdminOpen && account?.user.is_system_owner && <section className="profile-roles-admin-sheet" aria-label="Управление ролями"><h3>Роли</h3><p>Специализации для профилей и карточек.</p><div>{profileRoles.map((role) => <article key={role.id}><ProfileRoleChip role={role} /><span><button type="button" className="text-action" onClick={() => beginProfileRoleEdit(role)}>Изменить</button><button type="button" className="text-action danger-text" onClick={() => removeProfileRole(role)}>Удалить</button></span></article>)}</div><form className={`new-label-form profile-role-form ${isColorShakeActive ? 'is-shaking' : ''}`} onSubmit={saveProfileRole}><ChipNamePreview value={newProfileRoleName} onChange={setNewProfileRoleName} color={newProfileRoleColor} iconColor={newProfileRoleIconColor} shape={newProfileRoleShape} placeholder="Например, Программист" ariaLabel="Название роли" /><label title="Цвет роли"><input type="color" value={newProfileRoleColor} onChange={(event) => setNewProfileRoleColor(event.target.value)} aria-label="Цвет роли" /></label><ShapePicker value={newProfileRoleShape} onChange={setNewProfileRoleShape} label="Фигура роли" /><label title="Цвет фигуры"><input type="color" value={newProfileRoleIconColor} onChange={(event) => setNewProfileRoleIconColor(event.target.value)} aria-label="Цвет фигуры роли" /></label><button type="button" className="shake-colors-button" onClick={() => { setNewProfileRoleColor(randomChipColor()); setNewProfileRoleIconColor(randomChipColor()); setNewProfileRoleShape(roleShapes[Math.floor(Math.random() * roleShapes.length)]); triggerColorShake(); }}>Взболтнуть</button><button type="submit" disabled={isSavingProfileRole || !newProfileRoleName.trim()}>{editingProfileRole ? 'Сохранить' : 'Создать роль'}</button>{editingProfileRole && <button type="button" className="text-action" onClick={() => { setEditingProfileRole(null); setNewProfileRoleName(''); setNewProfileRoleColor('#6B7CFF'); setNewProfileRoleShape('circle'); setNewProfileRoleIconColor('#FFFFFF'); }}>Отмена</button>}</form></section>}
    {isSessionsOpen && <div className="modal-backdrop" role="presentation" onMouseDown={() => setSessionsOpen(false)}><section className="archive-modal team-modal security-modal" role="dialog" aria-modal="true" aria-label="Сессии" onMouseDown={(event) => event.stopPropagation()}><button className="modal-close" onClick={() => setSessionsOpen(false)} aria-label="Закрыть">×</button><p className="eyebrow">БЕЗОПАСНОСТЬ</p><h2>Активные сессии</h2><button className="secondary-button session-revoke-all" onClick={revokeOtherSessions}>Выйти на других устройствах</button><div className="team-list">{sessions.map((session) => <article key={session.id}><div><b>{session.current ? 'Это устройство' : 'Активная сессия'}</b><small>Последняя активность: {new Date(session.last_seen_at).toLocaleString('ru-RU')}</small></div>{!session.current && <button className="danger-action" onClick={() => revokeSession(session)}>Отозвать</button>}</article>)}</div></section></div>}

    {view === 'home' ? <section className="home-screen workspace-cards-screen">
      <div className="welcome"><p className="eyebrow">МОЯ РАБОТА</p><h1>{workspaces.length ? `${homeGreeting}, ${currentMember.name.split(' ')[0]}.` : 'Начните с пространства.'}</h1><p>{workspaces.length ? 'Проекты собраны внутри пространств — выберите нужную карту и продолжайте работу.' : 'Аккаунт существует отдельно от рабочих пространств. Создайте первое пространство или присоединитесь к уже созданному.'}</p>{workspaceId && boards.length > 0 && <button className="create-button" onClick={openBoard}>Открыть последнюю доску <span>→</span></button>}</div>
      <section className="workspace-cards-section" aria-label="Ваши пространства"><div className="section-title"><div><p className="eyebrow">ПРОСТРАНСТВА</p><h2>Ваши пространства</h2></div><button className="subtle-button" onClick={openWorkspaceComposer}>＋ Создать пространство</button></div>{workspaces.length ? <div className="workspace-cards">{workspaces.map((workspace) => { const isActive = workspace.id === workspaceId; const cardBoards = workspaceBoards[workspace.id] ?? []; return <article key={workspace.id} className={`workspace-card ${isActive ? 'active' : ''} ${workspace.background_image_url ? 'has-background' : ''}`} style={workspace.background_image_url ? { backgroundImage: `linear-gradient(105deg, rgb(15 19 25 / 70%), rgb(15 19 25 / 46%)), url("${assetUrl(workspace.background_image_url)}")` } : undefined}><header><button type="button" className="workspace-card-title" onClick={() => { if (!isActive) void selectWorkspace(workspace); }}><span className="workspace-option-icon">⌁</span><span><b>{workspace.name}</b><small>{isActive ? 'Открыто сейчас' : 'Открыть пространство'}</small></span></button><span className="workspace-card-actions">{workspace.can_manage && <button type="button" className="subtle-button" title="Фон карты пространства" onClick={() => openWorkspaceBackgroundEditor(workspace)}>▧ Фон</button>}{isActive && <button type="button" className="subtle-button" onClick={() => setNewBoardComposer((current) => !current)}>＋ Проект</button>}</span></header>{workspaceBackgroundEditorId === workspace.id && <form className="workspace-card-background-form" onSubmit={saveWorkspaceBackground}><input autoFocus value={workspaceBackgroundDraft} onChange={(event) => setWorkspaceBackgroundDraft(event.target.value)} placeholder="https://…/background.jpg" aria-label="Фон пространства по ссылке" /><button type="submit" disabled={isSavingWorkspaceBackground}>{isSavingWorkspaceBackground ? 'Сохраняем…' : 'Сохранить'}</button><button type="button" onClick={() => { setWorkspaceBackgroundDraft(''); }}>Снять</button><button type="button" onClick={() => setWorkspaceBackgroundEditorId(null)}>×</button></form>}{isActive && isNewBoardComposer && <form className="new-board-form workspace-card-new-board" onSubmit={createBoard}><input autoFocus value={newBoardTitle} onChange={(event) => setNewBoardTitle(event.target.value)} maxLength={200} placeholder="Название проекта" /><button className="create-button" type="submit" disabled={!newBoardTitle.trim() || isCreatingBoard}>{isCreatingBoard ? 'Создаём…' : 'Создать'}</button></form>}<div className="workspace-card-projects">{cardBoards.length ? cardBoards.map((board) => <button className="workspace-card-project" key={board.id} onClick={() => void selectBoard(board.id)}><span>⌁</span><b>{board.title}</b><i>→</i></button>) : <p>Проектов пока нет.</p>}</div>{workspace.can_manage && <button type="button" className="workspace-card-delete" onClick={() => deleteOwnedWorkspace(workspace)}>Удалить пространство</button>}</article>; })}</div> : <div className="empty-board-state"><b>Пространств пока нет.</b><span>Создайте первое пространство, когда будете готовы начать работу.</span></div>}</section>
    </section> : <>
      <section className="board-header">
        <div><button className="breadcrumbs" onClick={openHome}>{workspaceName} <span>/</span> {boardTitle}</button><div className="board-title-row">{isEditingBoardTitle ? <form className="board-title-form" onSubmit={(event) => { event.preventDefault(); saveBoardTitle(); }}><input autoFocus value={boardTitleDraft} onChange={(event) => setBoardTitleDraft(event.target.value)} maxLength={200} onKeyDown={(event) => { if (event.key === 'Escape') setEditingBoardTitle(false); }} /><button type="submit" disabled={isSavingBoardTitle}>✓</button></form> : <h1>{boardTitle}</h1>}<span className={`sync-status ${persistence}`}>{persistence === 'connected' ? 'Сохранено' : persistence === 'connecting' ? 'Подключение…' : 'Нет подключения'}</span><button className="title-edit" onClick={beginBoardRename} aria-label="Переименовать доску">✎</button></div></div>
        <div className="board-tools">
          <div className="board-members-control"><div className="avatars">{workspaceMembers.slice(0, 3).map((person) => <Avatar key={person.name} member={person} />)}{workspaceMembers.length > 3 && <button className="more-members" type="button" aria-label={`Показать всех участников: ${workspaceMembers.length}`} aria-expanded={isMembersPopoverOpen} onClick={() => setMembersPopoverOpen((current) => !current)}>+{workspaceMembers.length - 3}</button>}</div>{isMembersPopoverOpen && <div className="board-members-popover"><div className="popover-heading"><b>Участники проекта</b><span>{workspaceMembers.length}</span></div><div className="board-members-list">{workspaceMembers.map((person) => <div key={person.id}><Avatar member={person} /><span>@{person.name}</span></div>)}</div></div>}</div>
          <div className="board-content-toggle" role="group" aria-label="Группировка задач"><button type="button" className={boardContentMode === 'columns' ? 'active' : ''} onClick={() => setBoardContentMode('columns')}>Колонки</button><button type="button" className={boardContentMode === 'members' ? 'active' : ''} onClick={() => setBoardContentMode('members')}>По людям</button></div>
          {!isPublicViewer && boardContentMode === 'columns' && <div className="board-view-toggle" role="group" aria-label="Режим расположения колонок"><button type="button" className={boardViewMode === 'standard' ? 'active' : ''} onClick={() => changeBoardViewMode('standard')}>Ряд</button><button type="button" className={boardViewMode === 'freeform' ? 'active' : ''} onClick={() => changeBoardViewMode('freeform')} title="Колонки можно свободно двигать; Shift включает привязку к сетке">Свободно</button></div>}
          <div className="filter-control">
            <button className={`board-icon-button ${filterMode !== 'all' || cardSort !== 'manual' ? 'active-filter' : ''}`} type="button" title="Фильтры и сортировка" aria-label="Фильтры и сортировка" aria-expanded={isFilterOpen} onClick={() => setFilterOpen((current) => !current)}><BoardToolbarIcon type="filter" /></button>
          {isFilterOpen && <div className="filter-popover"><p>Показывать</p>{([['all', 'Все задачи'], ['assigned', 'Назначенные мне'], ['my_roles', 'По моим ролям'], ['due', 'С дедлайном'], ['overdue', 'Просроченные']] as [FilterMode, string][]).map(([mode, label]) => <button key={mode} className={filterMode === mode ? 'active' : ''} onClick={() => { setFilterMode(mode); setFilterOpen(false); }}>{label}{filterMode === mode && <b>✓</b>}</button>)}<p className="filter-popover-section">Порядок в колонках</p>{([['manual', 'Как на доске'], ['priority', 'Сначала важные'], ['activity', 'Недавно обновлённые']] as [CardSort, string][]).map(([mode, label]) => <button key={mode} className={cardSort === mode ? 'active' : ''} onClick={() => setCardSort(mode)}>{label}{cardSort === mode && <b>✓</b>}</button>)}</div>}
          </div>
          {renderBoardLabelsControl()}
          {renderMilestonesControl()}
          <button className="board-icon-button board-team-button" type="button" title="Команда проекта" aria-label="Команда проекта" onClick={openTeam}><BoardToolbarIcon type="team" /></button>
          <button className="board-icon-button board-archive-button" type="button" title="Архив задач" aria-label="Архив задач" onClick={openArchive}><BoardToolbarIcon type="archive" /></button>
          <div className="board-menu-control"><button className={`secondary-button more ${isBoardMenuOpen ? 'is-juggling' : ''}`} onClick={() => setBoardMenuOpen((current) => !current)} aria-expanded={isBoardMenuOpen} aria-label="Дополнительные действия"><span className="more-dots" aria-hidden="true"><i /><i /><i /></span></button>{isBoardMenuOpen && <div className="board-menu">{!isPublicViewer && <button onClick={() => { setBoardMenuOpen(false); openDiscordIntegration(); }}>⌁ Discord API</button>}<button onClick={exportCurrentBoard}>⇩ Экспорт JSON</button><button onClick={() => importFileRef.current?.click()}>⇧ Импорт Trello / Flowboard JSON</button><button className="danger-action" onClick={deleteCurrentBoard}>Удалить проект</button><input ref={importFileRef} type="file" accept="application/json,.json" onChange={importBoardFile} /><section className="visibility-control"><b>Доступ к доске</b><p>{boardVisibility === 'public' ? 'Public: любой аккаунт может только смотреть.' : 'Private: видят только участники проекта.'}</p><div><button type="button" className={boardVisibility === 'public' ? 'selected' : ''} onClick={() => changeBoardVisibility('public')}>Public · просмотр всем</button><button type="button" className={boardVisibility === 'private' ? 'selected' : ''} onClick={() => changeBoardVisibility('private')}>Private</button></div>{boardVisibility === 'public' && <button className="copy-public-link" type="button" onClick={copyPublicBoardLink}>Скопировать публичную ссылку</button>}</section><form onSubmit={saveBoardBackground}><label>Фон проекта по ссылке<input value={backgroundDraft} onChange={(event) => setBackgroundDraft(event.target.value)} placeholder="https://…/background.jpg" /></label><section className="background-display-control"><b>Отображение фона</b><div className="background-fit-options"><button type="button" className={boardBackgroundFit === 'cover' ? 'selected' : ''} onClick={() => setBoardBackgroundFit('cover')}>Заполнить</button><button type="button" className={boardBackgroundFit === 'contain' ? 'selected' : ''} onClick={() => setBoardBackgroundFit('contain')}>Целиком</button><button type="button" className={boardBackgroundFit === 'fill' ? 'selected' : ''} onClick={() => setBoardBackgroundFit('fill')}>Растянуть</button></div><div className="background-position-options"><button type="button" className={boardBackgroundPosition === 'top' ? 'selected' : ''} onClick={() => setBoardBackgroundPosition('top')}>↑ Верх</button><button type="button" className={boardBackgroundPosition === 'center' ? 'selected' : ''} onClick={() => setBoardBackgroundPosition('center')}>⊙ Центр</button><button type="button" className={boardBackgroundPosition === 'bottom' ? 'selected' : ''} onClick={() => setBoardBackgroundPosition('bottom')}>↓ Низ</button></div><small>«Целиком» сохраняет изображение без обрезки, «Растянуть» подгоняет его под экран.</small></section><div><button type="submit" disabled={isSavingBackground}>{isSavingBackground ? 'Сохраняем…' : 'Сохранить фон'}</button><button type="button" onClick={() => { setBackgroundDraft(''); }}>Снять</button></div></form><input ref={boardBackgroundFileRef} type="file" accept="image/jpeg,image/png,image/gif,image/webp" onChange={uploadBoardBackground} /><button type="button" onClick={() => boardBackgroundFileRef.current?.click()} disabled={isUploadingBoardBackground}>{isUploadingBoardBackground ? 'Загружаем фон…' : '▧ Загрузить фон проекта'}</button></div>}</div></div>
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
      <section className={`board ${isBoardPanning ? 'board-panning' : ''} ${boardViewMode === 'freeform' ? 'board-freeform' : ''}`} ref={boardRef} aria-label="Канбан-доска" onPointerDown={startBoardPan} onPointerMove={(event) => { moveBoardPan(event); updateFreeformCursor(event); }} onPointerUp={stopBoardPan} onPointerCancel={stopBoardPan} onScroll={(event) => { if (boardViewMode === 'freeform') { const board = event.currentTarget; setFreeformViewport({ x: board.scrollLeft / freeformZoom, y: board.scrollTop / freeformZoom, width: board.clientWidth / freeformZoom, height: board.clientHeight / freeformZoom }); } }}>
        {persistence === 'connecting' ? <div className="board-loading" role="status"><span className="loading-dot" />Загружаем вашу доску</div> : boardContentMode === 'members' ? renderMemberBoard() : <div ref={freeformCanvasRef} className={boardViewMode === 'freeform' ? 'freeform-canvas' : 'board-columns'} style={boardViewMode === 'freeform' ? { width: freeformCanvasSize.width * freeformZoom, height: freeformCanvasSize.height * freeformZoom } : undefined} onContextMenu={(event) => { if (boardViewMode !== 'freeform' || isPublicViewer || (event.target instanceof Element && event.target.closest('.column, .freeform-detached-card'))) return; const point = getFreeformPoint(event.clientX, event.clientY); if (!point) return; event.preventDefault(); setFreeformContextMenu({ x: Math.min(event.clientX, window.innerWidth - 230), y: Math.min(event.clientY, window.innerHeight - 170), position: point }); }} onDragOver={(event) => { if (boardViewMode === 'freeform' && dragging && !(event.target instanceof Element && event.target.closest('.column, .freeform-detached-card'))) event.preventDefault(); }} onDrop={detachDraggedCard}>
          <div className={boardViewMode === 'freeform' ? 'freeform-scene' : undefined} style={boardViewMode === 'freeform' ? { width: freeformCanvasSize.width, height: freeformCanvasSize.height, transform: `scale(${freeformZoom})` } : { display: 'contents' }}>
          {boardViewMode === 'freeform' && <><svg className={`freeform-ink ${isFreeformDrawing || isFreeformErasing ? 'active' : ''} ${isFreeformErasing ? 'erasing' : ''}`} width={freeformCanvasSize.width} height={freeformCanvasSize.height} viewBox={`0 0 ${freeformCanvasSize.width} ${freeformCanvasSize.height}`} onPointerDown={startFreeformInk} onPointerMove={continueFreeformInk} onPointerUp={finishFreeformInk} onPointerCancel={finishFreeformInk}>{freeformDrawing.strokes.map((stroke, index) => <polyline key={stroke.id ?? index} points={stroke.points.map((point) => `${point.x},${point.y}`).join(' ')} fill="none" stroke={stroke.color} strokeWidth={stroke.width} strokeLinecap="round" strokeLinejoin="round" />)}</svg>{freeformLive.cursors.filter((cursor) => cursor.user_id !== account?.user.id).map((cursor) => <div className="freeform-cursor" key={cursor.user_id} style={{ left: cursor.x, top: cursor.y }}><span>⌖</span><b>@{cursor.username}</b></div>)}{freeformLive.pings.map((ping) => <span className="freeform-ping" key={ping.id} style={{ left: ping.x, top: ping.y }} title={`@${ping.username} зовёт сюда`}><i />@{ping.username}</span>)}{freeformLive.pings.map((ping) => { const dx = ping.x - freeformViewport.x; const dy = ping.y - freeformViewport.y; if (dx >= 0 && dx <= freeformViewport.width && dy >= 0 && dy <= freeformViewport.height) return null; const horizontal = Math.abs(dx - freeformViewport.width / 2) > Math.abs(dy - freeformViewport.height / 2); const arrow = horizontal ? dx < 0 ? '←' : '→' : dy < 0 ? '↑' : '↓'; return <span className="freeform-ping-direction" key={`${ping.id}-direction`} style={{ left: freeformViewport.x + Math.max(16, Math.min(Math.max(16, freeformViewport.width - 150), dx)), top: freeformViewport.y + Math.max(16, Math.min(Math.max(16, freeformViewport.height - 38), dy)) }}>{arrow} @{ping.username}</span>; })}</>}
          {boardViewMode === 'freeform' && freeformDetachedCards.map(({ card, listId, position }) => <article className={`task-card freeform-detached-card ${card.completedAt ? 'completed' : ''} ${labelsCollapsed ? 'labels-collapsed' : ''} ${dragging?.cardId === card.id ? 'dragging' : ''}`} key={`detached-${card.id}`} style={{ left: position.x, top: position.y }} draggable={!isPublicViewer} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setCardContextMenu({ card, x: Math.min(event.clientX, window.innerWidth - 210), y: Math.min(event.clientY, window.innerHeight - 170) }); }} onDragStart={(event) => { didDragRef.current = false; event.dataTransfer.setData('application/x-flowboard-card', String(card.id)); event.dataTransfer.effectAllowed = 'move'; setDragging({ cardId: card.id, sourceListId: listId }); }} onDragEnd={() => { didDragRef.current = true; clearDragState(); window.setTimeout(() => { didDragRef.current = false; }, 0); }} onDragOver={(event) => { if (dragging) event.preventDefault(); }} onDrop={(event) => { if (!dragging || isPublicViewer) return; event.preventDefault(); event.stopPropagation(); const point = getFreeformPoint(event.clientX, event.clientY); if (point) setFreeformCardPosition(dragging.cardId, point); clearDragState(); }} onClick={() => { if (!didDragRef.current) openCard(card); }}>
            {card.hasUnreadMentions && <span className="card-mention-dot" title="Вас упомянули в этой карточке" />}{card.coverUrl && <div className={`card-cover ${card.coverMode ?? 'full'}`}><img src={assetUrl(card.coverUrl)} alt="" /></div>}<div className="card-main">{(card.labels.length > 0 || card.roles.length > 0) && <div className="card-top"><div className="card-labels">{card.labels.map((label) => <LabelChip label={label} key={label.id} />)}{card.roles.map((role) => <ProfileRoleChip role={role} key={role.id} compact />)}</div></div>}<div className="card-title-row"><button className="card-complete" aria-label={card.completedAt ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(card.completedAt)} onClick={(event) => toggleCardCompletion(card, event)}>{card.completedAt && '✓'}</button><h3>{card.title}</h3></div>{card.dueAt && <p className={`due ${new Date(card.dueAt).getTime() < Date.now() ? 'today' : ''}`}>◷ {formatDue(card.dueAt)}</p>}</div>
            {card.priority ? <span className="card-priority-corner" style={{ right: card.members.length ? 96 : 14 }}><PrioritySignal priority={card.priority} /></span> : null}{(card.checklist || card.comments || card.attachments || card.members.length > 0) && <footer className="card-footer"><div className="card-meta">{card.checklist && <span className={isChecklistComplete(card.checklist) ? 'checklist-complete' : ''}><CardMetaIcon type="checklist" />{card.checklist}</span>}{card.comments && <span><CardMetaIcon type="comments" />{card.comments}</span>}{card.attachments && <span title="Есть вложения"><CardMetaIcon type="attachments" /></span>}</div><div className="card-avatars">{card.members.map((member) => <Avatar key={member.id} member={member} />)}</div></footer>}
          </article>)}
          {renderedColumns.map((column, index) => <section data-list-id={String(column.id)} className={`column ${dragOverListId === column.id ? 'drag-target' : ''} ${draggingColumnId === column.id ? 'column-dragging' : ''}`} key={column.id} aria-label={column.title} style={boardViewMode === 'freeform' ? (() => { const position = freeformLayout[String(column.id)] ?? { x: index * 336, y: 0 }; return { left: position.x, top: position.y }; })() : undefined} onContextMenu={(event) => { if (isPublicViewer || (event.target instanceof Element && event.target.closest('.task-card'))) return; event.preventDefault(); event.stopPropagation(); setColumnContextMenu({ column, x: Math.min(event.clientX, window.innerWidth - 210), y: Math.min(event.clientY, window.innerHeight - 150) }); }} onDragEnter={() => { if (dragging) setDragOverListId(column.id); }} onDragOver={(event) => { moveCardDragPreview(event); event.preventDefault(); event.dataTransfer.dropEffect = 'move'; if (draggingColumnId && boardViewMode === 'standard') { const bounds = event.currentTarget.getBoundingClientRect(); const next = event.clientX > bounds.left + bounds.width / 2 ? renderedColumns[index + 1] : column; setColumnDropBeforeId(next?.id ?? null); updateBoardAutoScroll(event); return; } const cardTarget = event.target instanceof Element ? event.target.closest('.task-card') : null; if (!cardTarget) setDragDropTarget({ listId: column.id, beforeCardId: null }); updateCardListAutoScroll(event); }} onDragLeave={(event) => { if (!event.currentTarget.contains(event.relatedTarget as Node)) { setDragOverListId(null); setDragDropTarget(null); stopCardListAutoScroll(); } }} onDrop={(event) => { event.preventDefault(); event.stopPropagation(); if (draggingColumnId && boardViewMode === 'standard') { moveColumn(draggingColumnId, columnDropBeforeId ?? undefined); return; } const beforeCardId = dragDropTarget?.listId === column.id ? dragDropTarget.beforeCardId ?? undefined : undefined; if (dragging) moveCard(dragging.cardId, dragging.sourceListId, column.id, beforeCardId); }}>
          <div className="column-head column-drag-handle" draggable={!isPublicViewer && boardViewMode === 'standard'} onPointerDown={(event) => beginFreeformColumnDrag(event, column, index)} onPointerMove={moveFreeformColumnDrag} onPointerUp={endFreeformColumnDrag} onPointerCancel={endFreeformColumnDrag} onDragStart={(event) => { if (boardViewMode !== 'standard') { event.preventDefault(); return; } event.stopPropagation(); event.dataTransfer.setData('application/x-flowboard-column', String(column.id)); event.dataTransfer.effectAllowed = 'move'; event.dataTransfer.setDragImage(event.currentTarget, 30, 22); setDraggingColumnId(column.id); }} onDragEnd={clearColumnDragState}><span className="column-drag-icon" aria-hidden="true">⠿</span><div>{editingColumnId === column.id ? <form className="column-rename" onSubmit={(event) => { event.preventDefault(); saveColumnTitle(column.id); }}><input autoFocus maxLength={200} value={columnTitleDraft} onChange={(event) => setColumnTitleDraft(event.target.value)} onKeyDown={(event) => { if (event.key === 'Escape') setEditingColumnId(null); }} aria-label="Название колонки" /><button type="submit" disabled={isSavingColumn}>✓</button></form> : <><h2>{column.title}</h2><span>{column.cards.length}</span></>}</div><div className="column-actions"><button className="column-menu" aria-label={`Меню колонки ${column.title}`} onClick={() => setColumnMenuId((current) => current === column.id ? null : column.id)}>•••</button>{columnMenuId === column.id && <div className="column-popover"><button onClick={() => beginColumnRename(column)}>Переименовать</button><button className="danger-action" onClick={() => deleteColumn(column)}>Удалить пустую</button></div>}</div></div>
          <div className={`card-list ${dragDropTarget?.listId === column.id && dragDropTarget.beforeCardId === null ? 'drop-at-end' : ''}`} style={dragDropTarget?.listId === column.id ? { '--card-drag-gap': `${cardDragPreview?.height ?? 84}px` } as CSSProperties : undefined}>{column.cards.map((card) => <article data-card-id={String(card.id)} className={`task-card ${card.completedAt ? 'completed' : ''} ${labelsCollapsed ? 'labels-collapsed' : ''} ${dragging?.cardId === card.id ? 'dragging' : ''} ${dragDropTarget?.listId === column.id && dragDropTarget.beforeCardId === card.id ? 'drop-before' : ''} ${cardMoveMotion?.cardId === String(card.id) ? 'card-settling' : ''}`} key={card.id} draggable={!isPublicViewer && boardViewMode === 'freeform'} onPointerDown={(event) => beginPointerCardDrag(event, card, column.id)} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); setCardContextMenu({ card, x: Math.min(event.clientX, window.innerWidth - 210), y: Math.min(event.clientY, window.innerHeight - 170) }); }} onDragStart={(event) => { didDragRef.current = false; event.dataTransfer.setData('application/x-flowboard-card', String(card.id)); event.dataTransfer.effectAllowed = 'move'; beginCardDragPreview(event, card); setDragging({ cardId: card.id, sourceListId: column.id }); setDragDropTarget(null); }} onDrag={(event) => moveCardDragPreview(event)} onDragEnd={() => { didDragRef.current = true; clearDragState(); window.setTimeout(() => { didDragRef.current = false; }, 0); }} onDragOver={(event) => { moveCardDragPreview(event); event.preventDefault(); event.dataTransfer.dropEffect = 'move'; if (draggingColumnId || !dragging || dragging.cardId === card.id) return; const bounds = event.currentTarget.getBoundingClientRect(); const cardIndex = column.cards.findIndex((item) => item.id === card.id); const nextCard = event.clientY > bounds.top + bounds.height / 2 ? column.cards[cardIndex + 1] : card; setDragOverListId(column.id); setDragDropTarget({ listId: column.id, beforeCardId: nextCard?.id ?? null }); updateCardListAutoScroll(event); }} onDrop={(event) => { event.preventDefault(); event.stopPropagation(); if (draggingColumnId && boardViewMode === 'standard') { moveColumn(draggingColumnId, columnDropBeforeId ?? undefined); return; } const beforeCardId = dragDropTarget?.listId === column.id ? dragDropTarget.beforeCardId ?? undefined : card.id; if (!dragging || dragging.cardId === card.id || beforeCardId === dragging.cardId) { clearDragState(); return; } moveCard(dragging.cardId, dragging.sourceListId, column.id, beforeCardId); }} onClick={() => { if (!didDragRef.current) openCard(card); }}>
            {card.hasUnreadMentions && <span className="card-mention-dot" title="Вас упомянули в этой карточке" aria-label="Вас упомянули в этой карточке" />}{card.coverUrl && <div className={`card-cover ${card.coverMode ?? 'full'}`}><img src={assetUrl(card.coverUrl)} alt="" /></div>}<div className="card-main">{(card.labels.length > 0 || card.roles.length > 0) && <div className="card-top"><div className="card-labels">{card.labels.map((label) => <LabelChip label={label} key={label.id} asButton onClick={(event) => { event.stopPropagation(); setLabelsCollapsed((current) => !current); }} />)}{card.roles.map((role) => <ProfileRoleChip role={role} key={role.id} compact />)}</div></div>}<div className="card-title-row"><button className="card-complete" aria-label={card.completedAt ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(card.completedAt)} onClick={(event) => toggleCardCompletion(card, event)}>{card.completedAt && '✓'}</button><h3>{card.title}</h3></div>{card.dueAt && <p className={`due ${new Date(card.dueAt).getTime() < Date.now() ? 'today' : ''}`}>◷ {formatDue(card.dueAt)}</p>}</div>
            {card.priority ? <span className="card-priority-corner" style={{ right: card.members.length ? 96 : 14 }}><PrioritySignal priority={card.priority} /></span> : null}
            {(card.checklist || card.comments || card.attachments || card.members.length > 0) && <footer className="card-footer"><div className="card-meta">{card.checklist && <span className={isChecklistComplete(card.checklist) ? 'checklist-complete' : ''}><CardMetaIcon type="checklist" />{card.checklist}</span>}{card.comments && <span><CardMetaIcon type="comments" />{card.comments}</span>}{card.attachments && <span title="Есть вложения"><CardMetaIcon type="attachments" /></span>}</div><div className="card-avatars">{card.members.map((member) => <Avatar key={member.id} member={member} />)}</div></footer>}
          </article>)}</div>
          {isComposerOpen === column.id ? <form className="composer" onSubmit={(event) => addCard(event, column.id)}><textarea autoFocus value={draft} onChange={(event) => setDraft(event.target.value)} placeholder="Название задачи" /><div><button className="add-card" type="submit">Добавить</button><button className="cancel" type="button" onClick={() => { setComposerOpen(null); setDraft(''); }}>Отмена</button></div></form> : <button className="add-task" onClick={() => setComposerOpen(column.id)}>＋ Добавить задачу</button>}
        </section>)}
        {boardViewMode !== 'freeform' && <button className="add-column" onClick={() => addColumn()}>＋ Добавить колонку</button>}</div></div>}
      </section>
      {freeformContextMenu && !isPublicViewer && <div className="freeform-context-menu" style={{ left: freeformContextMenu.x, top: freeformContextMenu.y }} role="menu"><b>Свободная доска</b><button type="button" onClick={() => { addColumn(freeformContextMenu.position); setFreeformContextMenu(null); }}>＋ Создать колонку здесь</button><button type="button" onClick={() => { publishFreeformCursor(freeformContextMenu.position, true); setFreeformContextMenu(null); showToast('Метка показана участникам на 5 секунд'); }}>⌁ Дать метку · 5 сек</button><button type="button" onClick={() => { setFreeformErasing(false); setFreeformDrawingMode(true); setFreeformContextMenu(null); showToast('Рисование включено'); }}>✎ Рисовать</button></div>}
      {boardViewMode === 'freeform' && !isPublicViewer && <div className="freeform-drawing-toolbar"><button type="button" className={isFreeformDrawing ? 'active' : ''} onClick={() => { setFreeformErasing(false); setFreeformDrawingMode((current) => !current); }}>✎ {isFreeformDrawing ? 'Рисование' : 'Рисовать'}</button><button type="button" className={isFreeformErasing ? 'active' : ''} onClick={() => { setFreeformDrawingMode(false); setFreeformErasing((current) => !current); }}>⌫ Ластик</button><span className="freeform-zoom">{Math.round(freeformZoom * 100)}%</span><button type="button" onClick={() => setFreeformZoom(1)}>100%</button>{(isFreeformDrawing || isFreeformErasing) && <><input type="color" value={freeformInkColor} onChange={(event) => setFreeformInkColor(event.target.value)} aria-label="Цвет кисти" /><select value={freeformInkWidth} onChange={(event) => setFreeformInkWidth(Number(event.target.value))} aria-label="Толщина кисти"><option value="2">Тонкая</option><option value="4">Средняя</option><option value="7">Толстая</option></select></>}<button type="button" onClick={clearOwnFreeformInk}>Стереть мои линии</button></div>}
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
              <div className="card-quick-actions"><span className="card-quick-actions-left">{renderCardMilestoneControl()}<button className={`quick-action ${sidebarPanel === 'labels' ? 'active' : ''}`} onClick={() => { setExistingLabelsOnly(false); setSidebarPanel((current) => current === 'labels' ? null : 'labels'); }} title="Метки" aria-label="Настроить метки"><BoardToolbarIcon type="labels" /></button><button className={`quick-action ${sidebarPanel === 'background' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'background' ? null : 'background')} title="Фон карточки" aria-label="Настроить фон карточки">▧</button><button className={`quick-action ${sidebarPanel === 'public-visibility' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'public-visibility' ? null : 'public-visibility')} title="Доступ" aria-label="Настроить доступ к карточке">◉</button></span><span className="card-quick-actions-right">{authState === 'signed-in' && <button type="button" className={`quick-action card-watch-toggle ${isWatchingCard ? 'active' : ''}`} onClick={toggleCardWatch} title={isWatchingCard ? 'Отключить уведомления об этой карточке' : 'Подписаться на изменения карточки'} aria-label={isWatchingCard ? 'Отключить уведомления об этой карточке' : 'Подписаться на изменения карточки'}>{isWatchingCard ? '◉' : '◌'}</button>}<button className={`quick-action ${sidebarPanel === 'due' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'due' ? null : 'due')} title={selected.dueAt ? `Дедлайн: ${formatDue(selected.dueAt)}` : 'Дедлайн'} aria-label="Настроить дедлайн">◷</button><button className="quick-action" onClick={openDiagram} title="Схема" aria-label="Открыть схему">⌁</button></span></div>
              {sidebarPanel && sidebarPanel !== 'assignees' && <div className="property-popover quick-property-popover" role="dialog" aria-label="Настройки карточки">
                {sidebarPanel === 'labels' && <><div className="popover-heading"><b>Метки</b><button onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><div className="label-options">{boardLabels.map((label) => <button key={label.id} className={`label-option ${selected.labels.some((current) => current.id === label.id) ? 'selected' : ''}`} style={{ borderColor: label.color, backgroundColor: `${label.color}22` }} onClick={() => toggleSelectedLabel(label)}><i style={{ backgroundColor: label.color }} /><span>{label.name}</span>{selected.labels.some((current) => current.id === label.id) && <b>✓</b>}</button>)}</div>{!existingLabelsOnly && <form className="new-label-form" onSubmit={createLabel}><input value={newLabelName} onChange={(event) => setNewLabelName(event.target.value)} maxLength={60} placeholder="Новая метка" aria-label="Название новой метки" /><input type="color" value={newLabelColor} onChange={(event) => setNewLabelColor(event.target.value)} aria-label="Цвет метки" /><button type="submit" disabled={!newLabelName.trim() || isSavingLabel}>{isSavingLabel ? 'Создаём…' : 'Создать метку'}</button></form>}</>}
                {sidebarPanel === 'due' && <div className="date-panel"><div className="popover-heading"><b>Дедлайн</b><button onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><div className="calendar-head"><button onClick={() => setDueCursor((current) => new Date(current.getFullYear(), current.getMonth() - 1, 1))} aria-label="Предыдущий месяц">‹</button><strong>{monthNames[dueCursor.getMonth()]} {dueCursor.getFullYear()}</strong><button onClick={() => setDueCursor((current) => new Date(current.getFullYear(), current.getMonth() + 1, 1))} aria-label="Следующий месяц">›</button></div><div className="calendar-weekdays">{weekdayNames.map((day) => <span key={day}>{day}</span>)}</div><div className="calendar-grid">{dueDays.map((day) => <button key={day.toISOString()} className={`${day.getMonth() !== dueCursor.getMonth() ? 'outside' : ''} ${selected.dueAt && isSameDay(day, new Date(selected.dueAt)) ? 'chosen' : ''} ${isSameDay(day, new Date()) ? 'today' : ''}`} onClick={() => saveDueDate(day, dueTime)}>{day.getDate()}</button>)}</div><div className="time-options">{dueTimeOptions.map((time) => <button key={time} className={dueTime === time ? 'selected' : ''} onClick={() => { setDueTime(time); if (selected.dueAt) saveDueDate(new Date(selected.dueAt), time); }}>{time}</button>)}</div>{selected.dueAt && <button className="clear-deadline" onClick={clearDueDate}>Снять дедлайн</button>}</div>}
                {sidebarPanel === 'background' && <div className="card-background-form"><div className="popover-heading"><b>Фон карточки</b><button type="button" onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><p>Загрузите изображение с компьютера — оно сохранится в Flowboard.</p><input ref={cardBackgroundFileRef} type="file" accept="image/jpeg,image/png,image/gif,image/webp" onChange={uploadCardBackground} /><div><button className="secondary-button" type="button" onClick={clearCardBackground}>Снять</button><button className="create-button" type="button" onClick={() => cardBackgroundFileRef.current?.click()} disabled={isUploadingCardBackground}>{isUploadingCardBackground ? 'Загружаем…' : 'Выбрать файл'}</button></div></div>}
                {sidebarPanel === 'public-visibility' && <div className="card-public-visibility"><div className="popover-heading"><b>Видимость карточки</b><button type="button" onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><label><input type="checkbox" checked={selected.isPublic ?? true} onChange={(event) => setSelectedCardPublicVisibility(event.target.checked)} /> Видна гостям</label><p>Снимите галочку, чтобы карточка и её вложения были доступны только после входа в аккаунт.</p></div>}
                {sidebarPanel === 'roles' && <><div className="popover-heading"><b>Роли карточки</b><button onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><div className="label-options">{profileRoles.map((role) => <button key={role.id} className={`label-option ${selected.roles.some((current) => current.id === role.id) ? 'selected' : ''}`} style={{ borderColor: role.color, backgroundColor: `${role.color}22` }} onClick={() => toggleSelectedProfileRole(role)}><ShapeIcon shape={role.icon_shape} color={role.icon_color ?? role.color} /><span>{role.name}</span>{selected.roles.some((current) => current.id === role.id) && <b>✓</b>}</button>)}</div>{!profileRoles.length && <p className="empty-comments">System owner ещё не создал роли.</p>}</>}
              </div>}
              <div className="detail-title-row"><button className={`detail-card-complete ${selected.completedAt ? 'done' : ''}`} aria-label={selected.completedAt ? 'Вернуть задачу в работу' : 'Отметить задачу выполненной'} aria-pressed={Boolean(selected.completedAt)} onClick={(event) => toggleCardCompletion(selected, event)}>{selected.completedAt && '✓'}</button><input className="card-title-input" value={cardTitleDraft} onChange={(event) => setCardTitleDraft(event.target.value)} aria-label="Название задачи" /></div>
              <section className="card-priority-editor" aria-label="Приоритет задачи"><span>Приоритет</span><div><button type="button" className={(selected.priority ?? 0) === 0 ? 'selected' : ''} onClick={() => setSelectedCardPriority(0)}>Нет</button>{[1, 2, 3, 4, 5].map((level) => <button type="button" className={(selected.priority ?? 0) === level ? 'selected' : ''} onClick={() => setSelectedCardPriority(level)} key={level}><PrioritySignal key={`${priorityMotionKey}-${level}`} priority={level} wave={priorityMotionKey > 0 && (selected.priority ?? 0) === level} /></button>)}</div></section>
              <div className="card-members-labels-row"><div className="card-members-row"><span>Исполнители</span><div className="card-member-control">{selected.members.map((member) => <Avatar member={member} key={member.id} />)}<button className={`member-plus ${sidebarPanel === 'assignees' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'assignees' ? null : 'assignees')} aria-label="Назначить исполнителя">＋</button>{sidebarPanel === 'assignees' && <div className="property-popover assignees-popover" role="dialog" aria-label="Выбор исполнителей"><div className="popover-heading"><b>Исполнители</b><button onClick={() => setSidebarPanel(null)} aria-label="Закрыть">×</button></div><div className="member-options">{workspaceMembers.map((member) => <button key={member.id} className={`member-option ${selected.members.some((current) => current.id === member.id) ? 'selected' : ''}`} onClick={() => toggleSelectedMember(member)}><Avatar member={member} /><span>{member.name}</span>{selected.members.some((current) => current.id === member.id) && <b>✓</b>}</button>)}</div><p className="empty-comments">Состав пространства меняется в разделе «Команда».</p></div>}</div></div><div className="detail-card-labels"><span>Метки</span><div className="card-labels">{selected.labels.map((label) => <LabelChip label={label} key={label.id} />)}<button className={`label-plus ${sidebarPanel === 'labels' ? 'active' : ''}`} onClick={() => { setExistingLabelsOnly(true); setSidebarPanel((current) => current === 'labels' ? null : 'labels'); }} aria-label="Добавить существующую метку">＋</button></div></div><div className="detail-card-labels"><span>Роли</span><div className="card-labels">{selected.roles.map((role) => <ProfileRoleChip role={role} key={role.id} />)}{!isPublicViewer && <button className={`label-plus ${sidebarPanel === 'roles' ? 'active' : ''}`} onClick={() => setSidebarPanel((current) => current === 'roles' ? null : 'roles')} aria-label="Добавить роль карточке">＋</button>}</div></div></div>
            </div>
            <section className="description-section"><div className="section-heading"><h3>Описание</h3></div>{isEditingCardDescription ? <MentionTextarea autoFocus className={`card-description-input media-drop-target ${isUploadingAttachment ? 'uploading' : ''} ${unreadMentionSourceIds.includes(String(selected.id)) ? 'mention-highlight' : ''}`} value={cardDescriptionDraft} onValueChange={setCardDescriptionDraft} onBlur={() => setEditingCardDescription(false)} members={account ? workspaceMembers : []} onDragOver={(event) => { if (event.dataTransfer.types.includes('Files')) event.preventDefault(); }} onDrop={(event) => handleMediaDrop(event, 'description')} onPaste={(event) => handleMediaPaste(event, 'description')} placeholder="Добавьте описание или перетащите изображение/видео…" ariaLabel="Описание задачи" /> : <div className={`markdown-editable-description ${unreadMentionSourceIds.includes(String(selected.id)) ? 'mention-highlight' : ''}`} role={isPublicViewer ? undefined : 'button'} tabIndex={isPublicViewer ? undefined : 0} onClick={() => { if (!isPublicViewer) setEditingCardDescription(true); }} onKeyDown={(event) => { if (!isPublicViewer && (event.key === 'Enter' || event.key === ' ')) { event.preventDefault(); setEditingCardDescription(true); } }}><MarkdownDescription value={cardDescriptionDraft} highlightMentions={unreadMentionSourceIds.includes(String(selected.id))} /></div>}</section>
            </div>
            <section className="checklists"><div className="section-heading"><h3>Чек-листы</h3><span>{checklists.length || '—'}</span></div>{isDetailsLoading ? <p className="detail-loading">Загружаем чек-листы…</p> : <>{checklists.map((checklist) => { const completed = checklist.items.filter((item) => item.is_completed).length; const itemIds = checklist.items.map((item) => String(item.id)); const allExpanded = itemIds.length > 0 && itemIds.every((id) => expandedChecklistItemIds.includes(id)); return <section className="checklist" key={checklist.id}><div className="section-heading"><h4>{checklist.title}</h4><span>{completed}/{checklist.items.length}</span><button className="text-action checklist-all-toggle" type="button" title={allExpanded ? 'Свернуть все детали' : 'Раскрыть все детали'} onClick={() => setExpandedChecklistItemIds((current) => allExpanded ? current.filter((id) => !itemIds.includes(id)) : [...new Set([...current, ...itemIds])])}>{allExpanded ? '⌃ Все' : '⌄ Все'}</button><button className="text-action danger-text" onClick={() => deleteChecklist(checklist)}>Удалить</button></div><div className="progress"><i style={{ width: `${checklist.items.length ? completed / checklist.items.length * 100 : 0}%` }} /></div>{checklists.map((checklist) => checklist).filter((currentChecklist) => currentChecklist.id === checklist.id).flatMap((currentChecklist) => currentChecklist.items).map((item) => { const itemId = String(item.id); const isExpanded = expandedChecklistItemIds.includes(itemId); return <div className="checklist-item" key={item.id}><div className="check-row"><button className={`check-item ${item.is_completed ? 'done' : ''}`} onClick={() => toggleChecklistItem(checklist.id, item)} aria-pressed={item.is_completed}><span className="check-control">{item.is_completed && '✓'}</span>{item.title}</button><button className={`check-item-toggle ${isExpanded ? 'open' : ''}`} type="button" title={isExpanded ? 'Скрыть детали пункта' : 'Раскрыть детали пункта'} aria-expanded={isExpanded} onClick={() => setExpandedChecklistItemIds((current) => current.includes(itemId) ? current.filter((id) => id !== itemId) : [...current, itemId])}>⌄</button><button className="remove-check" onClick={() => removeChecklistItem(checklist.id, item)} aria-label={`Удалить пункт ${item.title}`}>×</button></div>{isExpanded && <div className="check-item-detail"><MentionTextarea className={unreadMentionSourceIds.includes(itemId) ? 'mention-highlight' : undefined} value={checklistItemDescriptionDrafts[itemId] ?? item.description} onValueChange={(value) => setChecklistItemDescriptionDrafts((current) => ({ ...current, [itemId]: value }))} onBlur={() => saveChecklistItemDescription(checklist.id, item)} members={account ? workspaceMembers : []} maxLength={4000} placeholder="Описание пункта…" ariaLabel={`Описание пункта ${item.title}`} /><label className="check-item-upload">{isUploadingChecklistItemAttachment ? 'Загружаем…' : '＋ Картинка или видео'}<input type="file" accept="image/jpeg,image/png,image/gif,image/webp,video/mp4,video/webm,video/quicktime" multiple disabled={isUploadingChecklistItemAttachment} onChange={(event) => { const files = Array.from(event.target.files ?? []); event.target.value = ''; void uploadChecklistItemAttachments(checklist.id, item, files); }} /></label>{item.attachments.length > 0 && <div className="check-item-attachments">{item.attachments.map((attachment) => <figure key={attachment.id}>{attachment.media_type.startsWith('image/') ? <button className="check-item-image" type="button" onClick={() => setImagePreview({ url: assetUrl(attachment.url), name: attachment.original_name })}><img src={assetUrl(attachment.url)} alt={attachment.original_name} /></button> : attachment.media_type.startsWith('video/') ? <video controls preload="metadata" src={assetUrl(attachment.url)} /> : <a href={assetUrl(attachment.url)} target="_blank" rel="noreferrer">{attachment.original_name}</a>}<figcaption><span>{attachment.original_name}</span><button type="button" onClick={() => deleteChecklistItemAttachment(checklist.id, item, attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></figcaption></figure>)}</div>}</div>}</div>; })}<form className="inline-composer" onSubmit={(event) => addChecklistItem(event, checklist.id)}><input value={checklistItemDrafts[checklist.id] ?? ''} onChange={(event) => setChecklistItemDrafts((current) => ({ ...current, [checklist.id]: event.target.value }))} maxLength={500} placeholder="Добавить пункт…" aria-label={`Новый пункт для ${checklist.title}`} /><button type="submit" disabled={isSavingChecklist || !(checklistItemDrafts[checklist.id] ?? '').trim()}>Добавить</button></form></section>; })}<form className="new-checklist-form" onSubmit={createChecklist}><input value={checklistNameDraft} onChange={(event) => setChecklistNameDraft(event.target.value)} maxLength={200} placeholder="Название нового чек-листа" aria-label="Название нового чек-листа" /><button type="submit" disabled={isSavingChecklist || !checklistNameDraft.trim()}>＋ Чек-лист</button></form></>}</section>
            {renderChecklists()}
            <div className="attachments"><div className="section-heading"><h3>Вложения</h3><span>{attachments.length}</span></div>{attachments.length ? <div className="attachment-grid">{attachments.map((attachment) => attachment.media_type.startsWith('image/') ? <figure className="attachment-preview" key={attachment.id}><img src={assetUrl(attachment.url)} alt={attachment.original_name} /><figcaption><span>{attachment.original_name}</span><div className="cover-controls"><select value={selected.coverAttachmentId === attachment.id ? selected.coverMode ?? 'full' : coverModeDraft} onChange={(event) => { const mode = event.target.value as 'full' | 'top'; setCoverModeDraft(mode); if (selected.coverAttachmentId === attachment.id) updateCardCover(attachment, mode); }} aria-label="Тип обложки"><option value="full">Фон</option><option value="top">Сверху</option></select><button className="cover-button" onClick={() => updateCardCover(selected.coverAttachmentId === attachment.id ? null : attachment)}>{selected.coverAttachmentId === attachment.id ? 'Снять' : 'Установить'}</button></div><button className="attachment-remove" onClick={() => deleteAttachment(attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></figcaption></figure> : attachment.media_type.startsWith('video/') ? <figure className="attachment-preview" key={attachment.id}><video controls preload="metadata" src={assetUrl(attachment.url)} /><figcaption>{attachment.original_name}<button onClick={() => deleteAttachment(attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></figcaption></figure> : <div className="attachment-file" key={attachment.id}><span>▶</span><a href={assetUrl(attachment.url)} target="_blank" rel="noreferrer">{attachment.original_name}</a><button onClick={() => deleteAttachment(attachment)} aria-label={`Удалить ${attachment.original_name}`}>×</button></div>)}</div> : <p className="empty-attachments">Прикрепите изображение или видео до 50 МиБ.</p>}<label className="upload-button">{isUploadingAttachment ? 'Загружаем…' : '＋ Добавить файл'}<input type="file" accept="image/jpeg,image/png,image/gif,image/png,image/webp,video/mp4,video/webm,video/quicktime" multiple disabled={isUploadingAttachment} onChange={uploadAttachments} /></label></div>
            <footer className="modal-actions"><button className="archive-button" onClick={archiveSelectedCard}>Архивировать</button><span className={`autosave-status ${cardSaveStatus}`}>{cardSaveStatus === 'saving' ? 'Изменения сохраняются' : cardSaveStatus === 'error' ? 'Не удалось сохранить' : 'Все изменения сохранены'}</span></footer>
          </div>
          <aside className="task-sidebar" aria-label="Комментарии и активность">
            <section className="conversation-panel" aria-label="Комментарии и активность">
              <div className="conversation-heading"><div><p className="sidebar-caption">ОБСУЖДЕНИЕ</p><h3>Комментарии и активность</h3></div><span>{comments.length}</span></div>
              {isDetailsLoading ? <p className="detail-loading">Загружаем сообщения…</p> : <div className="conversation-scroll">
                {comments.filter((comment) => !comment.parent_comment_id).map((comment) => <div className="comment-thread comment-arrive" key={comment.id}>
                  <div className="comment"><Avatar member={comment.author_id === account?.user.id ? currentMember : { id: `comment-${comment.id}`, initials: comment.author_name.slice(0, 2).toUpperCase() || 'У', color: 'mint', name: comment.author_name, avatarUrl: comment.author_avatar_url }} /><div className="comment-body">{editingCommentId === comment.id ? <form className="comment-edit" onSubmit={(event) => { event.preventDefault(); saveCommentEdit(comment); }}><MentionTextarea autoFocus value={commentEditDraft} onValueChange={setCommentEditDraft} members={account ? workspaceMembers : []} maxLength={10000} ariaLabel="Изменить комментарий" /><div><button type="submit">Сохранить</button><button type="button" onClick={() => setEditingCommentId(null)}>Отмена</button></div></form> : <><header><b>@{comment.author_name}</b><time>{comment.created_at ? new Date(comment.created_at).toLocaleString('ru-RU', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' }) : 'только что'}{comment.edited_at && ' · изменено'}</time></header><div className="comment-text"><MarkdownDescription value={comment.body} highlightMentions={unreadMentionSourceIds.includes(String(comment.id))} /></div><div className="comment-actions"><button onClick={() => { setReplyToCommentId(comment.id); setCommentDraft(''); }}>Ответить</button>{comment.author_id === account?.user.id && <><button onClick={() => beginCommentEdit(comment)}>Изменить</button><button onClick={() => removeComment(comment)}>Удалить</button></>}</div></>}</div></div>
                  {comments.filter((reply) => reply.parent_comment_id === String(comment.id)).map((reply) => <div className="comment comment-reply comment-arrive" key={reply.id}><Avatar member={reply.author_id === account?.user.id ? currentMember : { id: `comment-${reply.id}`, initials: reply.author_name.slice(0, 2).toUpperCase() || 'У', color: 'mint', name: reply.author_name, avatarUrl: reply.author_avatar_url }} /><div className="comment-body">{editingCommentId === reply.id ? <form className="comment-edit" onSubmit={(event) => { event.preventDefault(); saveCommentEdit(reply); }}><MentionTextarea autoFocus value={commentEditDraft} onValueChange={setCommentEditDraft} members={account ? workspaceMembers : []} maxLength={10000} ariaLabel="Изменить ответ" /><div><button type="submit">Сохранить</button><button type="button" onClick={() => setEditingCommentId(null)}>Отмена</button></div></form> : <><header><b>@{reply.author_name}</b><time>{reply.created_at ? new Date(reply.created_at).toLocaleString('ru-RU', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' }) : 'только что'}{reply.edited_at && ' · изменено'}</time></header><div className="comment-text"><MarkdownDescription value={reply.body} highlightMentions={unreadMentionSourceIds.includes(String(reply.id))} /></div><div className="comment-actions">{reply.author_id === account?.user.id && <><button onClick={() => beginCommentEdit(reply)}>Изменить</button><button onClick={() => removeComment(reply)}>Удалить</button></>}</div></>}</div></div>)}</div>)}
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
    {columnContextMenu && <div className="card-context-menu" role="menu" style={{ left: columnContextMenu.x, top: columnContextMenu.y }} onPointerDown={(event) => event.stopPropagation()}><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); setComposerOpen(columnContextMenu.column.id); setColumnContextMenu(null); }}>Добавить задачу</button><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); addColumn(columnContextMenu.column); setColumnContextMenu(null); }}>Добавить колонку ниже</button><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); moveColumn(columnContextMenu.column.id); setColumnContextMenu(null); }}>Вынести в отдельный ряд справа</button><button type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); beginColumnRename(columnContextMenu.column); setColumnContextMenu(null); }}>Переименовать</button><button className="danger-action" type="button" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); deleteColumn(columnContextMenu.column); setColumnContextMenu(null); }}>Удалить пустую</button></div>}
    {cardDragPreview && <div ref={cardDragPreviewElementRef} className="card-drag-preview" aria-hidden="true" style={{ left: cardDragPreview.x - 28, top: cardDragPreview.y - 20, width: cardDragPreview.width, height: cardDragPreview.height } as CSSProperties}>{cardDragPreview.card.coverUrl && <div className={`card-cover ${cardDragPreview.card.coverMode ?? 'full'}`}><img src={assetUrl(cardDragPreview.card.coverUrl)} alt="" /></div>}<div className="card-main">{(cardDragPreview.card.labels.length > 0 || cardDragPreview.card.roles.length > 0) && <div className="card-top"><div className="card-labels">{cardDragPreview.card.labels.map((label) => <LabelChip label={label} key={label.id} />)}{cardDragPreview.card.roles.map((role) => <ProfileRoleChip role={role} key={role.id} compact />)}</div></div>}<div className="card-title-row"><span className={`card-complete ${cardDragPreview.card.completedAt ? 'done' : ''}`}>{cardDragPreview.card.completedAt && '✓'}</span><h3>{cardDragPreview.card.title}</h3></div>{cardDragPreview.card.dueAt && <p className="due">◷ {formatDue(cardDragPreview.card.dueAt)}</p>}</div>{cardDragPreview.card.priority ? <span className="card-priority-corner" style={{ right: cardDragPreview.card.members.length ? 96 : 14 }}><PrioritySignal priority={cardDragPreview.card.priority} /></span> : null}{(cardDragPreview.card.checklist || cardDragPreview.card.comments || cardDragPreview.card.attachments || cardDragPreview.card.members.length > 0) && <footer className="card-footer"><div className="card-meta">{cardDragPreview.card.checklist && <span className={isChecklistComplete(cardDragPreview.card.checklist) ? 'checklist-complete' : ''}><CardMetaIcon type="checklist" />{cardDragPreview.card.checklist}</span>}{cardDragPreview.card.comments && <span><CardMetaIcon type="comments" />{cardDragPreview.card.comments}</span>}{cardDragPreview.card.attachments && <span><CardMetaIcon type="attachments" /></span>}</div><div className="card-avatars">{cardDragPreview.card.members.map((member) => <Avatar key={member.id} member={member} />)}</div></footer>}</div>}
    {cardMoveMotion && <div className="card-move-ghost" aria-hidden="true" style={{ left: cardMoveMotion.from.left, top: cardMoveMotion.from.top, width: cardMoveMotion.from.width, height: cardMoveMotion.from.height, '--card-move-x': `${cardMoveMotion.to.left - cardMoveMotion.from.left}px`, '--card-move-y': `${cardMoveMotion.to.top - cardMoveMotion.from.top}px`, '--card-move-scale-x': String(cardMoveMotion.to.width / cardMoveMotion.from.width), '--card-move-scale-y': String(cardMoveMotion.to.height / cardMoveMotion.from.height) } as CSSProperties}><b>{cardMoveMotion.title}</b></div>}
    {toast && <div className="toast">✓ {toast}</div>}
  </main>;
}
