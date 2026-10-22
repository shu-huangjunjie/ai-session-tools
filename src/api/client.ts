import {
  AntigravityCandidate,
  AntigravityProject,
  AntigravityPublishPreview,
  AntigravityPublishResult,
  AntigravityRecoveryReport,
  AntigravityRecoveryResult,
  AppPathsInfo,
  CanonicalMessage,
  ClaudeCandidate,
  ClaudeProject,
  ClaudePublishPreview,
  ClaudePublishResult,
  ClaudeRecoveryReport,
  ClaudeRecoveryResult,
  CodexProject,
  CodexPublishPreview,
  CodexPublishResult,
  CodexRecoveryReport,
  CodexRecoveryResult,
  CodexThreadCandidate,
  CodexVisibility,
  Conversation,
  ConversationSummary,
  FolderPin,
  ImportPreview,
  ImportResult,
  ResetResult,
  ResetScope,
  SkillClient,
  SkillDeleteOutcome,
  SkillEntry,
  SkillLocation,
} from '../types';
import { folderPinKey } from '../utils/folderPins';

const MOCK_FOLDER_PINS: FolderPin[] = [];


// Detect if running inside Tauri runtime
const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window);

async function invokeTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) {
    const { invoke } = await import('@tauri-apps/api/core');
    return invoke<T>(cmd, args);
  }
  return mockInvoke<T>(cmd, args);
}

