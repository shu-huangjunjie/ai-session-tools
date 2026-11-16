import type { AccountClient } from './accounts';

export type HandoffMode = 'native' | 'compact';

export interface HandoffRequest {
  conversationId: string;
  targetClient: AccountClient;
  targetAccountId?: string | null;
  mode: HandoffMode;
  toolId: string;
  folderPath?: string | null;
  terminal?: string | null;
  extraCliArgs?: string | null;
  recentMessageLimit?: number | null;
  followUpPrompt?: string | null;
  syncLatestFirst: boolean;
  prepareOnly: boolean;
  restartDesktopIfRunning?: boolean | null;
}

export interface HandoffResult {
  mode: HandoffMode;
  targetClient: AccountClient;
  targetSessionId: string | null;
  handoffFilePath: string | null;
  workingDir: string | null;
  workspaceCreated: boolean;
  workspaceProjectId: string | null;
  deepLinkUri: string | null;
  sessionTitle: string;
  cliCommand: string;
  clipboardPrompt: string;
  syncedNewMessages: number;
  totalMessages: number;
  launched: boolean;
  warnings: string[];
}
