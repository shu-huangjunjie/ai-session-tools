import React, { useState, useMemo, useEffect, useLayoutEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderOpen,
  MessageSquare,
  Search,
  Code2,
  Trash2,
  Edit2,
  FileDown,
  X,
  Pin,
  PinOff,
} from 'lucide-react';
import { CodexVisibility, ConversationSummary, FolderPin } from '../types';
import { folderPinKey } from '../utils/folderPins';
import { extractProjectName } from '../utils/formatters';
import { ItemGroup, groupByFolder, splitByCodexScope } from '../utils/codexScope';
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
  onOpenFolder: (path: string) => void;
  onOpenVSCode: (path: string) => void;
  onRename: (id: string, currentTitle: string) => void;
  onDelete: (id: string, title: string) => void;
  onSetPinned: (id: string, pinned: boolean) => void;
  folderPins: FolderPin[];
  onSetFolderPinned: (path: string, pinned: boolean) => void;
  searchQuery: string;
  onSearchChange: (query: string) => void;
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
  onOpenFolder,
  onOpenVSCode,
  onRename,
  onDelete,
  onSetPinned,
  folderPins,
  onSetFolderPinned,
  searchQuery,
  onSearchChange,
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

  const selecting = publishSelection.size > 0;
  // Checkboxes stay out of the way until hovered, or while a selection is in progress.
  const checkboxClass = (checked: boolean, hoverGroup: string) =>
    `rounded border-black/20 dark:border-white/20 w-3.5 h-3.5 flex-shrink-0 cursor-pointer accent-[#0071e3] transition-opacity ${
      checked || selecting ? '' : `opacity-0 ${hoverGroup} focus:opacity-100`
    }`;

  return (
    <aside
      className="h-full bg-white dark:bg-[#0c0c0e] flex flex-col flex-shrink-0 select-none overflow-hidden transition-colors"
      style={{ width: `${width}px` }}
      onClick={closeContextMenu}
    >
      <div className="px-3 pt-3 pb-2">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-[#86868b]" size={13} />
          <input
            id="sidebar-search"
            type="search"
            aria-label={t.sidebar.searchPlaceholder}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={t.sidebar.searchPlaceholder}
            className="w-full bg-black/[0.045] dark:bg-white/[0.07] text-[#1d1d1f] dark:text-[#ededf0] text-xs pl-8 pr-14 py-2 rounded-lg border border-transparent focus:border-[#0071e3] focus:bg-white dark:focus:bg-white/[0.09] focus:outline-none placeholder-[#86868b] transition-all"
          />
          <kbd className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] text-[#86868b] font-mono px-1 rounded border border-black/[0.08] dark:border-white/[0.12] bg-white dark:bg-transparent select-none">
            Ctrl K
          </kbd>
        </div>
      </div>

      {/* Project tree */}
      <div className="flex-1 overflow-y-auto px-2 pb-2">
        {treeRows.length === 0 ? (
          <div className="text-center py-16 px-4 text-[#86868b] text-xs leading-relaxed">
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
                'w-full flex items-center justify-between px-2 pt-3 pb-1 text-[11px] font-medium text-[#86868b]';
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
              <div key={group.key} className="pt-1">
                {/* Project Header Row */}
                <div
                  onContextMenu={(e) => {
                    if (!folder) return;
                    e.preventDefault();
                    setContextMenu({ kind: 'folder', x: e.clientX, y: e.clientY, path: folder });
                  }}
                  className="group flex items-center gap-1.5 pl-2 pr-1.5 py-1.5 rounded-lg hover:bg-black/[0.035] dark:hover:bg-white/[0.05] transition-colors"
                >
                  <input
                    type="checkbox"
                    checked={allChecked}
                    ref={(el) => {
                      if (el) el.indeterminate = someChecked;
                    }}
                    onChange={() => onToggleProjectPublishSelection(convIds)}
                    className={checkboxClass(allChecked || someChecked, 'group-hover:opacity-100')}
                  />

                  <button
                    onClick={() => toggleCollapse(group.key)}
                    className="flex items-center gap-1.5 text-left min-w-0 flex-1 text-[12.5px] font-semibold text-[#1d1d1f] dark:text-[#f5f5f7]"
                  >
                    {isCollapsed ? (
                      <ChevronRight size={13} className="text-[#86868b] flex-shrink-0" />
                    ) : (
                      <ChevronDown size={13} className="text-[#86868b] flex-shrink-0" />
                    )}
                    {group.key === PINNED_KEY ? (
                      <Pin size={13} className="text-[#0071e3] dark:text-[#2997ff] flex-shrink-0" />
                    ) : group.key === CHATS_KEY ? (
                      <MessageSquare size={13} className="text-[#86868b] flex-shrink-0" />
                    ) : isCollapsed ? (
                      <Folder size={13} className="text-[#86868b] flex-shrink-0" />
                    ) : (
                      <FolderOpen size={13} className="text-[#86868b] flex-shrink-0" />
                    )}
                    <span className="truncate tracking-tight" title={folder ?? projectName}>
                      {projectName}
                    </span>
                  </button>

                  {/* Folder shortcuts, then the count */}
                  {folder && (
                    <div className="flex items-center flex-shrink-0">
                      <button
                        type="button"
                        title={isFolderPinned ? t.sidebar.unpinFolder : t.sidebar.pinFolder}
                        aria-pressed={isFolderPinned}
                        onClick={() => onSetFolderPinned(folder, !isFolderPinned)}
                        className={`p-1 rounded-md hover:bg-black/[0.06] dark:hover:bg-white/[0.1] transition-opacity ${
                          isFolderPinned
                            ? 'text-[#0071e3] dark:text-[#2997ff]'
                            : 'text-[#86868b] opacity-0 group-hover:opacity-100 focus:opacity-100'
                        }`}
                      >
                        <Pin size={12} />
                      </button>
                      <button
                        onClick={() => onOpenFolder(folder)}
                        title={t.sidebar.projectOpenFolder}
                        className="p-1 rounded-md hover:bg-black/[0.06] dark:hover:bg-white/[0.1] text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
                      >
                        <Folder size={12} />
                      </button>
                      <button
                        onClick={() => onOpenVSCode(folder)}
                        title={t.sidebar.projectOpenVSCode}
                        className="p-1 rounded-md hover:bg-black/[0.06] dark:hover:bg-white/[0.1] text-[#86868b] hover:text-[#0071e3] dark:hover:text-[#2997ff] opacity-0 group-hover:opacity-100 focus:opacity-100 transition-opacity"
                      >
                        <Code2 size={12} />
                      </button>
                    </div>
                  )}
                  <span className="text-[11px] text-[#86868b] font-mono flex-shrink-0 min-w-[16px] text-right">
                    {items.length}
                  </span>
                </div>

                {/* Conversation Children */}
                {!isCollapsed && (
                  <div className="space-y-px">
                    {isFolderPinned && items.length === 0 && (
                      <p className="pl-9 pr-2 py-1.5 text-[11px] text-[#86868b]">{t.sidebar.folderConversationsPinned}</p>
                    )}
                    {items.map((conv) => {
                      const isSelected = selectedId === conv.id;
                      const isChecked = publishSelection.has(conv.id);

                      return (
                        <div
                          key={conv.id}
                          onContextMenu={(e) => handleContextMenu(e, conv)}
                          onClick={() => onSelectConversation(conv.id)}
                          className={`group/conversation flex items-center gap-2 pl-2 pr-1.5 py-[7px] ml-4 rounded-lg cursor-pointer transition-colors duration-150 text-[13px] ${
                            isSelected
                              ? 'bg-[#0071e3]/[0.09] dark:bg-[#2997ff]/15 text-[#0071e3] dark:text-[#64b5ff] font-medium'
                              : 'text-[#3a3a3c] dark:text-[#c7c7cc] hover:bg-black/[0.035] dark:hover:bg-white/[0.05] hover:text-[#1d1d1f] dark:hover:text-white'
                          }`}
                        >
                          <input
                            type="checkbox"
                            checked={isChecked}
                            onClick={(e) => e.stopPropagation()}
                            onChange={() => onTogglePublishSelection(conv.id)}
                            className={checkboxClass(isChecked, 'group-hover/conversation:opacity-100')}
                          />

                          <span className="truncate flex-1 tracking-tight" title={conv.title}>
                            {conv.title}
                          </span>

                          <button
                            type="button"
                            title={conv.pinned_at ? t.sidebar.unpinConversation : t.sidebar.pinConversation}
                            aria-pressed={!!conv.pinned_at}
                            onClick={(e) => {
                              e.stopPropagation();
                              onSetPinned(conv.id, !conv.pinned_at);
                            }}
                            className={`p-0.5 rounded-md hover:bg-black/[0.06] dark:hover:bg-white/[0.1] transition-opacity flex-shrink-0 ${
                              conv.pinned_at
                                ? 'text-[#0071e3] dark:text-[#2997ff]'
                                : 'text-[#86868b] opacity-0 group-hover/conversation:opacity-100 focus:opacity-100'
                            }`}
                          >
                            <Pin size={12} />
                          </button>
                          <span className="text-[11px] text-[#86868b] flex-shrink-0 font-mono font-normal min-w-[16px] text-right">
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

      {/* Batch actions for the checked conversations */}
      {selecting && (
        <div className="m-2 mt-0 pl-3 pr-1.5 py-1.5 rounded-xl bg-[#1d1d1f] dark:bg-[#2c2c2e] text-white flex items-center gap-1.5 text-xs shadow-lg animate-in fade-in slide-in-from-bottom-1 duration-150">
          <span className="flex-1 font-medium truncate">
            {interpolate(t.sidebar.selectedCount, { count: publishSelection.size })}
          </span>
          <button
            type="button"
            onClick={() => onBatchExportMarkdown(Array.from(publishSelection))}
            title={t.sidebar.batchExportMdTooltip}
            className="flex items-center gap-1 px-2 py-1 rounded-md border border-white/20 hover:bg-white/10 transition-colors"
          >
            <FileDown size={12} />
            {t.sidebar.batchExportMd}
          </button>
          <button
            type="button"
            onClick={() => onBatchDelete(Array.from(publishSelection))}
            title={t.sidebar.batchDeleteTooltip}
            className="flex items-center gap-1 px-2 py-1 rounded-md border border-white/20 hover:bg-[#ff453a]/30 transition-colors"
          >
            <Trash2 size={12} />
            {t.common.delete}
          </button>
          <button
            type="button"
            onClick={onClearSelection}
            title={t.sidebar.clearSelectionTooltip}
            aria-label={t.sidebar.clearSelectionTooltip}
            className="p-1 rounded-md text-white/70 hover:text-white hover:bg-white/10 transition-colors"
          >
            <X size={13} />
          </button>
        </div>
      )}

      {/* Right-click Context Menu. Rendered into <body> so that no ancestor's
          overflow or filter clips or repositions this fixed element. */}
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
