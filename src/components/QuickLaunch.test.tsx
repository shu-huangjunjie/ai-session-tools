import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { I18nProvider } from '../i18n';
import { AccountLaunchModal } from './AccountLaunchModal';
import { QuickLaunchBar, QuickLaunchModal } from './QuickLaunch';
import { AccountClient, AccountView, QuickLaunchTool } from '../types';

const mockApi = vi.hoisted(() => ({
  listQuickLaunchTools: vi.fn<() => Promise<QuickLaunchTool[]>>(),
  launchQuickTool:
    vi.fn<
      (
        toolId: string,
        folderPath?: string | null,
        terminal?: string | null,
        customCommand?: string | null,
      ) => Promise<void>
    >(),
  setQuickLaunchCustomPath:
    vi.fn<(toolId: string, customPath: string | null) => Promise<QuickLaunchTool[]>>(),
  pickExecutablePath: vi.fn<(initialDir?: string) => Promise<string | null>>(),
  pickFolderPath: vi.fn<(initialDir?: string) => Promise<string | null>>(),
  switchAccount: vi.fn<(client: AccountClient, id: string, restart: boolean) => Promise<boolean>>(),
  launchAccountTarget: vi.fn<(req: unknown) => Promise<void>>(),
}));

vi.mock('../api/client', () => ({
  api: mockApi,
  errorMessage: (err: unknown) => (err instanceof Error ? err.message : String(err)),
}));

const BASE_TOOLS: QuickLaunchTool[] = [
  {
    id: 'codex_app',
    name: 'Codex',
    category: 'ai_app',
    available: true,
    running: true,
    detected_path: 'shell:AppsFolder\\OpenAI.Codex_2p2nqsd0c76g0!App',
    custom_path: null,
    supports_folder: false,
  },
  {
    id: 'codex_cli',
    name: 'Codex CLI',
    category: 'ai_cli',
    available: true,
    running: false,
    detected_path: 'C:\\Users\\demo\\AppData\\Local\\OpenAI\\Codex\\bin\\codex.exe',
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'vscode',
    name: 'VS Code',
    category: 'editor',
    available: true,
    running: false,
    detected_path: 'C:\\Users\\demo\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe',
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'cursor',
    name: 'Cursor',
    category: 'editor',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'powershell',
    name: 'PowerShell',
    category: 'terminal',
    available: true,
    running: false,
    detected_path: 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe',
    custom_path: null,
    supports_folder: true,
  },
];

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem('cv_locale', 'zh');
  vi.clearAllMocks();
  mockApi.listQuickLaunchTools.mockResolvedValue(BASE_TOOLS);
  mockApi.launchQuickTool.mockResolvedValue(undefined);
  mockApi.switchAccount.mockResolvedValue(false);
  mockApi.launchAccountTarget.mockResolvedValue(undefined);
  mockApi.pickFolderPath.mockResolvedValue('D:\\repos\\my-app');
  mockApi.pickExecutablePath.mockResolvedValue('C:\\Apps\\Cursor.exe');
  mockApi.setQuickLaunchCustomPath.mockImplementation(async (toolId, customPath) =>
    BASE_TOOLS.map((tool) =>
      tool.id === toolId
        ? {
            ...tool,
            custom_path: customPath,
            available: Boolean(customPath || tool.detected_path),
          }
        : tool,
    ),
  );
});

afterEach(() => {
  cleanup();
});

test('QuickLaunchBar renders detected tools with running badge, manages folder, launches tools, and opens modal', async () => {
  const user = userEvent.setup();
  const onToast = vi.fn();

  render(
    <I18nProvider>
      <QuickLaunchBar onToast={onToast} />
    </I18nProvider>,
  );

  const bar = await screen.findByRole('region', { name: '快速启动' });
  await within(bar).findByRole('button', { name: /Codex 运行中/ });
  expect(within(bar).getByRole('button', { name: /Codex CLI/ })).toBeTruthy();
  expect(within(bar).getByRole('button', { name: /VS Code/ })).toBeTruthy();
  expect(within(bar).queryByRole('button', { name: /^Cursor$/ })).toBeNull();
  expect(within(bar).getByText('4/5 已就绪')).toBeTruthy();

  // Pick a working folder and verify launch forwards folder only for folder-aware tools.
  await user.click(within(bar).getByRole('button', { name: '选择目录' }));
  await waitFor(() => {
    expect(within(bar).getByText('my-app')).toBeTruthy();
  });

  await user.click(within(bar).getByRole('button', { name: /Codex 运行中/ }));
  expect(mockApi.launchQuickTool).toHaveBeenCalledWith('codex_app', null);

  await user.click(within(bar).getByRole('button', { name: /Codex CLI/ }));
  expect(mockApi.launchQuickTool).toHaveBeenCalledWith('codex_cli', 'D:\\repos\\my-app');

  // Clear the working folder.
  await user.click(within(bar).getByRole('button', { name: '清除目录' }));
  expect(within(bar).getByText('选择目录')).toBeTruthy();

  // Open full modal from manage button.
  await user.click(within(bar).getByRole('button', { name: '全部 / 配置' }));
  expect(await screen.findByText('快速启动工作台')).toBeTruthy();
});

