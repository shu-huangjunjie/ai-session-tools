import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { I18nProvider } from '../i18n';
import { api } from '../api/client';
import { SkillsManager } from './SkillsManager';
import { SkillEntry, SkillLocation } from '../types';
import { groupSkills } from '../utils/skillGroups';

vi.mock('../api/client', () => ({
  api: {
    listSkills: vi.fn(),
    readSkillFile: vi.fn(),
    createSkill: vi.fn(),
    setSkillEnabled: vi.fn(),
    mountSkill: vi.fn(),
    linkSkillToClient: vi.fn(),
    deleteSkill: vi.fn(),
    pickFolderPath: vi.fn(),
    openFolder: vi.fn(),
    openVSCode: vi.fn(),
  },
  errorMessage: (e: unknown) => String(e),
}));

const LIB = 'E:\\skills\\claude';
const CLAUDE = 'C:\\Users\\Demo\\.claude\\skills';
const CLAUDE_OFF = 'C:\\Users\\Demo\\.claude\\skills-disabled';
const CODEX = 'C:\\Users\\Demo\\.codex\\skills';

const skill = (dir: string, folder: string, extra: Partial<SkillEntry> = {}): SkillEntry => ({
  folder_name: folder, name: folder, description: `${folder} description`, path: `${dir}\\${folder}`,
  link_target: null, link_broken: false, has_skill_file: true, enabled: true, system: false,
  file_count: 1, total_bytes: 2048, modified_at: '2026-10-22T02:00:00Z', mounted_in: [], ...extra,
});

const locations = (): SkillLocation[] => [
  {
    kind: 'claude', root: CLAUDE, disabled_root: CLAUDE_OFF, exists: true,
    skills: [
      // Junction names may differ in case from the library path.
      skill(CLAUDE, 'word-prefs', { link_target: 'e:/skills/claude/word-prefs' }),
      skill(CLAUDE_OFF, 'old-notes', { enabled: false }),
    ],
  },
  { kind: 'codex', root: CODEX, exists: true, skills: [] },
  {
    kind: 'library', root: LIB, exists: true,
    skills: [skill(LIB, 'word-prefs', { mounted_in: ['claude'] }), skill(LIB, 'image-gen')],
  },
];

const toast = vi.fn();
const renderManager = () =>
  render(
    <I18nProvider>
      <SkillsManager onToast={toast} />
    </I18nProvider>
  );
const grid = () => within(screen.getByRole('grid'));
const switchState = (name: string) => screen.getByRole('switch', { name }).getAttribute('aria-checked');

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  localStorage.setItem('cv_locale', 'en');
  localStorage.setItem('cv_skill_libraries', JSON.stringify([LIB]));
  vi.mocked(api.listSkills).mockImplementation(async () => locations());
  vi.mocked(api.readSkillFile).mockResolvedValue('---\nname: x\n---\nbody text');
  vi.mocked(api.mountSkill).mockResolvedValue('linked');
  vi.mocked(api.linkSkillToClient).mockResolvedValue('linked');
  vi.mocked(api.deleteSkill).mockResolvedValue('link_removed');
  vi.mocked(api.createSkill).mockResolvedValue(`${LIB}\\new-skill`);
  vi.mocked(api.setSkillEnabled).mockResolvedValue('moved');
});

afterEach(cleanup);

test('a link and its original are one skill', () => {
  const groups = groupSkills(locations());
  expect(groups.map((g) => g.name)).toEqual(['image-gen', 'old-notes', 'word-prefs']);
  const prefs = groups.find((g) => g.name === 'word-prefs')!;
  expect(prefs.sources).toEqual([{ path: `${LIB}\\word-prefs`, origin: { kind: 'library', root: LIB } }]);
  expect(prefs.original?.path).toBe(`${LIB}\\word-prefs`);
  expect(prefs.clients.claude?.path).toBe(`${CLAUDE}\\word-prefs`);
  expect(prefs.clients.codex).toBeUndefined();
});

test('a skill whose clients link to different copies is still one row, naming both sources', async () => {
  const user = userEvent.setup();
  vi.mocked(api.listSkills).mockImplementation(async () => [
    { kind: 'claude', root: CLAUDE, exists: true, skills: [skill(CLAUDE, 'nature-x', { link_target: 'E:\\skills\\nature-skills\\skills\\nature-x' })] },
    { kind: 'codex', root: CODEX, exists: true, skills: [skill(CODEX, 'nature-x', { link_target: 'C:\\cfg\\cc-switch\\skills\\nature-x' })] },
  ]);
  renderManager();
  await screen.findByRole('switch', { name: 'Claude nature-x' });
  expect(grid().getAllByText('nature-x')).toHaveLength(1);
  expect(grid().getByText('nature-skills / cc-switch')).toBeTruthy();
  expect(switchState('Claude nature-x')).toBe('true');
  expect(switchState('Codex nature-x')).toBe('true');
  await user.click(grid().getByText('nature-x'));
  expect(screen.getByText(/two different copies of the files/)).toBeTruthy();
});

