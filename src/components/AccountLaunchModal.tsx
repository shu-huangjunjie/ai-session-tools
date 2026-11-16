import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertCircle,
  Bot,
  Braces,
  Check,
  Code2,
  Copy,
  Cpu,
  FolderOpen,
  FolderSearch,
  Layers,
  Loader2,
  LucideIcon,
  Play,
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
  AccountView,
  QuickLaunchTerminal,
  QuickLaunchTool,
} from '../types';
import { Button } from './common/Button';
import { Modal } from './common/Modal';

export interface AccountLaunchModalProps {
  isOpen: boolean;
  client: AccountClient;
  account: AccountView;
  accountLabel: string;
  onClose: () => void;
  onSwitched: () => void;
  onToast: (type: 'success' | 'warning' | 'error' | 'info', message: string) => void;
}

const QUICK_LAUNCH_DIR_KEY = 'cv_quick_launch_dir';
const ACCOUNT_LAUNCH_TERMINAL_KEY = 'cv_account_launch_terminal';
const QUICK_LAUNCH_TOOLS_EVENT = 'cv-quick-launch-updated';
const QUICK_LAUNCH_DIR_EVENT = 'cv-quick-launch-dir-changed';

const targetStorageKey = (client: AccountClient) => `cv_account_launch_target_${client}`;
const cliCmdStorageKey = (toolId: string) => `cv_account_cli_cmd_${toolId}`;

const CLIENT_TARGET_MAP: Record<
  AccountClient,
  { desktopId: string; cliId: string; editorIds: string[] }
> = {
  codex: {
    desktopId: 'codex_app',
    cliId: 'codex_cli',
    editorIds: ['vscode', 'cursor', 'windsurf', 'trae'],
  },
  claude: {
    desktopId: 'claude_app',
    cliId: 'claude_cli',
    editorIds: ['vscode', 'cursor', 'windsurf', 'trae'],
  },
  antigravity: {
    desktopId: 'antigravity_app',
    cliId: 'antigravity_cli',
    editorIds: ['vscode', 'cursor', 'windsurf', 'trae'],
  },
};

