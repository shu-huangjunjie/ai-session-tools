import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { App } from './App';
import { I18nProvider } from './i18n';
import { api } from './api/client';
import { MessageBubble } from './components/MessageBubble';
import { CodexRecoveryModal } from './components/CodexRecoveryModal';
import {
  CanonicalMessage,
  CodexScope,
  CodexThreadCandidate,
  Conversation,
  ConversationSummary,
  FolderPin,
} from './types';
import { folderPinKey } from './utils/folderPins';

vi.mock('./api/client', () => ({
  api: {
    listConversations: vi.fn(),
    searchConversations: vi.fn(),
    getConversation: vi.fn(),
    getMessages: vi.fn(),
    renameConversation: vi.fn(),
    setPinned: vi.fn(),
    listFolderPins: vi.fn(),
    setFolderPinned: vi.fn(),
    previewCodexPublish: vi.fn(),
    publishToCodex: vi.fn(),
    resetConversations: vi.fn(),
    scanCodex: vi.fn(),
    getCodexVisibility: vi.fn(),
    pickFolderPath: vi.fn(),
    batchExportMarkdown: vi.fn(),
  },
  errorMessage: (e: unknown) => String(e),
}));

const active: ConversationSummary = {
  id: 'active', title: 'Active fixture', title_user_edited: false, archived: false,
  source_kind: 'codex_main', project_path: 'C:\\demo', message_count: 0,
  created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
};
const archived = { ...active, id: 'archived', title: 'Archived fixture', archived: true };
const claude = { ...active, id: 'claude', title: 'Claude fixture', source_kind: 'claude' };

const asConversation = (c: ConversationSummary): Conversation => ({ ...c, system_prompt: '' });

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  localStorage.setItem('cv_locale', 'en');
  vi.mocked(api.listConversations).mockImplementation(async (isArchived = false) =>
    isArchived ? [archived] : [active, claude]);
  vi.mocked(api.searchConversations).mockImplementation(async (query, _limit, includeArchived = false) =>
    [active, ...(includeArchived ? [archived] : [])].filter((c) => c.title.toLowerCase().includes(query.toLowerCase())));
  vi.mocked(api.getConversation).mockImplementation(async (id) =>
    asConversation([active, archived, claude].find((c) => c.id === id)!));
  vi.mocked(api.getMessages).mockResolvedValue([]);
  vi.mocked(api.renameConversation).mockResolvedValue(true);
  vi.mocked(api.getCodexVisibility).mockResolvedValue({ available: false, projects: [], conversations: {} });
  vi.mocked(api.pickFolderPath).mockReset();
  vi.mocked(api.batchExportMarkdown).mockReset();
  vi.mocked(api.resetConversations).mockReset();
  vi.mocked(api.scanCodex).mockReset();
  vi.mocked(api.setPinned).mockReset();
  vi.mocked(api.listFolderPins).mockResolvedValue([]);
  vi.mocked(api.setFolderPinned).mockReset();
});
afterEach(cleanup);

const sidebar = () => within(screen.getByRole('complementary'));
const sidebarRow = (title: string) =>
  sidebar().getByText(title).closest('div[class*="cursor-pointer"]') as HTMLElement;

test('folder identities match Windows paths across casing, separators and extended prefixes', () => {
  expect(folderPinKey('C:/Work/')).toBe('c:\\work');
  expect(folderPinKey('\\\\?\\C:\\Work\\')).toBe('c:\\work');
  expect(folderPinKey('\\\\?\\UNC\\server\\share\\')).toBe('\\\\server\\share');
});

