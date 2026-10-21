import React, { useState, useMemo } from 'react';
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Folder,
  RefreshCw,
  Search,
} from 'lucide-react';
import { CodexRecoveryReport, CodexThreadCandidate } from '../types';
import { extractProjectName, formatDateTime } from '../utils/formatters';
import { ItemGroup, groupByFolder, splitByCodexScope } from '../utils/codexScope';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { useI18n } from '../i18n';

type CandidateRow =
  | {
      kind: 'section';
      key: string;
      label: string;
      tooltip: string;
      count: number;
      /** Expands and collapses the groups after it. */
      collapsible?: boolean;
    }
  | { kind: 'group'; group: ItemGroup<CodexThreadCandidate> };

interface CodexRecoveryModalProps {
  isOpen: boolean;
  onClose: () => void;
  report: CodexRecoveryReport | null;
  onConfirmRecovery: (candidates: CodexThreadCandidate[]) => Promise<void>;
  isRecovering: boolean;
}

export const CodexRecoveryModal: React.FC<CodexRecoveryModalProps> = ({
  isOpen,
  onClose,
  report,
  onConfirmRecovery,
  isRecovering,
}) => {
  const { t, interpolate } = useI18n();
  const [filterText, setFilterText] = useState('');
  const [hideExisting, setHideExisting] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(new Set());
  const [hiddenExpanded, setHiddenExpanded] = useState(false);

  // Initialize selection when report arrives
  React.useEffect(() => {
    if (report) {
      // By default select candidates that do NOT exist locally yet and that Codex lists.
      const initial = new Set(
        report.candidates
          .filter((c) => !c.exists_locally && c.codex_scope !== 'hidden')
          .map((c) => c.thread_id)
      );
      setSelectedIds(initial);
      setHiddenExpanded(false);
    }
  }, [report]);

  const filteredCandidates = useMemo(() => {
    if (!report) return [];
    let list = report.candidates;
    if (hideExisting) {
      list = list.filter((c) => !c.exists_locally);
    }
    const q = filterText.trim().toLowerCase();
    if (q) {
      list = list.filter(
        (c) =>
          c.title.toLowerCase().includes(q) ||
          (c.project_path && c.project_path.toLowerCase().includes(q))
      );
    }
    return list;
  }, [report, hideExisting, filterText]);

  // Group the way the Codex sidebar lists threads, or by folder without project state.
  const showHidden = hiddenExpanded || filterText.trim() !== '';
  const rows = useMemo<CandidateRow[]>(() => {
    const uncategorized = t.common.uncategorized;
    const folderOf = (c: CodexThreadCandidate) => c.project_path;
    const sidebarProjects = report?.sidebar_projects;
    if (!sidebarProjects) {
      return groupByFolder(filteredCandidates, folderOf, uncategorized).map((group) => ({
        kind: 'group',
        group,
      }));
    }
    const sections = splitByCodexScope(
      filteredCandidates,
      (c) => (c.codex_scope ? { scope: c.codex_scope, project_id: c.codex_project_id } : undefined),
      sidebarProjects,
      folderOf,
      uncategorized
    );
    const visible = [...sections.projects];
    if (sections.chats.length > 0) {
      visible.push({ key: 'chats', name: t.sidebar.codexChats, items: sections.chats });
    }
    const result: CandidateRow[] = [];
    if (visible.length > 0) {
      result.push({
        kind: 'section',
        key: 'section:visible',
        label: t.sidebar.codexVisible,
        tooltip: t.sidebar.codexVisibleTooltip,
        count: visible.reduce((n, g) => n + g.items.length, 0),
      });
      result.push(...visible.map((group): CandidateRow => ({ kind: 'group', group })));
    }
    if (sections.hiddenCount > 0) {
      result.push({
        kind: 'section',
        key: 'section:hidden',
        label: t.sidebar.codexHidden,
        tooltip: t.sidebar.codexHiddenTooltip,
        count: sections.hiddenCount,
        collapsible: true,
      });
      if (showHidden) {
        result.push(...sections.hidden.map((group): CandidateRow => ({ kind: 'group', group })));
      }
    }
    return result;
  }, [report, filteredCandidates, showHidden, t.common.uncategorized, t.sidebar]);

  const toggleSelect = (threadId: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(threadId)) next.delete(threadId);
      else next.add(threadId);
      return next;
    });
  };

  const toggleGroup = (candidates: CodexThreadCandidate[]) => {
    const ids = candidates.map((c) => c.thread_id);
    const allChecked = ids.every((id) => selectedIds.has(id));
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (allChecked) {
        ids.forEach((id) => next.delete(id));
      } else {
        ids.forEach((id) => next.add(id));
      }
      return next;
    });
  };

  const selectAll = () => {
    setSelectedIds(new Set(filteredCandidates.map((c) => c.thread_id)));
  };

  const selectNone = () => {
    setSelectedIds(new Set());
  };

  const handleConfirm = async () => {
    if (!report) return;
    const selectedList = report.candidates.filter((c) => selectedIds.has(c.thread_id));
    await onConfirmRecovery(selectedList);
  };

  if (!isOpen || !report) return null;

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={t.modals.codexRecoveryTitle}
      subtitle={interpolate(t.modals.codexRecoverySubtitle, {
        count: report.candidates.length,
        indices: report.index_count,
      })}
      widthClass="max-w-3xl"
    >
      <div className="flex flex-col gap-4 text-xs">
        {/* Filters and Toolbar */}
        <div className="flex items-center justify-between gap-3 bg-black/[0.02] dark:bg-white/[0.03] p-2.5 rounded-2xl border border-black/[0.06] dark:border-white/[0.08]">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-2.5 text-[#86868b]" size={13} />
            <input
              type="text"
              value={filterText}
              onChange={(e) => setFilterText(e.target.value)}
              placeholder={t.modals.filterPlaceholder}
              className="w-full bg-black/[0.04] dark:bg-white/[0.06] text-[#1d1d1f] dark:text-[#ededf0] text-xs pl-8 pr-3 py-1.5 rounded-full border border-black/[0.08] dark:border-white/[0.1] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]"
            />
          </div>

          <label className="flex items-center gap-2 cursor-pointer text-[#6e6e73] dark:text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#ededf0] select-none text-[11px] transition-colors">
            <input
              type="checkbox"
              checked={hideExisting}
              onChange={(e) => setHideExisting(e.target.checked)}
              className="rounded bg-black/[0.05] dark:bg-white/[0.08] border-black/20 dark:border-white/20 text-[#0071e3] w-3.5 h-3.5 cursor-pointer accent-[#0071e3]"
            />
            {t.modals.hideExisting}
          </label>

          <div className="flex items-center gap-1.5">
            <Button variant="ghost" size="sm" onClick={selectAll}>
              {t.modals.selectAll}
            </Button>
            <Button variant="ghost" size="sm" onClick={selectNone}>
              {t.modals.clearAll}
            </Button>
          </div>
        </div>

        {/* Tree List */}
        <div className="max-h-[380px] overflow-y-auto border border-black/[0.08] dark:border-white/[0.08] rounded-2xl bg-black/[0.01] dark:bg-white/[0.02] p-2 space-y-2">
          {rows.length === 0 ? (
            <div className="text-center py-12 text-[#86868b]">
              {t.modals.noCandidates}
            </div>
          ) : (
            rows.map((row) => {
              if (row.kind === 'section') {
                const content = (
                  <>
                    <span className="flex items-center gap-1">
                      {row.collapsible &&
                        (showHidden ? <ChevronDown size={12} /> : <ChevronRight size={12} />)}
                      {row.label}
                    </span>
                    <span className="font-mono">{row.count}</span>
                  </>
                );
                const className =
                  'w-full flex items-center justify-between px-1.5 pt-1 text-[10px] font-semibold uppercase tracking-wider text-[#86868b]';
                return row.collapsible ? (
                  <button
                    key={row.key}
                    type="button"
                    onClick={() => setHiddenExpanded((prev) => !prev)}
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
              const projectPath = group.key;
              const items = group.items;
              const projectName = group.name ?? extractProjectName(group.path ?? '');
              const isCollapsed = collapsedProjects.has(projectPath);
              const allChecked = items.every((c) => selectedIds.has(c.thread_id));
              const someChecked = items.some((c) => selectedIds.has(c.thread_id)) && !allChecked;

              return (
                <div
                  key={projectPath}
                  className="rounded-xl border border-black/[0.06] dark:border-white/[0.06] overflow-hidden bg-black/[0.01] dark:bg-white/[0.02]"
                >
                  {/* Group Header */}
                  <div className="flex items-center justify-between px-3 py-2 bg-black/[0.03] dark:bg-white/[0.04] border-b border-black/[0.04] dark:border-white/[0.04] text-xs">
                    <div className="flex items-center gap-2">
                      <input
                        type="checkbox"
                        checked={allChecked}
                        ref={(el) => {
                          if (el) el.indeterminate = someChecked;
                        }}
                        onChange={() => toggleGroup(items)}
                        className="rounded bg-black/[0.05] dark:bg-white/[0.08] border-black/20 dark:border-white/20 text-[#0071e3] w-3.5 h-3.5 cursor-pointer accent-[#0071e3]"
                      />
                      <button
                        onClick={() => {
                          setCollapsedProjects((prev) => {
                            const next = new Set(prev);
                            if (next.has(projectPath)) next.delete(projectPath);
                            else next.add(projectPath);
                            return next;
                          });
                        }}
                        className="flex items-center gap-1.5 font-semibold text-[#1d1d1f] dark:text-[#f5f5f7]"
                      >
                        {isCollapsed ? (
                          <ChevronRight size={13} className="text-[#86868b]" />
                        ) : (
                          <ChevronDown size={13} className="text-[#6e6e73] dark:text-[#a1a1a6]" />
                        )}
                        <Folder size={13} className="text-[#2997ff]" />
                        <span title={group.path}>{projectName}</span>
                      </button>
                    </div>

                    <span className="text-[10px] text-[#6e6e73] dark:text-[#86868b] bg-black/[0.04] dark:bg-white/[0.06] px-2 py-0.5 rounded-full font-medium">
                      {items.length} {t.common.conversations}
                    </span>
                  </div>

                  {/* Group Items */}
                  {!isCollapsed && (
                    <div className="divide-y divide-black/[0.04] dark:divide-white/[0.04]">
                      {items.map((cand) => {
                        const isChecked = selectedIds.has(cand.thread_id);

                        return (
                          <div
                            key={cand.thread_id}
                            onClick={() => toggleSelect(cand.thread_id)}
                            className="flex items-center justify-between px-3.5 py-2 hover:bg-black/[0.03] dark:hover:bg-white/[0.04] cursor-pointer transition-colors"
                          >
                            <div className="flex items-center gap-2 min-w-0 flex-1">
                              <input
                                type="checkbox"
                                checked={isChecked}
                                onChange={() => {}}
                                className="rounded bg-black/[0.05] dark:bg-white/[0.08] border-black/20 dark:border-white/20 text-[#0071e3] w-3.5 h-3.5 flex-shrink-0 cursor-pointer accent-[#0071e3]"
                              />
                              <span
                                className="font-medium text-[#1d1d1f] dark:text-[#ededf0] truncate tracking-tight"
                                title={cand.title}
                              >
                                {cand.title}
                              </span>
                            </div>

                            <div className="flex items-center gap-2 ml-2 flex-shrink-0">
                              {cand.exists_locally ? (
                                <span className="flex items-center gap-1 text-[10px] text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 px-2 py-0.5 rounded-full border border-emerald-500/20 font-medium">
                                  <CheckCircle2 size={10} />
                                  {t.modals.badgeIncremental}
                                </span>
                              ) : (
                                <span className="text-[10px] text-[#0071e3] dark:text-[#2997ff] bg-[#0071e3]/10 px-2 py-0.5 rounded-full border border-[#0071e3]/20 font-medium">
                                  {t.modals.badgeNotImported}
                                </span>
                              )}

                              <span className="text-[10px] text-[#86868b] font-mono">
                                {formatDateTime(cand.updated_at)}
                              </span>
                            </div>
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

        {/* Footer info & Buttons */}
        <div className="flex items-center justify-between pt-3 border-t border-black/[0.08] dark:border-white/[0.08]">
          <div className="text-[11px] text-[#6e6e73] dark:text-[#86868b]">
            {interpolate(t.modals.selectedSummary, { count: selectedIds.size })}
          </div>

          <div className="flex items-center gap-2">
            <Button variant="ghost" size="sm" onClick={onClose} disabled={isRecovering}>
              {t.common.cancel}
            </Button>
            <Button
              variant="primary"
              size="sm"
              icon={isRecovering ? <RefreshCw size={13} className="animate-spin" /> : undefined}
              disabled={isRecovering || selectedIds.size === 0}
              onClick={handleConfirm}
            >
              {isRecovering ? t.modals.recovering : interpolate(t.modals.executeRecovery, { count: selectedIds.size })}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
};
