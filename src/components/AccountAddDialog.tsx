import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  AlertTriangle,
  Check,
  Copy,
  Download,
  ExternalLink,
  FileUp,
  Globe,
  HardDriveDownload,
  KeyRound,
  Loader2,
  Save,
  Send,
  Smartphone,
} from 'lucide-react';
import { api, errorMessage } from '../api/client';
import { AccountClient, CodexDeviceAuthStart, CurrentLogin } from '../types';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { Segmented } from './common/Segmented';
import { RelayForm } from './RelayForm';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';
type AddTab = 'oauth' | 'token' | 'provider' | 'local';
type OAuthSubMode = 'pkce' | 'device' | 'cli';

const CLIENT_LABEL: Record<AccountClient, string> = {
  codex: 'Codex',
  claude: 'Claude',
  antigravity: 'Antigravity',
};

interface AccountAddDialogProps {
  client: AccountClient;
  /** Who is signed in to the client now, for importing that login. */
  current?: CurrentLogin | null;
  cockpitImportCount?: number;
  ccSwitchAvailable?: boolean;
  onClose: () => void;
  /** Called after an account was added, so the list reloads. */
  onAdded: () => void;
  onToast: (type: ToastType, message: string) => void;
}

/** Unified 4-tab account dialog inspired by Cockpit Tools: OAuth/Device, Token/JSON, API Provider, Local & External import. */
export const AccountAddDialog: React.FC<AccountAddDialogProps> = ({
  client,
  current,
  cockpitImportCount = 0,
  ccSwitchAvailable = false,
  onClose,
  onAdded,
  onToast,
}) => {
  const { t, interpolate } = useI18n();
  const a = t.accounts;
  const vars = { client: CLIENT_LABEL[client] };

  const tabs = useMemo<AddTab[]>(() => {
    if (client === 'antigravity') return ['oauth', 'token', 'local'];
    return ['oauth', 'token', 'provider', 'local'];
  }, [client]);

  const [tab, setTab] = useState<AddTab>('oauth');
  const [oauthSubMode, setOauthSubMode] = useState<OAuthSubMode>(
    client === 'claude' ? 'cli' : 'pkce'
  );
  const [busy, setBusy] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [loginUrl, setLoginUrl] = useState<string | null>(null);
  const [manualCallback, setManualCallback] = useState('');
  const [deviceStart, setDeviceStart] = useState<CodexDeviceAuthStart | null>(null);
  const [deviceCopied, setDeviceCopied] = useState(false);
  const [tokens, setTokens] = useState('');
  const [failures, setFailures] = useState<string[]>([]);

  const cancelled = useRef(false);
  // Tracked in ref so closing or unmounting stops background sign-in loops cleanly.
  const waiting = useRef(false);

  const fail = (err: unknown) =>
    onToast('error', interpolate(a.toastFailed, { error: errorMessage(err) }));

  const run = async (action: () => Promise<string | void>, closeOnDone = true) => {
    setBusy(true);
    try {
      const message = await action();
      if (message) onToast('success', message);
      onAdded();
      if (closeOnDone) onClose();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const cancelLogin = () => {
    cancelled.current = true;
    waiting.current = false;
    void api.cancelAccountLogin();
  };

  const close = () => {
    if (waiting.current) cancelLogin();
    onClose();
  };

  useEffect(
    () => () => {
      if (waiting.current) cancelLogin();
    },
    // Only on unmount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    []
  );

  const startBrowserOAuth = async () => {
    cancelled.current = false;
    waiting.current = true;
    setLoggingIn(true);
    setLoginUrl(null);
    setManualCallback('');
    const stop = await api.onAccountLoginUrl(setLoginUrl);
    try {
      let mode: 'oauth' | 'cli' | 'pkce' | undefined;
      if (client === 'codex') {
        mode = oauthSubMode === 'cli' ? 'cli' : 'oauth';
      } else if (client === 'claude') {
        mode = oauthSubMode === 'pkce' ? 'pkce' : 'cli';
      }
      await api.loginAccount(client, mode);
      onToast('success', a.toastLoggedIn);
      onAdded();
      onClose();
    } catch (err) {
      if (!cancelled.current) fail(err);
    } finally {
      stop();
      waiting.current = false;
      setLoggingIn(false);
    }
  };

  const submitManualCallback = async () => {
    const raw = manualCallback.trim();
    if (!raw) return;
    setBusy(true);
    try {
      await api.submitAccountLoginCallback(raw);
      onToast('info', a.toastCallbackSubmitted);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const startDeviceAuth = async () => {
    cancelled.current = false;
    waiting.current = true;
    setLoggingIn(true);
    setDeviceStart(null);
    setDeviceCopied(false);
    try {
      const started = await api.startCodexDeviceLogin();
      if (cancelled.current) return;
      setDeviceStart(started);
      await api.completeCodexDeviceLogin(started);
      if (cancelled.current) return;
      onToast('success', a.toastLoggedIn);
      onAdded();
      onClose();
    } catch (err) {
      if (!cancelled.current) fail(err);
    } finally {
      waiting.current = false;
      setLoggingIn(false);
    }
  };

  const handleImportResult = (result: { imported: number; failures: string[] }) => {
    if (result.imported > 0) {
      onToast('success', interpolate(a.toastTokensImported, { count: result.imported }));
      onAdded();
    }
    if (result.failures.length > 0) {
      setFailures(result.failures);
    } else if (result.imported > 0) {
      onClose();
    } else {
      onToast('warning', a.cockpitNoneFound);
    }
  };

  const importFile = async () => {
    const path = await api.pickAuthJsonPath();
    if (!path) return;
    setBusy(true);
    setFailures([]);
    try {
      const result = await api.importAccountFile(client, path);
      handleImportResult(result);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const importTokens = async () => {
    setBusy(true);
    setFailures([]);
    try {
      const result = await api.importAccountTokens(client, tokens);
      handleImportResult(result);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const importCockpit = async () => {
    setBusy(true);
    setFailures([]);
    try {
      const result = await api.importCockpitToolsAccounts(client);
      if (result.imported > 0) {
        onToast('success', interpolate(a.toastCockpitImported, { count: result.imported }));
        onAdded();
      }
      if (result.failures.length > 0) {
        setFailures(result.failures);
      } else if (result.imported > 0) {
        onClose();
      } else {
        onToast('info', a.cockpitNoneFound);
      }
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const tabLabel: Record<AddTab, string> = {
    oauth: a.tabOauth,
    token: a.tabTokenJson,
    provider: a.tabProvider,
    local: a.tabLocalImport,
  };

  const oauthModes = useMemo<Array<{ key: OAuthSubMode; label: string }>>(() => {
    if (client === 'codex') {
      return [
        { key: 'pkce', label: a.oauthModeBrowser },
        { key: 'device', label: a.oauthModeDevice },
        { key: 'cli', label: a.oauthModeCli },
      ];
    }
    if (client === 'claude') {
      return [
        { key: 'cli', label: a.oauthModeCli },
        { key: 'pkce', label: a.oauthModeBrowser },
      ];
    }
    return [];
  }, [client, a.oauthModeBrowser, a.oauthModeDevice, a.oauthModeCli]);

  const supportsManualCallback =
    (client === 'antigravity' || oauthSubMode === 'pkce') && loggingIn;

  const panel =
    'flex flex-col items-center gap-3 px-6 py-7 rounded-2xl border border-slate-200/90 dark:border-slate-700/75 bg-gradient-to-b from-slate-50/90 to-white/95 dark:from-[#162035]/85 dark:to-[#111829]/90 shadow-[inset_0_1px_0_rgba(255,255,255,0.7)] dark:shadow-[inset_0_1px_0_rgba(255,255,255,0.04)] text-center';
  const lead = 'text-[13px] font-semibold text-slate-900 dark:text-white';
  const hint = 'text-slate-500 dark:text-slate-400 leading-relaxed max-w-md';

  const tokenPlaceholder = {
    codex: a.tokenPlaceholderCodex,
    claude: a.tokenPlaceholderClaude,
    antigravity: a.tokenPlaceholder,
  }[client];

  return (
    <Modal isOpen onClose={close} title={interpolate(a.addTitle, vars)} widthClass="max-w-2xl">
      <div className="flex flex-col gap-4 text-xs min-h-[320px]">
        <div className="self-start">
          <Segmented
            value={tab}
            onChange={(next) => {
              if (loggingIn) return;
              setTab(next);
              setFailures([]);
            }}
            items={tabs.map((key) => ({ key, label: tabLabel[key] }))}
          />
        </div>

        {tab === 'oauth' && (
          <div className="flex flex-col gap-3">
            {oauthModes.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5">
                {oauthModes.map((mode) => (
                  <button
                    key={mode.key}
                    type="button"
                    disabled={loggingIn}
                    onClick={() => setOauthSubMode(mode.key)}
                    className={`px-3 py-1.5 rounded-full text-xs font-medium border transition-all ${
                      oauthSubMode === mode.key
                        ? 'bg-gradient-to-b from-blue-500 to-blue-600 text-white border-blue-600 shadow-[0_2px_6px_rgba(37,99,235,0.28)]'
                        : 'bg-white/80 dark:bg-[#192338] border-slate-200/90 dark:border-slate-700/75 text-slate-600 dark:text-slate-300 hover:border-blue-300 dark:hover:border-blue-500/40 hover:text-slate-900 dark:hover:text-white'
                    }`}
                  >
                    {mode.label}
                  </button>
                ))}
              </div>
            )}

            {oauthSubMode === 'device' && client === 'codex' ? (
              <div className={panel}>
                <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-blue-500/15 to-indigo-500/15 dark:from-blue-500/20 dark:to-indigo-500/20 border border-blue-200/70 dark:border-blue-400/25 flex items-center justify-center text-blue-600 dark:text-sky-400 shadow-[0_6px_16px_rgba(37,99,235,0.12)]">
                  <Smartphone size={24} />
                </div>
                <div className={lead}>{a.browserTitleCodex}</div>
                <p className={hint}>{a.oauthDeviceDesc}</p>

                {deviceStart ? (
                  <div className="w-full max-w-md flex flex-col items-center gap-2.5 mt-1">
                    <div className="flex items-center gap-2 px-4 py-2.5 rounded-xl bg-slate-100/90 dark:bg-[#0e1524] border border-slate-200/90 dark:border-slate-700/80 shadow-inner">
                      <span className="font-mono text-lg font-bold tracking-widest text-slate-900 dark:text-white select-all">
                        {deviceStart.user_code}
                      </span>
                      <Button
                        size="sm"
                        variant="secondary"
                        icon={deviceCopied ? <Check size={12} /> : <Copy size={12} />}
                        onClick={() =>
                          void navigator.clipboard.writeText(deviceStart.user_code).then(() => {
                            setDeviceCopied(true);
                            onToast('info', a.deviceCodeCopied);
                          })
                        }
                      >
                        {a.deviceCopyCode}
                      </Button>
                    </div>
                    <p className="text-[11px] text-slate-500 dark:text-slate-400">{a.deviceStepHint}</p>
                    <div className="flex flex-wrap items-center justify-center gap-2">
                      <Button
                        variant="primary"
                        icon={<ExternalLink size={13} />}
                        onClick={() => void api.openExternalUrl(deviceStart.verification_url)}
                      >
                        {a.deviceOpenVerify}
                      </Button>
                      <Button onClick={cancelLogin}>{a.cancelLogin}</Button>
                    </div>
                    <p className="flex items-center gap-1.5 text-blue-600 dark:text-sky-400 font-medium">
                      <Loader2 size={13} className="animate-spin" />
                      {a.devicePolling}
                    </p>
                  </div>
                ) : loggingIn ? (
                  <div className="flex items-center gap-2 text-blue-600 dark:text-sky-400 font-medium">
                    <Loader2 size={14} className="animate-spin" />
                    <span>{a.deviceRequesting}</span>
                  </div>
                ) : (
                  <Button variant="primary" icon={<KeyRound size={13} />} onClick={startDeviceAuth}>
                    {a.deviceRequestCode}
                  </Button>
                )}
              </div>
            ) : (
              <>
                <div className={panel}>
                  <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-blue-500/15 to-indigo-500/15 dark:from-blue-500/20 dark:to-indigo-500/20 border border-blue-200/70 dark:border-blue-400/25 flex items-center justify-center text-blue-600 dark:text-sky-400 shadow-[0_6px_16px_rgba(37,99,235,0.12)]">
                    <Globe size={24} />
                  </div>
                  <div className={lead}>
                    {{
                      codex: a.browserTitleCodex,
                      claude: a.browserTitleClaude,
                      antigravity: a.browserTitleAntigravity,
                    }[client]}
                  </div>
                  <p className={hint}>
                    {client === 'antigravity'
                      ? a.browserDescAntigravity
                      : oauthSubMode === 'pkce'
                        ? a.oauthPkceDesc
                        : a.browserDescCli}
                  </p>
                  {loggingIn ? (
                    <>
                      <p className="flex items-center gap-1.5 text-blue-600 dark:text-sky-400 font-medium">
                        <Loader2 size={13} className="animate-spin" />
                        {a.loginWaiting}
                      </p>
                      <Button onClick={cancelLogin}>{a.cancelLogin}</Button>
                    </>
                  ) : (
                    <Button variant="primary" icon={<ExternalLink size={13} />} onClick={startBrowserOAuth}>
                      {a.openBrowser}
                    </Button>
                  )}
                </div>

                {loggingIn && loginUrl && (
                  <div className="flex flex-col gap-1.5">
                    <span className="text-slate-500 dark:text-slate-400">{a.linkHint}</span>
                    <div className="flex items-center gap-2 px-3 py-2 rounded-xl bg-slate-100/90 dark:bg-[#0e1523] border border-slate-200/80 dark:border-slate-800/85 font-mono text-[11px] text-slate-600 dark:text-slate-300">
                      <span className="flex-1 truncate select-text" title={loginUrl}>
                        {loginUrl}
                      </span>
                      <Button
                        size="sm"
                        variant="ghost"
                        icon={<Copy size={12} />}
                        onClick={() =>
                          void navigator.clipboard
                            .writeText(loginUrl)
                            .then(() => onToast('info', a.linkCopied))
                        }
                      >
                        {a.copyLink}
                      </Button>
                    </div>
                  </div>
                )}

                {supportsManualCallback && (
                  <div className="flex flex-col gap-1.5 p-3.5 rounded-xl bg-slate-50/90 dark:bg-[#0e1523]/85 border border-slate-200/80 dark:border-slate-800/85">
                    <span className="text-slate-600 dark:text-slate-300 font-medium">{a.oauthManualLabel}</span>
                    <div className="flex items-center gap-2">
                      <input
                        value={manualCallback}
                        onChange={(e) => setManualCallback(e.target.value)}
                        placeholder={a.oauthManualPlaceholder}
                        className="flex-1 bg-white dark:bg-[#141d2f] px-3 py-1.5 rounded-lg border border-slate-200/90 dark:border-slate-700/80 font-mono text-[11px] focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15 focus:outline-none"
                      />
                      <Button
                        size="sm"
                        variant="secondary"
                        icon={<Send size={12} />}
                        disabled={busy || !manualCallback.trim()}
                        onClick={submitManualCallback}
                      >
                        {a.oauthManualSubmit}
                      </Button>
                    </div>
                  </div>
                )}
              </>
            )}
          </div>
        )}

        {tab === 'token' && (
          <div className="flex flex-col gap-3">
            <p className="text-slate-500 dark:text-slate-400 leading-relaxed">{a.tokenDescAll}</p>
            <textarea
              value={tokens}
              onChange={(e) => setTokens(e.target.value)}
              placeholder={tokenPlaceholder}
              aria-label={a.methodToken}
              spellCheck={false}
              className="w-full h-36 bg-slate-50/90 dark:bg-[#0e1523] text-slate-800 dark:text-slate-100 px-3.5 py-2.5 rounded-xl border border-slate-200/90 dark:border-slate-700/80 shadow-[inset_0_1px_2px_rgba(15,23,42,0.03)] focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15 focus:bg-white dark:focus:bg-[#141d2f] focus:outline-none placeholder-slate-400 dark:placeholder-slate-500 font-mono text-[11px] resize-none transition-all"
            />
            {failures.length > 0 && (
              <div className="flex flex-col gap-1 text-[11px] text-rose-600 dark:text-rose-400 p-2.5 rounded-xl bg-rose-50/80 dark:bg-rose-500/10 border border-rose-200/80 dark:border-rose-500/25">
                <span className="flex items-center gap-1.5 font-medium">
                  <AlertTriangle size={12} />
                  {a.tokenFailures}
                </span>
                {failures.map((f) => (
                  <span key={f} className="break-all pl-[18px]">
                    {f}
                  </span>
                ))}
              </div>
            )}
            <div className="flex flex-wrap items-center justify-between gap-2">
              <Button
                variant="secondary"
                icon={<FileUp size={13} />}
                disabled={busy}
                onClick={importFile}
              >
                {a.tokenChooseFile}
              </Button>
              <Button
                variant="primary"
                disabled={busy || !tokens.trim()}
                onClick={importTokens}
                icon={busy ? <Loader2 size={13} className="animate-spin" /> : undefined}
              >
                {busy ? a.tokenImporting : a.tokenImport}
              </Button>
            </div>
          </div>
        )}

        {tab === 'provider' && client !== 'antigravity' && (
          <RelayForm
            client={client}
            busy={busy}
            onCancel={close}
            onSave={(input) =>
              run(async () => {
                if (client === 'codex') {
                  await api.upsertCodexProvider({
                    id: input.id,
                    name: input.name,
                    api_key: input.token ?? '',
                    base_url: input.base_url || null,
                    model: input.model || null,
                  });
                } else {
                  await api.upsertClaudeProvider(input);
                }
                return a.toastRelaySaved;
              })
            }
          />
        )}

        {tab === 'local' && (
          <div className="flex flex-col gap-3">
            {/* Save current login */}
            <div className="p-4 rounded-2xl border border-slate-200/85 dark:border-slate-700/75 bg-white/90 dark:bg-[#151f33]/90 hover:border-blue-300/80 dark:hover:border-blue-400/35 shadow-[0_2px_8px_rgba(15,23,42,0.03)] flex flex-wrap items-center justify-between gap-3 transition-colors">
              <div className="flex items-start gap-3 min-w-0">
                <div className="w-9 h-9 rounded-xl bg-blue-50 dark:bg-blue-500/15 border border-blue-200/75 dark:border-blue-400/25 flex items-center justify-center text-blue-600 dark:text-sky-400 shrink-0">
                  <HardDriveDownload size={18} />
                </div>
                <div className="min-w-0">
                  <div className={lead}>{a.localCurrentTitle}</div>
                  <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                    {current
                      ? current.email || current.name || interpolate(a.currentDesc, vars)
                      : interpolate(a.currentNone, vars)}
                  </p>
                </div>
              </div>
              <Button
                variant="primary"
                size="sm"
                icon={<Save size={13} />}
                disabled={busy || !current || current.saved}
                onClick={() =>
                  run(async () => {
                    await api.saveCurrentAccount(client);
                    return a.toastSaved;
                  })
                }
              >
                {current?.saved ? a.currentSaved : a.saveCurrent}
              </Button>
            </div>

            {/* One-click import from Cockpit Tools */}
            <div className="p-4 rounded-2xl border border-slate-200/85 dark:border-slate-700/75 bg-white/90 dark:bg-[#151f33]/90 hover:border-blue-300/80 dark:hover:border-blue-400/35 shadow-[0_2px_8px_rgba(15,23,42,0.03)] flex flex-wrap items-center justify-between gap-3 transition-colors">
              <div className="flex items-start gap-3 min-w-0 flex-1">
                <div className="w-9 h-9 rounded-xl bg-indigo-50 dark:bg-indigo-500/15 border border-indigo-200/75 dark:border-indigo-400/25 flex items-center justify-center text-indigo-600 dark:text-indigo-300 shrink-0">
                  <Download size={18} />
                </div>
                <div className="min-w-0">
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className={lead}>{a.localCockpitTitle}</span>
                    {cockpitImportCount > 0 && (
                      <span className="px-2 py-0.5 rounded-full text-[11px] font-medium bg-blue-50 dark:bg-blue-500/15 text-blue-600 dark:text-sky-300 ring-1 ring-inset ring-blue-200/80 dark:ring-blue-400/25">
                        {cockpitImportCount}
                      </span>
                    )}
                  </div>
                  <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                    {interpolate(a.localCockpitDesc, {
                      client: CLIENT_LABEL[client],
                      count: cockpitImportCount,
                    })}
                  </p>
                </div>
              </div>
              <Button
                variant={cockpitImportCount > 0 ? 'primary' : 'secondary'}
                size="sm"
                icon={<Download size={13} />}
                disabled={busy}
                onClick={importCockpit}
              >
                {a.cockpitImportNow}
              </Button>
            </div>

            {/* One-click import from cc-switch (Codex & Claude) */}
            {client !== 'antigravity' && (
              <div className="p-4 rounded-2xl border border-slate-200/85 dark:border-slate-700/75 bg-white/90 dark:bg-[#151f33]/90 hover:border-blue-300/80 dark:hover:border-blue-400/35 shadow-[0_2px_8px_rgba(15,23,42,0.03)] flex flex-wrap items-center justify-between gap-3 transition-colors">
                <div className="flex items-start gap-3 min-w-0 flex-1">
                  <div className="w-9 h-9 rounded-xl bg-amber-50 dark:bg-amber-500/15 border border-amber-200/75 dark:border-amber-400/25 flex items-center justify-center text-amber-600 dark:text-amber-300 shrink-0">
                    <Download size={18} />
                  </div>
                  <div className="min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className={lead}>{a.localCcSwitchTitle}</span>
                      {!ccSwitchAvailable && (
                        <span className="px-2 py-0.5 rounded-full text-[11px] bg-slate-100 dark:bg-slate-800 text-slate-500 dark:text-slate-400 ring-1 ring-inset ring-slate-200/80 dark:ring-slate-700/75">
                          {a.localNotDetected}
                        </span>
                      )}
                    </div>
                    <p className="text-slate-500 dark:text-slate-400 mt-0.5">
                      {interpolate(a.localCcSwitchDesc, vars)}
                    </p>
                  </div>
                </div>
                <Button
                  variant="secondary"
                  size="sm"
                  disabled={busy}
                  onClick={() =>
                    run(async () =>
                      interpolate(a.toastCcSwitchImported, {
                        count: await api.importCcSwitchProviders(client),
                      })
                    )
                  }
                >
                  {a.importCcSwitch}
                </Button>
              </div>
            )}

            {failures.length > 0 && (
              <div className="flex flex-col gap-1 text-[11px] text-rose-600 dark:text-rose-400 p-2.5 rounded-xl bg-rose-50/80 dark:bg-rose-500/10 border border-rose-200/80 dark:border-rose-500/25">
                <span className="flex items-center gap-1.5 font-medium">
                  <AlertTriangle size={12} />
                  {a.tokenFailures}
                </span>
                {failures.map((f) => (
                  <span key={f} className="break-all pl-[18px]">
                    {f}
                  </span>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </Modal>
  );
};
