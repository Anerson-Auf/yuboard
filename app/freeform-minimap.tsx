'use client';

import { PointerEvent as ReactPointerEvent, useMemo } from 'react';
import './freeform-minimap.css';

export type FreeformMinimapItem = {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  kind: 'column' | 'card';
};

type Viewport = { x: number; y: number; width: number; height: number };

type Props = {
  canvas: { width: number; height: number };
  items: FreeformMinimapItem[];
  viewport: Viewport;
  onNavigate: (point: { x: number; y: number }) => void;
};

const MAP_WIDTH = 204;
const MAP_HEIGHT = 136;

export default function FreeformMinimap({ canvas, items, viewport, onNavigate }: Props) {
  const scale = useMemo(() => Math.min(MAP_WIDTH / Math.max(canvas.width, 1), MAP_HEIGHT / Math.max(canvas.height, 1)), [canvas.height, canvas.width]);

  function navigate(event: ReactPointerEvent<HTMLButtonElement>) {
    const bounds = event.currentTarget.getBoundingClientRect();
    const mapX = Math.max(0, Math.min(MAP_WIDTH, (event.clientX - bounds.left) * MAP_WIDTH / bounds.width));
    const mapY = Math.max(0, Math.min(MAP_HEIGHT, (event.clientY - bounds.top) * MAP_HEIGHT / bounds.height));
    onNavigate({ x: mapX / scale, y: mapY / scale });
  }

  return <button type="button" className="freeform-minimap" aria-label="Мини-карта свободной доски" title="Мини-карта — нажмите, чтобы перейти" onPointerDown={(event) => { event.preventDefault(); navigate(event); }}>
    <svg viewBox={`0 0 ${MAP_WIDTH} ${MAP_HEIGHT}`} role="img" aria-label="Положение колонок и карточек">
      <rect className="freeform-minimap-surface" x="0" y="0" width={MAP_WIDTH} height={MAP_HEIGHT} rx="8" />
      {items.map((item) => <rect key={item.id} className={`freeform-minimap-item ${item.kind}`} x={item.x * scale} y={item.y * scale} width={Math.max(2, item.width * scale)} height={Math.max(2, item.height * scale)} rx={item.kind === 'column' ? 2 : 1} />)}
      <rect className="freeform-minimap-viewport" x={viewport.x * scale} y={viewport.y * scale} width={Math.max(8, viewport.width * scale)} height={Math.max(8, viewport.height * scale)} rx="3" />
    </svg>
    <span>Карта</span>
  </button>;
}