test('QuickLaunchModal shows categories, filters tools, sets custom path, and resets path override', async () => {
  const user = userEvent.setup();
  const onToast = vi.fn();

  render(
    <I18nProvider>
      <QuickLaunchModal
        isOpen={true}
        onClose={() => {}}
        contextFolder={'D:\\work\\session-repo'}
        onToast={onToast}
      />
    </I18nProvider>,
  );

  await screen.findByText('AI 桌面应用');
  expect(screen.getByText('AI 命令行')).toBeTruthy();
  expect(screen.getByText('代码编辑器')).toBeTruthy();
  expect(screen.getByText('终端工具')).toBeTruthy();

  // Quick-apply active session folder.
  await user.click(screen.getByRole('button', { name: /使用当前会话目录: session-repo/ }));
  expect(
    (screen.getByLabelText('可选工作目录（用于 CLI / 编辑器 / 终端）') as HTMLInputElement).value,
  ).toBe('D:\\work\\session-repo');

  // Installed-only filter hides unavailable Cursor.
  const installedToggle = screen.getByRole('button', { name: '仅显示已安装' });
  await user.click(installedToggle);
  expect(screen.queryByRole('button', { name: '启动 Cursor' })).toBeNull();
  await user.click(installedToggle);
  expect((screen.getByRole('button', { name: '启动 Cursor' }) as HTMLButtonElement).disabled).toBe(true);

  // Search narrows down to Cursor.
  const searchBox = screen.getByLabelText('搜索工具名称或路径...');
  await user.type(searchBox, 'cursor');
  expect(screen.queryByRole('button', { name: '启动 PowerShell' })).toBeNull();
  expect(screen.getByRole('button', { name: '启动 Cursor' })).toBeTruthy();

  // Browse custom executable path for Cursor and confirm it becomes launchable.
  await user.click(screen.getByRole('button', { name: '手动指定路径 Cursor' }));
  await waitFor(() => {
    expect(mockApi.setQuickLaunchCustomPath).toHaveBeenCalledWith('cursor', 'C:\\Apps\\Cursor.exe');
  });
  expect(await screen.findByText('C:\\Apps\\Cursor.exe')).toBeTruthy();
  expect(screen.getByText('自定义')).toBeTruthy();
  expect((screen.getByRole('button', { name: '启动 Cursor' }) as HTMLButtonElement).disabled).toBe(false);

  // Reset custom executable path back to auto-detection.
  await user.click(screen.getByRole('button', { name: '恢复自动检测 Cursor' }));
  await waitFor(() => {
    expect(mockApi.setQuickLaunchCustomPath).toHaveBeenCalledWith('cursor', null);
  });
  expect((screen.getByRole('button', { name: '启动 Cursor' }) as HTMLButtonElement).disabled).toBe(true);
});

const SAMPLE_INACTIVE_ACCOUNT: AccountView = {
  id: 'acc-2',
  kind: 'oauth',
  email: 'alt@example.com',
  name: 'Alt User',
  account_id: 'org-2',
  plan_type: 'plus',
  detail: null,
  is_current: false,
  access_expires_at: null,
  last_refresh: null,
  added_at: '2026-11-01T00:00:00Z',
  last_used_at: null,
  has_quota: true,
  quota: null,
  quota_error: null,
  quota_updated_at: null,
  verification_url: null,
  tags: [],
  note: null,
  base_url: null,
  model: null,
  auth_field: null,
  haiku_model: null,
  sonnet_model: null,
  opus_model: null,
};

