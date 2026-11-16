import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { I18nProvider } from '../i18n';
import { ConversationHandoffModal } from './ConversationHandoffModal';
import { AccountClient, AccountsState, HandoffRequest, HandoffResult, QuickLaunchTool } from '../types';

const mockApi = vi.hoisted(() => ({
  listQuickLaunchTools: vi.fn<() => Promise<QuickLaunchTool[]>>(),
  listAccounts: vi.fn<(client: AccountClient) => Promise<AccountsState>>(),
  handoffConversation: vi.fn<(req: HandoffRequest) => Promise<HandoffResult>>(),
  setQuickLaunchCustomPath:
    vi.fn<(toolId: string, customPath: string | null) => Promise<QuickLaunchTool[]>>(),
  pickExecutablePath: vi.fn<(initialDir?: string) => Promise<string | null>>(),
  pickFolderPath: vi.fn<(initialDir?: string) => Promise<string | null>>(),
  launchQuickTool: vi.fn<
    (toolId: string, folderPath?: string | null, terminal?: string | null, customCommand?: string | null) => Promise<void>
  >(),
  openFolder: vi.fn<(path: string) => Promise<void>>(),
}));

vi.mock('../api/client', () => ({
  api: mockApi,
  errorMessage: (err: unknown) => (err instanceof Error ? err.message : String(err)),
}));

const BASE_TOOLS: QuickLaunchTool[] = [
  {
    id: 'codex_cli',
    name: 'Codex CLI',
    category: 'ai_cli',
    available: true,
    running: false,
    detected_path: 'C:\\Tools\\codex.exe',
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'claude_cli',
    name: 'Claude Code',
    category: 'ai_cli',
    available: true,
    running: false,
    detected_path: 'C:\\Tools\\claude.exe',
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'antigravity_cli',
    name: 'Antigravity CLI',
    category: 'ai_cli',
    available: true,
    running: false,
    detected_path: 'C:\\Tools\\agy.exe',
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'claude_app',
    name: 'Claude Desktop',
    category: 'ai_app',
    available: false,
    running: false,
    detected_path: null,
    custom_path: null,
    supports_folder: true,
  },
  {
    id: 'vscode',
    name: 'VS Code',
    category: 'editor',
    available: true,
    running: false,
    detected_path: 'C:\\Tools\\Code.exe',
    custom_path: null,
    supports_folder: true,
  },
];

const CLAUDE_ACCOUNTS: AccountsState = {
  auth_path: 'C:\\Users\\demo\\.claude.json',
  current: null,
  current_note: null,
  notes: [],
  cockpit_import_count: 0,
  cc_switch_available: false,
  accounts: [
    {
      id: 'cl-1',
      kind: 'oauth',
      email: 'work@example.com',
      name: 'Work Claude',
      account_id: null,
      plan_type: 'max',
      detail: null,
      is_current: false,
      access_expires_at: null,
      last_refresh: null,
      added_at: '2026-11-01T00:00:00Z',
      last_used_at: null,
      has_quota: true,
      quota: {
        plan_type: 'max',
        windows: [{ kind: 'five_hour', label: '5h', used_percent: 25, window_minutes: 300, resets_at: null }],
        credits_balance: null,
      },
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
    },
  ],
};

beforeEach(() => {
  localStorage.clear();
  localStorage.setItem('cv_locale', 'zh');
  vi.clearAllMocks();
  mockApi.listQuickLaunchTools.mockResolvedValue(BASE_TOOLS);
  mockApi.listAccounts.mockResolvedValue(CLAUDE_ACCOUNTS);
  mockApi.pickFolderPath.mockResolvedValue('D:\\repos\\picked-dir');
  mockApi.handoffConversation.mockImplementation(async (req) => ({
    mode: req.mode,
    targetClient: req.targetClient,
    targetSessionId: req.mode === 'native' ? 'new-uuid-777' : null,
    handoffFilePath: req.mode === 'compact' ? 'D:\\repos\\my-app\\.context-vault\\handoff-abc12345.md' : null,
    workingDir: req.folderPath ?? null,
    workspaceCreated: false,
    workspaceProjectId: 'proj-1',
    deepLinkUri: null,
    sessionTitle: '[续聊] 测试会话',
    cliCommand:
      req.mode === 'native'
        ? 'claude --resume new-uuid-777'
        : 'claude "请先阅读 .context-vault/handoff-abc12345.md 了解前序会话上下文与当前代码进度，然后继续推进其中的下一步任务。"',
    clipboardPrompt:
      req.mode === 'native'
        ? 'claude --resume new-uuid-777'
        : '请先阅读 .context-vault/handoff-abc12345.md 了解前序会话上下文与当前代码进度，然后继续推进其中的下一步任务。',
    syncedNewMessages: req.syncLatestFirst ? 2 : 0,
    totalMessages: 12,
    launched: !req.prepareOnly,
    warnings: [],
  }));
});

afterEach(() => {
  cleanup();
});