test('folders can be pinned from their menu, persist after reopening, and unpin independently of conversations', async () => {
  const other = { ...active, id: 'other', title: 'Other fixture', project_path: 'C:\\other' };
  const pinnedChat = { ...other, id: 'pinned-chat', title: 'Pinned chat', pinned_at: '2026-10-20T00:00:00Z' };
  const rows = [active, other, pinnedChat];
  let pins: FolderPin[] = [];
  vi.mocked(api.listConversations).mockResolvedValue(rows);
  vi.mocked(api.getConversation).mockImplementation(async (id) => asConversation(rows.find((c) => c.id === id)!));
  vi.mocked(api.listFolderPins).mockImplementation(async () => [...pins]);
  vi.mocked(api.setFolderPinned).mockImplementation(async (source, path, pinned) => {
    pins = pinned ? [{ source, path: folderPinKey(path), pinned_at: '2026-10-21T00:00:00Z' }] : [];
  });
  const user = userEvent.setup();
  const view = render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByRole('button', { name: 'other' });
  await user.pointer({ keys: '[MouseRight]', target: sidebar().getByRole('button', { name: 'other' }) });
  await user.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'Pin folder' }));
  await sidebar().findByText('Pinned folders');
  expect(api.setFolderPinned).toHaveBeenCalledWith('codex', other.project_path, true);
  expect(sidebar().getByRole('button', { name: 'other' }).compareDocumentPosition(sidebar().getByRole('button', { name: 'demo' })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(sidebar().getAllByText('Other fixture')).toHaveLength(1);
  expect(sidebar().getAllByText('Pinned chat')).toHaveLength(1);
  view.unmount();
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Pinned folders');
  await user.click(sidebar().getByRole('button', { name: 'Unpin folder' }));
  await waitFor(() => expect(sidebar().queryByText('Pinned folders')).toBeNull());
  expect(api.setFolderPinned).toHaveBeenLastCalledWith('codex', other.project_path, false);
  expect(sidebar().getByText('Pinned chat')).toBeTruthy();
  expect(api.setPinned).not.toHaveBeenCalled();
});

test('folder pins follow source and search filters and keep a folder whose conversations are individually pinned', async () => {
  const rows = [{ ...active, pinned_at: '2026-10-21T00:00:00Z' }, claude];
  vi.mocked(api.listConversations).mockResolvedValue(rows);
  vi.mocked(api.listFolderPins).mockResolvedValue([{ source: 'codex', path: folderPinKey(active.project_path!), pinned_at: '2026-10-21T00:00:00Z' }]);
  vi.mocked(api.searchConversations).mockImplementation(async (q) => rows.filter((c) => c.title.toLowerCase().includes(q.toLowerCase())));
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Pinned folders');
  expect(sidebar().getByRole('button', { name: 'demo' })).toBeTruthy();
  expect(sidebar().getByText('This folder’s conversations are pinned above')).toBeTruthy();
  expect(sidebar().getAllByText('Active fixture')).toHaveLength(1);
  const user = userEvent.setup();
  await user.click(screen.getByTitle('Claude'));
  expect(sidebar().queryByText('Pinned folders')).toBeNull();
  await user.click(screen.getByTitle('Codex'));
  await user.type(screen.getByRole('searchbox'), 'unmatched');
  await waitFor(() => expect(sidebar().queryByText('Pinned folders')).toBeNull());
});

test('pinning moves a conversation to the top once, survives a reload, and can be undone', async () => {
  const other = { ...active, id: 'other', title: 'Other fixture', project_path: 'C:\\other' };
  let rows: ConversationSummary[] = [active, other];
  vi.mocked(api.listConversations).mockImplementation(async () => rows);
  vi.mocked(api.getConversation).mockImplementation(async (id) => asConversation(rows.find((c) => c.id === id)!));
  vi.mocked(api.setPinned).mockImplementation(async (id, pinned) => {
    rows = rows.map((c) => c.id === id ? { ...c, pinned_at: pinned ? '2026-10-21T00:00:00Z' : null } : c);
    return true;
  });
  const user = userEvent.setup();
  const view = render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Other fixture');
  await user.pointer({ keys: '[MouseRight]', target: sidebarRow('Other fixture') });
  await user.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: 'Pin conversation' }));
  await sidebar().findByRole('button', { name: 'Pinned conversations' });
  expect(api.setPinned).toHaveBeenCalledWith('other', true);
  expect(sidebar().getAllByText('Other fixture')).toHaveLength(1);
  expect(sidebar().getByText('Other fixture').compareDocumentPosition(sidebar().getByText('Active fixture')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.getByRole('heading', { name: 'Active fixture' })).toBeTruthy();
  view.unmount();
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByRole('button', { name: 'Pinned conversations' });
  await user.click(sidebar().getByRole('button', { name: 'Unpin conversation' }));
  await waitFor(() => expect(sidebar().queryByRole('button', { name: 'Pinned conversations' })).toBeNull());
  expect(api.setPinned).toHaveBeenCalledWith('other', false);
  expect(sidebar().getByText('Other fixture')).toBeTruthy();
});

