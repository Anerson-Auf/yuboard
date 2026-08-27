import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const output = resolve(root, 'public', 'release-history.generated.json');
const buildMarkerOutput = resolve(root, 'public', 'build-marker.json');
const packageJson = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'));

function git(args) {
  try {
    return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  } catch {
    return '';
  }
}

const knownReleaseNotes = new Map([
  ['d1dc2da', 'Журнал версий больше не изменяется при сборке и не блокирует следующий deploy.'],
  ['808173c', 'Панель реакций не выходит за границы обсуждения; в карточке показана её текущая колонка.'],
  ['32797d4', 'Стикеры отображаются прямо внутри текста сообщения — в черновике и после отправки.'],
  ['01284d5', 'Добавлен постоянный журнал версий: можно открыть историю коммитов и листать её страницы.'],
  ['392c980', 'Выбранные стикеры остаются в черновике до явной отправки сообщения.'],
  ['7455a72', 'В чат добавлен встроенный набор стикеров с прозрачным отображением.'],
  ['cebf930', 'Участники могут самостоятельно присоединяться к карточке.'],
  ['830e7cc', 'Добавлены стикеры доски и реакции на сообщения в обсуждениях.'],
]);

function categoryFor(subject) {
  const prefix = /^(\w+)(?:\([^)]*\))?:\s*/i.exec(subject)?.[1]?.toLowerCase();
  if (prefix === 'fix') return 'Исправление';
  if (prefix === 'feat') return 'Новая возможность';
  if (prefix === 'style') return 'Интерфейс';
  if (prefix === 'refactor') return 'Внутреннее улучшение';
  if (prefix === 'docs') return 'Документация';
  if (prefix === 'test') return 'Проверки';
  if (prefix === 'chore') return 'Техническое изменение';
  if (/^revert/i.test(subject)) return 'Отмена изменения';
  return 'Изменение';
}

function releaseNoteFor(shortHash, subject) {
  const known = knownReleaseNotes.get(shortHash);
  if (known) return known;

  // Для новых коммитов текст после Conventional-префикса — готовая к показу release-note.
  // Мы не подменяем его абстрактным «разделом проекта»: так пользователю видно, что именно изменилось.
  const note = subject.replace(/^\w+(?:\([^)]*\))?:\s*/i, '').trim() || subject.trim();
  const sentence = note.charAt(0).toLocaleUpperCase('ru-RU') + note.slice(1);
  return sentence.endsWith('.') ? sentence : `${sentence}.`;
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
  const category = categoryFor(subject);
  const summary = releaseNoteFor(shortHash, subject);
  return { shortHash, date, author: author || 'Не указан', title: `${category}: ${summary}`, summary, files: [...new Set(files)].slice(0, 40), additions, deletions };
}

const raw = git(['log', '--reverse', '--date=short', '--format=%x1d%H%x1f%h%x1f%ad%x1f%an%x1f%s', '--numstat']);
if (!raw.trim()) {
  if (existsSync(output)) {
    mkdirSync(dirname(buildMarkerOutput), { recursive: true });
    writeFileSync(buildMarkerOutput, `${JSON.stringify({ build_id: `local-${Date.now()}`, revision: 'unknown', built_at: new Date().toISOString() })}\n`);
    process.exit(0);
  }
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
const revision = entries.at(-1)?.shortHash ?? 'unknown';
writeFileSync(buildMarkerOutput, `${JSON.stringify({ build_id: `${revision}-${Date.now()}`, revision, built_at: new Date().toISOString() })}\n`);
