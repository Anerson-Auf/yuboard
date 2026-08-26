'use client';

import { useCallback, useLayoutEffect, useRef, useState } from 'react';

export type SegmentOption = { value: string; label: string; title?: string };

export default function SegmentedToggle({ className, label, options, value, onChange }: { className?: string; label: string; options: readonly SegmentOption[]; value: string; onChange: (value: string) => void }) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const buttonRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const [indicator, setIndicator] = useState({ left: 0, width: 0, ready: false });
  const syncIndicator = useCallback(() => {
    const index = options.findIndex((option) => option.value === value);
    const button = buttonRefs.current[index];
    if (!button) return;
    setIndicator({ left: button.offsetLeft, width: button.offsetWidth, ready: true });
  }, [options, value]);

  useLayoutEffect(() => {
    syncIndicator();
    const root = rootRef.current;
    if (!root) return;
    const observer = new ResizeObserver(syncIndicator);
    observer.observe(root);
    window.addEventListener('resize', syncIndicator);
    return () => { observer.disconnect(); window.removeEventListener('resize', syncIndicator); };
  }, [syncIndicator]);

  return <div className={`segmented-toggle${className ? ` ${className}` : ''}`} ref={rootRef} role="group" aria-label={label}>
    <span className="segmented-toggle-indicator" aria-hidden="true" style={{ width: indicator.width, transform: `translateX(${indicator.left}px)`, opacity: indicator.ready ? 1 : 0 }} />
    {options.map((option, index) => <button type="button" className={value === option.value ? 'active' : ''} onClick={() => onChange(option.value)} title={option.title} key={option.value} ref={(node) => { buttonRefs.current[index] = node; }}>{option.label}</button>)}
  </div>;
}
