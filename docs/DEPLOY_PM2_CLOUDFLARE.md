# Flowboard на занятой SSH-ноду: PM2 + существующий Caddy/Nginx + Cloudflare

Этот вариант предназначен для сервера, где `80` и `443` уже заняты рабочим Caddy, Nginx или панелью. Flowboard **не получает эти порты** и не мешает существующим сервисам.

Схема:

```text
Интернет → Cloudflare → существующий Caddy/Nginx :443
                               ├─ /v1/*, /health → 127.0.0.1:8100 (Flowboard API)
                               └─ всё остальное  → 127.0.0.1:3100 (Flowboard web)
```

`3100`, `8100` и PostgreSQL остаются доступными только с самого сервера. Не открывайте их в firewall и не добавляйте в Cloudflare напрямую.

## 0. Что потребуется

- отдельный поддомен, например `board.example.com`;
- SSH-доступ с `sudo`;
- работающий Caddy или Nginx, который уже принимает `80/443`;
- Node.js `22.13+`, Rust stable, PostgreSQL 15+ и PM2.

Проверить установленное:

```bash
node --version
npm --version
rustc --version
cargo --version
pm2 --version
psql --version
```

Если Node или Rust не установлены, сначала установите их штатным способом вашей системы. Не меняйте глобальную версию Node у работающих сервисов без проверки совместимости.

## 1. Cloudflare и DNS

1. В **Cloudflare → DNS** создайте запись `A`: имя `board`, IP вашего VPS.
2. Оставьте оранжевое облако включённым (**Proxied**).
3. В **SSL/TLS → Overview** выберите **Full (strict)**.
4. На уже работающем Caddy/Nginx должен быть валидный сертификат для `board.example.com`: Let's Encrypt или Cloudflare Origin Certificate.

Cloudflare не должен ходить напрямую на `3100` или `8100`. Он ходит на стандартный HTTPS origin (`443`), а уже существующий proxy передаёт трафик в локальные порты.

## 2. Создать пользователя и подготовить каталог

Ниже используется отдельный системный пользователь `flowboard`. Выполняйте команды от своего sudo-пользователя:

```bash
sudo adduser --disabled-password --gecos '' flowboard
sudo mkdir -p /opt/flowboard /var/lib/flowboard/uploads
sudo chown -R flowboard:flowboard /opt/flowboard /var/lib/flowboard
sudo -iu flowboard
git clone <URL_ВАШЕГО_РЕПОЗИТОРИЯ> /opt/flowboard
cd /opt/flowboard
chmod +x deploy/flowboard-pm2.sh
```

Если репозиторий уже склонирован, не делайте второй clone: перейдите в его каталог и выполните `git pull`.

## 3. PostgreSQL

Используйте уже установленный PostgreSQL либо создайте отдельную базу. Не используйте базу другого приложения и не открывайте порт `5432` наружу.

От sudo-пользователя:

```bash
sudo -u postgres psql
```

В открывшейся консоли PostgreSQL выполните, подставив пароль из `openssl rand -hex 24`:

```sql
CREATE USER flowboard WITH LOGIN PASSWORD 'PUT_A_LONG_RANDOM_PASSWORD_HERE';
CREATE DATABASE flowboard OWNER flowboard;
\q
```

## 4. Production `.env`

От пользователя `flowboard` создайте файл `/opt/flowboard/.env`:

```bash
cd /opt/flowboard
cp .env.example .env
nano .env
```

Замените значения на следующие, подставив свой домен и пароль базы:

```dotenv
FLOWBOARD_DATABASE_URL=postgres://flowboard:PUT_A_LONG_RANDOM_PASSWORD_HERE@127.0.0.1:5432/flowboard
FLOWBOARD_UPLOAD_DIR=/var/lib/flowboard/uploads
FLOWBOARD_BIND_ADDR=127.0.0.1:8100
FLOWBOARD_COOKIE_SECURE=true
FLOWBOARD_API_ORIGIN=https://board.example.com
NEXT_PUBLIC_FLOWBOARD_API_URL=
```

Удалите из файла неиспользуемые local-development S3 переменные. После сохранения:

```bash
chmod 600 .env
```

Пароль в URL не должен содержать `@`, `:`, `/`, `?` или `#`; `openssl rand -hex 24` безопасен для этого случая.

