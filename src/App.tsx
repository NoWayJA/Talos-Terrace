import { useEffect, useState } from 'react';
import { Sidebar } from '@/components/Sidebar';
import type { Page } from '@/pages';
import { CanvasPage } from '@/pages/Canvas';
import { useLiveFeed } from '@/pages/Canvas/hooks/useLiveFeed';
import { SettingsPage } from '@/pages/Settings';
import { api, type AppInfo } from '@/services/api';
import { describeErrorForLog } from '@/services/errors';
import { selectTheme, useSettingsStore } from '@/stores/useSettingsStore';

export function App() {
  const [page, setPage] = useState<Page>('canvas');
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const theme = useSettingsStore(selectTheme);
  const loadSettings = useSettingsStore((state) => state.load);
  useLiveFeed();

  useEffect(() => {
    let mounted = true;
    api.app
      .info()
      .then((info) => {
        if (mounted) setAppInfo(info);
      })
      .catch((error: unknown) => {
        console.error(describeErrorForLog(error));
      });
    void loadSettings();
    return () => {
      mounted = false;
    };
  }, [loadSettings]);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  return (
    <div className="app-layout">
      <Sidebar
        current={page}
        onNavigate={setPage}
        collapsed={collapsed}
        onToggle={() => {
          setCollapsed((value) => !value);
        }}
        version={appInfo?.version ?? ''}
      />
      <main className="app-main">
        {page === 'canvas' ? (
          <CanvasPage
            onOpenSettings={() => {
              setPage('settings');
            }}
          />
        ) : (
          <SettingsPage />
        )}
      </main>
    </div>
  );
}
