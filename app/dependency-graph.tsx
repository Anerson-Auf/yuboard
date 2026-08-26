'use client';

import { useEffect, useMemo, useState } from 'react';
import './dependency-graph.css';

type RelationType = 'blocks' | 'depends_on' | 'duplicate' | 'related';

export type DependencyGraphNode = {
  id: string;
  title: string;
  listTitle: string;
  completed: boolean;
  priority?: number;
};

type DependencyRelation = {
  id: string;
  source_card_id: string;
  target_card_id: string;
  relation_type: RelationType;
  created_at: string;
};

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

function csrfHeaders() {
  const token = document.cookie.split('; ').find((item) => item.startsWith('flowboard_csrf='))?.slice('flowboard_csrf='.length);
  return token ? { 'Content-Type': 'application/json', 'x-flowboard-csrf': decodeURIComponent(token) } : { 'Content-Type': 'application/json' };
}

function relationDirection(relation: DependencyRelation) {
  return relation.relation_type === 'depends_on'
    ? { from: relation.target_card_id, to: relation.source_card_id }
    : { from: relation.source_card_id, to: relation.target_card_id };
}

function priorityLabel(priority?: number) {
  return priority ? `Приоритет ${priority} из 5` : 'Без приоритета';
}

export default function DependencyGraph({ boardId, nodes, canEdit, onOpenCard }: {
  boardId: string;
  nodes: DependencyGraphNode[];
  canEdit: boolean;
  onOpenCard: (cardId: string) => void;
}) {
  const [relations, setRelations] = useState<DependencyRelation[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState('');
  const [showUnrelated, setShowUnrelated] = useState(false);
  const [sourceId, setSourceId] = useState('');
  const [targetId, setTargetId] = useState('');
  const [relationType, setRelationType] = useState<RelationType>('blocks');
  const [selectedRelationId, setSelectedRelationId] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);

  const loadRelations = async () => {
    setIsLoading(true);
    setError('');
    try {
      const response = await fetch(`/v1/boards/${boardId}/relations`);
      if (!response.ok) throw new Error('load failed');
      const loaded = await response.json() as DependencyRelation[];
      setRelations(loaded.filter((relation) => nodeById.has(relation.source_card_id) && nodeById.has(relation.target_card_id)));
    } catch {
      setError('Не удалось загрузить связи. Обновите представление.');
    } finally {
      setIsLoading(false);
    }
  };

  // A board event refreshes the card list in the parent. Re-read relations
  // with it, so a link created by another teammate appears without reload.
  useEffect(() => { void loadRelations(); }, [boardId, nodes]); // eslint-disable-line react-hooks/exhaustive-deps

  const connectedIds = useMemo(() => new Set(relations.flatMap((relation) => [relation.source_card_id, relation.target_card_id])), [relations]);
  const graphNodes = useMemo(() => (showUnrelated ? nodes : nodes.filter((node) => connectedIds.has(node.id))), [connectedIds, nodes, showUnrelated]);
  const graphNodeIds = useMemo(() => new Set(graphNodes.map((node) => node.id)), [graphNodes]);
  const graphRelations = useMemo(() => relations.filter((relation) => graphNodeIds.has(relation.source_card_id) && graphNodeIds.has(relation.target_card_id)), [graphNodeIds, relations]);

  const layout = useMemo(() => {
    const rank = new Map(graphNodes.map((node) => [node.id, 0]));
    const indegree = new Map(graphNodes.map((node) => [node.id, 0]));
    const outgoing = new Map(graphNodes.map((node) => [node.id, [] as string[]]));
    graphRelations.filter((relation) => relationMeta[relation.relation_type].directed).forEach((relation) => {
      const { from, to } = relationDirection(relation);
      if (!outgoing.has(from) || !indegree.has(to)) return;
      outgoing.get(from)?.push(to);
      indegree.set(to, (indegree.get(to) ?? 0) + 1);
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
    graphNodes.forEach((node) => {
      const value = rank.get(node.id) ?? 0;
      const list = columns.get(value) ?? [];
      list.push(node);
      columns.set(value, list);
    });
    const layers = [...columns.entries()].sort(([left], [right]) => left - right).map(([, value]) => value.sort((left, right) => left.title.localeCompare(right.title, 'ru')));
    const positions = new Map<string, NodePosition>();
    layers.forEach((layer, columnIndex) => {
      layer.forEach((node, rowIndex) => positions.set(node.id, {
        x: PADDING + columnIndex * (NODE_WIDTH + COLUMN_GAP),
        y: PADDING + rowIndex * (NODE_HEIGHT + ROW_GAP),
      }));
    });
    return {
      positions,
      width: Math.max(680, PADDING * 2 + Math.max(1, layers.length) * NODE_WIDTH + Math.max(0, layers.length - 1) * COLUMN_GAP),
      height: Math.max(360, PADDING * 2 + Math.max(1, ...layers.map((layer) => layer.length)) * NODE_HEIGHT + Math.max(0, Math.max(1, ...layers.map((layer) => layer.length)) - 1) * ROW_GAP),
    };
  }, [graphNodes, graphRelations]);

  const selectedRelation = relations.find((relation) => relation.id === selectedRelationId) ?? null;

  async function createRelation() {
    if (!sourceId || !targetId || sourceId === targetId) return;
    setIsSaving(true);
    setError('');
    try {
      const response = await fetch(`/v1/cards/${sourceId}/relations`, {
        method: 'POST',
        headers: csrfHeaders(),
        body: JSON.stringify({ target_card_id: targetId, relation_type: relationType }),
      });
      if (!response.ok) throw new Error(await response.text());
      setTargetId('');
      await loadRelations();
    } catch {
      setError('Не удалось создать связь. Проверьте права и выбранные карточки.');
    } finally {
      setIsSaving(false);
    }
  }

  async function removeSelectedRelation() {
    if (!selectedRelation) return;
    setIsSaving(true);
    setError('');
    try {
      const response = await fetch(`/v1/cards/${selectedRelation.source_card_id}/relations/${selectedRelation.id}`, { method: 'DELETE', headers: csrfHeaders() });
      if (!response.ok) throw new Error(await response.text());
      setSelectedRelationId(null);
      await loadRelations();
    } catch {
      setError('Не удалось удалить связь.');
    } finally {
      setIsSaving(false);
    }
  }

  const selectNode = (id: string) => {
    setSourceId(id);
    if (targetId === id) setTargetId('');
    setSelectedRelationId(null);
  };

  return <section className="dependency-graph" aria-label="Граф зависимостей карточек">
    <header className="dependency-graph-header">
      <div><p className="eyebrow">СВЯЗИ КАРТОЧЕК</p><h2>Зависимости</h2><p>Стрелка показывает порядок работы: слева — условие, справа — задача, которая его ждёт.</p></div>
      <label className="dependency-unrelated-toggle"><input type="checkbox" checked={showUnrelated} onChange={(event) => setShowUnrelated(event.target.checked)} />Показать карточки без связей</label>
    </header>

    <div className="dependency-legend" aria-label="Типы связей">
      {(Object.keys(relationMeta) as RelationType[]).map((type) => <span key={type} className={`dependency-legend-item ${relationMeta[type].className}`}><i />{relationMeta[type].label}</span>)}
    </div>

    {canEdit && <form className="dependency-builder" onSubmit={(event) => { event.preventDefault(); void createRelation(); }}>
      <label>Откуда<select value={sourceId} onChange={(event) => { setSourceId(event.target.value); if (event.target.value === targetId) setTargetId(''); }}><option value="">Выберите карточку</option>{nodes.map((node) => <option key={node.id} value={node.id}>{node.title} · {node.listTitle}</option>)}</select></label>
      <label>Связь<select value={relationType} onChange={(event) => setRelationType(event.target.value as RelationType)}>{(Object.keys(relationMeta) as RelationType[]).map((type) => <option key={type} value={type}>{relationMeta[type].label}</option>)}</select></label>
      <label>С чем<select value={targetId} onChange={(event) => setTargetId(event.target.value)} disabled={!sourceId}><option value="">Выберите карточку</option>{nodes.filter((node) => node.id !== sourceId).map((node) => <option key={node.id} value={node.id}>{node.title} · {node.listTitle}</option>)}</select></label>
      <button className="create-button" type="submit" disabled={isSaving || !sourceId || !targetId || sourceId === targetId}>Связать</button>
      <small>{relationMeta[relationType].hint}</small>
    </form>}

    {selectedRelation && <aside className="dependency-selected-relation"><span><b>{nodeById.get(selectedRelation.source_card_id)?.title ?? 'Карточка'}</b> · {relationMeta[selectedRelation.relation_type].label.toLowerCase()} · <b>{nodeById.get(selectedRelation.target_card_id)?.title ?? 'Карточка'}</b></span>{canEdit && <button type="button" onClick={() => void removeSelectedRelation()} disabled={isSaving}>Удалить связь</button>}</aside>}
    {error && <p className="dependency-error" role="alert">{error}</p>}

    <div className="dependency-graph-scroll">
      {isLoading ? <p className="dependency-empty">Загружаем связи…</p> : !graphNodes.length ? <p className="dependency-empty">Связей пока нет. Выберите две карточки выше, чтобы собрать первое дерево.</p> : <div className="dependency-canvas" style={{ width: layout.width, height: layout.height }}>
        <svg className="dependency-lines" width={layout.width} height={layout.height} viewBox={`0 0 ${layout.width} ${layout.height}`} aria-hidden="true">
          <defs><marker id="dependency-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0,0 L8,4 L0,8 z" /></marker></defs>
          {graphRelations.map((relation) => {
            const direction = relationDirection(relation);
            const from = layout.positions.get(direction.from);
            const to = layout.positions.get(direction.to);
            if (!from || !to) return null;
            const fromX = from.x + NODE_WIDTH;
            const fromY = from.y + NODE_HEIGHT / 2;
            const toX = to.x;
            const toY = to.y + NODE_HEIGHT / 2;
            const curve = Math.max(46, Math.abs(toX - fromX) * 0.45);
            const meta = relationMeta[relation.relation_type];
            const midpointX = (fromX + toX) / 2;
            const midpointY = (fromY + toY) / 2;
            return <g key={relation.id} className={`dependency-line ${meta.className} ${selectedRelationId === relation.id ? 'selected' : ''}`} onClick={() => setSelectedRelationId(relation.id)}>
              <path className="dependency-line-hitbox" d={`M ${fromX} ${fromY} C ${fromX + curve} ${fromY}, ${toX - curve} ${toY}, ${toX} ${toY}`} />
              <path className="dependency-line-stroke" d={`M ${fromX} ${fromY} C ${fromX + curve} ${fromY}, ${toX - curve} ${toY}, ${toX} ${toY}`} markerEnd={meta.directed ? 'url(#dependency-arrow)' : undefined} />
              <text x={midpointX} y={midpointY - 10} textAnchor="middle">{meta.label}</text>
            </g>;
          })}
        </svg>
        {graphNodes.map((node) => {
          const position = layout.positions.get(node.id);
          if (!position) return null;
          return <button className={`dependency-node ${sourceId === node.id ? 'selected' : ''} ${node.completed ? 'completed' : ''}`} key={node.id} type="button" style={{ left: position.x, top: position.y }} onClick={() => selectNode(node.id)} onDoubleClick={() => onOpenCard(node.id)} title="Нажмите, чтобы выбрать. Двойной клик открывает карточку.">
            <span className="dependency-node-list">{node.listTitle}</span><b>{node.title}</b><footer><span title={priorityLabel(node.priority)}>{node.priority ? '▮'.repeat(node.priority) : '—'}</span>{node.completed && <em>Выполнено</em>}</footer>
          </button>;
        })}
      </div>}
    </div>
  </section>;
}
