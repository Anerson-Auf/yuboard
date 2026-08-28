'use client';

import { PointerEvent as ReactPointerEvent, useEffect, useMemo, useRef, useState } from 'react';
import './dependency-graph.css';

type RelationType = 'blocks' | 'depends_on' | 'duplicate' | 'related' | 'part_of';
export type DependencyGraphNode = { id: string; title: string; listTitle: string; completed: boolean; priority?: number };
type Relation = { id: string; source_card_id: string; target_card_id: string; relation_type: RelationType; note: string };
type Point = { x: number; y: number };
type PositionResponse = { card_id: string; x: number; y: number };
type CanvasMenu = { client: Point; canvas: Point };

const meta: Record<RelationType, { label: string; className: string; directed: boolean }> = {
  blocks: { label: 'Блокирует', className: 'blocks', directed: true },
  depends_on: { label: 'Зависит от', className: 'depends-on', directed: true },
  related: { label: 'Связана с', className: 'related', directed: false },
  duplicate: { label: 'Дубликат', className: 'duplicate', directed: false },
  part_of: { label: 'Является частью', className: 'part-of', directed: true },
};
const TYPE_LIST = Object.keys(meta) as RelationType[];
const NODE_WIDTH = 238;
const NODE_HEIGHT = 96;
const PAD = 48;
const CANVAS_SIZE = 12_000;
const INITIAL_PAN = { x: 28, y: 28 };
const headers = () => {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice(15);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
};
const clampPoint = (point: Point): Point => ({ x: Math.max(0, Math.min(CANVAS_SIZE - NODE_WIDTH, Math.round(point.x))), y: Math.max(0, Math.min(CANVAS_SIZE - NODE_HEIGHT, Math.round(point.y))) });
const direction = (relation: Relation) => relation.relation_type === 'depends_on' ? { from: relation.target_card_id, to: relation.source_card_id } : { from: relation.source_card_id, to: relation.target_card_id };
const linePath = (from: Point, to: Point) => {
  const dx = to.x - from.x;
  const bend = Math.max(54, Math.min(210, Math.abs(dx) * .48 + Math.abs(to.y - from.y) * .16));
  const sign = dx >= 0 ? 1 : -1;
  return `M ${from.x} ${from.y} C ${from.x + sign * bend} ${from.y}, ${to.x - sign * bend} ${to.y}, ${to.x} ${to.y}`;
};