test('pinned conversations follow search and source filters and show newest pins first', async () => {
  const rows = [
    { ...active, pinned_at: '2026-10-20T00:00:00Z' },
    { ...active, id: 'new', title: 'Newest pin', pinned_at: '2026-10-21T00:00:00Z' },
    { ...claude, pinned_at: '2026-10-21T00:00:00Z' },
  ];
  vi.mocked(api.listConversations).mockResolvedValue(rows);
  vi.mocked(api.searchConversations).mockImplementation(async (q) => rows.filter((c) => c.title.toLowerCase().includes(q.toLowerCase())));
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Newest pin');
  expect(sidebar().getByText('Newest pin').compareDocumentPosition(sidebar().getByText('Active fixture')) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  const user = userEvent.setup();
  await user.type(screen.getByRole('searchbox'), 'Newest');
  await waitFor(() => expect(sidebar().queryByText('Active fixture')).toBeNull());
  expect(sidebar().getByText('Newest pin')).toBeTruthy();
  await user.clear(screen.getByRole('searchbox'));
  await sidebar().findByText('Active fixture');
  await user.click(screen.getByTitle('Claude'));
  expect(sidebar().queryByText('Newest pin')).toBeNull();
  expect(sidebar().getByText('Claude fixture')).toBeTruthy();
});

test('a failed pin leaves the original conversation in place', async () => {
  vi.mocked(api.setPinned).mockRejectedValue('Database unavailable');
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.click(sidebar().getByRole('button', { name: 'Pin conversation' }));
  await screen.findByText(/Database unavailable/);
  expect(sidebar().queryByRole('button', { name: 'Pinned conversations' })).toBeNull();
  expect(sidebar().getByText('Active fixture')).toBeTruthy();
});

test('archive checkbox loads and hides archived conversations', async () => {
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  expect(sidebar().queryByText('Archived fixture')).toBeNull();
  const user = userEvent.setup();
  const checkbox = screen.getByRole('checkbox', { name: /archived/i });
  await user.click(checkbox);
  await sidebar().findByText('Archived fixture');
  expect(api.listConversations).toHaveBeenCalledWith(true);
  await user.click(checkbox);
  await waitFor(() => expect(sidebar().queryByText('Archived fixture')).toBeNull());
  expect(sidebar().getByText('Active fixture')).toBeTruthy();
});

test('archive checkbox also controls search results', async () => {
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.type(screen.getByRole('searchbox'), 'Archived fixture');
  await waitFor(() => expect(api.searchConversations).toHaveBeenLastCalledWith('Archived fixture', undefined, false));
  // Typing is debounced into a single query.
  expect(api.searchConversations).toHaveBeenCalledTimes(1);
  expect(sidebar().queryByText('Archived fixture')).toBeNull();
  await user.click(screen.getByRole('checkbox', { name: /archived/i }));
  await sidebar().findByText('Archived fixture');
  expect(api.searchConversations).toHaveBeenLastCalledWith('Archived fixture', undefined, true);
});

test('renaming updates the open conversation header', async () => {
  render(<I18nProvider><App /></I18nProvider>);
  await screen.findByRole('heading', { name: 'Active fixture' });
  const user = userEvent.setup();
  await user.click(screen.getByTitle(/rename conversation/i));
  const input = await screen.findByDisplayValue('Active fixture');
  // Browsers submit a form with Enter by clicking its first submit button, which
  // must be Save rather than Cancel.
  const form = input.closest('form') as HTMLFormElement;
  const submitButtons = within(form)
    .getAllByRole('button')
    .filter((b) => b.getAttribute('type') === 'submit');
  expect(submitButtons.map((b) => b.textContent)).toEqual(['Save']);
  vi.mocked(api.listConversations).mockImplementation(async (isArchived = false) =>
    isArchived ? [archived] : [{ ...active, title: 'Renamed fixture' }, claude]);
  await user.clear(input);
  await user.type(input, 'Renamed fixture{Enter}');
  expect(api.renameConversation).toHaveBeenCalledWith('active', 'Renamed fixture');
  await screen.findByRole('heading', { name: 'Renamed fixture' });
});

test('switching the source tab clears the batch selection', async () => {
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.click(within(sidebarRow('Active fixture')).getByRole('checkbox'));
  await screen.findByText('1 selected');
  await user.click(screen.getByTitle('Claude'));
  await sidebar().findByText('Claude fixture');
  expect(screen.queryByText('1 selected')).toBeNull();
});

test('batch export starts in the project folder and remembers the last successful destination', async () => {
  vi.mocked(api.pickFolderPath).mockResolvedValueOnce('E:\\导出目录\\会话').mockResolvedValueOnce(null);
  vi.mocked(api.batchExportMarkdown).mockResolvedValue(1);
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.click(within(sidebarRow('Active fixture')).getByRole('checkbox'));
  await user.click(screen.getByRole('button', { name: 'Batch Save MD' }));
  await waitFor(() => expect(api.batchExportMarkdown).toHaveBeenCalledWith(['active'], 'E:\\导出目录\\会话'));
  expect(api.pickFolderPath).toHaveBeenNthCalledWith(1, active.project_path);
  await waitFor(() => expect(localStorage.getItem('cv_batch_export_directory')).toBe('E:\\导出目录\\会话'));
  await user.click(screen.getByRole('button', { name: 'Batch Save MD' }));
  await waitFor(() => expect(api.pickFolderPath).toHaveBeenNthCalledWith(2, 'E:\\导出目录\\会话'));
  expect(api.batchExportMarkdown).toHaveBeenCalledTimes(1);
});

test('a failed batch export preserves the previous successful directory', async () => {
  localStorage.setItem('cv_batch_export_directory', 'E:\\previous');
  vi.mocked(api.pickFolderPath).mockResolvedValue('E:\\unwritable');
  vi.mocked(api.batchExportMarkdown).mockRejectedValue('Access denied');
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.click(within(sidebarRow('Active fixture')).getByRole('checkbox'));
  await user.click(screen.getByRole('button', { name: 'Batch Save MD' }));
  await screen.findByText(/Access denied/);
  expect(localStorage.getItem('cv_batch_export_directory')).toBe('E:\\previous');
});

test('codex publish asks for confirmation with the real counts', async () => {
  vi.mocked(api.previewCodexPublish).mockResolvedValue({
    active_provider: 'openai', eligible_thread_count: 5, selected_thread_count: 1,
    hidden_thread_count: 4, project_count: 1, existing_catalog_count: 5,
  });
  vi.mocked(api.publishToCodex).mockResolvedValue({
    active_provider: 'openai', state_thread_count: 1, catalog_thread_count: 1, hidden_thread_count: 4,
    purged_thread_count: 0, removed_catalog_count: 4, backup_directory: 'C:\\backups\\x',
    quarantined_rollout_count: 0, quarantine_warnings: [],
  });
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.click(within(sidebarRow('Active fixture')).getByRole('checkbox'));
  await user.click(screen.getByRole('button', { name: 'Publish' }));
  const dialog = (await screen.findByText('Publish to Codex Client')).closest('div[class*="rounded-3xl"]') as HTMLElement;
  expect(within(dialog).getByText('4')).toBeTruthy();
  expect(api.publishToCodex).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole('button', { name: /back up and publish/i }));
  await waitFor(() => expect(api.publishToCodex).toHaveBeenCalledWith(['active'], [], true));
});

