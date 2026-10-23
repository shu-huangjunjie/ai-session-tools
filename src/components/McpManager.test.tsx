import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { I18nProvider } from '../i18n';
import { api } from '../api/client';
import { McpManager } from './McpManager';
import { McpClient, McpLocation, McpServerEntry, McpSpec } from '../types';

vi.mock('../api/client', () => ({
  api: {
    listMcpServers: vi.fn(),
    upsertMcpServer: vi.fn(),
    setMcpEnabled: vi.fn(),
    deleteMcpServer: vi.fn(),
    copyMcpServer: vi.fn(),
  },
  errorMessage: (e: unknown) => String(e),
}));

const server = (client: McpClient, name: string, spec: McpSpec, extra: Partial<McpServerEntry> = {}): McpServerEntry => ({
  client, name, transport: (spec.type as string) || 'stdio', spec: { type: 'stdio', ...spec }, enabled: true,
  read_only: false, cc_switch: null, raw: `"${name}": {}`, ...extra,
});

const locations = (): McpLocation[] => [
  {
    client: 'claude', config_path: 'C:\\Users\\Demo\\.claude.json', exists: true,
    servers: [
      server('claude', 'pencil', { command: 'pencil.exe', args: ['--mcp'] }, { cc_switch: true }),
      server('claude', 'remote', { type: 'http', url: 'https://example.com/mcp', headers: { Authorization: 'Bearer secret' } }, { enabled: false }),
    ],
  },
  {
    client: 'codex', config_path: 'C:\\Users\\Demo\\.codex\\config.toml', exists: true,
    servers: [
      server('codex', 'pencil', { command: 'pencil.exe', args: ['--mcp'] }),
      server('codex', 'node_repl', { command: 'node', startup_timeout_sec: 120 }, { read_only: true }),
    ],
  },
];

const toast = vi.fn();
const renderManager = () =>
  render(
    <I18nProvider>
      <McpManager isOpen onClose={() => {}} onToast={toast} />
    </I18nProvider>
  );

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  localStorage.setItem('cv_locale', 'en');
  vi.mocked(api.listMcpServers).mockImplementation(async () => locations());
  vi.mocked(api.upsertMcpServer).mockResolvedValue(undefined);
  vi.mocked(api.setMcpEnabled).mockResolvedValue(undefined);
  vi.mocked(api.deleteMcpServer).mockResolvedValue(undefined);
  vi.mocked(api.copyMcpServer).mockResolvedValue({ dropped_fields: [] });
});

afterEach(cleanup);

test('overview shows each server per client and copies a missing one across', async () => {
  const user = userEvent.setup();
  renderManager();
  const grid = await screen.findByRole('grid');
  expect(within(grid).getByText('node_repl')).toBeTruthy();
  // remote exists only in Claude: the Codex cell offers a copy.
  await user.click(within(grid).getByRole('button', { name: 'Copy from Claude' }));
  expect(api.copyMcpServer).toHaveBeenCalledWith('claude', 'remote', 'codex', false);
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Copied remote to Codex'));
});

test('details mask secrets, toggle enabled state and confirm overwrites when copying', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByRole('button', { name: 'Claude remote' }));
  expect(screen.getByText(/Authorization: ••••••/)).toBeTruthy();
  expect(screen.queryByText(/Bearer secret/)).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Show values' }));
  expect(screen.getByText(/Bearer secret/)).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Enable' }));
  expect(api.setMcpEnabled).toHaveBeenCalledWith('claude', 'remote', true);

  await user.click(screen.getByRole('button', { name: 'Claude pencil' }));
  expect(screen.getAllByText(/cc-switch tracks this server/).length).toBeGreaterThan(0);
  await user.click(screen.getByRole('button', { name: 'Copy to Codex' }));
  expect(api.copyMcpServer).not.toHaveBeenCalled();
  expect(screen.getByText(/already has a server with this name/)).toBeTruthy();
  await user.click(screen.getByRole('button', { name: 'Replace' }));
  expect(api.copyMcpServer).toHaveBeenCalledWith('claude', 'pencil', 'codex', true);
});

test('read-only Codex entries cannot be changed', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByRole('button', { name: /^Codex/ }));
  await user.click(screen.getByText('node_repl'));
  for (const name of ['Edit', 'Disable', 'Copy to Claude', 'Delete']) {
    expect((screen.getByRole('button', { name }) as HTMLButtonElement).disabled).toBe(true);
  }
});

test('creating a server writes the form spec to every chosen client', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByRole('button', { name: 'New server' }));
  const create = screen.getByRole('button', { name: 'Create' }) as HTMLButtonElement;
  await user.type(screen.getByPlaceholderText('my-server'), 'bad.name');
  expect(create.disabled).toBe(true);
  await user.clear(screen.getByPlaceholderText('my-server'));
  await user.type(screen.getByPlaceholderText('my-server'), 'fluent');
  await user.click(screen.getByRole('checkbox', { name: 'Codex' }));
  await user.click(create);
  expect(screen.getByText('Enter a command')).toBeTruthy();

  await user.type(screen.getByPlaceholderText('npx'), 'py.exe');
  const [args, env] = screen.getAllByRole('textbox').filter((el) => el.tagName === 'TEXTAREA');
  await user.type(args, '-m{enter}mcp');
  await user.type(env, 'KEY=a=b');
  await user.click(create);
  const spec = { type: 'stdio', command: 'py.exe', args: ['-m', 'mcp'], env: { KEY: 'a=b' } };
  await waitFor(() => expect(api.upsertMcpServer).toHaveBeenCalledTimes(2));
  expect(api.upsertMcpServer).toHaveBeenCalledWith('claude', null, 'fluent', spec);
  expect(api.upsertMcpServer).toHaveBeenCalledWith('codex', null, 'fluent', spec);
});

test('editing keeps fields the form does not show and round-trips through JSON', async () => {
  const user = userEvent.setup();
  vi.mocked(api.listMcpServers).mockImplementation(async () => [
    locations()[0],
    { ...locations()[1], servers: [server('codex', 'tool', { command: 'tool.exe', startup_timeout_sec: 30 })] },
  ]);
  renderManager();
  await user.click(await screen.findByRole('button', { name: /^Codex/ }));
  await user.click(screen.getByText('tool'));
  await user.click(screen.getByRole('button', { name: 'Edit' }));
  await user.click(screen.getByRole('button', { name: 'JSON' }));
  const json = screen.getAllByRole('textbox').find((el) => el.tagName === 'TEXTAREA') as HTMLTextAreaElement;
  expect(JSON.parse(json.value)).toEqual({ type: 'stdio', command: 'tool.exe', startup_timeout_sec: 30 });
  await user.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() =>
    expect(api.upsertMcpServer).toHaveBeenCalledWith('codex', 'tool', 'tool', { type: 'stdio', command: 'tool.exe', startup_timeout_sec: 30 }));
});
