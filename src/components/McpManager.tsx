import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Eye, EyeOff, FileCode2, Lock, Pencil, Plus, RefreshCw, Search, Trash2 } from 'lucide-react';
import { api, errorMessage } from '../api/client';
import { McpClient, McpLocation, McpServerEntry, McpSpec } from '../types';
import { Badge } from './common/Badge';
import { Button } from './common/Button';
import { PageHeader } from './common/PageHeader';
import { Segmented } from './common/Segmented';
import { Switch } from './common/Switch';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';
type Filter = 'all' | McpClient | 'off';

interface McpManagerProps {
  onToast: (type: ToastType, message: string) => void;
}

const CLIENTS: McpClient[] = ['claude', 'codex', 'antigravity'];
const CLIENT_LABEL: Record<McpClient, string> = { claude: 'Claude', codex: 'Codex', antigravity: 'Antigravity' };
const NAME_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;
/** Fields the form edits; everything else in a spec is kept as is. */
const CORE_FIELDS = ['type', 'command', 'args', 'env', 'url', 'headers'];

const asStrings = (value: unknown): string[] => (Array.isArray(value) ? value.map(String) : []);
const asStringMap = (value: unknown): Record<string, string> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, String(v)]))
    : {};
const extraFields = (spec: McpSpec) => Object.fromEntries(Object.entries(spec).filter(([k]) => !CORE_FIELDS.includes(k)));
/** The part of a spec every client shares, for telling whether two clients run the same server. */
const coreOf = (spec: McpSpec) => JSON.stringify(CORE_FIELDS.map((k) => spec[k] ?? null));

/** One line per server: the command line for stdio, the URL otherwise. */
export function serverSummary(entry: McpServerEntry): string {
  if (entry.transport === 'stdio') return [String(entry.spec.command ?? ''), ...asStrings(entry.spec.args)].join(' ');
  return String(entry.spec.url ?? '');
}

/** One server name across clients, like a skill row: each client's entry, if it has one. */
interface McpGroup {
  key: string;
  name: string;
  entries: Partial<Record<McpClient, McpServerEntry>>;
  /** The entry the details show: the first client that has it. */
  primary: McpServerEntry;
}

const inputClass =
  'w-full bg-black/[0.04] dark:bg-white/[0.06] px-3 py-2 rounded-xl border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]';

