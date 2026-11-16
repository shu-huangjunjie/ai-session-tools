import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  Activity,
  AlertTriangle,
  ArrowDownWideNarrow,
  ArrowRightLeft,
  ArrowUpNarrowWide,
  ChevronRight,
  Download,
  ExternalLink,
  Eye,
  EyeOff,
  Gauge,
  Info,
  LayoutGrid,
  List,
  Pencil,
  Plus,
  RefreshCw,
  Rocket,
  Rows3,
  Save,
  Search,
  ShieldCheck,
  Sparkles,
  Tag,
  Trash2,
  Users,
  X,
} from 'lucide-react';
import { api, errorMessage } from '../api/client';
import {
  AccountClient,
  AccountSortField,
  AccountsState,
  AccountStatusFilter,
  AccountView,
  AccountViewMode,
  QuotaWindow,
} from '../types';
import { formatDateTime } from '../utils/formatters';
import {
  AccountSortConfig,
  loadAutoRefreshMinutes,
  loadPrivacyMode,
  loadSortConfig,
  loadViewMode,
  maskDetailUrl,
  maskEmail,
  maskName,
  saveAutoRefreshMinutes,
  savePrivacyMode,
  saveSortConfig,
  saveViewMode,
} from '../utils/accountPrivacy';
import { groupModelQuotas } from '../utils/quotaGroups';
import { Badge } from './common/Badge';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { PageHeader } from './common/PageHeader';
import { Segmented } from './common/Segmented';
import { Switch } from './common/Switch';
import { AccountAddDialog } from './AccountAddDialog';
import { AccountLaunchModal } from './AccountLaunchModal';
import { RelayForm } from './RelayForm';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';
type TypeFilter = 'all' | 'oauth' | 'provider';

interface AccountsManagerProps {
  onToast: (type: ToastType, message: string) => void;
}

const CLIENTS: AccountClient[] = ['codex', 'claude', 'antigravity'];
const TAB_KEY = 'cv_accounts_tab';
const CLIENT_COLOR: Record<AccountClient, string> = {
  codex: '#2563eb',
  claude: '#d97706',
  antigravity: '#9333ea',
};
const CLIENT_THEME: Record<
  AccountClient,
  {
    from: string;
    to: string;
    activeCard: string;
    iconBg: string;
  }
> = {
  codex: {
    from: '#3b82f6',
    to: '#4f46e5',
    activeCard:
      'border-blue-300/90 dark:border-blue-500/45 ring-1 ring-blue-500/20 shadow-[0_8px_24px_rgba(37,99,235,0.10)] dark:shadow-[0_10px_28px_rgba(37,99,235,0.16)] bg-gradient-to-b from-blue-50/45 via-white to-white dark:from-blue-500/[0.08] dark:via-[#131c2e] dark:to-[#131c2e]',
    iconBg: 'bg-blue-50 dark:bg-blue-500/15 text-blue-600 dark:text-sky-400 border-blue-200/75 dark:border-blue-400/25',
  },
  claude: {
    from: '#f59e0b',
    to: '#ea580c',
    activeCard:
      'border-amber-300/90 dark:border-amber-500/45 ring-1 ring-amber-500/20 shadow-[0_8px_24px_rgba(217,119,6,0.10)] dark:shadow-[0_10px_28px_rgba(217,119,6,0.16)] bg-gradient-to-b from-amber-50/45 via-white to-white dark:from-amber-500/[0.08] dark:via-[#131c2e] dark:to-[#131c2e]',
    iconBg: 'bg-amber-50 dark:bg-amber-500/15 text-amber-600 dark:text-amber-300 border-amber-200/75 dark:border-amber-400/25',
  },
  antigravity: {
    from: '#8b5cf6',
    to: '#c026d3',
    activeCard:
      'border-violet-300/90 dark:border-violet-500/45 ring-1 ring-violet-500/20 shadow-[0_8px_24px_rgba(139,92,246,0.10)] dark:shadow-[0_10px_28px_rgba(139,92,246,0.16)] bg-gradient-to-b from-violet-50/45 via-white to-white dark:from-violet-500/[0.08] dark:via-[#131c2e] dark:to-[#131c2e]',
    iconBg: 'bg-violet-50 dark:bg-violet-500/15 text-violet-600 dark:text-violet-300 border-violet-200/75 dark:border-violet-400/25',
  },
};
const CLIENT_LABEL: Record<AccountClient, string> = {
  codex: 'Codex',
  claude: 'Claude',
  antigravity: 'Antigravity',
};
const FEATURES: Record<AccountClient, { restart: boolean }> = {
  codex: { restart: true },
  claude: { restart: false },
  antigravity: { restart: true },
};
const AUTO_REFRESH_OPTIONS = [0, 5, 10, 15, 30] as const;

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
    // Preference still applies in memory for this session.
  }
}

function rawAccountLabel(account: AccountView): string {
  if (account.name && account.kind === 'provider') return account.name;
  return account.email || account.name || account.account_id || account.id;
}

function remainingPercent(usedPercent: number): number {
  return Math.max(0, Math.min(100, Math.round(100 - usedPercent)));
}

function remainingBarColor(remaining: number): string {
  if (remaining >= 50) {
    return 'bg-gradient-to-r from-emerald-500 to-teal-400 shadow-[0_0_8px_rgba(16,185,129,0.28)]';
  }
  if (remaining >= 20) {
    return 'bg-gradient-to-r from-amber-500 to-orange-400 shadow-[0_0_8px_rgba(245,158,11,0.28)]';
  }
  return 'bg-gradient-to-r from-rose-500 to-red-400 shadow-[0_0_8px_rgba(244,63,94,0.28)]';
}

function remainingTextColor(remaining: number): string {
  if (remaining < 20) return 'font-semibold text-rose-600 dark:text-rose-400';
  if (remaining < 50) return 'font-semibold text-amber-600 dark:text-amber-300';
  return 'font-semibold text-slate-900 dark:text-white';
}

function lowestRemaining(account: AccountView): number | null {
  if (!account.has_quota || !account.quota?.windows?.length) return null;
  let min = 100;
  for (const w of account.quota.windows) {
    const rem = remainingPercent(w.used_percent);
    if (rem < min) min = rem;
  }
  return min;
}

function earliestResetMs(account: AccountView): number | null {
  if (!account.quota?.windows?.length) return null;
  let earliest: number | null = null;
  for (const w of account.quota.windows) {
    if (!w.resets_at) continue;
    const ms = Date.parse(w.resets_at);
    if (!Number.isNaN(ms) && (earliest === null || ms < earliest)) {
      earliest = ms;
    }
  }
  return earliest;
}

function parseIsoMs(iso?: string | null): number | null {
  if (!iso) return null;
  const ms = Date.parse(iso);
  return Number.isNaN(ms) ? null : ms;
}

function compareNullableNumber(
  a: number | null,
  b: number | null,
  direction: 'asc' | 'desc'
): number {
  if (a === null && b === null) return 0;
  if (a === null) return 1;
  if (b === null) return -1;
  return direction === 'asc' ? a - b : b - a;
}