export const api = {
  // Conversations
  listConversations: (archived = false, limit?: number) =>
    invokeTauri<ConversationSummary[]>('list_conversations', { archived, limit }),

  getConversation: (id: string) =>
    invokeTauri<Conversation | null>('get_conversation', { id }),

  renameConversation: (id: string, title: string) =>
    invokeTauri<boolean>('rename_conversation', { id, title }),

  setArchived: (id: string, archived: boolean) =>
    invokeTauri<boolean>('set_archived', { id, archived }),

  setPinned: (id: string, pinned: boolean) =>
    invokeTauri<boolean>('set_pinned', { id, pinned }),

  listFolderPins: () => invokeTauri<FolderPin[]>('list_folder_pins'),
  setFolderPinned: (source: FolderPin['source'], path: string, pinned: boolean) =>
    invokeTauri<void>('set_folder_pinned', { source, path, pinned }),

  deleteConversation: (id: string) =>
    invokeTauri<boolean>('delete_conversation', { id }),

  deleteConversations: (ids: string[]) =>
    invokeTauri<number>('delete_conversations', { ids }),

  /** With `dryRun` only counts; otherwise backs up the database and clears the scope. */
  resetConversations: (scope: ResetScope, clearExclusions: boolean, dryRun: boolean) =>
    invokeTauri<ResetResult>('reset_conversations', { scope, clearExclusions, dryRun }),

  getMessages: (conversationId: string) =>
    invokeTauri<CanonicalMessage[]>('get_messages', { conversationId }),

  searchConversations: (query: string, limit?: number, includeArchived = false) =>
    invokeTauri<ConversationSummary[]>('search_conversations', { query, limit, includeArchived }),

  // Codex Operations
  scanCodex: () =>
    invokeTauri<CodexRecoveryReport>('scan_codex'),

  recoverCodexCandidates: (candidates: CodexThreadCandidate[]) =>
    invokeTauri<CodexRecoveryResult>('recover_codex_candidates', { candidates }),

  previewCodexPublish: (selectedThreadIds: string[]) =>
    invokeTauri<CodexPublishPreview>('preview_codex_publish', { selectedThreadIds }),

  publishToCodex: (selectedThreadIds: string[], purgeThreadIds: string[], autoRestartClient = true) =>
    invokeTauri<CodexPublishResult>('publish_to_codex', {
      selectedThreadIds,
      purgeThreadIds,
      autoRestartClient,
    }),

  getCodexProjects: () =>
    invokeTauri<CodexProject[]>('get_codex_projects'),

  /** Reads Codex's current sidebar state, so it follows changes made in Codex. */
  getCodexVisibility: () =>
    invokeTauri<CodexVisibility>('get_codex_visibility'),

  isCodexClientRunning: () =>
    invokeTauri<boolean>('is_codex_client_running'),

  stopCodexClient: () =>
    invokeTauri<boolean>('stop_codex_client'),

  startCodexClient: () =>
    invokeTauri<void>('start_codex_client'),

  // Antigravity Operations
  scanAntigravity: () =>
    invokeTauri<AntigravityRecoveryReport>('scan_antigravity'),

  recoverAntigravityCandidates: (candidates: AntigravityCandidate[]) =>
    invokeTauri<AntigravityRecoveryResult>('recover_antigravity_candidates', { candidates }),

  previewAntigravityPublish: (selectedConversationIds: string[] = []) =>
    invokeTauri<AntigravityPublishPreview>('preview_antigravity_publish', { selectedConversationIds }),

  publishToAntigravity: (selectedConversationIds: string[]) =>
    invokeTauri<AntigravityPublishResult>('publish_to_antigravity', { selectedConversationIds }),

  getAntigravityProjects: () =>
    invokeTauri<AntigravityProject[]>('get_antigravity_projects'),

  isAntigravityClientRunning: () =>
    invokeTauri<boolean>('is_antigravity_client_running'),

  stopAntigravityClient: () =>
    invokeTauri<boolean>('stop_antigravity_client'),

  startAntigravityClient: () =>
    invokeTauri<void>('start_antigravity_client'),

  // Claude Operations
  scanClaude: (customHome?: string) =>
    invokeTauri<ClaudeRecoveryReport>('scan_claude', { customHome }),

  recoverClaudeCandidates: (candidates: ClaudeCandidate[], customHome?: string) =>
    invokeTauri<ClaudeRecoveryResult>('recover_claude_candidates', { candidates, customHome }),

  previewClaudePublish: (selectedConversationIds: string[] = [], customHome?: string) =>
    invokeTauri<ClaudePublishPreview>('preview_claude_publish', { selectedConversationIds, customHome }),

  publishToClaude: (selectedConversationIds: string[], autoRestart: boolean = false, customHome?: string) =>
    invokeTauri<ClaudePublishResult>('publish_to_claude', { selectedConversationIds, autoRestart, customHome }),

  getClaudeProjects: (customHome?: string) =>
    invokeTauri<ClaudeProject[]>('get_claude_projects', { customHome }),

  isClaudeClientRunning: () =>
    invokeTauri<boolean>('is_claude_client_running'),

  stopClaudeClient: () =>
    invokeTauri<boolean>('stop_claude_client'),

  startClaudeClient: () =>
    invokeTauri<void>('start_claude_client'),

  // Importers
  previewImport: (filePath: string) =>

    invokeTauri<ImportPreview>('preview_import', { filePath }),

  executeImport: (filePath: string) =>
    invokeTauri<ImportResult>('execute_import', { filePath }),

  // System Operations
  backupDatabase: () =>
    invokeTauri<string>('backup_database'),

  exportMarkdown: (conversationId: string, targetPath: string) =>
    invokeTauri<void>('export_markdown', { conversationId, targetPath }),

  batchExportMarkdown: (conversationIds: string[], targetDir: string) =>
    invokeTauri<number>('batch_export_markdown', { conversationIds, targetDir }),

  exportJson: (conversationId: string, targetPath: string) =>
    invokeTauri<void>('export_json', { conversationId, targetPath }),

  openFolder: (path: string) =>
    invokeTauri<void>('open_folder', { path }),

  openVSCode: (path: string) =>
    invokeTauri<string>('open_vscode', { path }),

  getAppPaths: () =>
    invokeTauri<AppPathsInfo>('get_app_paths'),

  pickSavePath: (
    defaultFilename: string,
    filterName: string,
    filterExt: string,
    initialDir?: string
  ) =>
    invokeTauri<string | null>('pick_save_path', {
      defaultFilename,
      filterName,
      filterExt,
      initialDir: initialDir || null,
    }),

  pickFolderPath: (initialDir?: string) =>
    invokeTauri<string | null>('pick_folder_path', {
      initialDir: initialDir || null,
    }),

  pickOpenPath: (initialDir?: string) =>
    invokeTauri<string | null>('pick_open_path', {
      initialDir: initialDir || null,
    }),

  // Skills. `libraries` are the library folders the user registered; the backend
  // only changes skills inside them and the client skills folders.
  listSkills: (libraries: string[]) =>
    invokeTauri<SkillLocation[]>('list_skills', { libraries }),

  readSkillFile: (libraries: string[], path: string) =>
    invokeTauri<string>('read_skill_file', { libraries, path }),

  /** `target` is `claude`, `codex` or a library folder; returns the new skill folder. */
  createSkill: (
    libraries: string[],
    target: string,
    folderName: string,
    description: string,
    mountTo: SkillClient[] = []
  ) =>
    invokeTauri<string>('create_skill', { libraries, target, folderName, description, mountTo }),

  /** Moves a client skill to or from `skills-disabled`; returns its new path. */
  setSkillEnabled: (libraries: string[], path: string, enabled: boolean) =>
    invokeTauri<string>('set_skill_enabled', { libraries, path, enabled }),

  /** Links a library skill into a client skills folder with a directory junction. */
  mountSkill: (libraries: string[], path: string, client: SkillClient) =>
    invokeTauri<string>('mount_skill', { libraries, path, client }),

  deleteSkill: (libraries: string[], path: string) =>
    invokeTauri<SkillDeleteOutcome>('delete_skill', { libraries, path }),
};