const FALLBACK_TOOLS: QuickLaunchTool[] = [
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
    supports_folder: false,
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

const TOOL_META: Record<string, { icon: LucideIcon; defaultCmd?: string }> = {
  codex_app: { icon: Sparkles },
  claude_app: { icon: Bot },
  antigravity_app: { icon: Rocket },
  codex_cli: { icon: TerminalSquare, defaultCmd: 'codex' },
  claude_cli: { icon: TerminalSquare, defaultCmd: 'claude' },
  antigravity_cli: { icon: Cpu, defaultCmd: 'agy' },
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

function loadSavedTerminal(): QuickLaunchTerminal {
  try {
    const val = localStorage.getItem(ACCOUNT_LAUNCH_TERMINAL_KEY) as QuickLaunchTerminal | null;
    if (val && TERMINAL_OPTIONS.includes(val)) return val;
  } catch {
    // ignore storage errors
  }
  return 'system';
}

function splitCliCustomArgs(target: string, rawInput: string): string | null {
  const trimmed = rawInput.trim();
  if (!trimmed) return null;

  const baseName = target.split(/[\\/]/).pop() ?? target;
  const stem = baseName.replace(/\.[^.]+$/, '');
  const prefixes = [baseName, stem]
    .filter((p) => p.length > 0)
    .sort((a, b) => b.length - a.length);

  for (const prefix of prefixes) {
    if (trimmed.toLowerCase() === prefix.toLowerCase()) {
      return null;
    }
    if (
      trimmed.slice(0, prefix.length).toLowerCase() === prefix.toLowerCase() &&
      /^\s/.test(trimmed.slice(prefix.length))
    ) {
      const rest = trimmed.slice(prefix.length).trim();
      return rest || null;
    }
  }
  return trimmed;
}

export function buildAccountCliPreview(
  tool: QuickLaunchTool | undefined,
  customCommand: string,
  folderPath: string,
  terminal: QuickLaunchTerminal,
): string {
  if (!tool) return '';
  const defaultBin = TOOL_META[tool.id]?.defaultCmd ?? tool.id;
  const target = (tool.custom_path || tool.detected_path || defaultBin).trim();
  const args = splitCliCustomArgs(target, customCommand);
  const folder = folderPath.trim();

  if (terminal === 'cmd') {
    const invoke = args ? `"${target}" ${args}` : `"${target}"`;
    return folder ? `cd /d "${folder}" && ${invoke}` : invoke;
  }

  if (terminal === 'git_bash') {
    const posixTarget = target.replace(/\\/g, '/').replace(/'/g, "'\\''");
    const invoke = args ? `'${posixTarget}' ${args}` : `'${posixTarget}'`;
    if (folder) {
      const posixDir = folder.replace(/\\/g, '/').replace(/'/g, "'\\''");
      return `cd '${posixDir}' && ${invoke}`;
    }
    return invoke;
  }

  // system / wt / powershell / pwsh
  const psTarget = target.replace(/'/g, "''");
  const invoke = args ? `& '${psTarget}' ${args}` : `& '${psTarget}'`;
  if (folder) {
    const psDir = folder.replace(/'/g, "''");
    return `Set-Location -LiteralPath '${psDir}'; ${invoke}`;
  }
  return invoke;
}

export const AccountLaunchModal: React.FC<AccountLaunchModalProps> = ({
  isOpen,
  client,
  account,
  accountLabel,
  onClose,
  onSwitched,
  onToast,
}) => {
  const { t, interpolate } = useI18n();
  const q = t.quickLaunch;
  const am = q.accountModal;
  const a = t.accounts;

  const [tools, setTools] = useState<QuickLaunchTool[]>(FALLBACK_TOOLS);
  const [selectedToolId, setSelectedToolId] = useState<string>(CLIENT_TARGET_MAP[client].desktopId);
  const [folderPath, setFolderPath] = useState<string>(() => {
    try {
      return localStorage.getItem(QUICK_LAUNCH_DIR_KEY) ?? '';
    } catch {
      return '';
    }
  });
  const [terminal, setTerminal] = useState<QuickLaunchTerminal>(loadSavedTerminal);
  const [cliCommand, setCliCommand] = useState<string>('');
  const [pathDraft, setPathDraft] = useState<string>('');
  const [busyAction, setBusyAction] = useState<'launch' | 'switch' | 'copy' | 'path' | null>(null);
  const [copied, setCopied] = useState(false);
  const userPickedRef = useRef(false);

  const clientIds = CLIENT_TARGET_MAP[client];

  const loadTools = useCallback(async () => {
    if (typeof api.listQuickLaunchTools !== 'function') return;
    try {
      const list = await api.listQuickLaunchTools();
      if (Array.isArray(list) && list.length > 0) {
        setTools(list);
      }
    } catch {
      // Keep fallback tools if scan fails in browser preview
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    userPickedRef.current = false;
    void loadTools();
  }, [isOpen, client, loadTools]);

  const byId = useMemo(() => new Map(tools.map((item) => [item.id, item])), [tools]);
  const desktopTool = byId.get(clientIds.desktopId);
  const cliTool = byId.get(clientIds.cliId);
  const editorTools = useMemo(
    () =>
      clientIds.editorIds
        .map((id) => byId.get(id))
        .filter((item): item is QuickLaunchTool => Boolean(item)),
    [byId, clientIds.editorIds],
  );

  // Pick initial/remembered tool whenever modal opens or client changes
  useEffect(() => {
    if (!isOpen || userPickedRef.current) return;
    const allowedIds = [clientIds.desktopId, clientIds.cliId, ...clientIds.editorIds];
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(targetStorageKey(client));
    } catch {
      saved = null;
    }
    if (saved && allowedIds.includes(saved)) {
      setSelectedToolId(saved);
      return;
    }
    const firstReady = allowedIds.find((id) => byId.get(id)?.available);
    setSelectedToolId(firstReady ?? clientIds.desktopId);
  }, [isOpen, client, clientIds, byId]);

  const selectedTool = useMemo(
    () => byId.get(selectedToolId) ?? desktopTool ?? cliTool ?? editorTools[0],
    [byId, selectedToolId, desktopTool, cliTool, editorTools],
  );

  // Sync path draft & CLI custom command when selected tool changes
  useEffect(() => {
    if (!selectedTool) return;
    setPathDraft(selectedTool.custom_path ?? selectedTool.detected_path ?? '');
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

  const selectTarget = (toolId: string) => {
    userPickedRef.current = true;
    setSelectedToolId(toolId);
    try {
      localStorage.setItem(targetStorageKey(client), toolId);
    } catch {
      // ignore storage errors
    }
  };

  const persistFolder = (next: string) => {
    setFolderPath(next);
    try {
      localStorage.setItem(QUICK_LAUNCH_DIR_KEY, next);
      window.dispatchEvent(new CustomEvent(QUICK_LAUNCH_DIR_EVENT, { detail: next }));
    } catch {
      // ignore
    }
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
  const resolvedSavedPath = selectedTool?.custom_path ?? selectedTool?.detected_path ?? '';
  const pathDirty = Boolean(selectedTool && pathDraft.trim() !== resolvedSavedPath.trim());

  const saveCustomPath = async (nextPath: string | null) => {
    if (!selectedTool) return;
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
    const picked = await api.pickFolderPath();
    if (picked) {
      persistFolder(picked);
    }
  };

  const previewCommand = useMemo(
    () => (isCli ? buildAccountCliPreview(selectedTool, cliCommand, folderPath, terminal) : ''),
    [isCli, selectedTool, cliCommand, folderPath, terminal],
  );

  const handleSwitchOnly = async () => {
    setBusyAction('switch');
    try {
      await api.switchAccount(client, account.id, false);
      onToast('success', interpolate(a.toastSwitchedTarget, { account: accountLabel }));
      onSwitched();
      onClose();
    } catch (err) {
      onToast('error', interpolate(a.toastFailed, { error: errorMessage(err) }));
    } finally {
      setBusyAction(null);
    }
  };

  const handleCopyCommand = async () => {
    if (!previewCommand) return;
    setBusyAction('copy');
    try {
      if (!account.is_current) {
        await api.switchAccount(client, account.id, false);
        onSwitched();
      }
      if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(previewCommand);
      }
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
      onToast(
        'success',
        account.is_current ? am.copiedToast : a.toastSwitchedAndCopied,
      );
      if (!account.is_current) {
        onClose();
      }
    } catch (err) {
      onToast('error', interpolate(a.toastFailed, { error: errorMessage(err) }));
    } finally {
      setBusyAction(null);
    }
  };

  const handleLaunch = async () => {
    if (!selectedTool) return;
    setBusyAction('launch');
    try {
      if (pathDirty && typeof api.setQuickLaunchCustomPath === 'function') {
        const trimmed = pathDraft.trim();
        const updated = await api.setQuickLaunchCustomPath(
          selectedTool.id,
          trimmed || null,
        );
        setTools(updated);
        window.dispatchEvent(new CustomEvent(QUICK_LAUNCH_TOOLS_EVENT, { detail: updated }));
      }
      const trimmedFolder = folderPath.trim();
      await api.launchAccountTarget({
        client,
        accountId: account.id,
        toolId: selectedTool.id,
        folderPath: trimmedFolder || null,
        terminal: isCli ? terminal : null,
        customCommand: isCli ? cliCommand.trim() || null : null,
      });
      onToast(
        'success',
        interpolate(a.toastLaunchedTarget, {
          target: selectedTool.name,
          account: accountLabel,
        }),
      );
      if (!account.is_current) {
        onSwitched();
      }
      onClose();
    } catch (err) {
      onToast('error', interpolate(q.launchFailed, { name: selectedTool.name, error: errorMessage(err) }));
    } finally {
      setBusyAction(null);
    }
  };

  const cliPlaceholder =
    client === 'codex'
      ? am.cliCmdPlaceholderCodex
      : client === 'claude'
        ? am.cliCmdPlaceholderClaude
        : am.cliCmdPlaceholderAgy;

  const renderTargetOption = (tool: QuickLaunchTool) => {
    const active = selectedTool?.id === tool.id;
    const Icon = TOOL_META[tool.id]?.icon ?? Terminal;
    return (
      <button
        key={tool.id}
        type="button"
        onClick={() => selectTarget(tool.id)}
        aria-pressed={active}
        className={`group flex items-center justify-between gap-2 px-3 py-2.5 rounded-xl border text-left transition-all ${
          active
            ? 'border-blue-500/80 dark:border-sky-400/75 bg-blue-50/90 dark:bg-sky-500/12 text-blue-700 dark:text-sky-200 shadow-xs ring-1 ring-inset ring-blue-500/20'
            : 'border-slate-200/85 dark:border-slate-800 bg-white dark:bg-[#0f1624] text-slate-700 dark:text-slate-200 hover:border-slate-300 dark:hover:border-slate-700 hover:bg-slate-50/80 dark:hover:bg-slate-800/45'
        }`}
      >
        <div className="flex items-center gap-2 min-w-0">
          <span
            className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 ${
              active
                ? 'bg-blue-600 text-white dark:bg-sky-500 dark:text-slate-950'
                : 'bg-slate-100 dark:bg-slate-800 text-slate-500 dark:text-slate-400'
            }`}
          >
            <Icon size={14} />
          </span>
          <div className="min-w-0">
            <div className="text-[12px] font-semibold truncate">{tool.name}</div>
            <div className="text-[10px] text-slate-400 dark:text-slate-500 flex items-center gap-1">
              {tool.running ? (
                <span className="text-emerald-600 dark:text-emerald-400 flex items-center gap-1">
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
        {active && <Check size={14} className="shrink-0 text-blue-600 dark:text-sky-300" />}
      </button>
    );
  };

  const busy = busyAction !== null;

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={account.is_current ? am.titleLaunch : am.titleSwitchLaunch}
      subtitle={am.subtitle}
      widthClass="max-w-2xl"
    >
      <div className="flex flex-col gap-4 text-xs">
        {/* Target Account Banner */}
        <div className="rounded-xl border border-slate-200/80 dark:border-slate-800 bg-slate-50/85 dark:bg-[#0f1624] px-3.5 py-2.5 flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div className="text-[10px] uppercase tracking-wider font-semibold text-slate-400 dark:text-slate-500">
              {am.accountLabel}
            </div>
            <div className="text-[13px] font-semibold text-slate-800 dark:text-slate-100 truncate">
              {accountLabel}
            </div>
            {!account.is_current && (
              <div className="text-[11px] text-amber-600 dark:text-amber-400 mt-0.5">
                {am.switchHint}
              </div>
            )}
          </div>
          {account.is_current ? (
            <span className="shrink-0 inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[11px] font-medium bg-emerald-50 dark:bg-emerald-500/15 text-emerald-700 dark:text-emerald-300 ring-1 ring-inset ring-emerald-500/25">
              <span className="cv-live-dot" />
              {am.currentBadge}
            </span>
          ) : (
            account.plan_type && (
              <span className="shrink-0 px-2.5 py-1 rounded-full text-[11px] font-medium bg-blue-50 dark:bg-blue-500/15 text-blue-700 dark:text-sky-300 ring-1 ring-inset ring-blue-500/20">
                {account.plan_type}
              </span>
            )
          )}
        </div>

        {/* Target Selector Grid */}
        <div className="flex flex-col gap-2.5">
          <div className="text-[11px] font-semibold text-slate-600 dark:text-slate-300">
            {am.targetSection}
          </div>

          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2">
            {desktopTool && (
              <div className="flex flex-col gap-1">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-slate-400 dark:text-slate-500">
                  {am.groupDesktop}
                </span>
                {renderTargetOption(desktopTool)}
              </div>
            )}
            {cliTool && (
              <div className="flex flex-col gap-1">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-slate-400 dark:text-slate-500">
                  {am.groupCli}
                </span>
                {renderTargetOption(cliTool)}
              </div>
            )}
          </div>

          {editorTools.length > 0 && (
            <div className="flex flex-col gap-1">
              <span className="text-[10px] font-semibold uppercase tracking-wider text-slate-400 dark:text-slate-500">
                {am.groupEditors}
              </span>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
                {editorTools.map(renderTargetOption)}
              </div>
            </div>
          )}
        </div>

        {/* Executable Path Configuration */}
        {selectedTool && (
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="account-launch-exe-path"
              className="font-semibold text-slate-600 dark:text-slate-300 flex items-center justify-between"
            >
              <span>{am.pathSection}</span>
              {selectedTool.custom_path && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void saveCustomPath(null)}
                  className="inline-flex items-center gap-1 text-[11px] font-medium text-slate-500 hover:text-slate-800 dark:text-slate-400 dark:hover:text-slate-200"
                >
                  <RotateCcw size={11} />
                  {am.resetPath}
                </button>
              )}
            </label>
            <div className="flex items-center gap-1.5">
              <input
                id="account-launch-exe-path"
                value={pathDraft}
                onChange={(e) => setPathDraft(e.target.value)}
                placeholder={am.pathPlaceholderAuto}
                className="flex-1 min-w-0 px-3 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 font-mono text-[11px] focus:outline-none focus:ring-2 focus:ring-blue-500/35"
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
            {!selectedTool.available && !pathDraft.trim() ? (
              <div className="flex items-center gap-1.5 text-[11px] text-amber-600 dark:text-amber-400">
                <AlertCircle size={12} className="shrink-0" />
                <span>{am.pathNotConfigured}</span>
              </div>
            ) : selectedTool.detected_path && !selectedTool.custom_path ? (
              <div className="text-[11px] text-slate-400 dark:text-slate-500 truncate font-mono">
                {am.pathDetectedPrefix}
                {selectedTool.detected_path}
              </div>
            ) : null}
          </div>
        )}

        {/* Working Directory Picker */}
        {(isCli || selectedTool?.supports_folder) && (
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="account-launch-folder"
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
                  id="account-launch-folder"
                  value={folderPath}
                  onChange={(e) => persistFolder(e.target.value)}
                  placeholder={am.folderPlaceholder}
                  className="w-full pl-8 pr-7 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 font-mono text-[11px] focus:outline-none focus:ring-2 focus:ring-blue-500/35"
                />
                {folderPath && (
                  <button
                    type="button"
                    aria-label={q.clearFolder}
                    onClick={() => persistFolder('')}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-600 dark:hover:text-slate-200"
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
          </div>
        )}

        {/* CLI Terminal & Command Fields */}
        {isCli && (
          <div className="flex flex-col gap-3 rounded-xl border border-slate-200/80 dark:border-slate-800 bg-slate-50/70 dark:bg-[#0d1422] p-3">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-slate-600 dark:text-slate-300">
                  {am.terminalSection}
                </span>
                <select
                  aria-label={am.terminalSection}
                  value={terminal}
                  onChange={(e) => persistTerminal(e.target.value as QuickLaunchTerminal)}
                  className="px-2.5 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 text-[12px] focus:outline-none focus:ring-2 focus:ring-blue-500/35"
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
                  className="px-2.5 py-1.5 rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-[#0b1120] text-slate-800 dark:text-slate-100 font-mono text-[11px] focus:outline-none focus:ring-2 focus:ring-blue-500/35"
                />
              </label>
            </div>

            <div className="flex flex-col gap-1">
              <div className="flex items-center justify-between text-[10px] text-slate-400 dark:text-slate-500">
                <span>{am.previewLabel}</span>
                <span>{am.cliCmdHint}</span>
              </div>
              <pre className="px-2.5 py-1.5 rounded-lg bg-slate-900 dark:bg-black/60 text-slate-100 font-mono text-[11px] overflow-x-auto whitespace-pre-wrap break-all border border-slate-800">
                {previewCommand}
              </pre>
            </div>
          </div>
        )}

        {/* Modal Footer Actions */}
        <div className="flex items-center justify-end gap-2 pt-2 border-t border-slate-200/80 dark:border-slate-800 flex-wrap">
          <Button size="sm" variant="ghost" onClick={onClose} disabled={busy}>
            {t.common.cancel}
          </Button>
          {!account.is_current && (
            <Button
              size="sm"
              variant="secondary"
              disabled={busy}
              onClick={() => void handleSwitchOnly()}
            >
              {busyAction === 'switch' ? am.launchingBtn : am.switchOnlyBtn}
            </Button>
          )}
          {isCli && (
            <Button
              size="sm"
              variant="secondary"
              icon={copied ? <Check size={13} /> : <Copy size={13} />}
              disabled={busy || !previewCommand}
              onClick={() => void handleCopyCommand()}
            >
              {account.is_current ? am.copyBtn : am.switchCopyBtn}
            </Button>
          )}
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
            onClick={() => void handleLaunch()}
          >
            {busyAction === 'launch'
              ? am.launchingBtn
              : isCli
                ? account.is_current
                  ? am.execCliBtn
                  : am.switchExecCliBtn
                : account.is_current
                  ? am.launchBtn
                  : am.switchLaunchBtn}
          </Button>
        </div>
      </div>
    </Modal>
  );
};
