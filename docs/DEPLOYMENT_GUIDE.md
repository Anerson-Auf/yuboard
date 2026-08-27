# Flowboard: развёртывание с нуля

Этот guide рассчитан на человека, который впервые разворачивает приложение на Linux. В результате получится один HTTPS-адрес Flowboard; база данных и uploads останутся закрытыми внутри Docker.

## Что нужно заранее

- VPS с Ubuntu 22.04 или 24.04, минимум 2 GB RAM и 20 GB диска;
- домен, например `board.example.ru`;
- доступ к DNS домена и SSH-доступ к серверу под пользователем с `sudo`;
- URL Git-репозитория Flowboard.

Не продолжайте, пока A-record `board.example.ru` не указывает на публичный IP сервера. Проверка с вашего компьютера:

```bash
nslookup board.example.ru
```

IP в ответе должен совпасть с IP VPS. Для первого запуска откройте у хостера и в firewall только TCP `22`, `80` и `443`.

## 1. Войти на сервер и обновить систему

На своём компьютере выполните (подставьте IP):

```bash
ssh root@203.0.113.10
```

Создайте отдельного пользователя. Скопируйте свой публичный SSH key, когда команда попросит пароль:

```bash
adduser flowboard
usermod -aG sudo flowboard
rsync --archive --chown=flowboard:flowboard ~/.ssh /home/flowboard
exit
ssh flowboard@203.0.113.10
```

Установите Docker, Git и Caddy:

```bash
sudo apt update
sudo apt upgrade -y
sudo apt install -y ca-certificates curl git caddy
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker "$USER"
exit
```

Войдите заново и убедитесь, что Docker работает без `sudo`:

```bash
ssh flowboard@203.0.113.10
docker version
```

## 2. Скачать приложение и создать секреты

```bash
git clone <URL_ВАШЕГО_РЕПОЗИТОРИЯ> ~/flowboard
cd ~/flowboard
cp .env.example .env
nano .env
```

Полностью замените содержимое `.env` следующим шаблоном, изменив домен и пароль:

```dotenv
POSTGRES_DB=flowboard
POSTGRES_USER=flowboard
POSTGRES_PASSWORD=replace-with-a-random-32-character-password
FLOWBOARD_PUBLIC_ORIGIN=https://board.example.ru
FLOWBOARD_PROXY_BIND=127.0.0.1:8081
# Set both values to the matching Discord bridge configuration to refresh expired media.
FLOWBOARD_DISCORD_MEDIA_REFRESH_URL=https://discord-bridge.example.ru/api/flowboard/attachments/refresh
FLOWBOARD_DISCORD_MEDIA_REFRESH_SIGNING_SECRET=replace-with-the-shared-bridge-secret
```

Сгенерировать безопасный пароль можно так:

```bash
openssl rand -hex 24
```

Используйте только буквы, цифры, `.`, `_` и `-`: значение передаётся в URL подключения PostgreSQL. Сохраните файл в `nano` сочетанием `Ctrl+O`, затем `Enter`, выйдите через `Ctrl+X`. Ограничьте доступ:

```bash
chmod 600 .env
```

Никогда не отправляйте `.env` в Git, чат или issue.

## 3. Запустить Flowboard и проверить внутренний health check

```bash
cd ~/flowboard
docker compose -f docker-compose.production.yml up -d --build
docker compose -f docker-compose.production.yml ps
curl http://127.0.0.1:8081/health
```

Ожидаемый ответ содержит `"status":"ok"` и `"database":"ready"`. Если контейнер не имеет статуса `running`, посмотрите только его логи:

```bash
docker compose -f docker-compose.production.yml logs --tail=100 api
```

Миграции PostgreSQL применяются API автоматически при запуске. Не запускайте SQL вручную и не удаляйте volumes: в них живут база и файлы.

## 4. Подключить HTTPS

Откройте Caddyfile:

```bash
sudo nano /etc/caddy/Caddyfile
```

Добавьте, заменив домен:

