export type QuickLaunchCategory = 'ai_app' | 'ai_cli' | 'editor' | 'terminal';

export type QuickLaunchTerminal =
  | 'system'
  | 'wt'
  | 'powershell'
  | 'pwsh'
  | 'cmd'
  | 'git_bash';

export interface QuickLaunchTool {
  id: string;
  name: string;
  category: QuickLaunchCategory;
  available: boolean;
  running: boolean;
  detected_path: string | null;
  custom_path: string | null;
  supports_folder: boolean;
}

export interface AccountLaunchRequest {
  client: 'codex' | 'claude' | 'antigravity';
  accountId?: string | null;
  toolId: string;
  folderPath?: string | null;
  terminal?: QuickLaunchTerminal | null;
  customCommand?: string | null;
}
