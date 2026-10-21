import React, { useState, useMemo, useEffect, useLayoutEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  FolderSync,
  MessageSquare,
  Search,
  Send,
  Sparkles,
  Code2,
  Trash2,
  Edit2,
  FileDown,
  CheckSquare,
  X,
  UploadCloud,
  Bot,
  ExternalLink,
  Loader2,
  Pin,
  PinOff,
} from 'lucide-react';
import { CodexVisibility, ConversationSummary, FolderPin } from '../types';
import { folderPinKey } from '../utils/folderPins';
import { extractProjectName } from '../utils/formatters';
import { ItemGroup, groupByFolder, splitByCodexScope } from '../utils/codexScope';
import { Button } from './common/Button';
import { useI18n } from '../i18n';

const CHATS_KEY = 'chats';
const PINNED_KEY = 'pinned';

type TreeRow =
  | {
      kind: 'section';
      key: string;
      label: string;
      tooltip: string;
      count: number;
      /** Expands and collapses the groups after it. */
      collapsible?: boolean;
    }
  | { kind: 'group'; group: ItemGroup<ConversationSummary> };

interface SidebarProps {
  width: number;
  conversations: ConversationSummary[];
  selectedId: string | null;
  onSelectConversation: (id: string) => void;
  activeSource: 'codex' | 'antigravity' | 'claude';
  publishSelection: Set<string>;
  onTogglePublishSelection: (id: string) => void;
  onToggleProjectPublishSelection: (convIds: string[]) => void;
  onClearSelection: () => void;
  onBatchExportMarkdown: (selectedIds: string[]) => void;
  onBatchDelete: (selectedIds: string[]) => void;
  onScanCodex: () => void;
  onScanAntigravity: () => void;
  onScanClaude: () => void;
  onPublishToCodex: () => void;
  onPublishToAntigravity: () => void;
  onPublishToClaude: () => void;
  onOpenFolder: (path: string) => void;
  onOpenVSCode: (path: string) => void;
  onRename: (id: string, currentTitle: string) => void;
  onDelete: (id: string, title: string) => void;
  onSetPinned: (id: string, pinned: boolean) => void;
  folderPins: FolderPin[];
  onSetFolderPinned: (path: string, pinned: boolean) => void;
  searchQuery: string;
  showArchived: boolean;
  onShowArchivedChange: (show: boolean) => void;
  onSearchChange: (query: string) => void;
  onOpenClient?: (source: 'codex' | 'antigravity' | 'claude') => void;
  isOpeningClient?: boolean;
  isScanning?: boolean;
  /** Where Codex lists each conversation; the Codex tab groups by folder without it. */
  codexVisibility?: CodexVisibility | null;
}