/** One list of every server with a switch per client, like the skills page; nothing to copy by hand. */
export const McpManager: React.FC<McpManagerProps> = ({ onToast }) => {
  const { t, interpolate } = useI18n();
  const m = t.mcp;
  const [locations, setLocations] = useState<McpLocation[]>([]);
  const [filter, setFilter] = useState<Filter>('all');
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<{ group?: McpGroup } | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [reveal, setReveal] = useState(false);
  const [showRaw, setShowRaw] = useState(false);

  const fail = useCallback((err: unknown) => onToast('error', interpolate(m.toastFailed, { error: errorMessage(err) })), [onToast, interpolate, m.toastFailed]);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setLocations(await api.listMcpServers());
    } catch (err) {
      fail(err);
    } finally {
      setLoading(false);
    }
  }, [fail]);

  useEffect(() => {
    void refresh();
    // Reload only when the page opens; actions refresh explicitly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    setConfirmDelete(false);
    setReveal(false);
    setShowRaw(false);
  }, [selectedKey]);

  const location = (client: McpClient) => locations.find((l) => l.client === client);
  const writable = (server: McpServerEntry) => !server.read_only && !location(server.client)?.error;

  const groups = useMemo(() => {
    const byKey = new Map<string, McpGroup>();
    for (const l of locations) {
      for (const server of l.servers) {
        const key = server.name.toLowerCase();
        const group = byKey.get(key) ?? { key, name: server.name, entries: {}, primary: server };
        group.entries[server.client] ??= server;
        byKey.set(key, group);
      }
    }
    for (const group of byKey.values()) group.primary = CLIENTS.map((c) => group.entries[c]).find(Boolean)!;
    return [...byKey.values()].sort((a, b) => a.key.localeCompare(b.key));
  }, [locations]);

  const isOn = (g: McpGroup, client: McpClient) => Boolean(g.entries[client]?.enabled);
  const isOff = (g: McpGroup) => !CLIENTS.some((c) => isOn(g, c));
  const counts = {
    all: groups.length,
    off: groups.filter(isOff).length,
    ...Object.fromEntries(CLIENTS.map((c) => [c, groups.filter((g) => isOn(g, c)).length])),
  } as Record<Filter, number>;
  const visibleGroups = useMemo(() => {
    const q = query.trim().toLowerCase();
    return groups.filter(
      (g) =>
        (filter === 'all' || (filter === 'off' ? isOff(g) : isOn(g, filter))) &&
        (!q || Object.values(g.entries).some((s) => [s!.name, serverSummary(s!)].some((v) => v.toLowerCase().includes(q))))
    );
    // isOn and isOff only read the group.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groups, filter, query]);
  const selected = groups.find((g) => g.key === selectedKey) ?? null;

  /** Where switching a client on takes the server from: an enabled, writable copy first. */
  const sourceFor = (g: McpGroup, client: McpClient) => {
    const candidates = CLIENTS.filter((c) => c !== client)
      .map((c) => g.entries[c])
      .filter((s): s is McpServerEntry => Boolean(s) && writable(s!) && !(client === 'antigravity' && s!.transport === 'sse'));
    return candidates.find((s) => s.enabled) ?? candidates[0];
  };
  const canToggle = (g: McpGroup, client: McpClient) => {
    if (location(client)?.error) return false;
    const entry = g.entries[client];
    return entry ? writable(entry) : Boolean(sourceFor(g, client));
  };
  /** The writable entries that editing and deleting act on. */
  const writableEntries = (g: McpGroup) =>
    CLIENTS.map((c) => g.entries[c]).filter((s): s is McpServerEntry => Boolean(s) && writable(s!));

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  };

  /** Turns a server on in a client: enables its entry, or writes it in from another client. Returns dropped fields. */
  const switchOn = async (g: McpGroup, client: McpClient): Promise<string[]> => {
    const entry = g.entries[client];
    if (entry) {
      await api.setMcpEnabled(client, entry.name, true);
      return [];
    }
    const source = sourceFor(g, client);
    if (!source) return [];
    return (await api.copyMcpServer(source.client, source.name, client, false)).dropped_fields;
  };

  /** Off disables the client's entry; on enables it, or writes the server into a client that lacks it. */
  const toggleClient = (g: McpGroup, client: McpClient) =>
    run(async () => {
      const entry = g.entries[client];
      const vars = { name: g.name, client: CLIENT_LABEL[client] };
      if (entry?.enabled) {
        await api.setMcpEnabled(client, entry.name, false);
        onToast('success', interpolate(m.toastDisabledIn, vars));
      } else {
        const dropped = await switchOn(g, client);
        if (dropped.length > 0) {
          onToast('warning', interpolate(m.toastEnabledDropped, { ...vars, fields: dropped.join(', ') }));
        } else {
          onToast('success', interpolate(m.toastEnabledIn, vars));
        }
      }
      await refresh();
    });

  /** The listed servers a client could run but does not; enable-all acts on these. */
  const offIn = (client: McpClient) => visibleGroups.filter((g) => !isOn(g, client) && canToggle(g, client));

  const enableAll = (client: McpClient) =>
    run(async () => {
      const failed: string[] = [];
      const trimmed: string[] = [];
      let done = 0;
      for (const g of offIn(client)) {
        try {
          if ((await switchOn(g, client)).length > 0) trimmed.push(g.name);
          done += 1;
        } catch {
          failed.push(g.name);
        }
      }
      const vars = { client: CLIENT_LABEL[client] };
      if (done > 0) onToast('success', interpolate(m.toastEnabledAll, { ...vars, count: done }));
      if (trimmed.length > 0) onToast('warning', interpolate(m.toastDroppedSome, { ...vars, names: trimmed.join('、') }));
      if (failed.length > 0) onToast('warning', interpolate(m.toastEnableAllFailed, { count: failed.length, names: failed.join('、') }));
      await refresh();
    });

  /** Saves to every chosen client; an edit updates each client that has the server, keeping its switch. */
  const save = (clients: McpClient[], name: string, spec: McpSpec) =>
    run(async () => {
      const group = editing?.group;
      for (const client of clients) {
        await api.upsertMcpServer(client, group?.entries[client]?.name ?? null, name, spec);
      }
      onToast('success', interpolate(group ? m.toastSaved : m.toastCreated, { name }));
      setEditing(null);
      await refresh();
      setSelectedKey(name.toLowerCase());
    });

  const deleteGroup = (g: McpGroup) =>
    run(async () => {
      for (const server of writableEntries(g)) await api.deleteMcpServer(server.client, server.name);
      onToast('success', interpolate(m.toastDeleted, { name: g.name }));
      setSelectedKey(null);
      await refresh();
    });

  const stateLabel = (g: McpGroup, client: McpClient) => {
    const entry = g.entries[client];
    if (!entry) return m.stateNone;
    return entry.enabled ? m.enabledState : m.disabled;
  };
  const disableTooltip: Record<McpClient, string> = {
    claude: m.disableTooltipClaude,
    codex: m.disableTooltipCodex,
    antigravity: m.disableTooltipAntigravity,
  };
  const switchTitle = (g: McpGroup, client: McpClient) => {
    const entry = g.entries[client];
    if (entry) return entry.enabled ? disableTooltip[client] : m.enableTooltip;
    if (client === 'antigravity' && Object.values(g.entries).every((s) => s!.transport === 'sse')) return m.sseUnsupported;
    return m.addTooltip;
  };

  return (
    <div className="flex-1 flex flex-col min-h-0 text-xs">
      <PageHeader
        title={m.button}
        tabs={
          <Segmented
            value={filter}
            onChange={setFilter}
            items={[
              { key: 'all' as Filter, label: m.overview, count: counts.all },
              ...CLIENTS.map((client) => ({ key: client as Filter, label: CLIENT_LABEL[client], count: counts[client] })),
              { key: 'off' as Filter, label: m.filterOff, count: counts.off },
            ]}
          />
        }
      >
        <div className="relative">
          <Search size={12} className="absolute left-3 top-1/2 -translate-y-1/2 text-slate-400" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={m.search}
            className="w-56 bg-slate-100/90 dark:bg-[#101725] text-slate-800 dark:text-slate-100 pl-8 pr-3 py-1.5 rounded-lg border border-slate-200/85 dark:border-slate-700/75 focus:border-blue-500 focus:bg-white dark:focus:bg-[#162033] focus:outline-none placeholder-slate-400 transition-all"
          />
        </div>
        <Button
          variant="primary"
          icon={<Plus size={13} />}
          onClick={() => {
            setEditing({});
            setSelectedKey(null);
          }}
        >
          {m.newServer}
        </Button>
        <Button
          variant="ghost"
          onClick={() => refresh()}
          disabled={loading}
          title={m.refresh}
          aria-label={m.refresh}
          className="px-2"
          icon={<RefreshCw size={14} className={loading ? 'animate-spin' : ''} />}
        />
      </PageHeader>

      <div className="flex-1 flex min-h-0">
        <div className="flex-1 min-w-0 flex flex-col bg-white/75 dark:bg-[#121a2a]/75 backdrop-blur-md border-r border-slate-200/75 dark:border-slate-800/80">
          <div className="flex-1 overflow-y-auto">
            <table className="w-full border-separate border-spacing-0" role="grid">
              <thead>
                <tr className="text-[11px] text-slate-500 dark:text-slate-400">
                  <th className="sticky top-0 z-10 bg-slate-50/88 dark:bg-[#101725]/88 backdrop-blur-md text-left font-semibold px-4 py-2.5 border-b border-slate-200/80 dark:border-slate-800/85">{m.name}</th>
                  {CLIENTS.map((c) => (
                    <th key={c} className="sticky top-0 z-10 bg-slate-50/88 dark:bg-[#101725]/88 backdrop-blur-md text-left font-semibold px-4 py-2 w-[120px] border-b border-slate-200/80 dark:border-slate-800/85">
                      <div className="flex flex-col items-start gap-0.5">
                        <span>{CLIENT_LABEL[c]}</span>
                        <button
                          type="button"
                          onClick={() => enableAll(c)}
                          disabled={busy || offIn(c).length === 0}
                          title={interpolate(m.enableAllTooltip, { client: CLIENT_LABEL[c], count: offIn(c).length })}
                          aria-label={interpolate(m.enableAllIn, { client: CLIENT_LABEL[c] })}
                          className="text-[11px] font-normal text-blue-600 dark:text-sky-400 hover:underline disabled:text-slate-400 disabled:opacity-50 disabled:no-underline disabled:cursor-default"
                        >
                          {m.enableAll}
                        </button>
                      </div>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {visibleGroups.length === 0 && (
                  <tr>
                    <td colSpan={CLIENTS.length + 1} className="text-center text-slate-400 py-10">{query ? m.noMatch : m.empty}</td>
                  </tr>
                )}
                {visibleGroups.map((g) => (
                  <tr
                    key={g.key}
                    className={selected?.key === g.key ? 'bg-blue-50/80 dark:bg-blue-500/12' : 'hover:bg-slate-50/80 dark:hover:bg-white/[0.03]'}
                  >
                    <td className="px-4 py-2.5 border-b border-slate-100 dark:border-slate-800/80 max-w-0">
                      <button
                        type="button"
                        onClick={() => {
                          setSelectedKey(g.key);
                          setEditing(null);
                        }}
                        className="text-left w-full min-w-0"
                      >
                        <div className={`font-semibold text-[13px] text-slate-900 dark:text-white ${isOff(g) ? 'opacity-60' : ''}`}>{g.name}</div>
                        <div className="mt-0.5 font-mono text-[11px] text-slate-500 dark:text-slate-400 truncate" title={serverSummary(g.primary)}>
                          {serverSummary(g.primary)}
                        </div>
                      </button>
                    </td>
                    {CLIENTS.map((client) => (
                      <td key={client} className="px-4 py-2.5 border-b border-slate-100 dark:border-slate-800/80">
                        <div className="flex items-center gap-1.5">
                          <Switch
                            checked={isOn(g, client)}
                            disabled={busy || !canToggle(g, client)}
                            label={`${CLIENT_LABEL[client]} ${g.name}`}
                            title={switchTitle(g, client)}
                            onChange={() => toggleClient(g, client)}
                          />
                          {g.entries[client]?.read_only && <Lock size={11} className="text-purple-500" aria-label={m.readOnly} />}
                        </div>
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="px-4 py-2.5 border-t border-slate-200/85 dark:border-slate-800/85 bg-slate-50/60 dark:bg-[#111827]/60 text-[11px] text-slate-500 dark:text-slate-400 flex flex-col gap-1">
            {locations.map((l) => (
              <div key={l.client} className="flex items-center gap-1.5 min-w-0">
                <span className="font-semibold shrink-0">{CLIENT_LABEL[l.client]}</span>
                <span className="font-mono truncate" title={l.config_path}>{l.config_path}</span>
                {l.error && <Badge tone="red" title={l.error}><AlertTriangle size={10} />{interpolate(m.configError, { error: l.error })}</Badge>}
              </div>
            ))}
            <span>{m.unifiedHint}</span>
          </div>
        </div>

        {/* Detail or form */}
        <div className="w-[420px] flex-shrink-0 overflow-y-auto p-6 bg-slate-50/55 dark:bg-[#0d1320]/65 backdrop-blur-sm select-text">
          {editing ? (
            <McpServerForm
              key={editing.group ? editing.group.key : 'new'}
              entry={editing.group?.primary}
              defaultClients={editing.group ? writableEntries(editing.group).map((s) => s.client) : ['claude']}
              lockedClients={locations.filter((l) => l.error).map((l) => l.client)}
              busy={busy}
              onCancel={() => setEditing(null)}
              onSubmit={save}
            />
          ) : !selected ? (
            <div className="h-full flex items-center justify-center text-[#86868b]">{m.selectHint}</div>
          ) : (
            (() => {
              const entry = selected.primary;
              const present = CLIENTS.filter((c) => selected.entries[c]);
              const differs = present.some((c) => coreOf(selected.entries[c]!.spec) !== coreOf(entry.spec));
              const editable = writableEntries(selected);
              return (
                <div className="flex flex-col gap-3">
                  <div className="flex items-center gap-1.5 flex-wrap">
                    <h2 className="text-lg font-semibold tracking-tight text-[#1d1d1f] dark:text-white mr-1">{selected.name}</h2>
                    <Badge tone="blue">{entry.transport}</Badge>
                    {present.some((c) => selected.entries[c]!.read_only) && <Badge tone="purple"><Lock size={9} />{m.readOnly}</Badge>}
                    {present.some((c) => selected.entries[c]!.cc_switch != null) && <Badge tone="orange" title={m.ccSwitchHint}>{m.ccSwitch}</Badge>}
                  </div>

                  <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-[11px]">
                    {CLIENTS.map((client) => (
                      <React.Fragment key={client}>
                        <dt className="text-[#86868b]">{CLIENT_LABEL[client]}</dt>
                        <dd className={isOn(selected, client) ? 'text-[#248a3d] dark:text-[#30d158] font-medium' : 'text-[#86868b]'}>
                          {stateLabel(selected, client)}
                        </dd>
                      </React.Fragment>
                    ))}
                    <dt className="text-[#86868b] pt-2">{m.transport}</dt>
                    <dd className="font-mono pt-2">{entry.transport}</dd>
                    {entry.transport === 'stdio' ? (
                      <>
                        <dt className="text-[#86868b]">{m.command}</dt>
                        <dd className="font-mono break-all">{String(entry.spec.command ?? '')}</dd>
                        {asStrings(entry.spec.args).length > 0 && (
                          <>
                            <dt className="text-[#86868b]">{m.args}</dt>
                            <dd className="font-mono break-all">{asStrings(entry.spec.args).map((a, i) => <div key={i}>{a}</div>)}</dd>
                          </>
                        )}
                      </>
                    ) : (
                      <>
                        <dt className="text-[#86868b]">{m.url}</dt>
                        <dd className="font-mono break-all">{String(entry.spec.url ?? '')}</dd>
                      </>
                    )}
                    {(['env', 'headers'] as const).map((field) => {
                      const values = Object.entries(asStringMap(entry.spec[field]));
                      if (values.length === 0) return null;
                      return (
                        <React.Fragment key={field}>
                          <dt className="text-[#86868b]">{field === 'env' ? m.env : m.headers}</dt>
                          <dd className="font-mono break-all">
                            {values.map(([k, v]) => (
                              <div key={k}>
                                {k}
                                {field === 'env' ? '=' : ': '}
                                {reveal ? v : v ? '••••••' : ''}
                              </div>
                            ))}
                          </dd>
                        </React.Fragment>
                      );
                    })}
                    {Object.keys(extraFields(entry.spec)).length > 0 && (
                      <>
                        <dt className="text-[#86868b]">{m.otherFields}</dt>
                        <dd className="font-mono break-all whitespace-pre-wrap">
                          {reveal ? JSON.stringify(extraFields(entry.spec), null, 1) : Object.keys(extraFields(entry.spec)).join(', ')}
                        </dd>
                      </>
                    )}
                  </dl>

                  {differs && (
                    <p className="flex items-start gap-1.5 text-[11px] text-[#c93400] dark:text-[#ff9f0a] leading-relaxed">
                      <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                      {interpolate(m.differs, { client: CLIENT_LABEL[entry.client] })}
                    </p>
                  )}

                  <div className="flex items-center gap-2 flex-wrap">
                    <Button size="sm" icon={<Pencil size={13} />} disabled={busy || editable.length === 0} onClick={() => setEditing({ group: selected })}>
                      {m.edit}
                    </Button>
                    <Button size="sm" variant="danger" icon={<Trash2 size={13} />} disabled={busy || editable.length === 0} onClick={() => setConfirmDelete(true)}>
                      {m.delete}
                    </Button>
                    <div className="flex-1" />
                    <Button size="sm" variant="ghost" icon={reveal ? <EyeOff size={13} /> : <Eye size={13} />} onClick={() => setReveal((v) => !v)}>
                      {reveal ? m.hideValues : m.showValues}
                    </Button>
                    <Button size="sm" variant="ghost" icon={<FileCode2 size={13} />} onClick={() => setShowRaw((v) => !v)}>
                      {m.raw}
                    </Button>
                  </div>

                  {present.some((c) => selected.entries[c]!.cc_switch != null) && (
                    <p className="flex items-start gap-1.5 text-[11px] text-[#c93400] dark:text-[#ff9f0a] leading-relaxed">
                      <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                      {m.ccSwitchHint}
                    </p>
                  )}

                  {confirmDelete && (
                    <div className="p-3 rounded-xl border border-[#ff453a]/25 bg-[#ff453a]/[0.06] flex items-center gap-3">
                      <p className="flex-1 leading-relaxed">
                        {interpolate(m.deleteFrom, { clients: editable.map((s) => CLIENT_LABEL[s.client]).join('、') })}
                      </p>
                      <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(false)}>
                        {t.common.cancel}
                      </Button>
                      <Button size="sm" variant="danger" disabled={busy} onClick={() => deleteGroup(selected)}>
                        {m.confirmDelete}
                      </Button>
                    </div>
                  )}

                  {showRaw &&
                    present.map((client) => (
                      <div key={client}>
                        <div className="text-[11px] font-semibold text-[#86868b] mb-1">
                          {CLIENT_LABEL[client]} · <span className="font-mono font-normal">{location(client)?.config_path}</span>
                        </div>
                        <pre className="whitespace-pre-wrap break-all font-mono text-[11px] leading-relaxed p-3 rounded-xl bg-black/[0.04] dark:bg-black/40 border border-black/[0.06] dark:border-white/[0.06] max-h-[40vh] overflow-y-auto">
                          {selected.entries[client]!.raw}
                        </pre>
                      </div>
                    ))}
                </div>
              );
            })()
          )}
        </div>
      </div>
    </div>
  );
};

interface McpServerFormProps {
  /** The server being edited; absent when creating. */
  entry?: McpServerEntry;
  /** Where to save: the clients that have the server when editing, the preselected ones when creating. */
  defaultClients: McpClient[];
  /** Clients whose config file cannot be written. */
  lockedClients: McpClient[];
  busy: boolean;
  onCancel: () => void;
  onSubmit: (clients: McpClient[], name: string, spec: McpSpec) => void;
}

const linesOf = (text: string) => text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);

function parsePairs(text: string, separator: string): Record<string, string> {
  const result: Record<string, string> = {};
  for (const line of linesOf(text)) {
    const at = line.indexOf(separator);
    if (at > 0) result[line.slice(0, at).trim()] = line.slice(at + separator.length).trim();
  }
  return result;
}

const McpServerForm: React.FC<McpServerFormProps> = ({ entry, defaultClients, lockedClients, busy, onCancel, onSubmit }) => {
  const { t, interpolate } = useI18n();
  const m = t.mcp;
  const initial = entry?.spec ?? {};
  const [name, setName] = useState(entry?.name ?? '');
  // Editing saves to every client that has the server; creating to the clients ticked below.
  const [clients, setClients] = useState<McpClient[]>(defaultClients.filter((c) => !lockedClients.includes(c)));
  const [mode, setMode] = useState<'form' | 'json'>('form');
  const [transport, setTransport] = useState(entry?.transport ?? 'stdio');
  const [command, setCommand] = useState(String(initial.command ?? ''));
  const [args, setArgs] = useState(asStrings(initial.args).join('\n'));
  const [env, setEnv] = useState(Object.entries(asStringMap(initial.env)).map(([k, v]) => `${k}=${v}`).join('\n'));
  const [url, setUrl] = useState(String(initial.url ?? ''));
  const [headers, setHeaders] = useState(Object.entries(asStringMap(initial.headers)).map(([k, v]) => `${k}: ${v}`).join('\n'));
  const [json, setJson] = useState('');
  const [error, setError] = useState<string | null>(null);

  // Build the spec from the form fields, keeping fields the form does not show.
  const specFromForm = (): McpSpec => {
    const spec: McpSpec = { type: transport, ...extraFields(initial) };
    if (transport === 'stdio') {
      spec.command = command.trim();
      if (linesOf(args).length > 0 || 'args' in initial) spec.args = linesOf(args);
      if (linesOf(env).length > 0 || 'env' in initial) spec.env = parsePairs(env, '=');
    } else {
      spec.url = url.trim();
      if (linesOf(headers).length > 0 || 'headers' in initial) spec.headers = parsePairs(headers, ':');
    }
    return spec;
  };

  const loadSpec = (spec: McpSpec) => {
    setTransport(String(spec.type ?? (spec.url ? 'http' : 'stdio')));
    setCommand(String(spec.command ?? ''));
    setArgs(asStrings(spec.args).join('\n'));
    setEnv(Object.entries(asStringMap(spec.env)).map(([k, v]) => `${k}=${v}`).join('\n'));
    setUrl(String(spec.url ?? ''));
    setHeaders(Object.entries(asStringMap(spec.headers)).map(([k, v]) => `${k}: ${v}`).join('\n'));
  };

  const parseJson = (): McpSpec | null => {
    try {
      const value = JSON.parse(json);
      if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('{ … }');
      return value as McpSpec;
    } catch (err) {
      setError(interpolate(m.invalidJson, { error: errorMessage(err) }));
      return null;
    }
  };

  const switchMode = (next: 'form' | 'json') => {
    if (next === mode) return;
    setError(null);
    if (next === 'json') {
      setJson(JSON.stringify(specFromForm(), null, 2));
    } else {
      const spec = parseJson();
      if (!spec) return;
      loadSpec(spec);
    }
    setMode(next);
  };

  const nameValid = NAME_PATTERN.test(name) || (entry !== undefined && name === entry.name);

  const submit = () => {
    setError(null);
    const spec = mode === 'json' ? parseJson() : specFromForm();
    if (!spec) return;
    const kind = String(spec.type ?? (spec.url ? 'http' : 'stdio'));
    if (kind === 'stdio' && !String(spec.command ?? '').trim()) return setError(m.missingCommand);
    if (kind !== 'stdio' && !/^https?:\/\//.test(String(spec.url ?? '').trim())) return setError(m.missingUrl);
    onSubmit(clients, name, spec);
  };

  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (nameValid && clients.length > 0) submit();
      }}
    >
      <div className="flex items-center gap-2">
        <h4 className="text-sm font-semibold text-[#1d1d1f] dark:text-white flex-1">
          {entry ? interpolate(m.formEdit, { name: entry.name }) : m.formNew}
        </h4>
        <div className="flex items-center bg-black/[0.04] dark:bg-white/[0.06] p-0.5 rounded-full border border-black/[0.08] dark:border-white/[0.1]">
          {(['form', 'json'] as const).map((key) => (
            <button
              key={key}
              type="button"
              onClick={() => switchMode(key)}
              className={`px-3 py-0.5 rounded-full ${mode === key ? 'bg-white dark:bg-[#1c1c1e] text-[#0071e3] dark:text-[#2997ff] shadow-sm font-semibold' : 'text-[#86868b]'}`}
            >
              {key === 'form' ? m.modeForm : m.modeJson}
            </button>
          ))}
        </div>
      </div>

      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{m.name}</span>
        <input autoFocus value={name} onChange={(e) => setName(e.target.value.trim())} placeholder="my-server" className={`${inputClass} font-mono`} />
        <span className={`text-[11px] ${name && !nameValid ? 'text-[#d70015] dark:text-[#ff453a]' : 'text-[#86868b]'}`}>
          {name && !nameValid ? m.invalidName : m.formNameHint}
        </span>
      </label>

      {!entry && (
        <div className="flex items-center gap-3">
          <span className="font-semibold text-[#86868b]">{m.formClients}</span>
          {CLIENTS.map((client) => (
            <label key={client} className={`flex items-center gap-1.5 ${lockedClients.includes(client) ? 'opacity-40' : 'cursor-pointer'}`}>
              <input
                type="checkbox"
                disabled={lockedClients.includes(client)}
                checked={clients.includes(client)}
                onChange={(e) => setClients((prev) => (e.target.checked ? [...prev, client] : prev.filter((c) => c !== client)))}
              />
              {CLIENT_LABEL[client]}
            </label>
          ))}
        </div>
      )}

      {mode === 'json' ? (
        <label className="flex flex-col gap-1">
          <textarea value={json} onChange={(e) => setJson(e.target.value)} rows={14} spellCheck={false} className={`${inputClass} font-mono resize-y`} />
          <span className="text-[11px] text-[#86868b]">{m.jsonHint}</span>
        </label>
      ) : (
        <>
          <label className="flex flex-col gap-1">
            <span className="font-semibold text-[#86868b]">{m.transport}</span>
            <select value={transport} onChange={(e) => setTransport(e.target.value)} className={inputClass}>
              <option value="stdio">stdio</option>
              <option value="http">http</option>
              <option value="sse">sse</option>
            </select>
          </label>
          {transport === 'stdio' ? (
            <>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-[#86868b]">{m.command}</span>
                <input value={command} onChange={(e) => setCommand(e.target.value)} placeholder="npx" className={`${inputClass} font-mono`} />
              </label>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-[#86868b]">{m.args}</span>
                <textarea value={args} onChange={(e) => setArgs(e.target.value)} rows={3} spellCheck={false} className={`${inputClass} font-mono resize-y`} />
                <span className="text-[11px] text-[#86868b]">{m.argsHint}</span>
              </label>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-[#86868b]">{m.env}</span>
                <textarea value={env} onChange={(e) => setEnv(e.target.value)} rows={3} spellCheck={false} className={`${inputClass} font-mono resize-y`} />
                <span className="text-[11px] text-[#86868b]">{m.envHint}</span>
              </label>
            </>
          ) : (
            <>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-[#86868b]">{m.url}</span>
                <input value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://example.com/mcp" className={`${inputClass} font-mono`} />
              </label>
              <label className="flex flex-col gap-1">
                <span className="font-semibold text-[#86868b]">{m.headers}</span>
                <textarea value={headers} onChange={(e) => setHeaders(e.target.value)} rows={3} spellCheck={false} className={`${inputClass} font-mono resize-y`} />
                <span className="text-[11px] text-[#86868b]">{m.headersHint}</span>
              </label>
            </>
          )}
        </>
      )}

      {error && <p className="text-[#d70015] dark:text-[#ff453a]">{error}</p>}

      <div className="flex items-center justify-end gap-2 pt-1">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          {t.common.cancel}
        </Button>
        <Button size="sm" variant="primary" type="submit" disabled={busy || !nameValid || clients.length === 0}>
          {busy ? m.saving : entry ? m.save : m.create}
        </Button>
      </div>
    </form>
  );
};
