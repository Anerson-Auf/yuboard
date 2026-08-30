/* Flowboard PWA: Web Push is intentionally visible and only carries the
 * minimal information needed to open the relevant card after a tap. */
self.addEventListener('push', (event) => {
  let payload = {};
  try { payload = event.data ? event.data.json() : {}; } catch { payload = {}; }
  const title = payload.title || 'Flowboard';
  const options = {
    body: payload.body || 'Новое уведомление',
    icon: '/flowboard-coin.png',
    badge: '/flowboard-coin.png',
    tag: payload.tag || undefined,
    renotify: Boolean(payload.tag),
    data: { url: payload.url || '/' }
  };
  event.waitUntil(self.registration.showNotification(title, options));
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const target = new URL(event.notification.data?.url || '/', self.location.origin).href;
  event.waitUntil((async () => {
    const clients = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
    const existing = clients.find((client) => client.url.startsWith(self.location.origin));
    if (existing) {
      await existing.focus();
      await existing.navigate(target);
      return;
    }
    await self.clients.openWindow(target);
  })());
});
