'use client';

import { useEffect, useId, useState } from 'react';
import './theme-picker.css';

export const FLOWBOARD_THEMES = [
  'current',
  'dark',
  'light',
  'trello-dark',
  'oled',
  'pastel',
  'high-contrast',
] as const;

export type FlowboardTheme = (typeof FLOWBOARD_THEMES)[number];

export type ThemePickerProps = {
  className?: string;
  onThemeChange?: (theme: FlowboardTheme) => void;
};

type ThemeOption = {
  value: FlowboardTheme;
  label: string;
  description: string;
};

const STORAGE_KEY = 'flowboard-theme';
const SYSTEM_THEME_QUERY = '(prefers-color-scheme: dark)';

const THEME_OPTIONS: readonly ThemeOption[] = [
  { value: 'current', label: 'Как в системе', description: 'Автоматически светлая или тёмная' },
  { value: 'dark', label: 'Тёмная', description: 'Стандартная тёмная тема Flowboard' },
  { value: 'light', label: 'Светлая', description: 'Контрастная светлая рабочая тема' },
  { value: 'trello-dark', label: 'Trello dark', description: 'Холодная сине-серая палитра' },
  { value: 'oled', label: 'OLED black', description: 'Почти чёрный фон для OLED-экранов' },
  { value: 'pastel', label: 'Pastel', description: 'Мягкая лавандово-розовая палитра' },
  { value: 'high-contrast', label: 'High contrast', description: 'Максимальная читаемость интерфейса' },
];

function isFlowboardTheme(value: string | null): value is FlowboardTheme {
  return value !== null && (FLOWBOARD_THEMES as readonly string[]).includes(value);
}

function getStoredTheme(): FlowboardTheme {
  try {
    const value = window.localStorage.getItem(STORAGE_KEY);
    return isFlowboardTheme(value) ? value : 'current';
  } catch {
    return 'current';
  }
}

function applyTheme(theme: FlowboardTheme, isSystemDark: boolean) {
  const root = document.documentElement;
  root.dataset.flowboardTheme = theme;
  root.dataset.flowboardResolvedTheme = theme === 'current' ? (isSystemDark ? 'dark' : 'light') : theme;
  root.style.colorScheme = theme === 'current' ? (isSystemDark ? 'dark' : 'light') : theme === 'light' || theme === 'pastel' ? 'light' : 'dark';
}

export function ThemePicker({ className, onThemeChange }: ThemePickerProps) {
  const selectId = useId();
  const [theme, setTheme] = useState<FlowboardTheme>('current');
  const [systemIsDark, setSystemIsDark] = useState(true);

  useEffect(() => {
    const media = window.matchMedia(SYSTEM_THEME_QUERY);
    const savedTheme = getStoredTheme();
    const updateSystemTheme = (event: MediaQueryListEvent | MediaQueryList) => setSystemIsDark(event.matches);

    setTheme(savedTheme);
    updateSystemTheme(media);
    media.addEventListener('change', updateSystemTheme);
    return () => media.removeEventListener('change', updateSystemTheme);
  }, []);

  useEffect(() => {
    applyTheme(theme, systemIsDark);
  }, [systemIsDark, theme]);

  const updateTheme = (nextTheme: FlowboardTheme) => {
    setTheme(nextTheme);
    try {
      window.localStorage.setItem(STORAGE_KEY, nextTheme);
    } catch {
      // A blocked localStorage must not prevent an in-session theme switch.
    }
    onThemeChange?.(nextTheme);
  };

  return <div className={`theme-picker${className ? ` ${className}` : ''}`}>
    <label htmlFor={selectId}>Тема интерфейса</label>
    <select id={selectId} value={theme} onChange={(event) => updateTheme(event.target.value as FlowboardTheme)}>
      {THEME_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
    </select>
    <p aria-live="polite">{THEME_OPTIONS.find((option) => option.value === theme)?.description}</p>
  </div>;
}

export default ThemePicker;