test('lists every skill once with a switch per client and previews SKILL.md', async () => {
  const user = userEvent.setup();
  renderManager();
  await screen.findByRole('switch', { name: 'Claude word-prefs' });
  expect(api.listSkills).toHaveBeenCalledWith([LIB]);
  expect(grid().getAllByText('word-prefs')).toHaveLength(1);
  expect(switchState('Claude word-prefs')).toBe('true');
  expect(switchState('Codex word-prefs')).toBe('false');
  expect(switchState('Claude old-notes')).toBe('false');
  expect(grid().getAllByText('Library · claude')).toHaveLength(2);

  await user.click(grid().getByText('old-notes'));
  expect(await screen.findByText(/body text/)).toBeTruthy();
  expect(api.readSkillFile).toHaveBeenCalledWith([LIB], `${CLAUDE_OFF}\\old-notes`);
});

test('switches enable, disable and link a skill per client, and the filter narrows the list', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByRole('switch', { name: 'Claude old-notes' }));
  expect(api.setSkillEnabled).toHaveBeenCalledWith([LIB], `${CLAUDE_OFF}\\old-notes`, true);
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Enabled old-notes in Claude'));

  // A library original is linked into the client that does not have it yet.
  await user.click(screen.getByRole('switch', { name: 'Codex word-prefs' }));
  expect(api.mountSkill).toHaveBeenCalledWith([LIB], `${LIB}\\word-prefs`, 'codex');

  await user.click(screen.getByRole('switch', { name: 'Claude word-prefs' }));
  expect(api.setSkillEnabled).toHaveBeenLastCalledWith([LIB], `${CLAUDE}\\word-prefs`, false);

  await user.click(screen.getByRole('tab', { name: /^Not enabled/ }));
  expect(grid().queryByText('word-prefs')).toBeNull();
  expect(grid().getByText('image-gen')).toBeTruthy();
  expect(grid().getByText('old-notes')).toBeTruthy();
});

test('creating a skill validates the name and mounts on request', async () => {
  const user = userEvent.setup();
  renderManager();
  await screen.findByRole('switch', { name: 'Claude word-prefs' });
  await user.click(screen.getByRole('button', { name: 'New skill' }));
  await user.selectOptions(screen.getByRole('combobox'), LIB);

  const create = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
  await user.type(screen.getByPlaceholderText('my-skill'), 'Bad Name');
  expect(screen.getByText(/lowercase letters, digits and hyphens only/i)).toBeTruthy();
  expect(create.disabled).toBe(true);

  await user.clear(screen.getByPlaceholderText('my-skill'));
  await user.type(screen.getByPlaceholderText('my-skill'), 'new-skill');
  await user.type(screen.getByRole('textbox', { name: /Description/ }), 'Does a thing');
  await user.click(screen.getByRole('checkbox', { name: 'Codex' }));
  await user.click(create);
  await waitFor(() =>
    expect(api.createSkill).toHaveBeenCalledWith([LIB], LIB, 'new-skill', 'Does a thing', ['codex']));
});

test('deleting a skill removes the client links before recycling the original', async () => {
  const user = userEvent.setup();
  vi.mocked(api.deleteSkill).mockResolvedValueOnce('link_removed').mockResolvedValueOnce('recycled');
  renderManager();
  await user.click(await screen.findByText('word-prefs', { selector: 'span' }));
  await user.click(screen.getByRole('button', { name: 'Delete' }));
  expect(screen.getByText(/Removes the links in Claude\. .*Recycle Bin/)).toBeTruthy();
  const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
  await user.click(deleteButtons[deleteButtons.length - 1]);
  await waitFor(() => expect(api.deleteSkill).toHaveBeenCalledTimes(2));
  expect(vi.mocked(api.deleteSkill).mock.calls.map((c) => c[1])).toEqual([`${CLAUDE}\\word-prefs`, `${LIB}\\word-prefs`]);
  expect(toast).toHaveBeenCalledWith('success', 'Moved to the Recycle Bin: word-prefs');
});

test('skills kept outside the managed folders, such as cc-switch, only lose their links', async () => {
  const user = userEvent.setup();
  const SSOT = 'C:\\Users\\Demo\\cc-switch\\skills';
  vi.mocked(api.listSkills).mockImplementation(async () => [
    { kind: 'claude', root: CLAUDE, exists: true, skills: [skill(CLAUDE, 'ext', { link_target: `${SSOT}\\ext`, cc_switch: true })] },
    { kind: 'codex', root: CODEX, exists: true, skills: [] },
  ]);
  renderManager();
  await user.click(await screen.findByText('ext', { selector: 'span' }));
  expect(grid().getByText('cc-switch', { selector: 'span.text-\\[\\#6e6e73\\]' })).toBeTruthy();
  expect(screen.getAllByText(/cc-switch tracks this skill/).length).toBeGreaterThan(0);

  await user.click(screen.getByRole('switch', { name: 'Codex ext' }));
  expect(api.linkSkillToClient).toHaveBeenCalledWith([LIB], `${CLAUDE}\\ext`, 'codex');

  await user.click(screen.getByRole('button', { name: 'Delete' }));
  expect(screen.getByText(/The original stays in cc-switch\./)).toBeTruthy();
  const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
  await user.click(deleteButtons[deleteButtons.length - 1]);
  await waitFor(() => expect(api.deleteSkill).toHaveBeenCalledWith([LIB], `${CLAUDE}\\ext`));
  expect(api.deleteSkill).toHaveBeenCalledTimes(1);
  expect(toast).toHaveBeenCalledWith('success', 'Removed link: ext');
});

