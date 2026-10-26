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
  {
    client: 'antigravity', config_path: 'C:\\Users\\Demo\\.gemini\\config\\mcp_config.json', exists: true,
    servers: [
      server('antigravity', 'pencil', { command: 'pencil.exe', args: ['--mcp'] }),
      server('antigravity', 'remote', { type: 'http', url: 'https://example.com/mcp' }, { enabled: false }),
    ],
  },
];

const toast = vi.fn();
const renderManager = () =>
  render(
    <I18nProvider>
      <McpManager onToast={toast} />
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

const grid = () => within(screen.getByRole('grid'));
const switchOf = (name: string) => screen.getByRole('switch', { name }) as HTMLButtonElement;
const clickLastDelete = async (user: ReturnType<typeof userEvent.setup>) => {
  const buttons = screen.getAllByRole('button', { name: 'Delete' });
  await user.click(buttons[buttons.length - 1]);
};

test('every server is one row with a switch per client, and switching on adds it without any copy step', async () => {
  const user = userEvent.setup();
  renderManager();
  await screen.findByRole('switch', { name: 'Codex remote' });
  expect(grid().getAllByText('pencil')).toHaveLength(1);
  expect(screen.queryByRole('button', { name: /Copy/ })).toBeNull();
  expect(switchOf('Codex remote').getAttribute('aria-checked')).toBe('false');

  await user.click(switchOf('Codex remote'));
  expect(api.copyMcpServer).toHaveBeenCalledWith('claude', 'remote', 'codex', false);
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Enabled remote in Codex'));

  // node_repl belongs to the Codex desktop app: it is locked there and cannot be added elsewhere.
  expect(switchOf('Codex node_repl').disabled).toBe(true);
  expect(switchOf('Codex node_repl').getAttribute('aria-checked')).toBe('true');
  expect(switchOf('Claude node_repl').disabled).toBe(true);
});

test('switches enable and disable existing entries, and the filter narrows the list', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByRole('switch', { name: 'Claude pencil' }));
  expect(api.setMcpEnabled).toHaveBeenCalledWith('claude', 'pencil', false);
  await waitFor(() => expect(toast).toHaveBeenCalledWith('success', 'Disabled pencil in Claude'));
  await user.click(switchOf('Antigravity remote'));
  expect(api.setMcpEnabled).toHaveBeenLastCalledWith('antigravity', 'remote', true);

  await user.click(screen.getByRole('tab', { name: /^Not enabled/ }));
  expect(grid().getByText('remote')).toBeTruthy();
  expect(grid().queryByText('pencil')).toBeNull();
});

test('details mask secrets, show each client and note when the clients differ', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByText('remote', { selector: 'div' }));
  expect(screen.getByText(/Authorization: ••••••/)).toBeTruthy();
  expect(screen.queryByText(/Bearer secret/)).toBeNull();
  await user.click(screen.getByRole('button', { name: 'Show values' }));
  expect(screen.getByText(/Bearer secret/)).toBeTruthy();
  expect(screen.getByText('Not added')).toBeTruthy();
  expect(screen.getByText(/configure this server differently; this shows Claude's/)).toBeTruthy();

  await user.click(grid().getByText('pencil'));
  expect(screen.getAllByText(/cc-switch tracks this server/).length).toBeGreaterThan(0);
});

test('editing saves to every client that has the server, and deleting removes it from all of them', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByText('pencil', { selector: 'div' }));
  await user.click(screen.getByRole('button', { name: 'Edit' }));
  await user.click(screen.getByRole('button', { name: 'Save' }));
  const spec = { type: 'stdio', command: 'pencil.exe', args: ['--mcp'] };
  await waitFor(() => expect(api.upsertMcpServer).toHaveBeenCalledTimes(3));
  for (const client of ['claude', 'codex', 'antigravity']) {
    expect(api.upsertMcpServer).toHaveBeenCalledWith(client, 'pencil', 'pencil', spec);
  }

  await user.click(screen.getByRole('button', { name: 'Delete' }));
  expect(screen.getByText(/from the config of Claude、Codex、Antigravity/)).toBeTruthy();
  await clickLastDelete(user);
  await waitFor(() => expect(api.deleteMcpServer).toHaveBeenCalledTimes(3));
  expect(vi.mocked(api.deleteMcpServer).mock.calls.map((c) => c[0])).toEqual(['claude', 'codex', 'antigravity']);
});

test('read-only Codex entries cannot be edited or deleted', async () => {
  const user = userEvent.setup();
  renderManager();
  await user.click(await screen.findByText('node_repl', { selector: 'div' }));
  for (const name of ['Edit', 'Delete']) {
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
  await user.click(await screen.findByText('tool', { selector: 'div' }));
  await user.click(screen.getByRole('button', { name: 'Edit' }));
  await user.click(screen.getByRole('button', { name: 'JSON' }));
  const json = screen.getAllByRole('textbox').find((el) => el.tagName === 'TEXTAREA') as HTMLTextAreaElement;
  expect(JSON.parse(json.value)).toEqual({ type: 'stdio', command: 'tool.exe', startup_timeout_sec: 30 });
  await user.click(screen.getByRole('button', { name: 'Save' }));
  await waitFor(() =>
    expect(api.upsertMcpServer).toHaveBeenCalledWith('codex', 'tool', 'tool', { type: 'stdio', command: 'tool.exe', startup_timeout_sec: 30 }));
  expect(api.upsertMcpServer).toHaveBeenCalledTimes(1);
});

test('SSE servers cannot be switched on in Antigravity, which only speaks stdio and HTTP', async () => {
  const user = userEvent.setup();
  vi.mocked(api.listMcpServers).mockImplementation(async () => [
    { ...locations()[0], servers: [server('claude', 'stream', { type: 'sse', url: 'https://example.com/sse' })] },
    { ...locations()[1], servers: [] },
    { ...locations()[2], servers: [] },
  ]);
  renderManager();
  await screen.findByRole('switch', { name: 'Antigravity stream' });
  expect(switchOf('Antigravity stream').disabled).toBe(true);
  expect(switchOf('Antigravity stream').title).toMatch(/not SSE/);
  await user.click(switchOf('Codex stream'));
  expect(api.copyMcpServer).toHaveBeenCalledWith('claude', 'stream', 'codex', false);
});


test('enable all turns on every listed server that is off in that client', async () => {
  const user = userEvent.setup();
  vi.mocked(api.copyMcpServer).mockResolvedValue({ dropped_fields: ['headers'] });
  renderManager();
  await screen.findByRole('switch', { name: 'Codex remote' });
  await user.click(screen.getByRole('button', { name: 'Enable all in Antigravity' }));
  await waitFor(() => expect(api.setMcpEnabled).toHaveBeenCalledWith('antigravity', 'remote', true));
  expect(api.setMcpEnabled).toHaveBeenCalledTimes(1);

  await user.click(screen.getByRole('button', { name: 'Enable all in Codex' }));
  await waitFor(() => expect(api.copyMcpServer).toHaveBeenCalledWith('claude', 'remote', 'codex', false));
  expect(toast).toHaveBeenCalledWith('success', 'Enabled 1 servers in Codex');
  expect(toast).toHaveBeenCalledWith('warning', expect.stringContaining('remote'));
  // Claude still has remote switched off; read-only node_repl does not count.
  expect(screen.getByRole('button', { name: 'Enable all in Claude' }).getAttribute('title')).toMatch(/the 1 listed servers/);
});
