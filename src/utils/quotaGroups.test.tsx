import { expect, test } from 'vitest';
import { groupModelQuotas } from './quotaGroups';

const model = (label: string, used: number, resets_at?: string) => ({ kind: label.toLowerCase(), label, used_percent: used, resets_at });

test('models are grouped by family, and the scarcest model decides the line', () => {
  const groups = groupModelQuotas([
    model('Gemini 3.5 Flash (High)', 0, '2026-10-25T18:00:00Z'),
    model('Claude Sonnet 5.5 (Low)', 0, '2026-10-25T18:00:00Z'),
    model('Gemini 3.1 Flash Image', 10),
    model('Claude Opus 5.5 (High)', 60, '2026-10-25T16:00:00Z'),
    model('GPT-OSS 120B (Medium)', 0),
    model('Gemini 2.5 Pro', 25),
    model('Gemini 3 Flash', 0),
    model('Some New Model', 50),
  ]);
  expect(groups.map((g) => [g.name, g.remaining, g.models.length])).toEqual([
    ['Claude', 40, 2],
    ['Gemini Pro', 75, 1],
    ['Gemini Flash', 100, 2],
    ['Gemini Image', 90, 1],
    ['GPT-OSS', 100, 1],
    ['Some New Model', 50, 1],
  ]);
  expect(groups[0].resetsAt).toBe('2026-10-25T16:00:00Z');
  expect(groups[0].models.map((m) => m.name)).toEqual(['Opus 5.5 (High)', 'Sonnet 5.5 (Low)']);
  expect(groups[2].models.map((m) => m.name).sort()).toEqual(['3 Flash', '3.5 Flash (High)']);
  expect(groups[4].models[0].name).toBe('GPT-OSS 120B (Medium)');
});
