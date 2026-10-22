import React from 'react';
import {
  Bot,
  Database,
  DatabaseZap,
  Moon,
  Sparkles,
  Sun,
  UploadCloud,
  Languages,
  Puzzle,
  Terminal,
} from 'lucide-react';
import { Button } from './common/Button';
import { useI18n } from '../i18n';

export type SourceCategory = 'codex' | 'antigravity' | 'claude';

interface HeaderProps {
  onClearDatabase: () => void;
  isClearingDatabase: boolean;
  activeSource: SourceCategory;
  onSourceChange: (source: SourceCategory) => void;
  codexCount: number;
  /** Conversations Codex lists; absent when Codex keeps no project state. */
  codexVisibleCount?: number;
  antigravityCount: number;
  claudeCount: number;
  onBackup: () => void;
  onOpenImport: () => void;
  onOpenSkills: () => void;
  theme: 'dark' | 'light';
  onToggleTheme: () => void;
  onOpenClient?: (source: SourceCategory) => void;
  isOpeningClient?: boolean;
}

export const Header: React.FC<HeaderProps> = ({
  onClearDatabase,
  isClearingDatabase,
  activeSource,
  onSourceChange,
  codexCount,
  codexVisibleCount,
  antigravityCount,
  claudeCount,
  onBackup,
  onOpenImport,
  onOpenSkills,
  theme,
  onToggleTheme,
}) => {
  const { locale, toggleLocale, t, interpolate } = useI18n();

  return (
    <header className="h-14 backdrop-blur-2xl bg-white/80 dark:bg-black/80 border-b border-black/[0.06] dark:border-white/[0.08] px-5 flex items-center justify-between flex-shrink-0 z-30 transition-colors duration-300 select-none">
      {/* Left: Apple Style Brand & Source Switcher */}
      <div className="flex items-center gap-4">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-xl overflow-hidden shadow-md shadow-blue-500/25 border border-black/[0.08] dark:border-white/[0.15] p-0.5 bg-black/[0.03] dark:bg-white/[0.05] transition-transform duration-300 hover:scale-105 flex-shrink-0">
            <img
              src="/context-vault.png"
              alt="Context Vault Logo"
              className="w-full h-full object-cover rounded-[7px]"
            />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h1 className="text-sm font-semibold tracking-tight bg-gradient-to-r from-[#1d1d1f] via-[#2d2d30] to-[#515154] dark:from-white dark:via-[#f5f5f7] dark:to-[#a1a1a6] bg-clip-text text-transparent">
                Context Vault
              </h1>
              <span className="text-[10px] text-[#86868b] font-medium bg-black/[0.04] dark:bg-white/[0.06] px-1.5 py-0.5 rounded-full border border-black/[0.06] dark:border-white/[0.08] tracking-wider">
                PRO
              </span>
            </div>
            <p className="text-[11px] text-[#86868b] font-normal leading-none mt-0.5">{t.header.subtitle}</p>
          </div>
        </div>

        <div className="h-4 w-px bg-black/[0.08] dark:bg-white/[0.1] mx-1" />

        {/* Apple Style Segmented Switcher: Codex vs Antigravity vs Claude */}
        <div className="flex items-center bg-black/[0.04] dark:bg-white/[0.06] p-1 rounded-full border border-black/[0.08] dark:border-white/[0.1] shadow-inner gap-1">
          <button
            onClick={() => onSourceChange('codex')}
            className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs transition-all duration-200 ${
              activeSource === 'codex'
                ? 'bg-white dark:bg-[#1c1c1e] text-[#0071e3] dark:text-[#2997ff] shadow-sm font-semibold border border-black/[0.04] dark:border-white/[0.08]'
                : 'text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white font-medium'
            }`}
            title={
              codexVisibleCount === undefined
                ? 'Codex'
                : interpolate(t.header.codexCountTooltip, { visible: codexVisibleCount, total: codexCount })
            }
          >
            <Terminal
              size={12}
              className={activeSource === 'codex' ? 'text-[#0071e3] dark:text-[#2997ff]' : 'text-[#86868b]'}
            />
            <span>Codex</span>
            <span
              className={`text-[10px] px-1.5 py-0.2 rounded-full font-mono ${
                activeSource === 'codex'
                  ? 'bg-[#0071e3]/10 text-[#0071e3] dark:text-[#2997ff]'
                  : 'bg-black/[0.05] dark:bg-white/[0.08] text-[#86868b]'
              }`}
            >
              {codexVisibleCount === undefined ? codexCount : `${codexVisibleCount}/${codexCount}`}
            </span>
          </button>

          <button
            onClick={() => onSourceChange('antigravity')}
            className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs transition-all duration-200 ${
              activeSource === 'antigravity'
                ? 'bg-white dark:bg-[#1c1c1e] text-[#af52de] dark:text-[#bf5af2] shadow-sm font-semibold border border-black/[0.04] dark:border-white/[0.08]'
                : 'text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white font-medium'
            }`}
            title="Antigravity"
          >
            <Sparkles
              size={12}
              className={activeSource === 'antigravity' ? 'text-[#af52de] dark:text-[#bf5af2]' : 'text-[#86868b]'}
            />
            <span>Antigravity</span>
            <span
              className={`text-[10px] px-1.5 py-0.2 rounded-full font-mono ${
                activeSource === 'antigravity'
                  ? 'bg-[#af52de]/10 text-[#af52de] dark:text-[#bf5af2]'
                  : 'bg-black/[0.05] dark:bg-white/[0.08] text-[#86868b]'
              }`}
            >
              {antigravityCount}
            </span>
          </button>

          <button
            onClick={() => onSourceChange('claude')}
            className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs transition-all duration-200 ${
              activeSource === 'claude'
                ? 'bg-white dark:bg-[#1c1c1e] text-[#d97706] dark:text-[#f59e0b] shadow-sm font-semibold border border-black/[0.04] dark:border-white/[0.08]'
                : 'text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white font-medium'
            }`}
            title="Claude"
          >
            <Bot
              size={12}
              className={activeSource === 'claude' ? 'text-[#d97706] dark:text-[#f59e0b]' : 'text-[#86868b]'}
            />
            <span>Claude</span>
            <span
              className={`text-[10px] px-1.5 py-0.2 rounded-full font-mono ${
                activeSource === 'claude'
                  ? 'bg-[#d97706]/10 text-[#d97706] dark:text-[#f59e0b]'
                  : 'bg-black/[0.05] dark:bg-white/[0.08] text-[#86868b]'
              }`}
            >
              {claudeCount}
            </span>
          </button>
        </div>
      </div>

      {/* Right: Apple Pill Buttons, Language Toggle, Theme Toggle & View Switcher */}
      <div className="flex items-center gap-2">
        {/* Language Switcher Button (ZH / EN) */}
        <Button
          variant="secondary"
          size="sm"
          onClick={toggleLocale}
          title={locale === 'zh' ? t.header.switchToEnglish : t.header.switchToChinese}
          className="px-2.5"
        >
          <div className="flex items-center gap-1.5">
            <Languages size={13} className="text-[#0071e3] dark:text-[#2997ff]" />
            <span className="text-[11px] font-medium tracking-wide">
              {locale === 'zh' ? 'EN' : '中文'}
            </span>
          </div>
        </Button>

        {/* Theme Toggle Button (Light / Dark) */}
        <Button
          variant="secondary"
          size="sm"
          onClick={onToggleTheme}
          title={theme === 'dark' ? t.header.switchToLight : t.header.switchToDark}
          className="px-2.5"
        >
          {theme === 'dark' ? (
            <div className="flex items-center gap-1.5">
              <Sun size={13} className="text-amber-400" />
              <span className="text-[11px]">{t.header.lightMode}</span>
            </div>
          ) : (
            <div className="flex items-center gap-1.5">
              <Moon size={13} className="text-[#0071e3]" />
              <span className="text-[11px]">{t.header.darkMode}</span>
            </div>
          )}
        </Button>

        <Button
          variant="ghost"
          size="sm"
          icon={<Puzzle size={13} className="text-[#86868b]" />}
          onClick={onOpenSkills}
          title={t.skills.buttonTooltip}
        >
          {t.skills.button}
        </Button>

        <Button
          variant="ghost"
          size="sm"
          icon={<UploadCloud size={13} className="text-[#86868b]" />}
          onClick={onOpenImport}
          title={t.header.importTooltip}
        >
          {t.header.import}
        </Button>

        <Button
          variant="ghost"
          size="sm"
          icon={<Database size={13} className="text-[#86868b]" />}
          onClick={onBackup}
          title={t.header.backupTooltip}
        >
          {t.header.backup}
        </Button>

        <div className="h-4 w-px bg-black/[0.08] dark:bg-white/[0.1] mx-1" />

        <Button
          variant="danger"
          size="sm"
          icon={<DatabaseZap size={13} />}
          onClick={onClearDatabase}
          disabled={isClearingDatabase}
          title={t.header.clearDatabaseTooltip}
        >
          {t.header.clearDatabase}
        </Button>
      </div>
    </header>
  );
};
