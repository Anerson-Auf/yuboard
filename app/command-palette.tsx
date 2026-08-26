'use client';

import { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import './command-palette.css';

/** An operation shown in the command palette. */
export type Action = {
  /** Stable key. It is used for keyboard focus and React rendering. */
  id?: string;
  /** Short name shown in the result list. */
  label?: string;
  /** Explains the effect before the user runs the command. */
  description?: string;
  /** Extra terms that should find this action without appearing in the UI. */
  keywords?: string[];
  /** Hint displayed at the right edge of the action. */
  shortcut?: string;
  disabled?: boolean;
  onSelect: () => void;
};

type PaletteAction = Required<Pick<Action, 'id' | 'label'>> & Action & {
  group: string;
  defaultDescription: string;
  defaultShortcut?: string;
};

export type CommandPaletteProps = {
  /** Opens the card composer. */
  createCard?: Action;
  /** Opens one board action or a list of boards to choose from. */
  goToBoard?: Action | Action[];
  /** Assigns the signed-in user to the currently open card. */
  assignSelf?: Action;
  /** Opens the notification center. */
  notifications?: Action;
  /** Opens the current user's cross-board task queue. */
  myTasks?: Action;
  /** Opens board filters. */
  filters?: Action;
  placeholder?: string;
  /** Set false when the host page provides its own visible opener. Ctrl/Cmd+K still works. */
  showTrigger?: boolean;
  triggerLabel?: string;
};

const normalize = (value: string) => value.toLocaleLowerCase('ru-RU').trim();

function paletteAction(
  action: Action,
  defaults: Omit<PaletteAction, keyof Action | 'onSelect'>,
): PaletteAction {
  return {
    ...defaults,
    ...action,
    id: action.id ?? defaults.id,
    label: action.label ?? defaults.label,
    description: action.description ?? defaults.defaultDescription,
    shortcut: action.shortcut ?? defaults.defaultShortcut,
  };
}

export default function CommandPalette({
  createCard,
  goToBoard,
  assignSelf,
  notifications,
  myTasks,
  filters,
  placeholder = 'Введите команду или найдите действие…',
  showTrigger = true,
  triggerLabel = 'Команды',
}: CommandPaletteProps) {
  const [isOpen, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const listId = useId();

  const allActions = useMemo(() => {
    const result: PaletteAction[] = [];
    if (createCard) result.push(paletteAction(createCard, {
      id: 'create-card', label: 'Создать карточку', group: 'Создать', defaultDescription: 'Добавить новую задачу на текущую доску', defaultShortcut: 'C',
    }));
    if (goToBoard) {
      const boards = Array.isArray(goToBoard) ? goToBoard : [goToBoard];
      result.push(...boards.map((board, index) => paletteAction(board, {
        id: `go-to-board-${index}`, label: 'Перейти к доске', group: 'Навигация', defaultDescription: 'Открыть другую доску',
      })));
    }
    if (assignSelf) result.push(paletteAction(assignSelf, {
      id: 'assign-self', label: 'Назначить меня', group: 'Карточка', defaultDescription: 'Добавить себя в исполнители текущей карточки',
    }));
    if (notifications) result.push(paletteAction(notifications, {
      id: 'notifications', label: 'Открыть уведомления', group: 'Навигация', defaultDescription: 'Перейти к непрочитанным уведомлениям',
    }));
    if (myTasks) result.push(paletteAction(myTasks, {
      id: 'my-tasks', label: 'Открыть мои задачи', group: 'Навигация', defaultDescription: 'Посмотреть назначенные вам карточки из всех проектов',
    }));
    if (filters) result.push(paletteAction(filters, {
      id: 'filters', label: 'Открыть фильтры', group: 'Доска', defaultDescription: 'Настроить отображение карточек', defaultShortcut: 'F',
    }));
    return result;
  }, [assignSelf, createCard, filters, goToBoard, myTasks, notifications]);

  const visibleActions = useMemo(() => {
    const terms = normalize(query).split(/\s+/).filter(Boolean);
    if (!terms.length) return allActions;
    return allActions.filter((action) => {
      const haystack = normalize([action.label, action.description, action.group, ...(action.keywords ?? [])].filter(Boolean).join(' '));
      return terms.every((term) => haystack.includes(term));
    });
  }, [allActions, query]);

  const groupedActions = useMemo(() => {
    const groups = new Map<string, PaletteAction[]>();
    for (const action of visibleActions) groups.set(action.group, [...(groups.get(action.group) ?? []), action]);
    return [...groups.entries()];
  }, [visibleActions]);

  const close = useCallback((restoreFocus = true) => {
    setOpen(false);
    setQuery('');
    setActiveIndex(0);
    if (restoreFocus) requestAnimationFrame(() => previouslyFocusedRef.current?.focus());
  }, []);

  const open = useCallback(() => {
    previouslyFocusedRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setQuery('');
    setActiveIndex(0);
    setOpen(true);
  }, []);

  const runAction = useCallback((action: PaletteAction | undefined) => {
    if (!action || action.disabled) return;
    close(false);
    action.onSelect();
  }, [close]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // `key` depends on the active keyboard layout (Russian layout yields
      // "л" here), while `code` is the physical Ctrl+K key the UI promises.
      if ((event.ctrlKey || event.metaKey) && (event.code === 'KeyK' || event.key.toLowerCase() === 'k')) {
        event.preventDefault();
        if (isOpen) close(); else open();
        return;
      }
      if (!isOpen) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        close();
      } else if (event.key === 'ArrowDown') {
        event.preventDefault();
        setActiveIndex((current) => Math.min(current + 1, Math.max(visibleActions.length - 1, 0)));
      } else if (event.key === 'ArrowUp') {
        event.preventDefault();
        setActiveIndex((current) => Math.max(current - 1, 0));
      } else if (event.key === 'Enter') {
        event.preventDefault();
        runAction(visibleActions[activeIndex]);
      }
    };
    // Capture it before board/card hotkeys get a chance to stop propagation.
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [activeIndex, close, isOpen, open, runAction, visibleActions]);

  useEffect(() => {
    if (!isOpen) return;
    requestAnimationFrame(() => inputRef.current?.focus());
  }, [isOpen]);

  return <>
    {showTrigger && <button type="button" className="command-palette-trigger" onClick={open} aria-haspopup="dialog" aria-expanded={isOpen}>
      <span>⌘</span>{triggerLabel}<kbd>Ctrl K</kbd>
    </button>}
    {isOpen && <div className="command-palette-backdrop" role="presentation" onMouseDown={close}>
      <section className="command-palette" role="dialog" aria-modal="true" aria-label="Команды" onMouseDown={(event) => event.stopPropagation()}>
        <label className="command-palette-search" htmlFor={listId}>
          <span aria-hidden="true">⌕</span>
          <input
            ref={inputRef}
            id={listId}
            value={query}
            onChange={(event) => { setQuery(event.target.value); setActiveIndex(0); }}
            placeholder={placeholder}
            role="combobox"
            aria-expanded="true"
            aria-controls={`${listId}-options`}
            aria-activedescendant={visibleActions[activeIndex] ? `${listId}-option-${visibleActions[activeIndex].id}` : undefined}
            autoComplete="off"
          />
          <kbd>Esc</kbd>
        </label>
        <div className="command-palette-results" id={`${listId}-options`} role="listbox" aria-label="Доступные команды">
          {groupedActions.map(([group, actions]) => <section className="command-palette-group" key={group} aria-label={group}>
            <h3>{group}</h3>
            {actions.map((action) => {
              const index = visibleActions.indexOf(action);
              const selected = index === activeIndex;
              return <button
                key={action.id}
                id={`${listId}-option-${action.id}`}
                type="button"
                role="option"
                aria-selected={selected}
                className={selected ? 'is-active' : ''}
                disabled={action.disabled}
                onMouseEnter={() => setActiveIndex(index)}
                onClick={() => runAction(action)}
              >
                <span><b>{action.label}</b><small>{action.description}</small></span>
                {action.shortcut && <kbd>{action.shortcut}</kbd>}
              </button>;
            })}
          </section>)}
          {!visibleActions.length && <p className="command-palette-empty">Ничего не найдено. Попробуйте другой запрос.</p>}
        </div>
        <footer><span><kbd>↑</kbd><kbd>↓</kbd> выбрать</span><span><kbd>Enter</kbd> открыть</span><span><kbd>Esc</kbd> закрыть</span></footer>
      </section>
    </div>}
  </>;
}
