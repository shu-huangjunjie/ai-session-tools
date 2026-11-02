export type FolderOrders = Record<string, string[]>;

export const FOLDER_ORDER_STORAGE_KEY = 'cv_folder_order_v1';

export function readFolderOrders(): FolderOrders {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(FOLDER_ORDER_STORAGE_KEY) ?? '{}');
    if (!saved || typeof saved !== 'object' || Array.isArray(saved)) return {};
    return Object.fromEntries(Object.entries(saved).filter(([, keys]) =>
      Array.isArray(keys) && keys.every((key) => typeof key === 'string')
    ).map(([scope, keys]) => [scope, [...new Set(keys as string[])]]));
  } catch {
    return {};
  }
}

/** Keep the default order until customized; new folders follow the saved ones. */
export function sortFolders<T extends { key: string }>(groups: T[], order: string[] = []): T[] {
  const positions = new Map(order.map((key, index) => [key, index]));
  return [...groups].sort((a, b) =>
    (positions.get(a.key) ?? order.length) - (positions.get(b.key) ?? order.length)
  );
}

/** Preserve folders absent from search results instead of overwriting their order. */
export function moveFolder(
  saved: string[], visible: string[], from: string, to: string, edge: 'before' | 'after'
): string[] {
  const keys = [...new Set([...saved, ...visible])];
  if (from === to || !visible.includes(from) || !visible.includes(to)) return keys;
  const remaining = keys.filter((key) => key !== from);
  remaining.splice(remaining.indexOf(to) + (edge === 'after' ? 1 : 0), 0, from);
  return remaining;
}
