import type { AccountSortDirection, AccountSortField, AccountViewMode } from '../types/accounts';

export interface AccountSortConfig {
  field: AccountSortField;
  direction: AccountSortDirection;
}

const STORAGE_KEYS = {
  privacy: 'ai-session-tools.accounts.privacy',
  viewMode: 'ai-session-tools.accounts.viewMode',
  sort: 'ai-session-tools.accounts.sort',
  autoRefreshMinutes: 'ai-session-tools.accounts.autoRefreshMinutes',
} as const;

const VALID_VIEW_MODES: readonly AccountViewMode[] = ['grid', 'list', 'compact'];
const VALID_SORT_FIELDS: readonly AccountSortField[] = ['added', 'last_used', 'remaining', 'reset', 'name'];
const VALID_REFRESH_INTERVALS = [0, 5, 10, 15, 30] as const;

/** Masks an email address while keeping its provider domain recognizable, e.g. `us***@gmail.com`. */
export function maskEmail(email?: string | null): string {
  const trimmed = (email ?? '').trim();
  if (!trimmed) return '';
  const atIndex = trimmed.indexOf('@');
  if (atIndex <= 0) {
    return maskName(trimmed);
  }
  const local = Array.from(trimmed.slice(0, atIndex));
  const domain = trimmed.slice(atIndex + 1);
  if (!domain) return maskName(trimmed);
  const prefix = local.length <= 2 ? local[0] : local.slice(0, 2).join('');
  return `${prefix}***@${domain}`;
}

/** Masks an account display name or identifier; delegates email-like strings to `maskEmail`. */
export function maskName(name?: string | null): string {
  const trimmed = (name ?? '').trim();
  if (!trimmed) return '';
  if (trimmed.includes('@')) {
    return maskEmail(trimmed);
  }
  const chars = Array.from(trimmed);
  if (chars.length <= 1) return '*';
  if (chars.length <= 3) return `${chars[0]}***`;
  return `${chars.slice(0, 2).join('')}***`;
}

function maskHost(host: string): string {
  if (!host || host === 'localhost') return host;
  const parts = host.split('.');
  if (parts.length === 1) return maskName(parts[0]);
  const last = parts[parts.length - 1];
  const head = Array.from(parts[0]);
  const headMasked = head.length <= 2 ? `${head[0] ?? ''}***` : `${head.slice(0, 2).join('')}***`;
  if (parts.length === 2) return `${headMasked}.${last}`;
  return `${headMasked}.***.${last}`;
}

/** Masks URL hostnames and inline API token fragments inside provider detail text. */
export function maskDetailUrl(detail?: string | null): string {
  const trimmed = (detail ?? '').trim();
  if (!trimmed) return '';
  const maskedUrls = trimmed.replace(/https?:\/\/[^\s·]+/gi, (raw) => {
    try {
      const parsed = new URL(raw);
      const port = parsed.port ? `:${parsed.port}` : '';
      const path = parsed.pathname && parsed.pathname !== '/' ? '/…' : '';
      return `${parsed.protocol}//${maskHost(parsed.hostname)}${port}${path}`;
    } catch {
      return maskName(raw);
    }
  });
  return maskedUrls.replace(/\bsk-[A-Za-z0-9_-]{4,}/g, 'sk-***');
}

function getStorage(): Storage | null {
  try {
    if (typeof window === 'undefined' || !window.localStorage) return null;
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadPrivacyMode(): boolean {
  return getStorage()?.getItem(STORAGE_KEYS.privacy) === 'true';
}

export function savePrivacyMode(enabled: boolean): void {
  try {
    getStorage()?.setItem(STORAGE_KEYS.privacy, enabled ? 'true' : 'false');
  } catch {
    // Ignore storage quota or private-mode failures.
  }
}

export function loadViewMode(): AccountViewMode {
  const raw = getStorage()?.getItem(STORAGE_KEYS.viewMode);
  return VALID_VIEW_MODES.includes(raw as AccountViewMode) ? (raw as AccountViewMode) : 'grid';
}

export function saveViewMode(mode: AccountViewMode): void {
  try {
    getStorage()?.setItem(STORAGE_KEYS.viewMode, mode);
  } catch {
    // Ignore storage quota or private-mode failures.
  }
}

export function loadSortConfig(): AccountSortConfig {
  const fallback: AccountSortConfig = { field: 'added', direction: 'desc' };
  const raw = getStorage()?.getItem(STORAGE_KEYS.sort);
  if (!raw) return fallback;
  try {
    const parsed = JSON.parse(raw) as Partial<AccountSortConfig>;
    const field = VALID_SORT_FIELDS.includes(parsed.field as AccountSortField)
      ? (parsed.field as AccountSortField)
      : fallback.field;
    const direction = parsed.direction === 'asc' || parsed.direction === 'desc' ? parsed.direction : fallback.direction;
    return { field, direction };
  } catch {
    return fallback;
  }
}

export function saveSortConfig(config: AccountSortConfig): void {
  try {
    getStorage()?.setItem(STORAGE_KEYS.sort, JSON.stringify(config));
  } catch {
    // Ignore storage quota or private-mode failures.
  }
}

export function loadAutoRefreshMinutes(): number {
  const raw = Number(getStorage()?.getItem(STORAGE_KEYS.autoRefreshMinutes) ?? '0');
  return (VALID_REFRESH_INTERVALS as readonly number[]).includes(raw) ? raw : 0;
}

export function saveAutoRefreshMinutes(minutes: number): void {
  try {
    getStorage()?.setItem(STORAGE_KEYS.autoRefreshMinutes, String(minutes));
  } catch {
    // Ignore storage quota or private-mode failures.
  }
}