test('ConversationHandoffModal defaults to target CLI, copies Mode A resume command, and launches Mode B compact handoff', async () => {
  const user = userEvent.setup();
  const onClose = vi.fn();
  const onCompleted = vi.fn();
  const onToast = vi.fn();
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  });

  render(
    <I18nProvider>
      <ConversationHandoffModal
        isOpen
        conversation={{
          id: 'conv-1',
          title: '优化快启与跨工具接力',
          source_kind: 'codex_main',
          project_path: 'D:\\repos\\my-app',
          message_count: 10,
        }}
        onClose={onClose}
        onCompleted={onCompleted}
        onToast={onToast}
      />
    </I18nProvider>,
  );

  expect(await screen.findByRole('heading', { name: '跨工具会话接力' })).toBeTruthy();
  expect((screen.getByLabelText('工作目录（可选）') as HTMLInputElement).value).toBe('D:\\repos\\my-app');

  // Target accounts for Claude are listed with remaining quota percentage
  await waitFor(() => {
    expect(mockApi.listAccounts).toHaveBeenCalledWith('claude');
  });
  expect(await screen.findByText(/work@example\.com \(max · 剩余 75%\)/)).toBeTruthy();

  // Select target account & terminal, prepare-only Mode A command and copy
  await user.selectOptions(screen.getByLabelText('目标账号（可选切换）'), 'cl-1');
  await user.selectOptions(screen.getByLabelText('终端类型'), 'wt');
  await user.click(screen.getByRole('button', { name: '仅生成并复制指令' }));

  await waitFor(() => {
    expect(mockApi.handoffConversation).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: 'conv-1',
        targetClient: 'claude',
        targetAccountId: 'cl-1',
        mode: 'native',
        toolId: 'claude_cli',
        folderPath: 'D:\\repos\\my-app',
        terminal: 'wt',
        syncLatestFirst: true,
        prepareOnly: true,
      }),
    );
  });
  expect(writeText).toHaveBeenCalledWith('claude --resume new-uuid-777');
  expect(onCompleted).toHaveBeenCalledWith(2);

  // Switch to Mode B compact handoff, customize follow-up instructions, and launch CLI
  vi.clearAllMocks();
  mockApi.listQuickLaunchTools.mockResolvedValue(BASE_TOOLS);
  mockApi.listAccounts.mockResolvedValue(CLAUDE_ACCOUNTS);
  mockApi.handoffConversation.mockResolvedValue({
    mode: 'compact',
    targetClient: 'claude',
    targetSessionId: null,
    handoffFilePath: 'D:\\repos\\my-app\\.context-vault\\handoff-abc12345.md',
    workingDir: 'D:\\repos\\my-app',
    workspaceCreated: true,
    workspaceProjectId: 'proj-2',
    deepLinkUri: null,
    sessionTitle: '[续聊] 测试会话',
    cliCommand: 'claude --dangerously-skip-permissions "请先阅读 .context-vault/handoff-abc12345.md ..."',
    clipboardPrompt: '请先阅读 .context-vault/handoff-abc12345.md ...',
    syncedNewMessages: 0,
    totalMessages: 12,
    launched: true,
    warnings: [],
  });

  await user.click(screen.getByRole('button', { name: /模式 B：精简交接包/ }));
  await user.click(screen.getByRole('button', { name: '最近 20 条' }));
  await user.type(screen.getByLabelText('下一步任务补充（可选）'), '补充端到端验证');
  await user.type(
    screen.getByLabelText('启动命令 / 附加参数（可选）'),
    '--dangerously-skip-permissions',
  );
  await user.click(screen.getByRole('button', { name: '接力并终端启动' }));

  await waitFor(() => {
    expect(mockApi.handoffConversation).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: 'conv-1',
        targetClient: 'claude',
        mode: 'compact',
        toolId: 'claude_cli',
        extraCliArgs: '--dangerously-skip-permissions',
        recentMessageLimit: 20,
        followUpPrompt: '补充端到端验证',
        prepareOnly: false,
      }),
    );
  });
  expect(onToast).toHaveBeenCalledWith(
    'success',
    expect.stringContaining('已新建工作文件夹：D:\\repos\\my-app'),
  );
});