const Avatar: React.FC<{
  text: string;
  color: string;
  size: number;
  active?: boolean;
}> = ({ text, color, size, active }) => (
  <div className="relative flex-shrink-0" style={{ width: size, height: size }}>
    <div
      aria-hidden="true"
      className="w-full h-full rounded-xl grid place-items-center font-semibold shadow-sm"
      style={{
        background: `linear-gradient(135deg, ${color}1f 0%, ${color}36 100%)`,
        border: `1px solid ${color}38`,
        color,
        fontSize: Math.max(11, Math.round(size * 0.4)),
      }}
    >
      {(text.trim().charAt(0) || '?').toUpperCase()}
    </div>
    {active && (
      <span
        aria-hidden="true"
        className="absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full bg-emerald-500 ring-2 ring-white dark:ring-[#131c2e]"
      />
    )}
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
  const label: Record<AccountClient, string> = {
    codex: a.tabCodex,
    claude: a.tabClaude,
    antigravity: a.tabAntigravity,
  };

  const tabs = (
    <Segmented
      value={tab}
      onChange={(client) => {
        setTab(client);
        writeFlag(TAB_KEY, client);
      }}
      items={CLIENTS.map((client) => ({
        key: client,
        label: label[client],
        dot: CLIENT_COLOR[client],
      }))}
    />
  );

  // Remount per client so filter/selection state stays strictly isolated.
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
  const color = CLIENT_COLOR[client];
  const brand = CLIENT_THEME[client];

  const [state, setState] = useState<AccountsState | null>(null);
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [relayForm, setRelayForm] = useState<AccountView | null>(null);
  const [showModels, setShowModels] = useState<Set<string>>(() => new Set());
  const [restart, setRestart] = useState(() => readFlag(restartKey(client), true));

  // Overview / toolbar preferences persisted in localStorage.
  const [privacy, setPrivacy] = useState<boolean>(() => loadPrivacyMode());
  const [viewMode, setViewMode] = useState<AccountViewMode>(() => loadViewMode());
  const [sortConfig, setSortConfig] = useState<AccountSortConfig>(() => loadSortConfig());
  const [autoRefreshMinutes, setAutoRefreshMinutes] = useState<number>(() =>
    loadAutoRefreshMinutes()
  );

  // Filters & discovery UI state.
  const [searchQuery, setSearchQuery] = useState('');
  const [statusFilter, setStatusFilter] = useState<AccountStatusFilter>('all');
  const [typeFilter, setTypeFilter] = useState<TypeFilter>('all');
  const [planFilter, setPlanFilter] = useState<string>('all');
  const [tagFilter, setTagFilter] = useState<string>('all');
  const [cockpitDismissed, setCockpitDismissed] = useState(false);

  // Multi-select & batch actions.
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());
  const [confirmBatchDelete, setConfirmBatchDelete] = useState(false);
  const [batchTagsModalOpen, setBatchTagsModalOpen] = useState(false);
  const [batchAddInput, setBatchAddInput] = useState('');
  const [batchRemoveInput, setBatchRemoveInput] = useState('');

  // Single-account tags & note modal and quick-launch modal.
  const [metaTarget, setMetaTarget] = useState<AccountView | null>(null);
  const [metaTags, setMetaTags] = useState<string[]>([]);
  const [metaTagInput, setMetaTagInput] = useState('');
  const [metaNote, setMetaNote] = useState('');
  const [launchTarget, setLaunchTarget] = useState<AccountView | null>(null);

  const fail = useCallback(
    (err: unknown) => onToast('error', interpolate(a.toastFailed, { error: errorMessage(err) })),
    [onToast, interpolate, a.toastFailed]
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const next = await api.listAccounts(client);
      setState(next);
      const validIds = new Set(next.accounts.map((x) => x.id));
      setSelectedIds((prev) => {
        if (prev.size === 0) return prev;
        const pruned = new Set(Array.from(prev).filter((id) => validIds.has(id)));
        return pruned.size === prev.size ? prev : pruned;
      });
    } catch (err) {
      fail(err);
    } finally {
      setLoading(false);
    }
  }, [client, fail]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const busyRef = useRef(busy);
  busyRef.current = busy;
  const accounts = state?.accounts ?? [];
  const hasAnyQuotaAccountRef = useRef(false);
  hasAnyQuotaAccountRef.current = accounts.some((x) => x.has_quota);

  // Scheduled background auto-refresh when user selects >= 5 min interval.
  useEffect(() => {
    if (autoRefreshMinutes <= 0) return undefined;
    const ms = autoRefreshMinutes * 60_000;
    const timer = window.setInterval(() => {
      if (busyRef.current || !hasAnyQuotaAccountRef.current) return;
      void (async () => {
        try {
          await api.refreshAllAccountQuotas(client);
          await refresh();
        } catch {
          // Silent background refresh; explicit clicks surface errors via toast.
        }
      })();
    }, ms);
    return () => window.clearInterval(timer);
  }, [autoRefreshMinutes, client, refresh]);

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

  const disabled = busy !== null;
  const current = state?.current ?? null;

  const displayLabel = useCallback(
    (account: AccountView) => {
      const raw = rawAccountLabel(account);
      if (!privacy) return raw;
      return raw.includes('@') ? maskEmail(raw) : maskName(raw);
    },
    [privacy]
  );

  const displayDetail = useCallback(
    (detail?: string | null) => {
      if (!detail) return '';
      return privacy ? maskDetailUrl(detail) : detail;
    },
    [privacy]
  );

  const displayCurrentName = useMemo(() => {
    if (!current) return a.noLogin;
    const raw = current.email || current.name || '—';
    if (!privacy) return raw;
    return raw.includes('@') ? maskEmail(raw) : maskName(raw);
  }, [current, privacy, a.noLogin]);

  const summary = useMemo(() => {
    let healthyCount = 0;
    let peakRemaining: number | null = null;
    for (const acc of accounts) {
      const rem = lowestRemaining(acc);
      if (!acc.quota_error && !acc.verification_url && (rem === null || rem >= 20)) {
        healthyCount += 1;
      }
      if (rem !== null && (peakRemaining === null || rem > peakRemaining)) {
        peakRemaining = rem;
      }
    }
    return { healthyCount, peakRemaining };
  }, [accounts]);

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
      onToast(
        'success',
        interpolate(restarted ? a.toastSwitchedRestarted : a.toastSwitched, {
          name: displayLabel(account),
        })
      );
    });

  const handleExport = async (ids?: string[]) => {
    const defaultName = ids?.length ? `${client}-selected-accounts.json` : `${client}-accounts.json`;
    const path = await api.pickSavePath(defaultName, 'JSON', 'json');
    if (!path) return;
    const targetCount = ids?.length ? ids.length : accounts.length;
    await run('export', async () => {
      await api.exportAccountsJson(client, ids, path);
      onToast('success', interpolate(a.toastExported, { count: targetCount }));
    });
  };


  const openMetaModal = (account: AccountView) => {
    setMetaTarget(account);
    setMetaTags([...(account.tags ?? [])]);
    setMetaTagInput('');
    setMetaNote(account.note ?? '');
  };

  const commitMetaTagInput = (raw: string) => {
    const parts = raw
      .split(/[,\n，]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (parts.length === 0) return metaTags;
    const next = [...metaTags];
    for (const part of parts) {
      if (!next.includes(part)) next.push(part);
    }
    setMetaTags(next);
    setMetaTagInput('');
    return next;
  };

  const saveMetaModal = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!metaTarget) return;
    const finalTags = metaTagInput.trim() ? commitMetaTagInput(metaTagInput) : metaTags;
    const targetId = metaTarget.id;
    // Empty string clears the note in Rust (`normalize_note`); `null` would mean leave untouched.
    const nextNote = metaNote.trim();
    await run(`meta:${targetId}`, async () => {
      await api.updateAccountMeta(client, targetId, finalTags, nextNote);
      setMetaTarget(null);
      onToast('success', a.toastMetaSaved);
    });
  };

  const saveBatchTags = async (e: React.FormEvent) => {
    e.preventDefault();
    const ids = Array.from(selectedIds);
    if (ids.length === 0) return;
    const parseList = (text: string) =>
      text
        .split(/[,\s，]+/)
        .map((s) => s.trim())
        .filter(Boolean);
    const addTags = parseList(batchAddInput);
    const removeTags = parseList(batchRemoveInput);
    if (addTags.length === 0 && removeTags.length === 0) {
      setBatchTagsModalOpen(false);
      return;
    }
    await run('batch-tags', async () => {
      const updated = await api.batchUpdateAccountTags(client, ids, addTags, removeTags);
      setBatchTagsModalOpen(false);
      setBatchAddInput('');
      setBatchRemoveInput('');
      onToast('success', interpolate(a.toastBatchTagsUpdated, { count: updated }));
    });
  };

  const allPlans = useMemo(() => {
    const set = new Set<string>();
    for (const acc of accounts) {
      if (acc.plan_type) set.add(acc.plan_type);
    }
    return Array.from(set);
  }, [accounts]);

  const allTags = useMemo(() => {
    const set = new Set<string>();
    for (const acc of accounts) {
      for (const tag of acc.tags ?? []) {
        if (tag) set.add(tag);
      }
    }
    return Array.from(set);
  }, [accounts]);

  const hasActiveFilters =
    searchQuery.trim() !== '' ||
    statusFilter !== 'all' ||
    typeFilter !== 'all' ||
    planFilter !== 'all' ||
    tagFilter !== 'all';

  const clearFilters = () => {
    setSearchQuery('');
    setStatusFilter('all');
    setTypeFilter('all');
    setPlanFilter('all');
    setTagFilter('all');
  };

  const filteredAccounts = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    const filtered = accounts.filter((acc) => {
      if (q) {
        const haystack = [
          rawAccountLabel(acc),
          acc.email,
          acc.name,
          acc.account_id,
          acc.detail,
          acc.plan_type,
          acc.note,
          ...(acc.tags ?? []),
        ]
          .filter(Boolean)
          .join(' ')
          .toLowerCase();
        if (!haystack.includes(q)) return false;
      }

      if (typeFilter === 'oauth' && acc.kind === 'provider') return false;
      if (typeFilter === 'provider' && acc.kind !== 'provider') return false;

      if (planFilter !== 'all' && acc.plan_type !== planFilter) return false;
      if (tagFilter !== 'all' && !(acc.tags ?? []).includes(tagFilter)) return false;

      const lowRem = lowestRemaining(acc);
      if (statusFilter === 'current' && !acc.is_current) return false;
      if (
        statusFilter === 'available' &&
        (Boolean(acc.quota_error) || Boolean(acc.verification_url) || (lowRem !== null && lowRem < 20))
      ) {
        return false;
      }
      if (statusFilter === 'low' && (Boolean(acc.quota_error) || lowRem === null || lowRem >= 20)) {
        return false;
      }
      if (statusFilter === 'error' && !acc.quota_error) return false;
      if (statusFilter === 'verification' && !acc.verification_url) return false;

      return true;
    });

    const sorted = [...filtered].sort((x, y) => {
      const { field, direction } = sortConfig;
      if (field === 'added') {
        if (x.is_current !== y.is_current) return x.is_current ? -1 : 1;
        return compareNullableNumber(parseIsoMs(x.added_at), parseIsoMs(y.added_at), direction);
      }
      if (field === 'last_used') {
        const cmp = compareNullableNumber(
          parseIsoMs(x.last_used_at),
          parseIsoMs(y.last_used_at),
          direction
        );
        if (cmp !== 0) return cmp;
      } else if (field === 'remaining') {
        const cmp = compareNullableNumber(lowestRemaining(x), lowestRemaining(y), direction);
        if (cmp !== 0) return cmp;
      } else if (field === 'reset') {
        const cmp = compareNullableNumber(earliestResetMs(x), earliestResetMs(y), direction);
        if (cmp !== 0) return cmp;
      } else if (field === 'name') {
        const cmp = rawAccountLabel(x).localeCompare(rawAccountLabel(y));
        if (cmp !== 0) return direction === 'asc' ? cmp : -cmp;
      }
      return x.is_current === y.is_current ? 0 : x.is_current ? -1 : 1;
    });

    return sorted;
  }, [accounts, searchQuery, statusFilter, typeFilter, planFilter, tagFilter, sortConfig]);

  const allFilteredSelected =
    filteredAccounts.length > 0 && filteredAccounts.every((acc) => selectedIds.has(acc.id));

  const toggleSelectAll = () => {
    if (allFilteredSelected) {
      setSelectedIds(new Set());
    } else {
      setSelectedIds(new Set(filteredAccounts.map((acc) => acc.id)));
    }
  };

  const toggleSelectOne = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  };

  const toggleModels = (id: string) =>
    setShowModels((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });

  const modelList = (windows: QuotaWindow[]) => (
    <div className="p-3 rounded-xl bg-white/80 dark:bg-[#131c2e]/90 border border-slate-200/75 dark:border-slate-800/80 grid grid-cols-[repeat(auto-fill,minmax(175px,1fr))] gap-x-6 gap-y-3 text-[11px]">
      {groupModelQuotas(windows).map((g) => (
        <div key={g.key} className="flex flex-col gap-1.5 min-w-0">
          <span className="flex items-center gap-1.5 text-[10.5px] font-medium text-slate-500 dark:text-slate-400">
            <span className="w-[6px] h-[6px] rounded-full shrink-0" style={{ background: g.color }} />
            {g.name}
          </span>
          {g.models.map((m) => (
            <div
              key={m.name}
              className="flex items-center gap-2"
              title={
                m.remaining < 100 && m.resetsAt
                  ? interpolate(a.resetsIn, { time: relative(m.resetsAt) })
                  : undefined
              }
            >
              <span className="truncate text-slate-800 dark:text-slate-200">{m.name}</span>
              <span
                className={`ml-auto tabular-nums ${
                  m.remaining <= 30
                    ? 'font-semibold text-amber-600 dark:text-amber-300'
                    : 'text-slate-500 dark:text-slate-400'
                }`}
              >
                {m.remaining}%
              </span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );

  const groupedQuota = (account: AccountView, windows: QuotaWindow[]) => {
    const open = showModels.has(account.id);
    return (
      <div className="flex flex-col gap-2">
        <span className="text-[11px] font-medium text-slate-500 dark:text-slate-400">
          {a.quotaRemaining}
        </span>
        <div className="grid grid-cols-[auto_minmax(60px,1fr)_auto] items-center gap-x-3 gap-y-2">
          {groupModelQuotas(windows).map((g) => (
            <React.Fragment key={g.key}>
              <span
                className="flex items-center gap-1.5 min-w-0 text-[11.5px]"
                title={interpolate(a.modelsInGroup, { count: g.models.length })}
              >
                <span className="w-[7px] h-[7px] rounded-full shrink-0" style={{ background: g.color }} />
                <span className="font-semibold text-slate-800 dark:text-slate-100 truncate">{g.name}</span>
                {g.models.length > 1 && (
                  <span className="text-[10.5px] font-mono text-slate-400 dark:text-slate-500">
                    {g.models.length}
                  </span>
                )}
              </span>
              <div
                className="h-2 rounded-full bg-slate-200/85 dark:bg-slate-800/90 shadow-inner overflow-hidden"
                role="progressbar"
                aria-valuenow={g.remaining}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuetext={interpolate(a.remaining, { percent: g.remaining })}
                aria-label={g.name}
              >
                <div
                  className={`h-full rounded-full transition-all duration-300 ${remainingBarColor(g.remaining)}`}
                  style={{ width: `${g.remaining}%` }}
                />
              </div>
              <span className="text-right text-[11px] tabular-nums whitespace-nowrap">
                <span className={remainingTextColor(g.remaining)}>{g.remaining}%</span>
                {g.remaining < 100 && g.resetsAt && (
                  <span className="ml-1.5 text-slate-400 dark:text-slate-500" title={formatDateTime(g.resetsAt)}>
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
          className="w-fit flex items-center gap-1 text-[11px] font-medium text-blue-600 dark:text-sky-400 hover:underline"
        >
          <ChevronRight size={12} className={`transition-transform ${open ? 'rotate-90' : ''}`} />
          {interpolate(a.allModels, { count: windows.length })}
        </button>
        {open && modelList(windows)}
      </div>
    );
  };

  const quotaView = (account: AccountView) => (
    <>
      {!account.has_quota ? (
        <span className="text-[11px] text-slate-500 dark:text-slate-400">{a.relayNoQuota}</span>
      ) : account.quota && account.quota.windows.length > 0 && client === 'antigravity' ? (
        groupedQuota(account, account.quota.windows)
      ) : account.quota && account.quota.windows.length > 0 ? (
        <div className="flex flex-col gap-2">
          {account.quota.windows.map((w) => {
            const rem = remainingPercent(w.used_percent);
            return (
              <div key={w.kind} className="flex flex-col gap-1.5">
                <div className="flex items-center gap-2 text-[11px]">
                  <span className="font-semibold text-slate-800 dark:text-slate-100">{windowLabel(w)}</span>
                  <span className={`tabular-nums ${remainingTextColor(rem)}`}>
                    {interpolate(a.remaining, { percent: rem })}
                  </span>
                  {w.resets_at && (
                    <span
                      className="ml-auto text-slate-500 dark:text-slate-400 whitespace-nowrap tabular-nums"
                      title={formatDateTime(w.resets_at)}
                    >
                      {interpolate(a.resetsIn, { time: relative(w.resets_at) })}
                    </span>
                  )}
                </div>
                <div
                  className="h-2 rounded-full bg-slate-200/85 dark:bg-slate-800/90 shadow-inner overflow-hidden"
                  role="progressbar"
                  aria-valuenow={rem}
                  aria-valuemin={0}
                  aria-valuemax={100}
                  aria-valuetext={interpolate(a.remaining, { percent: rem })}
                  aria-label={windowLabel(w)}
                >
                  <div
                    className={`h-full rounded-full transition-all duration-300 ${remainingBarColor(rem)}`}
                    style={{ width: `${rem}%` }}
                  />
                </div>
              </div>
            );
          })}
          {account.quota.credits_balance && (
            <span className="text-[11px] text-slate-500 dark:text-slate-400">
              {interpolate(a.credits, {
                balance:
                  account.quota.credits_balance === 'unlimited'
                    ? a.unlimited
                    : account.quota.credits_balance,
              })}
            </span>
          )}
        </div>
      ) : (
        !account.quota_error && <span className="text-[11px] text-slate-500 dark:text-slate-400">{a.noQuota}</span>
      )}

      {account.quota?.credits_balance && client === 'antigravity' && (
        <span className="text-[11px] text-slate-500 dark:text-slate-400">
          {interpolate(a.credits, {
            balance:
              account.quota.credits_balance === 'unlimited'
                ? a.unlimited
                : account.quota.credits_balance,
          })}
        </span>
      )}

      {(account.quota_error || account.verification_url) && (
        <div className="flex flex-wrap items-center gap-2 text-[11px] text-rose-600 dark:text-rose-400">
          {account.quota_error && (
            <p className="flex items-start gap-1.5 flex-1 min-w-0">
              <AlertTriangle size={12} className="mt-0.5 shrink-0" />
              <span className="break-words">{account.quota_error}</span>
            </p>
          )}
          {account.verification_url && (
            <Button
              size="sm"
              variant="accent"
              icon={<ExternalLink size={12} />}
              onClick={() => void api.openExternalUrl(account.verification_url!)}
            >
              {a.verifyAction}
            </Button>
          )}
        </div>
      )}
    </>
  );

  const deleteConfirmation = (account: AccountView) =>
    confirmDelete === account.id && (
      <div className="p-3 rounded-xl border border-rose-300/80 dark:border-rose-500/30 bg-rose-50/90 dark:bg-rose-500/10 flex items-center gap-3 flex-wrap">
        <p className="flex-1 min-w-[180px] leading-relaxed text-rose-700 dark:text-rose-300">
          {a.deleteConfirm}
        </p>
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

  const renderActions = (account: AccountView, compactIcons = false) => {
    const labelText = displayLabel(account);
    return (
      <>
        {!account.is_current && (
          <Button
            size="sm"
            variant="primary"
            icon={<ArrowRightLeft size={13} />}
            disabled={disabled || Boolean(state?.current_note)}
            onClick={() => switchTo(account)}
          >
            {a.switchTo}
          </Button>
        )}
        {account.has_quota && (
          <Button
            size="sm"
            icon={
              <RefreshCw
                size={13}
                className={busy === `quota:${account.id}` ? 'animate-spin' : ''}
              />
            }
            disabled={disabled}
            aria-label={`${a.refresh} ${labelText}`}
            title={a.refresh}
            onClick={() =>
              run(`quota:${account.id}`, async () => {
                await api.refreshAccountQuota(client, account.id);
                onToast('success', a.toastRefreshed);
              })
            }
            className={compactIcons ? 'px-2' : undefined}
          >
            {compactIcons ? null : a.refresh}
          </Button>
        )}
        <Button
          size="sm"
          variant={compactIcons ? 'ghost' : 'accent'}
          icon={<Rocket size={13} />}
          disabled={disabled}
          onClick={() => setLaunchTarget(account)}
          title={a.quickLaunchTooltip}
          aria-label={`${a.quickLaunch} ${labelText}`}
          className={compactIcons ? 'px-2' : undefined}
        >
          {compactIcons ? null : a.quickLaunch}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          icon={<Tag size={13} />}
          disabled={disabled}
          onClick={() => openMetaModal(account)}
          title={a.editMeta}
          aria-label={`${a.editMeta} ${labelText}`}
          className="px-2"
        />
        {account.kind === 'provider' && (
          <Button
            size="sm"
            variant="ghost"
            icon={<Pencil size={13} />}
            disabled={disabled}
            onClick={() => setRelayForm(account)}
            title={a.edit}
            aria-label={`${a.edit} ${labelText}`}
            className="px-2"
          />
        )}
        {!compactIcons && <div className="flex-1" />}
        <Button
          size="sm"
          variant="ghost"
          icon={<Trash2 size={13} />}
          disabled={disabled}
          onClick={() => setConfirmDelete(account.id)}
          title={a.delete}
          aria-label={`${a.delete} ${labelText}`}
          className="px-2 hover:text-rose-600 dark:hover:text-rose-400"
        />
      </>
    );
  };

  const renderIdentityBadges = (account: AccountView) => (
    <div className="flex items-center gap-1.5 flex-wrap">
      <span className="font-semibold text-[13.5px] tracking-tight text-slate-900 dark:text-white break-all">
        {displayLabel(account)}
      </span>
      {account.is_current && (
        <Badge tone="green">
          <span className="cv-live-dot" aria-hidden="true" />
          {a.current}
        </Badge>
      )}
      {account.plan_type && <Badge tone="blue">{account.plan_type}</Badge>}
      {account.kind === 'provider' && <Badge tone="gray">{a.filterKindProvider}</Badge>}
      {account.verification_url && <Badge tone="orange">{a.verifyNeededBadge}</Badge>}
    </div>
  );

  const renderTagsAndNote = (account: AccountView) => {
    const tags = account.tags ?? [];
    if (tags.length === 0 && !account.note) return null;
    return (
      <div className="flex flex-col gap-1">
        {tags.length > 0 && (
          <div className="flex items-center gap-1 flex-wrap">
            {tags.map((tag) => (
              <span
                key={tag}
                className="px-2 py-0.5 rounded-full text-[10.5px] font-medium bg-slate-100 dark:bg-slate-800 text-slate-600 dark:text-slate-300 ring-1 ring-inset ring-slate-200/80 dark:ring-slate-700/75"
              >
                #{tag}
              </span>
            ))}
          </div>
        )}
        {account.note && (
          <p className="text-[11px] text-slate-500 dark:text-slate-400 line-clamp-2 break-words">
            {account.note}
          </p>
        )}
      </div>
    );
  };

  const restartLabel = client === 'codex' ? a.restartCodex : a.restartAntigravity;
  const hint = { codex: a.hintCodex, claude: a.hintClaude, antigravity: a.hintAntigravity }[client];
  const modalInputClass =
    'w-full bg-slate-50/90 dark:bg-[#0e1523] text-slate-800 dark:text-slate-100 px-3 py-2 rounded-xl border border-slate-200/90 dark:border-slate-700/80 shadow-[inset_0_1px_2px_rgba(15,23,42,0.03)] focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15 focus:bg-white dark:focus:bg-[#141d2f] focus:outline-none placeholder-slate-400 dark:placeholder-slate-500 transition-all';

  return (
    <div className="flex-1 flex flex-col min-h-0 text-xs">
      <PageHeader title={a.button} tabs={tabs}>
        <select
          aria-label={a.autoRefreshLabel}
          value={autoRefreshMinutes}
          onChange={(e) => {
            const minutes = Number(e.target.value);
            setAutoRefreshMinutes(minutes);
            saveAutoRefreshMinutes(minutes);
          }}
          className="cv-select font-medium"
        >
          {AUTO_REFRESH_OPTIONS.map((opt) => (
            <option key={opt} value={opt}>
              {opt === 0
                ? `${a.autoRefreshLabel}: ${a.autoRefreshOff}`
                : `${a.autoRefreshLabel}: ${interpolate(a.autoRefreshEvery, { n: opt })}`}
            </option>
          ))}
        </select>

        <Button
          size="sm"
          variant={privacy ? 'accent' : 'secondary'}
          aria-pressed={privacy}
          icon={privacy ? <EyeOff size={13} /> : <Eye size={13} />}
          onClick={() => {
            const next = !privacy;
            setPrivacy(next);
            savePrivacyMode(next);
          }}
        >
          {privacy ? a.privacyOn : a.privacyOff}
        </Button>

        <Button
          size="sm"
          icon={<Download size={13} />}
          disabled={disabled || accounts.length === 0}
          onClick={() => void handleExport()}
        >
          {a.exportJson}
        </Button>

        <Button
          size="sm"
          icon={<RefreshCw size={13} className={busy === 'all' ? 'animate-spin' : ''} />}
          disabled={disabled || !accounts.some((x) => x.has_quota)}
          onClick={() =>
            run('all', async () => {
              const failures = await api.refreshAllAccountQuotas(client);
              if (failures.length > 0) {
                onToast('warning', interpolate(a.toastRefreshFailed, { count: failures.length }));
              } else {
                onToast('success', a.toastRefreshed);
              }
            })
          }
        >
          {busy === 'all' ? a.refreshing : a.refreshAll}
        </Button>

        <Button
          variant="primary"
          size="sm"
          icon={<Plus size={13} />}
          disabled={disabled}
          onClick={() => setAdding(true)}
        >
          {a.addAccount}
        </Button>
      </PageHeader>

      <div className="flex-1 overflow-y-auto px-6 py-5 select-text">
        <div className="max-w-[1280px] flex flex-col gap-4">
          {[state?.current_note, ...(state?.notes ?? [])].filter(Boolean).map((note) => (
            <p
              key={note}
              className="flex items-start gap-2 px-3.5 py-2.5 rounded-xl bg-amber-50/85 dark:bg-amber-500/10 border border-amber-200/80 dark:border-amber-500/25 text-amber-700 dark:text-amber-300"
            >
              <AlertTriangle size={14} className="mt-0.5 shrink-0" />
              <span>{note}</span>
            </p>
          ))}

          {/* Client-themed 4-card KPI summary strip */}
          {(accounts.length > 0 || Boolean(current)) && (
            <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
              <div className="p-3.5 rounded-2xl bg-white/88 dark:bg-[#131c2e]/88 backdrop-blur-md border border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_10px_rgba(15,23,42,0.03)] flex items-center gap-3">
                <div className={`w-9 h-9 rounded-xl border flex items-center justify-center shrink-0 ${brand.iconBg}`}>
                  <Users size={17} />
                </div>
                <div className="min-w-0">
                  <div className="text-[11px] font-medium text-slate-500 dark:text-slate-400 truncate">
                    {a.statTotal}
                  </div>
                  <div className="text-[19px] font-bold tracking-tight tabular-nums text-slate-900 dark:text-white leading-tight">
                    {accounts.length}
                  </div>
                  <div className="text-[10.5px] text-slate-400 dark:text-slate-500 truncate">
                    {a.statTotalSub}
                  </div>
                </div>
              </div>

              <div className="p-3.5 rounded-2xl bg-white/88 dark:bg-[#131c2e]/88 backdrop-blur-md border border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_10px_rgba(15,23,42,0.03)] flex items-center gap-3">
                <div className="w-9 h-9 rounded-xl bg-emerald-50 dark:bg-emerald-500/15 border border-emerald-200/75 dark:border-emerald-400/25 flex items-center justify-center text-emerald-600 dark:text-emerald-300 shrink-0">
                  <ShieldCheck size={17} />
                </div>
                <div className="min-w-0">
                  <div className="text-[11px] font-medium text-slate-500 dark:text-slate-400 truncate">
                    {a.statHealthy}
                  </div>
                  <div className="text-[19px] font-bold tracking-tight tabular-nums text-slate-900 dark:text-white leading-tight">
                    {summary.healthyCount}
                  </div>
                  <div className="text-[10.5px] text-slate-400 dark:text-slate-500 truncate">
                    {a.statHealthySub}
                  </div>
                </div>
              </div>

              <div className="p-3.5 rounded-2xl bg-white/88 dark:bg-[#131c2e]/88 backdrop-blur-md border border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_10px_rgba(15,23,42,0.03)] flex items-center gap-3">
                <div className="w-9 h-9 rounded-xl bg-sky-50 dark:bg-sky-500/15 border border-sky-200/75 dark:border-sky-400/25 flex items-center justify-center text-sky-600 dark:text-sky-300 shrink-0">
                  <Gauge size={17} />
                </div>
                <div className="min-w-0">
                  <div className="text-[11px] font-medium text-slate-500 dark:text-slate-400 truncate">
                    {a.statPeak}
                  </div>
                  <div className="text-[19px] font-bold tracking-tight tabular-nums text-slate-900 dark:text-white leading-tight">
                    {summary.peakRemaining !== null ? `${summary.peakRemaining}%` : '—'}
                  </div>
                  <div className="text-[10.5px] text-slate-400 dark:text-slate-500 truncate">
                    {a.statPeakSub}
                  </div>
                </div>
              </div>

              <div className="p-3.5 rounded-2xl bg-white/88 dark:bg-[#131c2e]/88 backdrop-blur-md border border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_10px_rgba(15,23,42,0.03)] flex items-center gap-3">
                <div className="w-9 h-9 rounded-xl bg-indigo-50 dark:bg-indigo-500/15 border border-indigo-200/75 dark:border-indigo-400/25 flex items-center justify-center text-indigo-600 dark:text-indigo-300 shrink-0">
                  <Activity size={17} />
                </div>
                <div className="min-w-0">
                  <div className="text-[11px] font-medium text-slate-500 dark:text-slate-400 truncate">
                    {a.statActiveState}
                  </div>
                  <div className="text-[16px] font-bold tracking-tight text-slate-900 dark:text-white truncate leading-tight">
                    {current
                      ? current.plan_type
                        ? current.plan_type.toUpperCase()
                        : a.statActiveConnected
                      : a.statActiveNone}
                  </div>
                  <div className="text-[10.5px] text-slate-400 dark:text-slate-500 truncate">
                    {current
                      ? current.saved
                        ? a.statActiveSaved
                        : a.statActiveUnsaved
                      : a.statActiveEmptySub}
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* Cockpit Tools auto-discovery banner */}
          {(state?.cockpit_import_count ?? 0) > 0 && !cockpitDismissed && (
            <div className="px-4 py-3.5 rounded-2xl border border-blue-300/80 dark:border-blue-500/35 bg-gradient-to-r from-blue-50/90 via-indigo-50/70 to-white/90 dark:from-blue-500/[0.12] dark:via-indigo-500/[0.08] dark:to-[#131c2e]/90 shadow-[0_4px_16px_rgba(37,99,235,0.08)] flex flex-wrap items-center gap-3">
              <div className="w-8 h-8 rounded-xl bg-gradient-to-br from-blue-500 to-indigo-600 flex items-center justify-center text-white shadow-sm shrink-0">
                <Sparkles size={15} />
              </div>
              <div className="flex-1 min-w-[220px]">
                <div className="font-semibold text-slate-900 dark:text-white">
                  {interpolate(a.cockpitBannerTitle, {
                    count: state!.cockpit_import_count!,
                    client: CLIENT_LABEL[client],
                  })}
                </div>
                <p className="text-[11px] text-slate-600 dark:text-slate-300 mt-0.5">
                  {a.cockpitBannerDesc}
                </p>
              </div>
              <Button
                size="sm"
                variant="primary"
                icon={<Download size={13} />}
                disabled={disabled}
                onClick={() =>
                  run('cockpit-import', async () => {
                    const result = await api.importCockpitToolsAccounts(client);
                    if (result.imported > 0) {
                      onToast(
                        'success',
                        interpolate(a.toastCockpitImported, { count: result.imported })
                      );
                    } else {
                      onToast('info', a.cockpitNoneFound);
                    }
                  })
                }
              >
                {busy === 'cockpit-import' ? a.cockpitImporting : a.cockpitImportNow}
              </Button>
              <button
                type="button"
                onClick={() => setCockpitDismissed(true)}
                aria-label={t.common.close}
                className="p-1.5 rounded-lg text-slate-400 hover:text-slate-800 dark:hover:text-white hover:bg-black/[0.04] dark:hover:bg-white/[0.06] transition-colors"
              >
                <X size={14} />
              </button>
            </div>
          )}

          {/* Unsaved current login callout (saved active accounts appear directly inside the list) */}
          {current && !current.saved && (
            <section
              aria-label={a.currentLogin}
              className="px-4 py-3.5 rounded-2xl bg-gradient-to-r from-amber-50/95 via-orange-50/60 to-white/95 dark:from-amber-500/[0.12] dark:via-orange-500/[0.06] dark:to-[#131c2e]/95 border border-amber-300/80 dark:border-amber-500/35 shadow-[0_4px_14px_rgba(245,158,11,0.08)] flex flex-wrap items-center gap-3.5"
            >
              <Avatar text={current.email || current.name || '?'} color={color} size={38} active />
              <div className="flex-1 min-w-[200px]">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="font-semibold text-[13.5px] text-slate-900 dark:text-white">
                    {displayCurrentName}
                  </span>
                  {current.plan_type && <Badge tone="blue">{current.plan_type}</Badge>}
                  <Badge tone="orange">{a.notSaved}</Badge>
                </div>
                <p className="text-[11px] text-slate-600 dark:text-slate-400 mt-0.5">
                  {interpolate(a.unsavedCurrentTitle, { client: CLIENT_LABEL[client] })}
                </p>
              </div>
              <Button
                size="sm"
                variant="primary"
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
            </section>
          )}

          {/* Inline provider editor for Codex or Claude */}
          {relayForm && (
            <RelayForm
              client={client === 'codex' ? 'codex' : 'claude'}
              account={relayForm}
              busy={disabled}
              onCancel={() => setRelayForm(null)}
              onSave={(input) =>
                run('relay', async () => {
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
                  setRelayForm(null);
                  onToast('success', a.toastRelaySaved);
                })
              }
            />
          )}

          {/* Unified filter, sort, view mode & restart toolbar */}
          <div className="flex flex-wrap items-center gap-2 p-2.5 rounded-2xl bg-white/88 dark:bg-[#131c2e]/88 backdrop-blur-md border border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_10px_rgba(15,23,42,0.04)]">
            {filteredAccounts.length > 0 && (
              <label className="flex items-center gap-2 px-1.5 cursor-pointer select-none">
                <input
                  type="checkbox"
                  checked={allFilteredSelected}
                  onChange={toggleSelectAll}
                  aria-label={a.selectAll}
                  className="cv-checkbox"
                />
                <span className="text-[11.5px] font-medium text-slate-600 dark:text-slate-300">
                  {a.selectAll}
                </span>
              </label>
            )}

            <div className="relative flex-1 min-w-[200px]">
              <Search
                size={13}
                className="absolute left-2.5 top-1/2 -translate-y-1/2 text-slate-400 dark:text-slate-500 pointer-events-none"
              />
              <input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder={a.searchPlaceholder}
                aria-label={a.searchPlaceholder}
                className="w-full h-[30px] pl-8 pr-7 rounded-lg bg-slate-100/85 dark:bg-[#0c121e] text-slate-800 dark:text-slate-100 border border-slate-200/80 dark:border-slate-700/80 focus:bg-white dark:focus:bg-[#141d2f] focus:border-blue-500 focus:ring-2 focus:ring-blue-500/15 focus:outline-none text-xs placeholder-slate-400 dark:placeholder-slate-500 transition-all"
              />
              {searchQuery && (
                <button
                  type="button"
                  onClick={() => setSearchQuery('')}
                  aria-label={a.clearFilters}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-700 dark:hover:text-white"
                >
                  <X size={12} />
                </button>
              )}
            </div>

            <select
              aria-label="Status"
              value={statusFilter}
              onChange={(e) => setStatusFilter(e.target.value as AccountStatusFilter)}
              className="cv-select"
            >
              <option value="all">{a.filterAll}</option>
              <option value="current">{a.filterCurrent}</option>
              <option value="available">{a.filterAvailable}</option>
              <option value="low">{a.filterLow}</option>
              <option value="error">{a.filterError}</option>
              <option value="verification">{a.filterVerification}</option>
            </select>

            {(client === 'codex' || client === 'claude') && (
              <select
                aria-label="Type"
                value={typeFilter}
                onChange={(e) => setTypeFilter(e.target.value as TypeFilter)}
                className="cv-select"
              >
                <option value="all">{a.filterKindAll}</option>
                <option value="oauth">{a.filterKindOauth}</option>
                <option value="provider">{a.filterKindProvider}</option>
              </select>
            )}

            {allPlans.length > 1 && (
              <select
                aria-label="Plan"
                value={planFilter}
                onChange={(e) => setPlanFilter(e.target.value)}
                className="cv-select"
              >
                <option value="all">{a.filterPlanAll}</option>
                {allPlans.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            )}

            <div className="flex items-center gap-1">
              <select
                aria-label={a.sortBy}
                value={sortConfig.field}
                onChange={(e) => {
                  const next: AccountSortConfig = {
                    field: e.target.value as AccountSortField,
                    direction: sortConfig.direction,
                  };
                  setSortConfig(next);
                  saveSortConfig(next);
                }}
                className="cv-select"
              >
                <option value="added">{a.sortAdded}</option>
                <option value="last_used">{a.sortLastUsed}</option>
                <option value="remaining">{a.sortRemaining}</option>
                <option value="reset">{a.sortReset}</option>
                <option value="name">{a.sortName}</option>
              </select>
              <Button
                size="sm"
                variant="secondary"
                aria-label={sortConfig.direction === 'asc' ? a.sortAsc : a.sortDesc}
                title={sortConfig.direction === 'asc' ? a.sortAsc : a.sortDesc}
                icon={
                  sortConfig.direction === 'asc' ? (
                    <ArrowUpNarrowWide size={13} />
                  ) : (
                    <ArrowDownWideNarrow size={13} />
                  )
                }
                onClick={() => {
                  const next: AccountSortConfig = {
                    field: sortConfig.field,
                    direction: sortConfig.direction === 'asc' ? 'desc' : 'asc',
                  };
                  setSortConfig(next);
                  saveSortConfig(next);
                }}
                className="px-2"
              />
            </div>

            <div
              className="inline-flex items-center h-[30px] rounded-lg border border-slate-200/85 dark:border-slate-700/80 p-0.5 bg-slate-100/90 dark:bg-[#0c121e]"
              role="group"
              aria-label="View mode"
            >
              {(
                [
                  { mode: 'grid', label: a.viewGrid, icon: <LayoutGrid size={13} /> },
                  { mode: 'list', label: a.viewList, icon: <List size={13} /> },
                  { mode: 'compact', label: a.viewCompact, icon: <Rows3 size={13} /> },
                ] as const
              ).map((item) => (
                <button
                  key={item.mode}
                  type="button"
                  aria-pressed={viewMode === item.mode}
                  onClick={() => {
                    setViewMode(item.mode);
                    saveViewMode(item.mode);
                  }}
                  className={`inline-flex items-center gap-1 h-full px-2 rounded-md text-[11px] font-medium transition-all ${
                    viewMode === item.mode
                      ? 'bg-white dark:bg-[#1e2940] text-blue-600 dark:text-sky-300 shadow-[0_1px_2px_rgba(15,23,42,0.08)] ring-1 ring-slate-200/75 dark:ring-slate-700/75'
                      : 'text-slate-500 dark:text-slate-400 hover:text-slate-900 dark:hover:text-white'
                  }`}
                >
                  {item.icon}
                  <span>{item.label}</span>
                </button>
              ))}
            </div>

            {features.restart && (
              <label className="flex items-center gap-2 px-1 cursor-pointer text-slate-600 dark:text-slate-300 ml-auto">
                <Switch
                  checked={restart}
                  label={restartLabel}
                  onChange={(checked) => {
                    setRestart(checked);
                    writeFlag(restartKey(client), String(checked));
                  }}
                />
                <span className="whitespace-nowrap font-medium">{restartLabel}</span>
              </label>
            )}
          </div>

          {/* Tag filter chips */}
          {allTags.length > 0 && (
            <div className="flex items-center gap-1.5 flex-wrap">
              <button
                type="button"
                onClick={() => setTagFilter('all')}
                className={`px-2.5 py-1 rounded-full text-[11px] font-medium border transition-all ${
                  tagFilter === 'all'
                    ? 'bg-gradient-to-b from-blue-500 to-blue-600 text-white border-blue-600 shadow-sm'
                    : 'bg-white/85 dark:bg-[#131c2e] border-slate-200/85 dark:border-slate-700/80 text-slate-600 dark:text-slate-300 hover:border-blue-300'
                }`}
              >
                {a.filterTagAll}
              </button>
              {allTags.map((tag) => (
                <button
                  key={tag}
                  type="button"
                  onClick={() => setTagFilter(tagFilter === tag ? 'all' : tag)}
                  className={`px-2.5 py-1 rounded-full text-[11px] font-medium border transition-all ${
                    tagFilter === tag
                      ? 'bg-gradient-to-b from-blue-500 to-blue-600 text-white border-blue-600 shadow-sm'
                      : 'bg-white/85 dark:bg-[#131c2e] border-slate-200/85 dark:border-slate-700/80 text-slate-600 dark:text-slate-300 hover:text-slate-900 dark:hover:text-white hover:border-blue-300'
                  }`}
                >
                  #{tag}
                </button>
              ))}
            </div>
          )}

          {/* Batch action bar */}
          {selectedIds.size > 0 && (
            <div className="p-3 rounded-2xl border border-blue-300/85 dark:border-blue-500/35 bg-gradient-to-r from-blue-50/90 to-indigo-50/75 dark:from-blue-500/[0.12] dark:to-indigo-500/[0.08] shadow-sm flex flex-wrap items-center gap-2">
              <span className="font-semibold text-blue-700 dark:text-sky-300 mr-1">
                {interpolate(a.selectedCount, { count: selectedIds.size })}
              </span>
              <Button
                size="sm"
                icon={<RefreshCw size={12} />}
                disabled={disabled}
                onClick={() =>
                  run('batch-refresh', async () => {
                    const failures = await api.refreshAllAccountQuotas(
                      client,
                      Array.from(selectedIds)
                    );
                    if (failures.length > 0) {
                      onToast(
                        'warning',
                        interpolate(a.toastRefreshFailed, { count: failures.length })
                      );
                    } else {
                      onToast('success', a.toastRefreshed);
                    }
                  })
                }
              >
                {a.batchRefresh}
              </Button>
              <Button
                size="sm"
                icon={<Tag size={12} />}
                disabled={disabled}
                onClick={() => setBatchTagsModalOpen(true)}
              >
                {a.batchTags}
              </Button>
              <Button
                size="sm"
                icon={<Download size={12} />}
                disabled={disabled}
                onClick={() => void handleExport(Array.from(selectedIds))}
              >
                {a.batchExport}
              </Button>
              <Button
                size="sm"
                variant="danger"
                icon={<Trash2 size={12} />}
                disabled={disabled}
                onClick={() => setConfirmBatchDelete((v) => !v)}
              >
                {a.batchDelete}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => {
                  setSelectedIds(new Set());
                  setConfirmBatchDelete(false);
                }}
              >
                {t.common.cancel}
              </Button>

              {confirmBatchDelete && (
                <div className="w-full pt-2 mt-1 border-t border-rose-300/60 dark:border-rose-500/25 flex items-center gap-2 flex-wrap">
                  <span className="flex-1 text-rose-600 dark:text-rose-400">
                    {interpolate(a.batchDeleteConfirm, { count: selectedIds.size })}
                  </span>
                  <Button
                    size="sm"
                    variant="danger"
                    disabled={disabled}
                    onClick={() =>
                      run('batch-delete', async () => {
                        const deleted = await api.batchDeleteAccounts(
                          client,
                          Array.from(selectedIds)
                        );
                        setSelectedIds(new Set());
                        setConfirmBatchDelete(false);
                        onToast('success', interpolate(a.toastBatchDeleted, { count: deleted }));
                      })
                    }
                  >
                    {a.confirmDelete}
                  </Button>
                </div>
              )}
            </div>
          )}

          {/* Main unified accounts view */}
          {accounts.length === 0 && !loading ? (
            <div className="py-12 px-6 rounded-2xl border border-dashed border-slate-300/80 dark:border-slate-700/75 bg-white/60 dark:bg-[#121a2a]/60 text-center flex flex-col items-center gap-2.5">
              <div className={`w-11 h-11 rounded-2xl border flex items-center justify-center ${brand.iconBg}`}>
                <Users size={20} />
              </div>
              <span className="text-slate-600 dark:text-slate-300 font-medium">{a.empty}</span>
              <Button size="sm" variant="primary" icon={<Plus size={13} />} onClick={() => setAdding(true)}>
                {a.addAccount}
              </Button>
            </div>
          ) : filteredAccounts.length === 0 && !loading ? (
            <div className="py-12 px-6 rounded-2xl border border-dashed border-slate-300/80 dark:border-slate-700/75 bg-white/60 dark:bg-[#121a2a]/60 text-center flex flex-col items-center gap-2.5">
              <span className="text-slate-500 dark:text-slate-400">{a.filteredEmpty}</span>
              {hasActiveFilters && (
                <Button size="sm" variant="secondary" onClick={clearFilters}>
                  {a.clearFilters}
                </Button>
              )}
            </div>
          ) : viewMode === 'compact' ? (
            <div
              role="list"
              className="rounded-2xl bg-white/95 dark:bg-[#131c2e]/95 border border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_10px_rgba(15,23,42,0.03)] divide-y divide-slate-100 dark:divide-slate-800/80 overflow-hidden"
            >
              {filteredAccounts.map((account) => {
                const labelText = displayLabel(account);
                const checked = selectedIds.has(account.id);
                const windows = account.quota?.windows ?? [];
                return (
                  <div
                    key={account.id}
                    role="listitem"
                    aria-label={labelText}
                    className={`relative px-3.5 py-2.5 flex flex-col gap-2 transition-colors ${
                      account.is_current
                        ? 'bg-gradient-to-r from-blue-50/70 via-blue-50/20 to-transparent dark:from-blue-500/[0.08] dark:via-transparent'
                        : 'hover:bg-slate-50/80 dark:hover:bg-white/[0.02]'
                    }`}
                  >
                    {account.is_current && (
                      <div
                        aria-hidden="true"
                        className="absolute left-0 top-0 bottom-0 w-[3px]"
                        style={{ background: `linear-gradient(180deg, ${brand.from}, ${brand.to})` }}
                      />
                    )}
                    <div className="flex items-center gap-2.5 flex-wrap">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() => toggleSelectOne(account.id)}
                        aria-label={`Select ${labelText}`}
                        className="cv-checkbox"
                      />
                      <Avatar text={labelText} color={color} size={28} active={account.is_current} />
                      <div className="min-w-[180px] flex-1">
                        {renderIdentityBadges(account)}
                      </div>

                      {/* Compact inline quota summary */}
                      <div className="flex items-center gap-2 flex-wrap">
                        {!account.has_quota ? (
                          <span className="text-[11px] text-slate-400 dark:text-slate-500">{a.relayNoQuota}</span>
                        ) : windows.length > 0 ? (
                          windows.slice(0, 3).map((w) => {
                            const rem = remainingPercent(w.used_percent);
                            return (
                              <span
                                key={w.kind}
                                className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-slate-100/90 dark:bg-[#0c121e] border border-slate-200/75 dark:border-slate-800/80 text-[11px] tabular-nums"
                                title={w.resets_at ? formatDateTime(w.resets_at) : undefined}
                              >
                                <span className="text-slate-500 dark:text-slate-400">{w.label || w.kind}:</span>
                                <span className={remainingTextColor(rem)}>{rem}%</span>
                              </span>
                            );
                          })
                        ) : (
                          account.quota_error && (
                            <span className="text-[11px] text-rose-600 dark:text-rose-400 truncate max-w-[200px]">
                              {account.quota_error}
                            </span>
                          )
                        )}
                        {account.verification_url && (
                          <Button
                            size="sm"
                            variant="accent"
                            icon={<ExternalLink size={11} />}
                            onClick={() => void api.openExternalUrl(account.verification_url!)}
                          >
                            {a.verifyAction}
                          </Button>
                        )}
                      </div>

                      <div className="flex items-center gap-1 ml-auto">
                        {renderActions(account, true)}
                      </div>
                    </div>
                    {deleteConfirmation(account)}
                  </div>
                );
              })}
            </div>
          ) : viewMode === 'list' ? (
            <div role="list" className="flex flex-col gap-2.5">
              {filteredAccounts.map((account) => {
                const labelText = displayLabel(account);
                const checked = selectedIds.has(account.id);
                return (
                  <div
                    key={account.id}
                    role="listitem"
                    aria-label={labelText}
                    className={`relative overflow-hidden p-4 rounded-2xl border flex flex-col gap-2.5 transition-all duration-150 ${
                      account.is_current
                        ? brand.activeCard
                        : 'bg-white/95 dark:bg-[#131c2e]/95 border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_8px_rgba(15,23,42,0.03)] hover:border-slate-300 dark:hover:border-slate-700 hover:shadow-[0_6px_18px_rgba(15,23,42,0.06)]'
                    }`}
                  >
                    {account.is_current && (
                      <div
                        aria-hidden="true"
                        className="absolute left-0 top-0 bottom-0 w-[3.5px]"
                        style={{ background: `linear-gradient(180deg, ${brand.from}, ${brand.to})` }}
                      />
                    )}
                    <div className="flex flex-wrap items-center gap-3.5">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() => toggleSelectOne(account.id)}
                        aria-label={`Select ${labelText}`}
                        className="cv-checkbox"
                      />
                      <Avatar text={labelText} color={color} size={34} active={account.is_current} />
                      <div className="min-w-[210px] flex-1 flex flex-col gap-1">
                        {renderIdentityBadges(account)}
                        {account.detail && (
                          <span className="text-[11px] font-mono text-slate-500 dark:text-slate-400 break-all">
                            {displayDetail(account.detail)}
                          </span>
                        )}
                        {renderTagsAndNote(account)}
                      </div>

                      <div className="min-w-[240px] flex-1 max-w-[440px] px-3 py-2 rounded-xl bg-slate-50/85 dark:bg-[#0c121e]/80 border border-slate-200/70 dark:border-slate-800/80 flex flex-col gap-1.5">
                        {quotaView(account)}
                      </div>

                      <div className="flex items-center gap-1.5 flex-wrap ml-auto">
                        {renderActions(account, true)}
                      </div>
                    </div>
                    {deleteConfirmation(account)}
                  </div>
                );
              })}
            </div>
          ) : (
            <div
              role="list"
              className="grid grid-cols-[repeat(auto-fill,minmax(340px,1fr))] gap-4"
            >
              {filteredAccounts.map((account) => {
                const labelText = displayLabel(account);
                const checked = selectedIds.has(account.id);
                return (
                  <div
                    key={account.id}
                    role="listitem"
                    aria-label={labelText}
                    className={`relative overflow-hidden p-4 rounded-2xl border flex flex-col gap-3 transition-all duration-150 ${
                      account.is_current
                        ? brand.activeCard
                        : 'bg-white/95 dark:bg-[#131c2e]/95 border-slate-200/85 dark:border-slate-800/85 shadow-[0_2px_10px_rgba(15,23,42,0.03)] dark:shadow-[0_4px_16px_rgba(0,0,0,0.22)] hover:border-slate-300 dark:hover:border-slate-700 hover:shadow-[0_8px_24px_rgba(15,23,42,0.07)]'
                    }`}
                  >
                    {account.is_current && (
                      <div
                        aria-hidden="true"
                        className="absolute left-0 right-0 top-0 h-[3px]"
                        style={{ background: `linear-gradient(90deg, ${brand.from}, ${brand.to})` }}
                      />
                    )}
                    <div className="flex items-start gap-2.5">
                      <input
                        type="checkbox"
                        checked={checked}
                        onChange={() => toggleSelectOne(account.id)}
                        aria-label={`Select ${labelText}`}
                        className="cv-checkbox mt-1"
                      />
                      <Avatar text={labelText} color={color} size={36} active={account.is_current} />
                      <div className="flex-1 min-w-0 flex flex-col gap-1">
                        {renderIdentityBadges(account)}
                        {account.detail && (
                          <span className="text-[11px] font-mono text-slate-500 dark:text-slate-400 break-all">
                            {displayDetail(account.detail)}
                          </span>
                        )}
                      </div>
                    </div>

                    {renderTagsAndNote(account)}

                    <div className="p-3 rounded-xl bg-slate-50/85 dark:bg-[#0c121e]/80 border border-slate-200/70 dark:border-slate-800/80 flex flex-col gap-2">
                      {quotaView(account)}
                    </div>

                    {(account.quota_updated_at || account.last_used_at) && (
                      <div className="flex items-center gap-2 text-[11px] text-slate-400 dark:text-slate-500 flex-wrap tabular-nums">
                        {account.quota_updated_at && (
                          <span>
                            {interpolate(a.updated, { time: formatDateTime(account.quota_updated_at) })}
                          </span>
                        )}
                        {account.last_used_at && (
                          <span>
                            · {interpolate(a.lastUsed, { time: formatDateTime(account.last_used_at) })}
                          </span>
                        )}
                      </div>
                    )}

                    <div className="pt-2.5 mt-auto border-t border-slate-100 dark:border-slate-800/80 flex items-center gap-1.5 flex-wrap">
                      {renderActions(account, false)}
                    </div>
                    {deleteConfirmation(account)}
                  </div>
                );
              })}
            </div>
          )}

          <div className="px-3.5 py-2.5 rounded-xl bg-slate-100/75 dark:bg-[#101725]/80 border border-slate-200/70 dark:border-slate-800/80 flex items-start gap-2 text-[11px] text-slate-500 dark:text-slate-400 leading-relaxed">
            <Info size={13} className="mt-0.5 shrink-0 text-slate-400 dark:text-slate-500" />
            <p>{hint}</p>
          </div>
        </div>
      </div>

      {adding && (
        <AccountAddDialog
          client={client}
          current={current}
          cockpitImportCount={state?.cockpit_import_count ?? 0}
          ccSwitchAvailable={Boolean(state?.cc_switch_available)}
          onClose={() => setAdding(false)}
          onAdded={() => void refresh()}
          onToast={onToast}
        />
      )}

      {launchTarget && (
        <AccountLaunchModal
          isOpen
          client={client}
          account={launchTarget}
          accountLabel={displayLabel(launchTarget)}
          onClose={() => setLaunchTarget(null)}
          onSwitched={() => void refresh()}
          onToast={onToast}
        />
      )}

      {/* Single account tags & note modal */}
      {metaTarget && (
        <Modal
          isOpen
          onClose={() => setMetaTarget(null)}
          title={a.metaModalTitle}
          subtitle={displayLabel(metaTarget)}
          widthClass="max-w-md"
        >
          <form className="flex flex-col gap-3 text-xs" onSubmit={saveMetaModal}>
            <div className="flex flex-col gap-1.5">
              <label className="font-semibold text-slate-600 dark:text-slate-300" htmlFor="account-meta-tag-input">
                {a.metaTagsLabel}
              </label>
              {metaTags.length > 0 && (
                <div className="flex items-center gap-1.5 flex-wrap">
                  {metaTags.map((tag) => (
                    <button
                      key={tag}
                      type="button"
                      onClick={() => setMetaTags(metaTags.filter((x) => x !== tag))}
                      aria-label={`Remove ${tag}`}
                      className="inline-flex items-center gap-1 px-2.5 py-0.5 rounded-full bg-blue-50 dark:bg-blue-500/15 text-blue-600 dark:text-sky-300 ring-1 ring-inset ring-blue-200/80 dark:ring-blue-400/25 text-[11px]"
                    >
                      <span>#{tag}</span>
                      <X size={11} />
                    </button>
                  ))}
                </div>
              )}
              <input
                id="account-meta-tag-input"
                value={metaTagInput}
                onChange={(e) => {
                  const val = e.target.value;
                  if (val.endsWith(',') || val.endsWith('，')) {
                    commitMetaTagInput(val);
                  } else {
                    setMetaTagInput(val);
                  }
                }}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && metaTagInput.trim()) {
                    e.preventDefault();
                    commitMetaTagInput(metaTagInput);
                  }
                }}
                placeholder={a.metaTagsPlaceholder}
                className={modalInputClass}
              />
              <span className="text-[11px] text-slate-500 dark:text-slate-400">{a.metaTagsHint}</span>
            </div>

            <label className="flex flex-col gap-1">
              <span className="font-semibold text-slate-600 dark:text-slate-300">{a.metaNoteLabel}</span>
              <textarea
                aria-label={a.metaNoteLabel}
                value={metaNote}
                onChange={(e) => setMetaNote(e.target.value)}
                placeholder={a.metaNotePlaceholder}
                rows={3}
                className={`${modalInputClass} resize-none`}
              />
            </label>

            <div className="flex items-center justify-end gap-2 pt-1">
              <Button size="sm" variant="ghost" onClick={() => setMetaTarget(null)}>
                {t.common.cancel}
              </Button>
              <Button size="sm" variant="primary" type="submit" disabled={disabled}>
                {a.save}
              </Button>
            </div>
          </form>
        </Modal>
      )}

      {/* Batch tags modal */}
      {batchTagsModalOpen && (
        <Modal
          isOpen
          onClose={() => setBatchTagsModalOpen(false)}
          title={interpolate(a.batchTagTitle, { count: selectedIds.size })}
          widthClass="max-w-md"
        >
          <form className="flex flex-col gap-3 text-xs" onSubmit={saveBatchTags}>
            <label className="flex flex-col gap-1">
              <span className="font-semibold text-slate-600 dark:text-slate-300">{a.batchAddTags}</span>
              <input
                aria-label={a.batchAddTags}
                value={batchAddInput}
                onChange={(e) => setBatchAddInput(e.target.value)}
                placeholder={a.batchTagsPlaceholder}
                className={modalInputClass}
              />
            </label>

            <label className="flex flex-col gap-1">
              <span className="font-semibold text-slate-600 dark:text-slate-300">{a.batchRemoveTags}</span>
              <input
                aria-label={a.batchRemoveTags}
                value={batchRemoveInput}
                onChange={(e) => setBatchRemoveInput(e.target.value)}
                placeholder={a.batchTagsPlaceholder}
                className={modalInputClass}
              />
            </label>

            <div className="flex items-center justify-end gap-2 pt-1">
              <Button size="sm" variant="ghost" onClick={() => setBatchTagsModalOpen(false)}>
                {t.common.cancel}
              </Button>
              <Button size="sm" variant="primary" type="submit" disabled={disabled}>
                {a.save}
              </Button>
            </div>
          </form>
        </Modal>
      )}
    </div>
  );
};
