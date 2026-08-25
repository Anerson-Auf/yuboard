# Развёртывание Flowboard на SSH-ноду

Эта инструкция разворачивает Flowboard на одном Linux-сервере: PostgreSQL, Rust API и веб-интерфейс работают в Docker, а Caddy выдаёт и продлевает HTTPS-сертификат.

## 1. Подготовить сервер

Нужны Ubuntu 22.04+ (или другой Linux с Docker), домен с A-записью на IP сервера и открытые порты `80` и `443`.

```bash
sudo apt update
sudo apt install -y ca-certificates curl git caddy
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker "$USER"
```

Перезайдите по SSH после последней команды, чтобы группа `docker` применилась.

## 2. Загрузить проект и задать секреты

```bash
git clone <URL_РЕПОЗИТОРИЯ> flowboard
cd flowboard
cp .env.example .env
nano .env
```

В `.env` укажите production-значения:

```dotenv
POSTGRES_DB=flowboard
POSTGRES_USER=flowboard
POSTGRES_PASSWORD=CHANGE_ME_TO_A_LONG_RANDOM_VALUE
FLOWBOARD_PUBLIC_ORIGIN=https://board.example.ru
FLOWBOARD_PROXY_BIND=127.0.0.1:8081
```

Для `POSTGRES_PASSWORD` используйте минимум 24 случайных символа из букв, цифр, `.` `_` и `-`. Символы вроде `@`, `:`, `/`, `?` нельзя использовать: пароль включается в URL подключения к PostgreSQL.

Файл `.env` не коммитится. Ограничьте доступ:

```bash
chmod 600 .env
```

## 3. Запустить приложение

```bash
docker compose -f docker-compose.production.yml up -d --build
docker compose -f docker-compose.production.yml ps
curl http://127.0.0.1:8081/health
```

Ожидаемый ответ health endpoint содержит `"status":"ok"` и `"database":"ready"`.

## 4. Подключить HTTPS через Caddy

Откройте конфигурацию Caddy:

```bash
sudo nano /etc/caddy/Caddyfile
```

Добавьте блок, заменив домен:

```caddy
board.example.ru {
    reverse_proxy 127.0.0.1:8081 {
        header_up -X-Forwarded-For
        header_up X-Forwarded-For {remote_host}
    }
}
```

Проверьте и примените конфигурацию:

```bash
sudo caddy validate --config /etc/caddy/Caddyfile
sudo systemctl reload caddy
```

Caddy самостоятельно выпустит TLS-сертификат. После этого откройте `https://board.example.ru`: первый зарегистрированный пользователь станет system owner, а новые аккаунты будут активироваться только по account-invite. Полный guide: [DEPLOYMENT_GUIDE.md](DEPLOYMENT_GUIDE.md).

## Обновление

```bash
cd ~/flowboard
git pull
docker compose -f docker-compose.production.yml up -d --build
docker image prune -f
```

Миграции PostgreSQL запускаются API автоматически. Не удаляйте Docker volumes `postgres_data` и `uploads_data`: там находятся данные досок и вложения.

## Резервная копия PostgreSQL

```bash
mkdir -p ~/flowboard-backups
docker compose -f docker-compose.production.yml exec -T postgres pg_dump -U flowboard flowboard | gzip > ~/flowboard-backups/flowboard-$(date +%F).sql.gz
```

Проверьте восстановление резервной копии на отдельной тестовой базе до того, как рассчитывать на неё в production.

## Если TLS уже выдаёт панель или Nginx

Оставьте `FLOWBOARD_PROXY_BIND=127.0.0.1:8081` и направьте существующий HTTPS reverse proxy на `http://127.0.0.1:8081`. Не открывайте PostgreSQL (`5432`) и Rust API (`8080`) наружу.
