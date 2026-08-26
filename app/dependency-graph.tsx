'use client';

import { PointerEvent as ReactPointerEvent, WheelEvent, useEffect, useMemo, useRef, useState } from 'react';
import './dependency-graph.css';

type RelationType = 'blocks' | 'depends_on' | 'duplicate' | 'related';

export type DependencyGraphNode = { id: string; title: string; listTitle: string; completed: boolean; priority?: number };
type DependencyRelation = { id: string; source_card_id: string; target_card_id: string; relation_type: RelationType; note: string; created_at: string };
type NodePosition = { x: number; y: number };

const relationMeta: Record<RelationType, { label: string; hint: string; className: string; directed: boolean }> = {
  blocks: { label: 'Блокирует', hint: 'Первая задача должна быть завершена до второй.', className: 'blocks', directed: true },
  depends_on: { label: 'Зависит от', hint: 'Первая задача ждёт завершения второй.', className: 'depends-on', directed: true },
  related: { label: 'Связана с', hint: 'Контекстная связь без блокировки выполнения.', className: 'related', directed: false },
  duplicate: { label: 'Дубликат', hint: 'Обе карточки описывают одну и ту же работу.', className: 'duplicate', directed: false },
};
const NODE_WIDTH = 238;
const NODE_HEIGHT = 96;
const PADDING = 48;
const COLUMN_GAP = 150;
const ROW_GAP = 28;
const INITIAL_PAN = { x: 28, y: 28 };

function csrfHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}
function relationDirection(relation: DependencyRelation) { return relation.relation_type === 'depends_on' ? { from: relation.target_card_id, to: relation.source_card_id } : { from: relation.source_card_id, to: relation.target_card_id }; }
function priorityLabel(priority?: number) { return priority ? `Приоритет ${priority} из 5` : 'Без приоритета'; }

