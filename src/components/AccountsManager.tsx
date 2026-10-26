import React, { useCallback, useEffect, useState } from 'react';
import { AlertTriangle, ArrowRightLeft, ChevronRight, Pencil, Plus, RefreshCw, Save, Trash2 } from 'lucide-react';
import { api, errorMessage } from '../api/client';
import { AccountClient, AccountsState, AccountView, QuotaWindow } from '../types';
import { formatDateTime } from '../utils/formatters';
import { groupModelQuotas, remainingColor } from '../utils/quotaGroups';
import { Badge } from './common/Badge';
import { Button } from './common/Button';
import { PageHeader } from './common/PageHeader';
import { Segmented } from './common/Segmented';
import { Switch } from './common/Switch';
import { AccountAddDialog } from './AccountAddDialog';
import { RelayForm } from './RelayForm';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';

interface AccountsManagerProps {
  onToast: (type: ToastType, message: string) => void;
}

const CLIENTS: AccountClient[] = ['codex', 'claude', 'antigravity'];
const TAB_KEY = 'cv_accounts_tab';
const CLIENT_COLOR: Record<AccountClient, string> = { codex: '#0071e3', claude: '#d97706', antigravity: '#af52de' };

/**
 * Whether switching restarts the client app. Every tab has its own accounts and never touches another
 * client's files; the ways of adding an account are in the add dialog.
 */
const FEATURES: Record<AccountClient, { restart: boolean }> = {
  codex: { restart: true },
  claude: { restart: false },
  antigravity: { restart: true },
};

const restartKey = (client: AccountClient) => `cv_${client}_restart_after_switch`;

function readFlag(key: string, fallback: boolean): boolean {
  try {
    const v = localStorage.getItem(key);
    return v === null ? fallback : v !== 'false';
  } catch {
    return fallback;
  }
}

function writeFlag(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // The choice still applies for this session.
  }
}

/** Bar colour by how much is used. */
const barColor = (used: number) => (used >= 90 ? 'bg-[#ff3b30]' : used >= 70 ? 'bg-[#ff9f0a]' : 'bg-[#34c759]');

/** Initial of an account in a tinted circle. */
const Avatar: React.FC<{ text: string; color: string; size: number }> = ({ text, color, size }) => (
  <div
    aria-hidden="true"
    className="rounded-full grid place-items-center font-semibold flex-shrink-0"
    style={{ background: `${color}1f`, color, width: size, height: size, fontSize: Math.round(size * 0.38) }}
  >
    {(text.trim().charAt(0) || '?').toUpperCase()}
  </div>
);

export const AccountsManager: React.FC<AccountsManagerProps> = ({ onToast }) => {
  const { t } = useI18n();
  const a = t.accounts;
  const [tab, setTab] = useState<AccountClient>(() => {
    try {
      const saved = localStorage.getItem(TAB_KEY) as AccountClient | null;
      return saved && CLIENTS.includes(saved) ? saved : 'codex';
    } catch {
      return 'codex';
    }
  });
  const label: Record<AccountClient, string> = { codex: a.tabCodex, claude: a.tabClaude, antigravity: a.tabAntigravity };

  const tabs = (
    <Segmented
      value={tab}
      onChange={(client) => {
        setTab(client);
        writeFlag(TAB_KEY, client);
      }}
      items={CLIENTS.map((client) => ({ key: client, label: label[client], dot: CLIENT_COLOR[client] }))}
    />
  );

  // One panel per client, remounted on tab change so no state is shared.
  return <ClientPanel key={tab} client={tab} tabs={tabs} onToast={onToast} />;
};

interface ClientPanelProps {
  client: AccountClient;
  tabs: React.ReactNode;
  onToast: (type: ToastType, message: string) => void;
}

