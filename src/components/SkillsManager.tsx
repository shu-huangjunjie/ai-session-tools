import React, { useCallback, useEffect, useMemo, useState } from 'react';
import {
  AlertTriangle,
  Code2,
  FolderOpen,
  FolderPlus,
  Link2,
  Lock,
  Plus,
  Power,
  RefreshCw,
  Search,
  Trash2,
  Unlink,
} from 'lucide-react';
import { api, errorMessage } from '../api/client';
import { SkillClient, SkillEntry, SkillLocation } from '../types';
import { formatDateTime } from '../utils/formatters';
import { Button } from './common/Button';
import { Modal } from './common/Modal';
import { useI18n } from '../i18n';

type ToastType = 'success' | 'warning' | 'error' | 'info';

interface SkillsManagerProps {
  isOpen: boolean;
  onClose: () => void;
  onToast: (type: ToastType, message: string) => void;
}

const LIBRARIES_KEY = 'cv_skill_libraries';
const CLIENTS: SkillClient[] = ['claude', 'codex'];
const CLIENT_LABEL: Record<SkillClient, string> = { claude: 'Claude', codex: 'Codex' };
const NAME_PATTERN = /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/;

/** Windows paths compare case-insensitively and with either separator. */
export const pathKey = (path: string) => path.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
const baseName = (path: string) => path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path;
const tabKey = (location: SkillLocation) =>
  location.kind === 'library' ? `library:${pathKey(location.root)}` : location.kind;

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

const Badge: React.FC<{ tone: 'gray' | 'blue' | 'red' | 'orange' | 'purple' | 'green'; children: React.ReactNode; title?: string }> = ({
  tone,
  children,
  title,
}) => {
  const tones = {
    gray: 'bg-black/[0.05] dark:bg-white/[0.08] text-[#6e6e73] dark:text-[#a1a1a6]',
    blue: 'bg-[#0071e3]/10 text-[#0071e3] dark:text-[#2997ff]',
    red: 'bg-[#ff3b30]/10 text-[#d70015] dark:text-[#ff453a]',
    orange: 'bg-[#ff9f0a]/15 text-[#c93400] dark:text-[#ff9f0a]',
    purple: 'bg-[#af52de]/10 text-[#af52de] dark:text-[#bf5af2]',
    green: 'bg-[#34c759]/12 text-[#248a3d] dark:text-[#30d158]',
  }[tone];
  return (
    <span title={title} className={`inline-flex items-center gap-1 text-[10px] font-medium px-1.5 py-0.5 rounded-full ${tones}`}>
      {children}
    </span>
  );
};

