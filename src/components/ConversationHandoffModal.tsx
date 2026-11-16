import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertCircle,
  ArrowRightLeft,
  Bot,
  Braces,
  Check,
  Code2,
  Copy,
  Cpu,
  FileText,
  FolderOpen,
  FolderSearch,
  Layers,
  Loader2,
  LucideIcon,
  Play,
  RefreshCw,
  Rocket,
  RotateCcw,
  Save,
  Sparkles,
  Terminal,
  TerminalSquare,
  X,
} from 'lucide-react';

import { api, errorMessage } from '../api/client';
import { useI18n } from '../i18n';
import {
  AccountClient,
  AccountsState,
  AccountView,
  HandoffMode,
  HandoffResult,
  QuickLaunchTerminal,
  QuickLaunchTool,
} from '../types';
import { Button } from './common/Button';
import { Modal } from './common/Modal';

function folderBaseName(dir: string): string {
  const parts = dir.replace(/[\\/]+$/, '').split(/[\\/]/);
  return parts[parts.length - 1] || dir;
}

export interface HandoffConversationTarget {
  id: string;
  title: string;
  source_kind: string;
  project_path?: string | null;
  message_count?: number;
}

export interface ConversationHandoffModalProps {
  isOpen: boolean;
  conversation: HandoffConversationTarget;
  onClose: () => void;
  onCompleted: (syncedNewMessages: number) => void;
  onToast: (type: 'success' | 'warning' | 'error' | 'info', message: string) => void;
}

const ACCOUNT_LAUNCH_TERMINAL_KEY = 'cv_account_launch_terminal';
const QUICK_LAUNCH_TOOLS_EVENT = 'cv-quick-launch-updated';

const cliCmdStorageKey = (toolId: string) => `cv_account_cli_cmd_${toolId}`;

const CLIENT_OPTIONS: { id: AccountClient; label: string; dotColor: string }[] = [
  { id: 'codex', label: 'Codex', dotColor: '#2563eb' },
  { id: 'claude', label: 'Claude', dotColor: '#d97706' },
  { id: 'antigravity', label: 'Antigravity', dotColor: '#8b5cf6' },
];

const CLIENT_TARGET_MAP: Record<
  AccountClient,
  { cliId: string; desktopId: string; editorIds: string[] }
> = {
  codex: {
    cliId: 'codex_cli',
    desktopId: 'codex_app',
    editorIds: ['vscode', 'cursor', 'windsurf', 'trae'],
  },
  claude: {
    cliId: 'claude_cli',
    desktopId: 'claude_app',
    editorIds: ['vscode', 'cursor', 'windsurf', 'trae'],
  },
  antigravity: {
    cliId: 'antigravity_cli',
    desktopId: 'antigravity_app',
    editorIds: ['vscode', 'cursor', 'windsurf', 'trae'],
  },
};