test('codex tab lists what Codex lists and folds away the rest', async () => {
  const listed = { ...active, id: 'listed', title: 'Listed fixture' };
  const chat = { ...active, id: 'chat', title: 'Chat fixture', project_path: 'C:\\other' };
  // In the project's folder, but Codex lists only threads assigned to the project.
  const unlisted = { ...active, id: 'unlisted', title: 'Unlisted fixture' };
  vi.mocked(api.listConversations).mockImplementation(async (isArchived = false) =>
    isArchived ? [] : [listed, chat, unlisted]);
  vi.mocked(api.getConversation).mockImplementation(async (id) =>
    asConversation([listed, chat, unlisted].find((c) => c.id === id)!));
  vi.mocked(api.getCodexVisibility).mockResolvedValue({
    available: true,
    projects: [{ project_id: 'p1', name: 'Demo project', root_paths: ['C:\\demo'] }],
    conversations: { listed: { scope: 'project', project_id: 'p1' }, chat: { scope: 'chat' } },
  });
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Demo project');
  expect(sidebar().getByText('Listed fixture')).toBeTruthy();
  expect(sidebar().getByText('Chat fixture')).toBeTruthy();
  expect(sidebar().queryByText('Unlisted fixture')).toBeNull();
  expect(screen.getByText('2/3')).toBeTruthy();
  const user = userEvent.setup();
  await user.click(sidebar().getByRole('button', { name: /not listed in codex/i }));
  await sidebar().findByText('Unlisted fixture');
  // The section stays open after a restart.
  expect(localStorage.getItem('cv_codex_hidden_expanded')).toBe('1');
});