export const SkillsManager: React.FC<SkillsManagerProps> = ({ isOpen, onClose, onToast }) => {
  const { t, interpolate } = useI18n();
  const s = t.skills;
  const [libraries, setLibraries] = useState<string[]>(loadLibraries);
  const [locations, setLocations] = useState<SkillLocation[]>([]);
  const [activeTab, setActiveTab] = useState<string>('claude');
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
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
    if (isOpen) {
      setQuery('');
      setConfirmDelete(false);
      setCreating(false);
      void refresh();
    }
    // Reload only when the dialog opens; actions refresh explicitly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isOpen]);

  const saveLibraries = (next: string[]) => {
    setLibraries(next);
    try {
      localStorage.setItem(LIBRARIES_KEY, JSON.stringify(next));
    } catch {
      // Storage may be unavailable; the list still works for this session.
    }
  };

  const activeLocation = locations.find((l) => tabKey(l) === activeTab) ?? locations[0];
  const allSkills = useMemo(() => locations.flatMap((l) => l.skills), [locations]);
  const selected = allSkills.find((sk) => selectedPath !== null && pathKey(sk.path) === pathKey(selectedPath)) ?? null;
  const selectedLocation = selected ? locations.find((l) => l.skills.includes(selected)) : undefined;

  const visibleSkills = useMemo(() => {
    const q = query.trim().toLowerCase();
    const skills = activeLocation?.skills ?? [];
    if (!q) return skills;
    return skills.filter((sk) =>
      [sk.folder_name, sk.name, sk.description].some((v) => v?.toLowerCase().includes(q))
    );
  }, [activeLocation, query]);

  // A library skill's link inside a client skills folder (enabled or disabled).
  const clientLink = useCallback(
    (skill: SkillEntry, client: SkillClient) =>
      locations
        .find((l) => l.kind === client)
        ?.skills.find((sk) => sk.link_target && pathKey(sk.link_target) === pathKey(skill.path)),
    [locations]
  );

  useEffect(() => {
    setConfirmDelete(false);
    if (!selected || !selected.has_skill_file || selected.link_broken) {
      setPreview(null);
      return;
    }
    let cancelled = false;
    setPreview({ path: selected.path });
    api
      .readSkillFile(libraries, selected.path)
      .then((text) => !cancelled && setPreview({ path: selected.path, text }))
      .catch((err) => !cancelled && setPreview({ path: selected.path, error: errorMessage(err) }));
    return () => {
      cancelled = true;
    };
    // Re-read only when another skill is selected.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected?.path]);

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
      } else {
        const next = [...libraries, picked];
        saveLibraries(next);
        await refresh(next);
        onToast('success', interpolate(s.toastLibraryAdded, { path: picked }));
      }
      setActiveTab(`library:${pathKey(picked)}`);
    });

  const removeLibrary = (root: string) => {
    const next = libraries.filter((l) => pathKey(l) !== pathKey(root));
    saveLibraries(next);
    setActiveTab('claude');
    void refresh(next);
  };

  const toggleEnabled = (skill: SkillEntry) =>
    run(async () => {
      const path = await api.setSkillEnabled(libraries, skill.path, !skill.enabled);
      onToast('success', interpolate(skill.enabled ? s.toastDisabled : s.toastEnabled, { name: skill.folder_name }));
      setSelectedPath(path);
      await refresh();
    });

  const toggleMount = (skill: SkillEntry, client: SkillClient) =>
    run(async () => {
      const link = clientLink(skill, client);
      if (link) {
        await api.deleteSkill(libraries, link.path);
        onToast('success', interpolate(s.toastUnmounted, { name: skill.folder_name, client: CLIENT_LABEL[client] }));
      } else {
        await api.mountSkill(libraries, skill.path, client);
        onToast('success', interpolate(s.toastMounted, { name: skill.folder_name, client: CLIENT_LABEL[client] }));
      }
      await refresh();
    });

  const deleteSelected = (skill: SkillEntry) =>
    run(async () => {
      const outcome = await api.deleteSkill(libraries, skill.path);
      onToast('success', interpolate(outcome === 'link_removed' ? s.toastLinkRemoved : s.toastRecycled, { name: skill.folder_name }));
      setSelectedPath(null);
      await refresh();
    });

  const tabLabel = (location: SkillLocation) =>
    location.kind === 'library' ? `${s.library} · ${baseName(location.root)}` : CLIENT_LABEL[location.kind];

  return (
    <Modal isOpen={isOpen} onClose={onClose} title={s.title} subtitle={s.subtitle} widthClass="max-w-6xl">
      <div className="flex flex-col gap-3 h-[68vh] text-xs select-text">
        {/* Locations and toolbar */}
        <div className="flex items-center gap-2 flex-wrap">
          <div className="flex items-center bg-black/[0.04] dark:bg-white/[0.06] p-1 rounded-full border border-black/[0.08] dark:border-white/[0.1] gap-1 flex-wrap">
            {locations.map((location) => {
              const key = tabKey(location);
              const active = key === tabKey(activeLocation ?? location);
              return (
                <button
                  key={key}
                  onClick={() => {
                    setActiveTab(key);
                    setCreating(false);
                  }}
                  title={location.root}
                  className={`flex items-center gap-1.5 px-3 py-1 rounded-full transition-all ${
                    active
                      ? 'bg-white dark:bg-[#1c1c1e] text-[#0071e3] dark:text-[#2997ff] shadow-sm font-semibold'
                      : 'text-[#86868b] hover:text-[#1d1d1f] dark:hover:text-white'
                  }`}
                >
                  <span className="max-w-[180px] truncate">{tabLabel(location)}</span>
                  <span className="text-[10px] font-mono opacity-70">{location.skills.length}</span>
                </button>
              );
            })}
          </div>
          <div className="flex-1" />
          <div className="relative">
            <Search size={12} className="absolute left-3 top-1/2 -translate-y-1/2 text-[#86868b]" />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={s.search}
              className="w-56 bg-black/[0.04] dark:bg-white/[0.06] pl-8 pr-3 py-1.5 rounded-full border border-black/[0.08] dark:border-white/[0.08] focus:border-[#0071e3] focus:outline-none placeholder-[#86868b]"
            />
          </div>
          <Button size="sm" variant="ghost" onClick={() => refresh()} disabled={loading} icon={<RefreshCw size={13} className={loading ? 'animate-spin' : ''} />}>
            {s.refresh}
          </Button>
          <Button size="sm" variant="secondary" onClick={addLibrary} disabled={busy} title={s.addLibraryTooltip} icon={<FolderPlus size={13} />}>
            {s.addLibrary}
          </Button>
          <Button
            size="sm"
            variant="primary"
            onClick={() => {
              setCreating(true);
              setSelectedPath(null);
            }}
            icon={<Plus size={13} />}
          >
            {s.newSkill}
          </Button>
        </div>

        {activeLocation && (
          <div className="flex items-center gap-2 text-[11px] text-[#86868b]">
            <span className="font-mono truncate" title={activeLocation.root}>{activeLocation.root}</span>
            {!activeLocation.exists && (
              <Badge tone="orange">
                <AlertTriangle size={10} />
                {interpolate(s.folderMissing, { path: '' }).replace(/[:：]\s*$/, '')}
              </Badge>
            )}
            {activeLocation.kind === 'library' && (
              <Button size="sm" variant="link" onClick={() => removeLibrary(activeLocation.root)} title={s.removeLibraryTooltip}>
                {s.removeLibrary}
              </Button>
            )}
            <span className="ml-auto">{s.sessionHint}</span>
          </div>
        )}

        <div className="flex gap-3 flex-1 min-h-0">
          {/* Skill list */}
          <div className="w-[42%] overflow-y-auto pr-1 space-y-1.5" role="list">
            {visibleSkills.length === 0 && (
              <div className="text-center text-[#86868b] py-10">{query ? s.noMatch : s.empty}</div>
            )}
            {visibleSkills.map((skill) => {
              const active = selected?.path === skill.path;
              return (
                <button
                  key={skill.path}
                  role="listitem"
                  onClick={() => {
                    setSelectedPath(skill.path);
                    setCreating(false);
                  }}
                  className={`w-full text-left p-3 rounded-2xl border transition-all ${
                    active
                      ? 'border-[#0071e3]/50 bg-[#0071e3]/[0.06] dark:bg-[#2997ff]/10'
                      : 'border-black/[0.06] dark:border-white/[0.08] hover:bg-black/[0.03] dark:hover:bg-white/[0.04]'
                  } ${skill.enabled ? '' : 'opacity-60'}`}
                >
                  <div className="flex items-center gap-1.5 flex-wrap">
                    <span className="font-semibold text-[#1d1d1f] dark:text-white">{skill.folder_name}</span>
                    {skill.system && <Badge tone="purple"><Lock size={9} />{s.system}</Badge>}
                    {!skill.enabled && <Badge tone="gray">{s.disabled}</Badge>}
                    {skill.link_target && !skill.link_broken && <Badge tone="blue" title={skill.link_target}><Link2 size={9} />{s.link}</Badge>}
                    {skill.link_broken && <Badge tone="red">{s.linkBroken}</Badge>}
                    {!skill.has_skill_file && !skill.link_broken && <Badge tone="orange">{s.noSkillFile}</Badge>}
                    {skill.mounted_in.length > 0 && (
                      <Badge tone="green">{interpolate(s.mountedIn, { clients: skill.mounted_in.map((c) => CLIENT_LABEL[c]).join(' · ') })}</Badge>
                    )}
                  </div>
                  {skill.description && (
                    <p className="mt-1 text-[11px] text-[#6e6e73] dark:text-[#a1a1a6] leading-relaxed line-clamp-2">{skill.description}</p>
                  )}
                </button>
              );
            })}
          </div>

          {/* Detail or create form */}
          <div className="flex-1 min-w-0 overflow-y-auto rounded-2xl border border-black/[0.06] dark:border-white/[0.08] bg-black/[0.015] dark:bg-white/[0.02] p-4">
            {creating ? (
              <CreateSkillForm
                locations={locations}
                defaultTarget={activeLocation}
                busy={busy}
                onCancel={() => setCreating(false)}
                onCreate={(target, name, description, mountTo) =>
                  run(async () => {
                    const path = await api.createSkill(libraries, target, name, description, mountTo);
                    onToast('success', interpolate(s.toastCreated, { name }));
                    await refresh();
                    setCreating(false);
                    setActiveTab(target === 'claude' || target === 'codex' ? target : `library:${pathKey(target)}`);
                    setSelectedPath(path);
                  })
                }
              />
            ) : !selected ? (
              <div className="h-full flex items-center justify-center text-[#86868b]">{s.selectHint}</div>
            ) : (
              <div className="flex flex-col gap-3">
                <div>
                  <h4 className="text-sm font-semibold text-[#1d1d1f] dark:text-white">{selected.name || selected.folder_name}</h4>
                  {selected.description && (
                    <p className="mt-1 text-[#6e6e73] dark:text-[#a1a1a6] leading-relaxed whitespace-pre-wrap">{selected.description}</p>
                  )}
                </div>
                <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[11px]">
                  <dt className="text-[#86868b]">{s.path}</dt>
                  <dd className="font-mono break-all">{selected.path}</dd>
                  {selected.link_target && (
                    <>
                      <dt className="text-[#86868b]">{s.target}</dt>
                      <dd className={`font-mono break-all ${selected.link_broken ? 'text-[#d70015] dark:text-[#ff453a]' : ''}`}>{selected.link_target}</dd>
                    </>
                  )}
                  <dt />
                  <dd className="text-[#86868b]">
                    {interpolate(s.files, { count: selected.file_count, size: formatBytes(selected.total_bytes) })}
                    {selected.modified_at && ` · ${interpolate(s.modified, { time: formatDateTime(selected.modified_at) })}`}
                  </dd>
                </dl>

                <div className="flex items-center gap-2 flex-wrap">
                  <Button size="sm" variant="secondary" icon={<FolderOpen size={13} />} onClick={() => run(() => api.openFolder(selected.path))}>
                    {s.openFolder}
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    icon={<Code2 size={13} />}
                    disabled={selected.link_broken}
                    onClick={() => run(async () => void (await api.openVSCode(selected.link_target || selected.path)))}
                  >
                    {s.openVSCode}
                  </Button>
                  {selectedLocation?.kind !== 'library' && !selected.system && (
                    <Button
                      size="sm"
                      variant="secondary"
                      icon={<Power size={13} />}
                      disabled={busy}
                      title={selected.enabled ? s.disableTooltip : s.enableTooltip}
                      onClick={() => toggleEnabled(selected)}
                    >
                      {selected.enabled ? s.disable : s.enable}
                    </Button>
                  )}
                  {selectedLocation?.kind === 'library' &&
                    CLIENTS.map((client) => {
                      const mounted = Boolean(clientLink(selected, client));
                      return (
                        <Button
                          key={client}
                          size="sm"
                          variant={mounted ? 'secondary' : 'primary'}
                          icon={mounted ? <Unlink size={13} /> : <Link2 size={13} />}
                          disabled={busy || (!mounted && !selected.has_skill_file)}
                          title={s.mountHint}
                          onClick={() => toggleMount(selected, client)}
                        >
                          {interpolate(mounted ? s.unmountFrom : s.mountTo, { client: CLIENT_LABEL[client] })}
                        </Button>
                      );
                    })}
                  {!selected.system && (
                    <Button
                      size="sm"
                      variant="danger"
                      icon={<Trash2 size={13} />}
                      disabled={busy || selected.mounted_in.length > 0}
                      title={selected.mounted_in.length > 0 ? s.deleteMountedBlocked : undefined}
                      onClick={() => setConfirmDelete(true)}
                    >
                      {s.delete}
                    </Button>
                  )}
                </div>

                {confirmDelete && (
                  <div className="p-3 rounded-2xl border border-[#ff453a]/25 bg-[#ff453a]/[0.06] flex items-center gap-3">
                    <p className="flex-1 leading-relaxed">{selected.link_target ? s.deleteLinkConfirm : s.deleteFolderConfirm}</p>
                    <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(false)}>
                      {t.common.cancel}
                    </Button>
                    <Button size="sm" variant="danger" disabled={busy} onClick={() => deleteSelected(selected)}>
                      {s.confirmDelete}
                    </Button>
                  </div>
                )}

                {preview?.path === selected.path && (
                  <div>
                    <div className="text-[11px] font-semibold text-[#86868b] mb-1.5">{s.preview}</div>
                    {preview.error ? (
                      <p className="text-[#d70015] dark:text-[#ff453a]">{preview.error}</p>
                    ) : (
                      <pre className="whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed p-3 rounded-xl bg-black/[0.04] dark:bg-black/40 border border-black/[0.06] dark:border-white/[0.06] max-h-[40vh] overflow-y-auto">
                        {preview.text ?? s.previewLoading}
                      </pre>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </Modal>
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
  const isLibrary = target !== 'claude' && target !== 'codex';
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
