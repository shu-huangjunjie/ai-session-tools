import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Bot,
  Braces,
  Code2,
  Cpu,
  FolderOpen,
  FolderSearch,
  GitBranch,
  Layers,
  Loader2,
  LucideIcon,
  Play,
  RefreshCw,
  Rocket,
  RotateCcw,
  Search,
  SlidersHorizontal,
  Sparkles,
  Terminal,
  TerminalSquare,
  X,
} from 'lucide-react';

import { api, errorMessage } from '../api/client';
import { useI18n } from '../i18n';
import { QuickLaunchCategory, QuickLaunchTool } from '../types';
import { Button } from './common/Button';
import { Modal } from './common/Modal';

export type QuickLaunchToastFn = (
  type: 'success' | 'warning' | 'error' | 'info',
  message: string,
) => void;

const QUICK_LAUNCH_DIR_KEY = 'cv_quick_launch_dir';
const QUICK_LAUNCH_TOOLS_EVENT = 'cv-quick-launch-updated';
const QUICK_LAUNCH_DIR_EVENT = 'cv-quick-launch-dir-changed';

const CATEGORY_ORDER: QuickLaunchCategory[] = ['ai_app', 'ai_cli', 'editor', 'terminal'];

interface ToolVisualTheme {
  gradient: string;
  icon: LucideIcon;
}

const TOOL_THEMES: Record<string, ToolVisualTheme> = {
  codex_app: { gradient: 'from-blue-500 to-cyan-500', icon: Sparkles },
  claude_app: { gradient: 'from-amber-500 to-orange-500', icon: Bot },
  antigravity_app: { gradient: 'from-violet-500 to-indigo-500', icon: Rocket },
  codex_cli: { gradient: 'from-sky-500 to-teal-500', icon: TerminalSquare },
  claude_cli: { gradient: 'from-orange-500 to-amber-600', icon: TerminalSquare },
  antigravity_cli: { gradient: 'from-indigo-500 to-purple-600', icon: Cpu },
  vscode: { gradient: 'from-sky-500 to-blue-600', icon: Code2 },
  cursor: { gradient: 'from-slate-700 to-slate-900', icon: Braces },
  windsurf: { gradient: 'from-teal-500 to-emerald-600', icon: Layers },
  trae: { gradient: 'from-rose-500 to-pink-600', icon: Code2 },
  windows_terminal: { gradient: 'from-cyan-600 to-blue-700', icon: Terminal },
  powershell: { gradient: 'from-blue-600 to-indigo-700', icon: Terminal },
  git_bash: { gradient: 'from-orange-500 to-rose-600', icon: GitBranch },
};

const DEFAULT_THEME: ToolVisualTheme = {
  gradient: 'from-slate-500 to-slate-700',
  icon: Terminal,
};

function readSavedFolder(): string {
  try {
    return localStorage.getItem(QUICK_LAUNCH_DIR_KEY) ?? '';
  } catch {
    return '';
  }
}

function persistFolder(value: string): void {
  try {
    const trimmed = value.trim();
    if (trimmed) {
      localStorage.setItem(QUICK_LAUNCH_DIR_KEY, trimmed);
    } else {
      localStorage.removeItem(QUICK_LAUNCH_DIR_KEY);
    }
    window.dispatchEvent(new Event(QUICK_LAUNCH_DIR_EVENT));
  } catch {
    // Ignore storage quota or sandbox errors in restricted test contexts.
  }
}

function folderLeaf(path: string): string {
  const normalized = path.replace(/[\\/]+$/, '');
  const parts = normalized.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || normalized;
}

function effectiveToolPath(tool: QuickLaunchTool): string | null {
  return tool.custom_path || tool.detected_path;
}

