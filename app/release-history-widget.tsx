'use client';

import { useEffect, useMemo, useState } from 'react';

type ReleaseHistoryEntry = {
  shortHash: string;
  date: string;
  author: string;
  title: string;
  summary: string;
  files: string[];
  additions: number;
  deletions: number;
};

type ReleaseHistory = {
  version: string;
  revision: string;
  entries: ReleaseHistoryEntry[];
};

const PAGE_SIZE = 8;

function formatDate(value: string) {
  const date = new Date(`${value}T12:00:00`);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleDateString('ru-RU', { day: 'numeric', month: 'long', year: 'numeric' });
}

export default function ReleaseHistoryWidget() {
  const [history, setHistory] = useState<ReleaseHistory | null>(null);
  const [isOpen, setOpen] = useState(false);
  const [page, setPage] = useState(0);

  useEffect(() => {
    let active = true;
    void fetch('/release-history.json', { cache: 'no-store' })
      .then((response) => response.ok ? response.json() as Promise<ReleaseHistory> : null)
      .then((data) => { if (active && data) setHistory(data); })
      .catch(() => undefined);
    return () => { active = false; };
  }, []);

  const totalPages = Math.max(1, Math.ceil((history?.entries.length ?? 0) / PAGE_SIZE));
  const entries = useMemo(() => history?.entries.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE) ?? [], [history, page]);
  const label = history ? `v${history.version}` : 'Версия';

  return <aside className={`release-history-widget ${isOpen ? 'is-open' : ''}`} aria-label="Журнал версий">
    {isOpen && <section className="release-history-panel" role="dialog" aria-label="История версий">
      <header>
        <div><span>ЖУРНАЛ ВЕРСИЙ</span><h2>{label}</h2></div>
        <button type="button" onClick={() => setOpen(false)} aria-label="Закрыть журнал версий">×</button>
      </header>
      {history ? <>
        <p className="release-history-caption">{history.entries.length} изменений · ревизия {history.revision}</p>
        <div className="release-history-list">
          {entries.map((entry) => <details key={entry.shortHash}>
            <summary><time dateTime={entry.date}>{formatDate(entry.date)}</time><b>{entry.title}</b><span>{entry.shortHash}</span></summary>
            <div className="release-history-details">
              <p>{entry.summary}</p>
              <small>Автор: {entry.author} · {entry.files.length} файлов · +{entry.additions} / −{entry.deletions}</small>
              {entry.files.length > 0 && <ul aria-label="Затронутые файлы">{entry.files.map((file) => <li key={file}>{file}</li>)}</ul>}
            </div>
          </details>)}
        </div>
        <footer>
          <button type="button" onClick={() => setPage((current) => Math.max(0, current - 1))} disabled={page === 0}>← Новее</button>
          <span>{page + 1} / {totalPages}</span>
          <button type="button" onClick={() => setPage((current) => Math.min(totalPages - 1, current + 1))} disabled={page >= totalPages - 1}>Старее →</button>
        </footer>
      </> : <p className="release-history-caption">Загружаем историю…</p>}
    </section>}
    <button type="button" className="release-history-trigger" onClick={() => setOpen((current) => !current)} aria-expanded={isOpen}>
      <span>FLOWBOARD</span><b>{label}</b>{history && <small>{history.entries.length} изменений</small>}
    </button>
  </aside>;
}
