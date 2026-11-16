import React from 'react';
import { Languages, LucideIcon, MessagesSquare, Moon, Plug, Puzzle, Rocket, Sun, UserRound } from 'lucide-react';
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
  onOpenQuickLaunch?: () => void;
}

const itemClass = (active: boolean) =>
  `relative w-[58px] rounded-xl flex flex-col items-center justify-center gap-1 transition-all duration-150 ${
    active
      ? 'bg-gradient-to-b from-blue-500/[0.14] to-indigo-500/[0.08] dark:from-blue-500/[0.22] dark:to-indigo-500/[0.14] text-blue-600 dark:text-sky-300 ring-1 ring-inset ring-blue-500/20 dark:ring-sky-400/25 font-semibold shadow-[0_2px_6px_rgba(37,99,235,0.10)]'
      : 'text-slate-500 dark:text-slate-400 hover:bg-slate-200/60 dark:hover:bg-white/[0.06] hover:text-slate-900 dark:hover:text-white'
  }`;

/** The app's main navigation: one page per feature, plus language and theme. */
export const NavRail: React.FC<NavRailProps> = ({
  page,
  onPageChange,
  theme,
  onToggleTheme,
  onOpenQuickLaunch,
}) => {
  const { locale, toggleLocale, t } = useI18n();

  return (
    <nav
      aria-label={t.nav.label}
      className="w-[72px] flex-shrink-0 flex flex-col items-center gap-1.5 pt-3 pb-2.5 bg-white/82 dark:bg-[#0e1422]/88 backdrop-blur-xl border-r border-slate-200/80 dark:border-slate-800/85 select-none z-30 transition-colors"
    >
      <div className="w-10 h-10 rounded-[13px] bg-gradient-to-br from-white via-slate-50 to-blue-50/70 dark:from-[#1d2842] dark:via-[#151e32] dark:to-[#111829] border border-slate-200/90 dark:border-slate-700/80 shadow-[0_3px_10px_rgba(15,23,42,0.08),inset_0_1px_0_rgba(255,255,255,0.85)] dark:shadow-[0_4px_12px_rgba(0,0,0,0.4),inset_0_1px_0_rgba(255,255,255,0.08)] flex items-center justify-center mb-2 flex-shrink-0">
        <img
          src="/context-vault.png"
          alt="Context Vault"
          title="Context Vault"
          className="w-7 h-7 object-contain"
        />
      </div>

      {APP_PAGES.map((key) => {
        const Icon = ICONS[key];
        const active = page === key;
        return (
          <button
            key={key}
            type="button"
            aria-current={active ? 'page' : undefined}
            onClick={() => onPageChange(key)}
            className={`${itemClass(active)} h-[52px] text-[11.5px]`}
          >
            {active && (
              <span
                aria-hidden="true"
                className="absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-5 rounded-r-full bg-gradient-to-b from-blue-500 to-indigo-600 dark:from-sky-400 dark:to-blue-500 shadow-[0_0_8px_rgba(59,130,246,0.5)]"
              />
            )}
            <Icon size={19} strokeWidth={active ? 2.15 : 1.85} />
            <span className="leading-none">{t.nav[key]}</span>
          </button>
        );
      })}

      <div className="flex-1 min-h-[8px]" />

      {onOpenQuickLaunch && (
        <button
          type="button"
          onClick={onOpenQuickLaunch}
          title={t.nav.quickLaunchTooltip}
          aria-label={t.nav.quickLaunch}
          className="w-[56px] py-2 mb-1.5 rounded-2xl flex flex-col items-center justify-center gap-1 text-[11px] font-medium text-slate-600 dark:text-slate-300 hover:bg-blue-50/90 dark:hover:bg-blue-500/12 hover:text-blue-600 dark:hover:text-sky-300 transition-all duration-150 select-none active:scale-[0.97]"
        >
          <Rocket size={17} />
          <span className="truncate max-w-full px-1">{t.nav.quickLaunch}</span>
        </button>
      )}

      <div className="w-full px-1.5 pt-2.5 border-t border-slate-200/75 dark:border-slate-800/85 flex flex-col items-center gap-1.5 flex-shrink-0">
        <button
          type="button"
          onClick={toggleLocale}
          title={locale === 'zh' ? t.header.switchToEnglish : t.header.switchToChinese}
          className={`${itemClass(false)} h-[40px] w-[56px] text-[10px]`}
        >
          <Languages size={15} strokeWidth={1.9} />
          <span className="leading-none">{locale === 'zh' ? 'EN' : '中文'}</span>
        </button>
        <button
          type="button"
          onClick={onToggleTheme}
          title={theme === 'dark' ? t.header.switchToLight : t.header.switchToDark}
          className="w-[56px] h-[42px] rounded-xl flex flex-col items-center justify-center gap-1 text-[10px] font-medium bg-white/95 dark:bg-[#192338] border border-slate-200/90 dark:border-slate-700/75 text-slate-600 dark:text-slate-300 hover:border-blue-300 dark:hover:border-blue-400/40 hover:text-blue-600 dark:hover:text-sky-300 shadow-[0_1px_3px_rgba(15,23,42,0.05)] transition-all active:scale-[0.97]"
        >
          {theme === 'dark' ? (
            <Sun size={15} strokeWidth={2} className="text-amber-400" />
          ) : (
            <Moon size={15} strokeWidth={2} className="text-indigo-500" />
          )}
          <span className="leading-none">{theme === 'dark' ? t.header.lightMode : t.header.darkMode}</span>
        </button>
      </div>
    </nav>
  );
};