test('ConversationHandoffModal does not leak stale quick-launch folder when conversation has no project_path and copies boot prompt for editor', async () => {
  localStorage.setItem('cv_quick_launch_dir', 'D:\\unrelated\\other-project');
  const user = userEvent.setup();
  const onClose = vi.fn();
  const onCompleted = vi.fn();
  const onToast = vi.fn();
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  });

  mockApi.handoffConversation.mockResolvedValueOnce({
    mode: 'native',
    targetClient: 'codex',
    targetSessionId: null,
    handoffFilePath: 'C:\\Users\\demo\\ContextVault-Workspaces\\无目录对话\\.context-vault\\handoff-12345678.md',
    workingDir: 'C:\\Users\\demo\\ContextVault-Workspaces\\无目录对话',
    workspaceCreated: true,
    workspaceProjectId: 'proj-new',
    deepLinkUri: null,
    sessionTitle: '[续聊] 测试会话',
    cliCommand: 'codex resume new-1',
    clipboardPrompt: '请先阅读 .context-vault/handoff-12345678.md ...',
    syncedNewMessages: 0,
    totalMessages: 2,
    launched: true,
    warnings: [],
  });

  render(
    <I18nProvider>
      <ConversationHandoffModal
        isOpen
        conversation={{
          id: 'conv-no-proj',
          title: '无目录对话',
          source_kind: 'claude',
          project_path: null,
          message_count: 2,
        }}
        onClose={onClose}
        onCompleted={onCompleted}
        onToast={onToast}
      />
    </I18nProvider>,
  );

  expect(await screen.findByRole('heading', { name: '跨工具会话接力' })).toBeTruthy();
  expect((screen.getByLabelText('工作目录（可选）') as HTMLInputElement).value).toBe('');

  await user.click(screen.getByRole('button', { name: /VS Code/ }));
  await user.click(screen.getByRole('button', { name: '接力并启动' }));

  await waitFor(() => {
    expect(mockApi.handoffConversation).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: 'conv-no-proj',
        targetClient: 'codex',
        toolId: 'vscode',
        folderPath: null,
      }),
    );
  });
  expect(writeText).toHaveBeenCalledWith('请先阅读 .context-vault/handoff-12345678.md ...');
  expect(onToast).toHaveBeenCalledWith(
    'success',
    expect.stringContaining('已新建工作文件夹：C:\\Users\\demo\\ContextVault-Workspaces\\无目录对话'),
  );

  // After launching, the modal stays open with a clear "next steps" summary
  expect(await screen.findByText('接力完成 · 按下面的步骤继续聊')).toBeTruthy();
  expect(screen.getByText('已在 VS Code 打开工作文件夹「无目录对话」以及交接文档。')).toBeTruthy();
  expect(screen.getByText('[续聊] 测试会话')).toBeTruthy();
  expect(onClose).not.toHaveBeenCalled();
  await user.click(screen.getByRole('button', { name: '完成' }));
  expect(onClose).toHaveBeenCalled();
});

test('ConversationHandoffModal guides Antigravity desktop handoff and can switch to agy CLI', async () => {
  const user = userEvent.setup();
  const onToast = vi.fn();
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
  mockApi.listQuickLaunchTools.mockResolvedValue([
    ...BASE_TOOLS,
    {
      id: 'antigravity_app',
      name: 'Antigravity',
      category: 'ai_app',
      available: true,
      running: true,
      detected_path: 'C:\\Tools\\Antigravity.exe',
      custom_path: null,
      supports_folder: true,
    },
  ]);
  mockApi.launchQuickTool.mockResolvedValue(undefined);
  mockApi.handoffConversation.mockResolvedValueOnce({
    mode: 'native',
    targetClient: 'antigravity',
    targetSessionId: null,
    handoffFilePath: 'D:\\repos\\test\\.context-vault\\handoff-5249c905.md',
    workingDir: 'D:\\repos\\test',
    workspaceCreated: false,
    workspaceProjectId: 'ag-proj-1',
    deepLinkUri: null,
    sessionTitle: '[续聊] Respond to greeting',
    cliCommand: 'agy --project ag-proj-1 -i "请先阅读 .context-vault/handoff-5249c905.md ..."',
    clipboardPrompt: '请先阅读 .context-vault/handoff-5249c905.md ...',
    syncedNewMessages: 0,
    totalMessages: 3,
    launched: true,
    warnings: [],
  });

  render(
    <I18nProvider>
      <ConversationHandoffModal
        isOpen
        conversation={{
          id: 'conv-ag',
          title: 'Respond to greeting',
          source_kind: 'codex_main',
          project_path: 'D:\\repos\\test',
          message_count: 3,
        }}
        onClose={vi.fn()}
        onCompleted={vi.fn()}
        onToast={onToast}
      />
    </I18nProvider>,
  );

  await screen.findByRole('heading', { name: '跨工具会话接力' });
  await user.click(screen.getByRole('button', { name: /^Antigravity\b/ }));
  await user.click(await screen.findByRole('button', { name: /Antigravity\s*运行中/ }));

  // Antigravity is not force-restarted by default so open conversations are not interrupted
  const restart = screen.getByRole('checkbox', { name: /先关闭再重启/ }) as HTMLInputElement;
  expect(restart.checked).toBe(false);

  await user.click(screen.getByRole('button', { name: '接力并启动' }));

  expect(await screen.findByText(/在 Antigravity 左上角点击「\+ New Conversation」/)).toBeTruthy();
  expect(screen.getByText(/确认新会话所在的工作区是「test」/)).toBeTruthy();

  await user.click(screen.getByRole('button', { name: '改用 Antigravity CLI 直接进入' }));
  await waitFor(() => {
    expect(mockApi.launchQuickTool).toHaveBeenCalledWith(
      'antigravity_cli',
      'D:\\repos\\test',
      expect.any(String),
      'agy --project ag-proj-1 -i "请先阅读 .context-vault/handoff-5249c905.md ..."',
    );
  });
});