## 5. Первый запуск через PM2

Под пользователем `flowboard`:

```bash
cd /opt/flowboard
./deploy/flowboard-pm2.sh deploy
./deploy/flowboard-pm2.sh status
curl http://127.0.0.1:8100/health
```

Скрипт делает следующее в безопасном порядке:

1. ставит зависимости по lockfile (`npm ci`);
2. собирает frontend;
3. собирает Rust API в release с максимум шестью jobs;
4. перезагружает **только** `flowboard-web` и `flowboard-api` в PM2;
5. проверяет оба локальных endpoint.

Ни один чужой PM2-процесс не перезапускается. Миграции базы запускаются API автоматически при старте.

Чтобы процессы переживали reboot сервера:

```bash
pm2 save
pm2 startup systemd -u flowboard --hp /home/flowboard
```

PM2 выведет одну команду с `sudo`; скопируйте и выполните её **от sudo-пользователя**, затем снова выполните `pm2 save` от `flowboard`.

## 6. Добавить hostname в существующий Caddy

Не создавайте второй Caddy и не запускайте `caddy start`: отредактируйте конфигурацию уже работающего сервиса. Добавьте отдельный сайт, заменив домен:

```caddy
board.example.com {
    encode zstd gzip

    @api path /v1/* /health
    reverse_proxy @api 127.0.0.1:8100
    reverse_proxy 127.0.0.1:3100
}
```

Проверьте и примените **существующую** конфигурацию:

```bash
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
curl -I https://board.example.com/health
```

## 7. Альтернатива: блок для существующего Nginx

Если ваш ingress — Nginx, добавьте отдельный `server` для домена в его обычный каталог виртуальных хостов. Сертификатные строки не копируйте вслепую: используйте тот же способ TLS, что и у текущих сайтов.

```nginx
server {
    listen 443 ssl http2;
    server_name board.example.com;
    client_max_body_size 55m;

    # ssl_certificate /...;       # уже выданы вашим способом
    # ssl_certificate_key /...;

    location /v1/ {
        proxy_pass http://127.0.0.1:8100;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_buffering off;
        proxy_read_timeout 3600s;
    }

    location = /health {
        proxy_pass http://127.0.0.1:8100;
    }

    location / {
        proxy_pass http://127.0.0.1:3100;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

После добавления:

```bash
sudo nginx -t
sudo systemctl reload nginx
```

## 8. Финальная проверка

1. Откройте `https://board.example.com` в приватном окне.
2. Создайте первый аккаунт: он станет system owner.
3. Создайте invite и активируйте второй аккаунт в другом браузере.
4. Убедитесь, что на втором аккаунте нет доступа до добавления в конкретный проект.
5. Проверьте upload изображения до 50 МиБ и обновление с другой вкладки.

Полезные команды:

```bash
sudo -iu flowboard
cd /opt/flowboard
./deploy/flowboard-pm2.sh status
./deploy/flowboard-pm2.sh logs
pm2 logs flowboard-api --lines 100
curl http://127.0.0.1:8100/health
```

## 9. Обновление и откат

Перед обновлением сделайте backup базы и uploads. Затем:

```bash
sudo -iu flowboard
cd /opt/flowboard
git pull --ff-only
./deploy/flowboard-pm2.sh deploy
```

Если обновление не прошло, вернитесь на известный commit и снова выполните deploy:

```bash
git log --oneline -10
git checkout <ПРОВЕРЕННЫЙ_COMMIT>
./deploy/flowboard-pm2.sh deploy
```

Не используйте `pm2 delete all`, `pm2 restart all`, `docker system prune` или команды, затрагивающие чужие сервисы.

## 10. Backup

Минимум раз в сутки сохраняйте отдельно PostgreSQL и uploads, затем копируйте оба backup за пределы VPS:

```bash
sudo mkdir -p /var/backups/flowboard
sudo chown flowboard:flowboard /var/backups/flowboard
sudo -u postgres pg_dump flowboard | gzip > /var/backups/flowboard/postgres-$(date +%F).sql.gz
sudo tar -C /var/lib/flowboard -czf /var/backups/flowboard/uploads-$(date +%F).tgz uploads
```

Проверьте восстановление на тестовой базе. Backup на том же сервере не защищает от потери самого сервера.
