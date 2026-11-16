import React, { useState, useEffect, useCallback, useMemo, useTransition, useRef } from 'react';

import { api, errorMessage } from './api/client';
import {
  AntigravityCandidate,
  AntigravityPublishPreview,
  AntigravityRecoveryReport,
  CanonicalMessage,
  ClaudeCandidate,
  ClaudePublishPreview,
  ClaudeRecoveryReport,
  CodexPublishPreview,
  CodexRecoveryReport,
  CodexThreadCandidate,
  CodexVisibility,
  Conversation,
  ConversationSummary,
  FolderPin,
  PrismOpenMode,
  ResetResult,
  VaultFocusPayload,
} from './types';
import { NavRail, AppPage, APP_PAGES } from './components/NavRail';
import { SessionsHeader } from './components/SessionsHeader';
import { Sidebar } from './components/Sidebar';
import { ChatView } from './components/ChatView';
import { CodexRecoveryModal } from './components/CodexRecoveryModal';
import { AntigravityRecoveryModal } from './components/AntigravityRecoveryModal';
import { AntigravityPublishModal } from './components/AntigravityPublishModal';
import { ClaudeRecoveryModal } from './components/ClaudeRecoveryModal';
import { ClaudePublishModal } from './components/ClaudePublishModal';
import { ImportModal } from './components/ImportModal';
import { SkillsManager } from './components/SkillsManager';
import { McpManager } from './components/McpManager';
import { AccountsManager } from './components/AccountsManager';
import { QuickLaunchModal } from './components/QuickLaunch';
import {
  ConversationHandoffModal,
  HandoffConversationTarget,
} from './components/ConversationHandoffModal';

import { Modal } from './components/common/Modal';
import { Button } from './components/common/Button';
import { ToastContainer, ToastMessage } from './components/common/Toast';
import { useI18n } from './i18n';
import { RefreshCw } from 'lucide-react';

