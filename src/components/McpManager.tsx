import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Copy, Eye, EyeOff, FileCode2, Lock, Pencil, Plus, Power, RefreshCw, Search, Trash2 } from 'lucide-react';
import { api, errorMessage } from '../api/client';
import { McpClient, McpLocation, McpServerEntry, McpSpec } from '../types';
import { Badge } from './common/Badge';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';
type Tab = 'overview' | McpClient;

interface McpManagerProps {
  isOpen: boolean;
  onClose: () => void;
  onToast: (type: ToastType, message: string) => void;
}

const CLIENTS: McpClient[] = ['claude', 'codex'];
const CLIENT_LABEL: Record<McpClient, string> = { claude: 'Claude', codex: 'Codex' };
const NAME_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;
/** Fields the form edits; everything else in a spec is kept as is. */
const CORE_FIELDS = ['type', 'command', 'args', 'env', 'url', 'headers'];

const otherClient = (client: McpClient): McpClient => (client === 'claude' ? 'codex' : 'claude');
const asStrings = (value: unknown): string[] => (Array.isArray(value) ? value.map(String) : []);
const asStringMap = (value: unknown): Record<string, string> =>
  value && typeof value === 'object' && !Array.isArray(value)
    ? Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([k, v]) => [k, String(v)]))
    : {};
const extraFields = (spec: McpSpec) => Object.fromEntries(Object.entries(spec).filter(([k]) => !CORE_FIELDS.includes(k)));

/** One line per server: the command line for stdio, the URL otherwise. */
export function serverSummary(entry: McpServerEntry): string {
  if (entry.transport === 'stdio') return [String(entry.spec.command ?? ''), ...asStrings(entry.spec.args)].join(' ');
  return String(entry.spec.url ?? '');
}

const inputClass =
  'w-full bg-black/[0.04] dark:bg-white/[0.06] px-3 py-2 rounded-xl border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]';

