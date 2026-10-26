import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Code2, Ellipsis, FolderMinus, FolderOpen, FolderPlus, Link2, Lock, Plus, RefreshCw, Search, Trash2 } from 'lucide-react';
import { api, errorMessage } from '../api/client';
import { SkillClient, SkillLocation } from '../types';
import { formatDateTime } from '../utils/formatters';
import {
  SKILL_CLIENTS,
  SkillGroup,
  SkillSource,
  baseName,
  canToggle,
  deletePlan,
  groupSkills,
  isOnIn,
  linkSource,
  pathKey,
} from '../utils/skillGroups';
import { Badge } from './common/Badge';
import { Button } from './common/Button';
import { DropdownMenu } from './common/DropdownMenu';
import { PageHeader } from './common/PageHeader';
import { Segmented } from './common/Segmented';
import { Switch } from './common/Switch';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';
type Filter = 'all' | SkillClient | 'off';

interface SkillsManagerProps {
  onToast: (type: ToastType, message: string) => void;
}

const LIBRARIES_KEY = 'cv_skill_libraries';
const CLIENTS = SKILL_CLIENTS;
const CLIENT_LABEL: Record<SkillClient, string> = { claude: 'Claude', codex: 'Codex', antigravity: 'Antigravity' };
const NAME_PATTERN = /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/;

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

function loadLibraries(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(LIBRARIES_KEY) || '[]');
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string' && v.trim() !== '') : [];
  } catch {
    return [];
  }
}