test('codex recovery preselects only new threads that Codex lists', async () => {
  const candidate = (thread_id: string, codex_scope: CodexScope, exists_locally = false): CodexThreadCandidate => ({
    thread_id, title: `Thread ${thread_id}`, rollout_path: '', project_path: 'C:\\demo', exists_locally,
    observed_providers: [], updated_at: '2026-01-01T00:00:00Z', archived: false, source_indexes: [],
    codex_scope, codex_project_id: codex_scope === 'project' ? 'p1' : null,
  });
  const onConfirm = vi.fn().mockResolvedValue(undefined);
  render(
    <I18nProvider>
      <CodexRecoveryModal
        isOpen
        onClose={() => {}}
        isRecovering={false}
        onConfirmRecovery={onConfirm}
        report={{
          candidates: [candidate('new', 'project'), candidate('old', 'project', true), candidate('gone', 'hidden')],
          index_count: 1, missing_rollouts: 0, excluded_count: 0, warnings: [],
          sidebar_projects: [{ project_id: 'p1', name: 'Demo project', root_paths: ['C:\\demo'] }],
        }}
      />
    </I18nProvider>
  );
  expect(screen.getByText('Demo project')).toBeTruthy();
  expect(screen.queryByText('Thread gone')).toBeNull();
  const user = userEvent.setup();
  await user.click(screen.getByRole('button', { name: /not listed in codex/i }));
  expect(screen.getByText('Thread gone')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Recover Selected (1)' }));
  expect(onConfirm).toHaveBeenCalledWith([expect.objectContaining({ thread_id: 'new' })]);
});

test('thinking blocks render as collapsible sections', () => {
  const message: CanonicalMessage = {
    id: 'm', conversation_id: 'c', sequence_no: 1, role: 'assistant', status: 'complete',
    source_type: 'claude_code', created_at: '2026-01-01T00:00:00Z', updated_at: '2026-01-01T00:00:00Z',
    content: '<details>\n<summary>💭 <b>深度思考过程</b></summary>\n\nHidden reasoning\n</details>\n\n## Answer\n\n```rust\n    let x = 1;\n```',
  };
  const { container } = render(<I18nProvider><MessageBubble message={message} /></I18nProvider>);
  expect(container.textContent).not.toContain('<details>');
  const details = container.querySelector('details');
  expect(details?.textContent).toContain('深度思考过程');
  expect(details?.textContent).toContain('Hidden reasoning');
  expect(screen.getByText('Answer')).toBeTruthy();
  // Code keeps its indentation.
  expect(container.querySelector('pre code')?.textContent).toBe('    let x = 1;');
});

test('the header replaces management with database clearing and previews before confirmation', async () => {
  vi.mocked(api.resetConversations).mockImplementation(async (scope, _clear, dryRun) => ({
    conversation_count: scope === 'all' ? 2 : 1,
    message_count: 10,
    exclusion_count: scope === 'all' ? 3 : 0,
    backup_path: dryRun ? null : 'C:\backups\conversations-x.db',
  }));
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  expect(screen.queryByRole('button', { name: /manager/i })).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Clear Database' }));
  await screen.findByText(/All sources: 2 conversations, 10 messages/);
  expect(screen.getByText(/3 deletion records/)).toBeTruthy();
  expect(api.resetConversations).toHaveBeenCalledWith('all', true, true);
  expect(api.resetConversations).not.toHaveBeenCalledWith(expect.anything(), expect.anything(), false);
  await user.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByText('Confirm Clear Database')).toBeNull();
  expect(sidebar().getByText('Active fixture')).toBeTruthy();
  expect(api.resetConversations).not.toHaveBeenCalledWith(expect.anything(), expect.anything(), false);
});

