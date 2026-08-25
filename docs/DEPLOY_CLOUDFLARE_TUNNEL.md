# Flowboard без sudo: Cloudflare Tunnel + PM2

Этот вариант не использует существующий Nginx, публичные `80/443` и origin TLS certificates. Он безопасен для нагруженной ноды: Cloudflare Tunnel создаёт только исходящие соединения и публикует новый hostname.

## Границы изменений

- новый hostname: `flowboard.zei.su`;
- новые процессы: `flowboard-web`, `flowboard-api`, `flowboard-tunnel`;
- отдельный PM2 каталог: `~/.pm2-flowboard`;
- локальные порты: `127.0.0.1:3100`, `127.0.0.1:8100`;
- не меняются: текущий Nginx, `dash.yufu.su`, публичные порты, чужие PM2-приложения.

## 1. Авторизовать cloudflared

```bash
mkdir -p ~/.cloudflared
chmod 700 ~/.cloudflared
~/cloudflared tunnel login
```

Откройте URL, который напечатает команда, в своём браузере и выберите zone `zei.su`. После успеха появится `~/.cloudflared/cert.pem`. Не передавайте этот файл, его содержимое и URL авторизации кому-либо.

## 2. Создать пустой tunnel

```bash
~/cloudflared tunnel create flowboard
~/cloudflared tunnel list
```

Команда напечатает UUID и создаст credential JSON в `~/.cloudflared/`. На этом этапе DNS ещё не меняется, новый hostname не принимает трафик.

## 3. Конфигурация маршрутов

Создайте `~/.cloudflared/flowboard.yml`, заменив UUID:

```yaml
tunnel: YOUR_TUNNEL_UUID
credentials-file: /home/dash/.cloudflared/YOUR_TUNNEL_UUID.json

ingress:
  - hostname: flowboard.zei.su
    path: ^/v1/.*$
    service: http://127.0.0.1:8100
  - hostname: flowboard.zei.su
    path: ^/health$
    service: http://127.0.0.1:8100
  - hostname: flowboard.zei.su
    service: http://127.0.0.1:3100
  - service: http_status:404
```

Проверьте без запуска:

```bash
~/cloudflared --config ~/.cloudflared/flowboard.yml tunnel ingress validate
```

## 4. Привязать DNS

Только после успешной проверки создайте DNS route:

```bash
~/cloudflared tunnel route dns flowboard flowboard.zei.su
```

Команда создаст CNAME на `<UUID>.cfargotunnel.com`. Не создавайте A-record с IP ноды вручную.

## 5. Запустить Flowboard и tunnel

Подготовьте `.env` с PostgreSQL URL, `FLOWBOARD_BIND_ADDR=127.0.0.1:8100`, `FLOWBOARD_COOKIE_SECURE=true` и `FLOWBOARD_API_ORIGIN=https://flowboard.zei.su`. После Node.js 22:

```bash
cd ~/flowboard
chmod +x deploy/flowboard-pm2.sh deploy/flowboard-tunnel.sh
./deploy/flowboard-pm2.sh deploy
./deploy/flowboard-pm2.sh tunnel
```

## Откат

```bash
cd ~/flowboard
./deploy/flowboard-pm2.sh stop
PM2_HOME="$HOME/.pm2-flowboard" pm2 stop flowboard-tunnel
~/cloudflared tunnel route dns delete flowboard.zei.su
```

Откат не влияет на Nginx или другие PM2-приложения.