/** Tauri rejects with a plain string; other failures arrive as Error objects. */
export function errorMessage(err: unknown): string {
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  return String(err);
}

// ================= Mock Implementation for Web Browser Preview =================
const MOCK_CONVERSATIONS: ConversationSummary[] = [
  {
    id: 'demo-conv-1',
    title: 'Context Restoration App 架构设计与重构讨论',
    title_user_edited: false,
    project_path: './projects/Context-Restoration',
    source_kind: 'codex_main',
    archived: false,
    message_count: 6,
    created_at: new Date(Date.now() - 3600000 * 24).toISOString(),
    updated_at: new Date(Date.now() - 3600000 * 2).toISOString(),
  },
  {
    id: 'demo-conv-2',
    title: 'SQLite FTS5 全文索引构建与便携式路径迁移方案',
    title_user_edited: true,
    project_path: './projects/Context-Restoration',
    source_kind: 'codex_main',
    archived: false,
    message_count: 4,
    created_at: new Date(Date.now() - 3600000 * 48).toISOString(),
    updated_at: new Date(Date.now() - 3600000 * 12).toISOString(),
  },
  {
    id: 'demo-conv-3',
    title: 'React + TypeScript 深色科技风 UI 布局实现',
    title_user_edited: false,
    project_path: './projects/frontend-projects',
    source_kind: 'codex_main',
    archived: false,
    message_count: 8,
    created_at: new Date(Date.now() - 3600000 * 72).toISOString(),
    updated_at: new Date(Date.now() - 3600000 * 24).toISOString(),
  },
];

const MOCK_MESSAGES: Record<string, CanonicalMessage[]> = {
  'demo-conv-1': [
    {
      id: 'm-1',
      conversation_id: 'demo-conv-1',
      sequence_no: 1,
      role: 'user',
      content: '我想用 TypeScript 和 Rust 重构 Python 做的对话恢复 App，要求先完全复刻原功能，再进行界面优化。请给出架构建议。',
      status: 'complete',
      source_type: 'codex_rollout',
      created_at: new Date(Date.now() - 3600000 * 4).toISOString(),
      updated_at: new Date(Date.now() - 3600000 * 4).toISOString(),
    },
    {
      id: 'm-2',
      conversation_id: 'demo-conv-1',
      sequence_no: 2,
      role: 'assistant',
      content: `### 重构方案：Tauri v2 + TypeScript + Rust

采用 **Tauri v2** 是最佳选择，优势如下：

1. **极小体积**：利用 Windows 系统的 Webview2 渲染引擎，打包产物通常小于 15MB（相比 PySide6 的几百兆运行时释放）。
2. **原生安全与高性能**：后端使用 Rust 的 \`rusqlite\` 内置 FTS5 全文索引，零外部 C 库依赖，性能极为强悍。
3. **便携式全相对路径**：支持绿色免安装模式，优先读取同级 \`./data\`，无缝兼容任何 Windows 电脑。

\`\`\`rust
// 示例：便携路径发现
pub fn discover(custom_root: Option<&Path>) -> Result<AppPaths, std::io::Error> {
    // 优先检测本地 ./data 便携模式
    ...
}
\`\`\`
`,
      status: 'complete',
      model: 'gpt-4o',
      source_type: 'codex_rollout',
      created_at: new Date(Date.now() - 3600000 * 3).toISOString(),
      updated_at: new Date(Date.now() - 3600000 * 3).toISOString(),
    },
    {
      id: 'm-3',
      conversation_id: 'demo-conv-1',
      sequence_no: 3,
      role: 'user',
      content: '原版支持右键 F2 重命名、Delete 删除、批量管理以及扫描 Codex，这些必须完全对齐。',
      status: 'complete',
      source_type: 'codex_rollout',
      created_at: new Date(Date.now() - 3600000 * 2).toISOString(),
      updated_at: new Date(Date.now() - 3600000 * 2).toISOString(),
    },
    {
      id: 'm-4',
      conversation_id: 'demo-conv-1',
      sequence_no: 4,
      role: 'assistant',
      content: '完全明白！所有业务逻辑已在 Rust 后端实现：\n\n- **Codex 扫描与恢复**：解析 `.codex-global-state.json` 与 `rollout-*.jsonl`，提取真实用户/助手消息。\n- **客户端发布**：自动关闭 `ChatGPT.exe`，原子写入 `state_5.sqlite` 与 `codex-dev.db`，并自动重启客户端。\n- **数据导出**：支持标准 Markdown 与 JSON 导出。',
      status: 'complete',
      model: 'gpt-4o',
      source_type: 'codex_rollout',
      created_at: new Date(Date.now() - 3600000 * 2).toISOString(),
      updated_at: new Date(Date.now() - 3600000 * 2).toISOString(),
    },
  ],
};

