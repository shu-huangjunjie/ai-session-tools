import React, { useEffect, useRef, useState } from 'react';
import { AlertTriangle, Copy, Download, ExternalLink, FileUp, Globe, HardDriveDownload, Loader2, Save } from 'lucide-react';
import { api, errorMessage } from '../api/client';
import { AccountClient, CurrentLogin } from '../types';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { Segmented } from './common/Segmented';
import { RelayForm } from './RelayForm';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';
type Method = 'browser' | 'token' | 'file' | 'relay' | 'ccswitch' | 'current';

/** The ways each client can add an account; the first is the default. */
const METHODS: Record<AccountClient, Method[]> = {
  codex: ['browser', 'file', 'current'],
  claude: ['browser', 'relay', 'ccswitch', 'current'],
  antigravity: ['browser', 'token', 'current'],
};
const CLIENT_LABEL: Record<AccountClient, string> = { codex: 'Codex', claude: 'Claude', antigravity: 'Antigravity' };

interface AccountAddDialogProps {
  client: AccountClient;
  /** Who is signed in to the client now, for importing that login. */
  current?: CurrentLogin | null;
  onClose: () => void;
  /** Called after an account was added, so the list reloads. */
  onAdded: () => void;
  onToast: (type: ToastType, message: string) => void;
}

