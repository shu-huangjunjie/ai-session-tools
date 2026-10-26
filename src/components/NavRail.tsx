import React from 'react';
import { Languages, LucideIcon, MessagesSquare, Moon, Plug, Puzzle, Sun, UserRound } from 'lucide-react';
import { useI18n } from '../i18n';

export type AppPage = 'sessions' | 'skills' | 'mcp' | 'accounts';
export const APP_PAGES: AppPage[] = ['sessions', 'skills', 'mcp', 'accounts'];

const ICONS: Record<AppPage, LucideIcon> = {
  sessions: MessagesSquare,
  skills: Puzzle,
  mcp: Plug,
  accounts: UserRound,
};

interface NavRailProps {
  page: AppPage;
  onPageChange: (page: AppPage) => void;
  theme: 'dark' | 'light';
  onToggleTheme: () => void;
}

const itemClass = (active: boolean) =>
  `w-[62px] rounded-xl flex flex-col items-center justify-center gap-1 transition-colors duration-150 ${
    active
      ? 'bg-[#0071e3]/[0.09] dark:bg-[#2997ff]/15 text-[#0071e3] dark:text-[#64b5ff] font-semibold'
      : 'text-[#6e6e73] dark:text-[#a1a1a6] hover:bg-black/[0.04] dark:hover:bg-white/[0.06] hover:text-[#1d1d1f] dark:hover:text-white'
  }`;

/** The app's main navigation: one page per feature, plus language and theme. */
export const NavRail: React.FC<NavRailProps> = ({ page, onPageChange, theme, onToggleTheme }) => {
  const { locale, toggleLocale, t } = useI18n();

  return (
    <nav
      aria-label={t.nav.label}
      className="w-[76px] flex-shrink-0 flex flex-col items-center gap-1 pt-3.5 pb-3 bg-white dark:bg-[#111113] border-r border-black/[0.06] dark:border-white/[0.08] select-none z-30 transition-colors"
    >
      <img
        src="/context-vault.png"
        alt="Context Vault"
        title="Context Vault"
        className="w-10 h-10 object-contain mb-2.5"
      />
      {APP_PAGES.map((key) => {
        const Icon = ICONS[key];
        const active = page === key;
        return (
          <button
            key={key}
            type="button"
            aria-current={active ? 'page' : undefined}
            onClick={() => onPageChange(key)}
            className={`${itemClass(active)} h-[58px] text-xs`}
          >
            <Icon size={20} strokeWidth={1.9} />
            {t.nav[key]}
          </button>
        );
      })}
      <div className="flex-1" />
      <button
        type="button"
        onClick={toggleLocale}
        title={locale === 'zh' ? t.header.switchToEnglish : t.header.switchToChinese}
        className={`${itemClass(false)} h-[46px] text-[10px]`}
      >
        <Languages size={17} strokeWidth={1.9} />
        {locale === 'zh' ? 'EN' : '中文'}
      </button>
      <button
        type="button"
        onClick={onToggleTheme}
        title={theme === 'dark' ? t.header.switchToLight : t.header.switchToDark}
        className={`${itemClass(false)} h-[46px] text-[10px]`}
      >
        {theme === 'dark' ? <Sun size={17} strokeWidth={1.9} /> : <Moon size={17} strokeWidth={1.9} />}
        {theme === 'dark' ? t.header.lightMode : t.header.darkMode}
      </button>
    </nav>
  );
};