// Skills for the browser preview, keyed by folder path.
const MOCK_SKILL_ROOTS: Record<SkillClient, { root: string; disabled: string }> = {
  claude: { root: 'C:\\Users\\Demo\\.claude\\skills', disabled: 'C:\\Users\\Demo\\.claude\\skills-disabled' },
  codex: { root: 'C:\\Users\\Demo\\.codex\\skills', disabled: 'C:\\Users\\Demo\\.codex\\skills-disabled' },
};
const MOCK_LIBRARY = 'E:\\skills\\claude';

function mockSkill(dir: string, folder: string, description: string, extra: Partial<SkillEntry> = {}): SkillEntry {
  return {
    folder_name: folder,
    name: folder,
    description,
    path: `${dir}\\${folder}`,
    link_target: null,
    link_broken: false,
    has_skill_file: true,
    enabled: true,
    system: false,
    file_count: 1,
    total_bytes: 3501,
    modified_at: new Date(Date.now() - 3600000 * 5).toISOString(),
    mounted_in: [],
    ...extra,
  };
}

const MOCK_SKILLS: SkillEntry[] = [
  mockSkill(MOCK_LIBRARY, 'word-editing-preferences', '编辑本用户的 Word 报告时必须遵守的格式与流程偏好。'),
  mockSkill(MOCK_LIBRARY, 'codex-image-generation', '需要生成或编辑位图图片时使用，默认通过本机 Codex 生图。', { file_count: 2 }),
  mockSkill(MOCK_SKILL_ROOTS.claude.root, 'word-editing-preferences', '编辑本用户的 Word 报告时必须遵守的格式与流程偏好。', {
    link_target: `${MOCK_LIBRARY}\\word-editing-preferences`,
  }),
  mockSkill(MOCK_SKILL_ROOTS.claude.disabled, 'old-notes', '旧的笔记整理技能。', { enabled: false }),
  mockSkill(`${MOCK_SKILL_ROOTS.codex.root}\\.system`, 'imagegen', 'Generate or edit raster images.', { system: true }),
];

const parentDir = (path: string) => path.slice(0, path.lastIndexOf('\\'));

function mockSkillLocations(libraries: string[]): SkillLocation[] {
  const linked = (target: string) =>
    (['claude', 'codex'] as SkillClient[]).filter((client) =>
      MOCK_SKILLS.some(
        (s) =>
          s.link_target === target &&
          [MOCK_SKILL_ROOTS[client].root, MOCK_SKILL_ROOTS[client].disabled].includes(parentDir(s.path))
      )
    );
  const inDirs = (...dirs: string[]) => MOCK_SKILLS.filter((s) => dirs.includes(parentDir(s.path)));
  const clients = (['claude', 'codex'] as SkillClient[]).map((client) => ({
    kind: client,
    root: MOCK_SKILL_ROOTS[client].root,
    disabled_root: MOCK_SKILL_ROOTS[client].disabled,
    exists: true,
    skills: inDirs(
      MOCK_SKILL_ROOTS[client].root,
      MOCK_SKILL_ROOTS[client].disabled,
      `${MOCK_SKILL_ROOTS[client].root}\\.system`
    ).map((s) => ({ ...s })),
  }));
  return [
    ...clients,
    ...libraries.map((root) => ({
      kind: 'library' as const,
      root,
      disabled_root: null,
      exists: root === MOCK_LIBRARY,
      skills: inDirs(root).map((s) => ({ ...s, mounted_in: linked(s.path) })),
    })),
  ];
}