export default function DependencyGraph({ boardId, nodes, canEdit, onOpenCard }: { boardId: string; nodes: DependencyGraphNode[]; canEdit: boolean; onOpenCard: (cardId: string) => void }) {
  const [relations, setRelations] = useState<Relation[]>([]);
  const [positions, setPositions] = useState<Record<string, Point>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState(INITIAL_PAN);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [note, setNote] = useState('');
  const [saving, setSaving] = useState(false);
  const [canvasMenu, setCanvasMenu] = useState<CanvasMenu | null>(null);
  const [sourceMenu, setSourceMenu] = useState<CanvasMenu | null>(null);
  const [draft, setDraft] = useState<{ sourceId: string; cursor: Point } | null>(null);
  const [targetMenu, setTargetMenu] = useState<{ sourceId: string; client: Point; canvas: Point } | null>(null);
  const [typeMenu, setTypeMenu] = useState<{ sourceId: string; targetId: string; client: Point; canvas?: Point } | null>(null);
  const [relationMenu, setRelationMenu] = useState<{ id: string; x: number; y: number } | null>(null);
  const [draftNote, setDraftNote] = useState('');
  const [sourceSearch, setSourceSearch] = useState('');
  const [targetSearch, setTargetSearch] = useState('');
  const [linkTargetId, setLinkTargetId] = useState<string | null>(null);
  const viewportRef = useRef<HTMLDivElement>(null);
  const panDragRef = useRef<{ id: number; x: number; y: number; pan: Point } | null>(null);
  const nodeDragRef = useRef<{ id: number; nodeId: string; x: number; y: number; position: Point; moved: boolean } | null>(null);
  const suppressNodeClickRef = useRef(false);
  const byId = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
  const nodeIdsSignature = useMemo(() => nodes.map((node) => node.id).sort().join('|'), [nodes]);
  const matchingNodes = (query: string, excludedId?: string) => {
    const needle = query.trim().toLocaleLowerCase('ru-RU');
    return nodes.filter((node) => node.id !== excludedId && (!needle || `${node.title} ${node.listTitle}`.toLocaleLowerCase('ru-RU').includes(needle)));
  };

  const load = async () => {
    setLoading(true);
    setError('');
    try {
      const [relationsResponse, positionsResponse] = await Promise.all([
        fetch(`/v1/boards/${boardId}/relations`),
        fetch(`/v1/boards/${boardId}/dependency-layout`),
      ]);
      if (!relationsResponse.ok || !positionsResponse.ok) throw new Error();
      const [relationData, positionData] = await Promise.all([relationsResponse.json() as Promise<Relation[]>, positionsResponse.json() as Promise<PositionResponse[]>]);
      setRelations(relationData.filter((item) => byId.has(item.source_card_id) && byId.has(item.target_card_id)));
      setPositions(Object.fromEntries(positionData.filter((item) => byId.has(item.card_id)).map((item) => [item.card_id, { x: item.x, y: item.y }])));
    } catch {
      setError('Не удалось загрузить связи. Обновите представление.');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { void load(); }, [boardId, nodeIdsSignature]); // eslint-disable-line react-hooks/exhaustive-deps

  const graphNodes = useMemo(() => {
    const connected = new Set(relations.flatMap((relation) => [relation.source_card_id, relation.target_card_id]));
    if (draft) connected.add(draft.sourceId);
    return nodes.filter((node) => connected.has(node.id));
  }, [draft, nodes, relations]);
  const layout = useMemo(() => {
    const positionsById = new Map<string, Point>();
    graphNodes.forEach((node, index) => {
      const saved = positions[node.id];
      positionsById.set(node.id, saved ?? { x: PAD + (index % 4) * (NODE_WIDTH + 84), y: PAD + Math.floor(index / 4) * (NODE_HEIGHT + 70) });
    });
    return { positions: positionsById, width: CANVAS_SIZE, height: CANVAS_SIZE };
  }, [graphNodes, positions]);
  const visibleRelations = relations.filter((relation) => layout.positions.has(relation.source_card_id) && layout.positions.has(relation.target_card_id));
  const selected = relations.find((relation) => relation.id === selectedId) ?? null;
  const canvasPoint = (clientX: number, clientY: number) => {
    const rect = viewportRef.current?.getBoundingClientRect();
    return rect ? { x: (clientX - rect.left - pan.x) / zoom, y: (clientY - rect.top - pan.y) / zoom } : { x: 0, y: 0 };
  };
  const savePosition = async (cardId: string, position: Point) => {
    try {
      const response = await fetch(`/v1/boards/${boardId}/dependency-layout/${cardId}`, { method: 'PUT', headers: headers(), body: JSON.stringify(position) });
      if (!response.ok) throw new Error();
    } catch {
      setError('Не удалось сохранить позицию карточки в графе.');
    }
  };
  const ensurePosition = (cardId: string, position: Point) => {
    if (positions[cardId]) return;
    const next = clampPoint(position);
    setPositions((current) => current[cardId] ? current : { ...current, [cardId]: next });
    void savePosition(cardId, next);
  };
  const chooseRelation = (id: string) => { const item = relations.find((relation) => relation.id === id); setSelectedId(id); setNote(item?.note ?? ''); };
  const dismissMenus = () => { setCanvasMenu(null); setSourceMenu(null); setTargetMenu(null); setTypeMenu(null); setRelationMenu(null); setDraft(null); setDraftNote(''); setSourceSearch(''); setTargetSearch(''); setLinkTargetId(null); };
  // Menus are viewport-fixed. Clamp their trigger point before rendering so a
  // picker opened near an edge never disappears behind the browser boundary.
  const menuPosition = (point: Point, reservedHeight: number) => {
    if (typeof window === 'undefined') return { left: point.x, top: point.y };
    const gap = 12;
    return {
      left: Math.max(gap, Math.min(point.x, window.innerWidth - 352)),
      top: Math.max(gap, Math.min(point.y, window.innerHeight - reservedHeight - gap)),
    };
  };
  const reset = () => { setZoom(1); setPan(INITIAL_PAN); };
  const zoomAt = (x: number, y: number, next: number) => {
    const rect = viewportRef.current?.getBoundingClientRect();
    if (!rect) return;
    const localX = x - rect.left;
    const localY = y - rect.top;
    setPan((current) => ({ x: localX - ((localX - current.x) / zoom) * next, y: localY - ((localY - current.y) / zoom) * next }));
    setZoom(next);
  };
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const preventPageScroll = (event: WheelEvent) => {
      if (event.target instanceof Element && event.target.closest('.dependency-menu')) return;
      event.preventDefault();
      event.stopPropagation();
      const next = Math.min(1.8, Math.max(.38, zoom * (event.deltaY < 0 ? 1.12 : .89)));
      if (next !== zoom) zoomAt(event.clientX, event.clientY, next);
    };
    viewport.addEventListener('wheel', preventPageScroll, { passive: false });
    return () => viewport.removeEventListener('wheel', preventPageScroll);
  }, [zoom]); // zoomAt deliberately follows the current zoom level.

  useEffect(() => {
    const cancelInterruptedDrag = () => {
      panDragRef.current = null;
      nodeDragRef.current = null;
    };
    const cancelWhenHidden = () => { if (document.hidden) cancelInterruptedDrag(); };
    window.addEventListener('blur', cancelInterruptedDrag);
    document.addEventListener('visibilitychange', cancelWhenHidden);
    return () => {
      window.removeEventListener('blur', cancelInterruptedDrag);
      document.removeEventListener('visibilitychange', cancelWhenHidden);
    };
  }, []);

  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      dismissMenus();
    };
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  });

  const create = async (sourceId: string, targetId: string, relationType: RelationType, targetPoint?: Point) => {
    if (targetPoint) ensurePosition(targetId, targetPoint);
    setSaving(true);
    setError('');
    try {
      const response = await fetch(`/v1/cards/${sourceId}/relations`, { method: 'POST', headers: headers(), body: JSON.stringify({ target_card_id: targetId, relation_type: relationType, note: draftNote }) });
      if (!response.ok) {
        const body = await response.json().catch(() => null) as { message?: string } | null;
        throw new Error(body?.message || 'Не удалось создать связь.');
      }
      setDraft(null); setTargetMenu(null); setTypeMenu(null); setDraftNote('');
      await load();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : 'Не удалось создать связь.');
    } finally {
      setSaving(false);
    }
  };
  const remove = async () => {
    if (!selected) return;
    setSaving(true);
    try {
      const response = await fetch(`/v1/cards/${selected.source_card_id}/relations/${selected.id}`, { method: 'DELETE', headers: headers() });
      if (!response.ok) throw new Error();
      setSelectedId(null); setRelationMenu(null); await load();
    } catch { setError('Не удалось удалить связь.'); } finally { setSaving(false); }
  };
  const saveNote = async () => {
    if (!selected) return;
    setSaving(true);
    try {
      const response = await fetch(`/v1/cards/${selected.source_card_id}/relations/${selected.id}`, { method: 'PATCH', headers: headers(), body: JSON.stringify({ note }) });
      if (!response.ok) throw new Error();
      const updated = await response.json() as Relation;
      setRelations((current) => current.map((item) => item.id === updated.id ? { ...item, note: updated.note } : item));
    } catch { setError('Не удалось сохранить пояснение.'); } finally { setSaving(false); }
  };
  const changeType = async (type: RelationType) => {
    if (!selected) return;
    setSaving(true);
    try {
      const old = await fetch(`/v1/cards/${selected.source_card_id}/relations/${selected.id}`, { method: 'DELETE', headers: headers() });
      if (!old.ok) throw new Error();
      const created = await fetch(`/v1/cards/${selected.source_card_id}/relations`, { method: 'POST', headers: headers(), body: JSON.stringify({ target_card_id: selected.target_card_id, relation_type: type, note }) });
      if (!created.ok) throw new Error();
      setSelectedId(null); setRelationMenu(null); await load();
    } catch { setError('Не удалось изменить связь.'); } finally { setSaving(false); }
  };
  const begin = (sourceId: string, placement?: Point) => {
    if (placement) ensurePosition(sourceId, placement);
    const position = placement && !positions[sourceId] ? clampPoint(placement) : layout.positions.get(sourceId);
    setCanvasMenu(null); setSourceMenu(null); setTargetMenu(null); setTargetSearch(''); setLinkTargetId(null); setSelectedId(null);
    setDraft({ sourceId, cursor: position ? { x: position.x + NODE_WIDTH, y: position.y + NODE_HEIGHT / 2 } : { x: PAD + NODE_WIDTH, y: PAD + NODE_HEIGHT / 2 } });
  };
  const endpoints = (from: Point, to: Point) => {
    const sourceOnRight = to.x + NODE_WIDTH / 2 >= from.x + NODE_WIDTH / 2;
    return { start: { x: sourceOnRight ? from.x + NODE_WIDTH : from.x, y: from.y + NODE_HEIGHT / 2 }, end: { x: sourceOnRight ? to.x : to.x + NODE_WIDTH, y: to.y + NODE_HEIGHT / 2 } };
  };
  const startNodeDrag = (event: ReactPointerEvent<HTMLButtonElement>, nodeId: string) => {
    if (event.button !== 0 || draft || !canEdit) return;
    event.preventDefault(); event.stopPropagation();
    nodeDragRef.current = { id: event.pointerId, nodeId, x: event.clientX, y: event.clientY, position: layout.positions.get(nodeId) ?? { x: PAD, y: PAD }, moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  return <section className="dependency-graph" aria-label="Граф зависимостей карточек" onPointerDownCapture={(event) => {
    if (event.button !== 0 || !(canvasMenu || sourceMenu || targetMenu || typeMenu || relationMenu)) return;
    if (event.target instanceof Element && event.target.closest('.dependency-menu, .dependency-node, .dependency-line, .dependency-output')) return;
    dismissMenus();
    event.stopPropagation();
  }}>
    <header className="dependency-graph-header"><div><p className="eyebrow">СВЯЗИ КАРТОЧЕК</p><h2>Зависимости</h2><p>Свободное поле: расположение карточек не меняется при создании связи. ПКМ по пустому месту — выберите стартовую карточку, протяните связь и выберите целевую.</p></div><div className="dependency-header-actions"><button type="button" className="dependency-reset" onClick={reset}>↺ Исходный вид</button></div></header>
    <div className="dependency-legend">{TYPE_LIST.map((type) => <span key={type} className={`dependency-legend-item ${meta[type].className}`}><i />{meta[type].label}</span>)}</div>
    {selected && <aside className="dependency-selected-relation"><div><span><b>{byId.get(selected.source_card_id)?.title}</b> · {meta[selected.relation_type].label.toLowerCase()} · <b>{byId.get(selected.target_card_id)?.title}</b></span>{canEdit ? <label>Пояснение<textarea value={note} onChange={(event) => setNote(event.target.value)} maxLength={500} placeholder="Например, ждём API-метод" /></label> : selected.note && <p>{selected.note}</p>}</div>{canEdit && <div><button type="button" disabled={saving || note === selected.note} onClick={() => void saveNote()}>Сохранить</button><button type="button" disabled={saving} onClick={() => void remove()}>Удалить связь</button></div>}</aside>}
    {error && <p className="dependency-error">{error}</p>}
    <div ref={viewportRef} className={`dependency-graph-scroll ${draft ? 'is-linking' : ''}`} onPointerDown={(event: ReactPointerEvent<HTMLDivElement>) => {
      event.stopPropagation();
      if (event.button !== 0 || draft || (event.target instanceof Element && event.target.closest('button, textarea, .dependency-menu, .dependency-node, .dependency-line'))) return;
      panDragRef.current = { id: event.pointerId, x: event.clientX, y: event.clientY, pan };
      event.currentTarget.setPointerCapture(event.pointerId);
    }} onPointerMove={(event) => {
      event.stopPropagation();
      const nodeDrag = nodeDragRef.current;
      if (nodeDrag?.id === event.pointerId) {
        const next = clampPoint({ x: nodeDrag.position.x + (event.clientX - nodeDrag.x) / zoom, y: nodeDrag.position.y + (event.clientY - nodeDrag.y) / zoom });
        if (Math.abs(event.clientX - nodeDrag.x) > 3 || Math.abs(event.clientY - nodeDrag.y) > 3) nodeDrag.moved = true;
        setPositions((current) => ({ ...current, [nodeDrag.nodeId]: next }));
        return;
      }
      if (draft) {
        const target = document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-dependency-node-id]')?.dataset.dependencyNodeId;
        const nextTargetId = target && target !== draft.sourceId ? target : null;
        setLinkTargetId((current) => current === nextTargetId ? current : nextTargetId);
        setDraft((current) => current ? { ...current, cursor: canvasPoint(event.clientX, event.clientY) } : null);
        return;
      }
      const drag = panDragRef.current;
      if (drag?.id === event.pointerId) setPan({ x: drag.pan.x + event.clientX - drag.x, y: drag.pan.y + event.clientY - drag.y });
    }} onPointerUp={(event) => {
      event.stopPropagation();
      const nodeDrag = nodeDragRef.current;
      if (nodeDrag?.id === event.pointerId) {
        nodeDragRef.current = null;
        if (nodeDrag.moved) { suppressNodeClickRef.current = true; void savePosition(nodeDrag.nodeId, clampPoint({ x: nodeDrag.position.x + (event.clientX - nodeDrag.x) / zoom, y: nodeDrag.position.y + (event.clientY - nodeDrag.y) / zoom })); }
        return;
      }
      if (draft) {
        const client = { x: event.clientX, y: event.clientY };
        const target = linkTargetId ?? document.elementFromPoint(event.clientX, event.clientY)?.closest<HTMLElement>('[data-dependency-node-id]')?.dataset.dependencyNodeId;
        if (target && target !== draft.sourceId) {
          suppressNodeClickRef.current = true;
          setLinkTargetId(null);
          setTargetMenu(null);
          setTypeMenu({ sourceId: draft.sourceId, targetId: target, client, canvas: canvasPoint(event.clientX, event.clientY) });
          return;
        }
        setTargetMenu({ sourceId: draft.sourceId, client, canvas: canvasPoint(event.clientX, event.clientY) });
        return;
      }
      if (panDragRef.current?.id === event.pointerId) { panDragRef.current = null; if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); }
    }} onPointerCancel={(event) => {
      if (panDragRef.current?.id === event.pointerId) panDragRef.current = null;
      if (nodeDragRef.current?.id === event.pointerId) nodeDragRef.current = null;
    }} onLostPointerCapture={() => { panDragRef.current = null; nodeDragRef.current = null; }} onContextMenu={(event) => {
      event.stopPropagation();
      if (!canEdit || (event.target instanceof Element && event.target.closest('.dependency-node, .dependency-line, .dependency-menu'))) return;
      event.preventDefault();
      setCanvasMenu({ client: { x: event.clientX, y: event.clientY }, canvas: canvasPoint(event.clientX, event.clientY) }); setRelationMenu(null);
    }}>
      <div className="dependency-zoom-readout">{Math.round(zoom * 100)}%</div>
      {canvasMenu && <div className="dependency-menu dependency-canvas-menu" style={menuPosition(canvasMenu.client, 62)}><button type="button" onClick={() => { setSourceSearch(''); setSourceMenu(canvasMenu); setCanvasMenu(null); }}>↗ Создать зависимость</button></div>}
      {sourceMenu && <div className="dependency-menu dependency-source-picker" style={menuPosition(sourceMenu.client, 432)}><b>От какой карточки?</b><input className="dependency-menu-search" autoFocus value={sourceSearch} onChange={(event) => setSourceSearch(event.target.value)} placeholder="Найти по названию или колонке…" aria-label="Найти исходную карточку" />{matchingNodes(sourceSearch).map((node) => <button key={node.id} type="button" onClick={() => { setSourceSearch(''); begin(node.id, sourceMenu.canvas); }}><small>{node.listTitle}</small>{node.title}</button>)}{!matchingNodes(sourceSearch).length && <p className="dependency-menu-empty">Ничего не найдено.</p>}</div>}
      {targetMenu && <div className="dependency-menu dependency-source-picker" style={menuPosition(targetMenu.client, 432)}><b>С какой карточкой?</b><input className="dependency-menu-search" autoFocus value={targetSearch} onChange={(event) => setTargetSearch(event.target.value)} placeholder="Найти по названию или колонке…" aria-label="Найти целевую карточку" />{matchingNodes(targetSearch, targetMenu.sourceId).map((node) => <button key={node.id} type="button" onClick={() => { setTargetSearch(''); setTargetMenu(null); setTypeMenu({ sourceId: targetMenu.sourceId, targetId: node.id, client: targetMenu.client, canvas: targetMenu.canvas }); }}><small>{node.listTitle}</small>{node.title}</button>)}{!matchingNodes(targetSearch, targetMenu.sourceId).length && <p className="dependency-menu-empty">Ничего не найдено.</p>}</div>}
      {typeMenu && <div className="dependency-menu dependency-type-picker" style={menuPosition(typeMenu.client, 334)}><b>Как связать?</b>{TYPE_LIST.map((type) => <button key={type} type="button" disabled={saving} onClick={() => void create(typeMenu.sourceId, typeMenu.targetId, type, typeMenu.canvas)}><i className={meta[type].className} />{meta[type].label}</button>)}<textarea value={draftNote} onChange={(event) => setDraftNote(event.target.value)} maxLength={500} placeholder="Короткое пояснение (необязательно)" /><button type="button" className="dependency-cancel" onClick={() => { setTypeMenu(null); setTargetMenu(null); setDraft(null); }}>Отмена</button></div>}
      {relationMenu && selected && <div className="dependency-menu dependency-relation-menu" style={menuPosition({ x: relationMenu.x, y: relationMenu.y }, 298)}><b>Изменить связь</b>{TYPE_LIST.map((type) => <button key={type} type="button" disabled={saving || type === selected.relation_type} onClick={() => void changeType(type)}><i className={meta[type].className} />{meta[type].label}</button>)}<button type="button" className="dependency-menu-danger" onClick={() => void remove()}>Разорвать связь</button></div>}
      {loading ? <p className="dependency-empty">Загружаем связи…</p> : <div className="dependency-canvas" style={{ width: layout.width, height: layout.height, transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})` }}><svg className="dependency-lines" width={layout.width} height={layout.height} viewBox={`0 0 ${layout.width} ${layout.height}`}><defs><marker id="dependency-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" /></marker></defs>{visibleRelations.map((relation) => { const order = direction(relation); const from = layout.positions.get(order.from); const to = layout.positions.get(order.to); if (!from || !to) return null; const { start, end } = endpoints(from, to); const type = meta[relation.relation_type]; return <g key={relation.id} className={`dependency-line ${type.className} ${selectedId === relation.id ? 'selected' : ''}`} onClick={() => chooseRelation(relation.id)} onContextMenu={(event) => { if (!canEdit) return; event.preventDefault(); event.stopPropagation(); chooseRelation(relation.id); setRelationMenu({ id: relation.id, x: event.clientX, y: event.clientY }); }}><path className="dependency-line-hitbox" d={linePath(start, end)} /><path className="dependency-line-stroke" d={linePath(start, end)} markerEnd={type.directed ? 'url(#dependency-arrow)' : undefined} /><text x={(start.x + end.x) / 2} y={(start.y + end.y) / 2 - 10} textAnchor="middle">{type.label}</text></g>; })}{draft && (() => { const source = layout.positions.get(draft.sourceId); if (!source) return null; const { start } = endpoints(source, draft.cursor); return <path className="dependency-draft-line" d={linePath(start, draft.cursor)} markerEnd="url(#dependency-arrow)" />; })()}</svg>{graphNodes.map((node) => { const position = layout.positions.get(node.id); if (!position) return null; return <button key={node.id} data-dependency-node-id={node.id} className={`dependency-node ${draft?.sourceId === node.id ? 'selected' : ''} ${linkTargetId === node.id ? 'link-target' : ''} ${node.completed ? 'completed' : ''}`} type="button" style={{ left: position.x, top: position.y }} onPointerDown={(event) => startNodeDrag(event, node.id)} onClick={(event) => { event.stopPropagation(); if (suppressNodeClickRef.current) { suppressNodeClickRef.current = false; return; } if (draft && draft.sourceId !== node.id) { setLinkTargetId(null); setTargetMenu(null); setTypeMenu({ sourceId: draft.sourceId, targetId: node.id, client: { x: event.clientX, y: event.clientY } }); } else if (!draft) onOpenCard(node.id); }}><span className="dependency-node-list">{node.listTitle}</span><b>{node.title}</b><footer><span title={node.priority ? `Приоритет ${node.priority} из 5` : 'Без приоритета'}>{node.priority ? '▮'.repeat(node.priority) : '—'}</span>{node.completed && <em>Выполнено</em>}</footer>{canEdit && <span className="dependency-output" onPointerDown={(event) => { event.preventDefault(); event.stopPropagation(); begin(node.id); viewportRef.current?.setPointerCapture(event.pointerId); }} />}</button>; })}</div>}
    </div>
  </section>;
}