const ClientPanel: React.FC<ClientPanelProps> = ({ client, tabs, onToast }) => {
  const { t, interpolate } = useI18n();
  const a = t.accounts;
  const features = FEATURES[client];
  const [state, setState] = useState<AccountsState | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  /** The relay being edited; new relays are added through the add dialog. */
  const [relayForm, setRelayForm] = useState<AccountView | null>(null);
  /** Accounts whose full model list is open. */
  const [showModels, setShowModels] = useState<Set<string>>(() => new Set());
  const [restart, setRestart] = useState(() => readFlag(restartKey(client), true));

  const fail = useCallback((err: unknown) => onToast('error', interpolate(a.toastFailed, { error: errorMessage(err) })), [onToast, interpolate, a.toastFailed]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setState(await api.listAccounts(client));
    } catch (err) {
      fail(err);
    } finally {
      setLoading(false);
    }
  }, [client, fail]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  /** Runs one action at a time; `key` marks which button shows progress. */
  const run = async (key: string, action: () => Promise<void>) => {
    setBusy(key);
    try {
      await action();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(null);
      await refresh();
    }
  };

  const accountLabel = (account: AccountView) => account.name && account.kind === 'provider' ? account.name : account.email || account.name || account.account_id || account.id;

  const relative = (iso: string) => {
    const minutes = Math.max(0, Math.round((new Date(iso).getTime() - Date.now()) / 60000));
    if (minutes < 60) return interpolate(a.minutes, { n: minutes });
    if (minutes < 48 * 60) return interpolate(a.hours, { n: Math.round(minutes / 6) / 10 });
    return interpolate(a.days, { n: Math.round(minutes / 144) / 10 });
  };

  const windowLabel = (w: QuotaWindow) => {
    const minutes = w.window_minutes ?? null;
    const length =
      minutes === 300 || (minutes === null && w.kind === 'primary')
        ? a.primary
        : minutes === 10080 || (minutes === null && w.kind === 'secondary')
          ? a.secondary
          : minutes
            ? minutes % 1440 === 0
              ? interpolate(a.windowDays, { days: minutes / 1440 })
              : interpolate(a.windowHours, { hours: Math.round(minutes / 6) / 10 })
            : null;
    if (w.label && length) return `${length} · ${w.label}`;
    return w.label || length || w.kind;
  };

  const switchTo = (account: AccountView) =>
    run(`switch:${account.id}`, async () => {
      const restarted = await api.switchAccount(client, account.id, features.restart && restart);
      onToast('success', interpolate(restarted ? a.toastSwitchedRestarted : a.toastSwitched, { name: accountLabel(account) }));
    });

  const accounts = state?.accounts ?? [];
  const currentAccount = accounts.find((x) => x.is_current);
  const others = accounts.filter((x) => !x.is_current);
  const disabled = busy !== null;
  const current = state?.current;
  /**
   * Subscriptions and relays are different things, so every account, the current login too, sits in one of
   * two groups. Codex and Antigravity accounts are all subscriptions.
   */
  const groupOf = (kind?: string | null) => (kind === 'provider' ? 'provider' : 'oauth');
  const groups = [
    { key: 'oauth', title: a.officialAccounts },
    { key: 'provider', title: a.relayAccounts },
  ]
    .map((g) => ({ ...g, hasCurrent: Boolean(current) && groupOf(current?.kind) === g.key, items: others.filter((x) => groupOf(x.kind) === g.key) }))
    .filter((g) => g.hasCurrent || g.items.length > 0);
  const hint = { codex: a.hintCodex, claude: a.hintClaude, antigravity: a.hintAntigravity }[client];
  const color = CLIENT_COLOR[client];


  const toggleModels = (id: string) =>
    setShowModels((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  /** Every model's quota, in a column per family. */
  const modelList = (windows: QuotaWindow[]) => (
    <div className="p-3.5 rounded-xl bg-black/[0.025] dark:bg-white/[0.04] grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-x-8 gap-y-4 text-[11px]">
      {groupModelQuotas(windows).map((g) => (
        <div key={g.key} className="flex flex-col gap-1.5 min-w-0">
          <span className="flex items-center gap-1.5 text-[10.5px] font-medium text-[#86868b]">
            <span className="w-[6px] h-[6px] rounded-full shrink-0" style={{ background: g.color }} />
            {g.name}
          </span>
          {g.models.map((m) => (
            <div
              key={m.name}
              className="flex items-center gap-2"
              title={m.remaining < 100 && m.resetsAt ? interpolate(a.resetsIn, { time: relative(m.resetsAt) }) : undefined}
            >
              <span className="truncate text-[#1d1d1f] dark:text-[#ededf0]">{m.name}</span>
              <span className={`ml-auto tabular-nums ${m.remaining <= 30 ? 'font-semibold text-[#c93400] dark:text-[#ff9f0a]' : 'text-[#6e6e73] dark:text-[#a1a1a6]'}`}>
                {m.remaining}%
              </span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );

  /**
   * Antigravity has a quota per model: each family is one line, and the full list is one click away.
   * The current card shows that list across its whole width instead of under the lines.
   */
  const groupedQuota = (account: AccountView, windows: QuotaWindow[], listInline: boolean) => {
    const open = showModels.has(account.id);
    return (
      <div className="flex flex-col gap-2">
        <span className="text-[11px] text-[#86868b]">{a.quotaRemaining}</span>
        <div className="grid grid-cols-[auto_minmax(60px,1fr)_auto] items-center gap-x-3 gap-y-2.5">
          {groupModelQuotas(windows).map((g) => (
            <React.Fragment key={g.key}>
              <span className="flex items-center gap-1.5 min-w-0 text-[11.5px]" title={interpolate(a.modelsInGroup, { count: g.models.length })}>
                <span className="w-[7px] h-[7px] rounded-full shrink-0" style={{ background: g.color }} />
                <span className="font-semibold text-[#1d1d1f] dark:text-white truncate">{g.name}</span>
                {g.models.length > 1 && <span className="text-[10.5px] font-mono text-[#86868b]">{g.models.length}</span>}
              </span>
              <div
                className="h-1.5 rounded-full bg-black/[0.06] dark:bg-white/[0.08] overflow-hidden"
                role="progressbar"
                aria-valuenow={g.remaining}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuetext={interpolate(a.remaining, { percent: g.remaining })}
                aria-label={g.name}
              >
                <div className={`h-full rounded-full ${remainingColor(g.remaining)}`} style={{ width: `${g.remaining}%` }} />
              </div>
              <span className="text-right text-[11px] tabular-nums whitespace-nowrap">
                <span className={g.remaining <= 30 ? 'font-semibold text-[#c93400] dark:text-[#ff9f0a]' : 'font-medium text-[#1d1d1f] dark:text-white'}>
                  {g.remaining}%
                </span>
                {g.remaining < 100 && g.resetsAt && (
                  <span className="ml-1.5 text-[#86868b]" title={formatDateTime(g.resetsAt)}>
                    {interpolate(a.resetsIn, { time: relative(g.resetsAt) })}
                  </span>
                )}
              </span>
            </React.Fragment>
          ))}
        </div>
        <button
          type="button"
          aria-expanded={open}
          onClick={() => toggleModels(account.id)}
          className="w-fit flex items-center gap-1 text-[11px] text-[#0071e3] dark:text-[#2997ff] hover:underline"
        >
          <ChevronRight size={12} className={`transition-transform ${open ? 'rotate-90' : ''}`} />
          {interpolate(a.allModels, { count: windows.length })}
        </button>
        {listInline && open && modelList(windows)}
      </div>
    );
  };

  const quotaView = (account: AccountView, listInline = true) => (
    <>
      {!account.has_quota ? (
        <span className="text-[#86868b]">{a.relayNoQuota}</span>
      ) : account.quota && account.quota.windows.length > 0 && client === 'antigravity' ? (
        groupedQuota(account, account.quota.windows, listInline)
      ) : account.quota && account.quota.windows.length > 0 ? (
        <div className="flex flex-col gap-2.5">
          {account.quota.windows.map((w) => (
            <div key={w.kind} className="flex flex-col gap-1">
              <div className="flex items-center gap-2 text-[11px]">
                <span className="font-semibold text-[#1d1d1f] dark:text-white">{windowLabel(w)}</span>
                <span className="text-[#6e6e73] dark:text-[#a1a1a6]">{interpolate(a.used, { percent: Math.round(w.used_percent) })}</span>
                {w.resets_at && (
                  <span className="ml-auto text-[#86868b]" title={formatDateTime(w.resets_at)}>
                    {interpolate(a.resetsIn, { time: relative(w.resets_at) })}
                  </span>
                )}
              </div>
              <div
                className="h-1.5 rounded-full bg-black/[0.06] dark:bg-white/[0.08] overflow-hidden"
                role="progressbar"
                aria-valuenow={Math.round(w.used_percent)}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-label={windowLabel(w)}
              >
                <div className={`h-full rounded-full ${barColor(w.used_percent)}`} style={{ width: `${Math.min(100, w.used_percent)}%` }} />
              </div>
            </div>
          ))}
          {account.quota.credits_balance && (
            <span className="text-[11px] text-[#6e6e73] dark:text-[#a1a1a6]">
              {interpolate(a.credits, { balance: account.quota.credits_balance === 'unlimited' ? a.unlimited : account.quota.credits_balance })}
            </span>
          )}
        </div>
      ) : (
        !account.quota_error && <span className="text-[#86868b]">{a.noQuota}</span>
      )}
      {account.quota_error && (
        <p className="flex items-start gap-1.5 text-[11px] text-[#d70015] dark:text-[#ff453a]">
          <AlertTriangle size={12} className="mt-0.5 shrink-0" />
          {account.quota_error}
        </p>
      )}
    </>
  );

  const timestamps = (account: AccountView) => (
    <div className="flex items-center gap-2 text-[11px] text-[#86868b] flex-wrap">
      {account.quota_updated_at && <span>{interpolate(a.updated, { time: formatDateTime(account.quota_updated_at) })}</span>}
      {account.last_used_at && <span>· {interpolate(a.lastUsed, { time: formatDateTime(account.last_used_at) })}</span>}
    </div>
  );

  /** Refresh, edit and delete; switching is offered only for accounts that are not current. */
  const secondaryActions = (account: AccountView) => (
    <>
      {account.has_quota && (
        <Button
          size="sm"
          icon={<RefreshCw size={13} className={busy === `quota:${account.id}` ? 'animate-spin' : ''} />}
          disabled={disabled}
          onClick={() =>
            run(`quota:${account.id}`, async () => {
              await api.refreshAccountQuota(client, account.id);
              onToast('success', a.toastRefreshed);
            })
          }
        >
          {a.refresh}
        </Button>
      )}
      {account.kind === 'provider' && (
        <Button size="sm" icon={<Pencil size={13} />} disabled={disabled} onClick={() => setRelayForm(account)}>
          {a.edit}
        </Button>
      )}
      <div className="flex-1" />
      <Button
        size="sm"
        variant="ghost"
        icon={<Trash2 size={13} />}
        disabled={disabled}
        onClick={() => setConfirmDelete(account.id)}
        title={a.delete}
        aria-label={`${a.delete} ${accountLabel(account)}`}
        className="px-2"
      />
    </>
  );

  const deleteConfirmation = (account: AccountView) =>
    confirmDelete === account.id && (
      <div className="p-3 rounded-xl border border-[#ff453a]/25 bg-[#ff453a]/[0.06] flex items-center gap-3">
        <p className="flex-1 leading-relaxed">{a.deleteConfirm}</p>
        <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(null)}>
          {t.common.cancel}
        </Button>
        <Button
          size="sm"
          variant="danger"
          disabled={disabled}
          onClick={() =>
            run(`delete:${account.id}`, async () => {
              await api.deleteAccount(client, account.id);
              setConfirmDelete(null);
              onToast('success', a.toastDeleted);
            })
          }
        >
          {a.confirmDelete}
        </Button>
      </div>
    );

  const restartLabel = client === 'codex' ? a.restartCodex : a.restartAntigravity;

  /** The current login, with the saved account's quota when there is one. */
  const currentCard = (
    <section
      aria-label={a.currentLogin}
      className="p-5 rounded-2xl bg-white dark:bg-[#161618] border border-black/[0.06] dark:border-white/[0.08] flex flex-col gap-4"
    >
      <div className="flex items-start gap-4 flex-wrap">
        <Avatar text={current?.email || current?.name || '?'} color={color} size={44} />
        <div className="flex flex-col gap-1 min-w-[220px]">
          <span className="text-[11px] text-[#86868b]">{a.currentLogin}</span>
          <span className="font-semibold text-[15px] text-[#1d1d1f] dark:text-white break-all" title={state?.auth_path}>
            {current ? current.email || current.name || '—' : a.noLogin}
          </span>
          {currentAccount?.detail && <span className="text-[11px] font-mono text-[#6e6e73] dark:text-[#a1a1a6] break-all">{currentAccount.detail}</span>}
          <div className="flex items-center gap-1.5 flex-wrap mt-0.5">
            {current?.plan_type && <Badge tone="blue">{current.plan_type}</Badge>}
            {current && !current.saved && <Badge tone="orange">{a.notSaved}</Badge>}
            {current && !current.saved && (
              <Button
                size="sm"
                icon={<Save size={13} />}
                disabled={disabled}
                onClick={() =>
                  run('save', async () => {
                    await api.saveCurrentAccount(client);
                    onToast('success', a.toastSaved);
                  })
                }
              >
                {a.saveCurrent}
              </Button>
            )}
          </div>
        </div>
        {currentAccount && <div className="flex-1 min-w-[260px] max-w-[480px] flex flex-col gap-2">{quotaView(currentAccount, false)}</div>}
        <div className="flex-1" />
        {features.restart && (
          <label className="flex items-center gap-2 cursor-pointer text-[#6e6e73] dark:text-[#a1a1a6]">
            <Switch
              checked={restart}
              label={restartLabel}
              onChange={(checked) => {
                setRestart(checked);
                writeFlag(restartKey(client), String(checked));
              }}
            />
            {restartLabel}
          </label>
        )}
      </div>
      {currentAccount?.quota && client === 'antigravity' && showModels.has(currentAccount.id) && modelList(currentAccount.quota.windows)}
      {currentAccount && (
        <div className="flex items-center gap-2 flex-wrap pt-3 border-t border-black/[0.05] dark:border-white/[0.06]">
          {timestamps(currentAccount)}
          <div className="w-2" />
          {secondaryActions(currentAccount)}
        </div>
      )}
      {currentAccount && deleteConfirmation(currentAccount)}
    </section>
  );

  return (
    <div className="flex-1 flex flex-col min-h-0 text-xs">
      <PageHeader title={a.button} tabs={tabs}>
        <Button
          icon={<RefreshCw size={13} className={busy === 'all' ? 'animate-spin' : ''} />}
          disabled={disabled || !accounts.some((x) => x.has_quota)}
          onClick={() =>
            run('all', async () => {
              const failures = await api.refreshAllAccountQuotas(client);
              if (failures.length > 0) onToast('warning', interpolate(a.toastRefreshFailed, { count: failures.length }));
              else onToast('success', a.toastRefreshed);
            })
          }
        >
          {busy === 'all' ? a.refreshing : a.refreshAll}
        </Button>
        <Button variant="primary" icon={<Plus size={13} />} disabled={disabled} onClick={() => setAdding(true)}>
          {a.addAccount}
        </Button>
      </PageHeader>

      <div className="flex-1 overflow-y-auto px-6 py-5 select-text">
        <div className="max-w-[1200px] flex flex-col gap-4">
          {[state?.current_note, ...(state?.notes ?? [])].filter(Boolean).map((note) => (
            <p key={note} className="flex items-start gap-1.5 text-[#c93400] dark:text-[#ff9f0a]">
              <AlertTriangle size={12} className="mt-0.5 shrink-0" />
              {note}
            </p>
          ))}

          {relayForm && (
            <RelayForm
              account={relayForm}
              busy={disabled}
              onCancel={() => setRelayForm(null)}
              onSave={(input) =>
                run('relay', async () => {
                  await api.upsertClaudeProvider(input);
                  setRelayForm(null);
                  onToast('success', a.toastRelaySaved);
                })
              }
            />
          )}

          {/* Nobody is signed in: say so above the groups, once the accounts have loaded. */}
          {state && !current && currentCard}

          {groups.map((group) => (
            <section key={group.key} aria-label={group.title} className="flex flex-col gap-2.5">
              <h3 className="text-[12.5px] font-medium text-[#86868b] flex items-center gap-2">
                {group.title}
                <span className="font-mono text-[11px]">{group.items.length + (group.hasCurrent ? 1 : 0)}</span>
              </h3>
              {group.hasCurrent && currentCard}
              {group.items.length > 0 && (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(320px,1fr))] gap-3.5" role="list">
                  {group.items.map((account) => (
                    <div
                      key={account.id}
                      role="listitem"
                      aria-label={accountLabel(account)}
                      className="p-4 rounded-2xl bg-white dark:bg-[#161618] border border-black/[0.06] dark:border-white/[0.08] flex flex-col gap-3"
                    >
                      <div className="flex items-start gap-3">
                        <Avatar text={accountLabel(account)} color={color} size={34} />
                        <div className="flex-1 min-w-0 flex flex-col gap-0.5">
                          <div className="flex items-center gap-1.5 flex-wrap">
                            <span className="font-semibold text-[13px] text-[#1d1d1f] dark:text-white break-all">{accountLabel(account)}</span>
                            {/* The group says whether it is a subscription or a relay. */}
                            {account.plan_type && <Badge tone="blue">{account.plan_type}</Badge>}
                          </div>
                          {account.detail && <span className="text-[11px] font-mono text-[#6e6e73] dark:text-[#a1a1a6] break-all">{account.detail}</span>}
                        </div>
                      </div>
                      {quotaView(account)}
                      {timestamps(account)}
                      <div className="flex items-center gap-2 flex-wrap mt-auto">
                        <Button
                          size="sm"
                          icon={<ArrowRightLeft size={13} />}
                          disabled={disabled || Boolean(state?.current_note)}
                          onClick={() => switchTo(account)}
                        >
                          {a.switchTo}
                        </Button>
                        {secondaryActions(account)}
                      </div>
                      {deleteConfirmation(account)}
                    </div>
                  ))}
                </div>
              )}
            </section>
          ))}

          {accounts.length === 0 && !loading && <div className="text-center text-[#86868b] py-10">{a.empty}</div>}
          <p className="text-[11px] text-[#86868b] leading-relaxed">{hint}</p>
        </div>
      </div>
      {adding && (
        <AccountAddDialog client={client} current={current} onClose={() => setAdding(false)} onAdded={() => void refresh()} onToast={onToast} />
      )}
    </div>
  );
};