export default function DependencyGraph({ boardId, nodes, canEdit, onOpenCard }: { boardId: string; nodes: DependencyGraphNode[]; canEdit: boolean; onOpenCard: (cardId: string) => void }) {
  const [relations, setRelations] = useState<DependencyRelation[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState('');
  const [showUnrelated, setShowUnrelated] = useState(false);
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [sourceId, setSourceId] = useState('');
  const [targetId, setTargetId] = useState('');
  const [relationType, setRelationType] = useState<RelationType>('blocks');
  const [noteDraft, setNoteDraft] = useState('');
  const [selectedRelationId, setSelectedRelationId] = useState<string | null>(null);
  const [selectedNoteDraft, setSelectedNoteDraft] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState(INITIAL_PAN);
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const graphPanRef = useRef<{ pointerId: number; x: number; y: number; origin: { x: number; y: number } } | null>(null);

  const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
  const loadRelations = async () => {
    setIsLoading(true); setError('');
    try {
      const response = await fetch(`/v1/boards/${boardId}/relations`);
      if (!response.ok) throw new Error('load failed');
      const loaded = await response.json() as DependencyRelation[];
      setRelations(loaded.filter((relation) => nodeById.has(relation.source_card_id) && nodeById.has(relation.target_card_id)));
    } catch { setError('Не удалось загрузить связи. Обновите представление.'); }
    finally { setIsLoading(false); }
  };
  useEffect(() => { void loadRelations(); }, [boardId, nodes]); // eslint-disable-line react-hooks/exhaustive-deps

  const connectedIds = useMemo(() => new Set(relations.flatMap((relation) => [relation.source_card_id, relation.target_card_id])), [relations]);
  const graphNodes = useMemo(() => showUnrelated ? nodes : nodes.filter((node) => connectedIds.has(node.id)), [connectedIds, nodes, showUnrelated]);
  const graphNodeIds = useMemo(() => new Set(graphNodes.map((node) => node.id)), [graphNodes]);
  const graphRelations = useMemo(() => relations.filter((relation) => graphNodeIds.has(relation.source_card_id) && graphNodeIds.has(relation.target_card_id)), [graphNodeIds, relations]);
  const layout = useMemo(() => {
    const rank = new Map(graphNodes.map((node) => [node.id, 0]));
    const indegree = new Map(graphNodes.map((node) => [node.id, 0]));
    const outgoing = new Map(graphNodes.map((node) => [node.id, [] as string[]]));
    graphRelations.filter((relation) => relationMeta[relation.relation_type].directed).forEach((relation) => {
      const { from, to } = relationDirection(relation);
      if (!outgoing.has(from) || !indegree.has(to)) return;
      outgoing.get(from)?.push(to); indegree.set(to, (indegree.get(to) ?? 0) + 1);
    });
    const queue = [...indegree.entries()].filter(([, count]) => count === 0).map(([id]) => id).sort();
    while (queue.length) {
      const id = queue.shift()!;
      for (const target of outgoing.get(id) ?? []) {
        rank.set(target, Math.max(rank.get(target) ?? 0, (rank.get(id) ?? 0) + 1));
        const next = (indegree.get(target) ?? 1) - 1;
        indegree.set(target, next);
        if (next === 0) queue.push(target);
      }
      queue.sort();
    }
    const columns = new Map<number, DependencyGraphNode[]>();
    graphNodes.forEach((node) => { const value = rank.get(node.id) ?? 0; columns.set(value, [...(columns.get(value) ?? []), node]); });
    const layers = [...columns.entries()].sort(([left], [right]) => left - right).map(([, value]) => value.sort((left, right) => left.title.localeCompare(right.title, 'ru')));
    const positions = new Map<string, NodePosition>();
    layers.forEach((layer, columnIndex) => layer.forEach((node, rowIndex) => positions.set(node.id, { x: PADDING + columnIndex * (NODE_WIDTH + COLUMN_GAP), y: PADDING + rowIndex * (NODE_HEIGHT + ROW_GAP) })));
    const layerHeight = Math.max(1, ...layers.map((layer) => layer.length));
    return { positions, width: Math.max(680, PADDING * 2 + Math.max(1, layers.length) * NODE_WIDTH + Math.max(0, layers.length - 1) * COLUMN_GAP), height: Math.max(360, PADDING * 2 + layerHeight * NODE_HEIGHT + Math.max(0, layerHeight - 1) * ROW_GAP) };
  }, [graphNodes, graphRelations]);
  const selectedRelation = relations.find((relation) => relation.id === selectedRelationId) ?? null;

  function resetView() { setZoom(1); setPan(INITIAL_PAN); }
  function zoomAt(clientX: number, clientY: number, nextZoom: number) {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const rect = viewport.getBoundingClientRect();
    const x = clientX - rect.left; const y = clientY - rect.top;
    setPan((current) => ({ x: x - ((x - current.x) / zoom) * nextZoom, y: y - ((y - current.y) / zoom) * nextZoom }));
    setZoom(nextZoom);
  }
  function onGraphWheel(event: WheelEvent<HTMLDivElement>) {
    event.preventDefault(); event.stopPropagation();
    const nextZoom = Math.min(1.8, Math.max(.38, zoom * (event.deltaY < 0 ? 1.12 : .89)));
    if (nextZoom !== zoom) zoomAt(event.clientX, event.clientY, nextZoom);
  }
  function startGraphPan(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || (event.target instanceof Element && event.target.closest('button, input, select, textarea, label'))) return;
    event.stopPropagation();
    graphPanRef.current = { pointerId: event.pointerId, x: event.clientX, y: event.clientY, origin: pan };
    event.currentTarget.setPointerCapture(event.pointerId);
  }
  function moveGraphPan(event: ReactPointerEvent<HTMLDivElement>) {
    const drag = graphPanRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    event.preventDefault(); event.stopPropagation();
    setPan({ x: drag.origin.x + event.clientX - drag.x, y: drag.origin.y + event.clientY - drag.y });
  }
  function endGraphPan(event: ReactPointerEvent<HTMLDivElement>) {
    const drag = graphPanRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    graphPanRef.current = null; event.stopPropagation();
  }
  async function createRelation() {
    if (!sourceId || !targetId || sourceId === targetId) return;
    setIsSaving(true); setError('');
    try {
      const response = await fetch(`/v1/cards/${sourceId}/relations`, { method: 'POST', headers: csrfHeaders(), body: JSON.stringify({ target_card_id: targetId, relation_type: relationType, note: noteDraft }) });
      if (!response.ok) { const payload = await response.json().catch(() => null) as { message?: string } | null; throw new Error(payload?.message ?? 'Не удалось создать связь.'); }
      setTargetId(''); setNoteDraft(''); await loadRelations();
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'Не удалось создать связь.'); }
    finally { setIsSaving(false); }
  }
  async function saveSelectedRelationNote() {
    if (!selectedRelation) return;
    setIsSaving(true); setError('');
    try {
      const response = await fetch(`/v1/cards/${selectedRelation.source_card_id}/relations/${selectedRelation.id}`, { method: 'PATCH', headers: csrfHeaders(), body: JSON.stringify({ note: selectedNoteDraft }) });
      if (!response.ok) throw new Error('Не удалось сохранить пояснение.');
      const updated = await response.json() as DependencyRelation;
      setRelations((current) => current.map((relation) => relation.id === updated.id ? { ...relation, note: updated.note } : relation));
    } catch (reason) { setError(reason instanceof Error ? reason.message : 'Не удалось сохранить пояснение.'); }
    finally { setIsSaving(false); }
  }
  async function removeSelectedRelation() {
    if (!selectedRelation) return;
    setIsSaving(true); setError('');
    try {
      const response = await fetch(`/v1/cards/${selectedRelation.source_card_id}/relations/${selectedRelation.id}`, { method: 'DELETE', headers: csrfHeaders() });
      if (!response.ok) throw new Error('Не удалось удалить связь.');
      setSelectedRelationId(null); await loadRelations();
    } catch { setError('Не удалось удалить связь.'); }
    finally { setIsSaving(false); }
  }
  function selectRelation(id: string) { const relation = relations.find((item) => item.id === id); setSelectedRelationId(id); setSelectedNoteDraft(relation?.note ?? ''); }
  function selectNode(id: string) { setSourceId(id); if (targetId === id) setTargetId(''); setSelectedRelationId(null); }

  return <section className="dependency-graph" aria-label="Граф зависимостей карточек">
    <header className="dependency-graph-header"><div><p className="eyebrow">СВЯЗИ КАРТОЧЕК</p><h2>Зависимости</h2><p>Стрелка показывает порядок работы: слева — условие, справа — задача, которая его ждёт.</p></div><div className="dependency-header-actions"><label className="dependency-unrelated-toggle"><input type="checkbox" checked={showUnrelated} onChange={(event) => setShowUnrelated(event.target.checked)} />Показать без связей</label><button type="button" className="dependency-reset" onClick={resetView}>↺ Исходный вид</button></div></header>
    <div className="dependency-legend" aria-label="Типы связей">{(Object.keys(relationMeta) as RelationType[]).map((type) => <span key={type} className={`dependency-legend-item ${relationMeta[type].className}`}><i />{relationMeta[type].label}</span>)}</div>
    {canEdit && <section className="dependency-options"><button className="dependency-options-trigger" type="button" onClick={() => setOptionsOpen((current) => !current)} aria-expanded={optionsOpen}>Дополнительные опции <span>{optionsOpen ? '⌃' : '⌄'}</span></button>{optionsOpen && <form className="dependency-builder" onSubmit={(event) => { event.preventDefault(); void createRelation(); }}><label>Откуда<select value={sourceId} onChange={(event) => { setSourceId(event.target.value); if (event.target.value === targetId) setTargetId(''); }}><option value="">Выберите карточку</option>{nodes.map((node) => <option key={node.id} value={node.id}>{node.title} · {node.listTitle}</option>)}</select></label><label>Связь<select value={relationType} onChange={(event) => setRelationType(event.target.value as RelationType)}>{(Object.keys(relationMeta) as RelationType[]).map((type) => <option key={type} value={type}>{relationMeta[type].label}</option>)}</select></label><label>С чем<select value={targetId} onChange={(event) => setTargetId(event.target.value)} disabled={!sourceId}><option value="">Выберите карточку</option>{nodes.filter((node) => node.id !== sourceId).map((node) => <option key={node.id} value={node.id}>{node.title} · {node.listTitle}</option>)}</select></label><label className="dependency-note-field">Пояснение<textarea value={noteDraft} onChange={(event) => setNoteDraft(event.target.value)} maxLength={500} placeholder="Например, ждём API-метод" /></label><button className="create-button" type="submit" disabled={isSaving || !sourceId || !targetId || sourceId === targetId}>Связать</button><small>{relationMeta[relationType].hint} Циклические зависимости блокируются.</small></form>}</section>}
    {selectedRelation && <aside className="dependency-selected-relation"><div><span><b>{nodeById.get(selectedRelation.source_card_id)?.title ?? 'Карточка'}</b> · {relationMeta[selectedRelation.relation_type].label.toLowerCase()} · <b>{nodeById.get(selectedRelation.target_card_id)?.title ?? 'Карточка'}</b></span>{canEdit ? <label>Пояснение<textarea value={selectedNoteDraft} onChange={(event) => setSelectedNoteDraft(event.target.value)} maxLength={500} placeholder="Добавьте контекст этой связи" /></label> : selectedRelation.note && <p>{selectedRelation.note}</p>}</div>{canEdit && <div><button type="button" onClick={() => void saveSelectedRelationNote()} disabled={isSaving || selectedNoteDraft === selectedRelation.note}>Сохранить</button><button type="button" onClick={() => void removeSelectedRelation()} disabled={isSaving}>Удалить связь</button></div>}</aside>}
    {error && <p className="dependency-error" role="alert">{error}</p>}
    <div ref={viewportRef} className="dependency-graph-scroll" onWheel={onGraphWheel} onPointerDown={startGraphPan} onPointerMove={moveGraphPan} onPointerUp={endGraphPan} onPointerCancel={endGraphPan}>
      <div className="dependency-zoom-readout">{Math.round(zoom * 100)}%</div>
      {isLoading ? <p className="dependency-empty">Загружаем связи…</p> : !graphNodes.length ? <p className="dependency-empty">Связей пока нет. Раскройте «Дополнительные опции», чтобы собрать первое дерево.</p> : <div className="dependency-canvas" style={{ width: layout.width, height: layout.height, transform: `translate(${pan.x}px, ${pan.y}px) scale(${zoom})` }}>
        <svg className="dependency-lines" width={layout.width} height={layout.height} viewBox={`0 0 ${layout.width} ${layout.height}`} aria-hidden="true"><defs><marker id="dependency-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" /></marker></defs>{graphRelations.map((relation) => { const direction = relationDirection(relation); const from = layout.positions.get(direction.from); const to = layout.positions.get(direction.to); if (!from || !to) return null; const fromX = from.x + NODE_WIDTH; const fromY = from.y + NODE_HEIGHT / 2; const toX = to.x; const toY = to.y + NODE_HEIGHT / 2; const curve = Math.max(46, Math.abs(toX - fromX) * .45); const meta = relationMeta[relation.relation_type]; return <g key={relation.id} className={`dependency-line ${meta.className} ${selectedRelationId === relation.id ? 'selected' : ''}`} onClick={() => selectRelation(relation.id)}><path className="dependency-line-hitbox" d={`M ${fromX} ${fromY} C ${fromX + curve} ${fromY}, ${toX - curve} ${toY}, ${toX} ${toY}`} /><path className="dependency-line-stroke" d={`M ${fromX} ${fromY} C ${fromX + curve} ${fromY}, ${toX - curve} ${toY}, ${toX} ${toY}`} markerEnd={meta.directed ? 'url(#dependency-arrow)' : undefined} /><text x={(fromX + toX) / 2} y={(fromY + toY) / 2 - 10} textAnchor="middle">{meta.label}</text></g>; })}</svg>
        {graphNodes.map((node) => { const position = layout.positions.get(node.id); if (!position) return null; return <button className={`dependency-node ${sourceId === node.id ? 'selected' : ''} ${node.completed ? 'completed' : ''}`} key={node.id} type="button" style={{ left: position.x, top: position.y }} onClick={() => selectNode(node.id)} onDoubleClick={() => onOpenCard(node.id)} title="Нажмите, чтобы выбрать. Двойной клик открывает карточку."><span className="dependency-node-list">{node.listTitle}</span><b>{node.title}</b><footer><span title={priorityLabel(node.priority)}>{node.priority ? '▮'.repeat(node.priority) : '—'}</span>{node.completed && <em>Выполнено</em>}</footer></button>; })}
      </div>}
    </div>
  </section>;
}
