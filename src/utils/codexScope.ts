import { CodexPlacement, CodexProject } from '../types';

export interface ItemGroup<T> {
  /** Unique across all sections. */
  key: string;
  /** Folder shown on hover and opened by the folder buttons; absent for chats. */
  path?: string;
  /** Display name; derived from `path` when absent. */
  name?: string;
  items: T[];
}

/** Groups items by folder, with items that have none last. */
export function groupByFolder<T>(
  items: T[],
  folderOf: (item: T) => string | null | undefined,
  uncategorized: string
): ItemGroup<T>[] {
  const map = new Map<string, T[]>();
  for (const item of items) {
    const folder = folderOf(item) || uncategorized;
    const list = map.get(folder);
    if (list) list.push(item);
    else map.set(folder, [item]);
  }
  return Array.from(map.entries())
    .sort((a, b) => {
      if (a[0] === uncategorized) return 1;
      if (b[0] === uncategorized) return -1;
      return a[0].localeCompare(b[0]);
    })
    .map(([folder, list]) => ({
      key: `folder:${folder}`,
      path: folder === uncategorized ? undefined : folder,
      name: folder === uncategorized ? uncategorized : undefined,
      items: list,
    }));
}

export interface CodexSections<T> {
  /** The Codex sidebar's projects that hold items, in sidebar order. */
  projects: ItemGroup<T>[];
  /** Chats that belong to no project. */
  chats: T[];
  /** Items the Codex sidebar does not list, by folder. */
  hidden: ItemGroup<T>[];
  hiddenCount: number;
}

/** Splits items the way the Codex sidebar lists them. */
export function splitByCodexScope<T>(
  items: T[],
  placementOf: (item: T) => CodexPlacement | null | undefined,
  projects: CodexProject[],
  folderOf: (item: T) => string | null | undefined,
  uncategorized: string
): CodexSections<T> {
  const byProject = new Map<string, T[]>(projects.map((p) => [p.project_id, []]));
  const chats: T[] = [];
  const hidden: T[] = [];
  for (const item of items) {
    const placement = placementOf(item);
    const projectItems =
      placement?.scope === 'project' && placement.project_id
        ? byProject.get(placement.project_id)
        : undefined;
    if (projectItems) projectItems.push(item);
    else if (placement?.scope === 'chat') chats.push(item);
    else hidden.push(item);
  }
  return {
    projects: projects
      .filter((p) => byProject.get(p.project_id)!.length > 0)
      .map((p) => ({
        key: `project:${p.project_id}`,
        path: p.root_paths[0],
        name: p.name,
        items: byProject.get(p.project_id)!,
      })),
    chats,
    hidden: groupByFolder(hidden, folderOf, uncategorized),
    hiddenCount: hidden.length,
  };
}