const FALLBACK_TOOLS: QuickLaunchTool[] = [
  {
    id: 'codex_cli',
    name: 'Codex CLI',
    category: 'ai_cli',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'claude_cli',
    name: 'Claude Code',
    category: 'ai_cli',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'antigravity_cli',
    name: 'Antigravity CLI',
    category: 'ai_cli',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'codex_app',
    name: 'Codex Desktop',
    category: 'ai_app',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'claude_app',
    name: 'Claude Desktop',
    category: 'ai_app',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'antigravity_app',
    name: 'Antigravity',
    category: 'ai_app',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'vscode',
    name: 'VS Code',
    category: 'editor',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'cursor',
    name: 'Cursor',
    category: 'editor',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'windsurf',
    name: 'Windsurf',
    category: 'editor',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'trae',
    name: 'Trae',
    category: 'editor',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
];

const TOOL_META: Record<string, { icon: LucideIcon }> = {
  codex_app: { icon: Sparkles },
  claude_app: { icon: Bot },
  antigravity_app: { icon: Rocket },
  codex_cli: { icon: TerminalSquare },
  claude_cli: { icon: TerminalSquare },
  antigravity_cli: { icon: Cpu },
  vscode: { icon: Code2 },
  cursor: { icon: Braces },
  windsurf: { icon: Layers },
  trae: { icon: Code2 },
};

const TERMINAL_OPTIONS: QuickLaunchTerminal[] = [
  'system',
  'wt',
  'powershell',
  'pwsh',
  'cmd',
  'git_bash',
];

const RECENT_LIMIT_OPTIONS = [6, 10, 20, 0];

function sourceKindToClient(sourceKind: string): AccountClient | null {
  if (sourceKind === 'codex_main') return 'codex';
  if (sourceKind === 'claude') return 'claude';
  if (sourceKind === 'antigravity') return 'antigravity';
  return null;
}

function defaultTargetClient(sourceKind: string): AccountClient {
  if (sourceKind === 'codex_main') return 'claude';
  if (sourceKind === 'claude') return 'codex';
  if (sourceKind === 'antigravity') return 'codex';
  return 'claude';
}

function loadSavedTerminal(): QuickLaunchTerminal {
  try {
    const val = localStorage.getItem(ACCOUNT_LAUNCH_TERMINAL_KEY) as QuickLaunchTerminal | null;
    if (val && TERMINAL_OPTIONS.includes(val)) return val;
  } catch {
    // ignore storage errors
  }
  return 'system';
}

function stripLeadingCliName(rawInput: string): string {
  const trimmed = rawInput.trim();
  if (!trimmed) return '';
  for (const prefix of [
    'codex.exe',
    'codex.cmd',
    'codex',
    'claude.exe',
    'claude.cmd',
    'claude',
    'agy.exe',
    'agy.cmd',
    'agy',
  ]) {
    if (trimmed.toLowerCase() === prefix) return '';
    if (
      trimmed.slice(0, prefix.length).toLowerCase() === prefix &&
      /^\s/.test(trimmed.slice(prefix.length))
    ) {
      return trimmed.slice(prefix.length).trim();
    }
  }
  return trimmed;
}

function buildHandoffCliPreview(
  targetClient: AccountClient,
  mode: HandoffMode,
  extraCliArgs: string,
  folderPath: string,
): string {
  const cleanFlags = stripLeadingCliName(extraCliArgs);
  const bin =
    targetClient === 'codex' ? 'codex' : targetClient === 'claude' ? 'claude' : 'agy';
  const flagsPart = cleanFlags ? `${cleanFlags} ` : '';

  let cmd: string;
  if (targetClient === 'antigravity') {
    const prompt =
      '请先阅读 .context-vault/handoff-<id>.md 了解前序会话上下文与当前代码进度，然后继续推进其中的下一步任务。';
    cmd = `${bin} --project <project-id> ${flagsPart}-i "${prompt}"`;
  } else if (mode === 'native') {
    cmd =
      targetClient === 'codex'
        ? `${bin} resume ${flagsPart}<new-session-id>`
        : `${bin} ${flagsPart}--resume <new-session-id>`;
  } else {
    const prompt =
      '请先阅读 .context-vault/handoff-<id>.md 了解前序会话上下文与当前代码进度，然后继续推进其中的下一步任务。';
    cmd = `${bin} ${flagsPart}"${prompt}"`;
  }

  return folderPath.trim()
    ? `[${folderPath.trim()}] $ ${cmd}`
    : `[ContextVault-Workspaces/<auto>] $ ${cmd}`;
}

function accountPrimaryRemaining(acc: AccountView): number | null {
  const windows = acc.quota?.windows;
  if (!windows || windows.length === 0) return null;
  const remaining = windows.map((w) => Math.max(0, Math.round(100 - w.used_percent)));
  return Math.min(...remaining);
}

function formatAccountOptionLabel(
  acc: AccountView,
  currentLabel: string,
  quotaFmt: (pct: number) => string,
): string {
  const identity = acc.email || acc.name || acc.account_id || acc.id;
  const tags: string[] = [];
  if (acc.plan_type) tags.push(acc.plan_type);
  const rem = accountPrimaryRemaining(acc);
  if (rem !== null) tags.push(quotaFmt(rem));
  if (acc.is_current) tags.push(currentLabel);
  return tags.length > 0 ? `${identity} (${tags.join(' · ')})` : identity;
}

export const ConversationHandoffModal: React.FC<ConversationHandoffModalProps> = ({
  isOpen,
  conversation,
  onClose,
  onCompleted,
  onToast,
}) => {
  const { t, interpolate } = useI18n();
  const h = t.handoff;
  const q = t.quickLaunch;
  const am = q.accountModal;

  const sourceClient = sourceKindToClient(conversation.source_kind);
  const [targetClient, setTargetClient] = useState<AccountClient>(() =>
    defaultTargetClient(conversation.source_kind),
  );
  const [mode, setMode] = useState<HandoffMode>('native');
  const [recentLimit, setRecentLimit] = useState<number>(10);
  const [followUpPrompt, setFollowUpPrompt] = useState<string>('');
  const [syncLatestFirst, setSyncLatestFirst] = useState<boolean>(true);
  const [restartDesktop, setRestartDesktop] = useState<boolean>(true);

  const [tools, setTools] = useState<QuickLaunchTool[]>(FALLBACK_TOOLS);
  const [selectedToolId, setSelectedToolId] = useState<string>(
    CLIENT_TARGET_MAP[defaultTargetClient(conversation.source_kind)].cliId,
  );
  const [accountsState, setAccountsState] = useState<AccountsState | null>(null);
  const [selectedAccountId, setSelectedAccountId] = useState<string>('');
  const [folderPath, setFolderPath] = useState<string>('');
  const [terminal, setTerminal] = useState<QuickLaunchTerminal>(loadSavedTerminal);
  const [cliCommand, setCliCommand] = useState<string>('');
  const [pathDraft, setPathDraft] = useState<string>('');
  const [busyAction, setBusyAction] = useState<'prepare' | 'launch' | 'path' | null>(null);
  const [result, setResult] = useState<{
    res: HandoffResult;
    toolId: string;
    toolName: string;
  } | null>(null);

  const userPickedToolRef = useRef(false);

  // Reset modal defaults cleanly when opened for a conversation
  useEffect(() => {
    if (!isOpen) return;
    const defClient = defaultTargetClient(conversation.source_kind);
    userPickedToolRef.current = false;
    setTargetClient(defClient);
    setSelectedToolId(CLIENT_TARGET_MAP[defClient].cliId);
    setSelectedAccountId('');
    setMode('native');
    setRecentLimit(10);
    setFollowUpPrompt('');
    setSyncLatestFirst(true);
    setResult(null);
    setFolderPath(conversation.project_path?.trim() || '');
  }, [isOpen, conversation.id, conversation.source_kind, conversation.project_path]);

  // Load quick launch tools
  const loadTools = useCallback(async () => {
    if (typeof api.listQuickLaunchTools !== 'function') return;
    try {
      const list = await api.listQuickLaunchTools();
      if (Array.isArray(list) && list.length > 0) {
        setTools(list);
      }
    } catch {
      // Keep fallback tools
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    void loadTools();
  }, [isOpen, loadTools]);

  // Load saved accounts for targetClient
  useEffect(() => {
    if (!isOpen || typeof api.listAccounts !== 'function') {
      setAccountsState(null);
      return;
    }
    let cancelled = false;
    setSelectedAccountId('');
    void api
      .listAccounts(targetClient)
      .then((st) => {
        if (!cancelled) setAccountsState(st);
      })
      .catch(() => {
        if (!cancelled) setAccountsState(null);
      });
    return () => {
      cancelled = true;
    };
  }, [isOpen, targetClient]);

  const byId = useMemo(() => new Map(tools.map((item) => [item.id, item])), [tools]);
  const clientIds = CLIENT_TARGET_MAP[targetClient];
  const cliTool = byId.get(clientIds.cliId);
  const desktopTool = byId.get(clientIds.desktopId);
  const editorTools = useMemo(
    () =>
      clientIds.editorIds
        .map((id) => byId.get(id))
        .filter((item): item is QuickLaunchTool => Boolean(item)),
    [byId, clientIds.editorIds],
  );

  // Pick CLI by default when targetClient switches (unless user explicitly picked Editor/Desktop)
  useEffect(() => {
    if (!isOpen || userPickedToolRef.current) return;
    setSelectedToolId(clientIds.cliId);
  }, [isOpen, targetClient, clientIds]);

  const selectedTool = useMemo(
    () => byId.get(selectedToolId) ?? cliTool ?? desktopTool ?? editorTools[0],
    [byId, selectedToolId, cliTool, desktopTool, editorTools],
  );

  // Sync path draft and remembered extra CLI flags
  useEffect(() => {
    if (!selectedTool) return;
    setPathDraft(selectedTool.custom_path ?? selectedTool.detected_path ?? '');
    // Only Codex desktop must be restarted: it keeps its sidebar state in memory and would
    // overwrite the newly registered workspace. Restarting Antigravity/Claude would only
    // interrupt the conversations the user currently has open.
    setRestartDesktop(selectedTool.id === 'codex_app');
    if (selectedTool.category === 'ai_cli') {
      try {
        setCliCommand(localStorage.getItem(cliCmdStorageKey(selectedTool.id)) ?? '');
      } catch {
        setCliCommand('');
      }
    } else {
      setCliCommand('');
    }
  }, [selectedTool]);

  const handleSelectClient = (nextClient: AccountClient) => {
    userPickedToolRef.current = false;
    setTargetClient(nextClient);
    setSelectedToolId(CLIENT_TARGET_MAP[nextClient].cliId);
  };

  const handleSelectTool = (toolId: string) => {
    userPickedToolRef.current = true;
    setSelectedToolId(toolId);
  };

  const persistTerminal = (next: QuickLaunchTerminal) => {
    setTerminal(next);
    try {
      localStorage.setItem(ACCOUNT_LAUNCH_TERMINAL_KEY, next);
    } catch {
      // ignore
    }
  };

  const persistCliCommand = (next: string) => {
    setCliCommand(next);
    if (!selectedTool) return;
    try {
      localStorage.setItem(cliCmdStorageKey(selectedTool.id), next);
    } catch {
      // ignore
    }
  };

  const isCli = selectedTool?.category === 'ai_cli';
  const isDesktop = selectedTool?.category === 'ai_app';
  const resolvedSavedPath = selectedTool?.custom_path ?? selectedTool?.detected_path ?? '';
  const pathDirty = Boolean(selectedTool && pathDraft.trim() !== resolvedSavedPath.trim());

  const saveCustomPath = async (nextPath: string | null) => {
    if (!selectedTool || typeof api.setQuickLaunchCustomPath !== 'function') return;
    setBusyAction('path');
    try {
      const updated = await api.setQuickLaunchCustomPath(selectedTool.id, nextPath);
      setTools(updated);
      window.dispatchEvent(new CustomEvent(QUICK_LAUNCH_TOOLS_EVENT, { detail: updated }));
      onToast(
        'success',
        interpolate(nextPath ? q.pathUpdated : q.pathReset, { name: selectedTool.name }),
      );
    } catch (err) {
      onToast('error', interpolate(q.pathUpdateFailed, { error: errorMessage(err) }));
    } finally {
      setBusyAction(null);
    }
  };

  const handleBrowseExe = async () => {
    if (!selectedTool || typeof api.pickExecutablePath !== 'function') return;
    const picked = await api.pickExecutablePath(
      pathDraft.trim() || selectedTool.detected_path || undefined,
    );
    if (!picked) return;
    setPathDraft(picked);
    await saveCustomPath(picked);
  };

  const handleBrowseFolder = async () => {
    const picked = await api.pickFolderPath(folderPath.trim() || undefined);
    if (picked) {
      setFolderPath(picked);
    }
  };

  const cliPreview = useMemo(
    () => buildHandoffCliPreview(targetClient, mode, cliCommand, folderPath),
    [targetClient, mode, cliCommand, folderPath],
  );

  const runHandoff = async (prepareOnly: boolean) => {
    if (!selectedTool) return;
    setBusyAction(prepareOnly ? 'prepare' : 'launch');
    try {
      if (!prepareOnly && pathDirty && typeof api.setQuickLaunchCustomPath === 'function') {
        const trimmed = pathDraft.trim();
        const updated = await api.setQuickLaunchCustomPath(
          selectedTool.id,
          trimmed || null,
        );
        setTools(updated);
        window.dispatchEvent(new CustomEvent(QUICK_LAUNCH_TOOLS_EVENT, { detail: updated }));
      }

      const res = await api.handoffConversation({
        conversationId: conversation.id,
        targetClient,
        targetAccountId: selectedAccountId || null,
        mode,
        toolId: selectedTool.id,
        folderPath: folderPath.trim() || null,
        terminal: isCli ? terminal : null,
        extraCliArgs: isCli ? cliCommand.trim() || null : null,
        recentMessageLimit: mode === 'compact' ? recentLimit : null,
        followUpPrompt: mode === 'compact' ? followUpPrompt.trim() || null : null,
        syncLatestFirst,
        prepareOnly,
        restartDesktopIfRunning: isDesktop ? restartDesktop : null,
      });

      const needsManualPaste =
        !isCli && Boolean(res.handoffFilePath) && !res.deepLinkUri;
      const copiedPrompt =
        prepareOnly || needsManualPaste || (!isCli && mode === 'compact');
      if (copiedPrompt && typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
        const textToCopy = isCli && prepareOnly ? res.cliCommand : res.clipboardPrompt;
        if (textToCopy) {
          await navigator.clipboard.writeText(textToCopy).catch(() => {});
        }
      }

      const syncSuffix =
        res.syncedNewMessages > 0
          ? interpolate(h.toastSyncedSuffix, { count: res.syncedNewMessages })
          : '';
      const workspaceSuffix = res.workingDir
        ? interpolate(
            res.workspaceCreated ? h.toastWorkspaceCreatedSuffix : h.toastWorkspaceReusedSuffix,
            { dir: res.workingDir },
          )
        : '';
      const baseToast = prepareOnly
        ? interpolate(h.toastPrepared, { target: selectedTool.name })
        : mode === 'native'
          ? `${interpolate(h.toastLaunchedNative, { target: selectedTool.name })}${needsManualPaste ? h.toastPromptCopiedHint : ''}`
          : `${interpolate(h.toastLaunchedCompact, { target: selectedTool.name })}${needsManualPaste ? h.toastPromptCopiedHint : ''}`;

      onToast('success', `${baseToast}${workspaceSuffix}${syncSuffix}`);
      if (res.warnings.length > 0) {
        onToast('warning', res.warnings[0]);
      }

      onCompleted(res.syncedNewMessages);
      if (prepareOnly) {
        onClose();
      } else {
        setResult({ res, toolId: selectedTool.id, toolName: selectedTool.name });
      }
    } catch (err) {
      onToast('error', errorMessage(err));
    } finally {
      setBusyAction(null);
    }
  };

  const copyText = async (text: string) => {
    if (!text || typeof navigator === 'undefined' || !navigator.clipboard?.writeText) return;
    await navigator.clipboard.writeText(text).catch(() => {});
    onToast('success', h.promptCopiedToast);
  };

  const launchAgyInstead = async (res: HandoffResult) => {
    if (!res.workingDir || typeof api.launchQuickTool !== 'function') return;
    try {
      await api.launchQuickTool('antigravity_cli', res.workingDir, terminal, res.cliCommand);
      onToast('success', interpolate(h.agyLaunchedToast, { folder: folderBaseName(res.workingDir) }));
    } catch (err) {
      onToast('error', errorMessage(err));
    }
  };

  const renderResult = () => {
    if (!result) return null;
    const { res, toolId, toolName } = result;
    const folder = folderBaseName(res.workingDir ?? '');
    const isAgDesktop = toolId === 'antigravity_app';
    const isEditorTool = ['vscode', 'cursor', 'windsurf', 'trae'].includes(toolId);
    const isCliTool = toolId.endsWith('_cli');

    let steps: string[];
    if (!res.launched) {
      steps = [h.stepPreparedOnly];
    } else if (res.deepLinkUri) {
      steps = [
        res.mode === 'native'
          ? interpolate(h.stepDeepLinkNative, { target: toolName, title: res.sessionTitle })
          : interpolate(h.stepDeepLinkCompact, { target: toolName, folder }),
      ];
    } else if (isCliTool) {
      steps = [interpolate(h.stepCli, { target: toolName, folder })];
    } else if (isAgDesktop) {
      steps = [
        interpolate(h.stepAgDesktop1, { folder }),
        interpolate(h.stepAgDesktop2, { folder }),
        h.stepAgDesktop3,
      ];
    } else if (isEditorTool) {
      steps = [interpolate(h.stepEditor1, { target: toolName, folder }), h.stepEditor2];
    } else {
      steps = [interpolate(h.stepCli, { target: toolName, folder })];
    }

    const infoRow = (label: string, value: React.ReactNode) => (
      <div className="grid grid-cols-[92px_1fr] gap-3 items-start py-2 border-b last:border-b-0 border-slate-100 dark:border-slate-800">
        <span className="text-[12px] font-medium text-slate-500 dark:text-slate-400">{label}</span>
        <div className="min-w-0 text-[12.5px] text-slate-900 dark:text-slate-100 break-all">{value}</div>
      </div>
    );

    return (
      <div className="flex flex-col gap-4 text-sm">
        <div className="flex items-center gap-2.5 px-4 py-3 rounded-xl bg-emerald-50 dark:bg-emerald-500/10 border border-emerald-200 dark:border-emerald-500/30">
          <span className="w-7 h-7 rounded-full bg-emerald-600 text-white flex items-center justify-center shrink-0">
            <Check size={15} />
          </span>
          <span className="font-semibold text-[14px] text-emerald-800 dark:text-emerald-200">
            {h.resultTitle}
          </span>
        </div>

        <div className="rounded-xl border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0f1624] px-4 py-1">
          {res.workingDir &&
            infoRow(
              h.resultWorkspace,
              <div className="flex items-center gap-2 flex-wrap">
                <span className="font-mono">{res.workingDir}</span>
                <span
                  className={`px-2 py-0.5 rounded-full text-[11px] font-semibold ${
                    res.workspaceCreated
                      ? 'bg-blue-100 text-blue-700 dark:bg-blue-500/20 dark:text-blue-200'
                      : 'bg-slate-100 text-slate-700 dark:bg-slate-700 dark:text-slate-200'
                  }`}
                >
                  {res.workspaceCreated ? h.resultWorkspaceCreated : h.resultWorkspaceReused}
                </span>
              </div>,
            )}
          {infoRow(
            h.resultSession,
            <span className="font-semibold">
              {res.sessionTitle}
              {res.targetSessionId && (
                <span className="ml-2 font-mono text-[11px] font-normal text-slate-500 dark:text-slate-400">
                  {res.targetSessionId}
                </span>
              )}
            </span>,
          )}
          {res.handoffFilePath &&
            infoRow(h.resultHandoffFile, <span className="font-mono">{res.handoffFilePath}</span>)}
        </div>

        <div className="flex flex-col gap-2">
          <span className="font-semibold text-[13px] text-slate-800 dark:text-slate-100">
            {h.resultStepsTitle}
          </span>
          <ol className="flex flex-col gap-2">
            {steps.map((step, idx) => (
              <li key={idx} className="flex items-start gap-2.5">
                <span className="w-5 h-5 mt-px rounded-full bg-blue-600 text-white text-[11px] font-bold flex items-center justify-center shrink-0">
                  {idx + 1}
                </span>
                <span className="text-[13px] leading-relaxed text-slate-800 dark:text-slate-100">
                  {step}
                </span>
              </li>
            ))}
          </ol>
          {isAgDesktop && res.launched && (
            <div className="flex items-start gap-2 mt-1 px-3 py-2 rounded-lg bg-amber-50 dark:bg-amber-500/10 border border-amber-200 dark:border-amber-500/30 text-[12px] text-amber-800 dark:text-amber-200">
              <AlertCircle size={14} className="mt-px shrink-0" />
              <span>{h.agDesktopWarning}</span>
            </div>
          )}
        </div>

        {res.clipboardPrompt && !res.clipboardPrompt.startsWith(res.cliCommand) && (
          <pre className="px-3 py-2 rounded-lg bg-slate-100 dark:bg-black/40 border border-slate-200 dark:border-slate-700 text-slate-800 dark:text-slate-100 font-mono text-[12px] whitespace-pre-wrap break-all">
            {res.clipboardPrompt}
          </pre>
        )}

        <div className="flex items-center justify-end gap-2 pt-3 border-t border-slate-200 dark:border-slate-800 flex-wrap">
          {res.workingDir && (
            <Button
              size="sm"
              variant="secondary"
              icon={<FolderOpen size={13} />}
              onClick={() => void api.openFolder(res.workingDir!)}
            >
              {h.openFolderBtn}
            </Button>
          )}
          {isAgDesktop && res.workingDir && (
            <Button
              size="sm"
              variant="secondary"
              icon={<TerminalSquare size={13} />}
              onClick={() => void launchAgyInstead(res)}
            >
              {h.useAgyCliBtn}
            </Button>
          )}
          {res.clipboardPrompt && (
            <Button
              size="sm"
              variant="secondary"
              icon={<Copy size={13} />}
              onClick={() => void copyText(res.clipboardPrompt)}
            >
              {h.copyPromptBtn}
            </Button>
          )}
          <Button size="sm" variant="primary" icon={<Check size={13} />} onClick={onClose}>
            {h.doneBtn}
          </Button>
        </div>
      </div>
    );
  };

  const renderTargetOption = (tool: QuickLaunchTool) => {
    const active = selectedTool?.id === tool.id;
    const Icon = TOOL_META[tool.id]?.icon ?? Terminal;
    return (
      <button
        key={tool.id}
        type="button"
        onClick={() => handleSelectTool(tool.id)}
        aria-pressed={active}
        className={`group flex items-center justify-between gap-2 px-3 py-2 rounded-xl border text-left transition-all ${
          active
            ? 'border-blue-500/80 dark:border-sky-400/75 bg-blue-50/90 dark:bg-sky-500/12 text-blue-700 dark:text-sky-200 shadow-xs ring-1 ring-inset ring-blue-500/20'
            : 'border-slate-200/85 dark:border-slate-800 bg-white dark:bg-[#0f1624] text-slate-700 dark:text-slate-200 hover:border-slate-300 dark:hover:border-slate-700'
        }`}
      >
        <div className="flex items-center gap-2 min-w-0">
          <span
            className={`w-6 h-6 rounded-lg flex items-center justify-center shrink-0 ${
              active
                ? 'bg-blue-600 text-white dark:bg-sky-500 dark:text-slate-950'
                : 'bg-slate-100 dark:bg-slate-800 text-slate-500 dark:text-slate-400'
            }`}
          >
            <Icon size={13} />
          </span>
          <div className="min-w-0">
            <div className="text-[11.5px] font-semibold truncate">{tool.name}</div>
            <div className="text-[10px] text-slate-400 dark:text-slate-500">
              {tool.running ? (
                <span className="text-emerald-600 dark:text-emerald-400 inline-flex items-center gap-1">
                  <span className="cv-live-dot" />
                  {q.runningBadge}
                </span>
              ) : tool.available ? (
                <span className="text-emerald-600/85 dark:text-emerald-400/85">
                  {q.installedBadge}
                </span>
              ) : (
                <span className="text-amber-600/90 dark:text-amber-400/90">
                  {q.missingBadge}
                </span>
              )}
            </div>
          </div>
        </div>
        {active && <Check size={13} className="shrink-0 text-blue-600 dark:text-sky-300" />}
      </button>
    );
  };

  const cliPlaceholder =
    targetClient === 'codex'
      ? am.cliCmdPlaceholderCodex
      : targetClient === 'claude'
        ? am.cliCmdPlaceholderClaude
        : am.cliCmdPlaceholderAgy;

  const busy = busyAction !== null;
  const savedAccounts = accountsState?.accounts ?? [];

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={h.title}
      subtitle={h.subtitle}
      widthClass="max-w-2xl"
    >
      {result ? renderResult() : (
      <div className="flex flex-col gap-4 text-[12.5px]">
        {/* Source conversation summary + auto-sync toggle */}
        <div className="rounded-xl border border-slate-200/80 dark:border-slate-800 bg-slate-50/85 dark:bg-[#0f1624] px-3.5 py-2.5 flex flex-col gap-2">
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-2 min-w-0">
              <ArrowRightLeft size={14} className="text-blue-600 dark:text-sky-400 shrink-0" />
              <span className="font-semibold text-[13px] text-slate-900 dark:text-white truncate">
                {conversation.title}
              </span>
            </div>
            {conversation.message_count !== undefined && (
              <span className="shrink-0 text-[11px] font-mono text-slate-500 dark:text-slate-400">
                {interpolate(t.chat.messageCount, { count: conversation.message_count })}
              </span>
            )}
          </div>
          <label className="inline-flex items-center gap-2 text-[11px] text-slate-600 dark:text-slate-300 select-none cursor-pointer">
            <input
              type="checkbox"
              checked={syncLatestFirst}
              onChange={(e) => setSyncLatestFirst(e.target.checked)}
              className="cv-checkbox"
            />
            <RefreshCw size={11} className="text-slate-400" />
            <span>{h.syncLatestLabel}</span>
          </label>
        </div>

        {/* Target Tool + Optional Account Switch */}
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <span className="font-semibold text-slate-600 dark:text-slate-300">
              {h.targetClientSection}
            </span>
            <div className="grid grid-cols-3 gap-1.5">
              {CLIENT_OPTIONS.map((opt) => {
                const selected = targetClient === opt.id;
                const isSame = sourceClient === opt.id;
                return (
                  <button
                    key={opt.id}
                    type="button"
                    aria-pressed={selected}
                    onClick={() => handleSelectClient(opt.id)}
                    className={`flex flex-col items-center justify-center gap-0.5 px-2 py-2 rounded-xl border transition-all ${
                      selected
                        ? 'border-blue-500/80 dark:border-sky-400/75 bg-blue-50/90 dark:bg-sky-500/15 text-blue-700 dark:text-sky-200 ring-1 ring-inset ring-blue-500/20'
                        : 'border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0f1624] text-slate-700 dark:text-slate-200 hover:border-slate-300 dark:hover:border-slate-700'
                    }`}
                  >
                    <span className="inline-flex items-center gap-1.5 font-semibold text-[12px]">
                      <span
                        className="w-2 h-2 rounded-full"
                        style={{ backgroundColor: opt.dotColor }}
                      />
                      {opt.label}
                    </span>
                    {isSame && (
                      <span className="text-[9.5px] text-slate-400 dark:text-slate-500">
                        {h.sameClientBadge}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          </div>

          <label className="flex flex-col gap-1.5">
            <span className="font-semibold text-slate-600 dark:text-slate-300">
              {h.accountSection}
            </span>
            <select
              aria-label={h.accountSection}
              value={selectedAccountId}
              onChange={(e) => setSelectedAccountId(e.target.value)}
              disabled={busy}
              className="h-[42px] px-2.5 rounded-xl border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 text-[12px] focus:outline-none focus:ring-2 focus:ring-blue-500/35"
            >
              <option value="">{h.keepCurrentAccount}</option>
              {savedAccounts.map((acc) => (
                <option key={acc.id} value={acc.id}>
                  {formatAccountOptionLabel(acc, am.currentBadge, (pct) =>
                    interpolate(h.quotaRemaining, { percent: pct }),
                  )}
                </option>
              ))}
            </select>
          </label>
        </div>

        {/* Handoff Mode: Mode A vs Mode B */}
        <div className="flex flex-col gap-1.5">
          <span className="font-semibold text-slate-600 dark:text-slate-300">
            {h.modeSection}
          </span>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2.5">
            <button
              type="button"
              aria-pressed={mode === 'native'}
              onClick={() => setMode('native')}
              className={`p-3 rounded-xl border text-left transition-all flex flex-col gap-1 ${
                mode === 'native'
                  ? 'border-blue-500/80 dark:border-sky-400/75 bg-blue-50/85 dark:bg-sky-500/12 ring-1 ring-inset ring-blue-500/20'
                  : 'border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0f1624] hover:border-slate-300 dark:hover:border-slate-700'
              }`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="inline-flex items-center gap-1.5 font-semibold text-[12.5px] text-slate-900 dark:text-white">
                  <Sparkles size={13} className="text-blue-600 dark:text-sky-400" />
                  {h.modeNativeTitle}
                </span>
                {mode === 'native' && <Check size={14} className="text-blue-600 dark:text-sky-300" />}
              </div>
              <p className="text-[11px] text-slate-500 dark:text-slate-400 leading-relaxed">
                {h.modeNativeDesc}
              </p>
            </button>

            <button
              type="button"
              aria-pressed={mode === 'compact'}
              onClick={() => setMode('compact')}
              className={`p-3 rounded-xl border text-left transition-all flex flex-col gap-1 ${
                mode === 'compact'
                  ? 'border-blue-500/80 dark:border-sky-400/75 bg-blue-50/85 dark:bg-sky-500/12 ring-1 ring-inset ring-blue-500/20'
                  : 'border-slate-200 dark:border-slate-800 bg-white dark:bg-[#0f1624] hover:border-slate-300 dark:hover:border-slate-700'
              }`}
            >
              <div className="flex items-center justify-between gap-2">
                <span className="inline-flex items-center gap-1.5 font-semibold text-[12.5px] text-slate-900 dark:text-white">
                  <FileText size={13} className="text-emerald-600 dark:text-emerald-400" />
                  {h.modeCompactTitle}
                </span>
                {mode === 'compact' && <Check size={14} className="text-blue-600 dark:text-sky-300" />}
              </div>
              <p className="text-[11px] text-slate-500 dark:text-slate-400 leading-relaxed">
                {h.modeCompactDesc}
              </p>
            </button>
          </div>

          {mode === 'compact' && (
            <div className="rounded-xl border border-slate-200/80 dark:border-slate-800 bg-slate-50/75 dark:bg-[#0d1422] p-3 flex flex-col gap-2.5 mt-1">
              <div className="flex items-center justify-between gap-2 flex-wrap">
                <span className="font-semibold text-slate-600 dark:text-slate-300">
                  {h.recentLimitLabel}
                </span>
                <div className="inline-flex items-center gap-1">
                  {RECENT_LIMIT_OPTIONS.map((opt) => (
                    <button
                      key={opt}
                      type="button"
                      aria-pressed={recentLimit === opt}
                      onClick={() => setRecentLimit(opt)}
                      className={`px-2.5 py-1 rounded-lg text-[11px] font-medium transition-all ${
                        recentLimit === opt
                          ? 'bg-blue-600 text-white dark:bg-sky-500 dark:text-slate-950'
                          : 'bg-white dark:bg-slate-800 text-slate-600 dark:text-slate-300 border border-slate-200 dark:border-slate-700'
                      }`}
                    >
                      {opt === 0
                        ? h.recentAll
                        : interpolate(h.recentCount, { count: opt })}
                    </button>
                  ))}
                </div>
              </div>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">
                  {h.followUpLabel}
                </span>
                <input
                  aria-label={h.followUpLabel}
                  value={followUpPrompt}
                  onChange={(e) => setFollowUpPrompt(e.target.value)}
                  placeholder={h.followUpPlaceholder}
                  className="px-2.5 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 text-[12px] focus:outline-none focus:ring-2 focus:ring-blue-500/35"
                />
              </label>
            </div>
          )}
        </div>

        {/* Launch Target + Folder + CLI Options */}
        <div className="flex flex-col gap-2.5">
          <span className="font-semibold text-slate-600 dark:text-slate-300">
            {h.launchTargetSection}
          </span>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {cliTool && renderTargetOption(cliTool)}
            {desktopTool && renderTargetOption(desktopTool)}
          </div>
          {editorTools.length > 0 && (
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
              {editorTools.map(renderTargetOption)}
            </div>
          )}

          {isDesktop && (
            <label className="inline-flex items-center gap-2 text-[11px] text-slate-600 dark:text-slate-300 select-none cursor-pointer">
              <input
                type="checkbox"
                checked={restartDesktop}
                onChange={(e) => setRestartDesktop(e.target.checked)}
                className="cv-checkbox"
              />
              <span>{h.restartDesktopLabel}</span>
            </label>
          )}
        </div>

        {/* Executable Path */}
        {selectedTool && !selectedTool.available && (
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="handoff-exe-path"
              className="font-semibold text-slate-600 dark:text-slate-300 flex items-center justify-between"
            >
              <span>{am.pathSection}</span>
              {selectedTool.custom_path && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void saveCustomPath(null)}
                  className="inline-flex items-center gap-1 text-[11px] text-slate-500 hover:text-slate-800 dark:text-slate-400"
                >
                  <RotateCcw size={11} />
                  {am.resetPath}
                </button>
              )}
            </label>
            <div className="flex items-center gap-1.5">
              <input
                id="handoff-exe-path"
                value={pathDraft}
                onChange={(e) => setPathDraft(e.target.value)}
                placeholder={am.pathPlaceholderAuto}
                className="flex-1 min-w-0 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 font-mono text-[11px]"
              />
              <Button
                size="sm"
                variant="secondary"
                icon={<FolderSearch size={13} />}
                disabled={busy}
                onClick={() => void handleBrowseExe()}
              >
                {t.common.browse}
              </Button>
              {pathDirty && (
                <Button
                  size="sm"
                  variant="primary"
                  icon={<Save size={13} />}
                  disabled={busy}
                  onClick={() => void saveCustomPath(pathDraft.trim() || null)}
                >
                  {am.savePath}
                </Button>
              )}
            </div>
            {!pathDraft.trim() && (
              <div className="flex items-center gap-1.5 text-[11px] text-amber-600 dark:text-amber-400">
                <AlertCircle size={12} />
                <span>{am.pathNotConfigured}</span>
              </div>
            )}
          </div>
        )}

        {/* Working Directory */}
        <div className="flex flex-col gap-1.5">
          <label
            htmlFor="handoff-folder"
            className="font-semibold text-slate-600 dark:text-slate-300"
          >
            {am.folderSection}
          </label>
          <div className="flex items-center gap-1.5">
            <div className="relative flex-1 min-w-0">
              <FolderOpen
                size={13}
                className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-400 pointer-events-none"
              />
              <input
                id="handoff-folder"
                value={folderPath}
                onChange={(e) => setFolderPath(e.target.value)}
                placeholder={am.folderPlaceholder}
                className="w-full pl-8 pr-7 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 font-mono text-[11px] focus:outline-none focus:ring-2 focus:ring-blue-500/35"
              />
              {folderPath && (
                <button
                  type="button"
                  aria-label={q.clearFolder}
                  onClick={() => setFolderPath('')}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600"
                >
                  <X size={12} />
                </button>
              )}
            </div>
            <Button
              size="sm"
              variant="secondary"
              icon={<FolderSearch size={13} />}
              disabled={busy}
              onClick={() => void handleBrowseFolder()}
            >
              {q.pickFolder}
            </Button>
          </div>
          <p className="text-[11px] text-slate-400 dark:text-slate-500 leading-relaxed">
            {h.folderAutoCreateHint}
          </p>
        </div>

        {/* CLI Terminal & Flags + Preview */}
        {isCli && (
          <div className="flex flex-col gap-2.5 rounded-xl border border-slate-200/80 dark:border-slate-800 bg-slate-50/70 dark:bg-[#0d1422] p-3">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">
                  {am.terminalSection}
                </span>
                <select
                  aria-label={am.terminalSection}
                  value={terminal}
                  onChange={(e) => persistTerminal(e.target.value as QuickLaunchTerminal)}
                  className="px-2.5 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 text-[12px]"
                >
                  {TERMINAL_OPTIONS.map((termKey) => (
                    <option key={termKey} value={termKey}>
                      {am.terminals[termKey]}
                    </option>
                  ))}
                </select>
              </label>

              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">
                  {am.cliCmdSection}
                </span>
                <input
                  aria-label={am.cliCmdSection}
                  value={cliCommand}
                  onChange={(e) => persistCliCommand(e.target.value)}
                  placeholder={cliPlaceholder}
                  className="px-2.5 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 font-mono text-[11px]"
                />
              </label>
            </div>

            <div className="flex flex-col gap-1">
              <span className="text-[10px] text-slate-400 dark:text-slate-500">
                {am.previewLabel}
              </span>
              <pre className="px-2.5 py-1.5 rounded-lg bg-slate-900 dark:bg-black/60 text-slate-100 font-mono text-[11px] overflow-x-auto whitespace-pre-wrap break-all border border-slate-800">
                {cliPreview}
              </pre>
            </div>
          </div>
        )}

        {/* Modal Footer */}
        <div className="flex items-center justify-end gap-2 pt-2 border-t border-slate-200/80 dark:border-slate-800 flex-wrap">
          <Button size="sm" variant="ghost" onClick={onClose} disabled={busy}>
            {t.common.cancel}
          </Button>
          <Button
            size="sm"
            variant="secondary"
            icon={
              busyAction === 'prepare' ? (
                <Loader2 size={13} className="animate-spin" />
              ) : (
                <Copy size={13} />
              )
            }
            disabled={busy || !selectedTool}
            onClick={() => void runHandoff(true)}
          >
            {busyAction === 'prepare' ? h.workingBtn : h.prepareCopyBtn}
          </Button>
          <Button
            size="sm"
            variant="primary"
            icon={
              busyAction === 'launch' ? (
                <Loader2 size={13} className="animate-spin" />
              ) : (
                <Play size={13} />
              )
            }
            disabled={busy || !selectedTool}
            onClick={() => void runHandoff(false)}
          >
            {busyAction === 'launch'
              ? h.workingBtn
              : isCli
                ? h.executeCliBtn
                : h.executeGuiBtn}
          </Button>
        </div>
      </div>
      )}
    </Modal>
  );
};
