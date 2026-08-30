import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'Flowboard — работа в потоке',
  description: 'Пространство команды для задач, проектов и решений.',
  manifest: '/manifest.webmanifest',
  themeColor: '#0d1117',
  appleWebApp: { capable: true, title: 'Flowboard', statusBarStyle: 'black-translucent' },
  icons: { icon: [{ url: '/flowboard-coin.png', type: 'image/png', sizes: '96x96' }], apple: [{ url: '/flowboard-coin.png', type: 'image/png', sizes: '96x96' }] },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="ru"><body>{children}</body></html>;
}