/** Default name offered in the save dialog, which lets the user change it. */
const safeFileName = (title: string) =>
  title
    .replace(/[\\/:*?"<>|\u0000-\u001f]/g, '_')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 120) || 'conversation';

export const App: React.FC = () => {
  const { t, interpolate } = useI18n();
  const [theme, setTheme] = useState<'dark' | 'light'>(() => {
    const saved = localStorage.getItem('cv_theme_v2');
    return saved === 'dark' || saved === 'light' ? saved : 'light';
  });

  useEffect(() => {
    if (theme === 'dark') {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
    localStorage.setItem('cv_theme_v2', theme);
  }, [theme]);

  const toggleTheme = () => {
    setTheme((prev) => (prev === 'dark' ? 'light' : 'dark'));
  };

  const [page, setPage] = useState<AppPage>(() => {
    const saved = localStorage.getItem('cv_active_page') as AppPage | null;
    return saved && APP_PAGES.includes(saved) ? saved : 'sessions';
  });
  const changePage = (next: AppPage) => {
    setPage(next);
    localStorage.setItem('cv_active_page', next);
  };

  const [sidebarWidth, setSidebarWidth] = useState<number>(() => {
    const saved = localStorage.getItem('cv_sidebar_width');
    return saved ? Math.max(220, Math.min(600, parseInt(saved, 10))) : 320;
  });
  const [isResizing, setIsResizing] = useState(false);

  const startResizing = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      setIsResizing(true);
      const startX = e.clientX;
      const startW = sidebarWidth;

      const handleMouseMove = (moveEvent: MouseEvent) => {
        const newW = Math.max(220, Math.min(600, startW + moveEvent.clientX - startX));
        setSidebarWidth(newW);
        localStorage.setItem('cv_sidebar_width', newW.toString());
      };

      const handleMouseUp = () => {
        setIsResizing(false);
        document.removeEventListener('mousemove', handleMouseMove);
        document.removeEventListener('mouseup', handleMouseUp);
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
      };

      document.addEventListener('mousemove', handleMouseMove);
      document.addEventListener('mouseup', handleMouseUp);
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
    },
    [sidebarWidth]
  );

  const resetSidebarWidth = () => {
    setSidebarWidth(320);
    localStorage.setItem('cv_sidebar_width', '320');
  };

  const [conversations, setConversations] = useState<ConversationSummary[]>([]);
  const [folderPins, setFolderPins] = useState<FolderPin[]>([]);
  const [showArchived, setShowArchived] = useState(false);
  const listRequest = useRef(0);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // Publishing selection (independent from deletion)
  const [publishSelection, setPublishSelection] = useState<Set<string>>(new Set());
  const [scanning, setScanning] = useState<'codex' | 'antigravity' | 'claude' | null>(null);

  // Active source switcher state: 'codex' | 'antigravity' | 'claude' (独立分隔开)
  const [activeSource, setActiveSource] = useState<'codex' | 'antigravity' | 'claude'>(() => {
    return (localStorage.getItem('cv_active_source') as 'codex' | 'antigravity' | 'claude') || 'codex';
  });

  const handleSourceChange = (source: 'codex' | 'antigravity' | 'claude') => {
    if (source !== activeSource) {
      // Batch actions act on the selection, so it must never include conversations
      // from a tab that is not shown.
      setPublishSelection(new Set());
    }
    setActiveSource(source);
    localStorage.setItem('cv_active_source', source);
  };

  const codexConversations = useMemo(
    () => conversations.filter((c) => c.source_kind !== 'antigravity' && c.source_kind !== 'claude'),
    [conversations]
  );

  const antigravityConversations = useMemo(
    () => conversations.filter((c) => c.source_kind === 'antigravity'),
    [conversations]
  );

  const claudeConversations = useMemo(
    () => conversations.filter((c) => c.source_kind === 'claude'),
    [conversations]
  );

  const displayedConversations = useMemo(() => {
    if (activeSource === 'antigravity') return antigravityConversations;
    if (activeSource === 'claude') return claudeConversations;
    return codexConversations;
  }, [activeSource, codexConversations, antigravityConversations, claudeConversations]);

  // Where the Codex sidebar lists each conversation. Read again on every list reload
  // and when the window regains focus, so changes made in Codex show up.
  const [codexVisibility, setCodexVisibility] = useState<CodexVisibility | null>(null);
  const refreshCodexVisibility = useCallback(() => {
    api
      .getCodexVisibility()
      .then(setCodexVisibility)
      // Codex may be rewriting its state file; keep the last grouping.
      .catch(() => {});
  }, []);
  useEffect(() => {
    window.addEventListener('focus', refreshCodexVisibility);
    return () => window.removeEventListener('focus', refreshCodexVisibility);
  }, [refreshCodexVisibility]);

  const codexVisibleCount = useMemo(
    () =>
      codexVisibility?.available
        ? codexConversations.filter((c) => c.id in codexVisibility.conversations).length
        : undefined,
    [codexVisibility, codexConversations]
  );

  const [activeConversation, setActiveConversation] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<CanonicalMessage[]>([]);
  const [isLoadingMessages, setIsLoadingMessages] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [, startTransition] = useTransition();


  // Codex Modal state
  const [recoveryModalOpen, setRecoveryModalOpen] = useState(false);
  const [recoveryReport, setRecoveryReport] = useState<CodexRecoveryReport | null>(null);
  const [isRecovering, setIsRecovering] = useState(false);

  // Antigravity Modal state
  const [antigravityModalOpen, setAntigravityModalOpen] = useState(false);
  const [antigravityReport, setAntigravityReport] = useState<AntigravityRecoveryReport | null>(null);
  const [isRecoveringAntigravity, setIsRecoveringAntigravity] = useState(false);

  // Antigravity Publish Modal state
  const [antigravityPublishModalOpen, setAntigravityPublishModalOpen] = useState(false);
  const [antigravityPublishPreview, setAntigravityPublishPreview] = useState<AntigravityPublishPreview | null>(null);
  const [isPublishingAntigravity, setIsPublishingAntigravity] = useState(false);

  // Claude Modal state
  const [claudeModalOpen, setClaudeModalOpen] = useState(false);
  const [claudeReport, setClaudeReport] = useState<ClaudeRecoveryReport | null>(null);
  const [isRecoveringClaude, setIsRecoveringClaude] = useState(false);

  // Claude Publish Modal state
  const [claudePublishModalOpen, setClaudePublishModalOpen] = useState(false);
  const [claudePublishPreview, setClaudePublishPreview] = useState<ClaudePublishPreview | null>(null);
  const [isPublishingClaude, setIsPublishingClaude] = useState(false);

  const [importModalOpen, setImportModalOpen] = useState(false);
  const [quickLaunchOpen, setQuickLaunchOpen] = useState(false);
  const [handoffConv, setHandoffConv] = useState<HandoffConversationTarget | null>(null);

  // Codex publish confirmation
  const [codexPublishDialog, setCodexPublishDialog] = useState<{
    ids: string[];
    preview: CodexPublishPreview;
  } | null>(null);
  const [isPublishingCodex, setIsPublishingCodex] = useState(false);

  // Preview the whole local database before the user confirms clearing it.
  const [resetDialog, setResetDialog] = useState<ResetResult | null>(null);
  const [isPreparingReset, setIsPreparingReset] = useState(false);
  const [isResetting, setIsResetting] = useState(false);


  // Rename prompt state
  const [renameDialog, setRenameDialog] = useState<{
    id: string;
    currentTitle: string;
    newTitle: string;
  } | null>(null);

  // Delete confirm dialog state
  const [deleteDialog, setDeleteDialog] = useState<{
    ids: string[];
    title: string;
  } | null>(null);

  // Global shortcuts must not act behind an open dialog.
  const anyDialogOpen =
    recoveryModalOpen ||
    antigravityModalOpen ||
    antigravityPublishModalOpen ||
    claudeModalOpen ||
    claudePublishModalOpen ||
    importModalOpen ||
    quickLaunchOpen ||
    handoffConv !== null ||
    renameDialog !== null ||
    deleteDialog !== null ||
    codexPublishDialog !== null ||
    resetDialog !== null;

  // Toasts
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const addToast = (type: 'success' | 'warning' | 'error' | 'info', message: string) => {
    const id = `${Date.now()}-${Math.random()}`;
    setToasts((prev) => [...prev, { id, type, message }]);
    setTimeout(() => {
      setToasts((prev) => prev.filter((t) => t.id !== id));
    }, type === 'error' ? 8000 : 4500);
  };

  const dismissToast = (id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  };

  useEffect(() => {
    let cancelled = false;
    api.listFolderPins().then((pins) => {
      if (!cancelled) setFolderPins(pins);
    }).catch((err) => {
      if (!cancelled) addToast('error', `加载文件夹置顶失败：${errorMessage(err)}`);
    });
    return () => { cancelled = true; };
  }, []);

  // Reload conversation list
  const [detailRefreshSeq, setDetailRefreshSeq] = useState(0);
  const loadedConversationIdRef = useRef<string | null>(null);

  const reloadConversations = useCallback(
    async (selectIdAfter?: string, query?: string, refreshMessages = false) => {
      const request = ++listRequest.current;
      try {
        let list: ConversationSummary[];
        const currentQuery = query ?? searchQuery;
        if (currentQuery.trim()) {
          list = await api.searchConversations(currentQuery.trim(), undefined, showArchived);
        } else if (showArchived) {
          const [active, archived] = await Promise.all([api.listConversations(), api.listConversations(true)]);
          list = [...active, ...archived].sort((a, b) => b.updated_at.localeCompare(a.updated_at));
        } else {
          list = await api.listConversations();
        }
        if (request !== listRequest.current) return;
        setConversations(list);
        // Search results are a subset of the list, whose placements are already known.
        if (!currentQuery.trim()) refreshCodexVisibility();

        const resolvedId =
          selectIdAfter && list.some((c) => c.id === selectIdAfter)
            ? selectIdAfter
            : selectedId && list.some((c) => c.id === selectedId)
            ? selectedId
            : list.length > 0
            ? list[0].id
            : null;

        if (resolvedId) {
          const summary = list.find((c) => c.id === resolvedId);
          if (summary) {
            setActiveConversation((prev) =>
              prev && prev.id === summary.id ? { ...prev, ...summary } : prev
            );
          }
          setSelectedId(resolvedId);
          if (refreshMessages && resolvedId === selectedId) {
            setDetailRefreshSeq((n) => n + 1);
          }
        } else {
          loadedConversationIdRef.current = null;
          setSelectedId(null);
          setActiveConversation(null);
          setMessages([]);
        }
      } catch (err) {
        if (request === listRequest.current) addToast('error', `加载会话列表失败：${errorMessage(err)}`);
      }
    },
    [selectedId, showArchived, searchQuery, refreshCodexVisibility]
  );

  useEffect(() => {
    void reloadConversations();
  }, [showArchived]);

  // Synchronize selection when switching active source tab
  useEffect(() => {
    if (displayedConversations.length > 0) {
      if (!selectedId || !displayedConversations.some((c) => c.id === selectedId)) {
        setSelectedId(displayedConversations[0].id);
      }
    } else {
      setSelectedId(null);
      setActiveConversation(null);
      setMessages([]);
    }
  }, [activeSource, displayedConversations]);


  // Initial load
  useEffect(() => {
    api.listConversations().then((list) => {
      if (list.length === 0) {
        // Automatically offer Codex scan if database is fresh
        handleScanCodex();
      }
    }).catch((err) => addToast('error', `加载会话列表失败：${errorMessage(err)}`));
  }, []);

  // Load message detail when selectedId changes or an incremental recovery refreshes it
  useEffect(() => {
    if (!selectedId) {
      loadedConversationIdRef.current = null;
      setActiveConversation(null);
      setMessages([]);
      return;
    }

    let isCancelled = false;
    const isSwitchingConversation = loadedConversationIdRef.current !== selectedId;
    if (isSwitchingConversation) {
      setIsLoadingMessages(true);
    }

    Promise.all([api.getConversation(selectedId), api.getMessages(selectedId)])
      .then(([conv, msgs]) => {
        if (!isCancelled) {
          loadedConversationIdRef.current = selectedId;
          setActiveConversation(conv);
          setMessages(msgs);
        }
      })
      .catch((err) => {
        if (!isCancelled) {
          addToast('error', `获取消息失败：${errorMessage(err)}`);
        }
      })
      .finally(() => {
        if (!isCancelled && isSwitchingConversation) {
          setIsLoadingMessages(false);
        }
      });

    return () => {
      isCancelled = true;
    };
  }, [selectedId, detailRefreshSeq]);

  // Global Keyboard Shortcuts: Ctrl+K to search, F2 for rename, Del for delete
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'l') {
        e.preventDefault();
        if (quickLaunchOpen) {
          setQuickLaunchOpen(false);
        } else if (!anyDialogOpen) {
          setQuickLaunchOpen(true);
        }
        return;
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        if (anyDialogOpen) return;
        changePage('sessions');
        setTimeout(() => {
          const input = document.getElementById('sidebar-search') as HTMLInputElement | null;
          input?.focus();
          input?.select();
        }, 0);
        return;
      }
      // F2 and Del act on the open conversation, which only the sessions page shows.
      if (
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement ||
        anyDialogOpen ||
        page !== 'sessions'
      ) {
        return;
      }
      if (e.key === 'F2' && selectedId && activeConversation) {
        e.preventDefault();
        setRenameDialog({
          id: selectedId,
          currentTitle: activeConversation.title,
          newTitle: activeConversation.title,
        });
      } else if ((e.key === 'Delete' || e.key === 'Del') && selectedId && activeConversation) {
        e.preventDefault();
        setDeleteDialog({
          ids: [selectedId],
          title: `确定删除会话“${activeConversation.title}”？`,
        });
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [selectedId, activeConversation, anyDialogOpen, page, quickLaunchOpen]);

  // Search input handler: wait for a pause in typing before querying the index.
  const searchTimer = useRef<ReturnType<typeof setTimeout>>();
  useEffect(() => () => clearTimeout(searchTimer.current), []);
  const handleSearchChange = (q: string) => {
    setSearchQuery(q);
    clearTimeout(searchTimer.current);
    searchTimer.current = setTimeout(() => {
      startTransition(() => {
        void reloadConversations(undefined, q);
      });
    }, 200);
  };

  // Selection for Codex publish
  const togglePublishSelection = (id: string) => {
    setPublishSelection((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleProjectPublishSelection = (convIds: string[]) => {
    const allChecked = convIds.every((id) => publishSelection.has(id));
    setPublishSelection((prev) => {
      const next = new Set(prev);
      if (allChecked) {
        convIds.forEach((id) => next.delete(id));
      } else {
        convIds.forEach((id) => next.add(id));
      }
      return next;
    });
  };

  // Codex Scan
  const handleScanCodex = async () => {
    if (scanning) return;
    setScanning('codex');
    try {
      addToast('info', '正在扫描本地 Codex 历史与会话索引...');
      const report = await api.scanCodex();
      setRecoveryReport(report);
      setRecoveryModalOpen(true);
      if (conversations.length > 0) {
        void reloadConversations();
      }
    } catch (err) {
      addToast('error', `扫描 Codex 失败：${errorMessage(err)}`);
    } finally {
      setScanning(null);
    }
  };

  // Codex Recovery Confirm
  const handleConfirmRecovery = async (candidates: CodexThreadCandidate[]) => {
    setIsRecovering(true);
    try {
      const res = await api.recoverCodexCandidates(candidates);
      setRecoveryModalOpen(false);
      handleSourceChange('codex');
      addToast(
        'success',
        `恢复完成：新增 ${res.new_conversation_count} 个会话，写入 ${res.message_count} 条消息`
      );
      if (res.warnings.length > 0) addToast('warning', res.warnings.slice(0, 3).join('；'));
      await reloadConversations(undefined, undefined, true);
    } catch (err) {
      addToast('error', `恢复失败：${errorMessage(err)}`);
    } finally {
      setIsRecovering(false);
    }
  };

  // Antigravity Scan
  const handleScanAntigravity = async () => {
    if (scanning) return;
    setScanning('antigravity');
    try {
      addToast('info', '正在扫描 Antigravity 历史会话...');
      const report = await api.scanAntigravity();
      setAntigravityReport(report);
      setAntigravityModalOpen(true);
      if (conversations.length > 0) {
        void reloadConversations();
      }
    } catch (err) {
      addToast('error', `扫描 Antigravity 失败：${errorMessage(err)}`);
    } finally {
      setScanning(null);
    }
  };

  // Antigravity Recovery Confirm
  const handleConfirmAntigravityRecovery = async (candidates: AntigravityCandidate[]) => {
    setIsRecoveringAntigravity(true);
    try {
      const res = await api.recoverAntigravityCandidates(candidates);
      setAntigravityModalOpen(false);
      handleSourceChange('antigravity');
      addToast(
        'success',
        `Antigravity 恢复完成：新增 ${res.new_conversation_count} 个会话，解析 ${res.message_count} 条消息`
      );
      if (res.warnings.length > 0) addToast('warning', res.warnings.slice(0, 3).join('；'));
      await reloadConversations(undefined, undefined, true);
    } catch (err) {
      addToast('error', `恢复 Antigravity 失败：${errorMessage(err)}`);
    } finally {
      setIsRecoveringAntigravity(false);
    }
  };

  // Claude Scan
  const handleScanClaude = async () => {
    if (scanning) return;
    setScanning('claude');
    try {
      addToast('info', '正在扫描 Claude 本地会话...');
      const report = await api.scanClaude();
      setClaudeReport(report);
      setClaudeModalOpen(true);
      if (conversations.length > 0) {
        void reloadConversations();
      }
    } catch (err) {
      addToast('error', `扫描 Claude 失败：${errorMessage(err)}`);
    } finally {
      setScanning(null);
    }
  };

  // Claude Recovery Confirm
  const handleConfirmClaudeRecovery = async (candidates: ClaudeCandidate[]) => {
    setIsRecoveringClaude(true);
    try {
      const res = await api.recoverClaudeCandidates(candidates);
      setClaudeModalOpen(false);
      handleSourceChange('claude');
      addToast(
        'success',
        `Claude 恢复完成：新增 ${res.new_conversation_count} 个会话，解析 ${res.message_count} 条消息`
      );
      if (res.warnings.length > 0) addToast('warning', res.warnings.slice(0, 3).join('；'));
      await reloadConversations(undefined, undefined, true);
    } catch (err) {
      addToast('error', `恢复 Claude 失败：${errorMessage(err)}`);
    } finally {
      setIsRecoveringClaude(false);
    }
  };

  // Codex Publish
  const handlePublishToCodex = async () => {

    if (publishSelection.size === 0) {
      addToast('warning', '请先勾选需要发布到 Codex 客户端的会话');
      return;
    }

    try {
      const selectedIds = Array.from(publishSelection);
      const preview = await api.previewCodexPublish(selectedIds);
      if (preview.selected_thread_count === 0) {
        addToast('warning', '所选会话均不在当前 Codex 索引中，无法发布。请勾选从 Codex 恢复的会话。');
        return;
      }
      setCodexPublishDialog({ ids: selectedIds, preview });
    } catch (err) {
      addToast('error', `读取 Codex 发布预览失败：${errorMessage(err)}`);
    }
  };

  const executePublishToCodex = async () => {
    if (!codexPublishDialog) return;
    setIsPublishingCodex(true);
    try {
      addToast('info', '正在备份并写入 Codex 数据库...');
      const result = await api.publishToCodex(codexPublishDialog.ids, [], true);
      setCodexPublishDialog(null);
      setPublishSelection(new Set());
      addToast(
        'success',
        `发布成功！已向 Codex 侧边栏写入 ${result.catalog_thread_count} 个主会话，备份位于 ${result.backup_directory}`
      );
    } catch (err) {
      addToast('error', `发布失败：${errorMessage(err)}`);
    } finally {
      setIsPublishingCodex(false);
    }
  };

  // Antigravity Publish / Sync
  const handleOpenPublishAntigravity = async () => {
    try {
      const selectedIds = Array.from(publishSelection);
      const preview = await api.previewAntigravityPublish(selectedIds);
      if (preview.total_count === 0) {
        addToast('warning', '当前暂无可同步的 Antigravity 会话');
        return;
      }
      setAntigravityPublishPreview(preview);
      setAntigravityPublishModalOpen(true);
    } catch (err) {
      addToast('error', `加载 Antigravity 同步预览失败：${errorMessage(err)}`);
    }
  };

  const handleConfirmPublishAntigravity = async (selectedIds: string[]) => {
    setIsPublishingAntigravity(true);
    try {
      const result = await api.publishToAntigravity(selectedIds);
      addToast(
        'success',
        `已成功同步 ${result.published_count} 个会话至 Antigravity！`
      );
      if (publishSelection.size > 0) {
        setPublishSelection(new Set());
      }
      return result;
    } catch (err) {
      addToast('error', `更新至 Antigravity 失败：${errorMessage(err)}`);
    } finally {
      setIsPublishingAntigravity(false);
    }
  };

  // Claude Publish / Sync
  const handleOpenPublishClaude = async () => {
    try {
      const selectedIds = Array.from(publishSelection);
      const preview = await api.previewClaudePublish(selectedIds);
      if (preview.total_count === 0) {
        addToast('warning', '当前暂无可同步的 Claude 会话');
        return;
      }
      setClaudePublishPreview(preview);
      setClaudePublishModalOpen(true);
    } catch (err) {
      addToast('error', `加载 Claude 同步预览失败：${errorMessage(err)}`);
    }
  };

  const handleConfirmPublishClaude = async (selectedIds: string[], autoRestart: boolean) => {
    setIsPublishingClaude(true);
    try {
      const result = await api.publishToClaude(selectedIds, autoRestart);
      addToast(
        'success',
        `已成功同步 ${result.published_count} 个会话至 Claude！`
      );
      if (publishSelection.size > 0) {
        setPublishSelection(new Set());
      }
      return result;
    } catch (err) {
      addToast('error', `发布至 Claude 失败：${errorMessage(err)}`);
    } finally {
      setIsPublishingClaude(false);
    }
  };

  // Rename
  const handleRename = (id: string, currentTitle: string) => {
    setRenameDialog({
      id,
      currentTitle,
      newTitle: currentTitle,
    });
  };

  const executeRename = async () => {
    if (!renameDialog || !renameDialog.newTitle.trim()) return;
    const { id } = renameDialog;
    const title = renameDialog.newTitle.trim();
    try {
      await api.renameConversation(id, title);
      addToast('success', '重命名会话成功');
      setRenameDialog(null);
      // The header shows the loaded conversation, which a list reload does not refresh.
      setActiveConversation((prev) =>
        prev && prev.id === id ? { ...prev, title, title_user_edited: true } : prev
      );
      await reloadConversations(id);
    } catch (err) {
      addToast('error', `重命名失败：${errorMessage(err)}`);
    }
  };

  const handleSetPinned = async (id: string, pinned: boolean) => {
    try {
      if (!await api.setPinned(id, pinned)) throw new Error('会话不存在，请刷新列表后重试');
      await reloadConversations();
    } catch (err) {
      addToast('error', `更新置顶状态失败：${errorMessage(err)}`);
    }
  };

  const handleSetFolderPinned = async (path: string, pinned: boolean) => {
    try {
      await api.setFolderPinned(activeSource, path, pinned);
      setFolderPins(await api.listFolderPins());
    } catch (err) {
      addToast('error', `更新文件夹置顶失败：${errorMessage(err)}`);
    }
  };

  // Delete
  const handleDeleteSingle = (id: string, title: string) => {
    setDeleteDialog({
      ids: [id],
      title: `确定删除会话“${title}”？`,
    });
  };

  const handleBatchDelete = (ids: string[]) => {
    setDeleteDialog({
      ids,
      title: `确定批量删除选中的 ${ids.length} 个会话？`,
    });
  };

  const executeDelete = async () => {
    if (!deleteDialog) return;
    const deleted = new Set(deleteDialog.ids);
    try {
      const count = await api.deleteConversations(deleteDialog.ids);
      addToast('success', `成功删除 ${count} 个会话`);
      setDeleteDialog(null);
      setPublishSelection((prev) => new Set([...prev].filter((id) => !deleted.has(id))));
      await reloadConversations();
    } catch (err) {
      addToast('error', `删除失败：${errorMessage(err)}`);
    }
  };

  // Exports & Openers
  const handleExportMarkdown = async (id: string, title: string, projectPath?: string | null) => {
    const safeTitle = safeFileName(title);
    const defaultFilename = `${safeTitle}.md`;

    try {
      // 弹出系统原生文件保存选择对话框，用户可方便自由挑选目标文件夹与修改文件名
      const target = await api.pickSavePath(
        defaultFilename,
        'Markdown 文档',
        'md',
        projectPath || undefined
      );

      // 用户在选择器中点击了取消或关闭
      if (!target) return;

      await api.exportMarkdown(id, target);
      addToast('success', `已成功保存 Markdown 至：${target}`);
    } catch (err) {
      addToast('error', `保存 Markdown 失败：${errorMessage(err)}`);
    }
  };

  const handleExportJson = async (id: string, title: string, projectPath?: string | null) => {
    const safeTitle = safeFileName(title);
    const defaultFilename = `${safeTitle}.json`;

    try {
      const target = await api.pickSavePath(
        defaultFilename,
        'JSON 数据文件',
        'json',
        projectPath || undefined
      );

      if (!target) return;

      await api.exportJson(id, target);
      addToast('success', `已成功导出 JSON 至：${target}`);
    } catch (err) {
      addToast('error', `导出 JSON 失败：${errorMessage(err)}`);
    }
  };

  const handleBatchExportMarkdown = async (ids?: string[]) => {
    const targetIds = ids && ids.length > 0 ? ids : Array.from(publishSelection);
    if (targetIds.length === 0) {
      addToast('warning', '请先在左侧勾选需要导出的会话');
      return;
    }

    try {
      const selectedProjects = targetIds.map(
        (id) => conversations.find((conversation) => conversation.id === id)?.project_path
      );
      const commonProject = selectedProjects[0] && selectedProjects.every(
        (path) => path === selectedProjects[0]
      ) ? selectedProjects[0] : undefined;
      const previousDirectory = localStorage.getItem('cv_batch_export_directory');
      const targetDir = await api.pickFolderPath(previousDirectory || commonProject || undefined);
      if (!targetDir) {
        return; // 用户取消选择
      }

      addToast('info', `正在批量导出 ${targetIds.length} 个 Markdown 文档至：${targetDir}...`);
      const count = await api.batchExportMarkdown(targetIds, targetDir);
      localStorage.setItem('cv_batch_export_directory', targetDir);
      addToast('success', `已成功批量保存 ${count} 个 Markdown 文件至目标文件夹！`);
    } catch (err) {
      addToast('error', `批量导出 Markdown 失败：${errorMessage(err)}`);
    }
  };

  const handleOpenReset = async () => {
    if (isPreparingReset || isResetting) return;
    setIsPreparingReset(true);
    try {
      setResetDialog(await api.resetConversations('all', true, true));
    } catch (err) {
      addToast('error', `读取会话库统计失败：${errorMessage(err)}`);
    } finally {
      setIsPreparingReset(false);
    }
  };

  const executeReset = async () => {
    if (!resetDialog || isResetting) return;
    setIsResetting(true);
    try {
      const result = await api.resetConversations('all', true, false);
      setResetDialog(null);
      clearTimeout(searchTimer.current);
      ++listRequest.current;
      setSearchQuery('');
      setConversations([]);
      setFolderPins([]);
      setPublishSelection(new Set());
      setSelectedId(null);
      setActiveConversation(null);
      setMessages([]);
      addToast(
        'success',
        `已清空 ${result.conversation_count} 个会话（${result.message_count} 条消息），备份位于 ${result.backup_path}`
      );
      await reloadConversations(undefined, '');
    } catch (err) {
      addToast('error', `清空失败：${errorMessage(err)}`);
    } finally {
      setIsResetting(false);
    }
  };

  const handleClearSelection = () => {
    setPublishSelection(new Set());
  };

  const handleBackup = async () => {
    try {
      const path = await api.backupDatabase();
      addToast('success', `数据库安全备份完成：${path}`);
    } catch (err) {
      addToast('error', `备份失败：${errorMessage(err)}`);
    }
  };

  const handleOpenFolder = async (path: string) => {
    try {
      await api.openFolder(path);
    } catch (err) {
      addToast('error', `打开文件夹失败：${errorMessage(err)}`);
    }
  };

  const handleOpenVSCode = async (path: string) => {
    try {
      await api.openVSCode(path);
      addToast('info', '已在 VS Code 新窗口中打开项目');
    } catch (err) {
      addToast('error', `拉起 VS Code 失败：${errorMessage(err)}`);
    }
  };

  const handleOpenPrismProject = async (
    path: string,
    mode: PrismOpenMode = 'new-workspace'
  ) => {
    try {
      await api.openInPrismStudio(path, mode);
      addToast(
        'success',
        mode === 'parallel-root'
          ? '已并列挂载到棱镜工坊当前空间'
          : t.chat.toastOpenedInPrism
      );
    } catch (err) {
      addToast('error', `打开棱镜工坊失败：${errorMessage(err)}`);
    }
  };

  const handleOpenFileInPrism = async (
    filePath: string,
    line?: number,
    projectPath?: string | null
  ) => {
    try {
      await api.openInPrismStudio(filePath, 'file', line, projectPath);
      addToast(
        'success',
        line ? `已在棱镜工坊定位至第 ${line} 行` : t.chat.toastOpenedInPrism
      );
    } catch (err) {
      addToast('error', `在棱镜工坊打开文件失败：${errorMessage(err)}`);
    }
  };

  const handleSendCodeToPrism = async (
    lang: string,
    code: string,
    title: string,
    projectPath?: string | null,
    autoRun = false
  ) => {
    try {
      await api.sendCodeToPrismStudio({
        language: lang,
        code,
        title,
        projectPath,
        autoRun,
      });
      addToast(
        'success',
        autoRun ? '已送入棱镜工坊并启动运行' : t.chat.toastOpenedInPrism
      );
    } catch (err) {
      addToast('error', `发送代码到棱镜工坊失败：${errorMessage(err)}`);
    }
  };

  const handleReadConversationInPrism = async (conversationId: string) => {
    try {
      await api.openConversationInPrismStudio(conversationId);
      addToast('success', '已在棱镜工坊打开会话精排视图');
    } catch (err) {
      addToast('error', `在棱镜工坊打开会话失败：${errorMessage(err)}`);
    }
  };

  const applyBridgeFocus = useCallback(
    (payload: VaultFocusPayload) => {
      changePage('sessions');
      if (payload.sourceKind === 'antigravity') {
        handleSourceChange('antigravity');
      } else if (payload.sourceKind === 'claude') {
        handleSourceChange('claude');
      } else if (payload.sourceKind) {
        handleSourceChange('codex');
      }
      if (payload.conversationId) {
        setSearchQuery('');
        setSelectedId(payload.conversationId);
      } else if (payload.projectPath) {
        const parts = payload.projectPath.replace(/[\\/]+$/, '').split(/[\\/]/);
        const base = parts[parts.length - 1] || payload.projectPath;
        setSearchQuery(base);
      }
    },
    []
  );

  const handleJumpConversationLink = async (idOrSourceId: string) => {
    const direct = conversations.find((c) => c.id === idOrSourceId);
    if (direct) {
      applyBridgeFocus({
        conversationId: direct.id,
        sourceKind: direct.source_kind,
      });
      return;
    }
    try {
      const resolved = await api.resolveConversationLink(idOrSourceId);
      if (resolved && resolved.conversationId) {
        applyBridgeFocus(resolved);
      } else {
        addToast('warning', `本地会话库尚未收录会话：${idOrSourceId}（可先点击右上角“扫描”）`);
      }
    } catch (err) {
      addToast('error', `跳转会话失败：${errorMessage(err)}`);
    }
  };

  const handleOpenExternalUrl = async (url: string) => {
    try {
      await api.openExternalUrl(url);
    } catch (err) {
      addToast('error', `打开网页链接失败：${errorMessage(err)}`);
    }
  };

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void api
      .drainVaultBridgeFocus?.()
      ?.then((items) => {
        if (!cancelled && items && items.length > 0) {
          applyBridgeFocus(items[items.length - 1]);
        }
      })
      .catch(() => {});

    if (
      typeof window !== 'undefined' &&
      ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
    ) {
      void import('@tauri-apps/api/event').then(({ listen }) => {
        if (cancelled) return;
        void listen<VaultFocusPayload>('cv://bridge-focus', (ev) => {
          if (!cancelled && ev.payload) {
            applyBridgeFocus(ev.payload);
          }
        }).then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        });
      });
    }

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [applyBridgeFocus]);

  const [isOpeningClient, setIsOpeningClient] = useState(false);

  const handleOpenClient = async (source?: 'codex' | 'antigravity' | 'claude') => {
    const targetSource = source || activeSource;
    setIsOpeningClient(true);
    const clientName =
      targetSource === 'codex'
        ? 'Codex'
        : targetSource === 'antigravity'
        ? 'Antigravity'
        : 'Claude';
    addToast('info', `正在启动 ${clientName} 客户端...`);

    try {
      if (targetSource === 'codex') {
        await api.startCodexClient();
      } else if (targetSource === 'antigravity') {
        await api.startAntigravityClient();
      } else {
        await api.startClaudeClient();
      }
      addToast('success', `已成功调起 ${clientName} 客户端！`);
    } catch (err) {
      addToast('error', `启动 ${clientName} 客户端失败：${errorMessage(err)}`);
    } finally {
      setIsOpeningClient(false);
    }
  };

  return (
    <div className="w-full h-full flex cv-canvas overflow-hidden text-[#0f172a] dark:text-[#e2e8f0] transition-colors duration-200">
      <NavRail
        page={page}
        onPageChange={changePage}
        theme={theme}
        onToggleTheme={toggleTheme}
        onOpenQuickLaunch={() => setQuickLaunchOpen(true)}
      />

      {/* Only the active page is mounted; the others reload when opened. */}
      <div className="flex-1 min-w-0 flex flex-col overflow-hidden">
        {page === 'sessions' && (
          <>
            <SessionsHeader
              activeSource={activeSource}
              onSourceChange={handleSourceChange}
              codexCount={codexConversations.length}
              codexVisibleCount={codexVisibleCount}
              antigravityCount={antigravityConversations.length}
              claudeCount={claudeConversations.length}
              selectionCount={publishSelection.size}
              onScan={
                activeSource === 'codex'
                  ? handleScanCodex
                  : activeSource === 'antigravity'
                  ? handleScanAntigravity
                  : handleScanClaude
              }
              isScanning={scanning !== null}
              onPublish={
                activeSource === 'codex'
                  ? handlePublishToCodex
                  : activeSource === 'antigravity'
                  ? handleOpenPublishAntigravity
                  : handleOpenPublishClaude
              }
              onOpenClient={() => handleOpenClient()}
              isOpeningClient={isOpeningClient}
              onClearDatabase={handleOpenReset}
              isClearingDatabase={isPreparingReset || isResetting || scanning !== null}
              onOpenImport={() => setImportModalOpen(true)}
              onBackup={handleBackup}
              showArchived={showArchived}
              onShowArchivedChange={setShowArchived}
            />
            <div className="flex-1 flex overflow-hidden">
              <Sidebar
                width={sidebarWidth}
                conversations={displayedConversations}
                selectedId={selectedId}
                onSelectConversation={setSelectedId}
                activeSource={activeSource}
                publishSelection={publishSelection}
                onTogglePublishSelection={togglePublishSelection}
                onToggleProjectPublishSelection={toggleProjectPublishSelection}
                onClearSelection={handleClearSelection}
                onBatchExportMarkdown={handleBatchExportMarkdown}
                onBatchDelete={handleBatchDelete}
                onOpenFolder={handleOpenFolder}
                onOpenVSCode={handleOpenVSCode}
                onOpenPrismProject={handleOpenPrismProject}
                onReadConversationInPrism={handleReadConversationInPrism}
                onHandoff={(conv) => setHandoffConv(conv)}
                onRename={handleRename}
                onDelete={handleDeleteSingle}
                onSetPinned={handleSetPinned}
                folderPins={folderPins}
                onSetFolderPinned={handleSetFolderPinned}
                searchQuery={searchQuery}
                onSearchChange={handleSearchChange}
                codexVisibility={codexVisibility}
              />
              {/* Resizable Divider Handle */}
              <div
                onMouseDown={startResizing}
                onDoubleClick={resetSidebarWidth}
                title={t.sidebar.resizerTooltip}
                className={`sidebar-resizer ${
                  isResizing ? 'is-resizing' : ''
                } bg-[#f8fafc] dark:bg-[#121927] border-r border-slate-200/85 dark:border-slate-800/85 hover:bg-blue-600 transition-colors`}
              />
              <ChatView
                conversation={activeConversation}
                messages={messages}
                isLoading={isLoadingMessages}
                onRename={handleRename}
                onDelete={handleDeleteSingle}
                onExportMarkdown={handleExportMarkdown}
                onExportJson={handleExportJson}
                onOpenFolder={handleOpenFolder}
                onOpenVSCode={handleOpenVSCode}
                onOpenPrismProject={handleOpenPrismProject}
                onOpenFileInPrism={handleOpenFileInPrism}
                onSendCodeToPrism={handleSendCodeToPrism}
                onReadConversationInPrism={handleReadConversationInPrism}
                onHandoff={(conv) =>
                  setHandoffConv({
                    ...conv,
                    message_count: messages.length,
                  })
                }
                onSelectConversation={handleJumpConversationLink}
                onOpenExternalUrl={handleOpenExternalUrl}
              />
            </div>
          </>
        )}
        {page === 'skills' && <SkillsManager onToast={addToast} />}
        {page === 'mcp' && <McpManager onToast={addToast} />}
        {page === 'accounts' && <AccountsManager onToast={addToast} />}
      </div>


      {/* Codex Recovery Modal */}
      <CodexRecoveryModal
        isOpen={recoveryModalOpen}
        onClose={() => setRecoveryModalOpen(false)}
        report={recoveryReport}
        onConfirmRecovery={handleConfirmRecovery}
        isRecovering={isRecovering}
      />

      {/* Antigravity Recovery Modal */}
      <AntigravityRecoveryModal
        isOpen={antigravityModalOpen}
        onClose={() => setAntigravityModalOpen(false)}
        report={antigravityReport}
        onConfirmRecovery={handleConfirmAntigravityRecovery}
        isRecovering={isRecoveringAntigravity}
      />

      {/* Antigravity Publish / Sync Modal */}
      <AntigravityPublishModal
        isOpen={antigravityPublishModalOpen}
        onClose={() => setAntigravityPublishModalOpen(false)}
        preview={antigravityPublishPreview}
        onConfirmPublish={handleConfirmPublishAntigravity}
        isPublishing={isPublishingAntigravity}
      />

      {/* Claude Recovery Modal */}
      <ClaudeRecoveryModal
        isOpen={claudeModalOpen}
        onClose={() => setClaudeModalOpen(false)}
        report={claudeReport}
        onConfirmRecovery={handleConfirmClaudeRecovery}
        isRecovering={isRecoveringClaude}
      />

      {/* Claude Publish Modal */}
      <ClaudePublishModal
        isOpen={claudePublishModalOpen}
        onClose={() => setClaudePublishModalOpen(false)}
        preview={claudePublishPreview}
        onConfirmPublish={handleConfirmPublishClaude}
        isPublishing={isPublishingClaude}
      />


      {/* Import Modal */}
      <ImportModal
        isOpen={importModalOpen}
        onClose={() => setImportModalOpen(false)}
        onPickFile={() => api.pickOpenPath()}
        onPreview={api.previewImport}
        onExecuteImport={async (path) => {
          const res = await api.executeImport(path);
          addToast(
            'success',
            `导入成功：新增 ${res.new_conversation_count} 个会话，共 ${res.message_count} 条消息`
          );
          if (res.warning_count > 0) addToast('warning', res.warnings.slice(0, 3).join('；'));
          await reloadConversations(undefined, undefined, true);
        }}
      />

      {/* Codex Publish Confirmation */}
      {codexPublishDialog && (
        <Modal
          isOpen={true}
          onClose={() => !isPublishingCodex && setCodexPublishDialog(null)}
          title={t.modals.codexPublishTitle}
          widthClass="max-w-md"
        >
          <div className="flex flex-col gap-4 text-xs">
            <div className="grid grid-cols-3 gap-2.5">
              {(
                [
                  [t.modals.codexPublishSelected, codexPublishDialog.preview.selected_thread_count, 'text-blue-600 dark:text-sky-400'],
                  [t.modals.codexPublishHidden, codexPublishDialog.preview.hidden_thread_count, 'text-amber-600 dark:text-amber-400'],
                  [t.modals.codexPublishProjects, codexPublishDialog.preview.project_count, 'text-slate-900 dark:text-white'],
                ] as const
              ).map(([label, value, color]) => (
                <div
                  key={label}
                  className="p-3 rounded-2xl bg-slate-50 dark:bg-slate-800/50 border border-slate-200/80 dark:border-slate-700/70"
                >
                  <div className={`text-xl font-semibold ${color}`}>{value}</div>
                  <div className="text-[10.5px] text-slate-500 dark:text-slate-400 mt-1 leading-snug">{label}</div>
                </div>
              ))}
            </div>
            <p className="text-[11.5px] text-slate-500 dark:text-slate-400 leading-relaxed">
              {t.modals.codexPublishNote}
            </p>
            <div className="flex items-center justify-end gap-2 pt-1">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setCodexPublishDialog(null)}
                disabled={isPublishingCodex}
              >
                {t.common.cancel}
              </Button>
              <Button
                variant="primary"
                size="sm"
                onClick={executePublishToCodex}
                disabled={isPublishingCodex}
                icon={isPublishingCodex ? <RefreshCw size={13} className="animate-spin" /> : undefined}
              >
                {t.modals.codexPublishConfirm}
              </Button>
            </div>
          </div>
        </Modal>
      )}

      {/* Vault Reset */}
      {resetDialog && (
          <Modal
            isOpen={true}
            onClose={() => !isResetting && setResetDialog(null)}
            title={t.modals.resetTitle}
            widthClass="max-w-md"
          >
            <div className="flex flex-col gap-4 text-xs">
              <p className="text-slate-800 dark:text-slate-200 leading-relaxed">{t.modals.resetIntro}</p>
              <div className="p-4 rounded-2xl border border-rose-200 dark:border-rose-500/30 bg-rose-50/70 dark:bg-rose-500/10 space-y-2">
                <p className="font-medium text-rose-700 dark:text-rose-300">
                  {interpolate(t.modals.resetScopeAll, {
                    convs: resetDialog.conversation_count,
                    msgs: resetDialog.message_count,
                  })}
                </p>
                <p className="text-[11px] text-slate-600 dark:text-slate-400">
                  {interpolate(t.modals.resetClearExclusions, { count: resetDialog.exclusion_count })}
                </p>
                {(resetDialog.folder_pin_count ?? 0) > 0 && (
                  <p className="text-[11px] text-slate-600 dark:text-slate-400">
                    {interpolate(t.modals.resetFolderPins, { count: resetDialog.folder_pin_count ?? 0 })}
                  </p>
                )}
              </div>
              <p className="text-[11px] text-slate-500 dark:text-slate-400 bg-slate-50 dark:bg-slate-800/50 border border-slate-200/70 dark:border-slate-700/60 p-3 rounded-2xl leading-relaxed">
                {t.modals.resetBackupNote}
              </p>
              <div className="flex items-center justify-end gap-2 pt-1">
                <Button variant="ghost" size="sm" onClick={() => setResetDialog(null)} disabled={isResetting}>
                  {t.common.cancel}
                </Button>
                <Button
                  variant="danger"
                  size="sm"
                  onClick={executeReset}
                  disabled={
                    isResetting ||
                    (resetDialog.conversation_count === 0 && resetDialog.exclusion_count === 0 && !resetDialog.folder_pin_count)
                  }
                  icon={isResetting ? <RefreshCw size={13} className="animate-spin" /> : undefined}
                >
                  {isResetting ? t.modals.resetting : t.modals.resetConfirm}
                </Button>
              </div>
            </div>
          </Modal>
      )}

      {/* Rename Dialog */}
      {renameDialog && (
        <Modal
          isOpen={true}
          onClose={() => setRenameDialog(null)}
          title={t.modals.renameTitle}
          widthClass="max-w-md"
        >
          <form
            onSubmit={(e) => {
              e.preventDefault();
              executeRename();
            }}
            className="flex flex-col gap-4 text-xs"
          >
            <div>
              <label className="block font-semibold text-slate-500 dark:text-slate-400 mb-2">{t.modals.newTitleLabel}</label>
              <input
                type="text"
                autoFocus
                value={renameDialog.newTitle}
                onChange={(e) =>
                  setRenameDialog({ ...renameDialog, newTitle: e.target.value })
                }
                className="w-full bg-white dark:bg-[#101725] text-slate-900 dark:text-slate-100 text-xs px-3.5 py-2.5 rounded-xl border border-slate-300/90 dark:border-slate-700 focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15 focus:outline-none placeholder-slate-400 transition-all"
              />
            </div>
            <div className="flex items-center justify-end gap-2 pt-2">
              <Button variant="ghost" size="sm" onClick={() => setRenameDialog(null)}>
                {t.common.cancel}
              </Button>
              <Button variant="primary" size="sm" type="submit">
                {t.common.save}
              </Button>
            </div>
          </form>
        </Modal>
      )}

      {/* Delete Confirm Dialog */}
      {deleteDialog && (
        <Modal
          isOpen={true}
          onClose={() => setDeleteDialog(null)}
          title={t.modals.deleteTitle}
          widthClass="max-w-md"
        >
          <div className="flex flex-col gap-4 text-xs">
            <p className="text-slate-800 dark:text-slate-200 leading-relaxed break-words">{deleteDialog.title}</p>
            <p className="text-[11px] text-rose-600 dark:text-rose-400 bg-rose-50 dark:bg-rose-500/10 p-3.5 rounded-2xl border border-rose-200 dark:border-rose-500/30 leading-relaxed">
              {t.modals.deleteWarning}
            </p>
            <div className="flex items-center justify-end gap-2 pt-2">
              <Button variant="ghost" size="sm" onClick={() => setDeleteDialog(null)}>
                {t.common.cancel}
              </Button>
              <Button variant="danger" size="sm" onClick={executeDelete}>
                {t.common.confirmDelete}
              </Button>
            </div>
          </div>
        </Modal>
      )}

      {/* Quick Launch Modal */}
      <QuickLaunchModal
        isOpen={quickLaunchOpen}
        onClose={() => setQuickLaunchOpen(false)}
        contextFolder={activeConversation?.project_path ?? null}
        onToast={addToast}
      />

      {/* Cross-Tool Conversation Handoff Modal */}
      {handoffConv && (
        <ConversationHandoffModal
          isOpen
          conversation={handoffConv}
          onClose={() => setHandoffConv(null)}
          onCompleted={(syncedCount) => {
            if (syncedCount > 0) {
              void reloadConversations(handoffConv.id, undefined, true);
            }
          }}
          onToast={addToast}
        />
      )}

      {/* Global Toast Container */}
      <ToastContainer toasts={toasts} onDismiss={dismissToast} />
    </div>
  );
};
