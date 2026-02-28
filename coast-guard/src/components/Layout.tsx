import { useCallback } from 'react';
import { Outlet, Link } from 'react-router';
import { Sun, Moon, BookOpenText } from '@phosphor-icons/react';
import { useTranslation } from 'react-i18next';
import { useTheme } from '../providers/ThemeProvider';
import { useCoastEvents } from '../hooks/useWebSocket';
import { useDockerInfo, useOpenDockerSettingsMutation } from '../hooks/useDockerInfo';
import { formatBytes } from '../lib/formatBytes';
import LanguagePicker from './LanguagePicker';
import DockerIcon from './DockerIcon';
import logoUrl from '../../assets/coastguard_logo_with_name.svg';

export default function Layout() {
  useCoastEvents();
  const { theme, toggle } = useTheme();
  const { t } = useTranslation();
  const { data: dockerInfo } = useDockerInfo();
  const openSettings = useOpenDockerSettingsMutation();

  const handleOpenDockerSettings = useCallback(() => {
    openSettings.mutate();
  }, [openSettings]);

  return (
    <div className="min-h-screen flex flex-col text-main">
      <header className="app-header sticky top-0 z-50 h-14">
        <div className="page-shell !py-0 h-full flex items-center justify-between">
          <Link
            to="/"
            className="flex items-center gap-2 text-lg font-bold text-main no-underline hover:no-underline"
          >
            <img src={logoUrl} alt={t('app.title')} className="h-8" />
          </Link>
          <div className="flex items-center gap-1">
            {dockerInfo != null && (
              <button
                onClick={handleOpenDockerSettings}
                className="h-8 px-2.5 inline-flex items-center gap-2.5 rounded-lg text-xs text-subtle-ui hover:bg-white/30 dark:hover:bg-white/10 transition-colors cursor-pointer"
                title={t('docker.memoryTitle')}
              >
                <DockerIcon size={18} />
                <span className="font-medium">
                  {t('docker.label')}: {formatBytes(dockerInfo.mem_total_bytes)}
                </span>
              </button>
            )}
            <LanguagePicker />
            <button
              onClick={toggle}
              className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-subtle-ui hover:bg-white/30 dark:hover:bg-white/10 transition-colors"
              title={theme === 'dark' ? t('theme.switchToLight') : t('theme.switchToDark')}
            >
              {theme === 'dark' ? <Sun size={18} /> : <Moon size={18} />}
            </button>
            <Link
              to="/docs"
              className="h-8 w-8 inline-flex items-center justify-center rounded-lg text-subtle-ui hover:bg-white/30 dark:hover:bg-white/10 transition-colors"
              title={t('docs.title')}
            >
              <BookOpenText size={18} />
            </Link>
          </div>
        </div>
      </header>
      <main className="flex-1">
        <Outlet />
      </main>
    </div>
  );
}