```caddy
board.example.ru {
    reverse_proxy 127.0.0.1:8081 {
        header_up -X-Forwarded-For
        header_up X-Forwarded-For {remote_host}
    }
}
```

Проверьте синтаксис и примените:

```bash
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
curl -I https://board.example.ru/health
```

Caddy самостоятельно получит и будет продлевать TLS-сертификат. Если сертификат не выпускается, сначала проверьте DNS и что порты `80/443` не заняты другим reverse proxy.

## 5. Первый вход и проверка доступа

1. Откройте `https://board.example.ru` в браузере.
2. Создайте первый `@ник` и пароль. Этот аккаунт становится **system owner**.
3. В администрировании создайте account-invite и передайте ссылку пользователю безопасным каналом.
4. Новый пользователь активирует только свой аккаунт. Затем system owner или workspace owner добавляет его в нужный workspace через `Команда`.
5. В отдельном браузере проверьте viewer и contributor: viewer не может менять карточки, contributor не может удалять карточки или колонки.

Не создавайте общие аккаунты. Каждый человек должен иметь собственный `@ник`: это необходимо для audit log и отзыва сессий.

## 6. Ежедневные операции

### Учетные записи и доступ

System owner создаёт account-invites в приложении, а не в базе данных и не через shell. Если устройство потеряно, пользователь открывает `Сессии` в Flowboard и отзывает конкретную сессию либо все остальные. При блокировке аккаунта system owner все его sessions отзываются автоматически.

Login и activation invite ограничены по частоте. Не пытайтесь обходить это повторными запросами: подождите несколько минут или проверьте, что введён правильный `@ник` / invite link.

Архивирование workspace обратимо и подходит для временного закрытия проекта. Удаление workspace необратимо: оно удаляет доски, карточки и uploads этого пространства, поэтому сначала сделайте backup.

### Обновление приложения

Перед обновлением сделайте backup из следующего раздела, затем:

```bash
cd ~/flowboard
git pull
docker compose -f docker-compose.production.yml up -d --build
docker compose -f docker-compose.production.yml ps
curl https://board.example.ru/health
```

Если проверка не проходит, верните предыдущий commit через `git log --oneline`, затем `git checkout <предыдущий_commit>` и повторите `docker compose ... up -d --build`. Не используйте `docker volume rm`.

### Просмотр логов

```bash
cd ~/flowboard
docker compose -f docker-compose.production.yml logs --tail=100 api
docker compose -f docker-compose.production.yml logs --tail=100 proxy
```

## 7. Backups и проверка восстановления

Создайте папку вне репозитория:

```bash
mkdir -p ~/flowboard-backups
chmod 700 ~/flowboard-backups
```

Создать дамп PostgreSQL:

```bash
cd ~/flowboard
docker compose -f docker-compose.production.yml exec -T postgres pg_dump -U flowboard flowboard | gzip > ~/flowboard-backups/flowboard-$(date +%F).sql.gz
```

Uploads находятся в Docker volume `uploads_data`. Сохраните его архив вместе с дампом:

```bash
docker run --rm -v flowboard_uploads_data:/data -v ~/flowboard-backups:/backup alpine sh -c 'tar czf /backup/uploads-$(date +%F).tgz -C /data .'
ls -lh ~/flowboard-backups
```

Скопируйте оба файла на другое устройство или object storage. Backup, который остаётся на том же VPS, не защищает от потери сервера.

Раз в месяц проверьте восстановление на отдельном тестовом сервере: поднимите чистый PostgreSQL, выполните `gunzip -c <dump> | psql ...`, восстановите uploads в чистый volume и войдите в тестовый Flowboard. Backup без restore test не считается проверенным.

## 8. Что нельзя делать

- Не публикуйте `5432`, `8080` или MinIO наружу.
- Не выключайте `FLOWBOARD_COOKIE_SECURE=true` в production.
- Не используйте `docker compose down -v` на production: это удалит данные.
- Не храните PostgreSQL пароль в shell history, Git или скриншотах.
- Не отключайте Caddy без другого HTTPS reverse proxy.
