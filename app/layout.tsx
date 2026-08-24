import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = { title: 'Flowboard — работа в потоке', description: 'Пространство команды для задач, проектов и решений.' };

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="ru"><body>{children}</body></html>;
}