export const Sidebar: React.FC<SidebarProps> = ({
  width,
  conversations,
  selectedId,
  onSelectConversation,
  activeSource,
  publishSelection,
  onTogglePublishSelection,
  onToggleProjectPublishSelection,
  onClearSelection,
  onBatchExportMarkdown,
  onBatchDelete,
  onScanCodex,
  onScanAntigravity,
  onScanClaude,
  onPublishToCodex,
  onPublishToAntigravity,
  onPublishToClaude,
  onOpenFolder,
  onOpenVSCode,
  onRename,
  onDelete,
  onSetPinned,
  folderPins,
  onSetFolderPinned,
  searchQuery,
  showArchived,
  onShowArchivedChange,
  onSearchChange,
  onOpenClient,
  isOpeningClient = false,
  isScanning = false,
  codexVisibility = null,
}) => {
  const { t, interpolate } = useI18n();
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = useState<(
    { kind: 'conversation'; x: number; y: number; conv: ConversationSummary } |
    { kind: 'folder'; x: number; y: number; path: string }
  ) | null>(null);

  const uncategorized = t.common.uncategorized;
  const pinnedFolderTimes = useMemo(() => new Map(
    folderPins.filter((pin) => pin.source === activeSource).map((pin) => [folderPinKey(pin.path), pin.pinned_at])
  ), [folderPins, activeSource]);
  const pinnedConversations = useMemo(
    () => conversations.filter((c) => c.pinned_at).sort(
      (a, b) => b.pinned_at!.localeCompare(a.pinned_at!) || a.id.localeCompare(b.id)
    ),
    [conversations]
  );
  const unpinnedConversations = useMemo(
    () => conversations.filter((c) => !c.pinned_at),
    [conversations]
  );

  // The Codex tab lists conversations the way the Codex sidebar does, when Codex
  // keeps project state; otherwise every tab groups by folder.
  const codexSections = useMemo(
    () =>
      activeSource === 'codex' && codexVisibility?.available
        ? splitByCodexScope(
            unpinnedConversations,
            (c) => codexVisibility.conversations[c.id],
            codexVisibility.projects,
            (c) => c.project_path,
            uncategorized
          )
        : null,
    [activeSource, codexVisibility, unpinnedConversations, uncategorized]
  );
  const folderGroups = useMemo(
    () => (codexSections ? [] : groupByFolder(unpinnedConversations, (c) => c.project_path, uncategorized)),
    [codexSections, unpinnedConversations, uncategorized]
  );

  const pinnedFolderGroups = useMemo(() => {
    const sections = activeSource === 'codex' && codexVisibility?.available
      ? splitByCodexScope(conversations, (c) => codexVisibility.conversations[c.id],
          codexVisibility.projects, (c) => c.project_path, uncategorized)
      : null;
    const groups = sections ? [...sections.projects, ...sections.hidden]
      : groupByFolder(conversations, (c) => c.project_path, uncategorized);
    return groups.filter((group) => group.path && pinnedFolderTimes.has(folderPinKey(group.path)))
      .map((group) => ({ ...group, items: group.items.filter((c) => !c.pinned_at) }))
      .sort((a, b) => pinnedFolderTimes.get(folderPinKey(b.path!))!.localeCompare(
        pinnedFolderTimes.get(folderPinKey(a.path!))!) || a.key.localeCompare(b.key));
  }, [activeSource, codexVisibility, conversations, uncategorized, pinnedFolderTimes]);

  const [hiddenExpanded, setHiddenExpanded] = useState(
    () => localStorage.getItem('cv_codex_hidden_expanded') === '1'
  );
  const toggleHiddenExpanded = () => {
    setHiddenExpanded((prev) => {
      localStorage.setItem('cv_codex_hidden_expanded', prev ? '0' : '1');
      return !prev;
    });
  };
  // Search results must not stay behind the collapsed section.
  const showHidden = hiddenExpanded || searchQuery.trim() !== '';

  const treeRows = useMemo<TreeRow[]>(() => {
    const rows: TreeRow[] = [];
    if (pinnedConversations.length > 0) {
      rows.push({ kind: 'group', group: {
        key: PINNED_KEY, name: t.sidebar.pinned, items: pinnedConversations,
      } });
    }
    if (pinnedFolderGroups.length > 0) {
      rows.push({ kind: 'section', key: 'section:pinned-folders', label: t.sidebar.pinnedFolders,
        tooltip: t.sidebar.pinnedFolders, count: pinnedFolderGroups.length });
      rows.push(...pinnedFolderGroups.map((group): TreeRow => ({ kind: 'group', group })));
    }
    const ordinary = (group: ItemGroup<ConversationSummary>) =>
      !group.path || !pinnedFolderTimes.has(folderPinKey(group.path));
    if (!codexSections) {
      return [...rows, ...folderGroups.filter(ordinary).map((group): TreeRow => ({ kind: 'group', group }))];
    }
    const visible = codexSections.projects.filter(ordinary);
    if (codexSections.chats.length > 0) {
      visible.push({ key: CHATS_KEY, name: t.sidebar.codexChats, items: codexSections.chats });
    }
    if (visible.length > 0) {
      rows.push({
        kind: 'section',
        key: 'section:visible',
        label: t.sidebar.codexVisible,
        tooltip: t.sidebar.codexVisibleTooltip,
        count: visible.reduce((n, g) => n + g.items.length, 0),
      });
      rows.push(...visible.map((group): TreeRow => ({ kind: 'group', group })));
    }
    const hidden = codexSections.hidden.filter(ordinary);
    const hiddenCount = hidden.reduce((count, group) => count + group.items.length, 0);
    if (hiddenCount > 0) {
      rows.push({
        kind: 'section',
        key: 'section:hidden',
        label: t.sidebar.codexHidden,
        tooltip: t.sidebar.codexHiddenTooltip,
        count: hiddenCount,
        collapsible: true,
      });
      if (showHidden) {
        rows.push(...hidden.map((group): TreeRow => ({ kind: 'group', group })));
      }
    }
    return rows;
  }, [pinnedConversations, pinnedFolderGroups, pinnedFolderTimes, codexSections, folderGroups, showHidden, t.sidebar]);

  const toggleCollapse = (projectKey: string) => {
    setCollapsedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(projectKey)) {
        next.delete(projectKey);
      } else {
        next.add(projectKey);
      }
      return next;
    });
  };

  const handleContextMenu = (e: React.MouseEvent, conv: ConversationSummary) => {
    e.preventDefault();
    setContextMenu({
      kind: 'conversation',
      x: e.clientX,
      y: e.clientY,
      conv,
    });
  };

  const closeContextMenu = () => setContextMenu(null);

  // Close the menu on any outside interaction, and keep it inside the window.
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!contextMenu) return;
    const close = (e: Event) => {
      if (e instanceof KeyboardEvent && e.key !== 'Escape') return;
      if (e.type === 'mousedown' && menuRef.current?.contains(e.target as Node)) return;
      setContextMenu(null);
    };
    window.addEventListener('mousedown', close);
    window.addEventListener('keydown', close);
    window.addEventListener('blur', close);
    window.addEventListener('resize', close);
    document.addEventListener('scroll', close, true);
    return () => {
      window.removeEventListener('mousedown', close);
      window.removeEventListener('keydown', close);
      window.removeEventListener('blur', close);
      window.removeEventListener('resize', close);
      document.removeEventListener('scroll', close, true);
    };
  }, [contextMenu]);
  useLayoutEffect(() => {
    const el = menuRef.current;
    if (!el || !contextMenu) return;
    const rect = el.getBoundingClientRect();
    const x = Math.max(4, Math.min(contextMenu.x, window.innerWidth - rect.width - 4));
    const y = Math.max(4, Math.min(contextMenu.y, window.innerHeight - rect.height - 4));
    el.style.left = `${x}px`;
    el.style.top = `${y}px`;
  }, [contextMenu]);

  const emptyText = searchQuery.trim()
    ? interpolate(t.sidebar.noSearchResults, { query: searchQuery.trim() })
    : activeSource === 'codex'
    ? t.sidebar.emptyCodex
    : activeSource === 'antigravity'
    ? t.sidebar.emptyAntigravity
    : t.sidebar.emptyClaude;

  return (
    <aside
      className="h-full bg-[#fbfbfd]/90 dark:bg-[#0a0a0c]/90 backdrop-blur-2xl border-r border-black/[0.06] dark:border-white/[0.08] flex flex-col flex-shrink-0 select-none overflow-hidden"
      style={{ width: `${width}px` }}
      onClick={closeContextMenu}
    >
      {/* Apple Spotlight Search & Quick Action Buttons */}
      <div className="p-3.5 border-b border-black/[0.06] dark:border-white/[0.06] flex flex-col gap-3 bg-black/[0.01] dark:bg-white/[0.02]">
        {/* Spotlight Search Bar */}
        <div className="relative">
          <Search className="absolute left-3 top-2.5 text-[#86868b]" size={13} />
          <input
            id="sidebar-search"
            type="search"
            aria-label={t.sidebar.searchPlaceholder}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={t.sidebar.searchPlaceholder}
            className="w-full bg-black/[0.04] dark:bg-white/[0.06] text-[#1d1d1f] dark:text-[#ededf0] text-xs pl-8 pr-12 py-1.5 rounded-full border border-black/[0.06] dark:border-white/[0.08] focus:border-[#0071e3] focus:bg-white dark:focus:bg-white/[0.09] focus:outline-none placeholder-[#86868b] transition-all shadow-inner"
          />
          <span className="absolute right-3 top-2 text-[10px] text-[#86868b] font-mono select-none">
            Ctrl+K
          </span>
        </div>

        {/* Action Pills & Batch Actions */}
        {publishSelection.size > 0 ? (
          <div className="p-2.5 rounded-2xl bg-black/[0.03] dark:bg-white/[0.04] border border-black/[0.08] dark:border-white/[0.1] flex flex-col gap-2 shadow-sm animate-in fade-in slide-in-from-top-1 duration-200">
            <div className="flex items-center justify-between text-xs px-0.5">
              <span className="font-medium text-[#1d1d1f] dark:text-[#ededf0] flex items-center gap-1.5">
                <CheckSquare size={13} className="text-[#0071e3] dark:text-[#2997ff]" />
                {interpolate(t.sidebar.selectedCount, { count: publishSelection.size })}
              </span>
              <button
                onClick={onClearSelection}
                className="text-[11px] text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#ededf0] flex items-center gap-0.5 transition-colors"
                title={t.sidebar.clearSelectionTooltip}
              >
                <X size={11} />
                {t.sidebar.clearSelection}
              </button>
            </div>

            <div className="flex items-center gap-1.5">
              <Button
                variant="primary"
                size="sm"
                icon={<FileDown size={13} />}
                onClick={() => onBatchExportMarkdown(Array.from(publishSelection))}
                className="flex-1 text-xs bg-[#0071e3] hover:bg-[#0077ed] text-white shadow-md shadow-blue-500/15 font-medium"
                title={t.sidebar.batchExportMdTooltip}
              >
                {t.sidebar.batchExportMd}
              </Button>

              {activeSource === 'codex' && (
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<Send size={12} />}
                  onClick={onPublishToCodex}
                  className="text-xs"
                  title={t.sidebar.publishTooltip}
                >
                  {t.sidebar.publish}
                </Button>
              )}

              {activeSource === 'antigravity' && (
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<UploadCloud size={12} className="text-[#9333ea]" />}
                  onClick={onPublishToAntigravity}
                  className="text-xs text-[#9333ea] hover:text-[#7e22ce] dark:text-[#a855f7]"
                  title={t.sidebar.updateToAGTooltip}
                >
                  {t.sidebar.updateToAG}
                </Button>
              )}

              {activeSource === 'claude' && (
                <Button
                  variant="secondary"
                  size="sm"
                  icon={<UploadCloud size={12} className="text-[#d97706]" />}
                  onClick={onPublishToClaude}
                  className="text-xs text-[#d97706] hover:text-[#b45309] dark:text-[#f59e0b]"
                  title={t.sidebar.updateToClaudeTooltip}
                >
                  {t.sidebar.updateToClaude}
                </Button>
              )}

              <Button
                variant="danger"
                size="sm"
                icon={<Trash2 size={12} />}
                onClick={() => onBatchDelete(Array.from(publishSelection))}
                className="text-xs px-2.5"
                title={t.sidebar.batchDeleteTooltip}
              />
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {/* Quick Open Client Launcher */}
            {onOpenClient && (
              <button
                type="button"
                onClick={() => onOpenClient(activeSource)}
                disabled={isOpeningClient}
                className={`w-full py-1.5 px-3 rounded-xl text-xs font-medium flex items-center justify-center gap-1.5 transition-all duration-200 border shadow-sm active:scale-[0.99] select-none ${
                  activeSource === 'codex'
                    ? 'bg-[#0071e3]/10 hover:bg-[#0071e3]/15 text-[#0071e3] dark:text-[#2997ff] border-[#0071e3]/20 hover:border-[#0071e3]/30'
                    : activeSource === 'antigravity'
                    ? 'bg-[#af52de]/10 hover:bg-[#af52de]/15 text-[#af52de] dark:text-[#bf5af2] border-[#af52de]/20 hover:border-[#af52de]/30'
                    : 'bg-[#d97706]/10 hover:bg-[#d97706]/15 text-[#d97706] dark:text-[#f59e0b] border-[#d97706]/20 hover:border-[#d97706]/30'
                } ${isOpeningClient ? 'opacity-70 cursor-not-allowed' : ''}`}
                title={interpolate(t.sidebar.openClientTooltip, {
                  name: activeSource === 'codex' ? 'Codex' : activeSource === 'antigravity' ? 'Antigravity' : 'Claude',
                })}
              >
                {isOpeningClient ? (
                  <>
                    <Loader2 size={13} className="animate-spin" />
                    <span>{t.sidebar.launchingClient}</span>
                  </>
                ) : (
                  <>
                    <ExternalLink size={12} className="opacity-80" />
                    <span>
                      {interpolate(t.sidebar.openClientNamed, {
                        name: activeSource === 'codex' ? 'Codex' : activeSource === 'antigravity' ? 'Antigravity' : 'Claude',
                      })}
                    </span>
                  </>
                )}
              </button>
            )}

            {/* Scan and Publish Operations */}
            <div className="flex items-center gap-2">
              {activeSource === 'codex' ? (
                <>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={isScanning ? <Loader2 size={13} className="animate-spin" /> : <FolderSync size={13} className="text-[#0071e3] dark:text-[#2997ff]" />}
                    onClick={onScanCodex}
                    disabled={isScanning}
                    className="flex-1 text-xs"
                    title={t.sidebar.scanCodexTooltip}
                  >
                    {t.sidebar.scanCodex}
                  </Button>
                  <Button
                    variant="primary"
                    size="sm"
                    icon={<Send size={13} />}
                    onClick={onPublishToCodex}
                    className="flex-1 text-xs bg-[#0071e3] hover:bg-[#0077ed] text-white shadow-sm font-medium"
                    title={t.sidebar.publishToCodexTooltip}
                  >
                    {t.sidebar.publishToCodex}
                  </Button>
                </>
              ) : activeSource === 'antigravity' ? (
                <>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={isScanning ? <Loader2 size={13} className="animate-spin" /> : <Sparkles size={13} className="text-[#af52de] dark:text-[#bf5af2]" />}
                    onClick={onScanAntigravity}
                    disabled={isScanning}
                    className="flex-1 text-xs"
                    title={t.sidebar.scanAntigravityTooltip}
                  >
                    {t.sidebar.scanAntigravity}
                  </Button>
                  <Button
                    variant="primary"
                    size="sm"
                    icon={<UploadCloud size={13} />}
                    onClick={onPublishToAntigravity}
                    className="flex-1 text-xs !bg-[#af52de] hover:!bg-[#9d44c7] text-white shadow-sm font-medium"
                    title={t.sidebar.updateToAntigravityTooltip}
                  >
                    {t.sidebar.updateToAntigravity}
                  </Button>
                </>
              ) : (
                <>
                  <Button
                    variant="secondary"
                    size="sm"
                    icon={isScanning ? <Loader2 size={13} className="animate-spin" /> : <Bot size={13} className="text-[#d97706] dark:text-[#f59e0b]" />}
                    onClick={onScanClaude}
                    disabled={isScanning}
                    className="flex-1 text-xs"
                    title={t.sidebar.scanClaudeTooltip}
                  >
                    {t.sidebar.scanClaude}
                  </Button>
                  <Button
                    variant="primary"
                    size="sm"
                    icon={<UploadCloud size={13} />}
                    onClick={onPublishToClaude}
                    className="flex-1 text-xs !bg-[#d97706] hover:!bg-[#b45309] text-white shadow-sm font-medium"
                    title={t.sidebar.updateToClaudeClientTooltip}
                  >
                    {t.sidebar.updateToClaudeClient}
                  </Button>
                </>
              )}
            </div>
          </div>
        )}

        {/* Historical Projects Toggle & Source Tag */}
        <div className="flex items-center justify-between px-1 text-[11px] text-[#86868b]">
          <label className="flex items-center gap-2 cursor-pointer hover:text-[#1d1d1f] dark:hover:text-[#ededf0] transition-colors">
            <input
              type="checkbox"
              checked={showArchived}
              onChange={(e) => onShowArchivedChange(e.target.checked)}
              className="rounded-full bg-black/[0.05] dark:bg-white/[0.08] border-black/20 dark:border-white/20 text-[#0071e3] focus:ring-0 w-3.5 h-3.5 cursor-pointer accent-[#0071e3]"
            />
            {t.sidebar.showArchived}
          </label>
          {activeSource === 'codex' ? (
            <span className="text-[#86868b]/70">{t.sidebar.checkToBatchPublish}</span>
          ) : activeSource === 'antigravity' ? (
            <span className="text-[#af52de]/80 dark:text-[#bf5af2]/80 font-medium">
              {t.sidebar.agVault}
            </span>
          ) : (
            <span className="text-[#d97706]/80 dark:text-[#f59e0b]/80 font-medium">
              {t.sidebar.claudeVault}
            </span>
          )}
        </div>
      </div>

      {/* macOS Finder-style Project Tree View */}
      <div className="flex-1 overflow-y-auto p-2.5 space-y-1.5">
        {treeRows.length === 0 ? (
          <div className="text-center py-16 text-[#86868b] text-xs">
            {emptyText}
          </div>
        ) : (

          treeRows.map((row) => {
            if (row.kind === 'section') {
              const content = (
                <>
                  <span className="flex items-center gap-1 truncate">
                    {row.collapsible &&
                      (showHidden ? <ChevronDown size={11} /> : <ChevronRight size={11} />)}
                    {row.label}
                  </span>
                  <span className="font-mono">{row.count}</span>
                </>
              );
              const className =
                'w-full flex items-center justify-between px-2 pt-2 pb-0.5 text-[10px] font-semibold uppercase tracking-wider text-[#86868b]';
              return row.collapsible ? (
                <button
                  key={row.key}
                  type="button"
                  onClick={toggleHiddenExpanded}
                  aria-expanded={showHidden}
                  title={row.tooltip}
                  className={`${className} hover:text-[#1d1d1f] dark:hover:text-[#ededf0] transition-colors`}
                >
                  {content}
                </button>
              ) : (
                <div key={row.key} title={row.tooltip} className={className}>
                  {content}
                </div>
              );
            }

            const { group } = row;
            const items = group.items;
            const folder = group.path;
            const isFolderPinned = !!folder && pinnedFolderTimes.has(folderPinKey(folder));
            const projectName = group.name ?? extractProjectName(folder ?? '');
            const isCollapsed = collapsedProjects.has(group.key);
            const convIds = items.map((i) => i.id);
            const allChecked = convIds.length > 0 && convIds.every((id) => publishSelection.has(id));
            const someChecked = convIds.some((id) => publishSelection.has(id)) && !allChecked;

            return (
              <div
                key={group.key}
                className="rounded-2xl overflow-hidden border border-black/[0.06] dark:border-white/[0.06] bg-white dark:bg-white/[0.02] shadow-[0_1px_3px_rgba(0,0,0,0.03)] dark:shadow-none transition-colors"
              >
                {/* Project Header Row */}
                <div
                  onContextMenu={(e) => {
                    if (!folder) return;
                    e.preventDefault();
                    setContextMenu({ kind: 'folder', x: e.clientX, y: e.clientY, path: folder });
                  }}
                  className="flex items-center justify-between px-2.5 py-2 hover:bg-black/[0.03] dark:hover:bg-white/[0.04] transition-colors group"
                >
                  <div className="flex items-center gap-2 min-w-0 flex-1">
                    <input
                      type="checkbox"
                      checked={allChecked}
                      ref={(el) => {
                        if (el) el.indeterminate = someChecked;
                      }}
                      onChange={() => onToggleProjectPublishSelection(convIds)}
                      className="rounded bg-black/[0.05] dark:bg-white/[0.08] border-black/20 dark:border-white/20 text-[#0071e3] focus:ring-0 w-3.5 h-3.5 flex-shrink-0 cursor-pointer accent-[#0071e3]"
                    />

                    <button
                      onClick={() => toggleCollapse(group.key)}
                      className="flex items-center gap-1.5 text-left min-w-0 flex-1 text-xs font-semibold text-[#1d1d1f] dark:text-[#f5f5f7] truncate"
                    >
                      {isCollapsed ? (
                        <ChevronRight size={13} className="text-[#86868b] flex-shrink-0" />
                      ) : (
                        <ChevronDown size={13} className="text-[#86868b] dark:text-[#a1a1a6] flex-shrink-0" />
                      )}
                      {group.key === PINNED_KEY ? (
                        <Pin size={13} className="text-[#0071e3] dark:text-[#2997ff] flex-shrink-0" />
                      ) : group.key === CHATS_KEY ? (
                        <MessageSquare size={13} className="text-[#0071e3] dark:text-[#2997ff] flex-shrink-0" />
                      ) : isCollapsed ? (
                        <Folder size={13} className="text-[#86868b] flex-shrink-0" />
                      ) : (
                        <FolderOpen size={13} className="text-[#0071e3] dark:text-[#2997ff] flex-shrink-0" />
                      )}
                      <span className="truncate tracking-tight" title={folder ?? projectName}>
                        {projectName}
                      </span>
                    </button>
                  </div>

                  {/* Project Count & Action shortcuts */}
                  <div className="flex items-center gap-1.5 flex-shrink-0 ml-1">
                    <span className="text-[10px] bg-black/[0.04] dark:bg-white/[0.06] text-[#86868b] px-2 py-0.5 rounded-full font-medium">
                      {items.length}
                    </span>

                    {folder && (
                      <div className="flex items-center gap-1 opacity-60 group-hover:opacity-100 transition-opacity">
                        <button
                          type="button"
                          title={isFolderPinned ? t.sidebar.unpinFolder : t.sidebar.pinFolder}
                          aria-pressed={isFolderPinned}
                          onClick={() => onSetFolderPinned(folder, !isFolderPinned)}
                          className={`p-1 rounded-full hover:bg-black/[0.06] dark:hover:bg-white/[0.1] transition-colors ${
                            isFolderPinned ? 'text-[#0071e3] dark:text-[#2997ff]' : 'text-[#86868b]'
                          }`}
                        >
                          <Pin size={11} />
                        </button>
                        <button
                          onClick={() => onOpenFolder(folder)}
                          title={t.sidebar.projectOpenFolder}
                          className="p-1 rounded-full hover:bg-black/[0.06] dark:hover:bg-white/[0.1] text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white transition-colors"
                        >
                          <Folder size={11} />
                        </button>

                        <button
                          onClick={() => onOpenVSCode(folder)}
                          title={t.sidebar.projectOpenVSCode}
                          className="p-1 rounded-full hover:bg-black/[0.06] dark:hover:bg-white/[0.1] text-[#86868b] hover:text-[#0071e3] dark:hover:text-[#2997ff] transition-colors"
                        >
                          <Code2 size={11} />
                        </button>
                      </div>
                    )}
                  </div>
                </div>

                {/* Conversation Children */}
                {!isCollapsed && (
                  <div className="bg-black/[0.02] dark:bg-black/20 py-1 px-1.5 space-y-0.5 border-t border-black/[0.04] dark:border-white/[0.04]">
                    {isFolderPinned && items.length === 0 && (
                      <p className="px-2.5 py-2 text-[11px] text-[#86868b]">{t.sidebar.folderConversationsPinned}</p>
                    )}
                    {items.map((conv) => {
                      const isSelected = selectedId === conv.id;
                      const isChecked = publishSelection.has(conv.id);

                      return (
                        <div
                          key={conv.id}
                          onContextMenu={(e) => handleContextMenu(e, conv)}
                          onClick={() => onSelectConversation(conv.id)}
                          className={`group/conversation flex items-center justify-between px-2.5 py-1.5 rounded-xl cursor-pointer transition-all duration-150 text-xs ${
                            isSelected
                              ? 'bg-black/[0.07] dark:bg-white/[0.12] text-[#0071e3] dark:text-white font-medium shadow-sm border border-black/[0.04] dark:border-white/[0.08]'
                              : 'hover:bg-black/[0.03] dark:hover:bg-white/[0.05] text-[#515154] dark:text-[#a1a1a6] hover:text-[#1d1d1f] dark:hover:text-[#f5f5f7]'
                          }`}
                        >
                          <div className="flex items-center gap-2 min-w-0 flex-1">
                            <input
                              type="checkbox"
                              checked={isChecked}
                              onClick={(e) => e.stopPropagation()}
                              onChange={() => onTogglePublishSelection(conv.id)}
                              className="rounded bg-black/[0.05] dark:bg-white/[0.08] border-black/20 dark:border-white/20 text-[#0071e3] focus:ring-0 w-3.5 h-3.5 flex-shrink-0 cursor-pointer accent-[#0071e3]"
                            />

                            {conv.source_kind === 'antigravity' ? (
                              <Sparkles
                                size={12}
                                className={`flex-shrink-0 ${
                                  isSelected ? 'text-[#af52de] dark:text-[#bf5af2]' : 'text-[#af52de]/70'
                                }`}
                              />
                            ) : conv.source_kind === 'claude' ? (
                              <Bot
                                size={12}
                                className={`flex-shrink-0 ${
                                  isSelected ? 'text-[#d97706] dark:text-[#f59e0b]' : 'text-[#d97706]/70'
                                }`}
                              />
                            ) : (
                              <MessageSquare
                                size={12}
                                className={`flex-shrink-0 ${
                                  isSelected ? 'text-[#0071e3] dark:text-[#2997ff]' : 'text-[#86868b]'
                                }`}
                              />
                            )}

                            <span className="truncate flex-1 tracking-tight" title={conv.title}>
                              {conv.title}
                            </span>

                            {conv.source_kind === 'antigravity' ? (
                              <span className="text-[9px] px-1 py-0.2 rounded bg-purple-500/10 text-purple-600 dark:text-purple-400 border border-purple-500/20 font-medium flex-shrink-0">
                                AGY
                              </span>
                            ) : conv.source_kind === 'claude' ? (
                              <span className="text-[9px] px-1 py-0.2 rounded bg-amber-500/10 text-amber-700 dark:text-amber-300 border border-amber-500/20 font-medium flex-shrink-0">
                                CLAUDE
                              </span>
                            ) : null}

                          </div>

                          <button
                            type="button"
                            title={conv.pinned_at ? t.sidebar.unpinConversation : t.sidebar.pinConversation}
                            aria-pressed={!!conv.pinned_at}
                            onClick={(e) => {
                              e.stopPropagation();
                              onSetPinned(conv.id, !conv.pinned_at);
                            }}
                            className={`p-1 ml-1 rounded-full hover:bg-black/[0.06] dark:hover:bg-white/[0.1] transition-opacity flex-shrink-0 ${
                              conv.pinned_at
                                ? 'text-[#0071e3] dark:text-[#2997ff]'
                                : 'text-[#86868b] opacity-0 group-hover/conversation:opacity-100 focus:opacity-100'
                            }`}
                          >
                            <Pin size={12} />
                          </button>
                          <span className="text-[10px] text-[#86868b] ml-1.5 flex-shrink-0 font-mono">
                            {conv.message_count}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>

      {/* Right-click Context Menu. Rendered into <body>: the sidebar's backdrop-filter
          would otherwise become the containing block of this fixed element. */}
      {contextMenu &&
        createPortal(
          <div
            ref={menuRef}
            role="menu"
            className="fixed z-50 backdrop-blur-2xl bg-white/95 dark:bg-[#1c1c1e]/90 border border-black/[0.08] dark:border-white/[0.12] rounded-2xl shadow-2xl p-1.5 text-xs text-[#1d1d1f] dark:text-[#ededf0] min-w-[160px] animate-in fade-in zoom-in-95 duration-150"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            {contextMenu.kind === 'folder' ? (
              <button
                role="menuitem"
                onClick={() => {
                  onSetFolderPinned(contextMenu.path, !pinnedFolderTimes.has(folderPinKey(contextMenu.path)));
                  closeContextMenu();
                }}
                className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-black/[0.05] dark:hover:bg-white/[0.1] text-left transition-colors"
              >
                {pinnedFolderTimes.has(folderPinKey(contextMenu.path)) ? <PinOff size={12} /> : <Pin size={12} />}
                {pinnedFolderTimes.has(folderPinKey(contextMenu.path)) ? t.sidebar.unpinFolder : t.sidebar.pinFolder}
              </button>
            ) : (
            <>
            <button
              role="menuitem"
              onClick={() => {
                onSetPinned(contextMenu.conv.id, !contextMenu.conv.pinned_at);
                closeContextMenu();
              }}
              className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-black/[0.05] dark:hover:bg-white/[0.1] text-left transition-colors"
            >
              {contextMenu.conv.pinned_at ? <PinOff size={12} /> : <Pin size={12} />}
              {contextMenu.conv.pinned_at ? t.sidebar.unpinConversation : t.sidebar.pinConversation}
            </button>
            {publishSelection.size > 1 && publishSelection.has(contextMenu.conv.id) ? (
              <button
                onClick={() => {
                  onBatchExportMarkdown(Array.from(publishSelection));
                  closeContextMenu();
                }}
                className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-black/[0.05] dark:hover:bg-white/[0.1] text-left transition-colors font-medium text-[#0071e3] dark:text-[#2997ff]"
              >
                <FileDown size={12} />
                {t.sidebar.batchExportMd} ({publishSelection.size})
              </button>
            ) : (
              <button
                onClick={() => {
                  onBatchExportMarkdown([contextMenu.conv.id]);
                  closeContextMenu();
                }}
                className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-black/[0.05] dark:hover:bg-white/[0.1] text-left transition-colors"
              >
                <FileDown size={12} className="text-[#86868b]" />
                {t.sidebar.menuExportMd}
              </button>
            )}
            <button
              onClick={() => {
                onRename(contextMenu.conv.id, contextMenu.conv.title);
                closeContextMenu();
              }}
              className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-black/[0.05] dark:hover:bg-white/[0.1] text-left transition-colors"
            >
              <Edit2 size={12} className="text-[#86868b]" />
              {t.sidebar.menuRename}
            </button>
            <button
              onClick={() => {
                onDelete(contextMenu.conv.id, contextMenu.conv.title);
                closeContextMenu();
              }}
              className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-[#ff3b30]/15 dark:hover:bg-[#ff453a]/20 text-[#ff3b30] dark:text-[#ff453a] text-left transition-colors"
            >
              <Trash2 size={12} />
              {t.sidebar.menuDelete}
            </button>
            </>
            )}
          </div>,
          document.body
        )}
    </aside>
  );
};
