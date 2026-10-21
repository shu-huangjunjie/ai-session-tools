import React, { useState, useMemo } from 'react';
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Folder,
  Search,
  Bot,
  UploadCloud,
  CheckSquare,
  Square,
  Info,
} from 'lucide-react';
import { ClaudePublishCandidate, ClaudePublishPreview, ClaudePublishResult } from '../types';
import { extractProjectName, formatDateTime } from '../utils/formatters';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { useI18n } from '../i18n';

interface ClaudePublishModalProps {
  isOpen: boolean;
  onClose: () => void;
  preview: ClaudePublishPreview | null;
  onConfirmPublish: (selectedIds: string[], autoRestart: boolean) => Promise<ClaudePublishResult | void>;
  isPublishing: boolean;
}

export const ClaudePublishModal: React.FC<ClaudePublishModalProps> = ({
  isOpen,
  onClose,
  preview,
  onConfirmPublish,
  isPublishing,
}) => {
  const [filterText, setFilterText] = useState('');
  const [onlyNew, setOnlyNew] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(new Set());
  const [publishResult, setPublishResult] = useState<ClaudePublishResult | null>(null);
  const [autoRestart, setAutoRestart] = useState(false);
  const { t } = useI18n();
  const uncategorized = t.common.uncategorized;

  React.useEffect(() => {
    if (preview) {
      setSelectedIds(new Set(preview.candidates.map((c) => c.conversation_id)));
      setPublishResult(null);
      setAutoRestart(false);
    }
  }, [preview]);

  const filteredCandidates = useMemo(() => {
    if (!preview) return [];
    let list = preview.candidates;
    if (onlyNew) {
      list = list.filter((c) => !c.exists_in_target);
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
  }, [preview, onlyNew, filterText]);

  const grouped = useMemo(() => {
    const map = new Map<string, ClaudePublishCandidate[]>();
    for (const c of filteredCandidates) {
      const key = c.project_path || uncategorized;
      const list = map.get(key) || [];
      list.push(c);
      map.set(key, list);
    }
    return Array.from(map.entries()).sort((a, b) => {
      if (a[0] === uncategorized) return 1;
      if (b[0] === uncategorized) return -1;
      return a[0].localeCompare(b[0]);
    });
  }, [filteredCandidates, uncategorized]);

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleGroup = (candidates: ClaudePublishCandidate[]) => {
    const ids = candidates.map((c) => c.conversation_id);
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

  const toggleAll = () => {
    if (!preview) return;
    if (selectedIds.size === preview.candidates.length) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(preview.candidates.map((c) => c.conversation_id)));
    }
  };

  const toggleCollapse = (key: string) => {
    setCollapsedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const handleExecute = async () => {
    if (selectedIds.size === 0) return;
    const res = await onConfirmPublish(Array.from(selectedIds), autoRestart);
    if (res) {
      setPublishResult(res);
    }
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="发布 / 同步会话至 Claude"
      widthClass="max-w-4xl"
    >
      <div className="flex flex-col gap-4 max-h-[75vh] select-none text-xs">
        {/* Banner */}
        <div className="flex items-start gap-3 p-3.5 rounded-2xl bg-amber-500/10 border border-amber-500/20 text-[#1d1d1f] dark:text-[#ededf0]">
          <div className="p-2 rounded-xl bg-amber-500/20 text-amber-600 dark:text-amber-400 mt-0.5">
            <Bot size={18} />
          </div>
          <div className="flex-1">
            <h4 className="font-semibold text-amber-600 dark:text-amber-400 text-sm">
              Claude 本地会话与项目双向同步
            </h4>
            <p className="text-[#86868b] dark:text-[#a1a1a6] mt-1 leading-relaxed">
              选中的会话将被规范化写出至 Claude 本地工程目录（<code className="px-1 py-0.5 rounded bg-black/5 dark:bg-white/10 font-mono text-[11px]">{preview?.target_claude_home || '~/.claude'}</code>），并自动生成会话 JSONL 与更新历史检索索引。
            </p>
            {preview?.is_claude_running && (
              <div className="flex items-center gap-1.5 text-amber-600 dark:text-amber-400 font-medium text-[11px] mt-2">
                <Info size={13} />
                <span>检测到 Claude 客户端/进程正在运行，同步完成后会自动完成数据写入与刷新。</span>
              </div>
            )}
          </div>
        </div>

        {/* Result Message Banner */}
        {publishResult && (
          <div className="p-3.5 rounded-2xl bg-emerald-500/10 border border-emerald-500/20 text-emerald-700 dark:text-emerald-300 animate-in fade-in duration-200">
            <div className="flex items-center gap-2 font-semibold">
              <CheckCircle2 size={16} />
              <span>同步完成！成功同步 {publishResult.published_count} 个会话</span>
            </div>
            <div className="text-[11px] text-[#86868b] mt-1 space-y-0.5">
              <div>• 全新写出会话：{publishResult.new_count} 个</div>
              <div>• 增量更新会话：{publishResult.updated_count} 个</div>
              {publishResult.backup_path && (
                <div>• 历史备份文件：{publishResult.backup_path}</div>
              )}
            </div>
            {publishResult.warnings.length > 0 && (
              <div className="mt-2 pt-2 border-t border-emerald-500/15 text-[11px] text-amber-600 dark:text-amber-400">
                {publishResult.warnings.map((w, i) => (
                  <div key={i}>⚠️ {w}</div>
                ))}
              </div>
            )}
          </div>
        )}

        {/* Filter and Selection Controls */}
        <div className="flex items-center justify-between gap-3 bg-black/[0.02] dark:bg-white/[0.02] p-2.5 rounded-2xl border border-black/[0.06] dark:border-white/[0.06]">
          <div className="relative flex-1">
            <Search className="absolute left-3 top-2 text-[#86868b]" size={13} />
            <input
              type="text"
              value={filterText}
              onChange={(e) => setFilterText(e.target.value)}
              placeholder="过滤标题或关联项目..."
              className="w-full bg-white dark:bg-black/40 text-xs pl-8 pr-3 py-1.5 rounded-xl border border-black/[0.08] dark:border-white/[0.1] focus:outline-none focus:border-[#d97706]"
            />
          </div>

          <div className="flex items-center gap-3">
            <label className="flex items-center gap-1.5 cursor-pointer text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#ededf0] transition-colors">
              <input
                type="checkbox"
                checked={onlyNew}
                onChange={(e) => setOnlyNew(e.target.checked)}
                className="rounded accent-[#d97706]"
              />
              <span>只看目标尚无会话 ({preview?.new_count || 0})</span>
            </label>

            <Button
              variant="secondary"
              size="sm"
              onClick={toggleAll}
              className="text-xs"
            >
              {selectedIds.size === (preview?.candidates.length || 0) ? '取消全选' : '全选所有'}
            </Button>
          </div>
        </div>

        {/* List of candidates grouped by project */}
        <div className="flex-1 overflow-y-auto space-y-3 pr-1 max-h-[420px]">
          {grouped.length === 0 ? (
            <div className="py-12 text-center text-[#86868b]">
              没有符合条件的会话候选
            </div>
          ) : (
            grouped.map(([projectPath, items]) => {
              const isCollapsed = collapsedProjects.has(projectPath);
              const projectName = extractProjectName(projectPath);
              const allGroupChecked = items.every((i) => selectedIds.has(i.conversation_id));
              const someGroupChecked = items.some((i) => selectedIds.has(i.conversation_id)) && !allGroupChecked;

              return (
                <div
                  key={projectPath}
                  className="rounded-2xl border border-black/[0.06] dark:border-white/[0.06] bg-white dark:bg-white/[0.02] overflow-hidden"
                >
                  {/* Group Header */}
                  <div className="flex items-center justify-between p-2.5 bg-black/[0.02] dark:bg-white/[0.02] border-b border-black/[0.04] dark:border-white/[0.04]">
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => toggleCollapse(projectPath)}
                        className="text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white"
                      >
                        {isCollapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
                      </button>
                      <button
                        onClick={() => toggleGroup(items)}
                        className="flex items-center gap-1.5 font-medium text-xs text-[#1d1d1f] dark:text-[#ededf0]"
                      >
                        <div className="text-[#d97706]">
                          {allGroupChecked ? (
                            <CheckSquare size={14} />
                          ) : someGroupChecked ? (
                            <div className="w-3.5 h-3.5 rounded bg-[#d97706]/20 border border-[#d97706] flex items-center justify-center">
                              <div className="w-2 h-0.5 bg-[#d97706]" />
                            </div>
                          ) : (
                            <Square size={14} className="text-[#86868b]" />
                          )}
                        </div>
                        <Folder size={13} className="text-[#d97706]" />
                        <span>{projectName}</span>
                        <span className="text-[10px] text-[#86868b] font-mono">
                          ({items.length} 项)
                        </span>
                      </button>
                    </div>

                    <span className="text-[11px] text-[#86868b] truncate max-w-xs font-mono">
                      {projectPath}
                    </span>
                  </div>

                  {/* Group Items */}
                  {!isCollapsed && (
                    <div className="divide-y divide-black/[0.03] dark:divide-white/[0.03]">
                      {items.map((candidate) => {
                        const isChecked = selectedIds.has(candidate.conversation_id);
                        return (
                          <div
                            key={candidate.conversation_id}
                            onClick={() => toggleSelect(candidate.conversation_id)}
                            className={`flex items-center justify-between p-2.5 cursor-pointer transition-colors ${
                              isChecked
                                ? 'bg-amber-500/[0.04] dark:bg-amber-500/[0.06]'
                                : 'hover:bg-black/[0.02] dark:hover:bg-white/[0.02]'
                            }`}
                          >
                            <div className="flex items-center gap-2.5 flex-1 min-w-0 pr-4">
                              <div className="text-[#d97706]">
                                {isChecked ? <CheckSquare size={14} /> : <Square size={14} className="text-[#86868b]" />}
                              </div>
                              <div className="min-w-0 flex-1">
                                <div className="flex items-center gap-2">
                                  <span className="font-medium text-xs text-[#1d1d1f] dark:text-[#ededf0] truncate">
                                    {candidate.title}
                                  </span>
                                  {candidate.exists_in_target ? (
                                    <span className="px-1.5 py-0.2 rounded-full bg-blue-500/10 text-blue-600 dark:text-blue-400 text-[9px] font-medium border border-blue-500/20">
                                      目标已存在 (增量写出)
                                    </span>
                                  ) : (
                                    <span className="px-1.5 py-0.2 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 text-[9px] font-medium border border-emerald-500/20">
                                      全新写出
                                    </span>
                                  )}
                                </div>
                                <div className="text-[10px] text-[#86868b] mt-0.5 flex items-center gap-2">
                                  <span>{candidate.message_count} 条消息</span>
                                  <span>•</span>
                                  <span>会话ID: {candidate.session_id}</span>
                                </div>
                              </div>
                            </div>

                            <div className="text-[11px] text-[#86868b] flex-shrink-0 font-mono">
                              {formatDateTime(candidate.updated_at)}
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

        {/* Footer Actions */}
        <div className="flex items-center justify-between gap-3 pt-2 border-t border-black/[0.06] dark:border-white/[0.06]">
          <div className="flex flex-col gap-1 min-w-0">
            <div className="text-[11px] text-[#86868b]">
              已选 <strong className="text-[#d97706]">{selectedIds.size}</strong> 项 / 共 {preview?.total_count || 0} 项
            </div>
            <label
              className="flex items-center gap-1.5 text-[11px] text-[#6e6e73] dark:text-[#a1a1a6] cursor-pointer"
              title={t.modals.claudeAutoRestartHint}
            >
              <input
                type="checkbox"
                checked={autoRestart}
                onChange={(e) => setAutoRestart(e.target.checked)}
                className="w-3.5 h-3.5 accent-[#d97706] cursor-pointer"
              />
              {t.modals.claudeAutoRestart}
            </label>
            {autoRestart && (
              <p className="text-[10px] text-amber-700 dark:text-amber-300 leading-snug max-w-md">
                {t.modals.claudeAutoRestartHint}
              </p>
            )}
          </div>

          <div className="flex items-center gap-2">
            <Button variant="secondary" onClick={onClose}>
              {publishResult ? '关闭' : '取消'}
            </Button>
            <Button
              variant="primary"
              onClick={handleExecute}
              disabled={selectedIds.size === 0 || isPublishing}
              className="bg-[#d97706] hover:bg-[#b45309] text-white shadow-md shadow-amber-500/15"
              icon={<UploadCloud size={13} />}
            >
              {isPublishing ? '正在同步至 Claude...' : `同步至 Claude (${selectedIds.size} 项)`}
            </Button>
          </div>
        </div>
      </div>
    </Modal>
  );
};
