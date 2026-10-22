import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { I18nProvider } from '../i18n';
import { api } from '../api/client';
import { SkillsManager } from './SkillsManager';
import { SkillEntry, SkillLocation } from '../types';

vi.mock('../api/client', () => ({
  api: {
    listSkills: vi.fn(),
    readSkillFile: vi.fn(),
    createSkill: vi.fn(),
    setSkillEnabled: vi.fn(),
    mountSkill: vi.fn(),
    deleteSkill: vi.fn(),
    pickFolderPath: vi.fn(),
    openFolder: vi.fn(),
    openVSCode: vi.fn(),
  },
  errorMessage: (e: unknown) => String(e),
}));

const LIB = 'E:\\skills\\claude';
const CLAUDE = 'C:\\Users\\Demo\\.claude\\skills';

const skill = (dir: string, folder: string, extra: Partial<SkillEntry> = {}): SkillEntry => ({
  folder_name: folder, name: folder, description: `${folder} description`, path: `${dir}\\${folder}`,
  link_target: null, link_broken: false, has_skill_file: true, enabled: true, system: false,
  file_count: 1, total_bytes: 2048, modified_at: '2026-10-22T02:00:00Z', mounted_in: [], ...extra,
});

const locations = (): SkillLocation[] => [
  {
    kind: 'claude', root: CLAUDE, disabled_root: 'C:\\Users\\Demo\\.claude\\skills-disabled', exists: true,
    skills: [
      // Junction names may differ in case from the library path.
      skill(CLAUDE, 'word-prefs', { link_target: 'e:/skills/claude/word-prefs' }),
      skill('C:\\Users\\Demo\\.claude\\skills-disabled', 'old-notes', { enabled: false }),
    ],
  },
  { kind: 'codex', root: 'C:\\Users\\Demo\\.codex\\skills', exists: true, skills: [] },
  {
    kind: 'library', root: LIB, exists: true,
    skills: [skill(LIB, 'word-prefs', { mounted_in: ['claude'] }), skill(LIB, 'image-gen')],
  },
];

const toast = vi.fn();
const renderManager = () =>
  render(
    <I18nProvider>
      <SkillsManager isOpen onClose={() => {}} onToast={toast} />
    </I18nProvider>
  );

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  localStorage.setItem('cv_locale', 'en');
  localStorage.setItem('cv_skill_libraries', JSON.stringify([LIB]));
  vi.mocked(api.listSkills).mockImplementation(async () => locations());
  vi.mocked(api.readSkillFile).mockResolvedValue('---\nname: x\n---\nbody text');
  vi.mocked(api.mountSkill).mockResolvedValue('linked');
  vi.mocked(api.deleteSkill).mockResolvedValue('link_removed');
  vi.mocked(api.createSkill).mockResolvedValue(`${LIB}\\new-skill`);
  vi.mocked(api.setSkillEnabled).mockResolvedValue('moved');
});

afterEach(cleanup);

test('lists client skills with link and disabled state and previews SKILL.md', async () => {
  const user = userEvent.setup();
  renderManager();
  expect(await screen.findByText('word-prefs')).toBeTruthy();
  expect(api.listSkills).toHaveBeenCalledWith([LIB]);
  const list = screen.getByRole('list');
  expect(within(list).getByText('Link')).toBeTruthy();
  expect(within(list).getByText('Disabled')).toBeTruthy();

  await user.click(screen.getByText('old-notes'));
  expect(await screen.findByText(/body text/)).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Enable' }));
  expect(api.setSkillEnabled).toHaveBeenCalledWith([LIB], 'C:\\Users\\Demo\\.claude\\skills-disabled\\old-notes', true);
});

test('library skills mount and unmount through the client link', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByRole('button', { name: /Library · claude/ }));
  expect(screen.getByText('Mounted: Claude')).toBeTruthy();

  await user.click(screen.getAllByText('word-prefs')[0]);
  // Delete stays blocked while the skill is mounted.
  expect((screen.getByRole('button', { name: 'Delete' }) as HTMLButtonElement).disabled).toBe(true);
  await user.click(screen.getByRole('button', { name: 'Unmount from Claude' }));
  expect(api.deleteSkill).toHaveBeenCalledWith([LIB], `${CLAUDE}\\word-prefs`);

  await user.click(screen.getByRole('button', { name: 'Mount to Codex' }));
  expect(api.mountSkill).toHaveBeenCalledWith([LIB], `${LIB}\\word-prefs`, 'codex');
});

test('creating a skill validates the name and mounts on request', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByRole('button', { name: /Library · claude/ }));
  await user.click(screen.getByRole('button', { name: 'New skill' }));

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

test('deleting a link explains that the target folder is kept', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByText('word-prefs'));
  await user.click(screen.getByRole('button', { name: 'Delete' }));
  expect(screen.getByText(/folder it points to is kept/)).toBeTruthy();
  const deleteButtons = screen.getAllByRole('button', { name: 'Delete' });
  await user.click(deleteButtons[deleteButtons.length - 1]);
  await waitFor(() => expect(api.deleteSkill).toHaveBeenCalledWith([LIB], `${CLAUDE}\\word-prefs`));
  expect(toast).toHaveBeenCalledWith('success', 'Removed link: word-prefs');
});