test('confirmed clearing removes all sources and search state without starting a scan', async () => {
  vi.mocked(api.resetConversations).mockImplementation(async (_scope, _clear, dryRun) => {
    if (!dryRun) {
      vi.mocked(api.listConversations).mockResolvedValue([]);
      vi.mocked(api.searchConversations).mockResolvedValue([]);
    }
    return { conversation_count: 3, message_count: 10, exclusion_count: 1,
      backup_path: dryRun ? null : 'C:\\backups\\clear-test.db' };
  });
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.click(within(sidebarRow('Active fixture')).getByRole('checkbox'));
  await user.type(screen.getByRole('searchbox'), 'Active');
  await waitFor(() => expect(api.searchConversations).toHaveBeenCalled());
  await user.click(screen.getByRole('button', { name: 'Clear Database' }));
  await screen.findByText('Confirm Clear Database');
  await user.click(screen.getByRole('button', { name: /back up and clear/i }));
  await waitFor(() => expect(api.resetConversations).toHaveBeenCalledWith('all', true, false));
  await screen.findByText(/clear-test.db/);
  expect(sidebar().queryByText('Active fixture')).toBeNull();
  expect(screen.queryByRole('heading', { name: 'Active fixture' })).toBeNull();
  expect((screen.getByRole('searchbox') as HTMLInputElement).value).toBe('');
  expect(screen.queryByText('1 selected')).toBeNull();
  expect(api.scanCodex).not.toHaveBeenCalled();
  await user.click(screen.getByTitle('Claude'));
  expect(sidebar().queryByText('Claude fixture')).toBeNull();
});

test('a failed clear keeps the confirmation and existing conversations', async () => {
  vi.mocked(api.resetConversations).mockImplementation(async (_scope, _clear, dryRun) => {
    if (!dryRun) throw 'Backup failed';
    return { conversation_count: 2, message_count: 10, exclusion_count: 0 };
  });
  render(<I18nProvider><App /></I18nProvider>);
  await sidebar().findByText('Active fixture');
  const user = userEvent.setup();
  await user.click(screen.getByRole('button', { name: 'Clear Database' }));
  await screen.findByText('Confirm Clear Database');
  await user.click(screen.getByRole('button', { name: /back up and clear/i }));
  await screen.findByText(/Backup failed/);
  expect(screen.getByText('Confirm Clear Database')).toBeTruthy();
  expect(sidebar().getByText('Active fixture')).toBeTruthy();
  expect(api.scanCodex).not.toHaveBeenCalled();
});