export const McpManager: React.FC<McpManagerProps> = ({ isOpen, onClose, onToast }) => {
  const { t, interpolate } = useI18n();
  const m = t.mcp;
  const [locations, setLocations] = useState<McpLocation[]>([]);
  const [tab, setTab] = useState<Tab>('overview');
  const [selected, setSelected] = useState<{ client: McpClient; name: string } | null>(null);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<{ entry?: McpServerEntry } | null>(null);
  const [confirm, setConfirm] = useState<'delete' | 'overwrite' | null>(null);
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
    if (isOpen) {
      setQuery('');
      setEditing(null);
      setConfirm(null);
      void refresh();
    }
    // Reload only when the dialog opens; actions refresh explicitly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen]);

  useEffect(() => {
    setConfirm(null);
    setReveal(false);
    setShowRaw(false);
  }, [selected?.client, selected?.name]);

  const location = (client: McpClient) => locations.find((l) => l.client === client);
  const find = (client: McpClient, name: string) => location(client)?.servers.find((s) => s.name === name);
  const entry = selected ? find(selected.client, selected.name) ?? null : null;

  const matches = useCallback(
    (s: McpServerEntry) => {
      const q = query.trim().toLowerCase();
      return !q || [s.name, serverSummary(s)].some((v) => v.toLowerCase().includes(q));
    },
    [query]
  );

  // Overview rows: every server name in either client.
  const overviewNames = useMemo(() => {
    const names = new Map<string, string>();
    for (const l of locations) for (const s of l.servers) if (matches(s)) names.set(s.name.toLowerCase(), s.name);
    return [...names.values()].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
  }, [locations, matches]);

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

  const toggleEnabled = (server: McpServerEntry) =>
    run(async () => {
      await api.setMcpEnabled(server.client, server.name, !server.enabled);
      onToast('success', interpolate(server.enabled ? m.toastDisabled : m.toastEnabled, { name: server.name }));
      await refresh();
    });

  const copyTo = (server: McpServerEntry, overwrite: boolean) =>
    run(async () => {
      const to = otherClient(server.client);
      const outcome = await api.copyMcpServer(server.client, server.name, to, overwrite);
      setConfirm(null);
      if (outcome.dropped_fields.length > 0) {
        onToast('warning', interpolate(m.toastDropped, { name: server.name, client: CLIENT_LABEL[to], fields: outcome.dropped_fields.join(', ') }));
      } else {
        onToast('success', interpolate(m.toastCopied, { name: server.name, client: CLIENT_LABEL[to] }));
      }
      await refresh();
    });

  const requestCopy = (server: McpServerEntry) => {
    const existing = find(otherClient(server.client), server.name);
    // A disabled entry of the same name is refused by the backend with an explanation.
    if (existing?.enabled) setConfirm('overwrite');
    else void copyTo(server, false);
  };

  const deleteServer = (server: McpServerEntry) =>
    run(async () => {
      await api.deleteMcpServer(server.client, server.name);
      onToast('success', interpolate(m.toastDeleted, { name: server.name }));
      setSelected(null);
      await refresh();
    });

  const save = (clients: McpClient[], name: string, spec: McpSpec) =>
    run(async () => {
      const original = editing?.entry?.name ?? null;
      for (const client of clients) {
        await api.upsertMcpServer(client, original, name, spec);
      }
      onToast('success', interpolate(original ? m.toastSaved : m.toastCreated, { name }));
      setEditing(null);
      await refresh();
      setSelected({ client: clients[0], name });
      if (tab !== 'overview') setTab(clients[0]);
    });

  const select = (client: McpClient, name: string) => {
    setSelected({ client, name });
    setEditing(null);
  };

  const activeLocation = tab === 'overview' ? undefined : location(tab);
  const writable = (server: McpServerEntry) => !server.read_only && !location(server.client)?.error;

  const badges = (server: McpServerEntry) => (
    <>
      {!server.enabled && <Badge tone="gray">{m.disabled}</Badge>}
      {server.read_only && <Badge tone="purple"><Lock size={9} />{m.readOnly}</Badge>}
      {server.cc_switch != null && <Badge tone="orange" title={m.ccSwitchHint}>{m.ccSwitch}</Badge>}
    </>
  );

  return (
    <Modal isOpen={isOpen} onClose={onClose} title={m.title} subtitle={m.subtitle} widthClass="max-w-6xl">
      <div className="flex flex-col gap-3 h-[68vh] text-xs select-text">
        <div className="flex items-center gap-2 flex-wrap">
          <div className="flex items-center bg-black/[0.04] dark:bg-white/[0.06] p-1 rounded-full border border-black/[0.08] dark:border-white/[0.1] gap-1">
            {(['overview', ...CLIENTS] as Tab[]).map((key) => (
              <button
                key={key}
                onClick={() => setTab(key)}
                className={`flex items-center gap-1.5 px-3 py-1 rounded-full transition-all ${
                  tab === key
                    ? 'bg-white dark:bg-[#1c1c1e] text-[#0071e3] dark:text-[#2997ff] shadow-sm font-semibold'
                    : 'text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white'
                }`}
              >
                {key === 'overview' ? m.overview : CLIENT_LABEL[key]}
                {key !== 'overview' && <span className="text-[10px] font-mono opacity-70">{location(key)?.servers.length ?? 0}</span>}
              </button>
            ))}
          </div>
          <div className="flex-1" />
          <div className="relative">
            <Search size={12} className="absolute left-3 top-1/2 -translate-y-1/2 text-[#86868b]" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={m.search}
              className="w-56 bg-black/[0.04] dark:bg-white/[0.06] pl-8 pr-3 py-1.5 rounded-full border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]"
            />
          </div>
          <Button size="sm" variant="ghost" onClick={() => refresh()} disabled={loading} icon={<RefreshCw size={13} className={loading ? 'animate-spin' : ''} />}>
            {m.refresh}
          </Button>
          <Button
            size="sm"
            variant="primary"
            icon={<Plus size={13} />}
            onClick={() => {
              setEditing({});
              setSelected(null);
            }}
          >
            {m.newServer}
          </Button>
        </div>

        <div className="flex items-center gap-2 text-[11px] text-[#86868b] min-h-[18px]">
          {(activeLocation ? [activeLocation] : locations).map((l) => (
            <span key={l.client} className="flex items-center gap-1.5 min-w-0">
              {!activeLocation && <span className="font-semibold">{CLIENT_LABEL[l.client]}</span>}
              <span className="font-mono truncate" title={l.config_path}>{l.config_path}</span>
              {!l.exists && <Badge tone="orange"><AlertTriangle size={10} />{m.configMissing}</Badge>}
              {l.error && <Badge tone="red" title={l.error}><AlertTriangle size={10} />{interpolate(m.configError, { error: l.error })}</Badge>}
            </span>
          ))}
          <span className="ml-auto shrink-0">{m.sessionHint}</span>
        </div>

        <div className="flex gap-3 flex-1 min-h-0">
          {/* Server list or overview matrix */}
          <div className="w-[46%] overflow-y-auto pr-1">
            {tab === 'overview' ? (
              <table className="w-full border-separate border-spacing-y-1" role="grid">
                <thead>
                  <tr className="text-[11px] text-[#86868b]">
                    <th className="text-left font-semibold px-3">{m.name}</th>
                    {CLIENTS.map((c) => (
                      <th key={c} className="text-left font-semibold px-2 w-[34%]">{CLIENT_LABEL[c]}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {overviewNames.length === 0 && (
                    <tr>
                      <td colSpan={3} className="text-center text-[#86868b] py-10">{query ? m.noMatch : m.empty}</td>
                    </tr>
                  )}
                  {overviewNames.map((name) => (
                    <tr key={name}>
                      <td className="px-3 py-2 rounded-l-xl bg-black/[0.02] dark:bg-white/[0.03] font-semibold text-[#1d1d1f] dark:text-white break-all">{name}</td>
                      {CLIENTS.map((client, i) => {
                        const server = find(client, name);
                        const source = find(otherClient(client), name);
                        const active = selected?.client === client && selected.name === name;
                        return (
                          <td key={client} className={`px-2 py-1.5 bg-black/[0.02] dark:bg-white/[0.03] ${i === CLIENTS.length - 1 ? 'rounded-r-xl' : ''}`}>
                            {server ? (
                              <button
                                onClick={() => select(client, name)}
                                aria-label={`${CLIENT_LABEL[client]} ${name}`}
                                className={`w-full text-left px-2 py-1 rounded-lg border transition-all flex items-center gap-1 flex-wrap ${
                                  active ? 'border-[#0071e3]/50 bg-[#0071e3]/[0.06]' : 'border-transparent hover:bg-black/[0.04] dark:hover:bg-white/[0.06]'
                                }`}
                              >
                                <Badge tone={server.enabled ? 'green' : 'gray'}>{server.enabled ? m.enable : m.disabled}</Badge>
                                {server.read_only && <Lock size={10} className="text-[#af52de]" />}
                                {server.cc_switch != null && <Badge tone="orange" title={m.ccSwitchHint}>cc</Badge>}
                              </button>
                            ) : source && writable(source) && !location(client)?.error ? (
                              <Button size="sm" variant="link" disabled={busy} title={m.copyHint} onClick={() => copyTo(source, false)}>
                                {interpolate(m.copyHere, { client: CLIENT_LABEL[source.client] })}
                              </Button>
                            ) : (
                              <span className="px-2 text-[#86868b]">{m.notConfigured}</span>
                            )}
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            ) : (
              <div className="space-y-1.5" role="list">
                {(activeLocation?.servers.filter(matches) ?? []).length === 0 && (
                  <div className="text-center text-[#86868b] py-10">{query ? m.noMatch : m.empty}</div>
                )}
                {activeLocation?.servers.filter(matches).map((server) => {
                  const active = selected?.client === server.client && selected.name === server.name;
                  return (
                    <button
                      key={`${server.name}:${server.enabled}`}
                      role="listitem"
                      onClick={() => select(server.client, server.name)}
                      className={`w-full text-left p-3 rounded-2xl border transition-all ${
                        active
                          ? 'border-[#0071e3]/50 bg-[#0071e3]/[0.06] dark:bg-[#2997ff]/10'
                          : 'border-black/[0.06] dark:border-white/[0.08] hover:bg-black/[0.03] dark:hover:bg-white/[0.04]'
                      } ${server.enabled ? '' : 'opacity-60'}`}
                    >
                      <div className="flex items-center gap-1.5 flex-wrap">
                        <span className="font-semibold text-[#1d1d1f] dark:text-white">{server.name}</span>
                        <Badge tone="blue">{server.transport}</Badge>
                        {badges(server)}
                      </div>
                      <p className="mt-1 text-[11px] font-mono text-[#6e6e73] dark:text-[#a1a1a6] truncate">{serverSummary(server)}</p>
                    </button>
                  );
                })}
              </div>
            )}
          </div>

          {/* Detail or form */}
          <div className="flex-1 min-w-0 overflow-y-auto rounded-2xl border border-black/[0.06] dark:border-white/[0.08] bg-black/[0.015] dark:bg-white/[0.02] p-4">
            {editing ? (
              <McpServerForm
                key={editing.entry ? `${editing.entry.client}:${editing.entry.name}` : 'new'}
                entry={editing.entry}
                defaultClients={tab === 'overview' ? ['claude'] : [tab]}
                lockedClients={locations.filter((l) => l.error).map((l) => l.client)}
                busy={busy}
                onCancel={() => setEditing(null)}
                onSubmit={save}
              />
            ) : !entry ? (
              <div className="h-full flex items-center justify-center text-[#86868b]">{m.selectHint}</div>
            ) : (
              <div className="flex flex-col gap-3">
                <div className="flex items-center gap-1.5 flex-wrap">
                  <h4 className="text-sm font-semibold text-[#1d1d1f] dark:text-white mr-1">{entry.name}</h4>
                  <Badge tone="blue">{CLIENT_LABEL[entry.client]}</Badge>
                  {badges(entry)}
                </div>

                <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1.5 text-[11px]">
                  <dt className="text-[#86868b]">{m.configFile}</dt>
                  <dd className="font-mono break-all">{location(entry.client)?.config_path}</dd>
                  <dt className="text-[#86868b]">{m.transport}</dt>
                  <dd className="font-mono">{entry.transport}</dd>
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
                        {reveal
                          ? JSON.stringify(extraFields(entry.spec), null, 1)
                          : Object.keys(extraFields(entry.spec)).join(', ')}
                      </dd>
                    </>
                  )}
                </dl>

                <div className="flex items-center gap-2 flex-wrap">
                  <Button size="sm" variant="secondary" icon={<Pencil size={13} />} disabled={busy || !writable(entry)} onClick={() => setEditing({ entry })}>
                    {m.edit}
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={<Power size={13} />}
                    disabled={busy || !writable(entry)}
                    title={entry.enabled ? (entry.client === 'claude' ? m.disableTooltipClaude : m.disableTooltipCodex) : m.enableTooltip}
                    onClick={() => toggleEnabled(entry)}
                  >
                    {entry.enabled ? m.disable : m.enable}
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={<Copy size={13} />}
                    disabled={busy || !writable(entry) || Boolean(location(otherClient(entry.client))?.error)}
                    title={m.copyHint}
                    onClick={() => requestCopy(entry)}
                  >
                    {interpolate(m.copyTo, { client: CLIENT_LABEL[otherClient(entry.client)] })}
                  </Button>
                  <Button size="sm" variant="danger" icon={<Trash2 size={13} />} disabled={busy || !writable(entry)} onClick={() => setConfirm('delete')}>
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

                {entry.cc_switch != null && (
                  <p className="flex items-start gap-1.5 text-[11px] text-[#c93400] dark:text-[#ff9f0a] leading-relaxed">
                    <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                    {m.ccSwitchHint}
                  </p>
                )}

                {confirm && (
                  <div className="p-3 rounded-2xl border border-[#ff453a]/25 bg-[#ff453a]/[0.06] flex items-center gap-3">
                    <p className="flex-1 leading-relaxed">
                      {confirm === 'delete' ? m.deleteConfirm : interpolate(m.overwriteConfirm, { client: CLIENT_LABEL[otherClient(entry.client)] })}
                    </p>
                    <Button size="sm" variant="ghost" onClick={() => setConfirm(null)}>
                      {t.common.cancel}
                    </Button>
                    <Button
                      size="sm"
                      variant="danger"
                      disabled={busy}
                      onClick={() => (confirm === 'delete' ? deleteServer(entry) : copyTo(entry, true))}
                    >
                      {confirm === 'delete' ? m.confirmDelete : m.overwrite}
                    </Button>
                  </div>
                )}

                {showRaw && (
                  <pre className="whitespace-pre-wrap break-all font-mono text-[11px] leading-relaxed p-3 rounded-xl bg-black/[0.04] dark:bg-black/40 border border-black/[0.06] dark:border-white/[0.06] max-h-[40vh] overflow-y-auto">
                    {entry.raw}
                  </pre>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </Modal>
  );
};

interface McpServerFormProps {
  /** The server being edited; absent when creating. */
  entry?: McpServerEntry;
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
  const [clients, setClients] = useState<McpClient[]>(entry ? [entry.client] : defaultClients.filter((c) => !lockedClients.includes(c)));
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