async function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  console.log(`[Mock API] Invoke: ${cmd}`, args);
  await new Promise((r) => setTimeout(r, 80)); // simulate brief IPC delay

  switch (cmd) {
    case 'list_skills':
      return mockSkillLocations((args?.libraries as string[]) || []) as unknown as T;
    case 'read_skill_file': {
      const skill = MOCK_SKILLS.find((s) => s.path === args?.path);
      if (!skill) throw '技能不存在';
      return `---\nname: ${skill.folder_name}\ndescription: ${skill.description}\n---\n\n# ${skill.folder_name}\n\n（浏览器预览中的示例内容）\n` as unknown as T;
    }
    case 'create_skill': {
      const target = args?.target as string;
      const dir = target === 'claude' || target === 'codex' ? MOCK_SKILL_ROOTS[target].root : target;
      const folder = args?.folderName as string;
      if (MOCK_SKILLS.some((s) => s.path === `${dir}\\${folder}`)) throw `已存在同名技能：${dir}\\${folder}`;
      const created = mockSkill(dir, folder, args?.description as string);
      MOCK_SKILLS.push(created);
      for (const client of (args?.mountTo as SkillClient[]) || []) {
        MOCK_SKILLS.push(mockSkill(MOCK_SKILL_ROOTS[client].root, folder, created.description || '', { link_target: created.path }));
      }
      return created.path as unknown as T;
    }
    case 'set_skill_enabled': {
      const skill = MOCK_SKILLS.find((s) => s.path === args?.path);
      if (!skill) throw '技能不存在';
      const client = (['claude', 'codex'] as SkillClient[]).find((c) => skill.path.includes(c === 'claude' ? '.claude' : '.codex'))!;
      const enabled = args?.enabled as boolean;
      skill.enabled = enabled;
      skill.path = `${enabled ? MOCK_SKILL_ROOTS[client].root : MOCK_SKILL_ROOTS[client].disabled}\\${skill.folder_name}`;
      return skill.path as unknown as T;
    }
    case 'mount_skill': {
      const source = MOCK_SKILLS.find((s) => s.path === args?.path);
      const client = args?.client as SkillClient;
      if (!source) throw '技能不存在';
      const link = mockSkill(MOCK_SKILL_ROOTS[client].root, source.folder_name, source.description || '', { link_target: source.path });
      if (MOCK_SKILLS.some((s) => s.path === link.path)) throw `客户端中已有同名技能：${link.path}`;
      MOCK_SKILLS.push(link);
      return link.path as unknown as T;
    }
    case 'delete_skill': {
      const index = MOCK_SKILLS.findIndex((s) => s.path === args?.path);
      if (index < 0) throw '技能不存在';
      const [removed] = MOCK_SKILLS.splice(index, 1);
      return (removed.link_target ? 'link_removed' : 'recycled') as unknown as T;
    }
    case 'list_folder_pins':
      return [...MOCK_FOLDER_PINS] as unknown as T;
    case 'set_folder_pinned': {
      const source = args?.source as FolderPin['source'];
      const path = folderPinKey(args?.path as string);
      const index = MOCK_FOLDER_PINS.findIndex((pin) => pin.source === source && pin.path === path);
      if (args?.pinned && index === -1) MOCK_FOLDER_PINS.push({ source, path, pinned_at: new Date().toISOString() });
      if (!args?.pinned && index !== -1) MOCK_FOLDER_PINS.splice(index, 1);
      return undefined as T;
    }
    case 'list_conversations':
      return MOCK_CONVERSATIONS.filter((c) => c.archived === Boolean(args?.archived)) as unknown as T;
    case 'get_conversation': {
      const id = args?.id as string;
      const found = MOCK_CONVERSATIONS.find((c) => c.id === id);
      if (!found) return null as unknown as T;
      return {
        ...found,
        system_prompt: '',
        title_user_edited: found.title_user_edited,
      } as unknown as T;
    }
    case 'get_messages': {
      const id = args?.conversationId as string;
      return (MOCK_MESSAGES[id] || []) as unknown as T;
    }
    case 'search_conversations': {
      const q = ((args?.query as string) || '').toLowerCase();
      const filtered = MOCK_CONVERSATIONS.filter((c) =>
        (Boolean(args?.includeArchived) || !c.archived) && c.title.toLowerCase().includes(q));
      return filtered as unknown as T;
    }
    case 'rename_conversation': {
      const id = args?.id as string;
      const title = args?.title as string;
      const target = MOCK_CONVERSATIONS.find((c) => c.id === id);
      if (target) {
        target.title = title;
        target.title_user_edited = true;
      }
      return true as unknown as T;
    }
    case 'set_pinned': {
      const target = MOCK_CONVERSATIONS.find((c) => c.id === args?.id);
      if (!target) return false as unknown as T;
      target.pinned_at = args?.pinned ? target.pinned_at || new Date().toISOString() : null;
      return true as unknown as T;
    }
    case 'delete_conversation': {
      const id = args?.id as string;
      const idx = MOCK_CONVERSATIONS.findIndex((c) => c.id === id);
      if (idx !== -1) MOCK_CONVERSATIONS.splice(idx, 1);
      return true as unknown as T;
    }
    case 'reset_conversations': {
      const scope = args?.scope as ResetScope;
      const inScope = (c: ConversationSummary) =>
        scope === 'all' ||
        (scope === 'codex' ? c.source_kind !== 'antigravity' && c.source_kind !== 'claude' : c.source_kind === scope);
      const matched = MOCK_CONVERSATIONS.filter(inScope);
      const result: ResetResult = {
        conversation_count: matched.length,
        message_count: matched.reduce((n, c) => n + c.message_count, 0),
        exclusion_count: 0,
        folder_pin_count: MOCK_FOLDER_PINS.filter((pin) => scope === 'all' || pin.source === scope).length,
        backup_path: args?.dryRun ? null : './backups/conversations-mock.db',
      };
      if (!args?.dryRun) {
        for (const c of matched) MOCK_CONVERSATIONS.splice(MOCK_CONVERSATIONS.indexOf(c), 1);
        for (let i = MOCK_FOLDER_PINS.length - 1; i >= 0; i--) {
          if (scope === 'all' || MOCK_FOLDER_PINS[i].source === scope) MOCK_FOLDER_PINS.splice(i, 1);
        }
      }
      return result as unknown as T;
    }
    case 'delete_conversations': {
      const ids = (args?.ids as string[]) || [];
      let count = 0;
      for (const id of ids) {
        const idx = MOCK_CONVERSATIONS.findIndex((c) => c.id === id);
        if (idx !== -1) {
          MOCK_CONVERSATIONS.splice(idx, 1);
          count++;
        }
      }
      return count as unknown as T;
    }
    case 'scan_codex':
      return {
        candidates: [
          {
            thread_id: 'c-cand-1',
            title: 'Codex 候选会话 1',
            rollout_path: './sample_codex/sessions/rollout-1.jsonl',
            project_path: './projects/Context-Restoration',
            exists_locally: false,
            model: 'gpt-4o',
            observed_providers: ['openai'],
            updated_at: new Date().toISOString(),
            archived: false,
            source_indexes: ['Codex 线程缓存'],
            codex_scope: 'project',
            codex_project_id: 'proj-1',
          },
          {
            thread_id: 'c-cand-3',
            title: 'Codex 中看不到的旧会话',
            rollout_path: './sample_codex/sessions/rollout-3.jsonl',
            project_path: 'D:\\old-workspace\\demo',
            exists_locally: false,
            model: 'gpt-4o',
            observed_providers: ['openai'],
            updated_at: new Date(Date.now() - 3600000 * 24 * 90).toISOString(),
            archived: false,
            source_indexes: ['当前 Codex'],
            codex_scope: 'hidden',
          },
          {
            thread_id: 'c-cand-2',
            title: 'Codex 候选会话 2（已存在本地）',
            rollout_path: './sample_codex/sessions/rollout-2.jsonl',
            project_path: './projects/frontend-projects',
            exists_locally: true,
            model: 'gpt-4o',
            observed_providers: ['openai'],
            updated_at: new Date().toISOString(),
            archived: false,
            source_indexes: ['当前 Codex'],
            codex_scope: 'chat',
          },
        ],
        index_count: 2,
        missing_rollouts: 0,
        excluded_count: 0,
        warnings: [],
        sidebar_projects: [
          { project_id: 'proj-1', name: 'Context restoration app', root_paths: ['./projects/Context-Restoration'] },
        ],
      } as unknown as T;
    case 'preview_codex_publish':
      return {
        active_provider: 'openai',
        eligible_thread_count: 12,
        selected_thread_count: ((args?.selectedThreadIds as string[]) || []).length,
        hidden_thread_count: 12 - ((args?.selectedThreadIds as string[]) || []).length,
        project_count: 2,
        existing_catalog_count: 8,
      } as unknown as T;
    case 'publish_to_codex':
      return {
        active_provider: 'openai',
        state_thread_count: 5,
        catalog_thread_count: 5,
        hidden_thread_count: 7,
        purged_thread_count: 0,
        removed_catalog_count: 3,
        backup_directory: './backups/mock_backup',
        quarantined_rollout_count: 0,
        quarantine_warnings: [],
      } as unknown as T;
    case 'get_codex_visibility':
      // demo-conv-2 shares the project's folder but is not assigned to it, so Codex
      // does not list it.
      return {
        available: true,
        projects: [
          { project_id: 'proj-1', name: 'Context restoration app', root_paths: ['./projects/Context-Restoration'] },
        ],
        conversations: {
          'demo-conv-1': { scope: 'project', project_id: 'proj-1' },
          'demo-conv-3': { scope: 'chat' },
        },
      } as unknown as T;
    case 'get_codex_projects':
      return [
        {
          project_id: 'proj-1',
          name: 'Context restoration app',
          root_paths: ['./projects/Context-Restoration'],
        },
        {
          project_id: 'proj-2',
          name: 'frontend-projects',
          root_paths: ['./projects/frontend-projects'],
        },
      ] as unknown as T;
    case 'scan_antigravity':
      return {
        candidates: [
          {
            conversation_id: 'ag-demo-1',
            title: 'Demo project troubleshooting',
            preview: 'Demo project troubleshooting',
            step_count: 336,
            last_modified_time: new Date(Date.now() - 3600000 * 5).toISOString(),
            project_path: 'C:\\Projects\\demo-app',
            status: 'CASCADE_RUN_STATUS_IDLE',
            exists_locally: false,
            transcript_path: 'mock/path/transcript_full.jsonl',
            has_full_transcript: true,
          },
          {
            conversation_id: 'ag-demo-2',
            title: 'Context restoration app',
            preview: 'Context restoration app',
            step_count: 804,
            last_modified_time: new Date().toISOString(),
            project_path: 'C:\\Projects',
            status: 'CASCADE_RUN_STATUS_RUNNING',
            exists_locally: true,
            transcript_path: 'mock/path/transcript_full.jsonl',
            has_full_transcript: true,
          },
        ],
        total_scanned: 2,
        home_directory: '~/.gemini/antigravity',
        warnings: [],
      } as unknown as T;
    case 'recover_antigravity_candidates':
      return {
        conversation_count: ((args?.candidates as unknown[]) || []).length,
        new_conversation_count: 1,
        message_count: 12,
        title_update_count: 0,
        skipped_count: 0,
        warnings: [],
      } as unknown as T;
    case 'preview_antigravity_publish': {
      const selected = (args?.selectedConversationIds as string[]) || [];
      const list = selected.length > 0 ? selected : ['demo-conv-1', 'demo-conv-2'];
      return {
        candidates: list.map((id, idx) => ({
          conversation_id: id,
          antigravity_id: `ag-id-${id}`,
          title: `会话 ${idx + 1}`,
          project_path: 'C:\\Projects\\ai-session-tools',
          message_count: 10,
          exists_in_target: idx === 0,
          updated_at: new Date().toISOString(),
        })),
        total_count: list.length,
        new_count: list.length > 1 ? list.length - 1 : 0,
        existing_count: list.length > 0 ? 1 : 0,
        target_db_path: 'C:\\Users\\Demo\\.gemini\\antigravity\\conversation_summaries.db',
        is_antigravity_running: false,
      } as unknown as T;
    }
    case 'publish_to_antigravity': {
      const ids = (args?.selectedConversationIds as string[]) || [];
      return {
        published_count: ids.length,
        new_count: Math.max(1, ids.length - 1),
        updated_count: 1,
        backup_path: 'C:\\Users\\Demo\\.gemini\\antigravity\\backups\\conversation_summaries_mock.db',
        warnings: ['提示：若客户端侧边栏未自动刷新，按快捷键 Ctrl+R 重载即可查看。'],
      } as unknown as T;
    }
    case 'scan_claude':
      return {
        candidates: [
          {
            session_id: 'mock-claude-session-1',
            title: 'Demo application setup',
            preview: 'Help me set up a demo application and review its project structure.',
            message_count: 68,
            last_modified_time: '2026-09-29 14:16:00',
            project_path: 'G:\\web\\demo-app',
            exists_locally: false,
            session_file_path: 'C:\\Users\\Demo\\.claude\\projects\\C--Projects-demo-app\\mock.jsonl',
            custom_title: 'Demo application setup',
          },
        ],
        total_scanned: 1,
        claude_home: '~/.claude',
        warnings: [],
      } as unknown as T;
    case 'recover_claude_candidates':
      return {
        conversation_count: ((args?.candidates as unknown[]) || []).length,
        new_conversation_count: 1,
        message_count: 68,
        title_update_count: 0,
        skipped_count: 0,
        warnings: [],
      } as unknown as T;
    case 'get_antigravity_projects':
      return [
        {
          project_id: 'ag-proj-1',
          name: 'Context restoration app',
          root_paths: ['C:\\Projects\\ai-session-tools'],
        },
      ] as unknown as T;
    case 'is_antigravity_client_running':
      return false as unknown as T;
    case 'stop_antigravity_client':
      return true as unknown as T;
    case 'start_antigravity_client':
      return undefined as unknown as T;
    case 'preview_claude_publish': {
      const selected = (args?.selectedConversationIds as string[]) || [];
      const list = selected.length > 0 ? selected : ['demo-claude-1', 'demo-claude-2'];
      return {
        candidates: list.map((id, idx) => ({
          conversation_id: id,
          session_id: `claude-sess-${id}`,
          title: `Claude 会话 ${idx + 1}`,
          project_path: 'G:\\web\\demo-app',
          message_count: 24,
          exists_in_target: idx === 0,
          updated_at: new Date().toISOString(),
        })),
        total_count: list.length,
        new_count: list.length > 1 ? list.length - 1 : 0,
        existing_count: list.length > 0 ? 1 : 0,
        target_claude_home: 'C:\\Users\\Demo\\.claude',
        is_claude_running: false,
      } as unknown as T;
    }
    case 'publish_to_claude': {
      const ids = (args?.selectedConversationIds as string[]) || [];
      return {
        published_count: ids.length,
        new_count: Math.max(1, ids.length - 1),
        updated_count: 1,
        backup_path: 'C:\\Users\\Demo\\.claude\\backups\\history_backup_mock.jsonl',
        warnings: ['提示：会话已写入 Claude 本地工程与 history.jsonl。'],
      } as unknown as T;
    }
    case 'get_claude_projects':
      return [
        {
          project_id: 'claude-proj-1',
          name: 'demo-app',
          root_paths: ['G:\\web\\demo-app'],
        },
      ] as unknown as T;
    case 'is_claude_client_running':
      return false as unknown as T;
    case 'stop_claude_client':
      return true as unknown as T;
    case 'start_claude_client':
      return undefined as unknown as T;
    case 'is_codex_client_running':

      return false as unknown as T;
    case 'backup_database':
      return './backups/conversations-manual-mock.db' as unknown as T;
    case 'pick_save_path':
      return `./${(args?.defaultFilename as string) || 'export.md'}` as unknown as T;
    case 'pick_folder_path':
      return './' as unknown as T;
    case 'pick_open_path':
      return 'C:\\Users\\Demo\\Downloads\\conversations.json' as unknown as T;
    case 'preview_import':
      return {
        source_type: 'chatgpt_export',
        source_path: args?.filePath,
        source_sha256: 'mock',
        conversation_count: 2,
        message_count: 14,
        warning_count: 0,
        warnings: [],
        sample_titles: ['示例导入会话 1', '示例导入会话 2'],
      } as unknown as T;
    case 'execute_import':
      return {
        source_type: 'chatgpt_export',
        conversation_count: 2,
        new_conversation_count: 2,
        message_count: 14,
        warning_count: 0,
        warnings: [],
      } as unknown as T;
    case 'batch_export_markdown':
      return ((args?.conversationIds as string[]) || []).length as unknown as T;
    case 'get_app_paths':
      return {
        root: './data',
        data: './data',
        database: './data/conversations.db',
        backups: './backups',
        logs: './logs',
      } as unknown as T;
    default:
      return true as unknown as T;
  }
}
