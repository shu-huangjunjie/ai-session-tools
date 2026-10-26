import { QuotaWindow } from '../types';

/** One model's share of a group, by remaining quota. */
export interface ModelQuota {
  /** The model name without the family, such as "Opus 5.5 (High)". */
  name: string;
  remaining: number;
  resetsAt?: string | null;
}

/** A model family, such as Claude or Gemini Flash, shown as one quota line. */
export interface QuotaGroup {
  key: string;
  name: string;
  /** The family colour, for the dot before its name. */
  color: string;
  /** The lowest remaining percent: the model that runs out first decides. */
  remaining: number;
  /** When that model's quota comes back. */
  resetsAt?: string | null;
  models: ModelQuota[];
}

/** Antigravity's model families, in display order; the prefix is dropped from model names inside a group. */
const FAMILIES: { key: string; name: string; color: string; test: RegExp; prefix: RegExp }[] = [
  { key: 'claude', name: 'Claude', color: '#d97757', test: /^claude\b/i, prefix: /^claude\s+/i },
  { key: 'gemini-pro', name: 'Gemini Pro', color: '#4285f4', test: /^gemini\b(?!.*image).*\bpro\b/i, prefix: /^gemini\s+/i },
  { key: 'gemini-flash', name: 'Gemini Flash', color: '#1ea362', test: /^gemini\b(?!.*image).*\bflash\b/i, prefix: /^gemini\s+/i },
  { key: 'gemini-image', name: 'Gemini Image', color: '#a142f4', test: /^gemini\b.*\bimage\b/i, prefix: /^gemini\s+/i },
  { key: 'gpt-oss', name: 'GPT-OSS', color: '#10a37f', test: /^gpt-oss\b/i, prefix: /^$/ },
];
const OTHER_COLOR = '#8e8e93';

const byName = (a: ModelQuota, b: ModelQuota) => a.name.localeCompare(b.name, undefined, { numeric: true });

/** Groups per-model windows into families; a model outside the known families gets a group of its own. */
export function groupModelQuotas(windows: QuotaWindow[]): QuotaGroup[] {
  const groups = new Map<string, QuotaGroup>();
  for (const w of windows) {
    const label = (w.label || w.kind).trim();
    const family = FAMILIES.find((f) => f.test.test(label));
    const key = family?.key ?? `model:${label.toLowerCase()}`;
    let group = groups.get(key);
    if (!group) {
      group = { key, name: family?.name ?? label, color: family?.color ?? OTHER_COLOR, remaining: 100, models: [] };
      groups.set(key, group);
    }
    const name = family ? label.replace(family.prefix, '') || label : label;
    group.models.push({ name, remaining: Math.round(Math.max(0, Math.min(100, 100 - w.used_percent))), resetsAt: w.resets_at });
  }
  const rank = (key: string) => {
    const i = FAMILIES.findIndex((f) => f.key === key);
    return i < 0 ? FAMILIES.length : i;
  };
  return [...groups.values()]
    .map((group) => {
      const models = [...group.models].sort(byName);
      // The scarcest model decides; among equals, the one that resets first.
      const lowest = [...models].sort(
        (a, b) => a.remaining - b.remaining || (a.resetsAt ?? '￿').localeCompare(b.resetsAt ?? '￿')
      )[0];
      return { ...group, models, remaining: lowest.remaining, resetsAt: lowest.resetsAt };
    })
    .sort((a, b) => rank(a.key) - rank(b.key) || a.name.localeCompare(b.name));
}

/** Bar colour by remaining percent. */
export const remainingColor = (remaining: number) =>
  remaining <= 10 ? 'bg-[#ff3b30]' : remaining <= 30 ? 'bg-[#ff9f0a]' : 'bg-[#34c759]';