test('AccountLaunchModal supports switch-only, CLI terminal/flags preview, copy command, and launch', async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  const onSwitched = vi.fn();
  const onToast = vi.fn();
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  });

  render(
    <I18nProvider>
      <AccountLaunchModal
        isOpen
        client="codex"
        account={SAMPLE_INACTIVE_ACCOUNT}
        accountLabel="alt@example.com"
        onClose={onClose}
        onSwitched={onSwitched}
        onToast={onToast}
      />
    </I18nProvider>,
  );

  expect(await screen.findByRole('heading', { name: '切换并启动' })).toBeTruthy();
  expect(screen.getByText('alt@example.com')).toBeTruthy();

  // Switch only without launching app.
  await user.click(screen.getByRole('button', { name: '仅切换账号' }));
  await waitFor(() => {
    expect(mockApi.switchAccount).toHaveBeenCalledWith('codex', 'acc-2', false);
  });
  expect(onSwitched).toHaveBeenCalledTimes(1);
  expect(onClose).toHaveBeenCalledTimes(1);

  cleanup();
  vi.clearAllMocks();
  mockApi.listQuickLaunchTools.mockResolvedValue(BASE_TOOLS);
  mockApi.switchAccount.mockResolvedValue(false);
  mockApi.launchAccountTarget.mockResolvedValue(undefined);
  mockApi.pickFolderPath.mockResolvedValue('D:\\repos\\my-app');

  render(
    <I18nProvider>
      <AccountLaunchModal
        isOpen
        client="codex"
        account={SAMPLE_INACTIVE_ACCOUNT}
        accountLabel="alt@example.com"
        onClose={onClose}
        onSwitched={onSwitched}
        onToast={onToast}
      />
    </I18nProvider>,
  );

  // Switch target to CLI, set terminal + folder + custom flags, inspect preview, and copy command.
  await user.click(await screen.findByRole('button', { name: /Codex CLI/ }));
  await user.selectOptions(screen.getByLabelText('终端类型'), 'wt');
  await user.click(screen.getByRole('button', { name: '选择目录' }));
  await user.type(screen.getByLabelText('启动命令 / 附加参数（可选）'), 'codex --yolo');

  expect(
    screen.getByText(
      "Set-Location -LiteralPath 'D:\\repos\\my-app'; & 'C:\\Users\\demo\\AppData\\Local\\OpenAI\\Codex\\bin\\codex.exe' --yolo",
    ),
  ).toBeTruthy();

  await user.click(screen.getByRole('button', { name: '切换并复制命令' }));
  await waitFor(() => {
    expect(writeText).toHaveBeenCalledWith(
      "Set-Location -LiteralPath 'D:\\repos\\my-app'; & 'C:\\Users\\demo\\AppData\\Local\\OpenAI\\Codex\\bin\\codex.exe' --yolo",
    );
  });

  // Launch CLI via launchAccountTarget with persisted CLI options.
  await user.click(screen.getByRole('button', { name: '切换并终端执行' }));
  await waitFor(() => {
    expect(mockApi.launchAccountTarget).toHaveBeenCalledWith({
      client: 'codex',
      accountId: 'acc-2',
      toolId: 'codex_cli',
      folderPath: 'D:\\repos\\my-app',
      terminal: 'wt',
      customCommand: 'codex --yolo',
    });
  });
});

test('AccountLaunchModal hides switch-only when current and persists custom editor path before launch', async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  const onSwitched = vi.fn();
  const onToast = vi.fn();

  render(
    <I18nProvider>
      <AccountLaunchModal
        isOpen
        client="codex"
        account={{ ...SAMPLE_INACTIVE_ACCOUNT, id: 'acc-1', is_current: true }}
        accountLabel="main@example.com"
        onClose={onClose}
        onSwitched={onSwitched}
        onToast={onToast}
      />
    </I18nProvider>,
  );

  expect(await screen.findByRole('heading', { name: '快速启动' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '仅切换账号' })).toBeNull();

  await user.click(screen.getByRole('button', { name: /Cursor/ }));
  const pathInput = screen.getByLabelText('程序路径（可选）') as HTMLInputElement;
  await user.type(pathInput, 'C:\\Apps\\Cursor.exe');
  await user.click(screen.getByRole('button', { name: '立即启动' }));

  await waitFor(() => {
    expect(mockApi.setQuickLaunchCustomPath).toHaveBeenCalledWith('cursor', 'C:\\Apps\\Cursor.exe');
    expect(mockApi.launchAccountTarget).toHaveBeenCalledWith({
      client: 'codex',
      accountId: 'acc-1',
      toolId: 'cursor',
      folderPath: null,
      terminal: null,
      customCommand: null,
    });
  });
  expect(onSwitched).not.toHaveBeenCalled();
  expect(onClose).toHaveBeenCalledTimes(1);
});
