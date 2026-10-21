export interface ClaudeProject {
  project_id: string;
  name: string;
  root_paths: string[];
}

export interface ClaudeCandidate {
  session_id: string;
  title: string;
  preview: string;
  message_count: number;
  last_modified_time: string;
  project_path?: string;
  exists_locally: boolean;
  session_file_path: string;
  custom_title?: string;
}

export interface ClaudeRecoveryReport {
  candidates: ClaudeCandidate[];
  total_scanned: number;
  claude_home: string;
  excluded_count?: number;
  warnings: string[];
}

export interface ClaudeRecoveryResult {
  conversation_count: number;
  new_conversation_count: number;
  message_count: number;
  title_update_count: number;
  skipped_count: number;
  warnings: string[];
}

export interface ClaudePublishCandidate {
  conversation_id: string;
  session_id: string;
  title: string;
  project_path?: string;
  message_count: number;
  exists_in_target: boolean;
  updated_at: string;
}

export interface ClaudePublishPreview {
  candidates: ClaudePublishCandidate[];
  total_count: number;
  new_count: number;
  existing_count: number;
  target_claude_home: string;
  is_claude_running: boolean;
}

export interface ClaudePublishResult {
  published_count: number;
  new_count: number;
  updated_count: number;
  backup_path?: string;
  warnings: string[];
}
