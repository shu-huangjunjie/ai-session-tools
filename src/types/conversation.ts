export type MessageRole = 'system' | 'developer' | 'user' | 'assistant';

/** Conversations removed by a vault reset; matches the source tabs. */
export type ResetScope = 'all' | 'codex' | 'antigravity' | 'claude';

export interface ResetResult {
  conversation_count: number;
  message_count: number;
  exclusion_count: number;
  folder_pin_count?: number;
  backup_path?: string | null;
}

export interface FolderPin {
  source: 'codex' | 'antigravity' | 'claude';
  path: string;
  pinned_at: string;
}
export type MessageStatus = 'complete' | 'streaming' | 'interrupted' | 'failed' | 'cancelled';

export interface Conversation {
  id: string;
  title: string;
  source_title?: string;
  title_user_edited: boolean;
  system_prompt: string;
  active_provider_id?: string;
  active_model?: string;
  active_leaf_message_id?: string;
  project_path?: string;
  source_kind: string;
  archived: boolean;
  pinned_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface ConversationSummary {
  id: string;
  title: string;
  source_title?: string;
  title_user_edited: boolean;
  project_path?: string;
  source_kind: string;
  archived: boolean;
  pinned_at?: string | null;
  message_count: number;
  created_at: string;
  updated_at: string;
}

export interface CanonicalMessage {
  id: string;
  conversation_id: string;
  parent_message_id?: string;
  sequence_no: number;
  role: MessageRole;
  content: string;
  status: MessageStatus;
  provider_id?: string;
  model?: string;
  remote_response_id?: string;
  source_type: string;
  source_external_id?: string;
  created_at: string;
  updated_at: string;
}