/** One dialog for every way of adding an account, like cockpit-tools: browser sign-in, tokens, files and the current login. */
export const AccountAddDialog: React.FC<AccountAddDialogProps> = ({ client, current, onClose, onAdded, onToast }) => {
  const { t, interpolate } = useI18n();
  const a = t.accounts;
  const [method, setMethod] = useState<Method>(METHODS[client][0]);
  const [busy, setBusy] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [loginUrl, setLoginUrl] = useState<string | null>(null);
  const [tokens, setTokens] = useState('');
  const [failures, setFailures] = useState<string[]>([]);
  const cancelled = useRef(false);
  // Read on unmount, when state from the first render would be stale.
  const waiting = useRef(false);
  const vars = { client: CLIENT_LABEL[client] };

  const fail = (err: unknown) => onToast('error', interpolate(a.toastFailed, { error: errorMessage(err) }));

  /** Runs one action; on success the list reloads and the dialog closes. */
  const run = async (action: () => Promise<string | void>) => {
    setBusy(true);
    try {
      const message = await action();
      if (message) onToast('success', message);
      onAdded();
      onClose();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const login = async () => {
    cancelled.current = false;
    waiting.current = true;
    setLoggingIn(true);
    setLoginUrl(null);
    const stop = await api.onAccountLoginUrl(setLoginUrl);
    try {
      await api.loginAccount(client);
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

  const cancelLogin = () => {
    cancelled.current = true;
    void api.cancelAccountLogin();
  };

  // Closing the dialog must not leave a sign-in waiting in the background.
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

  const importFile = async () => {
    const path = await api.pickAuthJsonPath();
    if (!path) return;
    await run(async () => {
      await api.importAccountFile(client, path);
      return a.toastImported;
    });
  };

  const importTokens = async () => {
    setBusy(true);
    setFailures([]);
    try {
      const result = await api.importAccountTokens(client, tokens);
      if (result.imported > 0) {
        onToast('success', interpolate(a.toastTokensImported, { count: result.imported }));
        onAdded();
      }
      if (result.failures.length > 0) setFailures(result.failures);
      else onClose();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  const label: Record<Method, string> = {
    browser: { codex: a.methodBrowserCodex, claude: a.methodBrowserClaude, antigravity: a.methodBrowserAntigravity }[client],
    token: a.methodToken,
    file: a.methodFile,
    relay: a.addRelay,
    ccswitch: a.importCcSwitch,
    current: a.methodCurrent,
  };
  const panel = 'flex flex-col items-center gap-2.5 px-6 py-7 rounded-2xl border border-dashed border-black/[0.15] dark:border-white/[0.15] text-center';
  const lead = 'text-[13px] font-semibold text-[#1d1d1f] dark:text-white';
  const hint = 'text-[#86868b] leading-relaxed max-w-sm';

  return (
    <Modal isOpen onClose={close} title={interpolate(a.addTitle, vars)} widthClass="max-w-xl">
      <div className="flex flex-col gap-4 text-xs min-h-[300px]">
        <div className="self-start">
          <Segmented
            value={method}
            onChange={(next) => {
              if (!loggingIn) setMethod(next);
            }}
            items={METHODS[client].map((m) => ({ key: m, label: label[m] }))}
          />
        </div>

        {method === 'browser' && (
          <>
            <div className={panel}>
              <Globe size={28} className="text-[#0071e3] dark:text-[#2997ff]" />
              <div className={lead}>
                {{ codex: a.browserTitleCodex, claude: a.browserTitleClaude, antigravity: a.browserTitleAntigravity }[client]}
              </div>
              <p className={hint}>{client === 'antigravity' ? a.browserDescAntigravity : a.browserDescCli}</p>
              {loggingIn ? (
                <>
                  <p className="flex items-center gap-1.5 text-[#0071e3] dark:text-[#2997ff]">
                    <Loader2 size={13} className="animate-spin" />
                    {a.loginWaiting}
                  </p>
                  <Button onClick={cancelLogin}>{a.cancelLogin}</Button>
                </>
              ) : (
                <Button variant="primary" icon={<ExternalLink size={13} />} onClick={login}>
                  {a.openBrowser}
                </Button>
              )}
            </div>
            {loggingIn && loginUrl && (
              <div className="flex flex-col gap-1.5">
                <span className="text-[#86868b]">{a.linkHint}</span>
                <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-black/[0.04] dark:bg-white/[0.06] font-mono text-[11px] text-[#6e6e73] dark:text-[#a1a1a6]">
                  <span className="flex-1 truncate select-text" title={loginUrl}>{loginUrl}</span>
                  <Button
                    size="sm"
                    variant="ghost"
                    icon={<Copy size={12} />}
                    onClick={() => void navigator.clipboard.writeText(loginUrl).then(() => onToast('info', a.linkCopied))}
                  >
                    {a.copyLink}
                  </Button>
                </div>
              </div>
            )}
          </>
        )}

        {method === 'token' && (
          <div className="flex flex-col gap-2.5">
            <p className={hint.replace(' max-w-sm', '')}>{a.tokenDesc}</p>
            <textarea
              value={tokens}
              onChange={(e) => setTokens(e.target.value)}
              placeholder={a.tokenPlaceholder}
              aria-label={a.methodToken}
              spellCheck={false}
              className="w-full h-32 bg-black/[0.04] dark:bg-white/[0.06] px-3 py-2 rounded-xl border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b] font-mono text-[11px] resize-none"
            />
            {failures.length > 0 && (
              <div className="flex flex-col gap-1 text-[11px] text-[#d70015] dark:text-[#ff453a]">
                <span className="flex items-center gap-1.5"><AlertTriangle size={12} />{a.tokenFailures}</span>
                {failures.map((f) => <span key={f} className="break-all pl-[18px]">{f}</span>)}
              </div>
            )}
            <Button variant="primary" className="self-end" disabled={busy || !tokens.trim()} onClick={importTokens} icon={busy ? <Loader2 size={13} className="animate-spin" /> : undefined}>
              {busy ? a.tokenImporting : a.tokenImport}
            </Button>
          </div>
        )}

        {method === 'file' && (
          <div className={panel}>
            <FileUp size={28} className="text-[#0071e3] dark:text-[#2997ff]" />
            <p className={hint}>{a.fileDesc}</p>
            <Button variant="primary" disabled={busy} onClick={importFile}>
              {a.fileChoose}
            </Button>
          </div>
        )}

        {method === 'relay' && (
          <RelayForm
            busy={busy}
            onCancel={close}
            onSave={(input) =>
              run(async () => {
                await api.upsertClaudeProvider(input);
                return a.toastRelaySaved;
              })
            }
          />
        )}

        {method === 'ccswitch' && (
          <div className={panel}>
            <Download size={28} className="text-[#0071e3] dark:text-[#2997ff]" />
            <p className={hint}>{a.ccSwitchDesc}</p>
            <Button
              variant="primary"
              disabled={busy}
              onClick={() =>
                run(async () => interpolate(a.toastImportedCount, { count: await api.importCcSwitchClaudeProviders() }))
              }
            >
              {a.importCcSwitch}
            </Button>
          </div>
        )}

        {method === 'current' && (
          <div className={panel}>
            <HardDriveDownload size={28} className="text-[#0071e3] dark:text-[#2997ff]" />
            <p className={hint}>{interpolate(a.currentDesc, vars)}</p>
            {current ? (
              <>
                <div className={lead}>{current.email || current.name}</div>
                <Button
                  variant="primary"
                  icon={<Save size={13} />}
                  disabled={busy || current.saved}
                  onClick={() =>
                    run(async () => {
                      await api.saveCurrentAccount(client);
                      return a.toastSaved;
                    })
                  }
                >
                  {current.saved ? a.currentSaved : a.saveCurrent}
                </Button>
              </>
            ) : (
              <p className="text-[#86868b]">{interpolate(a.currentNone, vars)}</p>
            )}
          </div>
        )}
      </div>
    </Modal>
  );
};