function useQuickLaunchState(onToast: QuickLaunchToastFn, active: boolean) {
  const { t, interpolate } = useI18n();
  const [tools, setTools] = useState<QuickLaunchTool[]>([]);
  const [loading, setLoading] = useState(false);
  const [launchingId, setLaunchingId] = useState<string | null>(null);
  const [savingToolId, setSavingToolId] = useState<string | null>(null);
  const [folderPath, setFolderPathState] = useState<string>(() => readSavedFolder());
  const refreshTimerRef = useRef<number | null>(null);

  const setFolderPath = useCallback((next: string) => {
    setFolderPathState(next);
    persistFolder(next);
  }, []);

  const refreshTools = useCallback(async () => {
    if (typeof api.listQuickLaunchTools !== 'function') return;
    setLoading(true);
    try {
      const next = await api.listQuickLaunchTools();
      setTools(next);
    } catch (err) {
      onToast('error', errorMessage(err));
    } finally {
      setLoading(false);
    }
  }, [onToast]);

  useEffect(() => {
    if (!active || typeof api.listQuickLaunchTools !== 'function') return;
    void refreshTools();
  }, [active, refreshTools]);

  useEffect(() => {
    const onToolsUpdated = (ev: Event) => {
      const detail = (ev as CustomEvent<QuickLaunchTool[] | undefined>).detail;
      if (Array.isArray(detail)) {
        setTools(detail);
      } else if (active) {
        void refreshTools();
      }
    };
    const onDirChanged = () => {
      setFolderPathState(readSavedFolder());
    };
    window.addEventListener(QUICK_LAUNCH_TOOLS_EVENT, onToolsUpdated);
    window.addEventListener(QUICK_LAUNCH_DIR_EVENT, onDirChanged);
    return () => {
      window.removeEventListener(QUICK_LAUNCH_TOOLS_EVENT, onToolsUpdated);
      window.removeEventListener(QUICK_LAUNCH_DIR_EVENT, onDirChanged);
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
      }
    };
  }, [active, refreshTools]);

  const handlePickFolder = useCallback(async () => {
    if (typeof api.pickFolderPath !== 'function') return;
    try {
      const picked = await api.pickFolderPath(folderPath.trim() || undefined);
      if (picked) {
        setFolderPath(picked);
      }
    } catch (err) {
      onToast('error', errorMessage(err));
    }
  }, [folderPath, onToast, setFolderPath]);

  const handleLaunchTool = useCallback(
    async (tool: QuickLaunchTool) => {
      if (typeof api.launchQuickTool !== 'function') return;
      const targetFolder = tool.supports_folder && folderPath.trim() ? folderPath.trim() : null;
      setLaunchingId(tool.id);
      try {
        await api.launchQuickTool(tool.id, targetFolder);
        onToast('success', interpolate(t.quickLaunch.launchSuccess, { name: tool.name }));
        if (refreshTimerRef.current !== null) {
          window.clearTimeout(refreshTimerRef.current);
        }
        refreshTimerRef.current = window.setTimeout(() => {
          void refreshTools();
        }, 900);
      } catch (err) {
        onToast(
          'error',
          interpolate(t.quickLaunch.launchFailed, { name: tool.name, error: errorMessage(err) }),
        );
      } finally {
        setLaunchingId(null);
      }
    },
    [folderPath, interpolate, onToast, refreshTools, t.quickLaunch.launchFailed, t.quickLaunch.launchSuccess],
  );

  const handleBrowseCustomPath = useCallback(
    async (tool: QuickLaunchTool) => {
      if (
        typeof api.pickExecutablePath !== 'function' ||
        typeof api.setQuickLaunchCustomPath !== 'function'
      ) {
        return;
      }
      const current = effectiveToolPath(tool) ?? undefined;
      try {
        const picked = await api.pickExecutablePath(current);
        if (!picked) return;
        setSavingToolId(tool.id);
        const updated = await api.setQuickLaunchCustomPath(tool.id, picked);
        setTools(updated);
        window.dispatchEvent(new CustomEvent(QUICK_LAUNCH_TOOLS_EVENT, { detail: updated }));
        onToast('success', interpolate(t.quickLaunch.pathUpdated, { name: tool.name }));
      } catch (err) {
        onToast(
          'error',
          interpolate(t.quickLaunch.pathUpdateFailed, { error: errorMessage(err) }),
        );
      } finally {
        setSavingToolId(null);
      }
    },
    [interpolate, onToast, t.quickLaunch.pathUpdateFailed, t.quickLaunch.pathUpdated],
  );

  const handleResetCustomPath = useCallback(
    async (tool: QuickLaunchTool) => {
      if (typeof api.setQuickLaunchCustomPath !== 'function') return;
      setSavingToolId(tool.id);
      try {
        const updated = await api.setQuickLaunchCustomPath(tool.id, null);
        setTools(updated);
        window.dispatchEvent(new CustomEvent(QUICK_LAUNCH_TOOLS_EVENT, { detail: updated }));
        onToast('info', interpolate(t.quickLaunch.pathReset, { name: tool.name }));
      } catch (err) {
        onToast(
          'error',
          interpolate(t.quickLaunch.pathUpdateFailed, { error: errorMessage(err) }),
        );
      } finally {
        setSavingToolId(null);
      }
    },
    [interpolate, onToast, t.quickLaunch.pathReset, t.quickLaunch.pathUpdateFailed],
  );

  return {
    tools,
    loading,
    launchingId,
    savingToolId,
    folderPath,
    setFolderPath,
    refreshTools,
    handlePickFolder,
    handleLaunchTool,
    handleBrowseCustomPath,
    handleResetCustomPath,
  };
}