/** One list of every skill with a switch per client, like cc-switch; switching on links the original in. */
export const SkillsManager: React.FC<SkillsManagerProps> = ({ onToast }) => {
  const { t, interpolate } = useI18n();
  const s = t.skills;
  const [libraries, setLibraries] = useState<string[]>(loadLibraries);
  const [locations, setLocations] = useState<SkillLocation[]>([]);
  const [filter, setFilter] = useState<Filter>('all');
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [creating, setCreating] = useState(false);
  const [preview, setPreview] = useState<{ path: string; text?: string; error?: string } | null>(null);

  const fail = useCallback((err: unknown) => onToast('error', interpolate(s.toastFailed, { error: errorMessage(err) })), [onToast, interpolate, s.toastFailed]);

  const refresh = useCallback(
    async (libs: string[] = libraries) => {
      setLoading(true);
      try {
        setLocations(await api.listSkills(libs));
      } catch (err) {
        fail(err);
      } finally {
        setLoading(false);
      }
    },
    [libraries, fail]
  );

  useEffect(() => {
    void refresh();
    // Reload only when the page opens; actions refresh explicitly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const saveLibraries = (next: string[]) => {
    setLibraries(next);
    try {
      localStorage.setItem(LIBRARIES_KEY, JSON.stringify(next));
    } catch {
      // Storage may be unavailable; the list still works for this session.
    }
  };

  const groups = useMemo(() => groupSkills(locations), [locations]);
  const isOff = (g: SkillGroup) => !g.system && !CLIENTS.some((c) => isOnIn(g, c));
  const counts = {
    all: groups.length,
    off: groups.filter(isOff).length,
    ...Object.fromEntries(CLIENTS.map((c) => [c, groups.filter((g) => isOnIn(g, c)).length])),
  } as Record<Filter, number>;
  const visibleGroups = useMemo(() => {
    const q = query.trim().toLowerCase();
    return groups.filter(
      (g) =>
        (filter === 'all' || (filter === 'off' ? isOff(g) : isOnIn(g, filter))) &&
        (!q || [g.name, g.title, g.description].some((v) => v?.toLowerCase().includes(q)))
    );
    // isOff only reads the group.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [groups, filter, query]);
  const selected = groups.find((g) => g.key === selectedKey) ?? null;
  // Links are read through their target, so any entry of the skill shows its SKILL.md.
  const previewPath = selected && (selected.original ?? selected.clients.claude ?? selected.clients.codex)?.path;

  useEffect(() => {
    setConfirmDelete(false);
    if (!selected || !previewPath || !selected.hasSkillFile || selected.linkBroken) {
      setPreview(null);
      return;
    }
    let cancelled = false;
    setPreview({ path: previewPath });
    api
      .readSkillFile(libraries, previewPath)
      .then((text) => !cancelled && setPreview({ path: previewPath, text }))
      .catch((err) => !cancelled && setPreview({ path: previewPath, error: errorMessage(err) }));
    return () => {
      cancelled = true;
    };
    // Re-read only when another skill is selected.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected?.key, previewPath]);

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

  const addLibrary = () =>
    run(async () => {
      const picked = await api.pickFolderPath();
      if (!picked) return;
      if (libraries.some((l) => pathKey(l) === pathKey(picked))) {
        onToast('info', s.toastLibraryExists);
        return;
      }
      const next = [...libraries, picked];
      saveLibraries(next);
      await refresh(next);
      onToast('success', interpolate(s.toastLibraryAdded, { path: picked }));
    });

  const removeLibrary = (root: string) => {
    const next = libraries.filter((l) => pathKey(l) !== pathKey(root));
    saveLibraries(next);
    void refresh(next);
  };

  /** Off moves the client's copy to skills-disabled; on moves it back, or links the original in. */
  /** Turns a skill on in a client: moves its copy back from skills-disabled, or links the original in. */
  const switchOn = async (group: SkillGroup, client: SkillClient) => {
    const entry = group.clients[client];
    if (entry) {
      await api.setSkillEnabled(libraries, entry.path, true);
    } else if (group.original && group.sources[0]?.origin.kind === 'library') {
      await api.mountSkill(libraries, group.original.path, client);
    } else {
      const source = linkSource(group, client);
      if (source) await api.linkSkillToClient(libraries, source.path, client);
    }
  };

  const toggleClient = (group: SkillGroup, client: SkillClient) =>
    run(async () => {
      const entry = group.clients[client];
      const vars = { name: group.name, client: CLIENT_LABEL[client] };
      if (entry?.enabled) {
        await api.setSkillEnabled(libraries, entry.path, false);
        onToast('success', interpolate(s.toastDisabledIn, vars));
      } else {
        await switchOn(group, client);
        onToast('success', interpolate(s.toastEnabledIn, vars));
      }
      await refresh();
    });

  /** The listed skills a client could load but does not; enable-all acts on these. */
  const offIn = (client: SkillClient) => visibleGroups.filter((g) => !isOnIn(g, client) && canToggle(g, client));

  const enableAll = (client: SkillClient) =>
    run(async () => {
      const failed: string[] = [];
      let done = 0;
      for (const group of offIn(client)) {
        try {
          await switchOn(group, client);
          done += 1;
        } catch {
          failed.push(group.name);
        }
      }
      if (done > 0) onToast('success', interpolate(s.toastEnabledAll, { count: done, client: CLIENT_LABEL[client] }));
      if (failed.length > 0) onToast('warning', interpolate(s.toastEnableAllFailed, { count: failed.length, names: failed.join('、') }));
      await refresh();
    });

  const deleteGroup = (group: SkillGroup) =>
    run(async () => {
      const { links, recycle } = deletePlan(group);
      // Links first: a folder cannot be recycled while a client still links to it.
      for (const link of links) await api.deleteSkill(libraries, link.path);
      for (const folder of recycle) await api.deleteSkill(libraries, folder.path);
      onToast('success', interpolate(recycle.length > 0 ? s.toastRecycled : s.toastLinkRemoved, { name: group.name }));
      setSelectedKey(null);
      await refresh();
    });

  const sourceLabel = ({ origin }: SkillSource) => {
    if (origin.kind === 'library') return `${s.library} · ${baseName(origin.root)}`;
    if (origin.kind === 'external') return origin.folder;
    return interpolate(s.originClient, { client: CLIENT_LABEL[origin.kind] });
  };
  /** Where the files come from; two labels when the clients use different copies. */
  const originLabel = (group: SkillGroup) => [...new Set(group.sources.map(sourceLabel))].join(' / ');

  const stateLabel = (group: SkillGroup, client: SkillClient) => {
    const entry = group.clients[client];
    if (!entry) return s.stateNone;
    if (entry.system) return s.system;
    return entry.enabled ? s.stateOn : s.disabled;
  };

  const badges = (group: SkillGroup) => (
    <>
      {group.system && <Badge tone="purple"><Lock size={9} />{s.system}</Badge>}
      {group.linkBroken && <Badge tone="red">{s.linkBroken}</Badge>}
      {!group.hasSkillFile && !group.linkBroken && <Badge tone="orange">{s.noSkillFile}</Badge>}
    </>
  );

  const libraryLocations = locations.filter((l) => l.kind === 'library');

  return (
    <div className="flex-1 flex flex-col min-h-0 text-xs">
      <PageHeader
        title={s.button}
        tabs={
          <Segmented
            value={filter}
            onChange={setFilter}
            items={[
              { key: 'all' as Filter, label: s.filterAll, count: counts.all },
              ...CLIENTS.map((client) => ({ key: client as Filter, label: CLIENT_LABEL[client], count: counts[client] })),
              { key: 'off' as Filter, label: s.filterOff, count: counts.off },
            ]}
          />
        }
      >
        <div className="relative">
          <Search size={12} className="absolute left-3 top-1/2 -translate-y-1/2 text-[#86868b]" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={s.search}
            className="w-56 bg-black/[0.045] dark:bg-white/[0.07] pl-8 pr-3 py-1.5 rounded-lg border border-transparent focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]"
          />
        </div>
        <Button
          variant="primary"
          onClick={() => {
            setCreating(true);
            setSelectedKey(null);
          }}
          icon={<Plus size={13} />}
        >
          {s.newSkill}
        </Button>
        <Button
          variant="ghost"
          onClick={() => refresh()}
          disabled={loading}
          title={s.refresh}
          aria-label={s.refresh}
          className="px-2"
          icon={<RefreshCw size={14} className={loading ? 'animate-spin' : ''} />}
        />
        <DropdownMenu
          variant="ghost"
          title={t.sessions.more}
          icon={<Ellipsis size={16} />}
          items={[
            { key: 'add-library', label: s.addLibrary, icon: <FolderPlus size={13} />, onSelect: addLibrary, disabled: busy },
            ...libraryLocations.map((l) => ({
              key: `remove:${l.root}`,
              label: interpolate(s.removeLibraryNamed, { name: baseName(l.root) }),
              icon: <FolderMinus size={13} />,
              onSelect: () => removeLibrary(l.root),
            })),
          ]}
        />
      </PageHeader>

      <div className="flex-1 flex min-h-0">
        <div className="flex-1 min-w-0 flex flex-col bg-white dark:bg-[#121214] border-r border-black/[0.06] dark:border-white/[0.08]">
          <div className="flex-1 overflow-y-auto">
            <table className="w-full border-separate border-spacing-0" role="grid">
              <thead>
                <tr className="text-[11px] text-[#86868b]">
                  <th className="sticky top-0 z-10 bg-white dark:bg-[#121214] text-left font-medium px-4 py-2.5 border-b border-black/[0.06] dark:border-white/[0.08]">{s.columnSkill}</th>
                  {CLIENTS.map((c) => (
                    <th key={c} className="sticky top-0 z-10 bg-white dark:bg-[#121214] text-left font-medium px-4 py-2 w-[120px] border-b border-black/[0.06] dark:border-white/[0.08]">
                      <div className="flex flex-col items-start gap-0.5">
                        <span>{CLIENT_LABEL[c]}</span>
                        <button
                          type="button"
                          onClick={() => enableAll(c)}
                          disabled={busy || offIn(c).length === 0}
                          title={interpolate(s.enableAllTooltip, { client: CLIENT_LABEL[c], count: offIn(c).length })}
                          aria-label={interpolate(s.enableAllIn, { client: CLIENT_LABEL[c] })}
                          className="text-[11px] font-normal text-[#0071e3] dark:text-[#2997ff] hover:underline disabled:text-[#86868b] disabled:opacity-50 disabled:no-underline disabled:cursor-default"
                        >
                          {s.enableAll}
                        </button>
                      </div>
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {visibleGroups.length === 0 && (
                  <tr>
                    <td colSpan={3} className="text-center text-[#86868b] py-10">{query ? s.noMatch : s.empty}</td>
                  </tr>
                )}
                {visibleGroups.map((group) => (
                  <tr
                    key={group.key}
                    className={selected?.key === group.key ? 'bg-[#0071e3]/[0.05] dark:bg-[#2997ff]/10' : 'hover:bg-black/[0.02] dark:hover:bg-white/[0.03]'}
                  >
                    <td className="px-4 py-2.5 border-b border-black/[0.05] dark:border-white/[0.06] max-w-0">
                      <button
                        type="button"
                        onClick={() => {
                          setSelectedKey(group.key);
                          setCreating(false);
                        }}
                        className="text-left w-full min-w-0"
                      >
                        <div className="flex items-center gap-1.5 flex-wrap">
                          <span className={`font-semibold text-[13px] text-[#1d1d1f] dark:text-white ${isOff(group) ? 'opacity-60' : ''}`}>{group.name}</span>
                          {badges(group)}
                        </div>
                        <div className="mt-0.5 text-[11.5px] text-[#86868b] truncate">
                          <span className="text-[#6e6e73] dark:text-[#a1a1a6]">{originLabel(group)}</span>
                          {group.description && ` · ${group.description}`}
                        </div>
                      </button>
                    </td>
                    {CLIENTS.map((client) => (
                      <td key={client} className="px-4 py-2.5 border-b border-black/[0.05] dark:border-white/[0.06]">
                        {group.clients[client]?.system ? (
                          <Badge tone="purple"><Lock size={9} />{s.stateBuiltIn}</Badge>
                        ) : group.system ? (
                          <span className="text-[#86868b]">—</span>
                        ) : (
                          <Switch
                            checked={isOnIn(group, client)}
                            disabled={busy || !canToggle(group, client)}
                            label={`${CLIENT_LABEL[client]} ${group.name}`}
                            title={stateLabel(group, client)}
                            onChange={() => toggleClient(group, client)}
                          />
                        )}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <div className="px-4 py-2.5 border-t border-black/[0.06] dark:border-white/[0.08] text-[11px] text-[#86868b] flex flex-col gap-1">
            {locations.map((l) => (
              <div key={`${l.kind}:${l.root}`} className="flex items-center gap-1.5 min-w-0">
                <span className="font-semibold shrink-0">{l.kind === 'library' ? `${s.library} · ${baseName(l.root)}` : CLIENT_LABEL[l.kind]}</span>
                <span className="font-mono truncate" title={l.root}>{l.root}</span>
                {/* A client creates its skills folder on the first switch-on, so only a missing library is a problem. */}
                {!l.exists && l.kind === 'library' && (
                  <Badge tone="orange">
                    <AlertTriangle size={10} />
                    {interpolate(s.folderMissing, { path: '' }).replace(/[:：]\s*$/, '')}
                  </Badge>
                )}
              </div>
            ))}
            <span>{s.unifiedHint}</span>
          </div>
        </div>

        {/* Detail or create form */}
        <div className="w-[440px] flex-shrink-0 overflow-y-auto p-6 select-text">
          {creating ? (
            <CreateSkillForm
              locations={locations}
              defaultTarget={locations[0]}
              busy={busy}
              onCancel={() => setCreating(false)}
              onCreate={(target, name, description, mountTo) =>
                run(async () => {
                  const path = await api.createSkill(libraries, target, name, description, mountTo);
                  onToast('success', interpolate(s.toastCreated, { name }));
                  await refresh();
                  setCreating(false);
                  setSelectedKey(pathKey(path));
                })
              }
            />
          ) : !selected ? (
            <div className="h-full flex items-center justify-center text-[#86868b]">{s.selectHint}</div>
          ) : (
            (() => {
              const plan = deletePlan(selected);
              const info = selected.original ?? selected.clients.claude ?? selected.clients.codex;
              const folder = selected.sources[0]?.path;
              return (
                <div className="flex flex-col gap-3">
                  <div>
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <h2 className="text-lg font-semibold tracking-tight text-[#1d1d1f] dark:text-white mr-1">{selected.title || selected.name}</h2>
                      {badges(selected)}
                      {selected.ccSwitch && <Badge tone="orange" title={s.ccSwitchHint}>{s.ccSwitch}</Badge>}
                    </div>
                    {selected.description && (
                      <p className="mt-1 text-[#6e6e73] dark:text-[#a1a1a6] leading-relaxed whitespace-pre-wrap">{selected.description}</p>
                    )}
                  </div>

                  <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-2 text-[11px]">
                    {CLIENTS.map((client) => {
                      const entry = selected.clients[client];
                      return (
                        <React.Fragment key={client}>
                          <dt className="text-[#86868b]">{CLIENT_LABEL[client]}</dt>
                          <dd className="min-w-0">
                            <span className={isOnIn(selected, client) ? 'text-[#248a3d] dark:text-[#30d158] font-medium' : 'text-[#86868b]'}>
                              {stateLabel(selected, client)}
                            </span>
                            {entry && (
                              <div className="font-mono break-all text-[#6e6e73] dark:text-[#a1a1a6]">
                                {entry.path}
                                {entry.link_target && (
                                  <div className={`flex items-start gap-1 ${entry.link_broken ? 'text-[#d70015] dark:text-[#ff453a]' : 'text-[#0071e3] dark:text-[#2997ff]'}`}>
                                    <Link2 size={10} className="mt-[3px] shrink-0" />
                                    {entry.link_target}
                                  </div>
                                )}
                              </div>
                            )}
                          </dd>
                        </React.Fragment>
                      );
                    })}
                    <dt className="text-[#86868b]">{s.original}</dt>
                    <dd className="min-w-0 flex flex-col gap-1">
                      {selected.sources.map((source) => (
                        <div key={source.path}>
                          <span>{sourceLabel(source)}</span>
                          <div className="font-mono break-all text-[#6e6e73] dark:text-[#a1a1a6]">{source.path}</div>
                        </div>
                      ))}
                      {selected.sources.length > 1 && (
                        <span className="flex items-start gap-1 text-[#c93400] dark:text-[#ff9f0a]">
                          <AlertTriangle size={11} className="mt-[2px] shrink-0" />
                          {s.multipleSources}
                        </span>
                      )}
                    </dd>
                    {info && (
                      <>
                        <dt />
                        <dd className="text-[#86868b]">
                          {interpolate(s.files, { count: info.file_count, size: formatBytes(info.total_bytes) })}
                          {info.modified_at && ` · ${interpolate(s.modified, { time: formatDateTime(info.modified_at) })}`}
                        </dd>
                      </>
                    )}
                  </dl>

                  <div className="flex items-center gap-2 flex-wrap">
                    <Button size="sm" icon={<FolderOpen size={13} />} disabled={!folder || selected.linkBroken} onClick={() => folder && run(() => api.openFolder(folder))}>
                      {s.openFolder}
                    </Button>
                    <Button
                      size="sm"
                      icon={<Code2 size={13} />}
                      disabled={!folder || selected.linkBroken}
                      onClick={() => folder && run(async () => void (await api.openVSCode(folder)))}
                    >
                      {s.openVSCode}
                    </Button>
                    {!selected.system && (
                      <Button size="sm" variant="danger" icon={<Trash2 size={13} />} disabled={busy} onClick={() => setConfirmDelete(true)}>
                        {s.delete}
                      </Button>
                    )}
                  </div>

                  {selected.ccSwitch && (
                    <p className="flex items-start gap-1.5 text-[11px] text-[#c93400] dark:text-[#ff9f0a] leading-relaxed">
                      <AlertTriangle size={12} className="mt-0.5 shrink-0" />
                      {s.ccSwitchHint}
                    </p>
                  )}

                  {confirmDelete && (
                    <div className="p-3 rounded-xl border border-[#ff453a]/25 bg-[#ff453a]/[0.06] flex items-center gap-3">
                      <p className="flex-1 leading-relaxed">
                        {plan.links.length > 0 &&
                          interpolate(s.deleteLinks, {
                            clients: CLIENTS.filter((c) => plan.links.includes(selected.clients[c]!)).map((c) => CLIENT_LABEL[c]).join('、'),
                          })}
                        {plan.recycle.length > 0
                          ? interpolate(s.deleteRecycle, { path: plan.recycle.map((f) => f.path).join('、') })
                          : interpolate(s.deleteKeep, { origin: originLabel(selected) })}
                      </p>
                      <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(false)}>
                        {t.common.cancel}
                      </Button>
                      <Button size="sm" variant="danger" disabled={busy} onClick={() => deleteGroup(selected)}>
                        {s.confirmDelete}
                      </Button>
                    </div>
                  )}

                  {preview && preview.path === previewPath && (
                    <div>
                      <div className="text-[11px] font-semibold text-[#86868b] mb-1.5">{s.preview}</div>
                      {preview.error ? (
                        <p className="text-[#d70015] dark:text-[#ff453a]">{preview.error}</p>
                      ) : (
                        <pre className="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed p-3 rounded-xl bg-black/[0.04] dark:bg-black/40 border border-black/[0.06] dark:border-white/[0.06] max-h-[50vh] overflow-y-auto">
                          {preview.text ?? s.previewLoading}
                        </pre>
                      )}
                    </div>
                  )}
                </div>
              );
            })()
          )}
        </div>
      </div>
    </div>
  );
};

interface CreateSkillFormProps {
  locations: SkillLocation[];
  defaultTarget?: SkillLocation;
  busy: boolean;
  onCancel: () => void;
  onCreate: (target: string, name: string, description: string, mountTo: SkillClient[]) => void;
}

/** `target` is `claude`, `codex` or a library root, matching the backend. */
const targetOf = (location: SkillLocation) => (location.kind === 'library' ? location.root : location.kind);

const CreateSkillForm: React.FC<CreateSkillFormProps> = ({ locations, defaultTarget, busy, onCancel, onCreate }) => {
  const { t } = useI18n();
  const s = t.skills;
  const [target, setTarget] = useState(defaultTarget ? targetOf(defaultTarget) : 'claude');
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [mountTo, setMountTo] = useState<SkillClient[]>([]);
  const isLibrary = !(CLIENTS as string[]).includes(target);
  const nameValid = NAME_PATTERN.test(name);
  const inputClass =
    'w-full bg-black/[0.04] dark:bg-white/[0.06] px-3 py-2 rounded-xl border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]';

  return (
    <form
      className="flex flex-col gap-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (nameValid && description.trim()) onCreate(target, name, description.trim(), isLibrary ? mountTo : []);
      }}
    >
      <h4 className="text-sm font-semibold text-[#1d1d1f] dark:text-white">{s.createTitle}</h4>
      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{s.createLocation}</span>
        <select value={target} onChange={(e) => setTarget(e.target.value)} className={inputClass}>
          {locations.map((location) => (
            <option key={targetOf(location)} value={targetOf(location)}>
              {location.kind === 'library' ? `${s.library} · ${location.root}` : `${CLIENT_LABEL[location.kind]} · ${location.root}`}
            </option>
          ))}
        </select>
      </label>
      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{s.createName}</span>
        <input autoFocus value={name} onChange={(e) => setName(e.target.value.trim())} placeholder="my-skill" className={`${inputClass} font-mono`} />
        <span className={`text-[11px] ${name && !nameValid ? 'text-[#d70015] dark:text-[#ff453a]' : 'text-[#86868b]'}`}>
          {name && !nameValid ? s.invalidName : s.createNameHint}
        </span>
      </label>
      <label className="flex flex-col gap-1">
        <span className="font-semibold text-[#86868b]">{s.createDescription}</span>
        <textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={4} className={`${inputClass} resize-none`} />
        <span className="text-[11px] text-[#86868b]">{s.createDescriptionHint}</span>
      </label>
      {isLibrary && (
        <div className="flex items-center gap-3">
          <span className="font-semibold text-[#86868b]">{s.createMount}</span>
          {CLIENTS.map((client) => (
            <label key={client} className="flex items-center gap-1.5 cursor-pointer">
              <input
                type="checkbox"
                checked={mountTo.includes(client)}
                onChange={(e) => setMountTo((prev) => (e.target.checked ? [...prev, client] : prev.filter((c) => c !== client)))}
              />
              {CLIENT_LABEL[client]}
            </label>
          ))}
        </div>
      )}
      <div className="flex items-center justify-end gap-2 pt-1">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          {t.common.cancel}
        </Button>
        <Button size="sm" variant="primary" type="submit" disabled={busy || !nameValid || !description.trim()}>
          {busy ? s.creating : s.create}
        </Button>
      </div>
    </form>
  );
};