test('client skills link across clients, and built-in Codex skills stay read-only', async () => {
  const user = userEvent.setup();
  vi.mocked(api.listSkills).mockImplementation(async () => [
    { kind: 'claude', root: CLAUDE, exists: true, skills: [skill(CLAUDE, 'local', { shared_with: ['codex'] }), skill(CLAUDE, 'solo')] },
    {
      kind: 'codex', root: CODEX, exists: true,
      skills: [
        skill(CODEX, 'local', { link_target: `${CLAUDE}\\local`, shared_with: ['claude'] }),
        skill(`${CODEX}\\.system`, 'imagegen', { system: true }),
      ],
    },
  ]);
  renderManager();
  await user.click(await screen.findByRole('switch', { name: 'Codex solo' }));
  expect(api.linkSkillToClient).toHaveBeenCalledWith([LIB], `${CLAUDE}\\solo`, 'codex');

  expect(switchState('Codex local')).toBe('true');
  await user.click(screen.getByRole('switch', { name: 'Codex local' }));
  expect(api.setSkillEnabled).toHaveBeenCalledWith([LIB], `${CODEX}\\local`, false);

  expect(screen.queryByRole('switch', { name: 'Codex imagegen' })).toBeNull();
  expect(grid().getByText('Built-in')).toBeTruthy();
  await user.click(grid().getByText('imagegen'));
  expect(screen.queryByRole('button', { name: 'Delete' })).toBeNull();
});


test('Antigravity is a third column: built-in skills stay read-only and others link into it', async () => {
  const user = userEvent.setup();
  const AGY = 'C:\\Users\\Demo\\.gemini\\config\\skills';
  const AGY_BUILTIN = 'C:\\Users\\Demo\\.gemini\\antigravity\\builtin\\skills';
  vi.mocked(api.listSkills).mockImplementation(async () => [
    { kind: 'claude', root: CLAUDE, exists: true, skills: [skill(CLAUDE, 'skill-creator'), skill(CLAUDE, 'solo')] },
    { kind: 'codex', root: CODEX, exists: true, skills: [skill(`${CODEX}\\.system`, 'skill-creator', { system: true })] },
    { kind: 'antigravity', root: AGY, exists: true, skills: [skill(AGY_BUILTIN, 'antigravity_guide', { system: true })] },
  ]);
  renderManager();
  await user.click(await screen.findByRole('switch', { name: 'Antigravity solo' }));
  expect(api.linkSkillToClient).toHaveBeenCalledWith([LIB], `${CLAUDE}\\solo`, 'antigravity');

  // Codex's built-in skill-creator does not lock Claude's own skill of the same name.
  expect(switchState('Claude skill-creator')).toBe('true');
  expect(screen.queryByRole('switch', { name: 'Codex skill-creator' })).toBeNull();
  await user.click(screen.getByRole('switch', { name: 'Antigravity skill-creator' }));
  expect(api.linkSkillToClient).toHaveBeenLastCalledWith([LIB], `${CLAUDE}\\skill-creator`, 'antigravity');

  expect(screen.queryByRole('switch', { name: 'Antigravity antigravity_guide' })).toBeNull();
  await user.click(screen.getByRole('tab', { name: /^Antigravity/ }));
  expect(grid().getByText('antigravity_guide')).toBeTruthy();
  expect(grid().queryByText('solo')).toBeNull();
});


test('enable all turns on every listed skill that is off in that client, and follows the search', async () => {
  const user = userEvent.setup();
  renderManager();
  await screen.findByRole('switch', { name: 'Claude word-prefs' });
  await user.click(screen.getByRole('button', { name: 'Enable all in Codex' }));
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Enabled 3 skills in Codex'));
  expect(vi.mocked(api.mountSkill).mock.calls.map((c) => [c[1], c[2]])).toEqual([
    [`${LIB}\\image-gen`, 'codex'],
    [`${LIB}\\word-prefs`, 'codex'],
  ]);
  expect(api.linkSkillToClient).toHaveBeenCalledWith([LIB], `${CLAUDE_OFF}\\old-notes`, 'codex');

  vi.mocked(api.mountSkill).mockClear();
  await user.type(screen.getByPlaceholderText(/Search skill/), 'word');
  await user.click(screen.getByRole('button', { name: 'Enable all in Antigravity' }));
  await waitFor(() => expect(api.mountSkill).toHaveBeenCalledTimes(1));
  expect(api.mountSkill).toHaveBeenCalledWith([LIB], `${LIB}\\word-prefs`, 'antigravity');
});
