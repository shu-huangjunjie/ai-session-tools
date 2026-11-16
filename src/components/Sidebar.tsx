import React, { useState, useMemo, useEffect, useLayoutEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import {
  ArrowRightLeft,
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
  GripVertical,
  Sparkles,
  FolderPlus,
  BookOpen,
} from 'lucide-react';
import { CodexVisibility, ConversationSummary, FolderPin, PrismOpenMode } from '../types';
import { folderPinKey } from '../utils/folderPins';
import { extractProjectName } from '../utils/formatters';
import { ItemGroup, groupByFolder, splitByCodexScope } from '../utils/codexScope';
import { useI18n } from '../i18n';
import { FOLDER_ORDER_STORAGE_KEY, moveFolder, readFolderOrders, sortFolders } from '../utils/folderOrder';

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
  | { kind: 'group'; group: ItemGroup<ConversationSummary>; scope?: string };

type FolderDrop = { key: string; edge: 'before' | 'after' };
type FolderDrag = {
  key: string; scope: string; source: SidebarProps['activeSource'];
  pointerId: number; x: number; y: number; moved: boolean;
};

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
  onOpenPrismProject?: (path: string, mode?: PrismOpenMode) => void;
  onReadConversationInPrism?: (conversationId: string) => void;
  onHandoff?: (conv: ConversationSummary) => void;
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
  onOpenPrismProject,
  onReadConversationInPrism,
  onHandoff,
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
  const [folderOrders, setFolderOrders] = useState(readFolderOrders);
  const dragRef = useRef<FolderDrag | null>(null);
  const suppressFolderClick = useRef(false);
  const [draggingFolder, setDraggingFolder] = useState<string | null>(null);
  const [folderDrop, setFolderDrop] = useState<FolderDrop | null>(null);
  const treeRef = useRef<HTMLDivElement>(null);
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
    const ordered = (groups: ItemGroup<ConversationSummary>[], scope: string): TreeRow[] =>
      sortFolders(groups, folderOrders[`${activeSource}:${scope}`])
        .map((group) => ({ kind: 'group', group, scope }));
    if (pinnedConversations.length > 0) {
      rows.push({ kind: 'group', group: {
        key: PINNED_KEY, name: t.sidebar.pinned, items: pinnedConversations,
      } });
    }
    if (pinnedFolderGroups.length > 0) {
      rows.push({ kind: 'section', key: 'section:pinned-folders', label: t.sidebar.pinnedFolders,
        tooltip: t.sidebar.pinnedFolders, count: pinnedFolderGroups.length });
      rows.push(...ordered(pinnedFolderGroups, 'pinned-folders'));
    }
    const ordinary = (group: ItemGroup<ConversationSummary>) =>
      !group.path || !pinnedFolderTimes.has(folderPinKey(group.path));
    if (!codexSections) {
      return [...rows, ...ordered(folderGroups.filter(ordinary), 'folders')];
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
      rows.push(...ordered(visible, 'visible'));
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
        rows.push(...ordered(hidden, 'hidden'));
      }
    }
    return rows;
  }, [pinnedConversations, pinnedFolderGroups, pinnedFolderTimes, codexSections, folderGroups, showHidden, t.sidebar, folderOrders, activeSource]);

  const cancelFolderDrag = () => {
    dragRef.current = null;
    setDraggingFolder(null);
    setFolderDrop(null);
  };

  // Pointer dragging works in the Windows WebView without disabling native file drops.
  useEffect(() => {
    const cancelOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') cancelFolderDrag();
    };
    window.addEventListener('keydown', cancelOnEscape);
    window.addEventListener('blur', cancelFolderDrag);
    return () => {
      window.removeEventListener('keydown', cancelOnEscape);
      window.removeEventListener('blur', cancelFolderDrag);
    };
  }, []);
  useEffect(cancelFolderDrag, [activeSource, searchQuery]);

  const findFolderDrop = (x: number, y: number, drag: FolderDrag): FolderDrop | null => {
    const element = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-folder-sort-key]');
    if (!element || !treeRef.current?.contains(element) ||
        element.dataset.folderSortScope !== drag.scope ||
        element.dataset.folderSortKey === drag.key) return null;
    const header = element.querySelector<HTMLElement>('[data-folder-header]');
    if (!header) return null;
    const rect = header.getBoundingClientRect();
    return { key: element.dataset.folderSortKey!, edge: y < rect.top + rect.height / 2 ? 'before' : 'after' };
  };

  const saveFolderMove = (scope: string, key: string, drop: FolderDrop) => {
    const visible = treeRows.flatMap((row) =>
      row.kind === 'group' && row.scope === scope && row.group.path ? [row.group.key] : []);
    if (!visible.includes(key) || !visible.includes(drop.key)) return;
    const storageScope = `${activeSource}:${scope}`;
    const next = { ...folderOrders, [storageScope]: moveFolder(
      folderOrders[storageScope] ?? [], visible, key, drop.key, drop.edge
    ) };
    setFolderOrders(next);
    try {
      localStorage.setItem(FOLDER_ORDER_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // Keep sorting usable for this session when browser storage is unavailable.
    }
  };

  const handleFolderPointerMove = (event: React.PointerEvent) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) return;
    if (!drag.moved && Math.hypot(event.clientX - drag.x, event.clientY - drag.y) < 5) return;
    drag.moved = true;
    suppressFolderClick.current = true;
    event.preventDefault();
    setDraggingFolder(drag.key);
    const tree = treeRef.current;
    if (tree) {
      const rect = tree.getBoundingClientRect();
      if (event.clientY < rect.top + 28) tree.scrollTop -= 16;
      else if (event.clientY > rect.bottom - 28) tree.scrollTop += 16;
    }
    setFolderDrop(findFolderDrop(event.clientX, event.clientY, drag));
  };

  const handleFolderPointerUp = (event: React.PointerEvent) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) return;
    const drop = drag.moved && drag.source === activeSource
      ? findFolderDrop(event.clientX, event.clientY, drag) : null;
    if (drop) saveFolderMove(drag.scope, drag.key, drop);
    cancelFolderDrag();
  };

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
  // Unchecked checkboxes collapse to 0px width until hovered so titles get full sidebar space.
  const checkboxClass = (checked: boolean, kind: 'folder' | 'conversation') => {
    const base =
      'rounded border-slate-300 dark:border-slate-600 h-3.5 flex-shrink-0 cursor-pointer accent-blue-600 transition-all';
    if (checked || selecting) {
      return `${base} w-3.5 opacity-100`;
    }
    return kind === 'folder'
      ? `${base} w-0 overflow-hidden opacity-0 -mr-1.5 group-hover:w-3.5 group-hover:mr-0 group-hover:opacity-100 focus:w-3.5 focus:mr-0 focus:opacity-100`
      : `${base} w-0 overflow-hidden opacity-0 -mr-2 group-hover/conversation:w-3.5 group-hover/conversation:mr-0 group-hover/conversation:opacity-100 focus:w-3.5 focus:mr-0 focus:opacity-100`;
  };

  return (
    <aside
      className="h-full bg-white/70 dark:bg-[#101726]/78 backdrop-blur-lg border-r border-slate-200/75 dark:border-slate-800/80 flex flex-col flex-shrink-0 select-none overflow-hidden transition-colors"
      style={{ width: `${width}px` }}
      onClick={closeContextMenu}
    >
      <div className="px-3 pt-3 pb-2">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400 dark:text-slate-500" size={13} />
          <input
            id="sidebar-search"
            type="search"
            aria-label={t.sidebar.searchPlaceholder}
            value={searchQuery}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={t.sidebar.searchPlaceholder}
            className="w-full bg-white/95 dark:bg-[#172135] text-slate-800 dark:text-slate-100 text-xs pl-8 pr-14 py-2 rounded-xl border border-slate-200/90 dark:border-slate-700/75 shadow-[0_1px_2px_rgba(15,23,42,0.04)] focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15 focus:outline-none placeholder-slate-400 dark:placeholder-slate-500 transition-all"
          />
          <kbd className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[10px] text-slate-400 dark:text-slate-500 font-mono px-1.5 py-px rounded-md border border-slate-200/90 dark:border-slate-700 bg-slate-50 dark:bg-slate-800/80 select-none">
            Ctrl K
          </kbd>
        </div>
      </div>

      {/* Project tree */}
      <div ref={treeRef} className="flex-1 overflow-y-auto px-2.5 pb-2">
        {treeRows.length === 0 ? (
          <div className="text-center py-16 px-4 text-slate-400 dark:text-slate-500 text-xs leading-relaxed">
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
                  <span className="font-mono text-[10.5px]">{row.count}</span>
                </>
              );
              const className =
                'w-full flex items-center justify-between px-2 pt-3 pb-1 text-[11px] font-semibold uppercase tracking-wider text-slate-400 dark:text-slate-500';
              return row.collapsible ? (
                <button
                  key={row.key}
                  type="button"
                  onClick={toggleHiddenExpanded}
                  aria-expanded={showHidden}
                  title={row.tooltip}
                  className={`${className} hover:text-slate-700 dark:hover:text-slate-300 transition-colors`}
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
                data-folder-sort-key={folder ? group.key : undefined}
                data-folder-sort-scope={folder ? row.scope : undefined}
                className={`relative pt-1 ${draggingFolder === group.key ? 'opacity-40' : ''}`}
              >
                {folderDrop?.key === group.key && (
                  <div
                    data-folder-drop-indicator
                    className={`absolute left-2 right-1 h-0.5 rounded bg-blue-600 dark:bg-sky-400 pointer-events-none z-10 ${
                      folderDrop.edge === 'before' ? 'top-0' : 'bottom-0'
                    }`}
                  />
                )}
                {/* Project Header Row */}
                <div
                  data-folder-header
                  onContextMenu={(e) => {
                    if (!folder) return;
                    e.preventDefault();
                    setContextMenu({ kind: 'folder', x: e.clientX, y: e.clientY, path: folder });
                  }}
                  className="group flex items-center gap-1.5 pl-1.5 pr-1.5 py-1.5 rounded-lg hover:bg-slate-200/55 dark:hover:bg-white/[0.05] transition-colors"
                >
                  <input
                    type="checkbox"
                    checked={allChecked}
                    ref={(el) => {
                      if (el) el.indeterminate = someChecked;
                    }}
                    onChange={() => onToggleProjectPublishSelection(convIds)}
                    className={checkboxClass(allChecked || someChecked, 'folder')}
                  />

                  <button
                    type="button"
                    aria-expanded={!isCollapsed}
                    title={folder ? t.sidebar.reorderFolder : undefined}
                    style={folder ? { touchAction: 'none' } : undefined}
                    onPointerDown={(event) => {
                      suppressFolderClick.current = false;
                      if (!folder || !row.scope || event.button !== 0 || event.isPrimary === false) return;
                      closeContextMenu();
                      dragRef.current = {
                        key: group.key,
                        scope: row.scope,
                        source: activeSource,
                        pointerId: event.pointerId,
                        x: event.clientX,
                        y: event.clientY,
                        moved: false,
                      };
                      event.currentTarget.setPointerCapture?.(event.pointerId);
                    }}
                    onPointerMove={handleFolderPointerMove}
                    onPointerUp={handleFolderPointerUp}
                    onPointerCancel={cancelFolderDrag}
                    onLostPointerCapture={cancelFolderDrag}
                    onDragStart={(event) => event.preventDefault()}
                    onKeyDown={(event) => {
                      if (!folder || !row.scope || !event.altKey || !['ArrowUp', 'ArrowDown'].includes(event.key)) return;
                      event.preventDefault();
                      const siblings = treeRows.filter((item) => item.kind === 'group' && item.scope === row.scope && item.group.path);
                      const index = siblings.findIndex((item) => item.kind === 'group' && item.group.key === group.key);
                      const neighbor = siblings[index + (event.key === 'ArrowUp' ? -1 : 1)];
                      if (neighbor?.kind === 'group') {
                        saveFolderMove(row.scope, group.key, {
                          key: neighbor.group.key,
                          edge: event.key === 'ArrowUp' ? 'before' : 'after',
                        });
                      }
                    }}
                    onClick={(event) => {
                      if (suppressFolderClick.current && event.detail !== 0) {
                        suppressFolderClick.current = false;
                        return;
                      }
                      toggleCollapse(group.key);
                    }}
                    className={`flex items-center gap-1.5 text-left min-w-0 flex-1 text-[12.5px] font-semibold text-slate-800 dark:text-slate-100 ${
                      folder ? 'cursor-grab active:cursor-grabbing' : ''
                    }`}
                  >
                    {folder && (
                      <GripVertical
                        size={11}
                        aria-hidden="true"
                        className="text-slate-400 flex-shrink-0 w-0 overflow-hidden opacity-0 group-hover:w-3 group-hover:opacity-80 transition-all"
                      />
                    )}
                    {isCollapsed ? (
                      <ChevronRight size={13} className="text-slate-400 dark:text-slate-500 flex-shrink-0" />
                    ) : (
                      <ChevronDown size={13} className="text-slate-400 dark:text-slate-500 flex-shrink-0" />
                    )}
                    {group.key === PINNED_KEY ? (
                      <Pin size={13} className="text-blue-600 dark:text-sky-400 flex-shrink-0" />
                    ) : group.key === CHATS_KEY ? (
                      <MessageSquare size={13} className="text-slate-400 flex-shrink-0" />
                    ) : isCollapsed ? (
                      <Folder size={13.5} className="text-amber-500/80 dark:text-amber-400/75 flex-shrink-0" />
                    ) : (
                      <FolderOpen size={13.5} className="text-amber-500 dark:text-amber-400 flex-shrink-0" />
                    )}
                    <span className="truncate tracking-tight" title={folder ?? projectName}>
                      {projectName}
                    </span>
                  </button>

                  {/* Folder shortcuts collapse to 0px width until hovered so folder names never truncate prematurely */}
                  {folder && (
                    <div className="flex items-center flex-shrink-0">
                      {isFolderPinned && (
                        <button
                          type="button"
                          title={t.sidebar.unpinFolder}
                          aria-pressed={true}
                          onClick={() => onSetFolderPinned(folder, false)}
                          className="p-1 rounded-md hover:bg-slate-200/80 dark:hover:bg-white/[0.1] text-blue-600 dark:text-sky-400 transition-colors"
                        >
                          <Pin size={12} />
                        </button>
                      )}
                      <div className="flex items-center w-0 overflow-hidden opacity-0 group-hover:w-auto group-hover:opacity-100 focus-within:w-auto focus-within:opacity-100 transition-opacity">
                        {!isFolderPinned && (
                          <button
                            type="button"
                            title={t.sidebar.pinFolder}
                            aria-pressed={false}
                            onClick={() => onSetFolderPinned(folder, true)}
                            className="p-1 rounded-md hover:bg-slate-200/80 dark:hover:bg-white/[0.1] text-slate-400 hover:text-blue-600 dark:hover:text-sky-400 transition-colors"
                          >
                            <Pin size={12} />
                          </button>
                        )}
                        {onOpenPrismProject && (
                          <button
                            type="button"
                            onClick={() => onOpenPrismProject(folder, 'new-workspace')}
                            title={t.sidebar.projectOpenPrism}
                            className="p-1 rounded-md hover:bg-slate-200/80 dark:hover:bg-white/[0.1] text-slate-400 hover:text-indigo-600 dark:hover:text-indigo-400 transition-colors"
                          >
                            <Sparkles size={12} />
                          </button>
                        )}
                        <button
                          type="button"
                          onClick={() => onOpenFolder(folder)}
                          title={t.sidebar.projectOpenFolder}
                          className="p-1 rounded-md hover:bg-slate-200/80 dark:hover:bg-white/[0.1] text-slate-400 hover:text-slate-800 dark:hover:text-white transition-colors"
                        >
                          <Folder size={12} />
                        </button>
                        <button
                          type="button"
                          onClick={() => onOpenVSCode(folder)}
                          title={t.sidebar.projectOpenVSCode}
                          className="p-1 rounded-md hover:bg-slate-200/80 dark:hover:bg-white/[0.1] text-slate-400 hover:text-blue-600 dark:hover:text-sky-400 transition-colors"
                        >
                          <Code2 size={12} />
                        </button>
                      </div>
                    </div>
                  )}
                  <span className="text-[10.5px] text-slate-500 dark:text-slate-400 font-mono flex-shrink-0 px-1.5 py-px rounded-md bg-slate-200/65 dark:bg-slate-800/85 min-w-[18px] text-center">
                    {items.length}
                  </span>
                </div>

                {/* Conversation Children */}
                {!isCollapsed && (
                  <div className="space-y-0.5 mt-0.5">
                    {isFolderPinned && items.length === 0 && (
                      <p className="pl-8 pr-2 py-1.5 text-[11px] text-slate-400">{t.sidebar.folderConversationsPinned}</p>
                    )}
                    {items.map((conv) => {
                      const isSelected = selectedId === conv.id;
                      const isChecked = publishSelection.has(conv.id);

                      return (
                        <div
                          key={conv.id}
                          onContextMenu={(e) => handleContextMenu(e, conv)}
                          onClick={() => onSelectConversation(conv.id)}
                          className={`group/conversation relative flex items-center gap-2 pl-2.5 pr-1.5 py-[7px] ml-3.5 rounded-lg cursor-pointer transition-all duration-150 text-[12.5px] ${
                            isSelected
                              ? 'bg-blue-50/95 dark:bg-blue-500/[0.16] text-blue-700 dark:text-sky-300 font-medium ring-1 ring-inset ring-blue-200/85 dark:ring-blue-400/25 shadow-[0_1px_2px_rgba(37,99,235,0.06)]'
                              : 'text-slate-600 dark:text-slate-300 hover:bg-slate-200/55 dark:hover:bg-white/[0.05] hover:text-slate-900 dark:hover:text-white'
                          }`}
                        >
                          {isSelected && (
                            <span
                              aria-hidden="true"
                              className="absolute left-0 top-1.5 bottom-1.5 w-[3px] rounded-r-full bg-blue-600 dark:bg-sky-400"
                            />
                          )}
                          <input
                            type="checkbox"
                            checked={isChecked}
                            onClick={(e) => e.stopPropagation()}
                            onChange={() => onTogglePublishSelection(conv.id)}
                            className={checkboxClass(isChecked, 'conversation')}
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
                            className={`rounded-md hover:bg-slate-200/80 dark:hover:bg-white/[0.1] transition-all flex-shrink-0 ${
                              conv.pinned_at
                                ? 'p-0.5 text-blue-600 dark:text-sky-400 opacity-100'
                                : 'w-0 overflow-hidden opacity-0 p-0 group-hover/conversation:w-auto group-hover/conversation:p-0.5 group-hover/conversation:opacity-100 focus:w-auto focus:p-0.5 focus:opacity-100 text-slate-400 hover:text-blue-600 dark:hover:text-sky-400'
                            }`}
                          >
                            <Pin size={12} />
                          </button>
                          <span
                            className={`text-[10.5px] flex-shrink-0 font-mono font-normal min-w-[16px] text-right ${
                              isSelected ? 'text-blue-600/80 dark:text-sky-300/80' : 'text-slate-400 dark:text-slate-500'
                            }`}
                          >
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
        <div className="m-2.5 mt-0 pl-3 pr-1.5 py-2 rounded-xl bg-slate-900 dark:bg-[#1e293b] border border-slate-800 dark:border-slate-700 text-white flex items-center gap-1.5 text-xs shadow-lg animate-in fade-in slide-in-from-bottom-1 duration-150">
          <span className="flex-1 font-medium truncate">
            {interpolate(t.sidebar.selectedCount, { count: publishSelection.size })}
          </span>
          <button
            type="button"
            onClick={() => onBatchExportMarkdown(Array.from(publishSelection))}
            title={t.sidebar.batchExportMdTooltip}
            className="flex items-center gap-1 px-2 py-1 rounded-lg border border-white/15 hover:bg-white/10 transition-colors"
          >
            <FileDown size={12} />
            {t.sidebar.batchExportMd}
          </button>
          <button
            type="button"
            onClick={() => onBatchDelete(Array.from(publishSelection))}
            title={t.sidebar.batchDeleteTooltip}
            className="flex items-center gap-1 px-2 py-1 rounded-lg border border-rose-400/30 text-rose-300 hover:bg-rose-500/25 transition-colors"
          >
            <Trash2 size={12} />
            {t.common.delete}
          </button>
          <button
            type="button"
            onClick={onClearSelection}
            title={t.sidebar.clearSelectionTooltip}
            aria-label={t.sidebar.clearSelectionTooltip}
            className="p-1 rounded-lg text-white/70 hover:text-white hover:bg-white/10 transition-colors"
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
            className="fixed z-50 bg-white dark:bg-[#1a2336] border border-slate-200 dark:border-slate-700 rounded-2xl shadow-[0_16px_40px_rgba(15,23,42,0.18)] dark:shadow-[0_16px_40px_rgba(0,0,0,0.6)] p-1.5 text-[12.5px] text-slate-800 dark:text-slate-100 min-w-[180px] animate-in fade-in zoom-in-95 duration-150"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            {contextMenu.kind === 'folder' ? (
              <>
                {onOpenPrismProject && (
                  <>
                    <button
                      role="menuitem"
                      onClick={() => {
                        onOpenPrismProject(contextMenu.path, 'new-workspace');
                        closeContextMenu();
                      }}
                      className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors font-medium text-blue-600 dark:text-sky-400"
                    >
                      <Sparkles size={12} />
                      {t.sidebar.menuOpenPrismNew}
                    </button>
                    <button
                      role="menuitem"
                      onClick={() => {
                        onOpenPrismProject(contextMenu.path, 'parallel-root');
                        closeContextMenu();
                      }}
                      className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors"
                    >
                      <FolderPlus size={12} className="text-slate-400" />
                      {t.sidebar.menuOpenPrismParallel}
                    </button>
                  </>
                )}
                <button
                  role="menuitem"
                  onClick={() => {
                    onOpenFolder(contextMenu.path);
                    closeContextMenu();
                  }}
                  className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors"
                >
                  <Folder size={12} className="text-slate-400" />
                  {t.sidebar.menuOpenFolder}
                </button>
                <button
                  role="menuitem"
                  onClick={() => {
                    onOpenVSCode(contextMenu.path);
                    closeContextMenu();
                  }}
                  className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors"
                >
                  <Code2 size={12} className="text-slate-400" />
                  {t.sidebar.menuOpenVSCode}
                </button>
                <button
                  role="menuitem"
                  onClick={() => {
                    onSetFolderPinned(contextMenu.path, !pinnedFolderTimes.has(folderPinKey(contextMenu.path)));
                    closeContextMenu();
                  }}
                  className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors"
                >
                  {pinnedFolderTimes.has(folderPinKey(contextMenu.path)) ? <PinOff size={12} /> : <Pin size={12} />}
                  {pinnedFolderTimes.has(folderPinKey(contextMenu.path)) ? t.sidebar.unpinFolder : t.sidebar.pinFolder}
                </button>
              </>
            ) : (
              <>
                {onHandoff && (
                  <button
                    role="menuitem"
                    onClick={() => {
                      onHandoff(contextMenu.conv);
                      closeContextMenu();
                    }}
                    className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors font-medium text-blue-600 dark:text-sky-400"
                  >
                    <ArrowRightLeft size={12} />
                    {t.sidebar.menuHandoff}
                  </button>
                )}
                {onReadConversationInPrism && (
                  <button
                    role="menuitem"
                    onClick={() => {
                      onReadConversationInPrism(contextMenu.conv.id);
                      closeContextMenu();
                    }}
                    className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors font-medium text-blue-600 dark:text-sky-400"
                  >
                    <BookOpen size={12} />
                    {t.sidebar.menuReadInPrism}
                  </button>
                )}
                <button
                  role="menuitem"
                  onClick={() => {
                    onSetPinned(contextMenu.conv.id, !contextMenu.conv.pinned_at);
                    closeContextMenu();
                  }}
                  className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors"
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
                    className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors font-medium text-blue-600 dark:text-sky-400"
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
                    className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors"
                  >
                    <FileDown size={12} className="text-slate-400" />
                    {t.sidebar.menuExportMd}
                  </button>
                )}
                <button
                  onClick={() => {
                    onRename(contextMenu.conv.id, contextMenu.conv.title);
                    closeContextMenu();
                  }}
                  className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-slate-100 dark:hover:bg-slate-800/80 text-left transition-colors"
                >
                  <Edit2 size={12} className="text-slate-400" />
                  {t.sidebar.menuRename}
                </button>
                <button
                  onClick={() => {
                    onDelete(contextMenu.conv.id, contextMenu.conv.title);
                    closeContextMenu();
                  }}
                  className="w-full flex items-center gap-2 px-3 py-1.5 rounded-xl hover:bg-rose-50 dark:hover:bg-rose-500/15 text-rose-600 dark:text-rose-400 text-left transition-colors"
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
