import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(root, 'public', 'release-history.generated.json');
const packageJson = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));

function git(args) {
  try {
    return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  } catch {
    return '';
  }
}

function areaFor(subject) {
  const value = subject.toLowerCase();
  if (value.includes('discord')) return 'интеграция Discord';
  if (value.includes('github')) return 'интеграция GitHub';
  if (value.includes('freeform')) return 'свободная доска';
  if (value.includes('diagram')) return 'схемы карточек';
  if (value.includes('comment') || value.includes('discussion') || value.includes('thread')) return 'обсуждения';
  if (value.includes('checklist')) return 'чек-листы';
  if (value.includes('label') || value.includes('role')) return 'метки и роли';
  if (value.includes('notification') || value.includes('mention')) return 'уведомления';
  if (value.includes('attachment') || value.includes('media') || value.includes('cover') || value.includes('background')) return 'медиа и оформление';
  if (value.includes('drag') || value.includes('drop') || value.includes('panning')) return 'перетаскивание и навигация';
  if (value.includes('column') || value.includes('board')) return 'доски и колонки';
  if (value.includes('card')) return 'карточки';
  if (value.includes('workspace') || value.includes('home')) return 'пространство работы';
  if (value.includes('audio') || value.includes('voice')) return 'голосовые сообщения';
  if (value.includes('theme') || value.includes('style') || value.includes('ui')) return 'интерфейс';
  if (value.includes('migration') || value.includes('deploy') || value.includes('repository')) return 'техническая инфраструктура';
  return 'проект';
}

function categoryFor(subject) {
  const value = subject.toLowerCase();
  if (value.startsWith('revert')) return ['Отмена изменения', 'Возвращено безопасное состояние раздела'];
  if (/\b(fix|resolve|preserve|restore|prevent|clear|protect|enforce|restrict|hide|suppress|remove|stabilize)\b/.test(value)) return ['Исправление', 'Исправлена работа раздела'];
  if (/\b(style|compact|improve|refresh|update|normalize|reposition|increase|double|tone|lighten|refine|expand|simplify|iconify|fit|stretch|vary|disperse|enlarge|animate)\b/.test(value)) return ['Улучшение интерфейса', 'Улучшен интерфейс раздела'];
  if (/\b(chore|stage)\b/.test(value)) return ['Техническое обслуживание', 'Выполнено техническое обслуживание раздела'];
  if (/\b(test)\b/.test(value)) return ['Проверка', 'Добавлены или обновлены проверки раздела'];
  if (value === 'init') return ['Инициализация', 'Создана начальная версия проекта'];
  return ['Новая возможность', 'Расширены возможности раздела'];
}

function toEntry(header, lines) {
  const [, shortHash, date, author, subject = ''] = header.slice(1).split('\x1f');
  const files = [];
  let additions = 0;
  let deletions = 0;
  for (const line of lines) {
    const match = /^(\d+|-?)\t(\d+|-?)\t(.+)$/.exec(line.trim());
    if (!match) continue;
    additions += Number(match[1]) || 0;
    deletions += Number(match[2]) || 0;
    files.push(match[3]);
  }
  const [category, summaryLead] = categoryFor(subject);
  const area = areaFor(subject);
  return { shortHash, date, author: author || 'Не указан', title: `${category}: ${area}`, summary: `${summaryLead} «${area}».`, files: [...new Set(files)].slice(0, 40), additions, deletions };
}

const raw = git(['log', '--reverse', '--date=short', '--format=%x1d%H%x1f%h%x1f%ad%x1f%an%x1f%s', '--numstat']);
if (!raw.trim()) {
  if (existsSync(output)) process.exit(0);
  console.warn('Git history is unavailable; release history was not generated.');
  process.exit(0);
}

const entries = [];
let header = '';
let lines = [];
for (const line of raw.split(/\r?\n/)) {
  if (line.startsWith('\x1d')) {
    if (header) entries.push(toEntry(header, lines));
    header = line;
    lines = [];
  } else if (header) {
    lines.push(line);
  }
}
if (header) entries.push(toEntry(header, lines));
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, `${JSON.stringify({ version: packageJson.version, revision: entries.at(-1)?.shortHash ?? 'unknown', entries: entries.reverse() }, null, 2)}\n`);
