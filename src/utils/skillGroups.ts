import { SkillClient, SkillEntry, SkillLocation } from '../types';

export const SKILL_CLIENTS: SkillClient[] = ['claude', 'codex', 'antigravity'];

/** Windows paths compare case-insensitively and with either separator. */
export const pathKey = (path: string) => path.replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();
export const baseName = (path: string) => path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path;
const parentPath = (path: string) => path.replace(/[\\/]+$/, '').replace(/[\\/][^\\/]*$/, '');

/** Where a skill's files live. */
export type SkillOrigin =
  | { kind: SkillClient }
  | { kind: 'library'; root: string }
  /** Outside the managed folders, named after the folder that holds the skills, such as cc-switch. */
  | { kind: 'external'; folder: string };

export interface SkillSource {
  path: string;
  origin: SkillOrigin;
}

/** One skill as the user sees it, by folder name: each client's copy and the files behind them. */
export interface SkillGroup {
  /** Lower-case folder name; clients load a skill by its folder name. */
  key: string;
  name: string;
  title?: string | null;
  description?: string | null;
  /** What each client has: an enabled or disabled folder or link. */
  clients: Partial<Record<SkillClient, SkillEntry>>;
  /** The real folder to link new copies to: a library's first, otherwise one in a client folder. */
  original?: SkillEntry;
  /** Every distinct folder the copies stand for; more than one means the clients use different files. */
  sources: SkillSource[];
  /** Every copy is a client's built-in skill, so nothing about it can change. */
  system: boolean;
  linkBroken: boolean;
  hasSkillFile: boolean;
  ccSwitch: boolean;
}

/** Names a folder outside the managed locations: `…\cc-switch\skills\x` reads as cc-switch. */
function originOf(path: string, locations: SkillLocation[]): SkillOrigin {
  const key = pathKey(path);
  const under = (root?: string | null) => Boolean(root) && key.startsWith(`${pathKey(root!)}\\`);
  for (const location of locations) {
    if (location.kind === 'library') {
      if (under(location.root)) return { kind: 'library', root: location.root };
    } else if (under(location.root) || under(location.disabled_root)) {
      return { kind: location.kind };
    }
  }
  const parent = parentPath(path);
  const folder = baseName(parent);
  return { kind: 'external', folder: folder.toLowerCase() === 'skills' ? baseName(parentPath(parent)) || folder : folder };
}

/**
 * Merges every location's skills into one row per folder name, the way cc-switch lists
 * each skill once with a switch per client.
 */
export function groupSkills(locations: SkillLocation[]): SkillGroup[] {
  const groups = new Map<string, SkillGroup>();
  for (const location of locations) {
    for (const skill of location.skills) {
      // A link inside a library only points at a skill kept somewhere else.
      if (location.kind === 'library' && skill.link_target) continue;
      const key = skill.folder_name.toLowerCase();
      let group = groups.get(key);
      if (!group) {
        group = { key, name: skill.folder_name, clients: {}, sources: [], system: true, linkBroken: false, hasSkillFile: false, ccSwitch: false };
        groups.set(key, group);
      }
      if (location.kind !== 'library') {
        const existing = group.clients[location.kind];
        // A client may hold the name twice, enabled and in skills-disabled; the loaded one counts.
        if (!existing || (!existing.enabled && skill.enabled)) group.clients[location.kind] = skill;
      }
      const sourcePath = skill.link_target || skill.path;
      const known = group.sources.find((s) => pathKey(s.path) === pathKey(sourcePath));
      if (!known) {
        group.sources.push({ path: sourcePath, origin: originOf(sourcePath, locations) });
      } else if (!skill.link_target) {
        // A link may spell its target differently; show the folder's own path.
        known.path = skill.path;
      }
      if (!skill.link_target && (location.kind === 'library' || !group.original)) {
        group.original = skill;
      }
      group.title ??= skill.name;
      group.description ??= skill.description;
      group.system &&= skill.system;
      group.linkBroken ||= skill.link_broken;
      group.hasSkillFile ||= skill.has_skill_file;
      group.ccSwitch ||= skill.cc_switch != null;
    }
  }
  for (const group of groups.values()) {
    // The original's folder comes first, then what the clients link to.
    const first = group.original && pathKey(group.original.path);
    group.sources.sort((a, b) => Number(pathKey(b.path) === first) - Number(pathKey(a.path) === first));
  }
  return [...groups.values()].sort(
    (a, b) => Number(a.system) - Number(b.system) || a.name.toLowerCase().localeCompare(b.name.toLowerCase())
  );
}

/** Whether a client loads the skill now. */
export const isOnIn = (group: SkillGroup, client: SkillClient) => Boolean(group.clients[client]?.enabled);

/** Another client's copy to link from: a built-in or broken one cannot be linked. */
export const linkSource = (group: SkillGroup, client: SkillClient) =>
  SKILL_CLIENTS.filter((c) => c !== client)
    .map((c) => group.clients[c])
    .find((e) => e && !e.system && !e.link_broken);

/** Whether the switch for a client can change: built-in and broken skills stay as they are. */
export function canToggle(group: SkillGroup, client: SkillClient): boolean {
  if (group.system || group.linkBroken) return false;
  const entry = group.clients[client];
  if (entry) return !entry.system;
  // Turning on links the files in; they need a SKILL.md and somewhere to link from.
  const library = group.original && group.sources[0]?.origin.kind === 'library';
  return group.hasSkillFile && Boolean(library || linkSource(group, client));
}

/** What deleting a skill removes: every link first, then every real folder, which goes to the Recycle Bin. */
export function deletePlan(group: SkillGroup): { links: SkillEntry[]; recycle: SkillEntry[] } {
  // Built-in skills belong to their client and are never touched.
  const entries = SKILL_CLIENTS.map((c) => group.clients[c]).filter((e): e is SkillEntry => Boolean(e) && !e!.system);
  const links = entries.filter((e) => e.link_target);
  const recycle = entries.filter((e) => !e.link_target);
  if (group.original && !group.original.system && !recycle.includes(group.original)) recycle.push(group.original);
  return { links, recycle };
}
