export interface AntigravityProject {
  project_id: string;
  name: string;
  root_paths: string[];
}

export interface AntigravityCandidate {
  conversation_id: string;
  title: string;
  preview: string;
  step_count: number;
  last_modified_time: string;
  project_path?: string;
  status: string;
  exists_locally: boolean;
  transcript_path: string;
  has_full_transcript: boolean;
}

export interface AntigravityRecoveryReport {
  candidates: AntigravityCandidate[];
  total_scanned: number;
  home_directory: string;
  excluded_count?: number;
  warnings: string[];
}

export interface AntigravityRecoveryResult {
  conversation_count: number;
  new_conversation_count: number;
  message_count: number;
  title_update_count: number;
  skipped_count: number;
  warnings: string[];
}

export interface AntigravityPublishCandidate {
  conversation_id: string;
  antigravity_id: string;
  title: string;
  project_path?: string;
  message_count: number;
  exists_in_target: boolean;
  updated_at: string;
}

export interface AntigravityPublishPreview {
  candidates: AntigravityPublishCandidate[];
  total_count: number;
  new_count: number;
  existing_count: number;
  target_db_path: string;
  is_antigravity_running: boolean;
}

export interface AntigravityPublishResult {
  published_count: number;
  new_count: number;
  updated_count: number;
  backup_path?: string;
  warnings: string[];
}
