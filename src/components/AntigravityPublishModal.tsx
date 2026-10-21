import React, { useState, useMemo } from 'react';
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Folder,
  Search,
  Sparkles,
  UploadCloud,
  CheckSquare,
  Square,
  AlertCircle,
  Info,
} from 'lucide-react';
import { AntigravityPublishCandidate, AntigravityPublishPreview, AntigravityPublishResult } from '../types';
import { extractProjectName, formatDateTime } from '../utils/formatters';
import { Button } from './common/Button';
import { Modal } from './common/Modal';

interface AntigravityPublishModalProps {
  isOpen: boolean;
  onClose: () => void;
  preview: AntigravityPublishPreview | null;
  onConfirmPublish: (selectedIds: string[]) => Promise<AntigravityPublishResult | void>;
  isPublishing: boolean;
}

export const AntigravityPublishModal: React.FC<AntigravityPublishModalProps> = ({
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
  const [publishResult, setPublishResult] = useState<AntigravityPublishResult | null>(null);

  // 初始化勾选：默认全选预览中的全部会话
  React.useEffect(() => {
    if (preview) {
      setSelectedIds(new Set(preview.candidates.map((c) => c.conversation_id)));
      setPublishResult(null);
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

  // 按所属工程路径归类
  const grouped = useMemo(() => {
    const map = new Map<string, AntigravityPublishCandidate[]>();
    for (const c of filteredCandidates) {
      const key = c.project_path || '未分类会话';
      const list = map.get(key) || [];
      list.push(c);
      map.set(key, list);
    }
    return Array.from(map.entries()).sort((a, b) => {
      if (a[0] === '未分类会话') return 1;
      if (b[0] === '未分类会话') return -1;
      return a[0].localeCompare(b[0]);
    });
  }, [filteredCandidates]);

  const toggleSelect = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleGroup = (candidates: AntigravityPublishCandidate[]) => {
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

  const handleStartPublish = async () => {
    const ids = Array.from(selectedIds);
    if (ids.length === 0) return;
    const res = await onConfirmPublish(ids);
    if (res) {
      setPublishResult(res);
    }
  };

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title="更新/同步会话至 Antigravity"
      widthClass="max-w-3xl"
    >
      <div className="flex flex-col gap-4">
        {publishResult ? (
          <div className="p-6 flex flex-col items-center text-center space-y-4 animate-in fade-in duration-300">
            <div className="w-14 h-14 rounded-2xl bg-emerald-500/10 dark:bg-emerald-500/20 text-emerald-600 dark:text-emerald-400 flex items-center justify-center shadow-inner">
              <CheckCircle2 size={32} />
            </div>
            <div>
              <h3 className="text-lg font-bold text-[#1d1d1f] dark:text-[#ededf0]">
                会话已成功同步至 Antigravity！
              </h3>
              <p className="text-xs text-[#86868b] mt-1">
                成功同步 <strong>{publishResult.published_count}</strong> 个会话，其中{' '}
                <span className="text-emerald-600 dark:text-emerald-400 font-medium">
                  {publishResult.new_count}
                </span>{' '}
                个为新导入，
                <span className="text-blue-600 dark:text-blue-400 font-medium">
                  {publishResult.updated_count}
                </span>{' '}
                个更新了最新标题与元数据。
              </p>
            </div>

            {publishResult.backup_path && (
              <div className="w-full bg-black/[0.02] dark:bg-white/[0.03] border border-black/[0.06] dark:border-white/[0.08] rounded-xl p-3 text-left">
                <span className="text-[11px] text-[#86868b] block mb-1">
                  已自动创建 Antigravity 数据库安全备份：
                </span>
                <span className="text-xs font-mono text-[#1d1d1f] dark:text-[#ededf0] break-all select-all">
                  {publishResult.backup_path}
                </span>
              </div>
            )}

            {publishResult.warnings.length > 0 && (
              <div className="w-full bg-amber-500/10 border border-amber-500/20 rounded-xl p-3 text-left space-y-1">
                {publishResult.warnings.map((w, idx) => (
                  <div key={idx} className="flex items-start gap-2 text-xs text-amber-700 dark:text-amber-400">
                    <Info size={14} className="mt-0.5 flex-shrink-0" />
                    <span>{w}</span>
                  </div>
                ))}
              </div>
            )}

            <div className="flex items-center justify-end w-full pt-4 border-t border-black/[0.06] dark:border-white/[0.08]">
              <Button variant="primary" onClick={onClose} className="px-6">
                完成
              </Button>
            </div>
          </div>
        ) : (
          <div className="space-y-4">
            {/* Header Description & Notice */}
            <div className="bg-gradient-to-r from-purple-500/10 via-blue-500/5 to-transparent border border-purple-500/20 rounded-2xl p-4 flex items-start gap-3">
              <Sparkles className="text-purple-600 dark:text-purple-400 flex-shrink-0 mt-0.5" size={18} />
              <div className="text-xs text-[#1d1d1f] dark:text-[#ededf0] space-y-1">
                <div className="font-semibold text-purple-700 dark:text-purple-300">
                  跨账号会话迁移与无缝漫游
                </div>
                <p className="text-[#86868b] leading-relaxed">
                  切换 Antigravity 账号后，您保存在此处的会话数据可以通过此功能<strong>一键写回当前 Antigravity 客户端</strong>。
                  系统将自动更新索引数据库（<code className="text-[11px] font-mono bg-black/[0.04] dark:bg-white/[0.06] px-1 py-0.5 rounded">conversation_summaries.db</code>）并在必要时生成标准 Brain 对话日志。
                </p>
                {preview?.is_antigravity_running && (
                  <div className="flex items-center gap-1.5 text-blue-600 dark:text-blue-400 font-medium pt-1">
                    <AlertCircle size={13} />
                    <span>检测到 Antigravity 正在运行。更新后按快捷键 Ctrl+R 重载即可在客户端侧边栏显现。</span>
                  </div>
                )}
              </div>
            </div>

            {/* Filter Bar */}
            <div className="flex items-center justify-between gap-3">
              <div className="relative flex-1">
                <Search className="absolute left-3 top-2.5 text-[#86868b]" size={13} />
                <input
                  type="text"
                  value={filterText}
                  onChange={(e) => setFilterText(e.target.value)}
                  placeholder="搜索要同步的会话标题或项目路径..."
                  className="w-full bg-black/[0.04] dark:bg-white/[0.06] text-[#1d1d1f] dark:text-[#ededf0] text-xs pl-8 pr-3 py-2 rounded-xl border border-black/[0.06] dark:border-white/[0.08] focus:border-[#9333ea] focus:outline-none placeholder-[#86868b]"
                />
              </div>

              <div className="flex items-center gap-3 text-xs">
                <label className="flex items-center gap-1.5 text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#ededf0] cursor-pointer select-none">
                  <input
                    type="checkbox"
                    checked={onlyNew}
                    onChange={(e) => setOnlyNew(e.target.checked)}
                    className="rounded text-[#9333ea] focus:ring-0 cursor-pointer accent-[#9333ea]"
                  />
                  仅看新会话 (需迁入)
                </label>

                <button
                  onClick={toggleAll}
                  className="text-[#9333ea] dark:text-[#a855f7] hover:underline font-medium select-none"
                >
                  {selectedIds.size === preview?.candidates.length ? '取消全选' : '全选'}
                </button>
              </div>
            </div>

            {/* Candidates List grouped by Project */}
            <div className="border border-black/[0.06] dark:border-white/[0.08] rounded-2xl max-h-[360px] overflow-y-auto divide-y divide-black/[0.04] dark:divide-white/[0.04] bg-black/[0.01] dark:bg-white/[0.01]">
              {grouped.length === 0 ? (
                <div className="p-8 text-center text-xs text-[#86868b]">
                  没有匹配的 Antigravity 会话记录
                </div>
              ) : (
                grouped.map(([projectKey, candidates]) => {
                  const isCollapsed = collapsedProjects.has(projectKey);
                  const allGroupChecked = candidates.every((c) => selectedIds.has(c.conversation_id));
                  const someGroupChecked = candidates.some((c) => selectedIds.has(c.conversation_id));

                  return (
                    <div key={projectKey} className="group">
                      {/* Project Header */}
                      <div className="flex items-center justify-between px-3.5 py-2.5 bg-black/[0.02] dark:bg-white/[0.03] hover:bg-black/[0.04] dark:hover:bg-white/[0.05] transition-colors">
                        <div className="flex items-center gap-2 flex-1 min-w-0">
                          <button
                            onClick={() => toggleCollapse(projectKey)}
                            className="text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-[#ededf0] p-0.5 rounded"
                          >
                            {isCollapsed ? <ChevronRight size={13} /> : <ChevronDown size={13} />}
                          </button>

                          <button
                            onClick={() => toggleGroup(candidates)}
                            className="text-[#86868b] hover:text-[#9333ea] transition-colors"
                          >
                            {allGroupChecked ? (
                              <CheckSquare size={14} className="text-[#9333ea]" />
                            ) : someGroupChecked ? (
                              <div className="w-3.5 h-3.5 rounded bg-purple-600/30 flex items-center justify-center text-[10px] text-purple-600 font-bold">
                                -
                              </div>
                            ) : (
                              <Square size={14} />
                            )}
                          </button>

                          <Folder size={13} className="text-[#86868b] flex-shrink-0" />
                          <span className="text-xs font-semibold text-[#1d1d1f] dark:text-[#ededf0] truncate">
                            {extractProjectName(projectKey)}
                          </span>
                          <span className="text-[10px] text-[#86868b] font-mono">
                            ({candidates.length})
                          </span>
                        </div>

                        {projectKey !== '未分类会话' && (
                          <span className="text-[10px] text-[#86868b] truncate max-w-[200px] font-mono">
                            {projectKey}
                          </span>
                        )}
                      </div>

                      {/* Project Conversations */}
                      {!isCollapsed && (
                        <div className="divide-y divide-black/[0.02] dark:divide-white/[0.02]">
                          {candidates.map((cand) => {
                            const isChecked = selectedIds.has(cand.conversation_id);
                            return (
                              <div
                                key={cand.conversation_id}
                                onClick={() => toggleSelect(cand.conversation_id)}
                                className={`flex items-center justify-between px-4 py-2.5 text-xs hover:bg-purple-500/[0.04] dark:hover:bg-purple-500/[0.08] cursor-pointer transition-colors ${
                                  isChecked ? 'bg-purple-500/[0.02] dark:bg-purple-500/[0.05]' : ''
                                }`}
                              >
                                <div className="flex items-center gap-2.5 flex-1 min-w-0 pr-3">
                                  <input
                                    type="checkbox"
                                    checked={isChecked}
                                    onChange={() => {}} // handled by row onClick
                                    className="rounded text-[#9333ea] focus:ring-0 cursor-pointer accent-[#9333ea]"
                                  />
                                  <span className="text-xs text-[#1d1d1f] dark:text-[#ededf0] font-medium truncate">
                                    {cand.title}
                                  </span>
                                </div>

                                <div className="flex items-center gap-2.5 flex-shrink-0">
                                  <span className="text-[10px] text-[#86868b] font-mono">
                                    {cand.message_count} 条消息
                                  </span>

                                  {cand.exists_in_target ? (
                                    <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-blue-500/10 text-blue-600 dark:text-blue-400 border border-blue-500/20">
                                      客户端已有
                                    </span>
                                  ) : (
                                    <span className="px-2 py-0.5 rounded-full text-[10px] font-medium bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border border-emerald-500/20 flex items-center gap-1">
                                      <Sparkles size={9} />
                                      待迁入新会话
                                    </span>
                                  )}

                                  <span className="text-[10px] text-[#86868b] min-w-[70px] text-right">
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

            {/* Modal Bottom Action Footer */}
            <div className="flex items-center justify-between pt-3 border-t border-black/[0.06] dark:border-white/[0.08]">
              <div className="flex items-center gap-2 text-xs text-[#86868b]">
                <span>
                  已选择 <strong className="text-[#9333ea] dark:text-[#a855f7]">{selectedIds.size}</strong> /{' '}
                  {preview?.total_count || 0} 个会话
                </span>
                {preview && preview.new_count > 0 && (
                  <span className="text-emerald-600 dark:text-emerald-400 font-medium">
                    (包含 {preview.new_count} 个新会话)
                  </span>
                )}
              </div>

              <div className="flex items-center gap-2.5">
                <Button variant="ghost" onClick={onClose} disabled={isPublishing}>
                  取消
                </Button>
                <Button
                  variant="primary"
                  onClick={handleStartPublish}
                  disabled={selectedIds.size === 0 || isPublishing}
                  icon={<UploadCloud size={14} />}
                  className="bg-gradient-to-r from-[#9333ea] to-[#0071e3] hover:from-[#7e22ce] hover:to-[#005bb5] text-white shadow-md shadow-purple-500/20 px-5"
                >
                  {isPublishing
                    ? '正在同步写入 Antigravity...'
                    : `更新至 Antigravity (${selectedIds.size})`}
                </Button>
              </div>
            </div>
          </div>
        )}
      </div>
    </Modal>
  );
};
