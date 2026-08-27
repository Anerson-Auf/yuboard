'use client';

import { useEffect, useRef, useState } from 'react';

type BuildMarker = {
  build_id?: string;
};

const CHECK_INTERVAL_MS = 60_000;

export default function BuildUpdateNotice() {
  const knownBuildId = useRef<string | null>(null);
  const [isUpdateAvailable, setUpdateAvailable] = useState(false);

  useEffect(() => {
    let active = true;
    const checkForUpdate = async () => {
      const response = await fetch(`/build-marker.json?checked_at=${Date.now()}`, { cache: 'no-store' }).catch(() => null);
      if (!response?.ok) return;
      const marker = await response.json().catch(() => null) as BuildMarker | null;
      const buildId = typeof marker?.build_id === 'string' ? marker.build_id : '';
      if (!buildId || !active) return;
      if (knownBuildId.current === null) {
        knownBuildId.current = buildId;
        return;
      }
      if (knownBuildId.current !== buildId) setUpdateAvailable(true);
    };

    void checkForUpdate();
    const timer = window.setInterval(() => { void checkForUpdate(); }, CHECK_INTERVAL_MS);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, []);

  if (!isUpdateAvailable) return null;
  return <aside className="build-update-notice" role="status" aria-live="polite">
    <span><b>Новый патч готов</b><small>Обновите страницу, чтобы его увидеть</small></span>
    <button type="button" onClick={() => window.location.reload()}>Обновить</button>
  </aside>;
}