interface QuickLaunchBarProps {
  onToast: QuickLaunchToastFn;
  contextFolder?: string | null;
}

export const QuickLaunchBar: React.FC<QuickLaunchBarProps> = ({ onToast, contextFolder }) => {
  const { t, interpolate } = useI18n();
  const [modalOpen, setModalOpen] = useState(false);
  const apiReady = typeof api.listQuickLaunchTools === 'function';

  const {
    tools,
    loading,
    launchingId,
    folderPath,
    setFolderPath,
    refreshTools,
    handlePickFolder,
    handleLaunchTool,
  } = useQuickLaunchState(onToast, apiReady);

  if (!apiReady) return null;

  const readyTools = tools.filter((tool) => tool.available);
  const detectedSummary = interpolate(t.quickLaunch.detectedCount, {
    ready: String(readyTools.length),
    total: String(tools.length),
  });

  return (
    <>
      <section
        aria-label={t.quickLaunch.barTitle}
        className="rounded-2xl border border-slate-200/85 dark:border-slate-800/90 bg-white/88 dark:bg-[#11192a]/85 backdrop-blur-md px-3.5 py-3 shadow-[0_6px_22px_rgba(15,23,42,0.04)] dark:shadow-[0_10px_28px_rgba(0,0,0,0.3)] flex flex-col gap-2.5"
      >
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex items-center gap-2 min-w-0">
            <div className="w-7 h-7 rounded-xl bg-gradient-to-br from-blue-500 to-indigo-600 text-white flex items-center justify-center shadow-[0_4px_12px_rgba(59,130,246,0.28)] flex-shrink-0">
              <Rocket size={14} />
            </div>
            <span className="text-xs font-semibold tracking-tight text-slate-900 dark:text-white">
              {t.quickLaunch.barTitle}
            </span>
            {tools.length > 0 && (
              <span className="inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-medium bg-slate-100/90 dark:bg-slate-800/80 text-slate-600 dark:text-slate-300 border border-slate-200/75 dark:border-slate-700/70">
                {detectedSummary}
              </span>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-1.5">
            <div className="inline-flex items-center gap-1 rounded-lg bg-slate-100/85 dark:bg-slate-900/70 border border-slate-200/80 dark:border-slate-700/70 pl-2 pr-1 h-[28px] max-w-[260px]">
              <button
                type="button"
                onClick={handlePickFolder}
                title={t.quickLaunch.pickFolder}
                className="inline-flex items-center gap-1.5 text-[11px] text-slate-600 dark:text-slate-300 hover:text-blue-600 dark:hover:text-sky-400 min-w-0 transition-colors"
              >
                <FolderOpen size={12} className="flex-shrink-0 text-blue-500" />
                <span className="truncate font-mono">
                  {folderPath ? folderLeaf(folderPath) : t.quickLaunch.pickFolder}
                </span>
              </button>
              {folderPath && (
                <button
                  type="button"
                  onClick={() => setFolderPath('')}
                  title={t.quickLaunch.clearFolder}
                  aria-label={t.quickLaunch.clearFolder}
                  className="p-0.5 rounded text-slate-400 hover:text-slate-700 dark:hover:text-slate-200 transition-colors"
                >
                  <X size={11} />
                </button>
              )}
            </div>

            <button
              type="button"
              onClick={() => void refreshTools()}
              disabled={loading}
              title={t.quickLaunch.refresh}
              aria-label={t.quickLaunch.refresh}
              className="h-[28px] w-[28px] inline-flex items-center justify-center rounded-lg border border-slate-200/85 dark:border-slate-700/75 bg-white/90 dark:bg-[#192338] text-slate-500 dark:text-slate-300 hover:text-slate-900 dark:hover:text-white hover:bg-slate-50 dark:hover:bg-[#212e47] transition-all disabled:opacity-50"
            >
              <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
            </button>

            <Button
              size="sm"
              variant="secondary"
              icon={<SlidersHorizontal size={12} />}
              onClick={() => setModalOpen(true)}
              title={t.quickLaunch.barManageTooltip}
              className="!h-[28px] !px-2.5 !text-[11px]"
            >
              {t.quickLaunch.barManage}
            </Button>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          {readyTools.length === 0 ? (
            <button
              type="button"
              onClick={() => setModalOpen(true)}
              className="text-[11px] text-slate-500 dark:text-slate-400 hover:text-blue-600 dark:hover:text-sky-400 px-2.5 py-1.5 rounded-xl border border-dashed border-slate-300/85 dark:border-slate-700/80 transition-colors"
            >
              {t.quickLaunch.pathMissingHint}
            </button>
          ) : (
            readyTools.map((tool) => {
              const theme = TOOL_THEMES[tool.id] ?? DEFAULT_THEME;
              const Icon = theme.icon;
              const isLaunching = launchingId === tool.id;
              return (
                <button
                  key={tool.id}
                  type="button"
                  onClick={() => void handleLaunchTool(tool)}
                  disabled={isLaunching}
                  title={effectiveToolPath(tool) ?? tool.name}
                  className={`group inline-flex items-center gap-2 px-2.5 py-1.5 rounded-xl border text-xs font-medium transition-all duration-150 active:scale-[0.98] disabled:opacity-60 ${
                    tool.running
                      ? 'bg-emerald-50/75 dark:bg-emerald-500/10 border-emerald-200/90 dark:border-emerald-500/30 text-slate-900 dark:text-white hover:border-emerald-300 dark:hover:border-emerald-400/50'
                      : 'bg-slate-50/85 dark:bg-[#172033]/90 border-slate-200/85 dark:border-slate-700/75 text-slate-800 dark:text-slate-100 hover:border-blue-300/90 dark:hover:border-sky-500/45 hover:bg-white dark:hover:bg-[#1d2940]'
                  }`}
                >
                  <span
                    className={`w-5 h-5 rounded-md bg-gradient-to-br ${theme.gradient} text-white flex items-center justify-center shadow-xs flex-shrink-0`}
                  >
                    {isLaunching ? (
                      <Loader2 size={11} className="animate-spin" />
                    ) : (
                      <Icon size={11} />
                    )}
                  </span>
                  <span className="truncate">{tool.name}</span>
                  {tool.running && (
                    <span
                      className="inline-flex items-center gap-1 text-[10px] font-medium text-emerald-700 dark:text-emerald-300"
                      title={t.quickLaunch.runningBadge}
                    >
                      <span className="cv-live-dot" aria-hidden="true" />
                      {t.quickLaunch.runningBadge}
                    </span>
                  )}
                </button>
              );
            })
          )}
        </div>
      </section>

      <QuickLaunchModal
        isOpen={modalOpen}
        onClose={() => setModalOpen(false)}
        contextFolder={contextFolder}
        onToast={onToast}
      />
    </>
  );
};

interface QuickLaunchModalProps {
  isOpen: boolean;
  onClose: () => void;
  contextFolder?: string | null;
  onToast: QuickLaunchToastFn;
}

export const QuickLaunchModal: React.FC<QuickLaunchModalProps> = ({
  isOpen,
  onClose,
  contextFolder,
  onToast,
}) => {
  const { t, interpolate } = useI18n();
  const [search, setSearch] = useState('');
  const [onlyAvailable, setOnlyAvailable] = useState(false);

  const {
    tools,
    loading,
    launchingId,
    savingToolId,
    folderPath,
    setFolderPath,
    refreshTools,
    handlePickFolder,
    handleLaunchTool,
    handleBrowseCustomPath,
    handleResetCustomPath,
  } = useQuickLaunchState(onToast, isOpen);

  const filteredTools = useMemo(() => {
    const q = search.trim().toLowerCase();
    return tools.filter((tool) => {
      if (onlyAvailable && !tool.available) return false;
      if (!q) return true;
      const path = effectiveToolPath(tool)?.toLowerCase() ?? '';
      return tool.name.toLowerCase().includes(q) || tool.id.toLowerCase().includes(q) || path.includes(q);
    });
  }, [onlyAvailable, search, tools]);

  const grouped = useMemo(() => {
    return CATEGORY_ORDER.map((category) => ({
      category,
      items: filteredTools.filter((tool) => tool.category === category),
    })).filter((group) => group.items.length > 0);
  }, [filteredTools]);

  const readyCount = useMemo(() => tools.filter((tool) => tool.available).length, [tools]);

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={t.quickLaunch.title}
      subtitle={t.quickLaunch.subtitle}
      widthClass="max-w-4xl"
    >
      <div className="flex flex-col gap-4">
        {/* Controls strip */}
        <div className="rounded-2xl border border-slate-200/85 dark:border-slate-800/85 bg-slate-50/80 dark:bg-[#0f1727]/80 p-3 flex flex-col gap-2.5">
          <div className="flex flex-wrap items-center gap-2">
            <div className="relative flex-1 min-w-[200px]">
              <Search
                size={13}
                className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
              />
              <input
                type="text"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                placeholder={t.quickLaunch.searchPlaceholder}
                aria-label={t.quickLaunch.searchPlaceholder}
                className="w-full h-8 pl-8 pr-7 rounded-xl bg-white dark:bg-[#141d2f] border border-slate-200/90 dark:border-slate-700/80 text-xs text-slate-900 dark:text-slate-100 placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15"
              />
              {search && (
                <button
                  type="button"
                  onClick={() => setSearch('')}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
                >
                  <X size={12} />
                </button>
              )}
            </div>

            <button
              type="button"
              onClick={() => setOnlyAvailable((prev) => !prev)}
              aria-pressed={onlyAvailable}
              className={`h-8 px-3 rounded-xl text-xs font-medium border transition-all ${
                onlyAvailable
                  ? 'bg-blue-50 dark:bg-blue-500/15 text-blue-700 dark:text-sky-300 border-blue-200 dark:border-blue-400/35'
                  : 'bg-white dark:bg-[#141d2f] text-slate-600 dark:text-slate-300 border-slate-200/90 dark:border-slate-700/80 hover:bg-slate-50 dark:hover:bg-[#1b2740]'
              }`}
            >
              {t.quickLaunch.onlyAvailable}
            </button>

            <span className="text-[11px] font-medium px-2.5 py-1 rounded-full bg-white/90 dark:bg-slate-800/80 border border-slate-200/80 dark:border-slate-700/80 text-slate-600 dark:text-slate-300">
              {interpolate(t.quickLaunch.detectedCount, {
                ready: String(readyCount),
                total: String(tools.length),
              })}
            </span>

            <Button
              size="sm"
              variant="secondary"
              icon={<RefreshCw size={12} className={loading ? 'animate-spin' : ''} />}
              onClick={() => void refreshTools()}
              disabled={loading}
            >
              {t.quickLaunch.refresh}
            </Button>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <div className="relative flex-1 min-w-[220px]">
              <FolderOpen
                size={13}
                className="absolute left-3 top-1/2 -translate-y-1/2 text-blue-500 pointer-events-none"
              />
              <input
                type="text"
                value={folderPath}
                onChange={(e) => setFolderPath(e.target.value)}
                placeholder={t.quickLaunch.folderPlaceholder}
                aria-label={t.quickLaunch.folderPlaceholder}
                className="w-full h-8 pl-8 pr-7 rounded-xl bg-white dark:bg-[#141d2f] border border-slate-200/90 dark:border-slate-700/80 text-xs font-mono text-slate-800 dark:text-slate-200 placeholder-slate-400 focus:outline-none focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15"
              />
              {folderPath && (
                <button
                  type="button"
                  onClick={() => setFolderPath('')}
                  title={t.quickLaunch.clearFolder}
                  aria-label={t.quickLaunch.clearFolder}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-700 dark:hover:text-slate-200"
                >
                  <X size={12} />
                </button>
              )}
            </div>

            <Button size="sm" variant="secondary" icon={<FolderOpen size={12} />} onClick={handlePickFolder}>
              {t.quickLaunch.pickFolder}
            </Button>

            {contextFolder && contextFolder !== folderPath && (
              <Button
                size="sm"
                variant="accent"
                onClick={() => setFolderPath(contextFolder)}
                title={contextFolder}
              >
                {t.quickLaunch.useSessionFolder}: {folderLeaf(contextFolder)}
              </Button>
            )}
          </div>
        </div>

        {/* Categorized tool grid */}
        {grouped.length === 0 ? (
          <div className="py-10 text-center text-xs text-slate-500 dark:text-slate-400 rounded-2xl border border-dashed border-slate-200 dark:border-slate-800">
            {t.quickLaunch.noMatches}
          </div>
        ) : (
          <div className="flex flex-col gap-5">
            {grouped.map(({ category, items }) => (
              <div key={category} className="flex flex-col gap-2.5">
                <div className="flex items-center justify-between px-1">
                  <h4 className="text-xs font-semibold tracking-wide uppercase text-slate-500 dark:text-slate-400">
                    {t.quickLaunch.categories[category]}
                  </h4>
                  <span className="text-[11px] text-slate-400 dark:text-slate-500">
                    {items.filter((i) => i.available).length}/{items.length}
                  </span>
                </div>

                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                  {items.map((tool) => {
                    const theme = TOOL_THEMES[tool.id] ?? DEFAULT_THEME;
                    const Icon = theme.icon;
                    const resolvedPath = effectiveToolPath(tool);
                    const isLaunching = launchingId === tool.id;
                    const isSaving = savingToolId === tool.id;

                    return (
                      <div
                        key={tool.id}
                        className={`rounded-2xl border p-3.5 transition-all flex flex-col gap-3 ${
                          tool.running
                            ? 'bg-emerald-50/45 dark:bg-emerald-500/8 border-emerald-200/85 dark:border-emerald-500/30'
                            : tool.available
                            ? 'bg-white/95 dark:bg-[#172135]/90 border-slate-200/85 dark:border-slate-700/75 hover:border-blue-300/80 dark:hover:border-sky-500/40'
                            : 'bg-slate-50/65 dark:bg-[#11192a]/60 border-slate-200/65 dark:border-slate-800/75 opacity-85'
                        }`}
                      >
                        <div className="flex items-start gap-3">
                          <div
                            className={`w-10 h-10 rounded-xl bg-gradient-to-br ${theme.gradient} text-white flex items-center justify-center shadow-[0_6px_16px_rgba(15,23,42,0.14)] flex-shrink-0`}
                          >
                            <Icon size={18} />
                          </div>

                          <div className="flex-1 min-w-0">
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="text-sm font-semibold text-slate-900 dark:text-white truncate">
                                {tool.name}
                              </span>

                              {tool.running ? (
                                <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-medium bg-emerald-100/90 dark:bg-emerald-500/20 text-emerald-700 dark:text-emerald-300">
                                  <span className="cv-live-dot" aria-hidden="true" />
                                  {t.quickLaunch.runningBadge}
                                </span>
                              ) : tool.available ? (
                                <span className="inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-medium bg-blue-50 dark:bg-blue-500/15 text-blue-700 dark:text-sky-300">
                                  {t.quickLaunch.installedBadge}
                                </span>
                              ) : (
                                <span className="inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-medium bg-slate-200/80 dark:bg-slate-800 text-slate-600 dark:text-slate-400">
                                  {t.quickLaunch.missingBadge}
                                </span>
                              )}

                              {tool.custom_path && (
                                <span className="inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-medium bg-amber-50 dark:bg-amber-500/15 text-amber-700 dark:text-amber-300 border border-amber-200/70 dark:border-amber-500/30">
                                  {t.quickLaunch.customBadge}
                                </span>
                              )}

                              {tool.supports_folder && (
                                <span className="inline-flex items-center px-1.5 py-0.5 rounded-md text-[10px] bg-slate-100 dark:bg-slate-800/80 text-slate-500 dark:text-slate-400">
                                  {t.quickLaunch.supportsFolderBadge}
                                </span>
                              )}
                            </div>

                            <p
                              className="mt-1 text-[11px] font-mono text-slate-500 dark:text-slate-400 truncate"
                              title={resolvedPath ?? t.quickLaunch.pathMissingHint}
                            >
                              {resolvedPath ?? t.quickLaunch.pathMissingHint}
                            </p>
                          </div>
                        </div>

                        <div className="flex items-center justify-between gap-2 pt-1 border-t border-slate-100 dark:border-slate-800/80">
                          <div className="flex items-center gap-1.5">
                            <button
                              type="button"
                              onClick={() => void handleBrowseCustomPath(tool)}
                              disabled={isSaving}
                              aria-label={`${t.quickLaunch.browsePath} ${tool.name}`}
                              title={t.quickLaunch.browsePath}
                              className="h-7 px-2 inline-flex items-center gap-1 rounded-lg text-[11px] font-medium border border-slate-200/90 dark:border-slate-700/80 bg-white dark:bg-[#141d2f] text-slate-600 dark:text-slate-300 hover:text-slate-900 dark:hover:text-white hover:bg-slate-50 dark:hover:bg-[#1e2a42] transition-colors disabled:opacity-50"
                            >
                              <FolderSearch size={12} />
                              <span>{t.quickLaunch.browsePath}</span>
                            </button>

                            {tool.custom_path && (
                              <button
                                type="button"
                                onClick={() => void handleResetCustomPath(tool)}
                                disabled={isSaving}
                                aria-label={`${t.quickLaunch.resetPath} ${tool.name}`}
                                title={t.quickLaunch.resetPath}
                                className="h-7 px-2 inline-flex items-center gap-1 rounded-lg text-[11px] font-medium border border-slate-200/90 dark:border-slate-700/80 bg-white dark:bg-[#141d2f] text-slate-500 dark:text-slate-400 hover:text-rose-600 dark:hover:text-rose-400 hover:bg-rose-50/60 dark:hover:bg-rose-500/10 transition-colors disabled:opacity-50"
                              >
                                <RotateCcw size={11} />
                                <span>{t.quickLaunch.resetPath}</span>
                              </button>
                            )}
                          </div>

                          <Button
                            size="sm"
                            variant={tool.available ? 'primary' : 'secondary'}
                            disabled={!tool.available || isLaunching}
                            onClick={() => void handleLaunchTool(tool)}
                            aria-label={`${t.quickLaunch.launch} ${tool.name}`}
                            icon={
                              isLaunching ? (
                                <Loader2 size={12} className="animate-spin" />
                              ) : (
                                <Play size={12} />
                              )
                            }
                            className="!h-7 !px-3"
                          >
                            {isLaunching ? t.quickLaunch.launching : t.quickLaunch.launch}
                          </Button>
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </Modal>
  );
};
